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
        inner_field: None,
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
        inner_field: None,
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
        inner_field: None,
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
            inner_field,
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
            // Lambda must be Undef (data contract: lambda is always callable-or-Undef)
            assert!(
                matches!(**lambda, Value::Undef),
                "gradient field lambda should be Undef (data contract), got: {:?}",
                lambda
            );
            // inner_field must hold the original field
            let inner = inner_field
                .as_ref()
                .expect("gradient field should have inner_field = Some(...)");
            assert!(
                matches!(
                    inner.as_ref(),
                    Value::Field {
                        source: FieldSourceKind::Analytical,
                        ..
                    }
                ),
                "inner_field should be the original Analytical field, got: {:?}",
                inner
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

// ── Step 5: linear field gradient test ────────────────────────────────

/// Build a field lambda: f(p) = dot(p, vec3(a, b, c))
/// The lambda's param is `p` with the given ValueCellId.
fn make_dot_lambda(
    p_id: ValueCellId,
    coeffs: [f64; 3],
    domain_type: Type,
    result_type: Type,
) -> Value {
    // Build vec3(a, b, c) call
    let vec3_call = make_call(
        "vec3",
        vec![
            CompiledExpr::literal(Value::Real(coeffs[0]), Type::Real),
            CompiledExpr::literal(Value::Real(coeffs[1]), Type::Real),
            CompiledExpr::literal(Value::Real(coeffs[2]), Type::Real),
        ],
        Type::vec3(Type::dimensionless_scalar()),
    );

    // Build dot(p, vec3(a,b,c)) call
    let dot_call = make_call(
        "dot",
        vec![
            CompiledExpr::value_ref(p_id.clone(), domain_type),
            vec3_call,
        ],
        result_type.clone(),
    );

    Value::Lambda {
        params: vec![("p".to_string(), p_id)],
        body: Box::new(dot_call),
        captures: ValueMap::new(),
    }
}

#[test]
fn gradient_of_linear_field_dot_123() {
    // f(p) = dot(p, vec3(1, 2, 3)) = x + 2y + 3z
    // Domain: Point3<Length>, Codomain: Scalar<Length>
    // Gradient = [1, 2, 3] dimensionless (Length/Length)
    let p_id = ValueCellId::new("$lambda_field", "p");
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    let lambda = make_dot_lambda(p_id, [1.0, 2.0, 3.0], domain.clone(), codomain.clone());
    let field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Box::new(lambda),
        inner_field: None,
    };

    // gradient(field)
    let field_type = Type::Field {
        domain: Box::new(domain.clone()),
        codomain: Box::new(codomain),
    };
    let gradient_expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    // sample(gradient_field, point3(1m, 2m, 3m))
    let point = make_point3(1.0, 2.0, 3.0, DimensionVector::LENGTH);
    let sample_expr = make_call(
        "sample",
        vec![
            gradient_expr,
            CompiledExpr::literal(
                point,
                Type::point3(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            ),
        ],
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }),
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    match &result {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "gradient should have 3 components");
            let expected = [1.0, 2.0, 3.0];
            for (i, (comp, &exp)) in components.iter().zip(expected.iter()).enumerate() {
                let v = comp
                    .as_f64()
                    .unwrap_or_else(|| panic!("component {} should be numeric, got: {:?}", i, comp));
                assert!(
                    (v - exp).abs() < 1e-4,
                    "gradient component {} should be ~{}, got: {}",
                    i,
                    exp,
                    v
                );
            }
        }
        other => panic!(
            "sample(gradient(linear_field), point) should return Vector, got: {:?}",
            other
        ),
    }
}

// ── Step 7: edge case tests ───────────────────────────────────────────

/// Build a field lambda: f(p) = dot(p, p) = x² + y² + z²
fn make_dot_self_lambda(p_id: ValueCellId, domain_type: Type, result_type: Type) -> Value {
    let dot_call = make_call(
        "dot",
        vec![
            CompiledExpr::value_ref(p_id.clone(), domain_type.clone()),
            CompiledExpr::value_ref(p_id.clone(), domain_type),
        ],
        result_type.clone(),
    );
    Value::Lambda {
        params: vec![("p".to_string(), p_id)],
        body: Box::new(dot_call),
        captures: ValueMap::new(),
    }
}

#[test]
fn gradient_of_quadratic_field_dot_self() {
    // f(p) = dot(p, p) = x² + y² + z²
    // Domain: Point3<Length>, Codomain: Scalar<Length²>
    // At (1m, 2m, 3m): gradient = [2x, 2y, 2z] = [2, 4, 6] Scalar<Length>
    let p_id = ValueCellId::new("$lambda_field", "p");
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let length_squared = DimensionVector::LENGTH.mul(&DimensionVector::LENGTH);
    let codomain = Type::Scalar {
        dimension: length_squared,
    };

    let lambda = make_dot_self_lambda(p_id, domain.clone(), codomain.clone());
    let field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Box::new(lambda),
        inner_field: None,
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

    let point = make_point3(1.0, 2.0, 3.0, DimensionVector::LENGTH);
    let sample_expr = make_call(
        "sample",
        vec![
            gradient_expr,
            CompiledExpr::literal(
                point,
                Type::point3(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            ),
        ],
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    match &result {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3);
            // gradient of x²+y²+z² at (1,2,3) is (2, 4, 6)
            let expected = [2.0, 4.0, 6.0];
            for (i, (comp, &exp)) in components.iter().zip(expected.iter()).enumerate() {
                let v = comp
                    .as_f64()
                    .unwrap_or_else(|| panic!("component {} should be numeric, got: {:?}", i, comp));
                assert!(
                    (v - exp).abs() < 1e-3,
                    "gradient component {} should be ~{}, got: {} (diff: {})",
                    i,
                    exp,
                    v,
                    (v - exp).abs()
                );
                // Verify dimension is Length (Length² / Length)
                assert_eq!(
                    comp.dimension(),
                    DimensionVector::LENGTH,
                    "gradient component dimension should be LENGTH"
                );
            }
        }
        other => panic!(
            "sample(gradient(quadratic_field), point) should return Vector, got: {:?}",
            other
        ),
    }
}

#[test]
fn gradient_dimension_temperature_over_length() {
    // Field: Point3<Length> -> Scalar<Temperature>
    // Gradient should produce Vector3<Scalar<Temperature/Length>>
    let p_id = ValueCellId::new("$lambda_field", "p");
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    // Use TEMPERATURE dimension for codomain
    let temp_dim = DimensionVector::TEMPERATURE;
    let codomain = Type::Scalar {
        dimension: temp_dim,
    };

    // Constant field f(p) = 300 K
    let lambda = Value::Lambda {
        params: vec![("p".to_string(), p_id)],
        body: Box::new(CompiledExpr::literal(
            Value::Scalar {
                si_value: 300.0,
                dimension: temp_dim,
            },
            codomain.clone(),
        )),
        captures: ValueMap::new(),
    };
    let field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Box::new(lambda),
        inner_field: None,
    };

    // gradient(field)
    let field_type = Type::Field {
        domain: Box::new(domain.clone()),
        codomain: Box::new(codomain),
    };
    let gradient_expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    // Check gradient field's codomain type
    let values = ValueMap::new();
    let grad_field = eval_expr(&gradient_expr, &EvalContext::simple(&values));
    match &grad_field {
        Value::Field {
            codomain_type,
            source,
            ..
        } => {
            assert_eq!(*source, FieldSourceKind::Gradient);
            let expected_dim = temp_dim.div(&DimensionVector::LENGTH);
            let expected = Type::vec3(Type::Scalar {
                dimension: expected_dim,
            });
            assert_eq!(
                *codomain_type, expected,
                "gradient codomain should be Vector3<Scalar<Temperature/Length>>"
            );
        }
        other => panic!("expected gradient field, got: {:?}", other),
    }
}

#[test]
fn gradient_at_origin_stable() {
    // Test at origin (0, 0, 0) — h floor is 1e-6 * 1e-3 = 1e-9
    // Using linear field f(p) = dot(p, vec3(1, 2, 3)), gradient should still be [1, 2, 3]
    let p_id = ValueCellId::new("$lambda_field", "p");
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let lambda = make_dot_lambda(p_id, [1.0, 2.0, 3.0], domain.clone(), codomain.clone());
    let field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Analytical,
        lambda: Box::new(lambda),
        inner_field: None,
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

    // Sample at origin
    let point = make_point3(0.0, 0.0, 0.0, DimensionVector::LENGTH);
    let sample_expr = make_call(
        "sample",
        vec![
            gradient_expr,
            CompiledExpr::literal(
                point,
                Type::point3(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            ),
        ],
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }),
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    match &result {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3);
            let expected = [1.0, 2.0, 3.0];
            for (i, (comp, &exp)) in components.iter().zip(expected.iter()).enumerate() {
                let v = comp
                    .as_f64()
                    .unwrap_or_else(|| panic!("component {} should be numeric, got: {:?}", i, comp));
                assert!(
                    (v - exp).abs() < 1e-4,
                    "gradient at origin, component {} should be ~{}, got: {} (diff: {})",
                    i,
                    exp,
                    v,
                    (v - exp).abs()
                );
            }
        }
        other => panic!(
            "sample(gradient(field), origin) should return Vector, got: {:?}",
            other
        ),
    }
}

// ── Step 9: data contract tests ──────────────────────────────────────

#[test]
fn gradient_field_lambda_data_contract() {
    // After constructing a gradient field, the lambda data contract must hold:
    //   - lambda is Value::Undef (not a Field — callable-or-Undef invariant)
    //   - inner_field is Some(original_field) with source=Analytical
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
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    match &result {
        Value::Field {
            source,
            lambda,
            inner_field,
            ..
        } => {
            assert_eq!(*source, FieldSourceKind::Gradient);

            // Lambda must be Undef — not a Field value.
            // This enforces the documented invariant that lambda is always
            // "the callable lambda for analytical/composed fields, or Undef".
            assert!(
                lambda.is_undef(),
                "gradient field lambda must be Undef (data contract), got: {:?}",
                lambda
            );

            // inner_field must be Some(original field) with source=Analytical
            let inner = inner_field
                .as_ref()
                .expect("gradient field inner_field should be Some(...)");
            match inner.as_ref() {
                Value::Field {
                    source: inner_source,
                    domain_type: inner_domain,
                    lambda: inner_lambda,
                    ..
                } => {
                    assert!(
                        matches!(inner_source, FieldSourceKind::Analytical),
                        "inner field source should be Analytical, got: {:?}",
                        inner_source
                    );
                    // The inner field should have the same domain
                    assert_eq!(
                        format!("{}", inner_domain),
                        format!("{}", domain),
                        "inner field domain should match original"
                    );
                    // The inner field's lambda should be callable (Lambda, not Undef)
                    assert!(
                        matches!(inner_lambda.as_ref(), Value::Lambda { .. }),
                        "inner field lambda should be callable, got: {:?}",
                        inner_lambda
                    );
                }
                other => panic!(
                    "inner_field should contain a Value::Field, got: {:?}",
                    other
                ),
            }
        }
        other => panic!("expected gradient Value::Field, got: {:?}", other),
    }
}

// ── Step 11: uncallable-lambda guard tests ──────────────────────────

/// gradient() on a Sampled field (lambda=Undef) should return Undef.
/// Sampled fields have no callable lambda — central differences cannot be computed.
#[test]
fn gradient_of_sampled_field_returns_undef() {
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    };
    let sampled_field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Sampled,
        lambda: Box::new(Value::Undef),
        inner_field: None,
    };
    let expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(
            sampled_field,
            Type::Field {
                domain: Box::new(domain),
                codomain: Box::new(codomain),
            },
        )],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "gradient(sampled field with Undef lambda) should be Undef, got: {:?}",
        result
    );
}

/// gradient() on an Imported field (lambda=Undef) should return Undef.
/// Imported fields have no callable lambda — central differences cannot be computed.
#[test]
fn gradient_of_imported_field_returns_undef() {
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    };
    let imported_field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Imported,
        lambda: Box::new(Value::Undef),
        inner_field: None,
    };
    let expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(
            imported_field,
            Type::Field {
                domain: Box::new(domain),
                codomain: Box::new(codomain),
            },
        )],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "gradient(imported field with Undef lambda) should be Undef, got: {:?}",
        result
    );
}

/// gradient() on a Composed field with lambda=Undef should return Undef.
/// This covers the case where a composed field lost its lambda (e.g., serialized/deserialized).
#[test]
fn gradient_of_field_with_undef_lambda_returns_undef() {
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    };
    let composed_field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Composed,
        lambda: Box::new(Value::Undef),
        inner_field: None,
    };
    let expr = make_call(
        "gradient",
        vec![CompiledExpr::literal(
            composed_field,
            Type::Field {
                domain: Box::new(domain),
                codomain: Box::new(codomain),
            },
        )],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "gradient(composed field with Undef lambda) should be Undef, got: {:?}",
        result
    );
}
