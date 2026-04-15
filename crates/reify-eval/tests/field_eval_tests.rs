//! Field declaration evaluation tests.
//!
//! Tests for evaluating `field def` declarations into Value::Field values
//! and applying field operations (sample, gradient, etc.).

use reify_expr::{EvalContext, eval_expr};
use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{
    CompiledExpr, CompiledExprKind, ContentHash, FieldSourceKind, ResolvedFunction, Type, ValueMap,
    FIELD_ENTITY_PREFIX, ModulePath, Severity, Value, ValueCellId,
};

/// Helper: parse, compile, and eval source, return eval result.
fn eval_source(source: &str) -> reify_eval::EvalResult {
    let parsed = reify_syntax::parse(source, ModulePath::single("field_eval_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

// ── Step 21: eval analytical field at point ────────────────────────────

#[test]
fn eval_analytical_field_at_point() {
    let result = eval_source("field def temp : Point3 -> Scalar { source = analytical { |p| p } }");

    // The field should be stored in the values map
    let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "temp");
    let field_val = result
        .values
        .get(&field_id)
        .unwrap_or_else(|| panic!("field 'temp' not found in eval result values"));

    // Should be a Value::Field with correct types
    match field_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } => {
            // Domain should be Point3 (StructureRef)
            assert_eq!(format!("{}", domain_type), "Point3");
            // Codomain should be Scalar[m] (length-dimensioned)
            assert_eq!(format!("{}", codomain_type), "Scalar[m]");
            // Source should be Analytical
            assert!(
                matches!(source, reify_types::FieldSourceKind::Analytical),
                "expected Analytical source, got: {:?}",
                source
            );
            // Lambda should be a Lambda value (not Undef)
            assert!(
                matches!(**lambda, Value::Lambda { .. }),
                "expected Lambda value in analytical field, got: {:?}",
                lambda
            );
        }
        other => panic!("expected Value::Field, got: {:?}", other),
    }
}

// ── Step 23: eval sample(field, point) ─────────────────────────────

#[test]
fn eval_sample_field_point() {
    // Define a field and a structure that uses sample() to query it at a point.
    // The analytical field is `|p| p` (identity), so sample(field, 42) should return 42.
    let result = eval_source(
        r#"
field def identity_field : Scalar -> Scalar { source = analytical { |p| p } }

structure S {
    let val = sample(identity_field, 42)
}
"#,
    );

    let val_id = ValueCellId::new("S", "val");
    let val = result
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'val' not found in eval result"));

    // sample(identity_field, 42) should evaluate the lambda |p| p with p=42, returning 42
    match val {
        Value::Int(n) => assert_eq!(*n, 42, "expected 42, got {}", n),
        Value::Real(v) => assert!((v - 42.0).abs() < 1e-12, "expected 42.0, got {}", v),
        other => panic!("expected Int(42) or Real(42.0), got: {:?}", other),
    }
}

// ── Step 27: FIELD_ENTITY_PREFIX constant ──────────────────────────────

#[test]
fn field_entity_prefix_constant() {
    // Verify the constant exists and has the expected value
    assert_eq!(FIELD_ENTITY_PREFIX, "__field");

    // Verify it can be used to construct a ValueCellId matching the field pattern
    let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "temp");
    assert_eq!(field_id.entity, "__field");
    assert_eq!(field_id.member, "temp");
    assert_eq!(format!("{}", field_id), "__field.temp");
}

// ── Step 31: eval field snapshot consistency ─────────────────────────────

#[test]
fn eval_field_snapshot_consistency() {
    // Evaluate a module with a field and verify the field value appears
    // in snapshot.values (not just the cold values map).
    // This ensures incremental re-evaluation via edit_param/warm-starting
    // can see field values.
    let source = "field def temp : Point3 -> Scalar { source = analytical { |p| p } }";
    let parsed = reify_syntax::parse(source, ModulePath::single("field_snapshot_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let _result = engine.eval(&compiled);

    // The field should be in the snapshot values
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "temp");

    let snapshot_entry = snapshot.values.get(&field_id);
    assert!(
        snapshot_entry.is_some(),
        "field 'temp' not found in snapshot.values — field values must be inserted \
         into the snapshot for incremental re-evaluation to work"
    );

    let (val, det) = snapshot_entry.unwrap();
    // Should be a Value::Field
    assert!(
        matches!(val, Value::Field { .. }),
        "expected Value::Field in snapshot, got: {:?}",
        val
    );
    // Should be Determined
    assert_eq!(
        *det,
        reify_types::DeterminacyState::Determined,
        "field snapshot value should be Determined"
    );
}

// ── Analysis sampling dispatch tests (eval-level) ────────────────────────────
//
// These tests exercise the full sampling dispatch path:
//   sample(analysis_op(tensor_field), point)
//   → FieldSourceKind match in lib.rs:126-254
//   → sample_*_at_point → inner lambda eval → stdlib analysis builtin
//
// Unlike field_analysis_tests.rs in reify-expr (which uses Pressure-dimensioned
// Scalars), these use dimensionless Real tensor elements to focus on dispatch
// correctness without unit concerns.
//
// The tensor field is constructed programmatically because the .ri type system
// cannot express tensor codomain types in field definitions.

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a dimensionless 3×3 tensor from row data (Value::Real elements).
fn make_stress_tensor(rows: &[&[f64]]) -> Value {
    Value::Tensor(
        rows.iter()
            .map(|row| Value::Tensor(row.iter().map(|&v| Value::Real(v)).collect()))
            .collect(),
    )
}

/// Type: Matrix3x3<Real> (dimensionless).
fn real_matrix_type() -> Type {
    Type::Matrix {
        m: 3,
        n: 3,
        quantity: Box::new(Type::Real),
    }
}

/// Build an analytical field `Real → Matrix3x3(Real)` with a constant-tensor lambda.
///
/// The lambda takes a single parameter `p` and ignores it, always returning
/// `tensor`. This satisfies `validate_tensor_field` (Analytical source +
/// callable Lambda + 3×3 matrix codomain).
fn make_constant_tensor_field(tensor: Value) -> (Value, Type) {
    let p_id = ValueCellId::new("$lambda0", "p");
    let body = CompiledExpr::literal(tensor, real_matrix_type());
    let lambda = Value::Lambda {
        params: vec![("p".to_string(), p_id)],
        body: Box::new(body),
        captures: ValueMap::new(),
    };
    let domain = Type::Real;
    let codomain = real_matrix_type();
    let field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Box::new(lambda),
    };
    let field_type = Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(codomain),
    };
    (field, field_type)
}

/// Build a CompiledExpr::FunctionCall for a stdlib function.
fn make_function_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
    let hash = ContentHash::of(name.as_bytes());
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::{}", name),
            },
            args,
        },
        result_type,
        content_hash: hash,
    }
}

// ── step-1: von_mises dispatch ────────────────────────────────────────────────

#[test]
fn eval_sample_von_mises_field_dispatch() {
    // Uniaxial stress [[σ,0,0],[0,0,0],[0,0,0]]: von Mises = σ (dimensionless)
    let sigma = 100.0_f64;
    let tensor = make_stress_tensor(
        &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
    );
    let (field, field_type) = make_constant_tensor_field(tensor);

    // Build nested expr: sample(von_mises(field_literal), 0.5)
    // von_mises(Field) intercepts at lib.rs:277-282, wraps with VonMises source.
    // sample(VonMisesField, point) dispatches via FieldSourceKind::VonMises at lib.rs:201-212.
    let vm_field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::Real),
    };
    let vm_expr = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        vm_field_type.clone(),
    );
    let sample_expr = make_function_call(
        "sample",
        vec![
            vm_expr,
            CompiledExpr::literal(Value::Real(0.5), Type::Real),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // von Mises of uniaxial stress = σ
    match &result {
        Value::Real(v) => {
            assert!(
                (v - sigma).abs() < 1e-10,
                "expected von Mises ≈ {sigma}, got {v}"
            );
        }
        _ => panic!(
            "sample(von_mises(field), point) should return Real({sigma}), got {:?}",
            result
        ),
    }
}

// ── step-3: principal_stresses dispatch ───────────────────────────────────────

#[test]
fn eval_sample_principal_stresses_field_dispatch() {
    // Uniaxial [[100,0,0],[0,0,0],[0,0,0]]: eigenvalues [0.0, 0.0, 100.0] sorted ascending
    let sigma = 100.0_f64;
    let tensor = make_stress_tensor(
        &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
    );
    let (field, field_type) = make_constant_tensor_field(tensor);

    // Build nested expr: sample(principal_stresses(field_literal), 0.5)
    let ps_field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::List(Box::new(Type::Real))),
    };
    let ps_expr = make_function_call(
        "principal_stresses",
        vec![CompiledExpr::literal(field, field_type)],
        ps_field_type.clone(),
    );
    let sample_expr = make_function_call(
        "sample",
        vec![
            ps_expr,
            CompiledExpr::literal(Value::Real(0.5), Type::Real),
        ],
        Type::List(Box::new(Type::Real)),
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Uniaxial stress eigenvalues: 0, 0, σ → sorted ascending [0.0, 0.0, 100.0]
    let Value::List(items) = &result else {
        panic!(
            "sample(principal_stresses(field), pt) should return List, got {:?}",
            result
        );
    };
    assert_eq!(items.len(), 3, "should have 3 principal stresses");
    let expected = [0.0_f64, 0.0, sigma];
    for (i, (item, &exp)) in items.iter().zip(expected.iter()).enumerate() {
        match item {
            Value::Real(v) => {
                assert!(
                    (v - exp).abs() < 1e-10,
                    "principal stress[{i}]: expected {exp}, got {v}"
                );
            }
            _ => panic!("principal stress[{i}] should be Real, got {:?}", item),
        }
    }
}

// ── step-4: max_shear dispatch ────────────────────────────────────────────────

#[test]
fn eval_sample_max_shear_field_dispatch() {
    // Pure shear [[0,τ,0],[τ,0,0],[0,0,0]]: eigenvalues [-τ, 0, τ]
    // max_shear = (τ - (-τ)) / 2 = τ
    let tau = 50.0_f64;
    let tensor = make_stress_tensor(
        &[&[0.0, tau, 0.0], &[tau, 0.0, 0.0], &[0.0, 0.0, 0.0]],
    );
    let (field, field_type) = make_constant_tensor_field(tensor);

    // Build nested expr: sample(max_shear(field_literal), 0.5)
    let ms_field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::Real),
    };
    let ms_expr = make_function_call(
        "max_shear",
        vec![CompiledExpr::literal(field, field_type)],
        ms_field_type.clone(),
    );
    let sample_expr = make_function_call(
        "sample",
        vec![
            ms_expr,
            CompiledExpr::literal(Value::Real(0.5), Type::Real),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // max_shear of pure shear [[0,τ,0],[τ,0,0],[0,0,0]] = τ
    match &result {
        Value::Real(v) => {
            assert!(
                (v - tau).abs() < 1e-10,
                "expected max_shear ≈ {tau}, got {v}"
            );
        }
        _ => panic!(
            "sample(max_shear(field), point) should return Real({tau}), got {:?}",
            result
        ),
    }
}

// ── step-5: safety_factor dispatch ────────────────────────────────────────────

#[test]
fn eval_sample_safety_factor_field_dispatch() {
    // Uniaxial stress σ=100: von_mises = 100; yield=250 → safety_factor = 2.5
    let sigma = 100.0_f64;
    let yield_val = 250.0_f64;
    let tensor = make_stress_tensor(
        &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
    );
    let (field, field_type) = make_constant_tensor_field(tensor);

    // Build nested expr: sample(safety_factor(field_literal, 250.0), 0.5)
    // safety_factor(Field, yield) intercepts at lib.rs:295-300, captures [field, yield] in
    // lambda slot with SafetyFactor source.  sample dispatches via (_, SafetyFactor) at lib.rs:239.
    let sf_field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::Real),
    };
    let sf_expr = make_function_call(
        "safety_factor",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(yield_val), Type::Real),
        ],
        sf_field_type.clone(),
    );
    let sample_expr = make_function_call(
        "sample",
        vec![
            sf_expr,
            CompiledExpr::literal(Value::Real(0.5), Type::Real),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // safety_factor = yield / von_mises = 250 / 100 = 2.5
    match &result {
        Value::Real(v) => {
            assert!(
                (v - yield_val / sigma).abs() < 1e-10,
                "expected safety_factor ≈ {}, got {v}",
                yield_val / sigma
            );
        }
        _ => panic!(
            "sample(safety_factor(field, yield), point) should return Real(2.5), got {:?}",
            result
        ),
    }
}
