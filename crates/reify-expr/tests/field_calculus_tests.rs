//! Field calculus tests.
//!
//! Accuracy and correctness tests for field differential operators
//! (gradient, divergence, curl, laplacian) using analytical fields
//! with known mathematical derivatives.
//!
//! Helpers are defined locally following the pattern in gradient_tests.rs
//! and field_eval_tests.rs.

use reify_expr::{EvalContext, eval_expr};
use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ContentHash, DimensionVector, FieldSourceKind,
    ResolvedFunction, Type, UnOp, Value, ValueCellId, ValueMap,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build a FunctionCall expression for stdlib functions.
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

/// Build a Value::Lambda with (name, id) param pairs.
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
                    panic!(
                        "{label}: component {i} should be numeric, got {:?}",
                        comp
                    )
                });
                assert!(
                    (val - exp).abs() < tol,
                    "{label}: component {i} = {val} differs from expected {exp} by {} (tolerance {tol})",
                    (val - exp).abs()
                );
            }
        }
        _ => panic!("{label}: expected Value::Vector, got {:?}", result),
    }
}

// ── Step 1: Gradient accuracy tests ──────────────────────────────────────────

/// Gradient of f(x) = x*x at x=3.0 should be ≈6.0.
///
/// Analytical derivative: df/dx = 2x. At x=3.0: 2*3=6.0.
/// Central differences with h~1e-6 gives O(h²) error, well within 1e-4.
#[test]
fn gradient_1d_quadratic_accuracy() {
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
        lambda: Box::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    // gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of 1D quadratic should return a Field, got {:?}",
        grad_result
    );

    // sample(gradient_field, 3.0)
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result
        .as_f64()
        .unwrap_or_else(|| panic!("gradient sample should be numeric, got {:?}", sample_result));
    assert!(
        (val - 6.0).abs() < 1e-4,
        "gradient of x*x at x=3.0 should be ≈6.0, got {}",
        val
    );
}

/// Gradient of f(x,y,z)=x²+y²+z² at (1,2,3) should be ≈[2,4,6].
///
/// Partial derivatives: df/dx=2x, df/dy=2y, df/dz=2z.
/// At (1,2,3): [2*1, 2*2, 2*3] = [2, 4, 6].
#[test]
fn gradient_3d_sum_of_squares_accuracy() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + y*y + z*z
    let xx = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let yy = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let zz = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(BinOp::Add, xx, yy, Type::Real),
        zz,
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

    // gradient(field)
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
        "gradient of 3D field should return a Field, got {:?}",
        grad_result
    );

    // sample(gradient_field, Point3(1.0, 2.0, 3.0))
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

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

    // Expected: Vector3(2.0, 4.0, 6.0)
    assert_gradient_vector(
        &sample_result,
        &[2.0, 4.0, 6.0],
        1e-4,
        "gradient of x²+y²+z² at (1,2,3)",
    );
}

// ── Steps 2–3: Divergence and curl tests ─────────────────────────────────────

/// Divergence of the identity vector field F(x,y,z)=[x,y,z] at (1,2,3) ≈ 3.0.
///
/// Analytical divergence: ∂x/∂x + ∂y/∂y + ∂z/∂z = 1 + 1 + 1 = 3.
/// divergence wraps around central-difference sampling; tolerance 1e-3
/// accounts for multi-component summation.
#[test]
fn divergence_identity_vector_field() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(x, y, z)
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

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

    // divergence(field) → scalar field
    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::Real),
        },
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of identity vector field should return a Field, got {:?}",
        div_result
    );

    // sample(div_field, Point3(1.0, 2.0, 3.0))
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let div_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::Real),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(div_result, div_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!(
            "divergence sample should be numeric, got {:?}",
            sample_result
        )
    });
    assert!(
        (val - 3.0).abs() < 1e-3,
        "divergence of [x,y,z] at (1,2,3) should be ≈3.0, got {}",
        val
    );
}

/// Same as `divergence_identity_vector_field` but the sample point is
/// supplied as `Value::Vector` instead of `Value::Point`.
/// Prior to the fix this returns `Value::Undef` because
/// `compute_numerical_divergence_at_point` only matched `Value::Point`.
#[test]
fn divergence_accepts_vector_sample_point() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(x, y, z)
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

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

    // divergence(field) → scalar field
    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::Real),
        },
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    // sample(div_field, Vector3(1.0, 2.0, 3.0))  ← Vector, not Point
    let point = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let div_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::Real),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(div_result, div_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!(
            "divergence sample should be numeric, got {:?}",
            sample_result
        )
    });
    assert!(
        (val - 3.0).abs() < 1e-3,
        "divergence of [x,y,z] at Vector(1,2,3) should be ≈3.0, got {}",
        val
    );
}

// ── Step 3: Curl test ─────────────────────────────────────────────────────────

/// Curl of the rotation field F(x,y,z)=[-y,x,0] at (1,2,3) ≈ [0,0,2].
///
/// Analytical curl: (∂Fz/∂y - ∂Fy/∂z, ∂Fx/∂z - ∂Fz/∂x, ∂Fy/∂x - ∂Fx/∂y)
///   = (0-0, 0-0, 1-(-1)) = [0, 0, 2].
/// Tolerance 1e-3 accounts for multi-component numerical differentiation.
#[test]
fn curl_rotation_field() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(-y, x, 0)
    let neg_y = CompiledExpr::unop(
        UnOp::Neg,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let body = make_function_call(
        "vec3",
        vec![
            neg_y,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::literal(Value::Real(0.0), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

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

    // curl(field) → vector field
    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let curl_result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&curl_result, Value::Field { .. }),
        "curl of rotation field should return a Field, got {:?}",
        curl_result
    );

    // sample(curl_field, Point3(1.0, 2.0, 3.0))
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let curl_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(curl_result, curl_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Expected: Vector3(0.0, 0.0, 2.0)
    assert_gradient_vector(
        &sample_result,
        &[0.0, 0.0, 2.0],
        1e-3,
        "curl of [-y,x,0] at (1,2,3)",
    );
}

/// Same as `curl_rotation_field` but the sample point is
/// supplied as `Value::Vector` instead of `Value::Point`.
/// Prior to the fix this returns `Value::Undef` because
/// `compute_numerical_curl_at_point` only matched `Value::Point`.
#[test]
fn curl_accepts_vector_sample_point() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(-y, x, 0)
    let neg_y = CompiledExpr::unop(
        UnOp::Neg,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let body = make_function_call(
        "vec3",
        vec![
            neg_y,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::literal(Value::Real(0.0), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

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

    // curl(field) → vector field
    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::vec3(Type::Real)),
        },
    );

    let values = ValueMap::new();
    let curl_result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    // sample(curl_field, Vector3(1.0, 2.0, 3.0))  ← Vector, not Point
    let point = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let curl_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(curl_result, curl_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Expected: Vector3(0.0, 0.0, 2.0)
    assert_gradient_vector(
        &sample_result,
        &[0.0, 0.0, 2.0],
        1e-3,
        "curl of [-y,x,0] at Vector(1,2,3)",
    );
}

// ── Step 4: Laplacian test ────────────────────────────────────────────────────

/// Laplacian of f(x,y,z)=x²+y²+z² at (1,2,3) ≈ 6.0.
///
/// Laplacian = divergence(gradient(f)).
/// Second partials: ∂²f/∂x² + ∂²f/∂y² + ∂²f/∂z² = 2+2+2 = 6.
/// Tolerance 1e-2 accounts for two levels of numerical differentiation.
#[test]
fn laplacian_quadratic_accuracy() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + y*y + z*z
    let xx = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let yy = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let zz = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(BinOp::Add, xx, yy, Type::Real),
        zz,
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

    // laplacian(field) → scalar field
    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::Real),
        },
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of quadratic field should return a Field, got {:?}",
        lap_result
    );

    // sample(laplacian_field, Point3(1.0, 2.0, 3.0))
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let lap_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::Real),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(lap_result, lap_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result
        .as_f64()
        .unwrap_or_else(|| panic!("laplacian sample should be numeric, got {:?}", sample_result));
    assert!(
        (val - 6.0).abs() < 1e-2,
        "laplacian of x²+y²+z² at (1,2,3) should be ≈6.0, got {}",
        val
    );
}

/// Same as `laplacian_quadratic_accuracy` but the sample point is
/// supplied as `Value::Vector` instead of `Value::Point`.
/// Prior to the fix this returns `Value::Undef` because
/// `compute_numerical_laplacian_at_point` only matched `Value::Point`.
#[test]
fn laplacian_accepts_vector_sample_point() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + y*y + z*z
    let xx = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let yy = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let zz = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(BinOp::Add, xx, yy, Type::Real),
        zz,
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

    // laplacian(field) → scalar field
    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::Real),
        },
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    // sample(laplacian_field, Vector3(1.0, 2.0, 3.0))  ← Vector, not Point
    let point = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let lap_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::Real),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(lap_result, lap_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result
        .as_f64()
        .unwrap_or_else(|| panic!("laplacian sample should be numeric, got {:?}", sample_result));
    assert!(
        (val - 6.0).abs() < 1e-2,
        "laplacian of x²+y²+z² at Vector(1,2,3) should be ≈6.0, got {}",
        val
    );
}

// ── Step 5: Codomain type tests ───────────────────────────────────────────────

/// Gradient of a Real→Real field produces a Field with scalar (Real) codomain.
///
/// For a 1D field, the gradient is a scalar derivative (same dimensionality
/// as the codomain). Verify the gradient Field has codomain_type=Real.
#[test]
fn gradient_1d_scalar_codomain_type() {
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
        lambda: Box::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type,
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    // gradient should return a Field
    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Real→Real should return a Field, got {:?}",
        grad_result
    );

    // Codomain should be scalar (Real for dimensionless domain/codomain)
    if let Value::Field { codomain_type, .. } = &grad_result {
        // 1D gradient of Real→Real produces Real codomain
        match codomain_type {
            Type::Real => {} // correct
            Type::Scalar { dimension } if *dimension == DimensionVector::DIMENSIONLESS => {} // also fine
            other => panic!(
                "gradient_1d_scalar_codomain_type: expected Real or dimensionless Scalar codomain, got {:?}",
                other
            ),
        }
    }
}

/// Gradient of a Point3<Real>→Real field produces a Field with Vector3 codomain.
///
/// For a 3D scalar field, the gradient is a vector of partial derivatives.
/// Verify the gradient Field has codomain_type of vector kind.
#[test]
fn gradient_3d_vector_codomain_type() {
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
        lambda: Box::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
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

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Point3→Real should return a Field, got {:?}",
        grad_result
    );

    // Codomain should be a vector type (Vector { n: 3, .. } or equivalent)
    if let Value::Field { codomain_type, .. } = &grad_result {
        assert!(
            matches!(codomain_type, Type::Vector { n: 3, .. }),
            "gradient of 3D scalar field should have Vector(n=3) codomain, got {:?}",
            codomain_type
        );
    }
}

// ── Step 6: Sample identity test ─────────────────────────────────────────────

/// sample(field, 3.0) returns exactly 7.0 for f(x)=2*x+1.
///
/// Tests the sample pathway independently of differential operators.
/// Confirms field evaluation works as a baseline before testing operators
/// that depend on sampling.
#[test]
fn field_composition_sample_identity() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 2*x + 1
    let two_x = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        two_x,
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
        lambda: Box::new(lambda),
    };

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    // sample(field, 3.0) → 2*3+1 = 7.0
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Real(7.0),
        "sample(f(x)=2*x+1, 3.0) should return exactly 7.0, got {:?}",
        result
    );
}

// ── Step 7: Dimensional correctness test ──────────────────────────────────────

/// Gradient of a Length→Temperature field has codomain dimension Temperature/Length.
///
/// For f: Scalar<Length> → Scalar<Temperature> with lambda |x| → 2*x,
/// gradient codomain_type should have dimension TEMPERATURE / LENGTH.
/// This verifies the R/Q dimensional arithmetic in compute_gradient.
#[test]
fn gradient_dimensional_correctness() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 2.0 * x (temperature field over length domain)
    let body = CompiledExpr::binop(
        BinOp::Mul,
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

    // gradient(field) → gradient field with dimension Temperature/Length
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Length→Temperature field should return a Field, got {:?}",
        grad_result
    );

    // Verify codomain dimension: should be Temperature / Length
    if let Value::Field { codomain_type, .. } = &grad_result {
        let expected_dim = DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH);
        match codomain_type {
            Type::Scalar { dimension } => {
                assert_eq!(
                    *dimension,
                    expected_dim,
                    "gradient codomain dimension should be Temperature/Length ({:?}), got {:?}",
                    expected_dim,
                    dimension
                );
            }
            other => panic!(
                "gradient_dimensional_correctness: expected Scalar codomain, got {:?}",
                other
            ),
        }
    }
}

// ── Step 8: Robustness tests ──────────────────────────────────────────────────

/// Divergence of constant vector field F(x,y,z)=[1,1,1] should be ≈0.
///
/// The divergence of a constant field is exactly zero (no variation in any direction).
/// Central differences should give near-zero result within 1e-6.
#[test]
fn divergence_constant_field_near_zero() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(1.0, 1.0, 1.0) (constant vector field)
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

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

    // divergence(field) → scalar field ≈ 0
    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::Real),
        },
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of constant vector field should return a Field, got {:?}",
        div_result
    );

    // sample at any point, expect near 0
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let div_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::Real),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(div_result, div_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!("divergence sample should be numeric, got {:?}", sample_result)
    });
    assert!(
        val.abs() < 1e-6,
        "divergence of constant [1,1,1] should be ≈0, got {}",
        val
    );
}

/// Gradient of linear f(x,y,z)=x+2*y+3*z is constant [1,2,3] everywhere.
///
/// Verify gradient at two different points both give ≈[1,2,3] within 1e-4.
#[test]
fn gradient_linear_field_constant() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + 2*y + 3*z
    let two_y = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let three_z = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(3.0), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            two_y,
            Type::Real,
        ),
        three_z,
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

    // gradient(field) should give constant [1, 2, 3]
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

    let grad_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::vec3(Type::Real)),
    };

    // Verify at two different points
    let test_points = [
        Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]),
        Value::Point(vec![Value::Real(5.0), Value::Real(10.0), Value::Real(15.0)]),
    ];

    for (i, point) in test_points.iter().enumerate() {
        let sample_expr = make_function_call(
            "sample",
            vec![
                CompiledExpr::literal(grad_result.clone(), grad_field_type.clone()),
                CompiledExpr::literal(point.clone(), domain_type.clone()),
            ],
            Type::vec3(Type::Real),
        );
        let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
        assert_gradient_vector(
            &sample_result,
            &[1.0, 2.0, 3.0],
            1e-4,
            &format!("gradient of x+2y+3z at point {i}"),
        );
    }
}

/// Laplacian of linear f(x,y,z)=x+2*y+3*z should be ≈0.
///
/// Second partial derivatives of a linear function are all zero.
/// Tolerance 1e-4 accounts for two levels of numerical differentiation.
#[test]
fn laplacian_linear_field_near_zero() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + 2*y + 3*z
    let two_y = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let three_z = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(3.0), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            two_y,
            Type::Real,
        ),
        three_z,
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

    // laplacian(field) → scalar field ≈ 0
    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(Type::Real),
        },
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of linear field should return a Field, got {:?}",
        lap_result
    );

    // sample at (1, 2, 3)
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let lap_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::Real),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(lap_result, lap_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!("laplacian sample should be numeric, got {:?}", sample_result)
    });
    assert!(
        val.abs() < 1e-4,
        "laplacian of linear field should be ≈0, got {}",
        val
    );
}

// ── Step 9: Dimensional correctness tests ─────────────────────────────────────

/// Divergence of a Point{3,Length} → Vector{3,Velocity} field has codomain
/// dimension Velocity/Length = (Length/Time)/Length = 1/Time.
///
/// This verifies that compute_divergence correctly derives the result codomain
/// dimension from the input field's domain and codomain component dimensions,
/// rather than unconditionally returning Type::Real.
#[test]
fn divergence_dimensional_correctness() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // VELOCITY = LENGTH / TIME  (derived dimension)
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_quantity = Type::Scalar {
        dimension: velocity_dim,
    };

    let domain_type = Type::point3(domain_quantity.clone());
    let codomain_type = Type::vec3(codomain_quantity.clone());

    // Lambda: |x, y, z| vec3(x, y, z) — simple identity used only for
    // metadata test; we do not sample from this field.
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    // divergence(field) → scalar field with codomain = Velocity/Length = 1/Time
    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of Point{{3,Length}}→Vector{{3,Velocity}} should return a Field, got {:?}",
        div_result
    );

    // Verify codomain dimension: should be Velocity / Length = 1/Time
    if let Value::Field { codomain_type, .. } = &div_result {
        let expected_dim = velocity_dim.div(&DimensionVector::LENGTH);
        match codomain_type {
            Type::Scalar { dimension } => {
                assert_eq!(
                    *dimension,
                    expected_dim,
                    "divergence codomain should be Velocity/Length=1/Time ({:?}), got {:?}",
                    expected_dim,
                    dimension
                );
            }
            other => panic!(
                "divergence_dimensional_correctness: expected Type::Scalar codomain, got {:?}",
                other
            ),
        }
    }
}

/// Laplacian of a Point{3,Length} → Scalar<Temperature> field has codomain
/// dimension Temperature/Length² = Temperature.div(&LENGTH.pow(2)).
///
/// This verifies that compute_laplacian correctly derives the result codomain
/// dimension by dividing the input codomain dimension by domain_dim², rather
/// than preserving the input codomain_type unchanged.
#[test]
fn laplacian_dimensional_correctness() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };

    let domain_type = Type::point3(domain_quantity.clone());

    // Lambda: |x, y, z| x + y + z — simple body used only for metadata test;
    // we do not sample from this field.
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

    // laplacian(field) → scalar field with codomain = Temperature / Length²
    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Length}}→Scalar<Temperature> should return a Field, got {:?}",
        lap_result
    );

    // Verify codomain dimension: should be Temperature / Length²
    if let Value::Field { codomain_type, .. } = &lap_result {
        let expected_dim =
            DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH.pow(2));
        match codomain_type {
            Type::Scalar { dimension } => {
                assert_eq!(
                    *dimension,
                    expected_dim,
                    "laplacian codomain should be Temperature/Length² ({:?}), got {:?}",
                    expected_dim,
                    dimension
                );
            }
            other => panic!(
                "laplacian_dimensional_correctness: expected Type::Scalar codomain, got {:?}",
                other
            ),
        }
    }
}

/// Divergence of a dimensionless Point{3,Real} → Vector{3,Real} field still
/// returns Type::Real as the result codomain (regression guard).
///
/// Ensures the fallback path in compute_divergence does not break the existing
/// behaviour for dimensionless fields now that the dimensioned path is wired up.
#[test]
fn divergence_dimensionless_still_real() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

    // Lambda: |x, y, z| vec3(x, y, z)
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of dimensionless field should return a Field, got {:?}",
        div_result
    );

    if let Value::Field { codomain_type, .. } = &div_result {
        assert_eq!(
            *codomain_type,
            Type::Real,
            "divergence of dimensionless Point{{3,Real}}→Vector{{3,Real}} should have codomain Type::Real, got {:?}",
            codomain_type
        );
    }
}

/// Laplacian of a dimensionless Point{3,Real} → Real field still returns
/// Type::Real as the result codomain (regression guard).
///
/// Ensures the fallback path in compute_laplacian does not break the existing
/// behaviour for dimensionless fields now that the dimensioned path is wired up.
#[test]
fn laplacian_dimensionless_still_real() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::Real;

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

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of dimensionless field should return a Field, got {:?}",
        lap_result
    );

    if let Value::Field { codomain_type, .. } = &lap_result {
        assert_eq!(
            *codomain_type,
            Type::Real,
            "laplacian of dimensionless Point{{3,Real}}→Real should have codomain Type::Real, got {:?}",
            codomain_type
        );
    }
}

// ── Step 9: Mixed-dim divergence fallback tests ───────────────────────────────

/// Divergence of Point{3, Real} → Vector{3, Scalar<Length>}: the domain is
/// dimensionless (Real), so the unified preserve-codomain strategy should
/// preserve the Vector's component type (Scalar<Length>) as the result codomain.
///
/// Under the old divergence fallback (`_ => Type::Real`), this test fails.
#[test]
fn divergence_real_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let component_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_type = Type::vec3(component_type.clone());

    // Lambda body unused (metadata-only test).
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of Point{{3,Real}}→Vector{{3,Length}} should return a Field, got {:?}",
        div_result
    );

    if let Value::Field { codomain_type, .. } = &div_result {
        assert_eq!(
            *codomain_type,
            component_type,
            "divergence of Point{{3,Real}}→Vector{{3,Length}} should preserve codomain Scalar<Length>, got {:?}",
            codomain_type
        );
    }
}

/// Divergence of Point{3, Scalar<Length>} → Vector{3, Real}: the codomain
/// component is dimensionless (Real), so the result codomain is Real.
///
/// Under the unified preserve-codomain strategy this coincides with the current
/// fallback, but this test documents the intended behavior explicitly.
#[test]
fn divergence_dim_domain_preserves_real_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain_type = Type::point3(domain_quantity);
    let codomain_type = Type::vec3(Type::Real);

    // Lambda body unused (metadata-only test).
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of Point{{3,Length}}→Vector{{3,Real}} should return a Field, got {:?}",
        div_result
    );

    if let Value::Field { codomain_type, .. } = &div_result {
        assert_eq!(
            *codomain_type,
            Type::Real,
            "divergence of Point{{3,Length}}→Vector{{3,Real}} should preserve codomain Real, got {:?}",
            codomain_type
        );
    }
}

/// Divergence of Point{3, Int} → Vector{3, Scalar<Length>}: Int is treated as
/// dimensionless, so the unified preserve-codomain strategy preserves Scalar<Length>.
///
/// Under the old divergence fallback (`_ => Type::Real`), this test fails.
#[test]
fn divergence_int_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Int);
    let component_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_type = Type::vec3(component_type.clone());

    // Lambda body unused (metadata-only test).
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of Point{{3,Int}}→Vector{{3,Length}} should return a Field, got {:?}",
        div_result
    );

    if let Value::Field { codomain_type, .. } = &div_result {
        assert_eq!(
            *codomain_type,
            component_type,
            "divergence of Point{{3,Int}}→Vector{{3,Length}} should preserve codomain Scalar<Length>, got {:?}",
            codomain_type
        );
    }
}

// ── Step 10: Mixed-dim laplacian fallback tests ───────────────────────────────

/// Laplacian of Point{3, Real} → Scalar<Length>: domain is dimensionless (Real),
/// so the preserve-codomain strategy preserves Scalar<Length> unchanged.
///
/// This already coincides with the current `_ => codomain_type.clone()` fallback,
/// but documents the intended behavior under the unified strategy.
#[test]
fn laplacian_real_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Real}}→Scalar<Length> should return a Field, got {:?}",
        lap_result
    );

    if let Value::Field { codomain_type, .. } = &lap_result {
        assert_eq!(
            *codomain_type,
            Type::Scalar { dimension: DimensionVector::LENGTH },
            "laplacian of Point{{3,Real}}→Scalar<Length> should preserve Scalar<Length>, got {:?}",
            codomain_type
        );
    }
}

/// Laplacian of Point{3, Scalar<Length>} → Real: codomain is dimensionless (Real),
/// so the result codomain is Real.
///
/// This already coincides with the current fallback but documents intended behavior.
#[test]
fn laplacian_dim_domain_preserves_real_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain_type = Type::point3(domain_quantity);
    let codomain_type = Type::Real;

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Length}}→Real should return a Field, got {:?}",
        lap_result
    );

    if let Value::Field { codomain_type, .. } = &lap_result {
        assert_eq!(
            *codomain_type,
            Type::Real,
            "laplacian of Point{{3,Length}}→Real should preserve codomain Real, got {:?}",
            codomain_type
        );
    }
}

/// Laplacian of Point{3, Scalar<Length>} → Scalar{DIMENSIONLESS}: the codomain is
/// explicitly dimensionless, so the result should be downgraded to Type::Real.
///
/// The current fallback (`_ => codomain_type.clone()`) returns Scalar{DIMENSIONLESS}
/// instead of Real — this test exposes that bug.
#[test]
fn laplacian_explicit_dimensionless_scalar_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain_type = Type::point3(domain_quantity);
    // Explicitly-dimensionless Scalar (not Type::Real, but Scalar<DIMENSIONLESS>).
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Length}}→Scalar{{DIMENSIONLESS}} should return a Field, got {:?}",
        lap_result
    );

    if let Value::Field { codomain_type, .. } = &lap_result {
        assert_eq!(
            *codomain_type,
            Type::Real,
            "laplacian of Point{{3,Length}}→Scalar{{DIMENSIONLESS}} should downgrade codomain to Real, got {:?}",
            codomain_type
        );
    }
}

/// Laplacian of Point{3, Int} → Scalar<Length>: Int is treated as dimensionless,
/// so the preserve-codomain strategy preserves Scalar<Length>.
///
/// This already coincides with the current fallback but documents intended behavior.
#[test]
fn laplacian_int_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Int);
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Int}}→Scalar<Length> should return a Field, got {:?}",
        lap_result
    );

    if let Value::Field { codomain_type, .. } = &lap_result {
        assert_eq!(
            *codomain_type,
            Type::Scalar { dimension: DimensionVector::LENGTH },
            "laplacian of Point{{3,Int}}→Scalar<Length> should preserve Scalar<Length>, got {:?}",
            codomain_type
        );
    }
}

// ── Step 11: Mixed-dim gradient fallback tests ────────────────────────────────

/// Gradient of Point{3, Real} → Scalar<Length>: domain is dimensionless (Real),
/// so the preserve-codomain strategy returns Vector{3, Scalar<Length>}.
///
/// Gradient already handles this correctly via its fallback arm.
/// This test documents the behavior and serves as a regression guard.
#[test]
fn gradient_real_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Point{{3,Real}}→Scalar<Length> should return a Field, got {:?}",
        grad_result
    );

    if let Value::Field { codomain_type, .. } = &grad_result {
        let expected = Type::Vector {
            n: 3,
            quantity: Box::new(Type::Scalar { dimension: DimensionVector::LENGTH }),
        };
        assert_eq!(
            *codomain_type,
            expected,
            "gradient of Point{{3,Real}}→Scalar<Length> should have codomain Vector{{3,Scalar<Length>}}, got {:?}",
            codomain_type
        );
    }
}

/// Gradient of Point{3, Int} → Scalar<Length>: Int is treated as dimensionless,
/// so the preserve-codomain strategy returns Vector{3, Scalar<Length>}.
///
/// Gradient already handles this correctly via its fallback arm.
/// This test documents the behavior and serves as a regression guard.
#[test]
fn gradient_int_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Int);
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

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

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Point{{3,Int}}→Scalar<Length> should return a Field, got {:?}",
        grad_result
    );

    if let Value::Field { codomain_type, .. } = &grad_result {
        let expected = Type::Vector {
            n: 3,
            quantity: Box::new(Type::Scalar { dimension: DimensionVector::LENGTH }),
        };
        assert_eq!(
            *codomain_type,
            expected,
            "gradient of Point{{3,Int}}→Scalar<Length> should have codomain Vector{{3,Scalar<Length>}}, got {:?}",
            codomain_type
        );
    }
}
