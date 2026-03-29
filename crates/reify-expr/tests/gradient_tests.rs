//! Gradient tests.
//!
//! Tests for numerical gradient via central differences on analytical fields.
//! Includes edge-case guardrails and positive tests for gradient computation.

use reify_expr::{EvalContext, eval_expr};
use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ContentHash, DimensionVector, FieldSourceKind,
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
        lambda: Box::new(lambda),
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
        lambda: Box::new(lambda),
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
    match &sample_result {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "gradient should have 3 components");
            let expected = [2.0, 2.0, 3.0];
            for (i, (comp, &exp)) in components.iter().zip(expected.iter()).enumerate() {
                let val = comp
                    .as_f64()
                    .unwrap_or_else(|| panic!("component {} should be numeric, got {:?}", i, comp));
                assert!(
                    (val - exp).abs() < 1e-4,
                    "gradient component {} should be ~{}, got {}",
                    i,
                    exp,
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
        lambda: Box::new(lambda),
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
