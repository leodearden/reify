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
