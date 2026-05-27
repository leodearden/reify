//! Field evaluation edge-case tests.
//!
//! Tests for `sample()` and `gradient()` behavioral contracts, including
//! edge cases like constant fields, nested gradients, dimensioned domains,
//! and non-numeric lambda outputs.

use std::sync::Arc;

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, Value, ValueMap};

/// Helper to build a FunctionCall expression for stdlib functions.
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

/// Helper to build a Value::Lambda with (name, id) param pairs.
fn make_value_lambda(
    params: Vec<(&str, ValueCellId)>,
    body: CompiledExpr,
    captures: ValueMap,
) -> Value {
    Value::Lambda {
        params: params
            .into_iter()
            .map(|(n, id)| (n.to_string(), id))
            .collect(),
        body: Box::new(body),
        captures,
    }
}

/// Helper that builds a 3-param `|x, y, z| → (x + y) + z` analytical field and
/// its corresponding `Type::Field`, parameterised only on `domain_type`.
///
/// The four `sample_multi_param_lambda_*` tests share this construction — the only
/// variation between them is whether the domain is `point3(Real)` or `vec3(Real)`.
/// Codomain is always `Type::Real` and the source is always `Analytical`.
///
/// Returns `(Value::Field { .. }, Type::Field { .. })` ready for use in a
/// `make_function_call("sample", …)` expression.
fn make_xyz_sum_field(domain_type: Type) -> (Value, Type) {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda body: (x + y) + z  (left-associative; BinOp is binary)
    let xy = CompiledExpr::binop(
        reify_ir::BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        reify_ir::BinOp::Add,
        xy,
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );

    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    (field, field_type)
}

/// Build the unevaluated `gradient(field)` expression shared by the three
/// String-codomain gradient tests:
/// - `gradient_of_field_with_non_numeric_lambda`
/// - `gradient_of_field_with_non_numeric_lambda_sampling_returns_undef`
/// - `gradient_of_field_with_non_numeric_lambda_sampling_panics_in_debug`
///
/// Returns the `gradient(field)` [`CompiledExpr`] ready to be passed to
/// `eval_expr`. The caller is responsible for evaluation and any downstream
/// assertions.
fn build_string_codomain_grad_expr() -> CompiledExpr {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| "not_a_number"  (non-numeric return value)
    let body = CompiledExpr::literal(Value::String("not_a_number".to_string()), Type::String);
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Real;
    let codomain_type = Type::String;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type.clone()),
    };

    make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type,
    )
}

// ── Durable: sample behavior tests ──────────────────────────────────────────

/// Sampling a field with Undef lambda returns Undef.
///
/// Construct a Value::Field with lambda=Box::new(Value::Undef) and
/// source=FieldSourceKind::Analytical. This simulates a gradient field
/// where inner_field is None (a separate task #630 adds FieldSourceKind::Gradient
/// with inner_field). Since the lambda is not a Lambda variant, sample()
/// correctly returns Undef.
#[test]
fn sample_field_with_undef_lambda() {
    let domain_type = Type::point3(Type::length());
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(Value::Undef),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    // sample(field, point) -> Undef because lambda is not a Lambda variant
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(point, Type::point3(Type::length())),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        sample_result,
        Value::Undef,
        "sample of field with Undef lambda must return Undef"
    );
}

/// Sampling a temperature-over-length field evaluates correctly.
///
/// Build a 1D field with domain=Scalar<Length>, codomain=Scalar<Temperature>,
/// lambda: |x| -> 2.0 * x. Verify sample(field, 3.0) returns 6.0 (= 2.0 * 3.0).
#[test]
fn sample_temperature_over_length_field() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 2.0 * x  (linear temperature field over length domain)
    let body = CompiledExpr::binop(
        reify_ir::BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type),
    };

    // sample(field, 3.0) -> 6.0 (2.0 * 3.0)
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), domain_type),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        sample_result,
        Value::Real(6.0),
        "sample(temperature_field, 3.0) should return 6.0 (2.0 * 3.0)"
    );
}

/// Sampling a 1-param analytical field with a 3-element Point: sample() binds
/// the entire Point to the single lambda parameter. The identity body `|x| x`
/// returns the bound value directly, making the binding observable.
///
/// # Contract pinned
///
/// sample()'s analytical path binds the **entire** `Value::Point` to a 1-param
/// lambda (no unpacking). The `params.len() > 1` guard in sample() ensures that
/// 1-param lambdas NEVER unpack Point inputs, even if the Point has multiple
/// elements. For a 1-param lambda the arity check **passes** (1 arg == 1 param),
/// and the body executes with `x` bound to the full `Value::Point(...)`.
///
/// Multi-param lambdas whose arity matches the Point's length **do** unpack —
/// see `sample_multi_param_lambda_binds_unpacked_point_components`.
/// Multi-param lambdas with mismatched arity or scalar (non-Point) inputs still
/// hit the apply_lambda arity check and return Undef —
/// see `sample_multi_param_lambda_with_scalar_input_returns_undef`.
///
/// Uses an identity body (`|x| x`) rather than `-x` because `-x` on a Point
/// also returns Undef (via affine negate rules), which would mask a
/// hypothetical decomposition bug that also produces Undef. With the identity
/// body, a decomposition bug (3 args vs 1 param → Undef) would fail the
/// `result == Point(...)` assertion.
///
/// The Point components use `Value::Scalar { dimension: LENGTH }` to match
/// the declared `domain_type: Type::point3(Type::length())`.
///
/// Note: the apply_lambda arity check does NOT fire in this test (1 arg == 1
/// param). See `sample_multi_param_lambda_with_scalar_input_returns_undef`
/// for the test that directly pins the arity-check path.
#[test]
fn sample_one_param_lambda_binds_entire_point_as_single_value() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| -> x  (identity body — returns whatever x is bound to)
    let body = CompiledExpr::value_ref(x_id.clone(), Type::point3(Type::length()));

    // Inline verification: identity body with x = Point3(1m, 2m, 3m) returns
    // the Point unchanged.  Self-contained; no dependency on other test files.
    let point3_val = Value::Point(vec![
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    let mut body_check = ValueMap::new();
    body_check.insert(x_id.clone(), point3_val.clone());
    assert_eq!(
        eval_expr(&body, &EvalContext::simple(&body_check)),
        point3_val.clone(),
        "identity body with x=Point3 must return the Point itself"
    );

    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::point3(Type::length());
    let codomain_type = Type::point3(Type::length());

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type),
    };

    // sample(field, Point3(1m, 2m, 3m)) -> Point3(1m, 2m, 3m)
    // apply_lambda sees 1 arg (the whole Point) vs 1 param -> arity passes.
    // Identity body returns x = the whole Point directly.
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(point3_val.clone(), domain_type),
        ],
        Type::point3(Type::length()),
    );

    let values = ValueMap::new();
    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        sample_result, point3_val,
        "sample of 1-param field with 3-element Point must bind the entire Point \
         to x and return it (identity body); Undef would indicate arity mismatch \
         from incorrect decomposition"
    );
}

/// Sampling a multi-param lambda with a **scalar** (non-Point) input returns Undef
/// because sample() only unpacks `Value::Point` inputs; scalar inputs are always
/// forwarded as a single argument.
///
/// # Contract pinned
///
/// sample()'s unpacking path only fires when the second argument is `Value::Point`
/// **and** `params.len() > 1` **and** `params.len() == items.len()`. When the input
/// is a scalar (`Value::Real`, `Value::Int`, `Value::Scalar`, etc.) it is always
/// forwarded as a single-element slice to apply_lambda
/// (`&evaluated_args[1..]` has length 1). A lambda with more than one parameter
/// then hits the arity check in apply_lambda at `crates/reify-expr/src/lib.rs`:
///
/// ```rust
/// if args.len() != params.len() {
///     return Value::Undef;
/// }
/// ```
///
/// Here `args.len() == 1` but `params.len() == 3`, so the check fires and
/// `Value::Undef` is returned **before the body is evaluated**. The constant
/// body (`42.0`) is therefore unreachable — it satisfies lambda construction
/// and matches the idiomatic 3-param pattern from
/// `sample_gradient_of_constant_field_near_zero`.
///
/// # Cross-reference
///
/// `gradient_wrong_size_tensor_point_returns_undef` in `gradient_tests.rs`
/// pins the same `apply_lambda` arity contract via the gradient path.
///
/// For the case where a matching-arity `Value::Point` **is** provided and the
/// lambda has `params.len() > 1`, see
/// `sample_multi_param_lambda_binds_unpacked_point_components` — that test pins
/// the successful unpacking path.
///
/// For the case where a 1-param lambda receives a `Value::Point`, see
/// `sample_one_param_lambda_binds_entire_point_as_single_value` — the whole
/// Point is bound to the single parameter (no unpacking).
///
/// # Intentional type-incoherence
///
/// This `Field` uses `domain_type: Type::Real` with a 3-param lambda — a
/// combination the compiler would never emit. The mismatch is intentional: it
/// directly exercises the runtime's defensive arity-check path without
/// requiring a Point input. `sample()`'s analytical dispatch does not consult
/// the type metadata; the arity check fires on argument count alone.
#[test]
fn sample_multi_param_lambda_with_scalar_input_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| 42.0  (constant body; unreachable due to arity check)
    let body = CompiledExpr::literal(Value::Real(42.0), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::Real;
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type),
    };

    // sample(field, Real(1.0)) -> Undef
    // Scalar input is forwarded as a single arg; apply_lambda sees 1 arg vs
    // 3 params -> arity check fires -> Undef (unpacking only fires for Point).
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(1.0), domain_type),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        sample_result,
        Value::Undef,
        "sample() forwards scalar (non-Point) inputs as a single arg; \
         multi-param lambda with scalar input must hit the apply_lambda \
         arity check (1 forwarded arg vs 3 params) and return Undef"
    );
    // Positive control: the identical constant body IS reachable via a 1-param
    // lambda (arity matches).  This proves that Undef above is caused solely by
    // the arity check, not by anything in the body.
    let x_id2 = ValueCellId::new("$lambda0.S", "x");
    let body2 = CompiledExpr::literal(Value::Real(42.0), Type::Real);
    let lambda2 = make_value_lambda(vec![("x", x_id2)], body2, ValueMap::new());
    let field2 = Value::Field {
        domain_type: Type::Real,
        codomain_type: Type::Real,
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda2),
    };
    let field2_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::Real),
    };
    let sample_expr2 = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field2, field2_type),
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
        ],
        Type::Real,
    );
    let positive_result = eval_expr(&sample_expr2, &EvalContext::simple(&values));
    assert_eq!(
        positive_result,
        Value::Real(42.0),
        "1-param lambda body IS reachable (arity matches); proves Undef \
         above comes from the arity check, not the body content"
    );
}

/// Sampling a 3D analytical field with a matching-arity multi-param lambda unpacks
/// the input `Point` into individual lambda arguments.
///
/// # Contract pinned
///
/// When sample() is called with a `Value::Point` second argument and the lambda has
/// `params.len() > 1` **and** `params.len() == items.len()`, sample()'s analytical
/// path must unpack the Point components and forward them as individual scalar
/// arguments to apply_lambda.  The arity then matches (3 args == 3 params) and
/// the body executes with x, y, z bound to the three Real components.
///
/// This mirrors the calculus convention established in
/// `calculus::detect_single_point_param` (crates/reify-expr/src/calculus.rs:526):
/// a lambda with `params.len() == n > 1` receives `n` individual scalar arguments
/// when passed a matching-length Point.
///
/// Contrast with:
/// - `sample_one_param_lambda_binds_entire_point_as_single_value` — a 1-param
///   lambda always receives the **whole** Point (no unpacking) because
///   `params.len() > 1` is FALSE.
/// - `sample_multi_param_lambda_with_scalar_input_returns_undef` — a scalar (non-Point)
///   input with a 3-param lambda still returns Undef via the arity check, because
///   the unpacking path only fires for `Value::Point` inputs.
///
/// # Type choice
///
/// Uses `Type::point3(Type::Real)` domain and `Value::Real` Point components so
/// that the body `x + y + z` evaluates via the `Real + Real = Real` arm in eval_add
/// and returns exactly `Value::Real(6.0)` (not `Value::Scalar { .. }`).
#[test]
fn sample_multi_param_lambda_binds_unpacked_point_components() {
    let domain_type = Type::point3(Type::Real);
    let (field, field_type) = make_xyz_sum_field(domain_type.clone());

    // sample(field, Point([1.0, 2.0, 3.0])) -> Real(6.0)
    // sample() unpacks Point([1.0, 2.0, 3.0]) into [x=1.0, y=2.0, z=3.0].
    // apply_lambda sees 3 args == 3 params -> arity passes.
    // Body (x + y) + z = (1.0 + 2.0) + 3.0 = 6.0.
    let point_val = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(point_val, domain_type),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        sample_result,
        Value::Real(6.0),
        "sample() of 3D Point field with matching-arity multi-param lambda must unpack \
         Point components into x, y, z arguments; expected Real(6.0) from (1+2)+3 body"
    );
}

/// Sampling a multi-param lambda field with a Point whose length does NOT match
/// `params.len()` must fall through the unpacking guard and return `Undef` via
/// the apply_lambda arity check.
///
/// # Contract pinned
///
/// The unpacking guard in sample() fires only when **all three** conditions hold:
/// 1. `evaluated_args[1]` is `Value::Point` or `Value::Vector`,
/// 2. `params.len() > 1`, and
/// 3. `params.len() == items.len()` (arity matches).
///
/// When condition (3) fails — here a 3-param lambda receives a 2-element Point —
/// the guard does NOT fire.  The fallback `apply_lambda(lambda, &evaluated_args[1..],
/// ctx)` call forwards the whole Point as a single argument (arity 1 vs 3 params)
/// and apply_lambda returns `Undef` due to the arity mismatch.
///
/// Contrast with:
/// - `sample_multi_param_lambda_binds_unpacked_point_components` — matching arity
///   (3-param + Point(3)) → unpacking fires → `Real(6.0)`.
/// - `sample_multi_param_lambda_with_scalar_input_returns_undef` — scalar input
///   (non-Point) + 3-param lambda → `Undef` because the Point-guard never fires.
#[test]
fn sample_multi_param_lambda_with_mismatched_point_returns_undef() {
    let domain_type = Type::point3(Type::Real);
    let (field, field_type) = make_xyz_sum_field(domain_type.clone());

    // Point has 2 elements but lambda expects 3 params → guard condition (3) fails.
    // Fallback: apply_lambda sees 1 forwarded arg (the whole Point) vs 3 params → Undef.
    let mismatched_point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(mismatched_point, domain_type),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "sample() with 3-param lambda and a mismatched-length Point(2) must not unpack \
         (params.len() != items.len()); the arity check in apply_lambda must return Undef"
    );
}

/// Sampling a multi-param lambda field with a `Value::Vector` input unpacks the
/// vector components as individual scalar arguments, identical to the
/// `Value::Point` case.
///
/// # Contract pinned
///
/// `sample()` must accept both `Value::Point` and `Value::Vector` for multi-param
/// lambdas — they share structural representation (both wrap `Vec<Value>`), and
/// the calculus paths (`extract_point_coords`, `compute_numerical_divergence_at_point`,
/// `compute_numerical_curl_at_point`) already establish this convention with the
/// comment "Accept both Point and Vector — they share structural representation."
///
/// Keeping sample() Point-only creates a user-facing asymmetry where a
/// `Value::Vector([1.0, 2.0, 3.0])` passed to a 3-param lambda silently returns
/// `Undef` instead of `Real(6.0)`.
///
/// # Test specifics
///
/// - Field lambda: 3-param `|x, y, z| → (x + y) + z`
/// - Domain type: `Type::vec3(Type::Real)` (Vector3<Real>)
/// - Input: `Value::Vector([Real(1.0), Real(2.0), Real(3.0)])`
/// - Expected: `Real(6.0)` — vector unpacked into x=1.0, y=2.0, z=3.0; (1+2)+3=6
#[test]
fn sample_multi_param_lambda_with_vector_input() {
    let domain_type = Type::vec3(Type::Real);
    let (field, field_type) = make_xyz_sum_field(domain_type.clone());

    // sample(field, Vector([1.0, 2.0, 3.0])) -> Real(6.0)
    // sample() must unpack Vector([1.0, 2.0, 3.0]) into [x=1.0, y=2.0, z=3.0].
    // apply_lambda sees 3 args == 3 params -> arity passes.
    // Body (x + y) + z = (1.0 + 2.0) + 3.0 = 6.0.
    let vector_val = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(vector_val, domain_type),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(6.0),
        "sample() of a 3-param lambda with a matching-length Vector must unpack \
         Vector components into x, y, z arguments; expected Real(6.0) from (1+2)+3 body"
    );
}

/// Sampling a **1-param** lambda field with a `Value::Vector` input binds the
/// *entire* Vector as the single argument — no unpacking occurs.
///
/// # Contract pinned
///
/// The `params.len() > 1` guard in `sample()` is FALSE for a 1-param lambda, so
/// the Vector is forwarded to `apply_lambda` unchanged (as a single argument).
/// This mirrors `sample_one_param_lambda_binds_entire_point_as_single_value` and
/// guards against a future regression where someone inadvertently unpacks Vectors
/// but not Points (or vice-versa) in the 1-param path.
///
/// # Test specifics
///
/// - Field lambda: 1-param identity `|x| → x`
/// - Domain type: `Type::vec3(Type::Real)`
/// - Input: `Value::Vector([Real(1.0), Real(2.0), Real(3.0)])`
/// - Expected: `Value::Vector([Real(1.0), Real(2.0), Real(3.0)])` — the whole
///   Vector returned unchanged
#[test]
fn sample_one_param_lambda_binds_entire_vector_as_single_value() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    let domain_type = Type::vec3(Type::Real);

    // Lambda: |x| -> x  (identity body — returns whatever x is bound to)
    let body = CompiledExpr::value_ref(x_id.clone(), domain_type.clone());
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let codomain_type = domain_type.clone();

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type),
    };

    // sample(field, Vector([1.0, 2.0, 3.0])) -> Vector([1.0, 2.0, 3.0])
    // params.len() == 1, so the guard `params.len() > 1` is FALSE.
    // apply_lambda sees 1 arg (the whole Vector) vs 1 param -> arity passes.
    // Identity body returns x = the whole Vector directly.
    let vector_val = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(vector_val.clone(), domain_type),
        ],
        Type::vec3(Type::Real),
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        result, vector_val,
        "sample() of a 1-param field with a Vector must bind the entire Vector to x \
         and return it unchanged (no unpacking); Undef would indicate incorrect decomposition"
    );
}

/// Sampling a multi-param lambda field with a **mismatched-length** `Value::Vector`
/// input returns `Value::Undef` — the guard condition rejects it.
///
/// # Contract pinned
///
/// The guard in `sample()` fires when `params.len() != items.len()`. With a
/// 3-param lambda and a 2-element Vector the counts differ, so `sample()` falls
/// through to the single-arg path. `apply_lambda` then sees 1 arg (the whole
/// Vector) vs 3 params and returns `Value::Undef`. This mirrors
/// `sample_multi_param_lambda_with_mismatched_point_returns_undef` and pins that
/// the guard works symmetrically for Vector inputs.
///
/// # Test specifics
///
/// - Field lambda: 3-param `|x, y, z| → (x + y) + z`
/// - Domain type: `Type::vec3(Type::Real)` (3-element Vector)
/// - Input: `Value::Vector([Real(1.0), Real(2.0)])` — only 2 elements
/// - Expected: `Value::Undef` — length mismatch causes fallback to 1-arg path,
///   which then fails the arity check in `apply_lambda`
#[test]
fn sample_multi_param_lambda_with_mismatched_vector_returns_undef() {
    let domain_type = Type::vec3(Type::Real);
    let (field, field_type) = make_xyz_sum_field(domain_type.clone());

    // Vector has 2 elements but lambda expects 3 params → guard condition (3) fails.
    // Fallback: apply_lambda sees 1 forwarded arg (the whole Vector) vs 3 params → Undef.
    let mismatched_vector = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(mismatched_vector, domain_type),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "sample() with 3-param lambda and a mismatched-length Vector(2) must not unpack \
         (params.len() != items.len()); the arity check in apply_lambda must return Undef"
    );
}

// ── Transient: gradient stub tests (MUST be updated when gradient is implemented) ──

/// Gradient of a constant field should yield near-zero components.
///
/// Build a constant analytical field (lambda: |x,y,z| -> 42.0) and call
/// gradient(field). Currently returns Undef because gradient is a stub.
/// When gradient is implemented via numerical differentiation, the result
/// should be a vector field whose sampled components are all within 1e-9
/// of zero (the derivative of a constant is zero).
#[test]
fn sample_gradient_of_constant_field_near_zero() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| 42.0  (constant field)
    let body = CompiledExpr::literal(Value::Real(42.0), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::length());
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type),
    };

    // gradient(field) returns a gradient field. Sampling at any point should
    // yield components all within TOLERANCE of zero (constant field has zero gradient).
    const TOLERANCE: f64 = 1e-9;

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));
    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of constant field should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient field at Point3(1.0, 2.0, 3.0)
    let point = Value::Point(vec![
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    match &sample_result {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "gradient should have 3 components");
            for (i, comp) in components.iter().enumerate() {
                let val = comp
                    .as_f64()
                    .unwrap_or_else(|| panic!("component {} should be numeric, got {:?}", i, comp));
                assert!(
                    val.abs() < TOLERANCE,
                    "gradient component {} of constant field should be ~0, got {}",
                    i,
                    val
                );
            }
        }
        _ => panic!(
            "gradient sample should return a Vector, got {:?}",
            sample_result
        ),
    }
}

/// Gradient of gradient returns Undef (nested differential operators not supported).
///
/// Build an analytical field, construct gradient(field) which returns a Gradient-sourced
/// Field. Then construct gradient(gradient(field)). The outer gradient receives a
/// Gradient-sourced Field, which is rejected by the source whitelist (only Analytical
/// and Composed are supported), returning Undef.
#[test]
fn gradient_of_gradient_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x * x  (simple scalar field)
    let body = CompiledExpr::binop(
        reify_ir::BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Real;
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    // Inner gradient: gradient(field) -> Gradient-sourced Field
    let inner_gradient = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let inner_result = eval_expr(&inner_gradient, &EvalContext::simple(&values));
    assert!(
        matches!(&inner_result, Value::Field { .. }),
        "inner gradient(field) should return a Field, got {:?}",
        inner_result
    );

    // Outer gradient: gradient(gradient(field)) -> Undef
    // The Gradient-sourced field is rejected by the source whitelist.
    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    let outer_gradient = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(inner_result, grad_field_type)],
        Type::Real,
    );

    let outer_result = eval_expr(&outer_gradient, &EvalContext::simple(&values));
    assert_eq!(
        outer_result,
        Value::Undef,
        "gradient(gradient(field)) must return Undef: nested differential \
         operators are not supported"
    );
}

/// Gradient of field with Undef lambda returns Undef (stub).
///
/// Construct a Value::Field with lambda=Box::new(Value::Undef) and
/// source=FieldSourceKind::Analytical. gradient(field) returns Undef
/// because gradient is a stub.
#[test]
fn gradient_field_with_undef_lambda() {
    let domain_type = Type::point3(Type::length());
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(Value::Undef),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    // gradient(field) -> Undef (stub)
    let gradient_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    let values = ValueMap::new();
    let gradient_result = eval_expr(&gradient_expr, &EvalContext::simple(&values));
    assert_eq!(
        gradient_result,
        Value::Undef,
        "gradient of field with Undef lambda must return Undef"
    );
}

/// Gradient of temperature-over-length field returns a gradient Field.
///
/// Build a 1D field with domain=Scalar<Length>, codomain=Scalar<Temperature>,
/// lambda: |x| -> 2.0 * x. Call gradient(field) and verify the result is a Field.
/// Sample the gradient field at x=3.0[m] and verify the result is approximately
/// 2.0 with dimension Temperature/Length (not checked here since the lambda uses
/// Real(2.0) * Real(x), producing dimensionless Real — the codomain type annotation
/// drives the gradient's codomain, but the numerical result is 2.0).
#[test]
fn gradient_temperature_over_length_returns_field() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 2.0 * x  (linear temperature field over length domain)
    let body = CompiledExpr::binop(
        reify_ir::BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    // gradient(field) returns a Gradient-sourced field
    let gradient_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let gradient_result = eval_expr(&gradient_expr, &EvalContext::simple(&values));
    assert!(
        matches!(&gradient_result, Value::Field { .. }),
        "gradient of temperature/length field should return a Field, got {:?}",
        gradient_result
    );

    // Sample the gradient field at x=3.0[m]
    let point = Value::Scalar {
        si_value: 3.0,
        dimension: DimensionVector::LENGTH,
    };

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(gradient_result, grad_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // The derivative of f(x) = 2*x is 2.0
    let val = sample_result
        .as_f64()
        .unwrap_or_else(|| panic!("gradient sample should be numeric, got {:?}", sample_result));
    assert!(
        (val - 2.0).abs() < 1e-4,
        "gradient of 2*x should be ~2.0, got {}",
        val
    );
}

/// Gradient of a field whose lambda returns a non-numeric value: construction
/// succeeds.
///
/// Build a field whose lambda returns Value::String("not_a_number"). gradient()
/// construction succeeds because the field has valid domain/source/lambda. At
/// sampling time the debug guard in `compute_numerical_gradient_at_point` fires
/// on the unexpected `Type::String` codomain (see
/// `gradient_of_field_with_non_numeric_lambda_sampling_panics_in_debug` for the
/// debug-mode behaviour; the release-mode behaviour is tested in
/// `gradient_of_field_with_non_numeric_lambda_sampling_returns_undef`).
#[test]
fn gradient_of_field_with_non_numeric_lambda() {
    // gradient(field) succeeds at construction time — domain is scalar, source
    // is Analytical, lambda is a Lambda.
    let grad_expr = build_string_codomain_grad_expr();

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));
    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient construction should succeed (valid domain/source/lambda), got {:?}",
        grad_result
    );
}

/// Sampling a gradient field whose lambda returns a non-numeric value returns Undef
/// in release mode (where the debug guard is absent).
///
/// The debug-mode counterpart is
/// `gradient_of_field_with_non_numeric_lambda_sampling_panics_in_debug`.
///
/// NOTE: This test is gated on `#[cfg(not(debug_assertions))]` and is therefore
/// **excluded from `cargo test`** (which compiles with debug_assertions enabled).
/// It only runs under `cargo test --release`. The orchestrator (`orchestrator.yaml`)
/// always runs both a debug pass and a release pass, so CI coverage for this test
/// is preserved. The sibling debug-mode test is
/// `gradient_of_field_with_non_numeric_lambda_sampling_panics_in_debug`.
#[cfg(not(debug_assertions))]
#[test]
fn gradient_of_field_with_non_numeric_lambda_sampling_returns_undef() {
    let grad_expr = build_string_codomain_grad_expr();
    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    let point = Value::Real(1.0);
    let grad_field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::String),
    };
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::Real),
        ],
        Type::Real,
    );
    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        sample_result,
        Value::Undef,
        "sampling gradient of non-numeric lambda must return Undef in release mode"
    );
}

/// In debug mode, sampling a gradient field with a non-numeric (String) codomain
/// panics with the unexpected-codomain guard added to
/// `compute_numerical_gradient_at_point`.
///
/// `Type::String` is not a valid gradient codomain — the debug_assert in the
/// result_dim match fires before any numeric work begins.
///
/// NOTE: This test is gated on `#[cfg(debug_assertions)]` and is therefore
/// **excluded from `cargo test --release`**. It only runs under `cargo test`
/// (debug mode). The orchestrator (`orchestrator.yaml`) always runs both a debug
/// pass and a release pass, so CI coverage for this test is preserved. The sibling
/// release-mode test is
/// `gradient_of_field_with_non_numeric_lambda_sampling_returns_undef`.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "unexpected codomain_type")]
fn gradient_of_field_with_non_numeric_lambda_sampling_panics_in_debug() {
    let grad_expr = build_string_codomain_grad_expr();
    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    let point = Value::Real(1.0);
    let grad_field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::String),
    };
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::Real),
        ],
        Type::Real,
    );
    // In debug mode the result_dim debug_assert fires before any numeric work.
    let _sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
}

/// Characterization test: `make_xyz_sum_field` returns a correctly-structured
/// `(Value, Type)` pair for the given domain type.
///
/// Verifies that:
/// - The returned `Value` is a `Value::Field` with `source=Analytical` and
///   `codomain_type=Type::Real`.
/// - The returned `Type` is a `Type::Field` with `domain=Type::point3(Type::Real)`
///   and `codomain=Type::Real`.
#[test]
fn xyz_sum_field_helper_returns_expected_structure() {
    let domain_type = Type::point3(Type::Real);
    let (field_val, field_type) = make_xyz_sum_field(domain_type);

    // Check the Value side
    match &field_val {
        Value::Field {
            domain_type: d,
            codomain_type: c,
            source,
            ..
        } => {
            assert_eq!(
                d,
                &Type::point3(Type::Real),
                "domain_type must be point3(Real)"
            );
            assert_eq!(c, &Type::Real, "codomain_type must be Real");
            assert_eq!(
                source,
                &FieldSourceKind::Analytical,
                "source must be Analytical"
            );
        }
        other => panic!("expected Value::Field, got {:?}", other),
    }

    // Check the Type side
    match &field_type {
        Type::Field { domain, codomain } => {
            assert_eq!(
                domain.as_ref(),
                &Type::point3(Type::Real),
                "field_type domain must be point3(Real)"
            );
            assert_eq!(
                codomain.as_ref(),
                &Type::Real,
                "field_type codomain must be Real"
            );
        }
        other => panic!("expected Type::Field, got {:?}", other),
    }
}

// ── Step 2336: Kleene undef propagation regression tests ──────────────────────────

/// `sample(field, Undef)` returns Undef regardless of the lambda body.
///
/// Pins the strict-undef short-circuit in `eval_expr` at
/// `crates/reify-expr/src/lib.rs` (the `evaluated_args.iter().any(|v| v.is_undef())`
/// guard before dispatching `sample`). The lambda body `|x| x + 1.0` is
/// well-defined for any Real input; this test confirms that supplying an Undef
/// *argument* bypasses the body entirely and immediately yields Undef.
#[test]
fn sample_propagates_undef_point_argument() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x + 1.0  (well-defined for any Real input)
    let body = CompiledExpr::binop(
        reify_ir::BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::literal(Value::Real(1.0), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Real;
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type),
    };

    // sample(field, Undef) → Undef: strict-undef short-circuit fires before
    // the lambda body is evaluated.
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Undef, domain_type),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "sample(field, Undef) must short-circuit to Undef via the strict-undef \
         argument guard (lib.rs evaluated_args.iter().any(|v| v.is_undef()))"
    );
}

/// Division-by-zero inside the lambda body propagates to Undef through `sample`.
///
/// Pins the per-op Kleene rule for division: when a lambda body evaluates
/// `1.0 / 0.0`, `eval_binop` returns `Value::Undef`. That Undef then propagates
/// back through `apply_lambda` → `apply_lambda_with_point_unpacking` →
/// `eval_expr`'s `sample` dispatch arm, making `sample(field, 0.0)` return Undef.
///
/// References: `crates/reify-expr/src/lib.rs` (eval_binop Div arm, Kleene
/// per-op rule for real division-by-zero).
#[test]
fn sample_propagates_undef_from_lambda_body_division_by_zero() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 1.0 / x  (Undef when x == 0.0 due to division-by-zero)
    let body = CompiledExpr::binop(
        reify_ir::BinOp::Div,
        CompiledExpr::literal(Value::Real(1.0), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Real;
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type),
    };

    // sample(field, 0.0) → Undef: body evaluates 1.0 / 0.0 which yields Undef
    // via the per-op Kleene rule for real division-by-zero in eval_binop.
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(0.0), domain_type.clone()),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "sample(field, 0.0) where body is `1.0 / x` must return Undef: \
         division-by-zero in eval_binop yields Undef per the Kleene per-op rule, \
         and that Undef propagates back through the lambda invocation"
    );

    // Positive control: non-zero x gives a well-defined result.
    let field2 = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: Type::Real,
        source: FieldSourceKind::Analytical,
        lambda: {
            let x_id2 = ValueCellId::new("$lambda0.S", "x");
            let body2 = CompiledExpr::binop(
                reify_ir::BinOp::Div,
                CompiledExpr::literal(Value::Real(1.0), Type::Real),
                CompiledExpr::value_ref(x_id2.clone(), Type::Real),
                Type::Real,
            );
            Arc::new(make_value_lambda(
                vec![("x", x_id2)],
                body2,
                ValueMap::new(),
            ))
        },
    };
    let field2_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::Real),
    };
    let sample_nonzero = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field2, field2_type),
            CompiledExpr::literal(Value::Real(2.0), domain_type),
        ],
        Type::Real,
    );
    let nonzero_result = eval_expr(&sample_nonzero, &EvalContext::simple(&values));
    assert_eq!(
        nonzero_result,
        Value::Real(0.5),
        "sample(field, 2.0) where body is `1.0 / x` must return 0.5 (positive control)"
    );
}

/// Verifies that `make_xyz_sum_field` produces the correct structure when
/// called with `Type::vec3(Type::Real)` as the domain type.
///
/// This guards against a regression where the helper might accidentally
/// hardcode a domain type (e.g., always returning `point3`) regardless of
/// the argument passed.
#[test]
fn xyz_sum_field_helper_with_vec3_domain() {
    let domain_type = Type::vec3(Type::Real);
    let (field_val, field_type) = make_xyz_sum_field(domain_type);

    // Check the Value side
    match &field_val {
        Value::Field {
            domain_type: d,
            codomain_type: c,
            source,
            ..
        } => {
            assert_eq!(d, &Type::vec3(Type::Real), "domain_type must be vec3(Real)");
            assert_eq!(c, &Type::Real, "codomain_type must be Real");
            assert_eq!(
                source,
                &FieldSourceKind::Analytical,
                "source must be Analytical"
            );
        }
        other => panic!("expected Value::Field, got {:?}", other),
    }

    // Check the Type side
    match &field_type {
        Type::Field { domain, codomain } => {
            assert_eq!(
                domain.as_ref(),
                &Type::vec3(Type::Real),
                "field_type domain must be vec3(Real)"
            );
            assert_eq!(
                codomain.as_ref(),
                &Type::Real,
                "field_type codomain must be Real"
            );
        }
        other => panic!("expected Type::Field, got {:?}", other),
    }
}
