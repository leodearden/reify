//! Gradient tests.
//!
//! Tests for numerical gradient via central differences on analytical fields.
//! Includes edge-case guardrails and positive tests for gradient computation.

use std::sync::Arc;

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, TAG_CONDITIONAL, Value, ValueMap};

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
        lambda: Arc::new(Value::Undef), // lambda doesn't matter for this test
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

/// Assert that a `Value::Vector` has components matching `expected` within `tol`.
///
/// Panics with a descriptive message if:
/// - `result` is not a `Value::Vector`
/// - the vector length differs from `expected.len()`
/// - any component is non-numeric
/// - any component differs from the expected value by more than `tol`
///
/// Note: components are compared via `Value::as_f64()`, which accepts
/// `Value::Real`, `Value::Int`, and `Value::Scalar` (ignoring dimension).
/// A `Scalar[kg/m]` component is treated identically to a dimensionless
/// `Real`. For dimensioned assertions (e.g. verifying a specific
/// `DimensionVector`), use an inline `match Value::Vector(components)`
/// pattern instead.
fn assert_gradient_vector(result: &Value, expected: &[f64], tol: f64, label: &str) {
    match result {
        Value::Vector(components) => {
            assert_eq!(
                components.len(),
                expected.len(),
                "{label}: gradient vector has {} components, expected {}",
                components.len(),
                expected.len()
            );
            for (i, (comp, &exp)) in components.iter().zip(expected.iter()).enumerate() {
                let val = comp.as_f64().unwrap_or_else(|| {
                    panic!("{label}: component {} should be numeric, got {:?}", i, comp)
                });
                assert!(
                    (val - exp).abs() < tol,
                    "{label}: component {} = {} differs from expected {} by {} (tolerance {})",
                    i,
                    val,
                    exp,
                    (val - exp).abs(),
                    tol
                );
            }
        }
        Value::Undef => panic!("{label}: gradient returned Undef, expected Vector"),
        _ => panic!("{label}: expected Value::Vector, got {:?}", result),
    }
}

/// Characterization test: `assert_gradient_vector` accepts a matching Value::Vector.
///
/// Calls the helper with a Value::Vector([Real(1.0), Real(2.0), Real(3.0)]),
/// expected &[1.0, 2.0, 3.0], tol=1e-4.  This confirms the helper works for the
/// exact inputs that `gradient_decomposed_n3_dimensionless` produces before we
/// rely on it as a drop-in replacement.
#[test]
fn test_assert_gradient_vector_accepts_matching_vector() {
    let result = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
    assert_gradient_vector(&result, &[1.0, 2.0, 3.0], 1e-4, "matching vector");
}

/// Characterization test: `assert_gradient_vector` panics on a non-Vector value.
///
/// Calls the helper with Value::Real(1.0); the helper's `_ =>` arm must panic.
/// This mirrors the `_ => panic!(...)` branch present in the inline code we
/// are about to replace, ensuring the helper covers that error path.
/// (Uses Real rather than Undef so that the Undef branch added later does not
/// intercept this test and break catch-all arm coverage.)
#[test]
#[should_panic(expected = "expected Value::Vector")]
fn test_assert_gradient_vector_panics_on_non_vector() {
    assert_gradient_vector(&Value::Real(1.0), &[1.0, 2.0, 3.0], 1e-4, "non-vector");
}

/// Characterization test: `assert_gradient_vector` accepts a within-tolerance Value::Vector.
///
/// Calls the helper with Value::Vector([Real(1.00005), Real(2.0), Real(3.0)]) and
/// expected &[1.0, 2.0, 3.0] with tol=1e-4.  The first component differs by 5e-5,
/// which is strictly less than 1e-4, exercising the `(val - exp).abs() < tol` branch
/// that the exact-match happy-path test left trivially true.
#[test]
fn test_assert_gradient_vector_accepts_within_tolerance() {
    let result = Value::Vector(vec![
        Value::Real(1.00005),
        Value::Real(2.0),
        Value::Real(3.0),
    ]);
    assert_gradient_vector(&result, &[1.0, 2.0, 3.0], 1e-4, "within tolerance");
}

/// Characterization test: `assert_gradient_vector` panics when a component exceeds tolerance.
///
/// Calls the helper with Value::Vector([Real(1.001), Real(2.0), Real(3.0)]) and
/// expected &[1.0, 2.0, 3.0] with tol=1e-4.  The first component differs by 1e-3,
/// which exceeds 1e-4, exercising the failure side of the `(val - exp).abs() < tol`
/// tolerance assertion.  This complements the within-tolerance happy-path test by
/// covering the failure path of the same branch.
#[test]
#[should_panic(expected = "differs from expected")]
fn test_assert_gradient_vector_panics_on_out_of_tolerance() {
    let result = Value::Vector(vec![Value::Real(1.001), Value::Real(2.0), Value::Real(3.0)]);
    assert_gradient_vector(&result, &[1.0, 2.0, 3.0], 1e-4, "out of tolerance");
}

/// Characterization test: `assert_gradient_vector` panics when vector length mismatches.
///
/// Constructs a 2-element Value::Vector but passes a 3-element expected slice.
/// This exercises the `assert_eq!(components.len(), expected.len(), ...)` branch.
/// The panic message begins with "gradient vector has" verbatim.
#[test]
#[should_panic(expected = "gradient vector has")]
fn test_assert_gradient_vector_panics_on_length_mismatch() {
    let result = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0)]);
    assert_gradient_vector(&result, &[1.0, 2.0, 3.0], 1e-4, "length mismatch");
}

/// Characterization test: `assert_gradient_vector` panics when a component is non-numeric.
///
/// Constructs a Value::Vector whose first element is Value::Undef.  `Value::Undef.as_f64()`
/// returns None, which triggers the `unwrap_or_else` panic inside the component-decoding
/// branch of assert_gradient_vector.  The panic message contains "should be numeric" verbatim.
#[test]
#[should_panic(expected = "should be numeric")]
fn test_assert_gradient_vector_panics_on_non_numeric_component() {
    let result = Value::Vector(vec![Value::Undef, Value::Real(2.0), Value::Real(3.0)]);
    assert_gradient_vector(&result, &[1.0, 2.0, 3.0], 1e-4, "non-numeric component");
}

/// Characterization test: `assert_gradient_vector` panics on `Value::Undef` with a
/// distinctive, Undef-specific message.
///
/// Once the helper has a dedicated `Value::Undef` arm, callers that receive Undef
/// will see `"gradient returned Undef, expected Vector"` rather than the generic
/// `"expected Value::Vector, got Undef"`.  This test confirms that distinctive
/// message so grepping for "Undef" in failing-test output is diagnostic.
#[test]
#[should_panic(expected = "gradient returned Undef, expected Vector")]
fn test_assert_gradient_vector_panics_on_undef_with_distinctive_message() {
    assert_gradient_vector(&Value::Undef, &[1.0, 2.0, 3.0], 1e-4, "undef input");
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
        reify_ir::BinOp::Add,
        CompiledExpr::binop(
            reify_ir::BinOp::Add,
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
        lambda: Arc::new(lambda),
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
    let hash = ContentHash::of(&[TAG_CONDITIONAL])
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
        lambda: Arc::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(codomain_type),
    };

    // Sample at three points: x=1.0, x=2.0 (Undef-trigger), x=3.0
    let points = [1.0, 2.0, 3.0];
    let expected = [
        Value::Real(1.0), // 1.0 * 1.0
        Value::Undef,     // x == 2.0 triggers Undef
        Value::Real(9.0), // 3.0 * 3.0
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

/// Gradient of a 1D linear field f(x)=3*x should produce a field that,
/// when sampled at x=1.0, returns approximately 3.0.
#[test]
fn gradient_1d_linear_field() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 3 * x
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(3.0), Type::Real),
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    // gradient should return a Field, not Undef
    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of 1D linear field should return a Field, got {:?}",
        grad_result
    );

    // Now sample the gradient field at x=1.0
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // The derivative of 3*x is 3.0 everywhere
    let result_f64 = sample_result
        .as_f64()
        .expect("gradient sample should return a numeric value");
    assert!(
        (result_f64 - 3.0).abs() < 1e-4,
        "gradient of 3*x at x=1.0 should be ~3.0, got {}",
        result_f64
    );
}

/// Gradient of a 3D scalar field f(x,y,z) = x^2 + 2*y + 3*z at point (1,2,3)
/// should return Vector3 approximately (2.0, 2.0, 3.0).
///
/// Partial derivatives: df/dx = 2x, df/dy = 2, df/dz = 3.
/// At (1,2,3): (2*1, 2, 3) = (2.0, 2.0, 3.0).
#[test]
fn gradient_3d_scalar_field() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + 2*y + 3*z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            // x*x
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                Type::Real,
            ),
            // 2*y
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::literal(Value::Real(2.0), Type::Real),
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                Type::Real,
            ),
            Type::Real,
        ),
        // 3*z
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            Type::Real,
        ),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of 3D field should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient field at Point3(1.0, 2.0, 3.0)
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Expected: Vector3(2.0, 2.0, 3.0)
    assert_gradient_vector(
        &sample_result,
        &[2.0, 2.0, 3.0],
        1e-4,
        "gradient of x*x + 2y + 3z at (1,2,3)",
    );
}

/// Gradient of a Field with Undef lambda returns Undef.
///
/// A field might have Undef lambda (e.g., imported but not yet resolved).
/// Gradient cannot evaluate such a field, so it must return Undef.
#[test]
fn gradient_field_with_undef_lambda_returns_undef() {
    let domain_type = Type::Real;
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(Value::Undef),
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
        "gradient of field with Undef lambda must return Undef"
    );
}

/// Gradient of a Sampled field returns Undef.
///
/// Sampled fields don't have a callable lambda — they are data-driven.
/// Gradient requires perturbation at arbitrary points, which is only
/// possible with Analytical or Composed sources.
#[test]
fn gradient_sampled_field_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Real;
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(lambda),
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
        "gradient of Sampled field must return Undef"
    );
}

/// Gradient called with wrong number of arguments returns Undef.
///
/// gradient() expects exactly 1 argument. With 0 or 2 args, it falls
/// through to stdlib dispatch, which returns Undef for unknown functions.
#[test]
fn gradient_wrong_arg_count_returns_undef() {
    // 0 args
    let expr_0 = make_function_call("gradient", vec![], Type::Real);
    let values = ValueMap::new();
    let result_0 = eval_expr(&expr_0, &EvalContext::simple(&values));
    assert_eq!(
        result_0,
        Value::Undef,
        "gradient with 0 args must return Undef"
    );

    // 2 args (pass two Reals)
    let expr_2 = make_function_call(
        "gradient",
        vec![
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
            CompiledExpr::literal(Value::Real(2.0), Type::Real),
        ],
        Type::Real,
    );
    let result_2 = eval_expr(&expr_2, &EvalContext::simple(&values));
    assert_eq!(
        result_2,
        Value::Undef,
        "gradient with 2 args must return Undef"
    );
}

/// Undef propagation during gradient perturbation.
///
/// Build a 2D field f(x, y) = x*x when y >= 0, Undef when y < 0.
/// Gradient at (1.0, 0.0): perturbation along y produces y-h < 0,
/// which triggers Undef from the lambda. Gradient must propagate
/// this Undef rather than returning a partial result.
#[test]
fn gradient_undef_propagation_during_perturbation() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");

    // Lambda: |x, y| if y < 0 then Undef else x*x
    let body = make_conditional(
        // condition: y < 0
        CompiledExpr::binop(
            BinOp::Lt,
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::literal(Value::Real(0.0), Type::Real),
            Type::Bool,
        ),
        // then: Undef (y is negative → outside domain)
        CompiledExpr::literal(Value::Undef, Type::Real),
        // else: x*x (normal evaluation)
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            Type::Real,
        ),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id), ("y", y_id)], body, ValueMap::new());

    let domain_type = Type::Point {
        n: 2,
        quantity: Box::new(Type::Real),
    };
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::Vector {
                n: 2,
                quantity: Box::new(Type::Real),
            }),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample gradient at (1.0, 0.0) — y=0 means perturbation y-h < 0 → lambda returns Undef
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(0.0)]);
    let grad_field_type = Type::Field {
        domain: Box::new(Type::Point {
            n: 2,
            quantity: Box::new(Type::Real),
        }),
        codomain: Box::new(Type::Vector {
            n: 2,
            quantity: Box::new(Type::Real),
        }),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(
                point,
                Type::Point {
                    n: 2,
                    quantity: Box::new(Type::Real),
                },
            ),
        ],
        Type::Vector {
            n: 2,
            quantity: Box::new(Type::Real),
        },
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient at boundary where perturbation hits Undef must return Undef, got {:?}",
        sample_result
    );
}

/// Gradient of a 1D field with dimensionless Real domain (not Point type).
///
/// f: Field<Real, Real> with lambda |x| x*x*x.
/// Gradient at x=2.0 should be ≈ 12.0 (3*x^2 at x=2).
/// This tests that gradient handles non-Point scalar domains (1D case).
#[test]
fn gradient_1d_real_domain_cubic() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x * x * x
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            Type::Real,
        ),
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of 1D cubic field should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient field at x=2.0
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(Value::Real(2.0), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // The derivative of x^3 is 3*x^2. At x=2: 3*4 = 12.0
    let result_f64 = sample_result
        .as_f64()
        .expect("gradient sample should return a numeric value");
    assert!(
        (result_f64 - 12.0).abs() < 1e-4,
        "gradient of x^3 at x=2.0 should be ~12.0, got {}",
        result_f64
    );
}

/// Gradient of a dimensioned 3D field with domain Point3<Scalar[m]> and codomain Scalar[kg].
///
/// f(x,y,z) = 2*x + 3*y + 4*z (all in kg, with x,y,z in metres).
/// Gradient components should be Scalar values with dimension kg/m:
/// df/dx = 2 kg/m, df/dy = 3 kg/m, df/dz = 4 kg/m.
#[test]
fn gradient_dimensioned_field() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let dim_m = DimensionVector::LENGTH;
    let dim_kg = DimensionVector::MASS;
    let scalar_m = Type::Scalar { dimension: dim_m };
    let scalar_kg = Type::Scalar { dimension: dim_kg };

    // Lambda: |x, y, z| 2*x + 3*y + 4*z
    // In the lambda body, x/y/z are dimensionless Reals (SI values in meters),
    // and the coefficients encode the dimensional relationship.
    // The result should be a Scalar[kg] value.
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            // 2*x → Scalar[kg]
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::literal(
                    Value::Scalar {
                        si_value: 2.0,
                        dimension: dim_kg.div(&dim_m),
                    },
                    Type::Scalar {
                        dimension: dim_kg.div(&dim_m),
                    },
                ),
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                scalar_kg.clone(),
            ),
            // 3*y → Scalar[kg]
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::literal(
                    Value::Scalar {
                        si_value: 3.0,
                        dimension: dim_kg.div(&dim_m),
                    },
                    Type::Scalar {
                        dimension: dim_kg.div(&dim_m),
                    },
                ),
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                scalar_kg.clone(),
            ),
            scalar_kg.clone(),
        ),
        // 4*z → Scalar[kg]
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::literal(
                Value::Scalar {
                    si_value: 4.0,
                    dimension: dim_kg.div(&dim_m),
                },
                Type::Scalar {
                    dimension: dim_kg.div(&dim_m),
                },
            ),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            scalar_kg.clone(),
        ),
        scalar_kg.clone(),
    );

    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(scalar_m.clone());
    let codomain_type = scalar_kg.clone();

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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(scalar_kg.clone())),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample at Point3(1.0m, 2.0m, 3.0m)
    let point = Value::Point(vec![
        Value::Scalar {
            si_value: 1.0,
            dimension: dim_m,
        },
        Value::Scalar {
            si_value: 2.0,
            dimension: dim_m,
        },
        Value::Scalar {
            si_value: 3.0,
            dimension: dim_m,
        },
    ]);

    let grad_field_type = Type::Field {
        domain: Box::new(Type::point3(scalar_m)),
        codomain: Box::new(Type::vec3(scalar_kg)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Scalar { dimension: dim_m })),
        ],
        Type::vec3(Type::Scalar {
            dimension: dim_kg.div(&dim_m),
        }),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Expected: Vector3 with components ~2.0, ~3.0, ~4.0 in dimension kg/m
    let expected_dim = dim_kg.div(&dim_m);
    // Inline match retained: this site asserts dimensioned Value::Scalar components
    // (with a specific DimensionVector), which assert_gradient_vector cannot check
    // because it uses as_f64(), which discards dimension information.
    match &sample_result {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "gradient should have 3 components");
            let expected_vals = [2.0, 3.0, 4.0];
            for (i, (comp, &exp)) in components.iter().zip(expected_vals.iter()).enumerate() {
                match comp {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (si_value - exp).abs() < 1e-4,
                            "gradient component {} should be ~{}, got {}",
                            i,
                            exp,
                            si_value
                        );
                        assert_eq!(
                            *dimension, expected_dim,
                            "gradient component {} should have dimension kg/m",
                            i
                        );
                    }
                    _ => panic!(
                        "gradient component {} should be a Scalar, got {:?}",
                        i, comp
                    ),
                }
            }
        }
        _ => panic!(
            "gradient sample should return a Vector, got {:?}",
            sample_result
        ),
    }
}

/// Gradient at the origin (all coordinates = 0).
///
/// f(x,y,z) = x^2 + y^2 + z^2
/// Gradient: (2x, 2y, 2z). At (0,0,0): (0, 0, 0).
/// Tests the step-size formula h = 1e-6 * max(|coord|, 1e-3) where
/// |coord|=0 triggers the 1e-3 floor.
#[test]
fn gradient_at_origin() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + y*y + z*z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            // x*x
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                Type::Real,
            ),
            // y*y
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                Type::Real,
            ),
            Type::Real,
        ),
        // z*z
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            Type::Real,
        ),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample gradient at origin (0, 0, 0)
    let point = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Expected: Vector3(0.0, 0.0, 0.0) — gradient of x^2+y^2+z^2 at origin is zero
    assert_gradient_vector(
        &sample_result,
        &[0.0, 0.0, 0.0],
        1e-4,
        "gradient of x²+y²+z² at origin",
    );
}

/// Gradient perturbation with dimensioned Scalar lambda args.
///
/// Build a 3D field with domain Point3<Scalar[m]> and codomain Scalar[m²].
/// Lambda: |x,y,z| x*x + y*y + z*z where x/y/z receive Value::Scalar{dimension: m}.
/// With correct Scalar[m] args, Scalar[m]*Scalar[m] = Scalar[m²] via eval_mul.
/// With Real args (the bug), Real*Real = Real (dimensionless).
///
/// Gradient at (1,2,3): df/dx=2*1=2, df/dy=2*2=4, df/dz=2*3=6.
/// Components should be Scalar with dimension m²/m = m (LENGTH).
#[test]
fn gradient_dimensioned_scalar_lambda_args() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let dim_m = DimensionVector::LENGTH;
    let dim_m2 = dim_m.mul(&dim_m);
    let scalar_m = Type::Scalar { dimension: dim_m };
    let scalar_m2 = Type::Scalar { dimension: dim_m2 };

    // Lambda: |x, y, z| x*x + y*y + z*z
    // x, y, z are expected to be Scalar[m], so x*x = Scalar[m²].
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            // x*x → Scalar[m²]
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(x_id.clone(), scalar_m.clone()),
                CompiledExpr::value_ref(x_id.clone(), scalar_m.clone()),
                scalar_m2.clone(),
            ),
            // y*y → Scalar[m²]
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(y_id.clone(), scalar_m.clone()),
                CompiledExpr::value_ref(y_id.clone(), scalar_m.clone()),
                scalar_m2.clone(),
            ),
            scalar_m2.clone(),
        ),
        // z*z → Scalar[m²]
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(z_id.clone(), scalar_m.clone()),
            CompiledExpr::value_ref(z_id.clone(), scalar_m.clone()),
            scalar_m2.clone(),
        ),
        scalar_m2.clone(),
    );

    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(scalar_m.clone());
    let codomain_type = scalar_m2.clone();

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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(scalar_m2.clone())),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample at Point3(1.0m, 2.0m, 3.0m)
    let point = Value::Point(vec![
        Value::Scalar {
            si_value: 1.0,
            dimension: dim_m,
        },
        Value::Scalar {
            si_value: 2.0,
            dimension: dim_m,
        },
        Value::Scalar {
            si_value: 3.0,
            dimension: dim_m,
        },
    ]);

    let grad_field_type = Type::Field {
        domain: Box::new(Type::point3(scalar_m.clone())),
        codomain: Box::new(Type::vec3(scalar_m2)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(scalar_m)),
        ],
        Type::vec3(Type::Scalar { dimension: dim_m }),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Expected: Vector3 with components ≈2.0, ≈4.0, ≈6.0 in dimension m (=m²/m)
    // df/dx = 2x = 2*1 = 2, df/dy = 2y = 2*2 = 4, df/dz = 2z = 2*3 = 6
    let expected_dim = dim_m; // m²/m = m
    // Inline match retained: this site asserts dimensioned Value::Scalar components
    // (with a specific DimensionVector), which assert_gradient_vector cannot check
    // because it uses as_f64(), which discards dimension information.
    match &sample_result {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "gradient should have 3 components");
            let expected_vals = [2.0, 4.0, 6.0];
            for (i, (comp, &exp)) in components.iter().zip(expected_vals.iter()).enumerate() {
                match comp {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (si_value - exp).abs() < 1e-4,
                            "gradient component {} should be ~{}, got {}",
                            i,
                            exp,
                            si_value
                        );
                        assert_eq!(
                            *dimension, expected_dim,
                            "gradient component {} should have dimension LENGTH (m), got {:?}",
                            i, dimension
                        );
                    }
                    _ => panic!(
                        "gradient component {} should be a Scalar with dimension, got {:?}",
                        i, comp
                    ),
                }
            }
        }
        _ => panic!(
            "gradient sample should return a Vector, got {:?}",
            sample_result
        ),
    }
}

/// Gradient of a 1D dimensioned field: f: Field<Scalar[m], Scalar[m²]> with lambda |x| x*x.
///
/// At x=3.0m, the derivative of x² is 2x = 6.0, and the dimension should be m²/m = m.
/// This tests that the 1D scalar branch of domain_dim handling works correctly.
#[test]
fn gradient_1d_dimensioned_field() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    let dim_m = DimensionVector::LENGTH;
    let dim_m2 = dim_m.mul(&dim_m);
    let scalar_m = Type::Scalar { dimension: dim_m };
    let scalar_m2 = Type::Scalar { dimension: dim_m2 };

    // Lambda: |x| x * x
    // x is Scalar[m], so x*x = Scalar[m²] via eval_mul's Scalar*Scalar arm.
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), scalar_m.clone()),
        CompiledExpr::value_ref(x_id.clone(), scalar_m.clone()),
        scalar_m2.clone(),
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = scalar_m.clone();
    let codomain_type = scalar_m2.clone();

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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(scalar_m.clone()),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of 1D dimensioned field should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient field at x = 3.0m
    let grad_field_type = Type::Field {
        domain: Box::new(scalar_m.clone()),
        codomain: Box::new(scalar_m.clone()),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(
                Value::Scalar {
                    si_value: 3.0,
                    dimension: dim_m,
                },
                scalar_m,
            ),
        ],
        Type::Scalar { dimension: dim_m },
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // The derivative of x² is 2x. At x=3.0: 2*3 = 6.0.
    // Dimension should be m²/m = m.
    match &sample_result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 6.0).abs() < 1e-4,
                "gradient of x² at x=3.0 should be ~6.0, got {}",
                si_value
            );
            assert_eq!(
                *dimension, dim_m,
                "gradient dimension should be LENGTH (m²/m = m), got {:?}",
                dimension
            );
        }
        _ => panic!(
            "gradient sample should return a Scalar with dimension, got {:?}",
            sample_result
        ),
    }
}

/// Gradient of a 3D field with a 1-param lambda: |p| 5.0 (constant function).
///
/// The actual language convention uses `source = analytical { |p| ... }` where
/// the lambda has a single Point parameter. The gradient code must detect this
/// calling convention and wrap perturbed coordinates in a Point value before
/// calling apply_lambda.
///
/// Without the fix, apply_lambda receives 3 individual args but lambda has 1 param,
/// causing arity mismatch → Undef for all perturbations → gradient returns Undef.
#[test]
fn gradient_1param_constant_field() {
    let p_id = ValueCellId::new("$lambda0.S", "p");

    // Lambda: |p| 5.0 (constant function, ignores the Point parameter)
    let body = CompiledExpr::literal(Value::Real(5.0), Type::Real);
    let lambda = make_value_lambda(vec![("p", p_id)], body, ValueMap::new());

    let domain_type = Type::point3(Type::Real);
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient field at Point3(1.0, 2.0, 3.0)
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Gradient of a constant function should be approximately Vector3(0,0,0), NOT Undef.
    assert_gradient_vector(
        &sample_result,
        &[0.0, 0.0, 0.0],
        1e-4,
        "gradient of constant 5.0 at (1,2,3)",
    );
}

/// Gradient of a 3D field with a 1-param lambda: |p| magnitude(p).
///
/// magnitude(Point3(3,4,0)) = 5.0. The gradient of |p| at (3,4,0) is:
///   d|p|/dx_i = x_i / |p|
/// So gradient = (3/5, 4/5, 0/5) = (0.6, 0.8, 0.0).
///
/// This tests that the 1-param calling convention fix correctly wraps perturbed
/// coordinates in a Value::Point so that magnitude (via tensor_components_f64)
/// can extract components.
#[test]
fn gradient_1param_magnitude_field() {
    let p_id = ValueCellId::new("$lambda0.S", "p");

    // Lambda: |p| magnitude(p)
    let body = make_function_call(
        "magnitude",
        vec![CompiledExpr::value_ref(
            p_id.clone(),
            Type::point3(Type::Real),
        )],
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("p", p_id)], body, ValueMap::new());

    let domain_type = Type::point3(Type::Real);
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient field at Point3(3.0, 4.0, 0.0)
    let point = Value::Point(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Expected: Vector3(0.6, 0.8, 0.0) = (x/|p|, y/|p|, z/|p|) where |p|=5
    assert_gradient_vector(
        &sample_result,
        &[0.6, 0.8, 0.0],
        1e-4,
        "gradient of magnitude(p) at (3,4,0)",
    );
}

/// Gradient of a gradient field returns Undef.
///
/// Build a 3D analytical field f(x,y,z) = x*x + y*y + z*z with a 3-param lambda.
/// Call gradient(f) to produce grad_f (a Gradient-sourced field). Assert grad_f is
/// a Value::Field (first gradient succeeds). Call gradient(grad_f) and assert the
/// result is Value::Undef — not a silently broken Field.
///
/// Gradient-sourced fields are not valid inputs to compute_gradient because their
/// lambda slot contains a Value::Field rather than a Value::Lambda, making numerical
/// differentiation impossible without recursive field sampling. The test ensures the
/// limitation is caught at construction time (source whitelist rejection) rather than
/// silently at sampling time.
#[test]
fn gradient_of_gradient_field_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + y*y + z*z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            // x*x
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                Type::Real,
            ),
            // y*y
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                Type::Real,
            ),
            Type::Real,
        ),
        // z*z
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            Type::Real,
        ),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
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

    // First gradient: should succeed and produce a Gradient-sourced field
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "first gradient should return a Field, got {:?}",
        grad_result
    );

    // Second gradient (gradient of gradient): should return Undef
    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let grad_grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(grad_result, grad_field_type)],
        Type::Real, // result type doesn't matter — we expect Undef
    );

    let grad_grad_result = eval_expr(&grad_grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_grad_result, Value::Undef),
        "gradient(gradient(f)) should return Undef, got {:?}",
        grad_grad_result
    );
}

// ── NaN/Inf propagation tests ──────────────────────────────────────────

/// NaN in a Point coordinate must cause gradient sampling to return Undef.
///
/// Build a 3D analytical field f(x,y,z) = x + y + z with a 3-param lambda.
/// Construct a sample point with NaN in the y coordinate:
///   Point3(1.0, NaN, 3.0).
/// Without the is_finite guard, coords[1] = NaN, h = 1e-6 * NaN = NaN,
/// perturbed coords become NaN, and the gradient silently produces NaN.
#[test]
fn gradient_nan_in_point_coord_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + y + z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
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

    let domain_type = Type::point3(Type::Real);
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient at a point with NaN in y-coordinate
    let nan_point = Value::Point(vec![
        Value::Real(1.0),
        Value::Real(f64::NAN),
        Value::Real(3.0),
    ]);

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(nan_point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at a point with NaN coordinate must return Undef, got {:?}",
        sample_result
    );
}

/// Inf in a 1D Real coordinate must cause gradient sampling to return Undef.
///
/// Build a 1D field f(x) = x*x with a 1-param lambda.
/// Sample the gradient at Value::Real(f64::INFINITY).
/// Without the guard, coord = Inf, h = 1e-6 * Inf = Inf,
/// perturbed values are Inf, and the gradient produces NaN (Inf - Inf) / (2*Inf).
#[test]
fn gradient_inf_in_scalar_coord_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x * x
    let body = CompiledExpr::binop(
        BinOp::Mul,
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient at Infinity
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(Value::Real(f64::INFINITY), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at Infinity must return Undef, got {:?}",
        sample_result
    );
}

/// NaN in a dimensioned Scalar coordinate must cause gradient sampling to return Undef.
///
/// Build a 1D dimensioned field f(x) = x*x with domain Scalar[m].
/// Sample the gradient at Value::Scalar{si_value: NaN, dimension: LENGTH}.
/// This tests the Value::Scalar{si_value, ..} extraction path.
#[test]
fn gradient_nan_in_dimensioned_scalar_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x * x (Scalar[m] * Scalar[m] = Scalar[m^2])
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::length()),
        CompiledExpr::value_ref(x_id.clone(), Type::length()),
        Type::Scalar {
            dimension: DimensionVector::LENGTH.mul(&DimensionVector::LENGTH),
        },
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::length();
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH.mul(&DimensionVector::LENGTH),
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient at NaN with dimension LENGTH
    let nan_scalar = Value::Scalar {
        si_value: f64::NAN,
        dimension: DimensionVector::LENGTH,
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(nan_scalar, domain_type),
        ],
        codomain_type,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at Scalar(NaN, LENGTH) must return Undef, got {:?}",
        sample_result
    );
}

/// Lambda producing NaN must cause gradient to return Undef.
///
/// Build a 1D field whose lambda always returns Value::Real(NaN).
/// The gradient evaluates f(x+h) = NaN and f(x-h) = NaN. Without
/// the is_finite guard on fp/fm, as_f64() returns Some(NaN), the
/// match passes, and deriv = (NaN - NaN)/(2h) = NaN, yielding
/// Value::Real(NaN) instead of Undef.
///
/// Note: We use a literal NaN rather than 0.0/0.0 because the
/// evaluator's div-by-zero handler returns Undef (not NaN), which
/// is already caught by the existing None guard.
#[test]
fn gradient_lambda_produces_nan_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| NaN (literal NaN, bypasses div-by-zero → Undef path)
    let body = CompiledExpr::literal(Value::Real(f64::NAN), Type::Real);
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient at x=1.0 (finite input, NaN output from lambda)
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient of NaN-producing lambda must return Undef, got {:?}",
        sample_result
    );
}

/// Lambda producing Inf must cause gradient to return Undef.
///
/// Build a 1D field whose lambda always returns Value::Real(+Inf).
/// The gradient evaluates f(x+h) = Inf and f(x-h) = Inf. Without
/// the is_finite guard, as_f64() returns Some(Inf), the match passes,
/// and deriv = (Inf - Inf)/(2h) = NaN, yielding Value::Real(NaN).
///
/// Note: We use a literal Inf rather than 1.0/0.0 because the
/// evaluator's div-by-zero handler returns Undef (not Inf).
#[test]
fn gradient_lambda_produces_inf_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| +Inf (literal Inf, bypasses div-by-zero → Undef path)
    let body = CompiledExpr::literal(Value::Real(f64::INFINITY), Type::Real);
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient at x=2.0 (finite input, Inf output from lambda)
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(Value::Real(2.0), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient of Inf-producing lambda must return Undef, got {:?}",
        sample_result
    );
}

/// Derivative overflow (f64::MAX - (-f64::MAX))/(2h) must return Undef.
///
/// Build a 1D field with a conditional lambda:
///   |x| if x > 0.0 then f64::MAX else -f64::MAX
/// At x=0.0, perturbation evaluates f(0+h) = MAX and f(0-h) = -MAX.
/// deriv = (MAX - (-MAX))/(2h) = 2*MAX/(2h) → Inf (overflow).
/// Without the deriv is_finite guard, this silently produces Value::Real(Inf).
#[test]
fn gradient_deriv_overflow_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| if x > 0.0 then f64::MAX else -f64::MAX
    let body = make_conditional(
        // condition: x > 0.0
        CompiledExpr::binop(
            BinOp::Gt,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::literal(Value::Real(0.0), Type::Real),
            Type::Bool,
        ),
        // then: f64::MAX
        CompiledExpr::literal(Value::Real(f64::MAX), Type::Real),
        // else: -f64::MAX
        CompiledExpr::literal(Value::Real(-f64::MAX), Type::Real),
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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample the gradient at x=0.0, where perturbation crosses the
    // discontinuity and produces deriv overflow (MAX - (-MAX)) / (2h) → Inf
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(Value::Real(0.0), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient with deriv overflow must return Undef, got {:?}",
        sample_result
    );
}

/// Gradient of Field<Point3<Scalar[m]>, Scalar[kg]> must have codomain_type
/// = Vector { n: 3, quantity: Length[kg/m] }, not Vector { n: 3, quantity: Length[kg] }.
///
/// The runtime values are correct (dimension division happens in
/// compute_numerical_gradient_at_point), but the type metadata on the gradient
/// field itself currently uses codomain_type.clone() = Scalar[kg] instead of
/// computing the R/Q dimension.
#[test]
fn gradient_3d_codomain_type_is_rq() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let dim_m = DimensionVector::LENGTH;
    let dim_kg = DimensionVector::MASS;
    let scalar_m = Type::Scalar { dimension: dim_m };
    let scalar_kg = Type::Scalar { dimension: dim_kg };

    // Lambda: |x, y, z| 2*x + 3*y + 4*z  (produces Scalar[kg])
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::literal(
                    Value::Scalar {
                        si_value: 2.0,
                        dimension: dim_kg.div(&dim_m),
                    },
                    Type::Scalar {
                        dimension: dim_kg.div(&dim_m),
                    },
                ),
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                scalar_kg.clone(),
            ),
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::literal(
                    Value::Scalar {
                        si_value: 3.0,
                        dimension: dim_kg.div(&dim_m),
                    },
                    Type::Scalar {
                        dimension: dim_kg.div(&dim_m),
                    },
                ),
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                scalar_kg.clone(),
            ),
            scalar_kg.clone(),
        ),
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::literal(
                Value::Scalar {
                    si_value: 4.0,
                    dimension: dim_kg.div(&dim_m),
                },
                Type::Scalar {
                    dimension: dim_kg.div(&dim_m),
                },
            ),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            scalar_kg.clone(),
        ),
        scalar_kg.clone(),
    );

    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(scalar_m.clone());
    let codomain_type = scalar_kg.clone();

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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(scalar_kg.clone())),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    // Extract the codomain_type from the gradient field and verify it's kg/m, not kg.
    let expected_grad_quantity = Type::Scalar {
        dimension: dim_kg.div(&dim_m),
    };
    let expected_codomain = Type::Vector {
        n: 3,
        quantity: Box::new(expected_grad_quantity),
    };

    match &grad_result {
        Value::Field { codomain_type, .. } => {
            assert_eq!(
                *codomain_type, expected_codomain,
                "gradient codomain_type should be Vector3<Scalar[kg/m]>, got {:?}",
                codomain_type
            );
        }
        other => panic!("gradient should return a Field, got {:?}", other),
    }
}

/// Gradient of 1D Field<Scalar[m], Scalar[m²]> must have codomain_type = Scalar[m]
/// (= m²/m), not Scalar[m²].
///
/// The 1D branch in compute_gradient previously returned codomain_type.clone(),
/// which preserves m² instead of dividing by the domain dimension.
#[test]
fn gradient_1d_codomain_type_is_rq() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    let dim_m = DimensionVector::LENGTH;
    let dim_m2 = dim_m.mul(&dim_m);
    let scalar_m = Type::Scalar { dimension: dim_m };
    let scalar_m2 = Type::Scalar { dimension: dim_m2 };

    // Lambda: |x| x * x  (produces Scalar[m²])
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), scalar_m.clone()),
        CompiledExpr::value_ref(x_id.clone(), scalar_m.clone()),
        scalar_m2.clone(),
    );

    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = scalar_m.clone();
    let codomain_type = scalar_m2.clone();

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

    // Call gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(scalar_m2.clone()),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    // codomain_type should be Scalar[m] (= m²/m), not Scalar[m²].
    let expected_codomain = Type::Scalar { dimension: dim_m };

    match &grad_result {
        Value::Field { codomain_type, .. } => {
            assert_eq!(
                *codomain_type, expected_codomain,
                "1D gradient codomain_type should be Scalar[m] (m²/m), got {:?}",
                codomain_type
            );
        }
        other => panic!("gradient should return a Field, got {:?}", other),
    }
}

/// Regression: gradient of Field<Point3<Real>, Real> must have codomain_type
/// = Vector { n: 3, quantity: Real }, unchanged by the R/Q fix.
///
/// When both domain and codomain are dimensionless, no dimension division
/// should occur.
#[test]
fn gradient_dimensionless_codomain_type_unchanged() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + y + z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
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

    let domain_type = Type::point3(Type::Real);
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

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    let expected_codomain = Type::Vector {
        n: 3,
        quantity: Box::new(Type::Real),
    };

    match &grad_result {
        Value::Field { codomain_type, .. } => {
            assert_eq!(
                *codomain_type, expected_codomain,
                "dimensionless gradient codomain_type should be Vector3<Real>, got {:?}",
                codomain_type
            );
        }
        other => panic!("gradient should return a Field, got {:?}", other),
    }
}

/// Regression: gradient of Field<Point3<Scalar[m]>, Real> must have codomain_type
/// = Vector { n: 3, quantity: Real }.
///
/// When codomain is dimensionless Real, no R/Q division should occur even if
/// domain is dimensioned — mirrors the runtime logic which only divides when
/// result_dim != DIMENSIONLESS.
#[test]
fn gradient_mixed_dimensionless_codomain_type() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + y + z  (produces Real even though domain is Scalar[m])
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
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

    let domain_type = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
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

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    let expected_codomain = Type::Vector {
        n: 3,
        quantity: Box::new(Type::Real),
    };

    match &grad_result {
        Value::Field { codomain_type, .. } => {
            assert_eq!(
                *codomain_type, expected_codomain,
                "mixed-dimensionless gradient codomain_type should be Vector3<Real>, got {:?}",
                codomain_type
            );
        }
        other => panic!("gradient should return a Field, got {:?}", other),
    }
}

/// Regression: gradient of Field<Point3<Scalar[m]>, Scalar[DIMENSIONLESS]> must have
/// codomain_type = Vector { n: 3, quantity: Real }, NOT Vector { n: 3, quantity: Length[DIMENSIONLESS] }.
///
/// The codomain is explicitly typed as `Scalar[DIMENSIONLESS]` (not `Type::Real`), but the
/// runtime produces `Value::Real` for dimensionless gradient components, so the type-level
/// code must normalize `Scalar[DIMENSIONLESS]` → `Type::Real` in the fallback arm of
/// `gradient_quantity`.
#[test]
fn gradient_explicit_dimensionless_scalar_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + y + z  (produces Real even though domain is Scalar[m])
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
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

    let domain_type = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    // Explicitly Scalar[DIMENSIONLESS] — NOT Type::Real — to exercise the bug.
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
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

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    // The gradient of a dimensionless-scalar-codomain field must have codomain Vector3<Real>,
    // matching what the runtime produces (Value::Real components, not Value::Scalar[DIMENSIONLESS]).
    let expected_codomain = Type::Vector {
        n: 3,
        quantity: Box::new(Type::Real),
    };

    match &grad_result {
        Value::Field { codomain_type, .. } => {
            assert_eq!(
                *codomain_type, expected_codomain,
                "explicit-Scalar[DIMENSIONLESS] gradient codomain_type should be Vector3<Real>, got {:?}",
                codomain_type
            );
        }
        other => panic!("gradient should return a Field, got {:?}", other),
    }
}

// ── Step-1: Imported field ─────────────────────────────────────────────

/// Gradient of an Imported field returns Undef.
///
/// Mirrors gradient_sampled_field_returns_undef. Imported fields don't have a
/// callable lambda. compute_gradient rejects non-Analytical/Composed sources via
/// its FieldSourceKind whitelist, so gradient(ImportedField) → Undef immediately.
#[test]
fn gradient_imported_field_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Real;
    let codomain_type = Type::Real;

    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Imported,
        lambda: Arc::new(lambda),
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
        "gradient of Imported field must return Undef"
    );
}

// ── Step-2: Composed field ────────────────────────────────────────────

/// Gradient of a Composed field returns a gradient Field and produces correct values.
///
/// Composed is whitelisted in compute_gradient's FieldSourceKind check. This test verifies:
/// 1. gradient(ComposedField) produces Value::Field { source: Gradient, .. }
///    rather than Undef, so that the Composed path doesn't silently regress.
/// 2. Sampling the gradient field at x=1.0 yields ≈ 2.0 for lambda |x| 2*x,
///    turning the structural check into a real numerical regression guard.
#[test]
fn gradient_composed_field_returns_field() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 2 * x
    let body = CompiledExpr::binop(
        reify_ir::BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let field = Value::Field {
        domain_type: Type::Real,
        codomain_type: Type::Real,
        source: FieldSourceKind::Composed,
        lambda: Arc::new(lambda),
    };

    let expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(
            field,
            Type::Field {
                domain: Box::new(Type::Real),
                codomain: Box::new(Type::Real),
            },
        )],
        Type::Field {
            domain: Box::new(Type::Real),
            codomain: Box::new(Type::Real),
        },
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        matches!(
            &result,
            Value::Field {
                source: FieldSourceKind::Gradient,
                ..
            }
        ),
        "gradient of Composed field must return a gradient Field, got {:?}",
        result
    );

    // Sample the gradient field at x=1.0.
    // The derivative of 2*x is 2.0 everywhere — this is a real regression guard.
    let grad_field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::Real),
    };
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(result, grad_field_type),
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
        ],
        Type::Real,
    );
    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    let val = sample_result
        .as_f64()
        .expect("gradient sample of composed field should return a numeric value");
    assert!(
        (val - 2.0).abs() < 1e-4,
        "gradient of 2*x at x=1.0 should be ~2.0, got {}",
        val
    );
}

// ── Steps 3-5: 1-param lambda helper and non-trivial gradient tests ───

/// Shared scaffolding for building a 3D gradient field.
///
/// Accepts a `body_builder` closure that receives the lambda parameter's `ValueCellId`
/// and returns the body `CompiledExpr`. Constructs the full `Value::Field`, wraps it in a
/// gradient expression, evaluates it, asserts the result is a `Value::Field`, and returns
/// `(grad_result, domain_type, Type::vec3(Type::Real))`.
///
/// # Parameters
/// - `body_builder`: closure `FnOnce(ValueCellId) -> CompiledExpr` — builds the lambda body
///   from the parameter id. The closure receives a *clone* of `p_id`; the original is retained
///   for `make_value_lambda`.
/// - `label`: included in the assertion message for diagnostic specificity.
fn make_gradient_field(
    body_builder: impl FnOnce(ValueCellId) -> CompiledExpr,
    label: &str,
) -> (Value, Type, Type) {
    let p_id = ValueCellId::new("$lambda0.S", "p");

    let body = body_builder(p_id.clone());
    let lambda = make_value_lambda(vec![("p", p_id)], body, ValueMap::new());

    let domain_type = Type::point3(Type::Real);
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
        "{label}: gradient should return a Field, got {:?}",
        grad_result
    );

    (grad_result, domain_type, Type::vec3(Type::Real))
}

/// Build a gradient field for `|p: Point3<Real>| dot(p, [1,2,3])`.
///
/// Returns `(grad_field, domain_type, grad_codomain_type)` where:
/// - `grad_field` is the evaluated `Value::Field { source: Gradient, .. }`
/// - `domain_type` is `Type::point3(Type::Real)`
/// - `grad_codomain_type` is `Type::vec3(Type::Real)`
///
/// Used by four tests that share this setup:
/// `gradient_3d_field_single_point_param`,
/// `gradient_sample_with_nan_point_returns_undef`,
/// `gradient_sample_with_inf_point_returns_undef`,
/// `gradient_tensor_single_point_param_returns_undef`.
fn make_3d_dot_product_gradient_field() -> (Value, Type, Type) {
    // Delegates shared scaffolding to make_gradient_field; only the body differs.
    let weight_point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
    make_gradient_field(
        |p_id| {
            make_function_call(
                "dot",
                vec![
                    CompiledExpr::value_ref(p_id, Type::point3(Type::Real)),
                    CompiledExpr::literal(weight_point, Type::point3(Type::Real)),
                ],
                Type::Real,
            )
        },
        "make_3d_dot_product_gradient_field",
    )
}

/// Build a gradient field for `|p: Point3<Real>| dot(p, p)` = x²+y²+z².
///
/// Returns `(grad_field, domain_type, grad_codomain_type)` where:
/// - `grad_field` is the evaluated `Value::Field { source: Gradient, .. }`
/// - `domain_type` is `Type::point3(Type::Real)`
/// - `grad_codomain_type` is `Type::vec3(Type::Real)`
///
/// The gradient of dot(p,p) is 2p = (2x, 2y, 2z), which depends on the
/// coordinates. Used by `gradient_single_point_param_quadratic` and
/// `gradient_single_point_param_irrational_coords`.
fn make_3d_quadratic_gradient_field() -> (Value, Type, Type) {
    // Delegates shared scaffolding to make_gradient_field; only the body differs.
    make_gradient_field(
        |p_id| {
            make_function_call(
                "dot",
                vec![
                    CompiledExpr::value_ref(p_id.clone(), Type::point3(Type::Real)),
                    CompiledExpr::value_ref(p_id, Type::point3(Type::Real)),
                ],
                Type::Real,
            )
        },
        "make_3d_quadratic_gradient_field",
    )
}

/// Characterization test: `make_gradient_field` returns a triple matching `make_3d_dot_product_gradient_field`.
///
/// Verifies that the shared helper produces a `Value::Field` as the first element,
/// `Type::point3(Type::Real)` as the domain type, and `Type::vec3(Type::Real)` as
/// the gradient codomain type — identical to what the existing dot-product factory returns.
#[test]
fn test_make_gradient_field_produces_field() {
    let (grad_result, domain_type, grad_codomain_type) = make_gradient_field(
        |p_id| {
            make_function_call(
                "dot",
                vec![
                    CompiledExpr::value_ref(p_id, Type::point3(Type::Real)),
                    CompiledExpr::literal(
                        Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]),
                        Type::point3(Type::Real),
                    ),
                ],
                Type::Real,
            )
        },
        "test_make_gradient_field_produces_field",
    );

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "make_gradient_field: expected Value::Field, got {:?}",
        grad_result
    );
    assert_eq!(
        domain_type,
        Type::point3(Type::Real),
        "make_gradient_field: domain type should be Point3<Real>"
    );
    assert_eq!(
        grad_codomain_type,
        Type::vec3(Type::Real),
        "make_gradient_field: gradient codomain should be Vec3<Real>"
    );
}

/// Gradient of a 3D field with a 1-param lambda: |p| dot(p, [1,2,3]).
///
/// dot(p, [1,2,3]) = x + 2y + 3z, so its gradient is the constant vector
/// (1.0, 2.0, 3.0). Exercises single_point_param=true with a non-trivial
/// (and verifiably-correct) gradient.
#[test]
fn gradient_3d_field_single_point_param() {
    let (grad_result, domain_type, grad_codomain_type) = make_3d_dot_product_gradient_field();
    let values = ValueMap::new();

    // Sample at Point3(5.0, 7.0, 11.0) — linear field so gradient is constant
    let point = Value::Point(vec![Value::Real(5.0), Value::Real(7.0), Value::Real(11.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_gradient_vector(
        &sample_result,
        &[1.0, 2.0, 3.0],
        1e-3,
        "gradient of dot(p,[1,2,3]) at (5,7,11)",
    );
}

/// Gradient of a 3D field with a 1-param lambda: |p| dot(p, p) at integer coordinates.
///
/// dot(p, p) = x² + y² + z², so its gradient is 2p = (2x, 2y, 2z). The
/// coordinates (2, 3, 5) are exactly representable in IEEE 754, so there is no
/// FP-restore error; this test exercises the *analytical correctness* of the
/// gradient computation on a coordinate-dependent quadratic function. This is a
/// correctness test, not an FP-restore test; see
/// `gradient_single_point_param_irrational_coords` for the FP-restore coverage.
///
/// Central difference is mathematically exact for quadratic polynomials — the
/// O(h²) truncation error vanishes — leaving only FP roundoff. Tolerance 1e-9
/// is therefore well-justified and could even be tighter.
#[test]
fn gradient_single_point_param_quadratic() {
    let (grad_result, domain_type, grad_codomain_type) = make_3d_quadratic_gradient_field();
    let values = ValueMap::new();

    // Sample at Point3(2.0, 3.0, 5.0); expected gradient = (4.0, 6.0, 10.0)
    let point = Value::Point(vec![Value::Real(2.0), Value::Real(3.0), Value::Real(5.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_gradient_vector(
        &sample_result,
        &[4.0, 6.0, 10.0],
        1e-9,
        "gradient of dot(p,p) at (2,3,5)",
    );
}

/// Gradient of a 3D quadratic field `|p| dot(p,p)` sampled at the origin.
///
/// grad(x²+y²+z²) = (2x, 2y, 2z); at (0,0,0) this is the zero vector (0,0,0).
/// Exercises the zero-vector edge case: the central-difference perturbation must
/// not divide by zero or produce NaN when all coordinates are zero.
///
/// Central difference on a quadratic is mathematically exact (O(h²) truncation
/// vanishes), leaving only FP roundoff. At the origin, f(h) − f(−h) = h²− h² = 0
/// exactly in IEEE 754, so the result is exact zero. Tolerance 1e-9 is conservative.
#[test]
fn gradient_quadratic_at_origin() {
    let (grad_result, domain_type, grad_codomain_type) = make_3d_quadratic_gradient_field();
    let values = ValueMap::new();

    // Sample at the origin (0, 0, 0); expected gradient = (0.0, 0.0, 0.0)
    let point = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_gradient_vector(
        &sample_result,
        &[0.0, 0.0, 0.0],
        1e-9,
        "gradient of dot(p,p) at origin (0,0,0)",
    );
}

/// Gradient of a 3D field with a 1-param lambda: |p| dot(p, p) at irrational coordinates.
///
/// dot(p, p) = x² + y² + z² with gradient 2p = (2x, 2y, 2z). Unlike the old
/// linear dot(p, [1,2,3]) version (whose gradient is the constant (1,2,3)), this
/// function's gradient is coordinate-dependent: any restore error in one axis would
/// corrupt its gradient component. Exercises the single_point_param path at
/// IEEE-754-unrepresentable coordinates.
///
/// Tolerance 1e-8: central diff of a quadratic is exact to FP roundoff; the
/// irrational coordinates introduce small representation errors, giving worst-case
/// ε·|f|/2h ≈ 1.8e-9; 1e-8 provides 5× margin.
#[test]
fn gradient_single_point_param_irrational_coords() {
    use std::f64::consts::PI;

    let (grad_result, domain_type, grad_codomain_type) = make_3d_quadratic_gradient_field();
    let values = ValueMap::new();

    let cx = 1.0_f64 / 3.0;
    let cy = PI / 7.0;
    let cz = 2.0_f64.sqrt();

    // Sample at Point3(1/3, π/7, √2); expected gradient = (2/3, 2π/7, 2√2)
    let point = Value::Point(vec![Value::Real(cx), Value::Real(cy), Value::Real(cz)]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_gradient_vector(
        &sample_result,
        &[2.0 * cx, 2.0 * cy, 2.0 * cz],
        1e-8,
        "gradient of dot(p,p) at (1/3, π/7, √2)",
    );
}

/// Regression test for the `work_point` take/recover idiom across
/// all 3 axes, sampled at two distinct points.
///
/// The single_point_param path uses a pre-allocated inner Vec (`work_point`)
/// that is transferred to `work_args` via `std::mem::take` and then recovered
/// by popping the `Value::Point` back out.
/// This pattern runs for both f_plus and f_minus on every axis — 6 take/recover
/// cycles per sample call on a 3D field.
///
/// **Invariant documented by this test:** after every recovery cycle, the inner
/// Vec must have exactly n=3 elements. A broken recovery (wrong variant, missing
/// pop, or ownership leak) would either panic, produce garbage gradient values,
/// or silently corrupt subsequent axes.
///
/// Sampling at two different points ensures the gradient field can be re-used
/// and that no state leaks between calls: each `sample` call dispatches to a
/// fresh invocation of `compute_numerical_gradient_at_point`, so this also
/// validates that the field itself is correctly structured after the first sample.
///
/// Field: |p| dot(p, p) = x² + y² + z²; gradient = 2p = (2x, 2y, 2z).
///
/// Tolerance 1e-8: central difference truncation error vanishes for quadratics,
/// leaving only FP rounding. At larger coordinates the intermediate sums
/// (e.g., x² + y² + z² ≈ 195 at (5,7,11)) amplify cancellation error by ~195/20h;
/// 1e-8 gives comfortable margin (theoretical worst-case ~6e-9 at (5,7,11)).
#[test]
fn gradient_single_point_param_recovery_across_axes() {
    let (grad_result, domain_type, grad_codomain_type) = make_3d_quadratic_gradient_field();
    let values = ValueMap::new();

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(grad_codomain_type.clone()),
    };

    // --- First sample: Point3(1.0, 2.0, 3.0) → gradient = (2.0, 4.0, 6.0) ---
    let point1 = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let sample_expr1 = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result.clone(), grad_field_type.clone()),
            CompiledExpr::literal(point1, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let result1 = eval_expr(&sample_expr1, &EvalContext::simple(&values));
    assert_gradient_vector(
        &result1,
        &[2.0, 4.0, 6.0],
        1e-8,
        "gradient of dot(p,p) at (1,2,3): work_point recovery must preserve n=3 inner Vec",
    );

    // --- Second sample: Point3(5.0, 7.0, 11.0) → gradient = (10.0, 14.0, 22.0) ---
    // A distinct point confirms the gradient field is reusable and no state from
    // the first sample leaks into the second invocation.
    let point2 = Value::Point(vec![Value::Real(5.0), Value::Real(7.0), Value::Real(11.0)]);

    let sample_expr2 = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point2, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let result2 = eval_expr(&sample_expr2, &EvalContext::simple(&values));
    assert_gradient_vector(
        &result2,
        &[10.0, 14.0, 22.0],
        1e-8,
        "gradient of dot(p,p) at (5,7,11): work_point recovery must preserve n=3 inner Vec",
    );
}

/// NaN in a Point coordinate causes gradient sampling to return Undef
/// via the single-point-param path.
///
/// Build a 3D field with a 1-param lambda |p| dot(p, [1,2,3]).
/// Sample the gradient at Point3(1.0, NaN, 3.0). The is_finite guard in
/// compute_numerical_gradient_at_point must catch NaN before perturbing.
#[test]
fn gradient_sample_with_nan_point_returns_undef() {
    let (grad_result, domain_type, grad_codomain_type) = make_3d_dot_product_gradient_field();
    let values = ValueMap::new();

    // Sample at Point3(1.0, NaN, 3.0)
    let nan_point = Value::Point(vec![
        Value::Real(1.0),
        Value::Real(f64::NAN),
        Value::Real(3.0),
    ]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(nan_point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at Point3 with NaN coordinate must return Undef (single-point-param path)"
    );
}

/// Inf in a Point coordinate causes gradient sampling to return Undef
/// via the single-point-param path.
///
/// Build a 3D field with a 1-param lambda |p| dot(p, [1,2,3]).
/// Sample the gradient at Point3(1.0, Inf, 3.0). The is_finite guard
/// must catch Inf before perturbing (existing test only covers 1D Real).
#[test]
fn gradient_sample_with_inf_point_returns_undef() {
    let (grad_result, domain_type, grad_codomain_type) = make_3d_dot_product_gradient_field();
    let values = ValueMap::new();

    // Sample at Point3(1.0, Inf, 3.0)
    let inf_point = Value::Point(vec![
        Value::Real(1.0),
        Value::Real(f64::INFINITY),
        Value::Real(3.0),
    ]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(inf_point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at Point3 with Inf coordinate must return Undef (single-point-param path)"
    );
}

// ── Step-6: Tensor point rejection ────────────────────────────────────

/// Sampling a gradient field with a Value::Tensor point returns Undef.
///
/// A Tensor is rank-r (nested Vec<Value>), not a flat coordinate list.
/// Treating Tensor the same as Point/Vector extracts wrong coords and computes
/// a meaningless Jacobian of flattened elements instead of a gradient.
///
/// Pre-fix: Tensor is matched alongside Point/Vector in
/// compute_numerical_gradient_at_point's point-decomposition match, coords are
/// extracted, gradient succeeds → test FAILS.
/// Post-fix: Tensor falls through to `_ => Undef` → test passes.
#[test]
fn gradient_tensor_point_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + y + z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
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

    let domain_type = Type::point3(Type::Real);
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

    // Call gradient(field)
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
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Sample with a Tensor instead of a Point — must return Undef.
    // The type annotation is intentionally point3(Real) to match the field's domain,
    // so the Value variant (Tensor vs Point) is the sole distinguishing factor.
    // Undef is expected from the value-kind dispatch, not a type error.
    let tensor_point = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(tensor_point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at a Tensor point must return Undef (Tensor is not a coordinate list)"
    );
}

/// Sampling a gradient field with a Value::Tensor point returns Undef — single_point_param path.
///
/// Complements `gradient_tensor_point_returns_undef` (which exercises the decomposed
/// 3-param lambda, i.e. single_point_param=false). This test uses the 1-param lambda
/// `|p| dot(p, [1,2,3])` built by `make_3d_dot_product_gradient_field()`, which exercises
/// the single_point_param=true code path. The sample point is a Value::Tensor, so the
/// value-kind dispatch must return Undef before any coordinate extraction occurs.
#[test]
fn gradient_tensor_single_point_param_returns_undef() {
    let (grad_result, domain_type, grad_codomain_type) = make_3d_dot_product_gradient_field();
    let values = ValueMap::new();

    // Sample with a Tensor instead of a Point — must return Undef.
    // The type annotation is point3(Real) (matching the domain) so the Value variant is
    // the sole distinguishing factor; Undef comes from value-kind dispatch, not a type error.
    let tensor_point = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(tensor_point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at a Tensor point must return Undef (single_point_param=true path)"
    );
}

// ── Step-7: Decomposed calling convention ─────────────────────────────

/// Helper to build a gradient field for a 3D decomposed-param lambda |x,y,z| x + 2*y + 3*z.
///
/// Returns:
/// - `grad_result`: the `Value::Field` returned by `eval_expr(gradient(...))`.
/// - `domain_type`: `Type::point3(Type::Real)`
/// - `grad_codomain_type`: `Type::vec3(Type::Real)`
///
/// Used by tests that share this decomposed-lambda setup:
/// `gradient_decomposed_n3_dimensionless`,
/// `gradient_decomposed_nan_returns_undef`,
/// `gradient_decomposed_inf_returns_undef`,
/// `gradient_decomposed_neg_inf_returns_undef`.
fn make_decomposed_n3_gradient_field() -> (Value, Type, Type) {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + 2*y + 3*z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            // x
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            // 2*y
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::literal(Value::Real(2.0), Type::Real),
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                Type::Real,
            ),
            Type::Real,
        ),
        // 3*z
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            Type::Real,
        ),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
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

    // Call gradient(field)
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
        "make_decomposed_n3_gradient_field: gradient should return a Field, got {:?}",
        grad_result
    );

    (grad_result, domain_type, Type::vec3(Type::Real))
}

/// Gradient of a 3D field with decomposed params |x,y,z| x + 2*y + 3*z.
///
/// Uses the decomposed calling convention (single_point_param=false, params.len()==3==n)
/// with work_coords reuse. Samples at Point3(5,7,11); since the function is linear
/// the gradient is constant: [1.0, 2.0, 3.0].
///
/// Complements gradient_3d_field_single_point_param (which uses single_point_param=true).
#[test]
fn gradient_decomposed_n3_dimensionless() {
    let (grad_result, domain_type, grad_codomain_type) = make_decomposed_n3_gradient_field();
    let values = ValueMap::new();

    // Sample at Point3(5.0, 7.0, 11.0) — linear function so gradient is constant
    let point = Value::Point(vec![Value::Real(5.0), Value::Real(7.0), Value::Real(11.0)]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_gradient_vector(
        &sample_result,
        &[1.0, 2.0, 3.0],
        1e-4,
        "gradient of x+2y+3z at (5,7,11), decomposed path",
    );
}

/// Regression test for the decomposed (multi-param) path at irrational coordinates.
///
/// Lambda: |x, y, z| x + 2*y + 3*z.  Gradient is the constant vector (1, 2, 3).
/// Sample at Point3(1/3, π/7, √2) — coordinates not exactly representable in IEEE 754.
///
/// The exact-restore (`work_coords[i] = coord_i`) in compute_numerical_gradient_at_point affects both the
/// single_point_param and decomposed code paths.  The companion test
/// `gradient_single_point_param_irrational_coords` covers single_point_param=true;
/// this test covers single_point_param=false at the same irrational inputs.
///
/// Regression smoke test for the allocation refactor and decomposed path at
/// irrational inputs.  The tolerance is 1e-8 (5× margin over the theoretical
/// worst-case ~1.8e-9 from ε·|f|/2h).  Note: a linear function produces correct
/// gradients regardless of restore strategy, so this test primarily guards against
/// gross regressions in the decomposed-path refactor rather than FP-restore drift.
#[test]
fn gradient_decomposed_n3_irrational_coords() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + 2*y + 3*z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            // x
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            // 2*y
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::literal(Value::Real(2.0), Type::Real),
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                Type::Real,
            ),
            Type::Real,
        ),
        // 3*z
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            Type::Real,
        ),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
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

    // Call gradient(field)
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
        "gradient of 3D decomposed field should return a Field, got {:?}",
        grad_result
    );

    // Sample at Point3(1/3, π/7, √2) — not exactly representable in IEEE 754
    let x = 1.0_f64 / 3.0;
    let y = std::f64::consts::PI / 7.0;
    let z = 2.0_f64.sqrt();
    let point = Value::Point(vec![Value::Real(x), Value::Real(y), Value::Real(z)]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_gradient_vector(
        &sample_result,
        &[1.0, 2.0, 3.0],
        1e-8,
        "gradient of x+2y+3z at irrational coords, decomposed path",
    );
}

/// Gradient of a 1D field with heterogeneous dimensions: domain=Scalar[LENGTH],
/// codomain=Scalar[MASS], lambda |x| 2*x (coefficient carries MASS/LENGTH).
///
/// When the domain is dimensioned, make_arg passes Scalar{LENGTH} to the lambda.
/// Eval_mul(Scalar{MASS/LENGTH}, Scalar{LENGTH}) = Scalar{MASS}, matching the
/// declared codomain. The gradient dimension is codomain/domain = MASS/LENGTH.
///
/// At x=1.0m, the derivative of 2*x is 2.0, and the gradient dimension is MASS/LENGTH.
/// Uses heterogeneous dimensions (LENGTH domain, MASS codomain) to verify the
/// codomain_dim/domain_dim quotient handles non-self-referential dimension pairs.
/// Complements gradient_1d_dimensioned_field (LENGTH→LENGTH²) and gradient_dimensioned_field
/// (3D, LENGTH→MASS).
#[test]
fn gradient_codomain_type_with_dimensioned_domain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    let dim_length = DimensionVector::LENGTH;
    let dim_mass = DimensionVector::MASS;
    let dim_mass_per_length = dim_mass.div(&dim_length);

    let scalar_length = Type::Scalar {
        dimension: dim_length,
    };
    let scalar_mass = Type::Scalar {
        dimension: dim_mass,
    };
    let scalar_mass_per_length = Type::Scalar {
        dimension: dim_mass_per_length,
    };

    // Lambda: |x| Scalar{2.0, MASS/LENGTH} * x
    // make_arg passes Scalar{LENGTH} to the lambda (domain is dimensioned).
    // Scalar{MASS/LENGTH} * Scalar{LENGTH} = Scalar{MASS} via eval_mul.
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.0,
                dimension: dim_mass_per_length,
            },
            scalar_mass_per_length.clone(),
        ),
        CompiledExpr::value_ref(x_id.clone(), scalar_length.clone()),
        scalar_mass.clone(),
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = scalar_length.clone();
    let codomain_type = scalar_mass;

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

    // Gradient codomain: MASS/LENGTH (codomain_dim / domain_dim)
    let grad_codomain_type = scalar_mass_per_length.clone();

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(grad_codomain_type.clone()),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    match &grad_result {
        Value::Field {
            codomain_type: ct, ..
        } => {
            assert_eq!(
                ct, &scalar_mass_per_length,
                "gradient codomain_type should be Scalar{{MASS/LENGTH}}, got {:?}",
                ct
            );
        }
        _ => panic!("gradient should return a Field, got {:?}", grad_result),
    }

    // Sample the gradient field at x = 1.0m
    let grad_field_type = Type::Field {
        domain: Box::new(scalar_length.clone()),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(
                Value::Scalar {
                    si_value: 1.0,
                    dimension: dim_length,
                },
                scalar_length,
            ),
        ],
        scalar_mass_per_length.clone(),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Derivative of 2*x is 2.0. Gradient dimension: MASS/LENGTH.
    match &sample_result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 2.0).abs() < 1e-4,
                "gradient of 2*x at x=1.0m should be ~2.0, got {}",
                si_value
            );
            assert_eq!(
                *dimension, dim_mass_per_length,
                "gradient dimension should be MASS/LENGTH, got {:?}",
                dimension
            );
        }
        _ => panic!(
            "gradient sample should return Scalar with MASS/LENGTH dimension, got {:?}",
            sample_result
        ),
    }
}

/// Companion to `gradient_codomain_type_with_dimensioned_domain` using a non-linear lambda.
///
/// Non-linear coefficient k=Scalar{2.0, MASS/LENGTH²}, f(x) = k·x² has dimension MASS.
/// df/dx = 2·k·x has dimension MASS/LENGTH and non-trivial magnitude 12.0 at x=3.0m.
/// Exercises the numerical-sensitivity / FP-restore path that the linear sibling skips,
/// and verifies the structural codomain_type assertion on the gradient field.
#[test]
fn gradient_codomain_type_nonlinear_dimensioned_domain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    let dim_length = DimensionVector::LENGTH;
    let dim_mass = DimensionVector::MASS;
    let dim_length_sq = dim_length.mul(&dim_length);
    let dim_mass_per_length = dim_mass.div(&dim_length);
    let dim_mass_per_length_sq = dim_mass.div(&dim_length_sq);

    let scalar_length = Type::Scalar {
        dimension: dim_length,
    };
    let scalar_length_sq = Type::Scalar {
        dimension: dim_length_sq,
    };
    let scalar_mass = Type::Scalar {
        dimension: dim_mass,
    };
    let scalar_mass_per_length = Type::Scalar {
        dimension: dim_mass_per_length,
    };
    let scalar_mass_per_length_sq = Type::Scalar {
        dimension: dim_mass_per_length_sq,
    };

    // Lambda: |x| k * (x * x), k = Scalar{2.0, MASS/LENGTH²}
    // x is Scalar[LENGTH], x*x = Scalar[LENGTH²], k*(x*x) = Scalar[MASS].
    let x_x = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), scalar_length.clone()),
        CompiledExpr::value_ref(x_id.clone(), scalar_length.clone()),
        scalar_length_sq,
    );
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.0,
                dimension: dim_mass_per_length_sq,
            },
            scalar_mass_per_length_sq,
        ),
        x_x,
        scalar_mass.clone(),
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = scalar_length.clone();
    let codomain_type = scalar_mass;

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

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(scalar_mass_per_length.clone()),
        },
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    match &grad_result {
        Value::Field {
            codomain_type: ct, ..
        } => {
            assert_eq!(
                ct, &scalar_mass_per_length,
                "gradient codomain_type should be Scalar{{MASS/LENGTH}}, got {:?}",
                ct
            );
        }
        _ => panic!("gradient should return a Field, got {:?}", grad_result),
    }

    // Sample the gradient field at x = 3.0m.
    // f(x) = k·x², f'(x) = 2·k·x. At x=3.0: 2·2.0·3.0 = 12.0 [MASS/LENGTH].
    let grad_field_type = Type::Field {
        domain: Box::new(scalar_length.clone()),
        codomain: Box::new(scalar_mass_per_length.clone()),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(
                Value::Scalar {
                    si_value: 3.0,
                    dimension: dim_length,
                },
                scalar_length,
            ),
        ],
        scalar_mass_per_length.clone(),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    match &sample_result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 12.0).abs() < 1e-4,
                "gradient of 2·x² at x=3.0m should be ~12.0, got {}",
                si_value
            );
            assert_eq!(
                *dimension, dim_mass_per_length,
                "gradient dimension should be MASS/LENGTH, got {:?}",
                dimension
            );
        }
        _ => panic!(
            "gradient sample should return Scalar with MASS/LENGTH dimension, got {:?}",
            sample_result
        ),
    }
}

/// Tests for gradient behavior when `codomain_type` does not match the lambda's runtime
/// return type.
///
/// ## DESIGN DECISION: trust-the-declaration
///
/// The gradient computation trusts `codomain_type` for dimensioning the result, not the
/// runtime return type of the lambda. This means a misconfigured `codomain_type` silently
/// propagates the declared (wrong) dimension into the gradient field's metadata.
///
/// **Dimensionless domain:** In debug builds, a hard `debug_assert!` catches mismatches when
/// `domain_dim.is_none()` — the lambda's return type is unambiguous for dimensionless inputs,
/// so a mismatch is always a bug. The assertion fires after the first `f_plus` evaluation when
/// `f_plus.dimension() != expected_codomain_dim`.
///
/// **Dimensioned domain:** The debug assertion is intentionally skipped (`domain_dim.is_some()`)
/// because lambda arithmetic like `Real * Scalar<LENGTH>` returns `Scalar<LENGTH>` regardless
/// of the declared codomain — the `codomain_type` is metadata, not a runtime constraint. A
/// soft `eprintln!` warning is emitted instead. This avoids false positives in legitimate use
/// cases.
///
/// Rationale: changing to runtime-driven dimensioning would require propagating dimension
/// metadata through all arithmetic operations, which is architecturally expensive and
/// error-prone.
mod trust_the_declaration_tests {
    use super::*;

    /// Build a gradient field for a field with dimensioned domain (Type::length()) and
    /// declared codomain `Type::Scalar{MASS}`, where the lambda `|x| 2*x` returns
    /// `Scalar{LENGTH}` at runtime (not MASS as declared).
    ///
    /// Returns `(grad_field, domain_type, grad_codomain_type)` where:
    /// - `grad_field` is the evaluated `Value::Field { source: Gradient, .. }`
    /// - `domain_type` is `Type::length()` (Scalar{LENGTH})
    /// - `grad_codomain_type` is `Type::Scalar { dimension: MASS/LENGTH }` (trusts declaration)
    ///
    /// The lambda `|x| 2*x` for a Scalar{LENGTH} input returns Scalar{LENGTH},
    /// not the declared Scalar{MASS} — this demonstrates the trust-the-declaration behavior.
    fn make_dimensioned_domain_mismatch_gradient() -> (Value, Type, Type) {
        let x_id = ValueCellId::new("$lambda0.S", "x");
        let dim_kg = DimensionVector::MASS;

        // Lambda: |x| 2*x — receives Scalar{LENGTH} at runtime, returns Scalar{LENGTH},
        // NOT Scalar{MASS} as codomain_type declares.
        let body = CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::literal(Value::Real(2.0), Type::Real),
            CompiledExpr::value_ref(x_id.clone(), Type::length()),
            Type::length(),
        );
        let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

        // Domain: Length{LENGTH}; codomain: declared as Scalar{MASS} (mismatches runtime)
        let domain_type = Type::length();
        let codomain_type = Type::Scalar { dimension: dim_kg };

        let field = Value::Field {
            domain_type: domain_type.clone(),
            codomain_type: codomain_type.clone(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(lambda),
        };

        let grad_expr = make_function_call(
            "gradient",
            vec![CompiledExpr::literal(
                field,
                Type::Field {
                    domain: Box::new(domain_type.clone()),
                    codomain: Box::new(codomain_type.clone()),
                },
            )],
            Type::Field {
                domain: Box::new(Type::length()),
                codomain: Box::new(codomain_type.clone()),
            },
        );

        let values = ValueMap::new();
        let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

        assert!(
            matches!(&grad_result, Value::Field { .. }),
            "make_dimensioned_domain_mismatch_gradient: gradient should return a Field, got {:?}",
            grad_result
        );

        let grad_codomain_type = Type::Scalar {
            dimension: DimensionVector::MASS.div(&DimensionVector::LENGTH),
        };
        (grad_result, domain_type, grad_codomain_type)
    }

    /// Structural check: gradient field codomain_type trusts declaration (dimensionless domain).
    ///
    /// See module doc (trust-the-declaration). No sampling; verifies the returned gradient
    /// Field has codomain_type == Scalar[MASS] as declared (not the runtime Real).
    #[test]
    fn gradient_codomain_type_vs_runtime_mismatch_field_structure() {
        let x_id = ValueCellId::new("$lambda0.S", "x");
        let dim_kg = DimensionVector::MASS;

        let body = CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::literal(Value::Real(2.0), Type::Real),
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            Type::Real,
        );
        let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

        let domain_type = Type::Real;
        let codomain_type = Type::Scalar { dimension: dim_kg };

        let field = Value::Field {
            domain_type: domain_type.clone(),
            codomain_type: codomain_type.clone(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(lambda),
        };

        let grad_expr = make_function_call(
            "gradient",
            vec![CompiledExpr::literal(
                field,
                Type::Field {
                    domain: Box::new(domain_type),
                    codomain: Box::new(codomain_type.clone()),
                },
            )],
            Type::Field {
                domain: Box::new(Type::Real),
                codomain: Box::new(codomain_type.clone()),
            },
        );

        let values = ValueMap::new();
        let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

        // Gradient field's codomain_type should be Scalar[MASS] (trusts declaration)
        match &grad_result {
            Value::Field {
                codomain_type: ct, ..
            } => {
                assert_eq!(
                    ct, &codomain_type,
                    "gradient field codomain_type should be Scalar[MASS] (trusts declaration), got {:?}",
                    ct
                );
            }
            _ => panic!("gradient should return a Field, got {:?}", grad_result),
        }
    }

    /// Sampling panics in debug mode: dimensionless-domain mismatch hits the hard assertion.
    ///
    /// See module doc (trust-the-declaration). The debug_assert fires after f_plus evaluation:
    /// f_plus.dimension() (DIMENSIONLESS) != expected_codomain_dim (MASS).
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "codomain_type does not match")]
    fn gradient_codomain_type_vs_runtime_mismatch() {
        let x_id = ValueCellId::new("$lambda0.S", "x");
        let dim_kg = DimensionVector::MASS;

        // Lambda: |x| 2*x — body returns Value::Real at runtime (NOT Value::Scalar[MASS])
        let body = CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::literal(Value::Real(2.0), Type::Real),
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            Type::Real,
        );
        let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

        // Domain: dimensionless Real; codomain: declared as Scalar[MASS] (mismatch!)
        let domain_type = Type::Real;
        let codomain_type = Type::Scalar { dimension: dim_kg };

        let field = Value::Field {
            domain_type: domain_type.clone(),
            codomain_type: codomain_type.clone(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(lambda),
        };

        let grad_field_type = Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(codomain_type),
        };

        let grad_expr = make_function_call(
            "gradient",
            vec![CompiledExpr::literal(field, grad_field_type.clone())],
            grad_field_type.clone(),
        );

        let values = ValueMap::new();
        let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

        // Sampling triggers the debug assertion: f_plus returns Real (dimensionless) but
        // codomain declares MASS — the assertion fires here.
        let sample_expr = make_function_call(
            "sample",
            vec![
                CompiledExpr::literal(grad_result, grad_field_type),
                CompiledExpr::literal(Value::Real(1.0), Type::Real),
            ],
            Type::Scalar { dimension: dim_kg },
        );
        let _sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
        // Expected to panic before reaching here
    }

    /// Structural check: gradient field codomain_type trusts declaration (dimensioned domain).
    ///
    /// See module doc (trust-the-declaration). No sampling; verifies codomain_type ==
    /// Scalar{MASS/LENGTH} (declared MASS / domain LENGTH).
    #[test]
    fn gradient_codomain_mismatch_dimensioned_domain_trusts_declaration() {
        let (grad_result, _domain_type, grad_codomain_type) =
            make_dimensioned_domain_mismatch_gradient();

        // The gradient field's codomain_type must be Scalar{MASS/LENGTH} — derived from
        // the declared codomain (MASS) divided by the domain dimension (LENGTH).
        // This demonstrates trust-the-declaration: the runtime Scalar{LENGTH} returned
        // by the lambda is ignored; the declared MASS drives the gradient dimension.
        match &grad_result {
            Value::Field {
                codomain_type: ct, ..
            } => {
                assert_eq!(
                    ct, &grad_codomain_type,
                    "gradient codomain_type should be Scalar[MASS/LENGTH] (trusts declaration), got {:?}",
                    ct
                );
            }
            _ => panic!("gradient should return a Field, got {:?}", grad_result),
        }
    }

    /// Sampling completes without panic for dimensioned-domain mismatch (soft warning only).
    ///
    /// See module doc (trust-the-declaration). Samples at Scalar{1.0, LENGTH};
    /// result is Scalar{≈2.0, MASS/LENGTH}.
    #[test]
    fn gradient_codomain_mismatch_dimensioned_domain_no_panic() {
        let (grad_result, domain_type, grad_codomain_type) =
            make_dimensioned_domain_mismatch_gradient();
        let values = ValueMap::new();

        let grad_field_type = Type::Field {
            domain: Box::new(domain_type),
            codomain: Box::new(grad_codomain_type.clone()),
        };

        // Sample at Scalar{1.0, LENGTH} — must NOT panic (unlike the dimensionless-domain case)
        let point = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let sample_expr = make_function_call(
            "sample",
            vec![
                CompiledExpr::literal(grad_result, grad_field_type),
                CompiledExpr::literal(point, Type::length()),
            ],
            grad_codomain_type,
        );

        // This must not panic (contrast: gradient_codomain_type_vs_runtime_mismatch does panic)
        let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

        // Verify the result has dimension MASS/LENGTH and derivative ≈ 2.0
        match &sample_result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(
                    *dimension,
                    DimensionVector::MASS.div(&DimensionVector::LENGTH),
                    "derivative dimension should be MASS/LENGTH, got {:?}",
                    dimension
                );
                assert!(
                    (si_value - 2.0).abs() < 1e-4,
                    "derivative should be ≈2.0 (d/dx[2x] = 2), got {}",
                    si_value
                );
            }
            _ => panic!(
                "sample should return Scalar{{MASS/LENGTH}}, got {:?}",
                sample_result
            ),
        }
    }
}

/// NaN in a Point coordinate causes gradient sampling to return Undef
/// via the decomposed (multi-param) path.
///
/// Build a 3D field with decomposed lambda |x,y,z| x + 2*y + 3*z via
/// make_decomposed_n3_gradient_field(). Sample the gradient at
/// Point3(5.0, NaN, 11.0). The is_finite guard in compute_numerical_gradient
/// must catch NaN before perturbing, returning Undef.
///
/// Complements gradient_sample_with_nan_point_returns_undef (single-point-param path).
#[test]
fn gradient_decomposed_nan_returns_undef() {
    let (grad_result, domain_type, grad_codomain_type) = make_decomposed_n3_gradient_field();
    let values = ValueMap::new();

    // Sample at Point3(5.0, NaN, 11.0)
    let nan_point = Value::Point(vec![
        Value::Real(5.0),
        Value::Real(f64::NAN),
        Value::Real(11.0),
    ]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(nan_point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at Point3 with NaN coordinate must return Undef (decomposed path)"
    );
}

/// Inf in a Point coordinate causes gradient sampling to return Undef
/// via the decomposed (multi-param) path.
///
/// Build a 3D field with decomposed lambda |x,y,z| x + 2*y + 3*z via
/// make_decomposed_n3_gradient_field(). Sample the gradient at
/// Point3(5.0, Inf, 11.0). The is_finite guard in compute_numerical_gradient
/// must catch Inf before perturbing, returning Undef.
///
/// Complements gradient_sample_with_inf_point_returns_undef (single-point-param path).
#[test]
fn gradient_decomposed_inf_returns_undef() {
    let (grad_result, domain_type, grad_codomain_type) = make_decomposed_n3_gradient_field();
    let values = ValueMap::new();

    // Sample at Point3(5.0, Inf, 11.0)
    let inf_point = Value::Point(vec![
        Value::Real(5.0),
        Value::Real(f64::INFINITY),
        Value::Real(11.0),
    ]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(inf_point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at Point3 with Inf coordinate must return Undef (decomposed path)"
    );
}

/// Negative infinity in a Point coordinate causes gradient sampling to return Undef
/// via the decomposed (multi-param) path.
///
/// Complements gradient_decomposed_inf_returns_undef (+Inf) and
/// gradient_decomposed_nan_returns_undef (NaN).
///
/// Build a 3D field with decomposed lambda |x,y,z| x + 2*y + 3*z via
/// make_decomposed_n3_gradient_field(). Sample the gradient at
/// Point3(5.0, NEG_INFINITY, 11.0). The is_finite guard in compute_numerical_gradient
/// must catch NEG_INFINITY before perturbing, returning Undef.
///
/// Complements gradient_sample_with_inf_point_returns_undef (single-point-param path).
#[test]
fn gradient_decomposed_neg_inf_returns_undef() {
    let (grad_result, domain_type, grad_codomain_type) = make_decomposed_n3_gradient_field();
    let values = ValueMap::new();

    // Sample at Point3(5.0, NEG_INFINITY, 11.0)
    let neg_inf_point = Value::Point(vec![
        Value::Real(5.0),
        Value::Real(f64::NEG_INFINITY),
        Value::Real(11.0),
    ]);

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(grad_codomain_type),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, grad_field_type),
            CompiledExpr::literal(neg_inf_point, Type::point3(Type::Real)),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        sample_result,
        Value::Undef,
        "gradient sampled at Point3 with NEG_INFINITY coordinate must return Undef (decomposed path)"
    );
}
