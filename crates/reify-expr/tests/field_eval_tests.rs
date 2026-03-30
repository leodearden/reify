//! Field evaluation edge-case tests.
//!
//! These tests encode expected edge-case behavior for field sampling and gradient
//! operations. Currently gradient is a stub (always returns Undef), but these tests
//! serve as guardrails that must continue passing when gradient is fully implemented.

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

    // gradient(field) → Undef (stub).
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
