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

// ── Step 10: Sample-level dimensional correctness tests ───────────────────────

/// Regression guard: sampling from the divergence of a dimensionless
/// Point{3,Real}→Vector{3,Real} field returns `Value::Real`, not `Value::Scalar`.
///
/// Locks in the dimensionless fallback path in compute_numerical_divergence_at_point
/// so the step-3 implementation change cannot regress it.
#[test]
fn divergence_sample_dimensionless_returns_real() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(1.0, 1.0, 1.0) (constant vector field, same as
    // divergence_constant_field_near_zero)
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
        "divergence of dimensionless constant field should return a Field, got {:?}",
        div_result
    );

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

    match &sample_result {
        Value::Real(_) => {} // ← correct: dimensionless fallback returns Real
        Value::Scalar { .. } => panic!(
            "divergence_sample_dimensionless_returns_real: expected Value::Real but got \
             Value::Scalar — the dimensionless fallback path is broken: {:?}",
            sample_result
        ),
        other => panic!(
            "divergence_sample_dimensionless_returns_real: expected Value::Real, got {:?}",
            other
        ),
    }
}

/// Runtime drift test: sampling from the divergence of a dimensioned
/// Point{3,Length}→Vector{3,Velocity} field should return
/// `Value::Scalar { dimension: 1/Time }`, not `Value::Real`.
///
/// Lambda: |x, y, z| vec3(x, y, z) — identity field, divergence = 3.0.
/// Expected result dimension: Velocity/Length = (Length/Time)/Length = 1/Time.
///
/// FAILS before step-3 implementation because compute_numerical_divergence_at_point
/// returns Value::Real unconditionally.
#[test]
fn divergence_sample_dimensional_correctness_returns_scalar() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // VELOCITY = LENGTH / TIME
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_quantity = Type::Scalar {
        dimension: velocity_dim,
    };

    let domain_type = Type::point3(domain_quantity.clone());
    let codomain_type = Type::vec3(codomain_quantity.clone());

    // Lambda: |x, y, z| vec3(x, y, z) — identity (same structure as
    // divergence_dimensional_correctness at line 1402, but we sample from it).
    // Value refs use Type::Real annotations; at runtime they receive Scalar[LENGTH]
    // args (the "trust the declaration" pattern).
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
        "divergence of Point{{3,Length}}→Vector{{3,Velocity}} should return a Field, got {:?}",
        div_result
    );

    // Sample at (1m, 2m, 3m)
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

    // The divergence field's codomain is Scalar[Velocity/Length = 1/Time]
    let one_over_time = velocity_dim.div(&DimensionVector::LENGTH);
    let div_codomain = Type::Scalar {
        dimension: one_over_time,
    };
    let div_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(div_codomain),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(div_result, div_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Scalar {
            dimension: one_over_time,
        },
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // divergence of |x,y,z| vec3(x,y,z) = ∂x/∂x + ∂y/∂y + ∂z/∂z = 3.0
    match sample_result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension,
                one_over_time,
                "divergence sample dimension should be 1/Time ({:?}), got {:?}",
                one_over_time,
                dimension,
            );
            assert!(
                (si_value - 3.0).abs() < 1e-4,
                "divergence of identity field should be ≈3.0, got {}",
                si_value
            );
        }
        Value::Real(_) => panic!(
            "divergence_sample_dimensional_correctness_returns_scalar: \
             expected Value::Scalar but got Value::Real — runtime drift not fixed"
        ),
        other => panic!(
            "divergence_sample_dimensional_correctness_returns_scalar: \
             expected Value::Scalar, got {:?}",
            other
        ),
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

/// Regression guard: sampling from the Laplacian of a dimensionless
/// Point{3,Real}→Real field returns `Value::Real`, not `Value::Scalar`.
///
/// Locks in the dimensionless fallback path in compute_numerical_laplacian_at_point
/// so the step-6 implementation change cannot regress it.
#[test]
fn laplacian_sample_dimensionless_returns_real() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + 2*y + 3*z (same as laplacian_linear_field_near_zero)
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
        "laplacian of dimensionless linear field should return a Field, got {:?}",
        lap_result
    );

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

    match &sample_result {
        Value::Real(_) => {} // ← correct: dimensionless fallback returns Real
        Value::Scalar { .. } => panic!(
            "laplacian_sample_dimensionless_returns_real: expected Value::Real but got \
             Value::Scalar — the dimensionless fallback path is broken: {:?}",
            sample_result
        ),
        other => panic!(
            "laplacian_sample_dimensionless_returns_real: expected Value::Real, got {:?}",
            other
        ),
    }
}

/// Runtime drift test: sampling from the Laplacian of a dimensioned
/// Point{3,Length}→Scalar<Temperature> field should return
/// `Value::Scalar { dimension: Temperature/Length² }`, not `Value::Real`.
///
/// Lambda: |x, y, z| x*x + y*y + z*z — Laplacian = 6.0 (constant).
/// Expected result dimension: Temperature / Length².
///
/// FAILS before step-6 implementation because compute_numerical_laplacian_at_point
/// returns Value::Real unconditionally.
#[test]
fn laplacian_sample_dimensional_correctness_returns_scalar() {
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

    // Lambda: |x, y, z| x*x + y*y + z*z.
    // Value refs use Type::Real annotations; at runtime they receive Scalar[LENGTH]
    // args (trust-the-declaration pattern). The Laplacian of x²+y²+z² is 6.
    let x_sq = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let y_sq = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let z_sq = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(BinOp::Add, x_sq, y_sq, Type::Real),
        z_sq,
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
        "laplacian of Point{{3,Length}}→Scalar<Temperature> should return a Field, got {:?}",
        lap_result
    );

    // Sample at (1m, 1m, 1m)
    let point = Value::Point(vec![
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);

    // The laplacian field's codomain is Scalar[Temperature/Length²]
    let temp_per_len_sq = DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH.pow(2));
    let lap_codomain = Type::Scalar {
        dimension: temp_per_len_sq,
    };
    let lap_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(lap_codomain),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(lap_result, lap_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Scalar {
            dimension: temp_per_len_sq,
        },
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Laplacian of x²+y²+z² = ∂²/∂x²(x²) + ∂²/∂y²(y²) + ∂²/∂z²(z²) = 2+2+2 = 6.0
    match sample_result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension,
                temp_per_len_sq,
                "laplacian sample dimension should be Temperature/Length² ({:?}), got {:?}",
                temp_per_len_sq,
                dimension,
            );
            assert!(
                (si_value - 6.0).abs() < 1e-3,
                "laplacian of x²+y²+z² should be ≈6.0, got {}",
                si_value
            );
        }
        Value::Real(_) => panic!(
            "laplacian_sample_dimensional_correctness_returns_scalar: \
             expected Value::Scalar but got Value::Real — runtime drift not fixed"
        ),
        other => panic!(
            "laplacian_sample_dimensional_correctness_returns_scalar: \
             expected Value::Scalar, got {:?}",
            other
        ),
    }
}

// ── Step 10: Edge-case Undef return paths ─────────────────────────────────────

/// divergence(Real) returns Undef when the argument is not a Field.
///
/// Mirrors gradient_non_field_returns_undef in lambda_eval_tests.rs. Exercises the
/// first early-return guard in compute_divergence (lib.rs:732–739).
#[test]
fn divergence_non_field_returns_undef() {
    let expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(Value::Real(1.0), Type::Real)],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "divergence of non-Field must return Undef"
    );
}

/// curl(Field<Point2, Vec2>) returns Undef — curl requires a 3D domain.
///
/// compute_curl hardwires `Type::Point { n: 3, .. }` in the domain check
/// (lib.rs:852). A 2D vector field (n=2) fails that arm and returns Undef.
/// Mirrors the domain-dimension guard tests in gradient_tests.rs.
#[test]
fn curl_2d_vector_field_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");

    // Lambda: |x, y| vec2(-y, x)
    let neg_y = CompiledExpr::unop(
        UnOp::Neg,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let body = make_function_call(
        "vec2",
        vec![
            neg_y,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
        ],
        Type::vec2(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point2(Type::Real);
    let codomain_type = Type::vec2(Type::Real);

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

    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real, // result type doesn't matter — we expect Undef
    );

    let values = ValueMap::new();
    let result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&result, Value::Undef),
        "curl of 2D vector field must return Undef (curl requires 3D domain), got {:?}",
        result
    );
}

/// divergence(gradient(f)) returns Undef — gradient-sourced fields are not
/// accepted by compute_divergence's source-kind whitelist.
///
/// Build a 1D analytical field, produce a Gradient-sourced field via gradient(),
/// then pass that to divergence. The source check fires before domain/codomain
/// validation, so Undef is returned immediately. Mirrors
/// gradient_of_gradient_field_returns_undef in gradient_tests.rs.
#[test]
fn divergence_gradient_field_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x*x
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

    // gradient(field) — should succeed and produce a Gradient-sourced field
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real, // result type doesn't matter here
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "first gradient should return a Field, got {:?}",
        grad_result
    );

    // divergence(gradient_field) — source=Gradient not in {Analytical, Composed},
    // so compute_divergence returns Undef immediately (lib.rs:742–747).
    let grad_field_type = Type::Field {
        domain: Box::new(domain_type),
        codomain: Box::new(Type::Real),
    };

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(grad_result, grad_field_type)],
        Type::Real, // result type doesn't matter — we expect Undef
    );

    let result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&result, Value::Undef),
        "divergence(gradient(f)) must return Undef, got {:?}",
        result
    );
}

/// divergence(Field<Point3, Vec2>) returns Undef — domain dim 3 ≠ codomain dim 2.
///
/// compute_divergence validates that vec_n (codomain dimension) equals n (domain
/// dimension) before constructing the result field (lib.rs:795–801). A field
/// mapping R³ → Vec2 fails that guard and returns Undef.
#[test]
fn divergence_field_with_mismatched_dims_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec2(x, y)
    let body = make_function_call(
        "vec2",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
        ],
        Type::vec2(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);   // n=3
    let codomain_type = Type::vec2(Type::Real);   // n=2 — mismatch!

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

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real, // result type doesn't matter — we expect Undef
    );

    let values = ValueMap::new();
    let result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&result, Value::Undef),
        "divergence of Field<Point3, Vec2> (mismatched dims) must return Undef, got {:?}",
        result
    );
}

// ── Step 11: Curl irrotational + 1D laplacian coverage ───────────────────────

/// Curl of the conservative field F(x,y,z)=[x,y,z] at (1,2,3) ≈ [0,0,0].
///
/// F = ∇φ where φ(x,y,z) = (x²+y²+z²)/2 — a gradient field is always
/// irrotational, so curl(F) ≡ 0 analytically. Numerical central differences
/// should give near-zero within 1e-3.
///
/// Note: divergence(F)=3 everywhere on the same field (see divergence_identity_vector_field).
/// These two tests together confirm both operators on the same lambda.
#[test]
fn curl_irrotational_field_near_zero() {
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
        "curl of irrotational field should return a Field, got {:?}",
        curl_result
    );

    // sample(curl_field, Point3(1.0, 2.0, 3.0)) — expect ≈ [0, 0, 0]
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

    assert_gradient_vector(
        &sample_result,
        &[0.0, 0.0, 0.0],
        1e-3,
        "curl of conservative field [x,y,z] at (1,2,3)",
    );
}

/// Laplacian of the 1D quadratic f(x) = x*x at x=3.0 ≈ 2.0.
///
/// d²/dx²(x²) = 2 at every x. Domain is Type::Real (bare scalar, not Point),
/// which exercises the `Type::Real` arm in compute_laplacian (lib.rs:933) and
/// the `Value::Real(r) if r.is_finite() => vec![*r]` coords-extraction arm in
/// compute_numerical_laplacian_at_point (lib.rs:1627). With n=1 and a single
/// lambda param, single_point_param = (1==1 && 1>1) = false, so the decomposed
/// per-axis path runs with n=1.
#[test]
fn laplacian_1d_quadratic_accuracy() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x*x
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
        "laplacian of 1D quadratic should return a Field, got {:?}",
        lap_result
    );

    // sample(lap_field, Value::Real(3.0)) — d²/dx²(x²) = 2 at every x
    let lap_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(Type::Real),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(lap_result, lap_field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!("laplacian sample should be numeric, got {:?}", sample_result)
    });
    assert!(
        (val - 2.0).abs() < 1e-2,
        "laplacian of x*x at x=3.0 should be ≈2.0, got {}",
        val
    );
}
