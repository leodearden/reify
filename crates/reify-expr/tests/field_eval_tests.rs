//! Field evaluation edge-case tests.
//!
//! This file contains two categories of tests:
//!
//! 1. **Durable: sample behavior tests** -- These encode permanent behavioral contracts
//!    for `sample()` that must always pass regardless of gradient implementation status.
//!
//! 2. **Transient: gradient stub tests** -- These assert that `gradient()` returns
//!    `Value::Undef` because gradient is currently a stub. These tests MUST be updated
//!    when gradient is fully implemented (task #630 and related work). They are NOT
//!    durable guardrails -- they encode current stub behavior.

use reify_expr::{EvalContext, eval_expr};
use reify_types::{
    CompiledExpr, CompiledExprKind, ContentHash, DimensionVector, FieldSourceKind,
    ResolvedFunction, Type, Value, ValueCellId, ValueMap,
};

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
        lambda: Box::new(Value::Undef),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    // sample(field, point) -> Undef because lambda is not a Lambda variant
    let point = Value::Point(vec![
        Value::Real(1.0),
        Value::Real(2.0),
        Value::Real(3.0),
    ]);
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
        reify_types::BinOp::Mul,
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
        lambda: Box::new(lambda),
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
        lambda: Box::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    // gradient(field) -> Undef (stub).
    // TODO: When gradient is implemented, sampling the gradient field at any
    // point should yield components all within TOLERANCE of zero.
    #[allow(unused)]
    const TOLERANCE: f64 = 1e-9;

    let expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "gradient of constant field must return Undef (stub; when implemented, \
         sampled gradient components should be within 1e-9 of zero)"
    );
}

/// Gradient of gradient returns Undef (nested differential operators not supported).
///
/// Build an analytical field, construct gradient(field) which returns Undef (stub),
/// then construct gradient(gradient(field)). The outer gradient receives Undef as
/// its argument due to strict Undef propagation at FunctionCall arg evaluation,
/// short-circuiting before even reaching the gradient stub logic.
#[test]
fn gradient_of_gradient_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x * x  (simple scalar field)
    let body = CompiledExpr::binop(
        reify_types::BinOp::Mul,
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
        lambda: Box::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    // Inner gradient: gradient(field) -> Undef (stub)
    let inner_gradient = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    let values = ValueMap::new();
    let inner_result = eval_expr(&inner_gradient, &EvalContext::simple(&values));
    assert_eq!(
        inner_result,
        Value::Undef,
        "inner gradient(field) must return Undef (stub)"
    );

    // Outer gradient: gradient(gradient(field)) -> Undef
    // The inner gradient evaluates to Undef, which triggers strict Undef
    // propagation at the outer FunctionCall's arg evaluation, short-circuiting
    // before even reaching the gradient handler.
    let outer_gradient = make_function_call(
        "gradient",
        vec![inner_gradient],
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
        lambda: Box::new(Value::Undef),
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

/// Gradient of temperature-over-length field returns Undef (stub).
///
/// Build a 1D field with domain=Scalar<Length>, codomain=Scalar<Temperature>,
/// lambda: |x| -> 2.0 * x. Call gradient(field) -> Undef (stub).
/// When gradient is implemented, the gradient of a Temperature(Length)
/// field should produce a field with codomain dimension Temperature/Length.
#[test]
fn gradient_temperature_over_length_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 2.0 * x  (linear temperature field over length domain)
    let body = CompiledExpr::binop(
        reify_types::BinOp::Mul,
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
        lambda: Box::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    // gradient(field) -> Undef (stub)
    // TODO: When gradient is implemented, the gradient of a Temperature(Length)
    // field should produce a field with codomain dimension Temperature/Length.
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
        "gradient of temperature/length field must return Undef (stub; when \
         implemented, gradient dimension should be Temperature/Length)"
    );
}

/// Gradient of a field whose lambda returns a non-numeric value returns Undef.
///
/// Build a field whose lambda returns Value::String("not_a_number"). Call
/// gradient(field) -> Undef (stub). When gradient is implemented with numerical
/// differentiation, non-numeric lambda output must propagate as Undef because
/// arithmetic perturbation (f(x+h) - f(x-h)) / 2h requires numeric values.
#[test]
fn gradient_of_field_with_non_numeric_lambda() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| "not_a_number"  (non-numeric return value)
    let body = CompiledExpr::literal(
        Value::String("not_a_number".to_string()),
        Type::String,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Real;
    let codomain_type = Type::String;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Box::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    // gradient(field) -> Undef (stub)
    // When gradient is implemented, numerical differentiation will attempt
    // (f(x+h) - f(x-h)) / 2h, which requires numeric f values. A String
    // return from the lambda cannot participate in subtraction/division,
    // so gradient must propagate Undef.
    let expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "gradient of field with non-numeric lambda must return Undef"
    );
}
