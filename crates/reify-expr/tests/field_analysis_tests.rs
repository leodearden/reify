//! Field-level analysis wrapper tests.
//!
//! Tests for stress analysis field operators (von_mises, principal_stresses,
//! max_shear, safety_factor) that wrap tensor fields and apply pointwise
//! analysis when sampled.

use std::sync::Arc;

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, Value, ValueMap};

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
        lambda: Arc::new(lambda),
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
        panic!("von_mises(Field) should return a Field, got {:?}", result);
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
        matches!(
            lambda.as_ref(),
            Value::Field {
                source: FieldSourceKind::Analytical,
                ..
            }
        ),
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
        Value::Scalar {
            si_value,
            dimension,
        } => {
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
        Value::Scalar {
            si_value,
            dimension,
        } => {
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

// ── Step 21: principal_stresses, max_shear, safety_factor on Field ──────────

/// Helper: build an analysis field wrapper and verify its metadata.
fn assert_analysis_wrapper(
    op_name: &str,
    expected_source: FieldSourceKind,
    expected_codomain: Type,
) {
    let sigma = 100e6;
    let tensor = make_stress_tensor(
        &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);

    let result_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(expected_codomain.clone()),
    };

    let args = vec![CompiledExpr::literal(field, field_type)];
    let expr = make_function_call(op_name, args, result_type);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    let Value::Field {
        domain_type,
        codomain_type,
        source,
        lambda,
    } = &result
    else {
        panic!("{op_name}(Field) should return a Field, got {:?}", result);
    };

    assert_eq!(
        *domain_type,
        Type::point3(Type::Real),
        "{op_name}: domain should be Point3(Real)"
    );
    assert_eq!(
        *codomain_type, expected_codomain,
        "{op_name}: codomain mismatch"
    );
    assert_eq!(*source, expected_source, "{op_name}: source kind mismatch");
    assert!(
        matches!(
            lambda.as_ref(),
            Value::Field {
                source: FieldSourceKind::Analytical,
                ..
            }
        ),
        "{op_name}: lambda slot should contain original analytical field"
    );
}

#[test]
fn principal_stresses_field_returns_field_with_correct_source() {
    // principal_stresses sampling returns a Value::List of 3 scalars, so
    // the codomain type must be Type::List(Box<Scalar<Q>>).
    assert_analysis_wrapper(
        "principal_stresses",
        FieldSourceKind::PrincipalStresses,
        Type::List(Box::new(pressure_scalar_type())),
    );
}

#[test]
fn max_shear_field_returns_field_with_correct_source() {
    assert_analysis_wrapper(
        "max_shear",
        FieldSourceKind::MaxShear,
        pressure_scalar_type(),
    );
}

#[test]
fn safety_factor_field_returns_field_with_correct_source() {
    // safety_factor takes 2 args: tensor field + yield_strength scalar
    let sigma = 100e6;
    let tensor = make_stress_tensor(
        &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);

    let yield_strength = Value::Scalar {
        si_value: 250e6,
        dimension: DimensionVector::PRESSURE,
    };

    let result_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(Type::Real),
    };

    let expr = make_function_call(
        "safety_factor",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(yield_strength, pressure_scalar_type()),
        ],
        result_type,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    let Value::Field {
        domain_type,
        codomain_type,
        source,
        ..
    } = &result
    else {
        panic!(
            "safety_factor(Field, Scalar) should return a Field, got {:?}",
            result
        );
    };

    assert_eq!(*domain_type, Type::point3(Type::Real));
    assert_eq!(*source, FieldSourceKind::SafetyFactor);
    // Safety factor is dimensionless (yield / von_mises cancels PRESSURE dims)
    assert_eq!(*codomain_type, Type::Real);
}

// ── Sampling tests for principal_stresses, max_shear, safety_factor ─────────

#[test]
fn sample_principal_stresses_field_diagonal_returns_sorted_list() {
    // Diagonal tensor [[100,0,0],[0,50,0],[0,0,25]] → sorted [25, 50, 100]
    let tensor = make_stress_tensor(
        &[&[100.0, 0.0, 0.0], &[0.0, 50.0, 0.0], &[0.0, 0.0, 25.0]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);

    let ps_field_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(Type::List(Box::new(pressure_scalar_type()))),
    };
    let ps_expr = make_function_call(
        "principal_stresses",
        vec![CompiledExpr::literal(field, field_type)],
        ps_field_type.clone(),
    );

    let values = ValueMap::new();
    let ps_field = eval_expr(&ps_expr, &EvalContext::simple(&values));

    let sample_point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(ps_field, ps_field_type),
            CompiledExpr::literal(sample_point, Type::point3(Type::Real)),
        ],
        Type::List(Box::new(pressure_scalar_type())),
    );

    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let Value::List(items) = &result else {
        panic!(
            "sample(principal_stresses(field), pt) should return List, got {:?}",
            result
        );
    };

    assert_eq!(items.len(), 3, "should have 3 principal stresses");
    let expected = [25.0, 50.0, 100.0];
    for (i, (item, &exp)) in items.iter().zip(expected.iter()).enumerate() {
        match item {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(*dimension, DimensionVector::PRESSURE);
                assert!(
                    (si_value - exp).abs() < 1e-6,
                    "principal stress {i}: expected {exp}, got {si_value}"
                );
            }
            _ => panic!("principal stress {i} should be Scalar, got {:?}", item),
        }
    }
}

#[test]
fn sample_max_shear_field_uniaxial_returns_half_sigma() {
    // Uniaxial [[σ,0,0],[0,0,0],[0,0,0]] → max_shear = σ/2
    let sigma = 200.0_f64;
    let tensor = make_stress_tensor(
        &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);

    let ms_field_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(pressure_scalar_type()),
    };
    let ms_expr = make_function_call(
        "max_shear",
        vec![CompiledExpr::literal(field, field_type)],
        ms_field_type.clone(),
    );

    let values = ValueMap::new();
    let ms_field = eval_expr(&ms_expr, &EvalContext::simple(&values));

    let sample_point = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(ms_field, ms_field_type),
            CompiledExpr::literal(sample_point, Type::point3(Type::Real)),
        ],
        pressure_scalar_type(),
    );

    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    match &result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(*dimension, DimensionVector::PRESSURE);
            let expected = sigma / 2.0;
            assert!(
                (si_value - expected).abs() < 1e-6,
                "max_shear of uniaxial: expected {expected}, got {si_value}"
            );
        }
        _ => panic!(
            "sample(max_shear(field), pt) should return Scalar, got {:?}",
            result
        ),
    }
}

#[test]
fn sample_safety_factor_field_returns_yield_over_von_mises() {
    // Uniaxial stress=100e6: von_mises = 100e6
    // yield_strength = 250e6 → safety_factor = 2.5
    let sigma = 100e6_f64;
    let tensor = make_stress_tensor(
        &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);

    let yield_val = Value::Scalar {
        si_value: 250e6,
        dimension: DimensionVector::PRESSURE,
    };

    let sf_field_type = Type::Field {
        domain: Box::new(Type::point3(Type::Real)),
        codomain: Box::new(Type::Real),
    };
    let sf_expr = make_function_call(
        "safety_factor",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(yield_val, pressure_scalar_type()),
        ],
        sf_field_type.clone(),
    );

    let values = ValueMap::new();
    let sf_field = eval_expr(&sf_expr, &EvalContext::simple(&values));

    let sample_point = Value::Point(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(sf_field, sf_field_type),
            CompiledExpr::literal(sample_point, Type::point3(Type::Real)),
        ],
        Type::Real,
    );

    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    match &result {
        Value::Real(v) => {
            assert!(
                (v - 2.5).abs() < 1e-6,
                "safety_factor: expected 2.5, got {v}"
            );
        }
        _ => panic!(
            "sample(safety_factor(field, yield), pt) should return Real, got {:?}",
            result
        ),
    }
}
