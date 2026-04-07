//! Field evaluation edge-case tests.
//!
//! Tests for `sample()` and `gradient()` behavioral contracts, including
//! edge cases like constant fields, nested gradients, dimensioned domains,
//! and non-numeric lambda outputs.

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
/// succeeds but sampling returns Undef.
///
/// Build a field whose lambda returns Value::String("not_a_number"). gradient()
/// construction succeeds because the field has valid domain/source/lambda. But
/// sampling the gradient field returns Undef because numerical differentiation
/// (f(x+h) - f(x-h)) / 2h requires numeric f values, and String cannot
/// participate in as_f64 extraction.
#[test]
fn gradient_of_field_with_non_numeric_lambda() {
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
        lambda: Box::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    // gradient(field) succeeds at construction time — domain is scalar, source
    // is Analytical, lambda is a Lambda.
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));
    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient construction should succeed (valid domain/source/lambda), got {:?}",
        grad_result
    );

    // Sampling the gradient field returns Undef because lambda returns String,
    // which fails as_f64 extraction in the perturbation loop.
    let point = Value::Real(1.0);
    let grad_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        sample_result,
        Value::Undef,
        "sampling gradient of non-numeric lambda must return Undef"
    );
}
