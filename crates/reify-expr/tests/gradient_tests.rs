//! Gradient edge-case tests.
//!
//! These tests encode expected edge-case behavior for gradient operations.
//! Currently gradient is a stub (always returns Undef), but these tests serve
//! as guardrails that must continue passing when gradient is fully implemented.

use reify_expr::{EvalContext, eval_expr};
use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ContentHash, FieldSourceKind, ResolvedFunction, Type,
    Value, ValueCellId, ValueMap,
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

/// Helper to build a Conditional (if-then-else) expression.
fn make_conditional(
    condition: CompiledExpr,
    then_branch: CompiledExpr,
    else_branch: CompiledExpr,
    result_type: Type,
) -> CompiledExpr {
    let hash = ContentHash::of(&[5])
        .combine(condition.content_hash)
        .combine(then_branch.content_hash)
        .combine(else_branch.content_hash);
    CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        },
        result_type,
        content_hash: hash,
    }
}

/// Partial Undef propagation: a lambda that returns Undef on one specific input.
///
/// Build a lambda that returns Undef when its input equals a specific perturbed
/// value (simulating perturbation along one axis yielding Undef) and returns a
/// valid Real otherwise. Create three sample(field, point) calls — one for each
/// of three different points. Assert that sampling at the Undef-triggering value
/// returns Undef while the other two return valid values.
///
/// This verifies the short-circuit behavior that gradient's numerical
/// differentiation will rely on: if perturbation along one axis yields Undef
/// from the field function, gradient must propagate that Undef rather than
/// computing garbage.
#[test]
fn partial_undef_propagation_lambda_returns_undef_on_one_axis() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| if x == 2.0 then Undef else x * x
    // This simulates a field function that is undefined at x=2.0
    // (e.g., a perturbation point that falls outside the field domain).
    let body = make_conditional(
        // condition: x == 2.0
        CompiledExpr::binop(
            BinOp::Eq,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::literal(Value::Real(2.0), Type::Real),
            Type::Bool,
        ),
        // then: Undef (perturbation along this axis fails)
        CompiledExpr::literal(Value::Undef, Type::Real),
        // else: x * x (normal evaluation)
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            Type::Real,
        ),
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
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    // Sample at three points: x=1.0, x=2.0 (Undef-trigger), x=3.0
    let points = [1.0, 2.0, 3.0];
    let expected = [
        Value::Real(1.0),  // 1.0 * 1.0
        Value::Undef,      // x == 2.0 triggers Undef
        Value::Real(9.0),  // 3.0 * 3.0
    ];

    for (point_val, expected_val) in points.iter().zip(expected.iter()) {
        let expr = make_function_call(
            "sample",
            vec![
                CompiledExpr::literal(field.clone(), field_type.clone()),
                CompiledExpr::literal(Value::Real(*point_val), Type::Real),
            ],
            Type::Real,
        );
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));
        assert_eq!(
            &result, expected_val,
            "sample(field, {}) should return {:?}, got {:?}",
            point_val, expected_val, result
        );
    }
}
