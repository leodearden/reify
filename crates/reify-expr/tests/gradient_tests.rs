//! Gradient edge-case tests.
//!
//! These tests encode expected edge-case behavior for gradient operations.
//! Currently gradient is a stub (always returns Undef), but these tests serve
//! as guardrails that must continue passing when gradient is fully implemented.

use reify_expr::{EvalContext, eval_expr};
use reify_types::{
    CompiledExpr, CompiledExprKind, ContentHash, FieldSourceKind, ResolvedFunction, Type, Value,
    ValueCellId, ValueMap,
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

/// Gradient of a field whose domain has non-scalar quantity components returns Undef.
///
/// Build a Field with domain_type: Point3<Vec3<Length>> (Point3 whose components
/// are Vec3, not Scalar) and codomain_type: Length. Gradient requires the domain
/// to have scalar quantity components for meaningful partial derivatives.
#[test]
fn gradient_field_with_non_scalar_domain_quantity_returns_undef() {
    // Domain: Point3 whose quantity is Vec3<Length> instead of a scalar like Length
    let non_scalar_quantity = Type::vec3(Type::length());
    let domain_type = Type::point3(non_scalar_quantity);
    let codomain_type = Type::length();

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Box::new(Value::Undef), // lambda doesn't matter for this test
    };

    let expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(
            field,
            Type::Field {
                domain: Box::new(domain_type),
                codomain: Box::new(codomain_type),
            },
        )],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "gradient of field with non-scalar domain quantity must return Undef"
    );
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

/// Sampling a field with a wrong-size Tensor point returns Undef.
///
/// Build a 3D analytical field whose lambda expects 3 decomposed coordinate
/// parameters (x, y, z). Pass a 2-component Tensor as the point argument to
/// `sample`. The apply_lambda call receives 1 argument (the Tensor) for a
/// 3-param lambda, triggering arity mismatch → Undef. This tests the dimension
/// mismatch pathway that `compute_numerical_gradient` will depend on.
#[test]
fn gradient_wrong_size_tensor_point_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + y + z  (expects 3 decomposed coordinate args)
    let body = CompiledExpr::binop(
        reify_types::BinOp::Add,
        CompiledExpr::binop(
            reify_types::BinOp::Add,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            Type::Real,
        ),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::length());
    let codomain_type = Type::length();

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Box::new(lambda),
    };

    // Wrong-size point: 2 components instead of 3
    let wrong_size_point = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]);

    let expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(
                field,
                Type::Field {
                    domain: Box::new(domain_type),
                    codomain: Box::new(codomain_type),
                },
            ),
            CompiledExpr::literal(wrong_size_point, Type::vec3(Type::length())),
        ],
        Type::length(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    // sample passes [Tensor(2)] as 1 arg to a 3-param lambda → arity mismatch → Undef.
    // When gradient is implemented, it will decompose the point into individual
    // coordinates for perturbation. A wrong-size point means wrong number of
    // coordinates → dimension mismatch → Undef.
    assert_eq!(
        result,
        Value::Undef,
        "sample with wrong-size Tensor point must return Undef"
    );
}
