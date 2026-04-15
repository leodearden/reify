//! Field-level analysis wrapper tests.
//!
//! Tests for stress analysis field operators (von_mises, principal_stresses,
//! max_shear, safety_factor) that wrap tensor fields and apply pointwise
//! analysis when sampled.

use reify_expr::{EvalContext, eval_expr};
use reify_types::{
    CompiledExpr, CompiledExprKind, ContentHash, DimensionVector, FieldSourceKind,
    ResolvedFunction, Type, Value, ValueCellId, ValueMap,
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

/// Build a `Value::Field` / `Type::Field` pair with an explicit source kind.
fn make_field_with_source(
    domain: Type,
    codomain: Type,
    source: FieldSourceKind,
    lambda: Value,
) -> (Value, Type) {
    let field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source,
        lambda: Box::new(lambda),
    };
    let field_type = Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(codomain),
    };
    (field, field_type)
}

/// Build an analytical `Value::Field` / `Type::Field` pair.
fn make_analytical_field(domain: Type, codomain: Type, lambda: Value) -> (Value, Type) {
    make_field_with_source(domain, codomain, FieldSourceKind::Analytical, lambda)
}

/// Build a 3×3 dimensioned stress tensor as `Value::Tensor`.
fn make_stress_tensor(rows: &[&[f64]], dim: DimensionVector) -> Value {
    Value::Tensor(
        rows.iter()
            .map(|row| {
                Value::Tensor(
                    row.iter()
                        .map(|&v| Value::Scalar {
                            si_value: v,
                            dimension: dim,
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

/// The PRESSURE dimension type.
fn pressure_scalar_type() -> Type {
    Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    }
}

/// A 3×3 matrix type with PRESSURE-dimensioned elements.
fn pressure_matrix_type() -> Type {
    Type::Matrix {
        m: 3,
        n: 3,
        quantity: Box::new(pressure_scalar_type()),
    }
}

/// Build a constant-tensor-returning analytical field.
///
/// Creates a field F: Point3(Real) → Matrix3x3(Scalar[PRESSURE]) where the
/// lambda ignores the input coordinates and returns a constant stress tensor.
fn make_constant_stress_field(tensor: Value) -> (Value, Type) {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let body = CompiledExpr::literal(tensor, pressure_matrix_type());
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain = Type::point3(Type::Real);
    let codomain = pressure_matrix_type();
    make_analytical_field(domain, codomain, lambda)
}

// ── Step 17: von_mises on Field creates VonMises-wrapped field ──────────────

#[test]
fn von_mises_field_returns_field_with_von_mises_source() {
    // Uniaxial stress tensor [[100e6, 0, 0], [0, 0, 0], [0, 0, 0]]
    let sigma = 100e6;
    let tensor = make_stress_tensor(
        &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);

    // Call von_mises(field)
    let result_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(pressure_scalar_type()),
    };
    let vm_expr = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        result_type,
    );

    let values = ValueMap::new();
    let result = eval_expr(&vm_expr, &EvalContext::simple(&values));

    // Should return a Field, not Undef
    let Value::Field {
        domain_type,
        codomain_type,
        source,
        ..
    } = &result
    else {
        panic!(
            "von_mises(Field) should return a Field, got {:?}",
            result
        );
    };

    // Domain preserved: Point3(Real)
    assert_eq!(
        *domain_type,
        Type::point3(Type::Real),
        "domain should be Point3(Real)"
    );

    // Codomain: Scalar with PRESSURE dimension (same as tensor elements)
    assert_eq!(
        *codomain_type,
        pressure_scalar_type(),
        "codomain should be Scalar[PRESSURE]"
    );

    // Source kind: VonMises
    assert_eq!(
        *source,
        FieldSourceKind::VonMises,
        "source should be VonMises"
    );
}

#[test]
fn von_mises_field_stores_original_field_in_lambda_slot() {
    let tensor = make_stress_tensor(
        &[&[50e6, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);

    let result_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(pressure_scalar_type()),
    };
    let vm_expr = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        result_type,
    );

    let values = ValueMap::new();
    let result = eval_expr(&vm_expr, &EvalContext::simple(&values));

    // The lambda slot should contain the original Field (not a Lambda)
    let Value::Field { lambda, .. } = &result else {
        panic!("von_mises(Field) should return a Field, got {:?}", result);
    };

    assert!(
        matches!(lambda.as_ref(), Value::Field { source: FieldSourceKind::Analytical, .. }),
        "lambda slot should contain the original analytical field, got {:?}",
        lambda
    );
}

// ── Step 19: sampling a VonMises-wrapped field ──────────────────────────────

#[test]
fn sample_von_mises_field_uniaxial_returns_sigma() {
    // Uniaxial stress [[σ,0,0],[0,0,0],[0,0,0]]: von Mises = σ
    let sigma = 100e6_f64;
    let tensor = make_stress_tensor(
        &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);

    // Build: von_mises(field)
    let vm_field_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(pressure_scalar_type()),
    };
    let vm_expr = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        vm_field_type.clone(),
    );

    let values = ValueMap::new();
    let vm_field = eval_expr(&vm_expr, &EvalContext::simple(&values));

    // Build: sample(vm_field, Point3(1.0, 2.0, 3.0))
    let sample_point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(vm_field, vm_field_type),
            CompiledExpr::literal(sample_point, Type::point3(Type::Real)),
        ],
        pressure_scalar_type(),
    );

    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Should return Scalar { si_value ≈ sigma, dimension: PRESSURE }
    match &result {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "result should have PRESSURE dimension"
            );
            assert!(
                (si_value - sigma).abs() < 1e-3,
                "expected ≈{sigma}, got {si_value}"
            );
        }
        _ => panic!(
            "sample(von_mises(field), point) should return Scalar, got {:?}",
            result
        ),
    }
}

#[test]
fn sample_von_mises_field_hydrostatic_returns_zero() {
    // Hydrostatic stress [[p,0,0],[0,p,0],[0,0,p]]: von Mises = 0
    let p = 100e6_f64;
    let tensor = make_stress_tensor(
        &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);

    let vm_field_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(pressure_scalar_type()),
    };
    let vm_expr = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        vm_field_type.clone(),
    );

    let values = ValueMap::new();
    let vm_field = eval_expr(&vm_expr, &EvalContext::simple(&values));

    let sample_point = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(vm_field, vm_field_type),
            CompiledExpr::literal(sample_point, Type::point3(Type::Real)),
        ],
        pressure_scalar_type(),
    );

    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    match &result {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(*dimension, DimensionVector::PRESSURE);
            assert!(
                si_value.abs() < 1e-6,
                "hydrostatic von Mises should be ≈0, got {si_value}"
            );
        }
        _ => panic!(
            "sample(von_mises(field), point) should return Scalar, got {:?}",
            result
        ),
    }
}
