//! Field declaration evaluation tests.
//!
//! Tests for evaluating `field def` declarations into Value::Field values
//! and applying field operations (sample, gradient, etc.).

use std::sync::Arc;

use reify_expr::{EvalContext, eval_expr};
use reify_test_support::{eval_source, make_engine, parse_and_compile};
use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ContentHash, FIELD_ENTITY_PREFIX, FieldSourceKind,
    ResolvedFunction, Type, Value, ValueCellId, ValueMap,
};

/// Extract eigenvalues from a `Value::List` of three `Value::Real` items.
///
/// Panics with a descriptive message if any item is not `Value::Real`.
/// Panics if `items` does not contain exactly 3 elements.
/// Used by the three principal-stress tests to avoid duplicating the
/// extraction loop.
fn extract_eigenvalues(items: &[Value]) -> [f64; 3] {
    assert_eq!(
        items.len(),
        3,
        "expected 3 eigenvalues, got {}",
        items.len()
    );
    let mut eigenvalues = [0.0_f64; 3];
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::Real(v) => eigenvalues[i] = *v,
            _ => panic!("principal stress[{i}] should be Real, got {:?}", item),
        }
    }
    eigenvalues
}

#[test]
#[should_panic(expected = "expected 3 eigenvalues")]
fn extract_eigenvalues_panics_on_too_few_items() {
    extract_eigenvalues(&[Value::Real(1.0), Value::Real(2.0)]);
}

#[test]
#[should_panic(expected = "expected 3 eigenvalues")]
fn extract_eigenvalues_panics_on_too_many_items() {
    extract_eigenvalues(&[
        Value::Real(1.0),
        Value::Real(2.0),
        Value::Real(3.0),
        Value::Real(4.0),
    ]);
}

// ── Step 21: eval analytical field at point ────────────────────────────

#[test]
fn eval_analytical_field_at_point() {
    let result =
        eval_source("field def temp : Point3 -> Scalar { source = analytical { |p| 1.0m } }");

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
    // Uses Real -> Real so the body type (Real) matches the declared codomain (Real).
    let result = eval_source(
        r#"
field def identity_field : Real -> Real { source = analytical { |p| p } }

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
    let source = "field def temp : Point3 -> Scalar { source = analytical { |p| 1.0m } }";
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
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

// ── Task 2343 step-7: composed-field invalidation on dep change ──────────
//
// Pin the integration of composed-field re-elaboration with `edit_param`.
// The chain `composed → analytical` is sampled inside a structure body via
// `sample(scaled, k)` where `k` is a structure param. After the param edits,
// the let-binding's expression is re-evaluated against the new param value;
// the composed lambda must be available (and consistent with its registered
// deps) at the new sample point.
//
// Note: in v0.1, field lambda bodies cannot reference structure params
// directly (`unresolved name` at compile time), so we drive the change
// through the sample-point arm — the structure param `k` is the second
// argument to `sample`. The plan's expected values are preserved by
// scaling: `scaled(p) = base(p) * 30`, so `sample(scaled, k) = k * 30`.

/// Initial eval at k=2.0 yields 60.0; after `edit_param(k=5.0)`, the let
/// binding `val = sample(scaled, k)` re-evaluates to 150.0.  Pins the
/// edit-cycle through a composed-field sample point and exercises the
/// reverse-index plumbing wired in step-6.
#[test]
fn eval_composed_field_invalidates_on_dep_change() {
    let source = r#"
field def base : Real -> Real { source = analytical { |p| p } }
field def scaled : Real -> Real { source = composed { |p| base(p) * 30.0 } }

structure def S {
    param k : Real = 2.0
    let val = sample(scaled, k)
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();

    // Cold-start eval: k = 2.0, val = sample(scaled, 2.0) = base(2.0) * 30 = 60.0.
    let initial = engine.eval(&compiled);
    let val_id = ValueCellId::new("S", "val");
    let val = initial
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'S.val' not found in initial eval values"));
    assert_numeric_approx(val, 60.0, "initial S.val (k=2.0)");

    // Edit param k from 2.0 to 5.0; val must re-evaluate to 5.0 * 30 = 150.0.
    let k_id = ValueCellId::new("S", "k");
    let after = engine
        .edit_param(k_id, Value::Real(5.0))
        .expect("edit_param(S.k) should succeed");
    let val_after = after
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'S.val' not found in post-edit values"));
    assert_numeric_approx(val_after, 150.0, "post-edit S.val (k=5.0)");
}

// ── Task 2343 step-9: composed-field re-elaboration precision ────────────
//
// Pin the negative half of the precision contract for `Engine::edit_param`'s
// composed-field re-elaboration loop (added in step-8): an edit to a param
// that is NOT a transitive dep of any field must NOT re-elaborate any field's
// `Value::Field { lambda, .. }`. Verified via `Arc::ptr_eq` on the lambda
// pointers in `snapshot.values` before and after the edit.
//
// The positive half (a field IS re-elaborated when one of its tracked deps
// changes) cannot be driven through `edit_param` alone in v0.1: field lambda
// bodies cannot reference structure params directly (`unresolved name` at
// compile time), so no path from a param edit to a field-cell dirty-cone hit
// exists. The end-to-end edit cycle through a composed-field sample point —
// which exercises the runtime field-call dispatch fallback (step-7b) and
// confirms that re-evaluating downstream `Let` cells against fresh values
// produces correct results — is covered by step-7's
// `eval_composed_field_invalidates_on_dep_change`.

/// After cold-start eval, capture every field's lambda Arc from
/// `snapshot.values`. Edit a structure param that is NOT a dep of any field
/// (`S.k` only flows through `sample(...,k)` arguments — `k` is not in any
/// field's captured set). Assert: every field's lambda Arc is the SAME
/// pointer post-edit. This confirms the re-elaboration loop is gated on
/// `dirty_cone.contains(field_node)` and does NOT re-elaborate fields whose
/// NodeId is absent from the dirty cone.
#[test]
fn eval_composed_field_invalidates_only_when_dep_changes() {
    let source = r#"
field def f1 : Real -> Real { source = analytical { |p| p * 2.0 } }
field def f2 : Real -> Real { source = analytical { |p| p + 1.0 } }
field def composed_a : Real -> Real { source = composed { |p| f1(p) * 10.0 } }
field def composed_b : Real -> Real { source = composed { |p| f2(p) * 10.0 } }

structure def S {
    param k : Real = 2.0
    let val_a = sample(composed_a, k)
    let val_b = sample(composed_b, k)
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    let _initial = engine.eval(&compiled);

    let composed_a_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "composed_a");
    let composed_b_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "composed_b");
    let f1_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "f1");
    let f2_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "f2");

    /// Pluck the `Arc<Value>` lambda pointer from a `Value::Field` entry in
    /// `snapshot.values`. Panics with a descriptive message if the entry is
    /// missing or not a `Value::Field` — both indicate a structural bug
    /// upstream (the field elaboration loop must produce `Value::Field`
    /// entries for every declared field).
    fn extract_lambda_arc(
        snapshot: &reify_eval::snapshot::Snapshot,
        id: &ValueCellId,
    ) -> Arc<Value> {
        match snapshot.values.get(id).map(|(v, _)| v) {
            Some(Value::Field { lambda, .. }) => Arc::clone(lambda),
            Some(other) => panic!("expected Value::Field for {id}, got {:?}", other),
            None => panic!("expected snapshot entry for {id}, got None"),
        }
    }

    let (pre_a, pre_b, pre_f1, pre_f2) = {
        let snapshot = engine
            .snapshot()
            .expect("snapshot should exist after eval");
        (
            extract_lambda_arc(snapshot, &composed_a_id),
            extract_lambda_arc(snapshot, &composed_b_id),
            extract_lambda_arc(snapshot, &f1_id),
            extract_lambda_arc(snapshot, &f2_id),
        )
    };

    // Edit S.k. k is NOT a dep of any field's lambda — no field should be
    // re-elaborated. Sanity-check: val_a and val_b ARE dirty (they take k as
    // a sample-point argument), so the eval loop will refresh them; that
    // refresh path uses runtime field-call dispatch (step-7b), not field
    // re-elaboration.
    let k_id = ValueCellId::new("S", "k");
    let _after = engine
        .edit_param(k_id, Value::Real(5.0))
        .expect("edit_param(S.k) should succeed");

    let (post_a, post_b, post_f1, post_f2) = {
        let snapshot = engine
            .snapshot()
            .expect("snapshot should exist after edit");
        (
            extract_lambda_arc(snapshot, &composed_a_id),
            extract_lambda_arc(snapshot, &composed_b_id),
            extract_lambda_arc(snapshot, &f1_id),
            extract_lambda_arc(snapshot, &f2_id),
        )
    };

    // Precision: every field's lambda Arc must be the SAME pointer post-edit.
    // No field is in the dirty cone of S.k, so the step-8 re-elaboration loop
    // must skip every field.
    assert!(
        Arc::ptr_eq(&pre_a, &post_a),
        "composed_a lambda Arc changed across edit_param(S.k); step-8 \
         re-elaboration must NOT fire when the field is absent from the dirty cone"
    );
    assert!(
        Arc::ptr_eq(&pre_b, &post_b),
        "composed_b lambda Arc changed across edit_param(S.k); step-8 \
         re-elaboration must NOT fire when the field is absent from the dirty cone"
    );
    assert!(
        Arc::ptr_eq(&pre_f1, &post_f1),
        "f1 (analytical) lambda Arc changed across edit_param(S.k); analytical \
         fields are excluded from the step-8 re-elaboration loop"
    );
    assert!(
        Arc::ptr_eq(&pre_f2, &post_f2),
        "f2 (analytical) lambda Arc changed across edit_param(S.k); analytical \
         fields are excluded from the step-8 re-elaboration loop"
    );
}

/// Like `assert_real_approx` but also accepts `Value::Int` whose value matches
/// the expected magnitude. Reify's literal parser can collapse integer-valued
/// Real literals (e.g. `30.0`) to `Value::Int` along certain compile/eval
/// paths; tests asserting numeric equality should accept either representation.
/// Sister helper: pattern at `eval_sample_field_point` (lines ~122-126).
fn assert_numeric_approx(val: &Value, expected: f64, label: &str) {
    match val {
        Value::Real(v) => assert!(
            (v - expected).abs() < REAL_TOLERANCE,
            "{label}: expected {expected}, got Real({v}) (diff = {})",
            (v - expected).abs()
        ),
        Value::Int(n) => assert!(
            (*n as f64 - expected).abs() < REAL_TOLERANCE,
            "{label}: expected {expected}, got Int({n})"
        ),
        other => panic!(
            "{label}: expected numeric value ≈ {expected}, got {:?}",
            other
        ),
    }
}

// ── Analysis sampling dispatch tests (eval-level) ────────────────────────────
//
// These tests exercise the full sampling dispatch path:
//   sample(analysis_op(tensor_field), point)
//   → the FieldSourceKind dispatch match in eval_expr's "sample" arm (crates/reify-expr/src/lib.rs)
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
///
/// Sister helper: `make_stress_tensor` in `crates/reify-expr/tests/field_analysis_tests.rs`
/// uses `Value::Scalar { si_value, dimension }` elements (Pressure-dimensioned); keep
/// the two in sync if the `Value::Tensor` nesting structure ever changes.
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
/// # Domain choice
/// A single-parameter `(p: Real)` domain is intentional. The sampling dispatch
/// calls `apply_lambda_with_point_unpacking`, which unpacks a `Point3` into
/// `(x, y, z)` for real fields. Using `Real` avoids that complexity and keeps
/// the focus on dispatch correctness. The Point3 unpacking path is covered by
/// `make_constant_stress_field` and its tests in
/// `crates/reify-expr/tests/field_analysis_tests.rs`.
///
/// # Sister helper
/// `make_constant_stress_field` in `crates/reify-expr/tests/field_analysis_tests.rs`
/// uses Pressure-dimensioned Scalars and a 3-parameter Point3 lambda; keep the
/// structural shape consistent if refactoring either.
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
        lambda: Arc::new(lambda),
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

/// Build a `sample(field_expr, point)` CompiledExpr.
///
/// Delegates to `make_function_call("sample", ...)`, fixing the function name to
/// `"sample"` and constructing the point literal from a bare `f64`.
fn make_sample_at(field_expr: CompiledExpr, point: f64, result_type: Type) -> CompiledExpr {
    make_function_call(
        "sample",
        vec![
            field_expr,
            CompiledExpr::literal(Value::Real(point), Type::Real),
        ],
        result_type,
    )
}

/// Tolerance for floating-point equality assertions in Real-value tests.
const REAL_TOLERANCE: f64 = 1e-10;

/// Assert that `val` is a `Value::Real` whose value is within [`REAL_TOLERANCE`] of `expected`.
///
/// Panics with a descriptive message if `val` is not `Value::Real` or if the
/// absolute difference exceeds [`REAL_TOLERANCE`].
fn assert_real_approx(val: &Value, expected: f64, label: &str) {
    match val {
        Value::Real(v) => {
            assert!(
                (v - expected).abs() < REAL_TOLERANCE,
                "{label}: expected {expected}, got {v} (diff = {})",
                (v - expected).abs()
            );
        }
        other => panic!("{label}: expected Value::Real({expected}), got {:?}", other),
    }
}

/// Eval `sample(fn_name(constant_tensor_field, …extra_args), 0.5)` and return the result.
///
/// Encapsulates the recurring setup pattern in edge-case dispatch tests:
///   1. Wrap `tensor` in an analytical `Real → Matrix3x3(Real)` field.
///   2. Build `fn_name(field_lit, …extra_args)` with field-type codomain `codomain`.
///   3. Wrap in `sample(…, 0.5)`.
///   4. Evaluate and return the `Value`.
///
/// `codomain` is the scalar/list return type of `fn_name`, e.g. `Type::Real` for
/// von_mises / max_shear, `Type::List(Box::new(Type::Real))` for principal_stresses.
/// `extra_args` carries any additional literal arguments (e.g. yield stress for
/// safety_factor) as `(Value, Type)` pairs.
fn eval_sampled_analysis(
    fn_name: &str,
    tensor: Value,
    extra_args: Vec<(Value, Type)>,
    codomain: Type,
) -> Value {
    let (field, field_type) = make_constant_tensor_field(tensor);
    let mut args = vec![CompiledExpr::literal(field, field_type)];
    args.extend(
        extra_args
            .into_iter()
            .map(|(v, t)| CompiledExpr::literal(v, t)),
    );
    let ft = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(codomain.clone()),
    };
    let inner = make_function_call(fn_name, args, ft);
    let sample = make_sample_at(inner, 0.5, codomain);
    eval_expr(&sample, &EvalContext::simple(&ValueMap::new()))
}

// ── Helper test: make_sample_at shape ─────────────────────────────────────────

#[test]
fn test_make_sample_at_produces_sample_call() {
    let field_expr = CompiledExpr::literal(Value::Real(0.0), Type::Real);
    let result = make_sample_at(field_expr.clone(), 0.5, Type::Real);

    // Verify kind is FunctionCall with name "sample"
    match &result.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            assert_eq!(
                function.name, "sample",
                "function name should be \"sample\""
            );
            assert_eq!(args.len(), 2, "should have exactly 2 args");
            // First arg should be the original field_expr (same content hash)
            assert_eq!(
                args[0].content_hash, field_expr.content_hash,
                "first arg should be the field_expr"
            );
            // Second arg should be a Real literal 0.5
            match &args[1].kind {
                CompiledExprKind::Literal(Value::Real(v)) => {
                    assert!(
                        (v - 0.5).abs() < 1e-12,
                        "second arg should be Real(0.5), got {v}"
                    );
                }
                other => panic!("second arg should be Literal(Real(0.5)), got {:?}", other),
            }
        }
        other => panic!("expected FunctionCall kind, got {:?}", other),
    }
    assert_eq!(result.result_type, Type::Real, "result_type should be Real");
}

// ── Helper test: assert_real_approx behavior ─────────────────────────────────

#[test]
#[allow(clippy::assertions_on_constants)]
fn test_assert_real_approx_passes_within_tolerance() {
    // Should not panic: value matches expected within REAL_TOLERANCE
    assert_real_approx(
        &Value::Real(std::f64::consts::PI),
        std::f64::consts::PI,
        "pi",
    );
    // Should not panic: difference is exactly at zero
    assert_real_approx(&Value::Real(0.0), 0.0, "zero");
    // Should not panic: value is just inside tolerance (90% of REAL_TOLERANCE)
    assert_real_approx(
        &Value::Real(std::f64::consts::PI + REAL_TOLERANCE * 0.9),
        std::f64::consts::PI,
        "near-boundary",
    );
    // Verify REAL_TOLERANCE constant exists and has the expected magnitude
    assert!(REAL_TOLERANCE > 0.0, "REAL_TOLERANCE must be positive");
    assert!(
        REAL_TOLERANCE <= 1e-10,
        "REAL_TOLERANCE should be small (≤1e-10)"
    );
}

#[test]
#[should_panic(expected = "diff =")]
fn test_assert_real_approx_panics_outside_tolerance() {
    // Difference of 1.0 is far beyond REAL_TOLERANCE — must panic
    assert_real_approx(&Value::Real(1.0), 2.0, "should fail");
}

#[test]
#[should_panic(expected = "diff =")]
fn test_assert_real_approx_panics_at_exact_boundary() {
    // Difference of exactly REAL_TOLERANCE is NOT strictly less-than, so must panic
    assert_real_approx(
        &Value::Real(std::f64::consts::PI + REAL_TOLERANCE),
        std::f64::consts::PI,
        "boundary",
    );
}

#[test]
#[should_panic(expected = "diff =")]
fn test_assert_real_approx_panics_at_exact_boundary_negative() {
    // Negative-direction: subtracting REAL_TOLERANCE also yields a difference of exactly
    // REAL_TOLERANCE (via .abs()), which is NOT strictly less-than, so must panic.
    // Guards against a regression where .abs() is removed from assert_real_approx.
    assert_real_approx(
        &Value::Real(std::f64::consts::PI - REAL_TOLERANCE),
        std::f64::consts::PI,
        "boundary-neg",
    );
}

#[test]
#[should_panic(expected = "expected Value::Real")]
fn test_assert_real_approx_panics_for_non_real_variant() {
    // Bool is not a Real — must panic with a descriptive message
    assert_real_approx(&Value::Bool(true), 0.0, "should fail");
}

// ── step-1: von_mises dispatch ────────────────────────────────────────────────
// (step-2 in the plan was "run test to verify it passes" — a verification step,
// not a distinct test; there is no step-2 test to write here.
// step-3 through step-5 correspond to the three remaining dispatch tests below.
// The wrapping-only check — that von_mises(field) returns a VonMises-sourced Field —
// lives in `von_mises_field_returns_field_with_von_mises_source` in
// crates/reify-expr/tests/field_analysis_tests.rs.)

#[test]
fn eval_sample_von_mises_field_dispatch() {
    // Uniaxial stress [[σ,0,0],[0,0,0],[0,0,0]]: von Mises = σ (dimensionless)
    let sigma = 100.0_f64;
    let tensor = make_stress_tensor(&[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
    let (field, field_type) = make_constant_tensor_field(tensor);

    // Build nested expr: sample(von_mises(field_literal), 0.5)
    // von_mises(Field) wraps via analysis::compute_von_mises in eval_expr's "von_mises" arm.
    // sample(VonMisesField, point) dispatches via the FieldSourceKind::VonMises match arm in eval_expr's "sample" arm.
    let vm_field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::Real),
    };
    let vm_expr = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        vm_field_type.clone(),
    );
    let sample_expr = make_sample_at(vm_expr, 0.5, Type::Real);

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // von Mises of uniaxial stress = σ
    assert_real_approx(&result, sigma, "von Mises");
}

// ── step-3: principal_stresses dispatch ───────────────────────────────────────

#[test]
fn eval_sample_principal_stresses_field_dispatch() {
    // Uniaxial [[100,0,0],[0,0,0],[0,0,0]]: eigenvalues [0.0, 0.0, 100.0] sorted ascending
    let sigma = 100.0_f64;
    let tensor = make_stress_tensor(&[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
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
    let sample_expr = make_sample_at(ps_expr, 0.5, Type::List(Box::new(Type::Real)));

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

    let eigenvalues = extract_eigenvalues(items);

    // Eigenvalues should be sorted ascending
    assert!(
        eigenvalues[0] <= eigenvalues[1] && eigenvalues[1] <= eigenvalues[2],
        "eigenvalues should be sorted ascending, got {:?}",
        eigenvalues
    );

    // Check known values: uniaxial stress eigenvalues = [0.0, 0.0, σ]
    let expected = [0.0_f64, 0.0, sigma];
    for (i, (item, &exp)) in items.iter().zip(expected.iter()).enumerate() {
        assert_real_approx(item, exp, &format!("principal stress[{i}]"));
    }
}

// ── step-4: max_shear dispatch ────────────────────────────────────────────────

#[test]
fn eval_sample_max_shear_field_dispatch() {
    // Pure shear [[0,τ,0],[τ,0,0],[0,0,0]]: eigenvalues [-τ, 0, τ]
    // max_shear = (τ - (-τ)) / 2 = τ
    let tau = 50.0_f64;
    let tensor = make_stress_tensor(&[&[0.0, tau, 0.0], &[tau, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
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
    let sample_expr = make_sample_at(ms_expr, 0.5, Type::Real);

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // max_shear of pure shear [[0,τ,0],[τ,0,0],[0,0,0]] = τ
    assert_real_approx(&result, tau, "max_shear");
}

// ── step-5: safety_factor dispatch ────────────────────────────────────────────

#[test]
fn eval_sample_safety_factor_field_dispatch() {
    // Uniaxial stress σ=100: von_mises = 100; yield=250 → safety_factor = 2.5
    let sigma = 100.0_f64;
    let yield_val = 250.0_f64;
    let tensor = make_stress_tensor(&[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
    let (field, field_type) = make_constant_tensor_field(tensor);

    // Build nested expr: sample(safety_factor(field_literal, 250.0), 0.5)
    // safety_factor(Field, yield) intercepts via analysis::compute_safety_factor in eval_expr's "safety_factor" arm.
    // sample dispatches via the (_, FieldSourceKind::SafetyFactor) match arm in eval_expr's "sample" arm.
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
    let sample_expr = make_sample_at(sf_expr, 0.5, Type::Real);

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // safety_factor = yield / von_mises = 250 / 100 = 2.5
    assert_real_approx(&result, yield_val / sigma, "safety_factor");
}

// ── Edge-case dispatch tests: zero tensor and hydrostatic tensor ──────────────
//
// These tests exercise the same eval-dispatch path as the 'happy path' tests
// above, but with degenerate stress states (zero and hydrostatic) that trigger
// boundary conditions in the analysis functions:
//   - von_mises = 0  →  safety_factor = yield/0 → infinity → sanitize_value → Undef
//   - hydrostatic eigenvalues all equal  →  max_shear = 0

// ── Edge case: von_mises of zero tensor through dispatch ──────────────────────

#[test]
fn eval_sample_von_mises_zero_tensor_dispatch() {
    // Zero tensor: all entries 0 → von Mises = 0
    let tensor = make_stress_tensor(&[&[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
    let result = eval_sampled_analysis("von_mises", tensor, vec![], Type::Real);

    match &result {
        Value::Real(v) => assert!(
            v.abs() < 1e-10,
            "expected von Mises ≈ 0.0 for zero tensor, got {v}"
        ),
        _ => panic!(
            "sample(von_mises(zero_field), point) should return Real(0.0), got {:?}",
            result
        ),
    }
}

// ── Edge case: safety_factor of zero tensor → Undef (divide by zero) ─────────

#[test]
fn eval_sample_safety_factor_zero_tensor_dispatch() {
    // Zero tensor with yield=250.0: von_mises=0 → yield/0 → infinity → sanitize_value → Undef
    let yield_val = 250.0_f64;
    let tensor = make_stress_tensor(&[&[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
    let result = eval_sampled_analysis(
        "safety_factor",
        tensor,
        vec![(Value::Real(yield_val), Type::Real)],
        Type::Real,
    );

    assert!(
        result.is_undef(),
        "sample(safety_factor(zero_field, 250.0), point) should return Undef (divide by zero), got {:?}",
        result
    );
}

// ── Edge case: von_mises of hydrostatic tensor through dispatch ───────────────

#[test]
fn eval_sample_von_mises_hydrostatic_dispatch() {
    // Hydrostatic tensor diag(p, p, p): all deviatoric differences are zero → von Mises = 0
    let p = 100.0_f64;
    let tensor = make_stress_tensor(&[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]]);
    let result = eval_sampled_analysis("von_mises", tensor, vec![], Type::Real);

    match &result {
        Value::Real(v) => assert!(
            v.abs() < 1e-10,
            "expected von Mises ≈ 0.0 for hydrostatic p={p}, got {v}"
        ),
        _ => panic!(
            "sample(von_mises(hydrostatic_field), point) should return Real(0.0), got {:?}",
            result
        ),
    }
}

// ── Edge case: principal_stresses of hydrostatic tensor ───────────────────────

#[test]
fn eval_sample_principal_stresses_hydrostatic_dispatch() {
    // Hydrostatic tensor diag(p, p, p): all eigenvalues equal p.
    // Exercises the diagonal fast-path in compute_eigenvalues_3x3 (off-diagonal sum ≤ 1e-30).
    let p = 100.0_f64;
    let tensor = make_stress_tensor(&[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]]);
    let result = eval_sampled_analysis(
        "principal_stresses",
        tensor,
        vec![],
        Type::List(Box::new(Type::Real)),
    );

    // Eigenvalues of diag(p, p, p) = [p, p, p] sorted ascending
    let Value::List(items) = &result else {
        panic!(
            "sample(principal_stresses(hydrostatic_field), pt) should return List, got {:?}",
            result
        );
    };
    assert_eq!(items.len(), 3, "should have 3 principal stresses");

    let eigenvalues = extract_eigenvalues(items);

    // Each eigenvalue should equal p
    for (i, &v) in eigenvalues.iter().enumerate() {
        assert!(
            (v - p).abs() < 1e-10,
            "principal stress[{i}]: expected {p}, got {v}"
        );
    }

    // Eigenvalues should be sorted ascending
    // Degenerate: all equal, so trivially sorted — guards against regression if tensor changes
    assert!(
        eigenvalues[0] <= eigenvalues[1] && eigenvalues[1] <= eigenvalues[2],
        "eigenvalues should be sorted ascending, got {:?}",
        eigenvalues
    );
}

// ── Edge case: max_shear of hydrostatic tensor ────────────────────────────────

#[test]
fn eval_sample_max_shear_hydrostatic_dispatch() {
    // Hydrostatic tensor diag(p, p, p): eigenvalues all equal → max_shear = (p−p)/2 = 0
    let p = 100.0_f64;
    let tensor = make_stress_tensor(&[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]]);
    let result = eval_sampled_analysis("max_shear", tensor, vec![], Type::Real);

    match &result {
        Value::Real(v) => assert!(
            v.abs() < 1e-10,
            "expected max_shear ≈ 0.0 for hydrostatic p={p}, got {v}"
        ),
        _ => panic!(
            "sample(max_shear(hydrostatic_field), point) should return Real(0.0), got {:?}",
            result
        ),
    }
}

// ── Edge case: safety_factor of hydrostatic tensor → Undef ───────────────────

#[test]
fn eval_sample_safety_factor_hydrostatic_dispatch() {
    // Hydrostatic tensor diag(p, p, p) with yield=250: von_mises=0 → Undef.
    // Confirms dispatch doesn't special-case tensor shape; same divide-by-zero path.
    let p = 100.0_f64;
    let yield_val = 250.0_f64;
    let tensor = make_stress_tensor(&[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]]);
    let result = eval_sampled_analysis(
        "safety_factor",
        tensor,
        vec![(Value::Real(yield_val), Type::Real)],
        Type::Real,
    );

    assert!(
        result.is_undef(),
        "sample(safety_factor(hydrostatic_field, 250.0), point) should return Undef, got {:?}",
        result
    );
}

// ── Edge case: principal_stresses of fully populated symmetric tensor ──────────
//
// Uses [[2,1,1],[1,3,1],[1,1,4]], which exercises the trigonometric eigenvalue
// branch in compute_eigenvalues_3x3 (non-zero off-diagonals). Trace=9 acts as
// a checksum; expected eigenvalues ≈ [1.3249, 2.4608, 5.2143].

#[test]
fn eval_sample_principal_stresses_full_symmetric_dispatch() {
    // Fully populated symmetric tensor with non-zero off-diagonal entries:
    // [[2,1,1],[1,3,1],[1,1,4]] — trace=9, exercises trigonometric eigenvalue branch
    let tensor = make_stress_tensor(&[&[2.0, 1.0, 1.0], &[1.0, 3.0, 1.0], &[1.0, 1.0, 4.0]]);
    let result = eval_sampled_analysis(
        "principal_stresses",
        tensor,
        vec![],
        Type::List(Box::new(Type::Real)),
    );

    let Value::List(items) = &result else {
        panic!(
            "sample(principal_stresses(full_sym_field), pt) should return List, got {:?}",
            result
        );
    };
    assert_eq!(items.len(), 3, "should have 3 principal stresses");

    let eigenvalues = extract_eigenvalues(items);

    // Checksum: sum of eigenvalues = trace = 2 + 3 + 4 = 9
    let trace_sum = eigenvalues.iter().sum::<f64>();
    assert!(
        (trace_sum - 9.0).abs() < 1e-8,
        "sum of eigenvalues should equal trace=9, got {trace_sum}"
    );

    // Eigenvalues should be sorted ascending
    assert!(
        eigenvalues[0] <= eigenvalues[1] && eigenvalues[1] <= eigenvalues[2],
        "eigenvalues should be sorted ascending, got {:?}",
        eigenvalues
    );

    // Known eigenvalues (trigonometric closed-form result): characteristic polynomial
    // λ³ - 9λ² + 23λ - 17 = 0, depressed form t³ - 4t - 2 = 0 (t = λ - 3).
    // The closed-form trig computation is accurate to ~1e-12; tolerance 1e-8 gives
    // comfortable margin while catching regressions much earlier than 1e-3.
    let expected = [1.3248691294_f64, 2.4608111272, 5.2143197434];
    for (i, (&got, &exp)) in eigenvalues.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - exp).abs() < 1e-8,
            "eigenvalue[{i}]: expected ≈ {exp}, got {got}"
        );
    }
}

// ── step-6: spatially-varying lambda — point propagation ─────────────────────
//
// Addresses the concern that constant-tensor tests could accidentally pass even
// if the dispatch short-circuits before evaluating the inner lambda. This test
// uses a conditional body:  |p| if p > 50.0 { tensor_a } else { tensor_b }
// sampling at two distinct points verifies that `p` is actually threaded through.

#[test]
fn eval_sample_von_mises_spatially_varying_field() {
    // tensor_a: uniaxial σ=100 → von Mises = 100
    let sigma_a = 100.0_f64;
    let tensor_a = make_stress_tensor(&[&[sigma_a, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
    // tensor_b: uniaxial σ=200 → von Mises = 200
    let sigma_b = 200.0_f64;
    let tensor_b = make_stress_tensor(&[&[sigma_b, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);

    // Build lambda body:  if p > 50.0 { tensor_a } else { tensor_b }
    let p_id = ValueCellId::new("$lambda0", "p");
    let p_ref = CompiledExpr::value_ref(p_id.clone(), Type::Real);
    let threshold = CompiledExpr::literal(Value::Real(50.0), Type::Real);
    let cond_expr = CompiledExpr::binop(BinOp::Gt, p_ref, threshold, Type::Bool);
    let then_branch = CompiledExpr::literal(tensor_a, real_matrix_type());
    let else_branch = CompiledExpr::literal(tensor_b, real_matrix_type());
    let body = CompiledExpr {
        content_hash: ContentHash::of(&[3])
            .combine(cond_expr.content_hash)
            .combine(then_branch.content_hash)
            .combine(else_branch.content_hash),
        kind: CompiledExprKind::Conditional {
            condition: Box::new(cond_expr),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        },
        result_type: real_matrix_type(),
    };
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
        lambda: Arc::new(lambda),
    };
    let field_type = Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(codomain),
    };

    let vm_field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::Real),
    };

    // Sample at 75.0 → condition true → tensor_a → von Mises ≈ 100
    let vm_expr_high = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field.clone(), field_type.clone())],
        vm_field_type.clone(),
    );
    let sample_high = make_sample_at(vm_expr_high, 75.0, Type::Real);
    let values = ValueMap::new();
    let result_high = eval_expr(&sample_high, &EvalContext::simple(&values));
    assert_real_approx(&result_high, sigma_a, "point=75 (>50): von Mises");

    // Sample at 25.0 → condition false → tensor_b → von Mises ≈ 200
    let vm_expr_low = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        vm_field_type,
    );
    let sample_low = make_sample_at(vm_expr_low, 25.0, Type::Real);
    let result_low = eval_expr(&sample_low, &EvalContext::simple(&values));
    assert_real_approx(&result_low, sigma_b, "point=25 (<50): von Mises");
}
