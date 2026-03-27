//! Field gradient evaluation tests.
//!
//! Tests for evaluating `gradient(field)` to produce gradient Field values
//! and sampling gradient fields via central differences.

use reify_expr::{EvalContext, eval_expr};
use reify_types::{
    CompiledExpr, CompiledExprKind, ContentHash, DimensionVector, FieldSourceKind,
    ResolvedFunction, Type, Value, ValueCellId, ValueMap,
};

// ── Helpers ───────────────────────────────────────────────────────────

/// Build a CompiledExpr::FunctionCall for a stdlib function.
fn make_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
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

/// Build a Value::Lambda with a literal body (constant field).
fn make_constant_lambda(body_value: Value, body_type: Type) -> Value {
    let p_id = ValueCellId::new("$lambda_field", "p");
    Value::Lambda {
        params: vec![("p".to_string(), p_id)],
        body: Box::new(CompiledExpr::literal(body_value, body_type)),
        captures: ValueMap::new(),
    }
}

/// Build a valid scalar field: Point3<Length> -> Scalar<Length> with a constant lambda.
fn make_valid_scalar_field() -> Value {
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let lambda = make_constant_lambda(
        Value::Scalar {
            si_value: 5.0,
            dimension: DimensionVector::LENGTH,
        },
        codomain.clone(),
    );
    Value::Field {
        domain_type: domain,
        codomain_type: codomain,
        source: FieldSourceKind::Analytical,
        lambda: Box::new(lambda),
    }
}

// ── Step 1: gradient field construction tests ─────────────────────────

#[test]
fn gradient_of_non_field_returns_undef() {
    // gradient(42) should return Undef
    let expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(Value::Int(42), Type::Int)],
        Type::Real, // result type doesn't matter for Undef
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "gradient(non_field) should be Undef, got: {:?}", result);
}

#[test]
fn gradient_of_field_with_non_point3_domain_returns_undef() {
    // Field<Scalar, Scalar> — domain is not Point3
    let field = Value::Field {
        domain_type: Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        codomain_type: Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        source: FieldSourceKind::Analytical,
        lambda: Box::new(make_constant_lambda(Value::Real(1.0), Type::Real)),
    };
    let expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(
            field,
            Type::Field {
                domain: Box::new(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
                codomain: Box::new(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            },
        )],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "gradient(non-Point3-domain field) should be Undef, got: {:?}", result);
}

#[test]
fn gradient_of_field_with_non_scalar_codomain_returns_undef() {
    // Field<Point3<Length>, Vector3<Length>> — codomain is Vector, not Scalar
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::vec3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Box::new(make_constant_lambda(Value::Real(1.0), Type::Real)),
    };
    let expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(
            field,
            Type::Field {
                domain: Box::new(domain),
                codomain: Box::new(codomain),
            },
        )],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "gradient(vector-codomain field) should be Undef, got: {:?}", result);
}

#[test]
fn gradient_of_valid_field_returns_gradient_field() {
    let field = make_valid_scalar_field();
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(
            field,
            Type::Field {
                domain: Box::new(domain.clone()),
                codomain: Box::new(codomain),
            },
        )],
        Type::Real, // result type at compiled level doesn't affect eval
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    match &result {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } => {
            // Source should be Gradient
            assert_eq!(
                *source,
                FieldSourceKind::Gradient,
                "expected Gradient source, got: {:?}",
                source
            );
            // Domain type should be the same (Point3<Length>)
            assert_eq!(
                *domain_type, domain,
                "gradient field domain should match original"
            );
            // Codomain should be Vector3<Scalar<Length/Length>> = Vector3<Scalar<Dimensionless>>
            let expected_gradient_dim = DimensionVector::LENGTH.div(&DimensionVector::LENGTH);
            let expected_codomain = Type::vec3(Type::Scalar {
                dimension: expected_gradient_dim,
            });
            assert_eq!(
                *codomain_type, expected_codomain,
                "gradient codomain should be Vector3<Scalar<gradient_dim>>"
            );
            // Lambda should contain the original field (not Undef)
            assert!(
                matches!(**lambda, Value::Field { .. }),
                "gradient field lambda should store the original field, got: {:?}",
                lambda
            );
        }
        other => panic!("expected Value::Field for gradient(valid_field), got: {:?}", other),
    }
}

// ── Step 3: sampling gradient fields tests ────────────────────────────

/// Build a Point3 value with given coordinates and dimension.
fn make_point3(x: f64, y: f64, z: f64, dim: DimensionVector) -> Value {
    Value::Point(vec![
        Value::Scalar {
            si_value: x,
            dimension: dim,
        },
        Value::Scalar {
            si_value: y,
            dimension: dim,
        },
        Value::Scalar {
            si_value: z,
            dimension: dim,
        },
    ])
}

#[test]
fn sample_gradient_of_constant_field_near_zero() {
    // Build constant field f(p) = 5.0 (Scalar<Length>)
    let field = make_valid_scalar_field(); // Point3<Length> -> Scalar<Length>, lambda body = 5.0

    // Build gradient(field) expr
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let field_type = Type::Field {
        domain: Box::new(domain.clone()),
        codomain: Box::new(codomain),
    };
    let gradient_expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    // Build sample(gradient_field, point3(1,2,3)) expr
    let point = make_point3(1.0, 2.0, 3.0, DimensionVector::LENGTH);
    let sample_expr = make_call(
        "sample",
        vec![
            gradient_expr,
            CompiledExpr::literal(point, Type::point3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        ],
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }),
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Gradient of a constant field should be ~[0, 0, 0]
    match &result {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "gradient should have 3 components");
            for (i, comp) in components.iter().enumerate() {
                let v = comp.as_f64().unwrap_or_else(|| {
                    panic!("component {} should be numeric, got: {:?}", i, comp)
                });
                assert!(
                    v.abs() < 1e-3,
                    "gradient component {} of constant field should be ~0, got: {}",
                    i,
                    v
                );
            }
        }
        other => panic!(
            "sample(gradient(constant_field), point) should return Vector, got: {:?}",
            other
        ),
    }
}

#[test]
fn sample_gradient_with_non_point3_returns_undef() {
    // Build gradient field
    let field = make_valid_scalar_field();
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let field_type = Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(codomain),
    };
    let gradient_expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    // Try to sample with a non-Point3 value (a scalar)
    let sample_expr = make_call(
        "sample",
        vec![
            gradient_expr,
            CompiledExpr::literal(Value::Real(42.0), Type::Real),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "sample(gradient_field, non_point3) should be Undef, got: {:?}",
        result
    );
}
