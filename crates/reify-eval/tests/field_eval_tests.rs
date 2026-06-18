//! Field declaration evaluation tests.
//!
//! Tests for evaluating `field def` declarations into Value::Field values
//! and applying field operations (sample, gradient, etc.).

use std::sync::Arc;

use reify_core::{ContentHash, DiagnosticCode, FIELD_ENTITY_PREFIX, Severity, Type, ValueCellId};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, FieldSourceKind, InterpolationKind, ResolvedFunction,
    SampledGridKind, Value, ValueMap,
};
use reify_test_support::{
    collect_errors, eval_source, make_engine, make_simple_engine, parse_and_compile,
    parse_and_compile_with_stdlib,
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
        eval_source("field def temp : Point3 -> Length { source = analytical { |p| 1.0m } }");

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
                matches!(source, reify_ir::FieldSourceKind::Analytical),
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
    let source = "field def temp : Point3 -> Length { source = analytical { |p| 1.0m } }";
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
        reify_ir::DeterminacyState::Determined,
        "field snapshot value should be Determined"
    );
}

// ── Task 2343 step-7: composed-field resampling after param edit ─────────
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
//
// TODO(v0.1 limitation): This test does NOT exercise the step-8 // ptodo:allow known v0.1 test coverage gap, no live task
// re-elaboration loop (`engine_edit.rs::edit_param`'s composed-field
// rebuild gated on `dirty_cone.contains(field_node)`). The change here is
// `S.k`, a structure param that is NOT in any field's captured set, so no
// field lambda is re-elaborated. What the test does pin: (a) the let
// cell's resample under the new param, (b) the reverse-index plumbing
// from step-6 (the let cell consuming `sample(scaled, k)` lands in the
// dirty cone and re-evaluates), and (c) the runtime field-call dispatch
// from step-7b (`base(p)` inside the composed lambda body resolves to
// the captured `__field.base` cell). The step-8 loop's positive path —
// a captured field cell changing and triggering a composed field's
// lambda Arc to be rebuilt — has no end-to-end edit_param-driven test
// in v0.1 because field lambdas cannot capture structure params. It is
// covered at the unit level by `reverse_index_includes_composed_field_dependencies`
// in `crates/reify-eval/src/deps.rs`. The negative half (no
// re-elaboration when no captured dep changes) is pinned by
// `eval_composed_field_invalidates_only_when_dep_changes` below.

/// Initial eval at k=2.0 yields 60.0; after `edit_param(k=5.0)`, the let
/// binding `val = sample(scaled, k)` re-evaluates to 150.0. Pins the
/// edit-cycle through a composed-field sample point and exercises the
/// reverse-index plumbing wired in step-6 plus the runtime field-call
/// dispatch from step-7b. See the TODO above for the scope limitation
/// vs. the step-8 re-elaboration loop.
#[test]
fn eval_composed_field_resamples_after_param_edit() {
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
// `eval_composed_field_resamples_after_param_edit`.

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
        let snapshot = engine.snapshot().expect("snapshot should exist after eval");
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
        let snapshot = engine.snapshot().expect("snapshot should exist after edit");
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

// ── Task 2343 step-11: composed-field lazy evaluation per sample ─────────
//
// Pin the lazy-evaluation guarantee from the task spec: the composed
// lambda body is evaluated per `sample()` call against the supplied
// point — never pre-applied at elaboration time. Regression guard
// against any future change that would cache lambda body output across
// sample points (e.g. memoizing the first-sample result inside
// `Value::Field` and short-circuiting subsequent `sample()` calls).
//
// Sample the same composed field at two distinct points within the same
// structure body. If the body were eagerly evaluated at elaboration, the
// two `sample()` calls would return identical values; instead they must
// reflect the supplied point. The composed body `|p| 2.0 * p + 1.0` is a
// non-constant function of its argument, so any short-circuit collapse
// to a single value would be detected.

/// `composed_f(p) = 2*p + 1` sampled at p=0 (→ 1.0) and p=10 (→ 21.0)
/// within a single structure body. Pins the per-sample evaluation
/// contract: the composed lambda body must be re-applied to each
/// supplied point, not collapsed to a single cached value.
#[test]
fn eval_composed_field_lazy_per_sample() {
    let source = r#"
field def composed_f : Real -> Real { source = composed { |p| 2.0 * p + 1.0 } }

structure def S {
    let val_at_zero = sample(composed_f, 0.0)
    let val_at_ten = sample(composed_f, 10.0)
}
"#;
    let result = eval_source(source);

    let val_at_zero = result
        .values
        .get(&ValueCellId::new("S", "val_at_zero"))
        .unwrap_or_else(|| panic!("'S.val_at_zero' not found in eval result"));
    assert_numeric_approx(val_at_zero, 1.0, "S.val_at_zero (p=0)");

    let val_at_ten = result
        .values
        .get(&ValueCellId::new("S", "val_at_ten"))
        .unwrap_or_else(|| panic!("'S.val_at_ten' not found in eval result"));
    assert_numeric_approx(val_at_ten, 21.0, "S.val_at_ten (p=10)");
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
        quantity: Box::new(Type::dimensionless_scalar()),
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
    let domain = Type::dimensionless_scalar();
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
            CompiledExpr::literal(Value::Real(point), Type::dimensionless_scalar()),
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
/// `codomain` is the scalar/list return type of `fn_name`, e.g. `Type::dimensionless_scalar()` for
/// von_mises / max_shear, `Type::List(Box::new(Type::dimensionless_scalar()))` for principal_stresses.
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
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(codomain.clone()),
    };
    let inner = make_function_call(fn_name, args, ft);
    let sample = make_sample_at(inner, 0.5, codomain);
    eval_expr(&sample, &EvalContext::simple(&ValueMap::new()))
}

// ── Helper test: make_sample_at shape ─────────────────────────────────────────

#[test]
fn test_make_sample_at_produces_sample_call() {
    let field_expr = CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar());
    let result = make_sample_at(field_expr.clone(), 0.5, Type::dimensionless_scalar());

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
    assert_eq!(result.result_type, Type::dimensionless_scalar(), "result_type should be Real");
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
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };
    let vm_expr = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        vm_field_type.clone(),
    );
    let sample_expr = make_sample_at(vm_expr, 0.5, Type::dimensionless_scalar());

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
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::List(Box::new(Type::dimensionless_scalar()))),
    };
    let ps_expr = make_function_call(
        "principal_stresses",
        vec![CompiledExpr::literal(field, field_type)],
        ps_field_type.clone(),
    );
    let sample_expr = make_sample_at(ps_expr, 0.5, Type::List(Box::new(Type::dimensionless_scalar())));

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
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };
    let ms_expr = make_function_call(
        "max_shear",
        vec![CompiledExpr::literal(field, field_type)],
        ms_field_type.clone(),
    );
    let sample_expr = make_sample_at(ms_expr, 0.5, Type::dimensionless_scalar());

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
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };
    let sf_expr = make_function_call(
        "safety_factor",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(yield_val), Type::dimensionless_scalar()),
        ],
        sf_field_type.clone(),
    );
    let sample_expr = make_sample_at(sf_expr, 0.5, Type::dimensionless_scalar());

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
    let result = eval_sampled_analysis("von_mises", tensor, vec![], Type::dimensionless_scalar());

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
        vec![(Value::Real(yield_val), Type::dimensionless_scalar())],
        Type::dimensionless_scalar(),
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
    let result = eval_sampled_analysis("von_mises", tensor, vec![], Type::dimensionless_scalar());

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
        Type::List(Box::new(Type::dimensionless_scalar())),
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
    let result = eval_sampled_analysis("max_shear", tensor, vec![], Type::dimensionless_scalar());

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
        vec![(Value::Real(yield_val), Type::dimensionless_scalar())],
        Type::dimensionless_scalar(),
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
        Type::List(Box::new(Type::dimensionless_scalar())),
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
    let p_ref = CompiledExpr::value_ref(p_id.clone(), Type::dimensionless_scalar());
    let threshold = CompiledExpr::literal(Value::Real(50.0), Type::dimensionless_scalar());
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
    let domain = Type::dimensionless_scalar();
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
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };

    // Sample at 75.0 → condition true → tensor_a → von Mises ≈ 100
    let vm_expr_high = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field.clone(), field_type.clone())],
        vm_field_type.clone(),
    );
    let sample_high = make_sample_at(vm_expr_high, 75.0, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result_high = eval_expr(&sample_high, &EvalContext::simple(&values));
    assert_real_approx(&result_high, sigma_a, "point=75 (>50): von Mises");

    // Sample at 25.0 → condition false → tensor_b → von Mises ≈ 200
    let vm_expr_low = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        vm_field_type,
    );
    let sample_low = make_sample_at(vm_expr_low, 25.0, Type::dimensionless_scalar());
    let result_low = eval_expr(&sample_low, &EvalContext::simple(&values));
    assert_real_approx(&result_low, sigma_b, "point=25 (<50): von Mises");
}

// ── Task 2341 step-9: eval-time elaboration of sampled field ────────────────
//
// Pins the v0.2 contract for `engine_eval::elaborate_field`'s Sampled arm:
// the five-key config is evaluated, the resulting Values parse into a
// `SampledField`, and the field's `lambda: Arc<Value>` slot holds
// `Value::SampledField(...)`. This test is the elaboration pin — sample
// dispatch is exercised separately by step-11/13/15/17 tests below.
//
// The source uses stdlib builtins `bbox` and `point3` to construct a
// `Value::BoundingBox`. Reify's `bbox(...)` constructor takes only 3D
// `Point3` arguments today; for `Regular1D` fields the eval-time parser
// uses just the x-component of the bounding box's min/max points.

#[test]
fn eval_sampled_field_elaborates_to_sampled_field_value() {
    let source = r#"field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "f");
    let field_val = result
        .values
        .get(&field_id)
        .unwrap_or_else(|| panic!("field 'f' not found in eval result values"));

    let lambda = match field_val {
        Value::Field { source, lambda, .. } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "expected FieldSourceKind::Sampled, got: {:?}",
                source
            );
            lambda
        }
        other => panic!("expected Value::Field, got: {:?}", other),
    };

    let sf = match lambda.as_ref() {
        Value::SampledField(sf) => sf,
        other => panic!(
            "expected Value::SampledField in field.lambda for Sampled source, got: {:?}",
            other
        ),
    };

    assert_eq!(sf.name, "f", "SampledField.name");
    assert_eq!(
        sf.kind,
        SampledGridKind::Regular1D,
        "expected Regular1D for grid = \"RegularGrid1\""
    );
    assert_eq!(
        sf.bounds_min,
        vec![0.0],
        "Regular1D bounds_min should be the x-component of the bbox min"
    );
    assert_eq!(
        sf.bounds_max,
        vec![2.0],
        "Regular1D bounds_max should be the x-component of the bbox max"
    );
    assert_eq!(sf.spacing, vec![1.0], "spacing in SI metres");
    assert_eq!(
        sf.interpolation,
        InterpolationKind::Linear,
        "interpolation = \"Linear\""
    );
    assert_eq!(sf.data, vec![0.0, 1.0, 2.0], "data in SI units, row-major");
    assert_eq!(
        sf.axis_grids.len(),
        1,
        "Regular1D should have one axis grid"
    );
    // axis_grids[0] is linspace(0, 2, spacing=1) → [0.0, 1.0, 2.0]
    let grid0 = &sf.axis_grids[0];
    assert_eq!(grid0.len(), 3, "axis_grids[0] should have 3 nodes");
    for (i, &expected) in [0.0_f64, 1.0, 2.0].iter().enumerate() {
        assert!(
            (grid0[i] - expected).abs() < 1e-12,
            "axis_grids[0][{i}]: expected {expected}, got {}",
            grid0[i]
        );
    }
}

// ── Task 2341 step-11: 1D sample dispatch on a Sampled field ───────────────
//
// Pins the v0.2 contract for `sample(f, x)` when `f.lambda` is
// `Value::SampledField`: the runtime sampled-field helper extracts the
// scalar query coord, calls `interp::interpolate_1d` with Linear method,
// and wraps the result in `Value::Real` for a dimensionless codomain.
//
// The grid is [0.0m, 1.0m, 2.0m] with data [0.0, 1.0, 2.0]. Linear
// interpolation midway between nodes returns the linear midpoint;
// querying exactly on a node returns the node value.

/// Linear midpoint between data nodes. `f` over `[0.0m, 2.0m]` spacing
/// `1.0m` data `[0.0, 1.0, 2.0]` Linear: `sample(f, 0.5m)` ≈ 0.5.
#[test]
fn sample_sampled_field_1d_linear_interpolation_returns_expected_value() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }

structure S {
    let val = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let val_id = ValueCellId::new("S", "val");
    let val = result
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'S.val' not found in eval result values"));
    match val {
        Value::Real(v) => assert!(
            (v - 0.5).abs() < 1e-12,
            "sample(f, 0.5m) expected 0.5 (linear midpoint), got {v}"
        ),
        Value::Scalar { si_value, .. } => assert!(
            (si_value - 0.5).abs() < 1e-12,
            "sample(f, 0.5m) expected 0.5 (linear midpoint), got {si_value}"
        ),
        other => panic!("expected Value::Real(0.5), got: {:?}", other),
    }
}

/// 2D bilinear interpolation at the centroid of a 2×2 grid.
///
/// Grid nodes (axis-0 outer, row-major):
/// `f(0,0)=0`, `f(0,1)=1`, `f(1,0)=2`, `f(1,1)=3`
/// Bilinear at `(0.5, 0.5)` is the average of all four corners = `1.5`.
#[test]
fn sample_sampled_field_2d_linear_interpolation_returns_expected_value() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid2" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(1.0m, 1.0m, 0.0m)) spacing = [1.0m, 1.0m] interpolation = "Linear" data = [0.0, 1.0, 2.0, 3.0] } }

structure S {
    let val = sample(f, point2(0.5m, 0.5m))
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let val_id = ValueCellId::new("S", "val");
    let val = result
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'S.val' not found in eval result values"));
    match val {
        Value::Real(v) => assert!(
            (v - 1.5).abs() < 1e-12,
            "sample(f, point2(0.5m, 0.5m)) expected 1.5 (bilinear centroid), got {v}"
        ),
        Value::Scalar { si_value, .. } => assert!(
            (si_value - 1.5).abs() < 1e-12,
            "sample(f, point2(0.5m, 0.5m)) expected 1.5, got {si_value}"
        ),
        other => panic!("expected Value::Real(1.5), got: {:?}", other),
    }
}

/// 3D trilinear interpolation at the centroid of a 2×2×2 grid.
///
/// Data `[0..7]` flattened row-major (axis-0 outer, axis-2 inner): the
/// eight corners of the unit cube hold values `0..=7`. Trilinear at
/// `(0.5, 0.5, 0.5)` is the average of all corners = `28/8 = 3.5`.
#[test]
fn sample_sampled_field_3d_linear_interpolation_returns_expected_value() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid3" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(1.0m, 1.0m, 1.0m)) spacing = [1.0m, 1.0m, 1.0m] interpolation = "Linear" data = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0] } }

structure S {
    let val = sample(f, point3(0.5m, 0.5m, 0.5m))
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let val_id = ValueCellId::new("S", "val");
    let val = result
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'S.val' not found in eval result values"));
    match val {
        Value::Real(v) => assert!(
            (v - 3.5).abs() < 1e-12,
            "sample(f, point3(0.5m,0.5m,0.5m)) expected 3.5 (trilinear centroid), got {v}"
        ),
        Value::Scalar { si_value, .. } => assert!(
            (si_value - 3.5).abs() < 1e-12,
            "sample(f, point3(0.5m,0.5m,0.5m)) expected 3.5, got {si_value}"
        ),
        other => panic!("expected Value::Real(3.5), got: {:?}", other),
    }
}

/// Exact-on-node sample. `sample(f, 1.0m)` should return the data value
/// at the midpoint node, i.e. `1.0`.
#[test]
fn sample_sampled_field_1d_at_grid_node_returns_exact_sample() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }

structure S {
    let val = sample(f, 1.0m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let val_id = ValueCellId::new("S", "val");
    let val = result
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'S.val' not found in eval result values"));
    match val {
        Value::Real(v) => assert!(
            (v - 1.0).abs() < 1e-12,
            "sample(f, 1.0m) expected 1.0 (exact node), got {v}"
        ),
        Value::Scalar { si_value, .. } => assert!(
            (si_value - 1.0).abs() < 1e-12,
            "sample(f, 1.0m) expected 1.0 (exact node), got {si_value}"
        ),
        other => panic!("expected Value::Real(1.0), got: {:?}", other),
    }
}

// ── Task 2341 step-15: out-of-bounds sample → Undef + once-per-session warning ──
//
// Pin two contracts at once:
// 1. Every OOB query returns `Value::Undef` (no fallback to clamped value).
// 2. The `W_FIELD_OUT_OF_BOUNDS` warning fires AT MOST ONCE per field per
//    session even when the same field has multiple OOB queries. The
//    `AtomicBool` flag on `SampledField` is the suppression mechanism.
//
// The structure has three sample sites that all query outside the field's
// `[0.0m, 1.0m]` bounds (5.0m, 10.0m, 2.0m). Each must produce Undef; the
// diagnostic vector should contain exactly one entry whose code is
// `FieldOutOfBounds`, severity `Warning`, and whose message names field
// `'f'`.

#[test]
fn sample_sampled_field_out_of_bounds_returns_undef_and_emits_warning_once() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(1.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0] } }

structure S {
    let oob_a = sample(f, 5.0m)
    let oob_b = sample(f, 10.0m)
    let oob_c = sample(f, 2.0m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    // Every OOB sample call returns Undef.
    for member in &["oob_a", "oob_b", "oob_c"] {
        let val = result
            .values
            .get(&ValueCellId::new("S", *member))
            .unwrap_or_else(|| panic!("'S.{member}' not found in eval result values"));
        assert!(
            val.is_undef(),
            "expected S.{member} = Undef for out-of-bounds sample, got {:?}",
            val
        );
    }

    // The `W_FIELD_OUT_OF_BOUNDS` warning fires exactly once across all
    // three OOB queries in this session (suppression by AtomicBool on
    // SampledField).
    let oob_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.code == Some(DiagnosticCode::FieldOutOfBounds)
        })
        .collect();
    assert_eq!(
        oob_warnings.len(),
        1,
        "expected exactly one W_FIELD_OUT_OF_BOUNDS warning, got {}: {:?}",
        oob_warnings.len(),
        result.diagnostics
    );
    assert!(
        oob_warnings[0].message.contains("'f'"),
        "OOB warning message should name field 'f', got: {:?}",
        oob_warnings[0].message
    );
}

/// RBF interpolation on a Sampled field is deferred post-v0.1: it falls back
/// to Linear and emits exactly one `W_INTERPOLATION_DEFERRED` warning.
///
/// Pinned by task 2341 step-17. The `interp::resolve_method` returns the
/// deferral diagnostic whenever the user asks for `Rbf` or `Kriging`; the
/// `sampled::sample_at_point` helper forwards `InterpolationResult.diagnostics`
/// into `ctx.diagnostics`, which the runtime sink then drains into
/// `EvalResult.diagnostics`.
///
/// Fallback proof: at the linear midpoint `0.5m` of `[0.0m, 2.0m]` data
/// `[0.0, 1.0, 2.0]`, Linear yields `0.5` — the test asserts that exact value
/// to confirm RBF dispatched through the Linear path, not some unimplemented
/// RBF kernel.
#[test]
fn sample_sampled_field_with_rbf_emits_interpolation_deferred_warning_and_falls_back_to_linear() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Rbf" data = [0.0, 1.0, 2.0] } }

structure S {
    let val = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    // (a) At least one InterpolationDeferred warning is emitted.
    let deferred_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.code == Some(DiagnosticCode::InterpolationDeferred)
        })
        .collect();
    assert!(
        !deferred_warnings.is_empty(),
        "expected at least one W_INTERPOLATION_DEFERRED warning, got diagnostics: {:?}",
        result.diagnostics
    );

    // (b) Fallback proof: RBF dispatched through the Linear path, so the
    // sample at 0.5m of [0.0, 1.0, 2.0] equals 0.5 (linear midpoint).
    let val_id = ValueCellId::new("S", "val");
    let val = result
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'S.val' not found in eval result values"));
    match val {
        Value::Real(v) => assert!(
            (v - 0.5).abs() < 1e-12,
            "RBF→Linear fallback at 0.5m expected 0.5 (linear midpoint), got {v}"
        ),
        Value::Scalar { si_value, .. } => assert!(
            (si_value - 0.5).abs() < 1e-12,
            "RBF→Linear fallback at 0.5m expected 0.5, got {si_value}"
        ),
        other => panic!("expected Value::Real(0.5), got: {:?}", other),
    }
}

/// Kriging interpolation on a Sampled field is deferred post-v0.1: it falls
/// back to Linear and emits exactly one `W_INTERPOLATION_DEFERRED` warning.
/// Companion to the RBF test above; same dispatch contract.
#[test]
fn sample_sampled_field_with_kriging_emits_interpolation_deferred_warning_and_falls_back_to_linear()
{
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Kriging" data = [0.0, 1.0, 2.0] } }

structure S {
    let val = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let deferred_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.code == Some(DiagnosticCode::InterpolationDeferred)
        })
        .collect();
    assert!(
        !deferred_warnings.is_empty(),
        "expected at least one W_INTERPOLATION_DEFERRED warning, got diagnostics: {:?}",
        result.diagnostics
    );

    let val_id = ValueCellId::new("S", "val");
    let val = result
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'S.val' not found in eval result values"));
    match val {
        Value::Real(v) => assert!(
            (v - 0.5).abs() < 1e-12,
            "Kriging→Linear fallback at 0.5m expected 0.5 (linear midpoint), got {v}"
        ),
        Value::Scalar { si_value, .. } => assert!(
            (si_value - 0.5).abs() < 1e-12,
            "Kriging→Linear fallback at 0.5m expected 0.5, got {si_value}"
        ),
        other => panic!("expected Value::Real(0.5), got: {:?}", other),
    }
}

// ── Step 21: parse-failure diagnostics for Sampled-field config ──────────
//
// These tests pin the W_FIELD_SAMPLED_INVALID_CONFIG warning emitted from
// `engine_eval::build_sampled_field` when the user's `sampled` config has
// a typo'd grid-kind tag, an unknown interpolation name, or a non-string
// value in a string-keyed slot. The field's lambda becomes `Value::Undef`
// (poisoned no-op) and any `sample(...)` returns `Undef`.
//
// Required diagnostic shape:
//   * `severity == Severity::Warning`
//   * `code == Some(DiagnosticCode::FieldSampledInvalidConfig)`
//   * message names the field (`'f'`) and (where applicable) both the
//     offending value and an allowed-set hint to guide the user toward
//     a valid config.

/// A typo'd grid-kind tag (`"RegularGrid42"`) emits a single
/// `W_FIELD_SAMPLED_INVALID_CONFIG` warning whose message names both the
/// offending value and the allowed-set hint, and the sample call returns
/// `Value::Undef`. Pins the parse-failure path in `parse_grid_kind`.
#[test]
fn eval_sampled_field_with_typo_grid_kind_emits_specific_diagnostic() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid42" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }

structure S {
    let v = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let invalid_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::FieldSampledInvalidConfig)
        })
        .collect();
    assert_eq!(
        invalid_warnings.len(),
        1,
        "expected exactly one W_FIELD_SAMPLED_INVALID_CONFIG warning, got {}: {:?}",
        invalid_warnings.len(),
        result.diagnostics
    );
    let msg = &invalid_warnings[0].message;
    assert!(
        msg.contains("'f'"),
        "diagnostic should name field 'f', got: {msg:?}"
    );
    assert!(
        msg.contains("RegularGrid42"),
        "diagnostic should mention the offending value 'RegularGrid42', got: {msg:?}"
    );
    assert!(
        msg.contains("RegularGrid1"),
        "diagnostic should mention the allowed-set hint 'RegularGrid1', got: {msg:?}"
    );

    let val = result
        .values
        .get(&ValueCellId::new("S", "v"))
        .expect("'S.v' not found in eval result values");
    assert!(
        val.is_undef(),
        "expected S.v = Undef on poisoned sampled field, got {val:?}"
    );
}

/// A typo'd interpolation tag (`"QuadraticSpline"`) emits a single
/// `W_FIELD_SAMPLED_INVALID_CONFIG` warning whose message names both the
/// offending value and the allowed-set hint (`"Linear"`), and the sample
/// call returns `Value::Undef`. Pins the parse-failure path in
/// `parse_interpolation`.
#[test]
fn eval_sampled_field_with_typo_interpolation_emits_specific_diagnostic() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "QuadraticSpline" data = [0.0, 1.0, 2.0] } }

structure S {
    let v = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let invalid_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::FieldSampledInvalidConfig)
        })
        .collect();
    assert_eq!(
        invalid_warnings.len(),
        1,
        "expected exactly one W_FIELD_SAMPLED_INVALID_CONFIG warning, got {}: {:?}",
        invalid_warnings.len(),
        result.diagnostics
    );
    let msg = &invalid_warnings[0].message;
    assert!(
        msg.contains("'f'"),
        "diagnostic should name field 'f', got: {msg:?}"
    );
    assert!(
        msg.contains("QuadraticSpline"),
        "diagnostic should mention the offending value 'QuadraticSpline', got: {msg:?}"
    );
    assert!(
        msg.contains("Linear"),
        "diagnostic should mention an allowed-set hint 'Linear', got: {msg:?}"
    );

    let val = result
        .values
        .get(&ValueCellId::new("S", "v"))
        .expect("'S.v' not found in eval result values");
    assert!(
        val.is_undef(),
        "expected S.v = Undef on poisoned sampled field, got {val:?}"
    );
}

/// A non-string value in the `grid = …` slot (here an `Int`) emits a single
/// `W_FIELD_SAMPLED_INVALID_CONFIG` warning whose message names the field
/// and indicates the slot expected a `String`, and the sample call returns
/// `Value::Undef`. Pins the wrong-Value-variant path in `parse_grid_kind`.
#[test]
fn eval_sampled_field_with_non_string_grid_value_emits_specific_diagnostic() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = 42 bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }

structure S {
    let v = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let invalid_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::FieldSampledInvalidConfig)
        })
        .collect();
    assert_eq!(
        invalid_warnings.len(),
        1,
        "expected exactly one W_FIELD_SAMPLED_INVALID_CONFIG warning, got {}: {:?}",
        invalid_warnings.len(),
        result.diagnostics
    );
    let msg = &invalid_warnings[0].message;
    assert!(
        msg.contains("'f'"),
        "diagnostic should name field 'f', got: {msg:?}"
    );
    // Must either explicitly name the expected variant ('String') or render
    // the unexpected value type (`Int(42)`) so the user can locate the typo.
    let mentions_expected_or_value = (msg.contains("expected") && msg.contains("String"))
        || msg.contains("Int")
        || msg.contains("42");
    assert!(
        mentions_expected_or_value,
        "diagnostic should mention either 'expected … String' or render the unexpected value type / value '42', got: {msg:?}"
    );

    let val = result
        .values
        .get(&ValueCellId::new("S", "v"))
        .expect("'S.v' not found in eval result values");
    assert!(
        val.is_undef(),
        "expected S.v = Undef on poisoned sampled field, got {val:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// Step-23: runtime-invariant violations must surface as
// W_FIELD_SAMPLED_INVALID_CONFIG warnings rather than panicking the eval
// loop via the `assert!`s in `interp::interpolate_Nd`.  Each test uses a
// configuration that would have hit one of those assertions before
// step-24 added pre-flight invariant checks in `build_sampled_field`.
// ──────────────────────────────────────────────────────────────────────────

/// `data` slice is shorter than the axis grid implied by `bounds`+`spacing`
/// (3-node grid, 2 data values).  Without step-24's invariant check,
/// `interp::interpolate_1d`'s `assert!(grid.len() == values.len())` would
/// panic the eval loop.  Pins: pre-flight check in `build_sampled_field`
/// turns the mismatch into a `W_FIELD_SAMPLED_INVALID_CONFIG` warning,
/// the field's lambda becomes `Value::Undef`, and any sample call returns
/// `Undef`.
#[test]
fn eval_sampled_field_with_mismatched_data_length_emits_diagnostic_no_panic() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0] } }

structure S {
    let v = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    // Must run to completion: any `assert!` panic in interp would surface
    // here as a panic message and fail the test (no `#[should_panic]`).
    let result = engine.eval(&compiled);

    let invalid_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::FieldSampledInvalidConfig)
        })
        .collect();
    assert_eq!(
        invalid_warnings.len(),
        1,
        "expected exactly one W_FIELD_SAMPLED_INVALID_CONFIG warning, got {}: {:?}",
        invalid_warnings.len(),
        result.diagnostics
    );
    let msg = &invalid_warnings[0].message;
    assert!(
        msg.contains("'f'"),
        "diagnostic should name field 'f', got: {msg:?}"
    );
    assert!(
        msg.contains("data length") || msg.contains("data"),
        "diagnostic should mention 'data length' or 'data', got: {msg:?}"
    );
    assert!(
        msg.contains("3") || msg.contains("axis grid"),
        "diagnostic should mention the grid shape '3' or 'axis grid', got: {msg:?}"
    );

    let val = result
        .values
        .get(&ValueCellId::new("S", "v"))
        .expect("'S.v' not found in eval result values");
    assert!(
        val.is_undef(),
        "expected S.v = Undef on poisoned sampled field, got {val:?}"
    );
}

/// Zero-length `bounds` span collapses the axis grid to a single node
/// (`linspace_inclusive(1.0, 1.0, 1.0) → [1.0]`).  Without step-24's
/// invariant check, `interp::interpolate_1d`'s
/// `assert!(grid.len() >= 2)` would panic the eval loop.  Pins: pre-flight
/// check in `build_sampled_field` turns this into a
/// `W_FIELD_SAMPLED_INVALID_CONFIG` warning and the sample returns
/// `Undef`.
#[test]
fn eval_sampled_field_with_degenerate_axis_grid_emits_diagnostic_no_panic() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(1.0m, 0.0m, 0.0m), point3(1.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0] } }

structure S {
    let v = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let invalid_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::FieldSampledInvalidConfig)
        })
        .collect();
    assert_eq!(
        invalid_warnings.len(),
        1,
        "expected exactly one W_FIELD_SAMPLED_INVALID_CONFIG warning, got {}: {:?}",
        invalid_warnings.len(),
        result.diagnostics
    );
    let msg = &invalid_warnings[0].message;
    assert!(
        msg.contains("'f'"),
        "diagnostic should name field 'f', got: {msg:?}"
    );
    assert!(
        msg.contains("axis") || msg.contains("at least 2") || msg.contains("nodes"),
        "diagnostic should mention 'axis', 'at least 2', or 'nodes', got: {msg:?}"
    );

    let val = result
        .values
        .get(&ValueCellId::new("S", "v"))
        .expect("'S.v' not found in eval result values");
    assert!(
        val.is_undef(),
        "expected S.v = Undef on poisoned sampled field, got {val:?}"
    );
}

/// Non-positive `spacing` (`0.0m`) collapses the axis grid to a single
/// node via `linspace_inclusive`'s defensive `spacing <= 0.0` guard.
/// Without step-24's invariant check, the resulting 1-node grid would
/// trip `interp::interpolate_1d`'s `assert!(grid.len() >= 2)` and panic
/// the eval loop.  Pins: pre-flight check in `build_sampled_field`
/// turns this into a `W_FIELD_SAMPLED_INVALID_CONFIG` warning whose
/// message names the `spacing` slot, and the sample returns `Undef`.
#[test]
fn eval_sampled_field_with_non_positive_spacing_emits_diagnostic_no_panic() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 0.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }

structure S {
    let v = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let invalid_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::FieldSampledInvalidConfig)
        })
        .collect();
    assert_eq!(
        invalid_warnings.len(),
        1,
        "expected exactly one W_FIELD_SAMPLED_INVALID_CONFIG warning, got {}: {:?}",
        invalid_warnings.len(),
        result.diagnostics
    );
    let msg = &invalid_warnings[0].message;
    assert!(
        msg.contains("'f'"),
        "diagnostic should name field 'f', got: {msg:?}"
    );
    assert!(
        msg.contains("spacing"),
        "diagnostic should mention 'spacing', got: {msg:?}"
    );

    let val = result
        .values
        .get(&ValueCellId::new("S", "v"))
        .expect("'S.v' not found in eval result values");
    assert!(
        val.is_undef(),
        "expected S.v = Undef on poisoned sampled field, got {val:?}"
    );
}

// ── Task 3060: excessive-axis cap diagnostic ─────────────────────────────

/// Task 3060: `build_sampled_field` rejects an axis whose interval count
/// exceeds [`reify_types::sampled::LINSPACE_MAX_INTERVALS`], emitting a
/// `W_FIELD_SAMPLED_INVALID_CONFIG` warning whose message names the cap
/// and returning `Undef` for the sample.
///
/// `bounds_max = 11_000_000 m` with `spacing = 1.0 m` gives
/// `n_intervals = 11_000_000`, just above the 10_000_000 cap.  This is
/// large enough to trip the cap but small enough that, without the cap,
/// the allocation (~88 MB) would succeed, so the RED failure mode is
/// deterministic: the data-shape-mismatch warning fires with the wrong
/// message content, and the cap-specific assertion fails cleanly.
#[test]
fn eval_sampled_field_with_excessive_axis_emits_diagnostic_no_panic() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(11000000.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }

structure S {
    let v = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let invalid_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::FieldSampledInvalidConfig)
        })
        .collect();
    assert_eq!(
        invalid_warnings.len(),
        1,
        "expected exactly one W_FIELD_SAMPLED_INVALID_CONFIG warning, got {}: {:?}",
        invalid_warnings.len(),
        result.diagnostics
    );
    let msg = &invalid_warnings[0].message;
    assert!(
        msg.contains("'f'"),
        "diagnostic should name field 'f', got: {msg:?}"
    );
    // Pin the cap phrase rather than the bare cap value — `msg.contains(&cap)` is
    // fragile when bounds_max happens to be a numeric string that contains the cap
    // as a substring (e.g. `bounds_max=110000000` contains "10000000").
    let cap = reify_ir::sampled::LINSPACE_MAX_INTERVALS.to_string();
    assert!(
        msg.contains(&format!("{cap} interval cap")),
        "diagnostic should contain '{cap} interval cap', got: {msg:?}"
    );
    // Pin the n_intervals embedding: the diagnostic must embed the actual computed
    // count (11000000) so users see how far over the cap they are.
    assert!(
        msg.contains("11000000 grid intervals"),
        "diagnostic should embed the computed interval count '11000000 grid intervals', got: {msg:?}"
    );
    assert!(
        msg.contains("axis 0"),
        "diagnostic should identify the offending axis (axis 0), got: {msg:?}"
    );

    let val = result
        .values
        .get(&ValueCellId::new("S", "v"))
        .expect("'S.v' not found in eval result values");
    assert!(
        val.is_undef(),
        "expected S.v = Undef on cap-exceeded sampled field, got {val:?}"
    );
}

/// Task 3187 step-2 RED: an overflowing axis (span/spacing > usize::MAX as f64) must
/// produce a *distinct* diagnostic from the cap-exceeded case.  The overflow phrasing
/// "more intervals than usize can represent" must appear and "interval cap" must NOT —
/// the cap phrase is reserved for the finite-too-large case.
///
/// Input: `bounds_max = 1e308 m`, `spacing = 1.0 m` → `(span / spacing) = 1e308`,
/// which exceeds `usize::MAX as f64` ≈ 1.84e19 → `LinspaceError::Overflow`.
#[test]
fn eval_sampled_field_with_overflowing_axis_emits_distinct_diagnostic_no_panic() {
    let source = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(1e308m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }

structure S {
    let v = sample(f, 0.5m)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let invalid_warnings: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::FieldSampledInvalidConfig)
        })
        .collect();
    assert_eq!(
        invalid_warnings.len(),
        1,
        "expected exactly one W_FIELD_SAMPLED_INVALID_CONFIG warning, got {}: {:?}",
        invalid_warnings.len(),
        result.diagnostics
    );
    let msg = &invalid_warnings[0].message;
    assert!(
        msg.contains("'f'"),
        "diagnostic should name field 'f', got: {msg:?}"
    );
    assert!(
        msg.contains("axis 0"),
        "diagnostic should identify the offending axis (axis 0), got: {msg:?}"
    );
    // Overflow case: distinct phrasing — not a cap-exceeded message.
    assert!(
        msg.contains("more intervals than usize can represent"),
        "overflow diagnostic should say 'more intervals than usize can represent', got: {msg:?}"
    );
    // The cap phrase must NOT appear: it belongs only to the finite-too-large case.
    assert!(
        !msg.contains("interval cap"),
        "overflow diagnostic must NOT contain 'interval cap' (cap-specific phrasing \
         should be reserved for LinspaceError::Excessive), got: {msg:?}"
    );

    let val = result
        .values
        .get(&ValueCellId::new("S", "v"))
        .expect("'S.v' not found in eval result values");
    assert!(
        val.is_undef(),
        "expected S.v = Undef on overflowing-axis sampled field, got {val:?}"
    );
}

// ── Plan 2913 step-21: end-to-end field reductions integration ───────────
//
// Pin the full parse → compile → eval pipeline for the four eager Field
// reductions added in plan 2913: `max(field)`, `min(field)`,
// `argmax(field)`, `argmin(field)`. The eval-time dispatch arms in
// `crates/reify-expr/src/lib.rs:371-390` route 1-arg-Field calls to
// `field_reductions::compute_*`; the unit tests in
// `crates/reify-expr/tests/field_reductions_tests.rs` exercise those
// helpers directly. This test confirms the same pipeline works when the
// call originates from a parsed `.ri` source — i.e. that no compile-time
// type-resolution gap blocks `argmax`/`argmin` from reaching the runtime
// dispatcher even though they are not declared in a stdlib `.ri` file
// (mirroring how `sample`/`gradient`/`von_mises` are reachable purely
// via the runtime dispatcher).
//
// Source: `field def temperature : Length -> Real { source = sampled
// { grid = "RegularGrid1" data = [1.0, 5.0, 3.0, 4.0, 2.0] ... } }`.
// Using `Length -> Real` (rather than `Real -> Real`) keeps the typing
// internally consistent with the dimensioned `bbox(point3(0.0m, ...))`
// bounds and `spacing = 1.0m` — no reliance on the elaborator's
// dimensionless-coercion path. Domain `Length` resolves to
// `Type::Scalar { dimension: LENGTH }`, so `argmax`/`argmin` return
// dimensioned `Value::Scalar { dimension: LENGTH }` coords. Codomain
// `Real` (dimensionless) keeps `max`/`min` as `Value::Real`.
//
// Expected:
// - `max(temperature) == Real(5.0)` (data buffer maximum)
// - `min(temperature) == Real(1.0)` (data buffer minimum)
// - `argmax(temperature) == Scalar { 1.0, LENGTH }` (coord at index 1)
// - `argmin(temperature) == Scalar { 0.0, LENGTH }` (coord at index 0)

#[test]
fn eval_field_reductions_on_sampled_field_returns_expected_values() {
    use reify_core::DimensionVector;

    let source = r#"
field def temperature : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(4.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [1.0, 5.0, 3.0, 4.0, 2.0] } }

structure S {
    let m = max(temperature)
    let lo = min(temperature)
    let xmax = argmax(temperature)
    let xmin = argmin(temperature)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let m = result
        .values
        .get(&ValueCellId::new("S", "m"))
        .unwrap_or_else(|| panic!("'S.m' not found in eval result values"));
    match m {
        Value::Real(v) => assert!(
            (v - 5.0).abs() < 1e-12,
            "max(temperature) expected 5.0, got {v}"
        ),
        other => panic!("expected Value::Real(5.0) for S.m, got: {:?}", other),
    }

    let lo = result
        .values
        .get(&ValueCellId::new("S", "lo"))
        .unwrap_or_else(|| panic!("'S.lo' not found in eval result values"));
    match lo {
        Value::Real(v) => assert!(
            (v - 1.0).abs() < 1e-12,
            "min(temperature) expected 1.0, got {v}"
        ),
        other => panic!("expected Value::Real(1.0) for S.lo, got: {:?}", other),
    }

    let xmax = result
        .values
        .get(&ValueCellId::new("S", "xmax"))
        .unwrap_or_else(|| panic!("'S.xmax' not found in eval result values"));
    match xmax {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "argmax(temperature) coord should carry LENGTH dimension"
            );
            assert!(
                (si_value - 1.0).abs() < 1e-12,
                "argmax(temperature) expected coord 1.0m (index 1), got {si_value}"
            );
        }
        other => panic!(
            "expected Value::Scalar {{ LENGTH, 1.0 }} for S.xmax, got: {:?}",
            other
        ),
    }

    let xmin = result
        .values
        .get(&ValueCellId::new("S", "xmin"))
        .unwrap_or_else(|| panic!("'S.xmin' not found in eval result values"));
    match xmin {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "argmin(temperature) coord should carry LENGTH dimension"
            );
            assert!(
                (si_value - 0.0).abs() < 1e-12,
                "argmin(temperature) expected coord 0.0m (index 0), got {si_value}"
            );
        }
        other => panic!(
            "expected Value::Scalar {{ LENGTH, 0.0 }} for S.xmin, got: {:?}",
            other
        ),
    }
}
