//! Field-level analysis wrapper tests.
//!
//! Tests for stress analysis field operators (von_mises, principal_stresses,
//! max_shear, safety_factor) that wrap tensor fields and apply pointwise
//! analysis when sampled.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
use reify_ir::{
    CompiledExpr, CompiledExprKind, FieldSourceKind, InterpolationKind, ResolvedFunction,
    SampledField, SampledGridKind, Value, ValueMap,
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

    let domain = Type::point3(Type::dimensionless_scalar());
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
        domain: Box::new(Type::point3(Type::dimensionless_scalar())),
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
        Type::point3(Type::dimensionless_scalar()),
        "domain should be Point3(Real)"
    );

    // Codomain: Length with PRESSURE dimension (same as tensor elements)
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
        domain: Box::new(Type::point3(Type::dimensionless_scalar())),
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
        domain: Box::new(Type::point3(Type::dimensionless_scalar())),
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
            CompiledExpr::literal(sample_point, Type::point3(Type::dimensionless_scalar())),
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
        domain: Box::new(Type::point3(Type::dimensionless_scalar())),
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
            CompiledExpr::literal(sample_point, Type::point3(Type::dimensionless_scalar())),
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
        domain: Box::new(Type::point3(Type::dimensionless_scalar())),
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
        Type::point3(Type::dimensionless_scalar()),
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
        domain: Box::new(Type::point3(Type::dimensionless_scalar())),
        codomain: Box::new(Type::dimensionless_scalar()),
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

    assert_eq!(*domain_type, Type::point3(Type::dimensionless_scalar()));
    assert_eq!(*source, FieldSourceKind::SafetyFactor);
    // Safety factor is dimensionless (yield / von_mises cancels PRESSURE dims)
    assert_eq!(*codomain_type, Type::dimensionless_scalar());
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
        domain: Box::new(Type::point3(Type::dimensionless_scalar())),
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
            CompiledExpr::literal(sample_point, Type::point3(Type::dimensionless_scalar())),
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
        domain: Box::new(Type::point3(Type::dimensionless_scalar())),
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
            CompiledExpr::literal(sample_point, Type::point3(Type::dimensionless_scalar())),
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
        domain: Box::new(Type::point3(Type::dimensionless_scalar())),
        codomain: Box::new(Type::dimensionless_scalar()),
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
            CompiledExpr::literal(sample_point, Type::point3(Type::dimensionless_scalar())),
        ],
        Type::dimensionless_scalar(),
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

// ── Task 7129: analysis wrappers over a Sampled-backed tensor field ─────────
//
// `solve_elastic_static` returns its stress field as a
// `Value::Field { source: FieldSourceKind::Sampled, lambda: Value::SampledField(_) }`
// (`crates/reify-eval/src/compute_targets/mod.rs:108-125`, `sampled_stress_field`).
// Before this task `validate_tensor_field` (`analysis.rs`) accepted only
// `Analytical | Composed` sources with a `Value::Lambda` in the lambda slot, so
// all four analysis builtins returned `Value::Undef` over a real FEA stress
// field — while `field_reductions.rs` was already written to consume exactly
// the wrapper shape that `analysis.rs` refused to construct.
//
// The fixtures below are ported from `field_reductions_tests.rs:1968/2000/2018`
// (integration test files are separate binaries, so they must be copied, not
// imported).

/// Uniaxial window for a single principal stress σ: `[σ,0,0, 0,0,0, 0,0,0]`.
///
/// Closed forms for this window (all exactly representable in binary):
/// von Mises = |σ|, max_shear = σ/2, principal max = σ, principal min = 0.
fn uniaxial_window(sigma: f64) -> [f64; 9] {
    [sigma, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
}

/// Build a 1-D `SampledField` with stride-9 row-major tensor data.
///
/// Mirror of `field_reductions_tests.rs:1968`. Each `windows[i]` is a 9-float
/// 3×3 matrix stored row-major; axis coords are `axis[0..K]`.
fn make_sampled_tensor_1d(name: &str, axis: Vec<f64>, windows: Vec<[f64; 9]>) -> SampledField {
    assert_eq!(
        axis.len(),
        windows.len(),
        "axis and windows must have equal length"
    );
    let bounds_min = vec![*axis.first().expect("axis must be non-empty")];
    let bounds_max = vec![*axis.last().expect("axis must be non-empty")];
    let spacing = if axis.len() >= 2 {
        vec![axis[1] - axis[0]]
    } else {
        vec![1.0]
    };
    let mut data: Vec<f64> = Vec::with_capacity(windows.len() * 9);
    for w in &windows {
        data.extend_from_slice(w);
    }
    assert_eq!(
        data.len(),
        axis.len() * 9,
        "stride-9 tensor buffer must hold 9 floats per axis coordinate"
    );
    SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min,
        bounds_max,
        spacing,
        axis_grids: vec![axis],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    }
}

/// A `Tensor<2,3,Scalar[PRESSURE]>` codomain — byte-identical to the codomain
/// production `sampled_stress_field` stamps on `ElasticResult.stress`
/// (`crates/reify-eval/src/compute_targets/mod.rs:108-125`).
fn pressure_tensor_type() -> Type {
    Type::tensor(2, 3, pressure_scalar_type())
}

/// Wrap a stride-9 `SampledField` as the production stress-field shape:
/// `Value::Field { source: Sampled, lambda: Arc(Value::SampledField(sf)) }`
/// with a `Tensor<2,3,Pressure>` codomain.
fn wrap_sampled_stress_field(sf: SampledField) -> (Value, Type) {
    make_field_with_source(
        Type::dimensionless_scalar(),
        pressure_tensor_type(),
        FieldSourceKind::Sampled,
        Value::SampledField(sf),
    )
}

/// The canonical three-window uniaxial stress fixture: σ_xx = {100e6, 250e6,
/// 175e6} MPa at axis coords {0.0, 1.0, 2.0}.
fn sampled_stress_fixture() -> (Value, Type) {
    let sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(175e6),
        ],
    );
    wrap_sampled_stress_field(sf)
}

/// Evaluate a single-argument analysis builtin over `field`.
fn eval_analysis_wrapper(op: &str, field: Value, field_type: Type, codomain: Type) -> Value {
    let result_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(codomain),
    };
    let expr = make_function_call(
        op,
        vec![CompiledExpr::literal(field, field_type)],
        result_type,
    );
    let values = ValueMap::new();
    eval_expr(&expr, &EvalContext::simple(&values))
}

/// Evaluate `safety_factor(field, yield)`.
fn eval_safety_factor(field: Value, field_type: Type, yield_si: f64) -> Value {
    let yield_val = Value::Scalar {
        si_value: yield_si,
        dimension: DimensionVector::PRESSURE,
    };
    let result_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };
    let expr = make_function_call(
        "safety_factor",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(yield_val, pressure_scalar_type()),
        ],
        result_type,
    );
    let values = ValueMap::new();
    eval_expr(&expr, &EvalContext::simple(&values))
}

// ── (b) Wrapper construction over a Sampled tensor field ────────────────────

#[test]
fn von_mises_over_sampled_tensor_field_returns_vonmises_wrapped_field() {
    let (field, field_type) = sampled_stress_fixture();
    let expected_inner = field.clone();

    let result = eval_analysis_wrapper(
        "von_mises",
        field,
        field_type,
        pressure_scalar_type(),
    );

    assert_ne!(
        result,
        Value::Undef,
        "von_mises over a Sampled-backed stress field must not be Undef — \
         this is the silent false-green fixed by task 7129"
    );

    let Value::Field {
        domain_type,
        codomain_type,
        source,
        lambda,
    } = &result
    else {
        panic!("von_mises(Field{{Sampled}}) should return a Field, got {result:?}");
    };

    assert_eq!(
        *domain_type,
        Type::dimensionless_scalar(),
        "von_mises: domain must be preserved from the input field"
    );
    assert_eq!(
        *codomain_type,
        pressure_scalar_type(),
        "von_mises: codomain should be Scalar[PRESSURE]"
    );
    assert_eq!(*source, FieldSourceKind::VonMises, "von_mises: source kind");
    assert_eq!(
        lambda.as_ref(),
        &expected_inner,
        "von_mises: lambda slot must hold the ORIGINAL Sampled field unchanged — \
         this is the shape field_reductions::project_sampled_tensor_windows consumes"
    );
}

#[test]
fn max_shear_over_sampled_tensor_field_returns_maxshear_wrapped_field() {
    let (field, field_type) = sampled_stress_fixture();
    let expected_inner = field.clone();

    let result =
        eval_analysis_wrapper("max_shear", field, field_type, pressure_scalar_type());

    assert_ne!(
        result,
        Value::Undef,
        "max_shear over a Sampled-backed stress field must not be Undef"
    );

    let Value::Field {
        domain_type,
        codomain_type,
        source,
        lambda,
    } = &result
    else {
        panic!("max_shear(Field{{Sampled}}) should return a Field, got {result:?}");
    };

    assert_eq!(*domain_type, Type::dimensionless_scalar());
    assert_eq!(*codomain_type, pressure_scalar_type());
    assert_eq!(*source, FieldSourceKind::MaxShear);
    assert_eq!(lambda.as_ref(), &expected_inner);
}

#[test]
fn principal_stresses_over_sampled_tensor_field_returns_wrapped_field() {
    let (field, field_type) = sampled_stress_fixture();
    let expected_inner = field.clone();

    let result = eval_analysis_wrapper(
        "principal_stresses",
        field,
        field_type,
        Type::List(Box::new(pressure_scalar_type())),
    );

    assert_ne!(
        result,
        Value::Undef,
        "principal_stresses over a Sampled-backed stress field must not be Undef"
    );

    let Value::Field {
        domain_type,
        codomain_type,
        source,
        lambda,
    } = &result
    else {
        panic!("principal_stresses(Field{{Sampled}}) should return a Field, got {result:?}");
    };

    assert_eq!(*domain_type, Type::dimensionless_scalar());
    assert_eq!(
        *codomain_type,
        Type::List(Box::new(pressure_scalar_type())),
        "principal_stresses: sampling yields a 3-element list, so the codomain is List<Scalar[PRESSURE]>"
    );
    assert_eq!(*source, FieldSourceKind::PrincipalStresses);
    assert_eq!(lambda.as_ref(), &expected_inner);
}

#[test]
fn safety_factor_over_sampled_tensor_field_returns_wrapped_field() {
    let (field, field_type) = sampled_stress_fixture();
    let expected_inner = field.clone();
    let yield_si = 500e6;

    let result = eval_safety_factor(field, field_type, yield_si);

    assert_ne!(
        result,
        Value::Undef,
        "safety_factor over a Sampled-backed stress field must not be Undef"
    );

    let Value::Field {
        domain_type,
        codomain_type,
        source,
        lambda,
    } = &result
    else {
        panic!("safety_factor(Field{{Sampled}}, yield) should return a Field, got {result:?}");
    };

    assert_eq!(*domain_type, Type::dimensionless_scalar());
    assert_eq!(
        *codomain_type,
        Type::dimensionless_scalar(),
        "safety_factor: yield / von_mises cancels PRESSURE, so the codomain is dimensionless"
    );
    assert_eq!(*source, FieldSourceKind::SafetyFactor);
    assert_eq!(
        lambda.as_ref(),
        &Value::List(vec![
            expected_inner,
            Value::Scalar {
                si_value: yield_si,
                dimension: DimensionVector::PRESSURE,
            },
        ]),
        "safety_factor: lambda slot captures [original field, yield value]"
    );
}

// ── (c) Standing rejection guards ───────────────────────────────────────────

/// Drive all four analysis builtins over `field` and assert every one is
/// `Value::Undef`.
fn assert_all_four_wrappers_undef(field: Value, field_type: Type, why: &str) {
    for (op, codomain) in [
        ("von_mises", pressure_scalar_type()),
        ("max_shear", pressure_scalar_type()),
        (
            "principal_stresses",
            Type::List(Box::new(pressure_scalar_type())),
        ),
    ] {
        assert_eq!(
            eval_analysis_wrapper(op, field.clone(), field_type.clone(), codomain),
            Value::Undef,
            "{op} must reject this field: {why}"
        );
    }
    assert_eq!(
        eval_safety_factor(field, field_type, 500e6),
        Value::Undef,
        "safety_factor must reject this field: {why}"
    );
}

/// `FieldSourceKind::Imported` ALSO carries a `Value::SampledField` in its
/// lambda slot (`crates/reify-expr/src/lib.rs:3505`), so a relaxation phrased
/// as "accept whenever the lambda is a SampledField" would wrongly admit it.
///
/// That would be a false fix: `project_sampled_tensor_windows`
/// (`field_reductions.rs:821-843`) requires `source: Sampled` on the INNER
/// field, so an Imported-backed wrapper would construct successfully and then
/// reduce to `Value::Undef` — trading this task's silent false-green for the
/// identical one a layer down. The relaxed predicate must match the PAIR
/// `(Sampled, Value::SampledField)`, never either half alone.
#[test]
fn analysis_wrappers_reject_imported_source_field() {
    let sf = make_sampled_tensor_1d(
        "imported_stress",
        vec![0.0, 1.0, 2.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(175e6),
        ],
    );
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        pressure_tensor_type(),
        FieldSourceKind::Imported,
        Value::SampledField(sf),
    );

    assert_all_four_wrappers_undef(
        field,
        field_type,
        "source is Imported, which project_sampled_tensor_windows does not accept",
    );
}

/// `source: Sampled` but a non-`SampledField` lambda slot is malformed and
/// must stay rejected — the pair must match on BOTH halves.
#[test]
fn analysis_wrappers_reject_sampled_field_with_non_sampledfield_lambda() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let lambda = make_value_lambda(
        vec![("x", x_id)],
        CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar()),
        ValueMap::new(),
    );
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        pressure_tensor_type(),
        FieldSourceKind::Sampled,
        lambda,
    );
    assert_all_four_wrappers_undef(
        field,
        field_type,
        "source is Sampled but the lambda slot holds a Value::Lambda",
    );

    let (undef_field, undef_field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        pressure_tensor_type(),
        FieldSourceKind::Sampled,
        Value::Undef,
    );
    assert_all_four_wrappers_undef(
        undef_field,
        undef_field_type,
        "source is Sampled but the lambda slot holds Value::Undef",
    );
}

/// The `tensor_element_dimension` 3×3 check is unchanged by the relaxation:
/// a Sampled field with a non-tensor codomain is still rejected.
#[test]
fn analysis_wrappers_reject_sampled_field_with_non_3x3_codomain() {
    let sf = make_sampled_tensor_1d(
        "not_a_tensor",
        vec![0.0, 1.0, 2.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(175e6),
        ],
    );
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        Type::vec3(pressure_scalar_type()),
        FieldSourceKind::Sampled,
        Value::SampledField(sf),
    );
    assert_all_four_wrappers_undef(
        field,
        field_type,
        "codomain is Vec3, not a 3x3 tensor",
    );
}

/// Characterization pin: relaxing `validate_tensor_field` must not narrow the
/// pre-existing `Analytical` + `Value::Lambda` path.
#[test]
fn analysis_wrappers_over_analytical_field_unchanged() {
    let tensor = make_stress_tensor(
        &[&[100e6, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    );
    let (field, field_type) = make_constant_stress_field(tensor);
    let point3 = Type::point3(Type::dimensionless_scalar());

    for (op, expected_source, expected_codomain) in [
        ("von_mises", FieldSourceKind::VonMises, pressure_scalar_type()),
        ("max_shear", FieldSourceKind::MaxShear, pressure_scalar_type()),
        (
            "principal_stresses",
            FieldSourceKind::PrincipalStresses,
            Type::List(Box::new(pressure_scalar_type())),
        ),
    ] {
        let result_type = Type::Field {
            domain: Box::new(point3.clone()),
            codomain: Box::new(expected_codomain.clone()),
        };
        let expr = make_function_call(
            op,
            vec![CompiledExpr::literal(field.clone(), field_type.clone())],
            result_type,
        );
        let values = ValueMap::new();
        let result = eval_expr(&expr, &EvalContext::simple(&values));

        let Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } = &result
        else {
            panic!("{op}(analytical Field) should still return a Field, got {result:?}");
        };
        assert_eq!(*domain_type, point3, "{op}: analytical domain unchanged");
        assert_eq!(
            *codomain_type, expected_codomain,
            "{op}: analytical codomain unchanged"
        );
        assert_eq!(*source, expected_source, "{op}: analytical source unchanged");
        assert_eq!(
            lambda.as_ref(),
            &field,
            "{op}: analytical lambda slot still holds the original field"
        );
    }

    // safety_factor's 2-arg form over the same analytical field.
    let sf_result = eval_safety_factor(field.clone(), field_type, 250e6);
    let Value::Field {
        codomain_type,
        source,
        ..
    } = &sf_result
    else {
        panic!("safety_factor(analytical Field, yield) should still return a Field, got {sf_result:?}");
    };
    assert_eq!(*source, FieldSourceKind::SafetyFactor);
    assert_eq!(*codomain_type, Type::dimensionless_scalar());
}

// ── Task 7129 step-3: end-to-end reduction over a Sampled-backed wrapper ────
//
// These drive the FULL chain through the real builtin dispatch:
//   von_mises(Field{Sampled}) → Field{VonMises, lambda: Field{Sampled}}
//   max(that)                 → field_reductions::compute_extremum
//                             → project_sampled_tensor_windows (stride 9)
//                             → reify_stdlib::compute_von_mises_3x3 per window
//                             → reduce_sampled_extremum → argmax_argmin_index
//
// Every expected value below is a closed form for the uniaxial window
// σ_xx = S (all other components zero), and every one is exactly
// representable in binary, so the assertions use `assert_eq!` on the `Value`
// rather than an epsilon. The exactness is an identity, not a tolerance:
// `project_sampled_windows` applies the kernel per window and
// `argmax_argmin_index` selects a buffer element VERBATIM — there is no
// interpolation and no accumulation anywhere on the path.
//
// Result encodings (from `field_reductions::wrap_codomain` / `wrap_scalar_coord`):
//   PRESSURE codomain          → Value::Scalar { si_value, dimension: PRESSURE }
//   dimensionless codomain     → Value::Real(v)
//   dimensionless-scalar domain → Value::Real(coord) for argmax/argmin

/// Reduce `field` with a 1-argument reduction builtin (`max`/`min`/`argmax`/`argmin`).
fn reduce(op: &str, field: Value, field_type: Type, result_type: Type) -> Value {
    let expr = make_function_call(
        op,
        vec![CompiledExpr::literal(field, field_type)],
        result_type,
    );
    let values = ValueMap::new();
    eval_expr(&expr, &EvalContext::simple(&values))
}

/// Build `<op>(<stress fixture>)` and return the resulting wrapper field plus
/// its `Type::Field`, so it can be fed straight into [`reduce`].
fn wrapper_over_fixture(op: &str, codomain: Type) -> (Value, Type) {
    let (field, field_type) = sampled_stress_fixture();
    let wrapper = eval_analysis_wrapper(op, field, field_type, codomain.clone());
    assert_ne!(
        wrapper,
        Value::Undef,
        "{op} over the Sampled stress fixture must construct a wrapper"
    );
    let wrapper_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(codomain),
    };
    (wrapper, wrapper_type)
}

/// A `Value::Scalar` carrying PRESSURE.
fn pressure(si_value: f64) -> Value {
    Value::Scalar {
        si_value,
        dimension: DimensionVector::PRESSURE,
    }
}

/// `max`/`min` of `von_mises` over the Sampled stress fixture.
///
/// Uniaxial windows σ_xx = {100e6, 250e6, 175e6} → von Mises = |σ_xx| =
/// {100e6, 250e6, 175e6}. Expected max = 250e6 Pa, min = 100e6 Pa, exactly.
#[test]
fn max_min_of_von_mises_over_sampled_field_reduce_to_closed_form() {
    let (wrapper, wrapper_type) = wrapper_over_fixture("von_mises", pressure_scalar_type());

    assert_eq!(
        reduce(
            "max",
            wrapper.clone(),
            wrapper_type.clone(),
            pressure_scalar_type()
        ),
        pressure(250e6),
        "max(von_mises(Sampled stress)) should be the largest window's von Mises"
    );
    assert_eq!(
        reduce("min", wrapper, wrapper_type, pressure_scalar_type()),
        pressure(100e6),
        "min(von_mises(Sampled stress)) should be the smallest window's von Mises"
    );
}

/// `max`/`min` of `max_shear` over the Sampled stress fixture.
///
/// For a uniaxial window the eigenvalues are {0, 0, σ} ascending (σ > 0), so
/// `compute_max_shear_3x3` = (eigs[2] − eigs[0]) / 2 = σ/2 exactly:
/// {50e6, 125e6, 87.5e6}. Expected max = 125e6 Pa, min = 50e6 Pa.
#[test]
fn max_min_of_max_shear_over_sampled_field_reduce_to_closed_form() {
    let (wrapper, wrapper_type) = wrapper_over_fixture("max_shear", pressure_scalar_type());

    assert_eq!(
        reduce(
            "max",
            wrapper.clone(),
            wrapper_type.clone(),
            pressure_scalar_type()
        ),
        pressure(125e6),
        "max(max_shear(Sampled stress)) = max σ/2 = 250e6/2"
    );
    assert_eq!(
        reduce("min", wrapper, wrapper_type, pressure_scalar_type()),
        pressure(50e6),
        "min(max_shear(Sampled stress)) = min σ/2 = 100e6/2"
    );
}

/// `max`/`min` of `principal_stresses` over the Sampled stress fixture.
///
/// The projection is find_min-DEPENDENT (`field_reductions.rs`
/// `project_principal_stresses_sampled`): `max` selects eigs[2] (σ₁, the
/// largest principal stress) while `min` selects eigs[0] (σ₃, the smallest).
/// For a uniaxial window the eigenvalues are {0, 0, σ}, so eigs[2] = σ and
/// eigs[0] = 0 for every window. Expected max = 250e6 Pa, min = 0.0 Pa.
///
/// This find_min dependence is precisely why the wrapper is kept LAZY rather
/// than eagerly projected at construction time: no single pre-projected
/// buffer could serve both reductions.
#[test]
fn max_min_of_principal_stresses_over_sampled_field_reduce_to_closed_form() {
    let (wrapper, wrapper_type) = wrapper_over_fixture(
        "principal_stresses",
        Type::List(Box::new(pressure_scalar_type())),
    );

    assert_eq!(
        reduce(
            "max",
            wrapper.clone(),
            wrapper_type.clone(),
            pressure_scalar_type()
        ),
        pressure(250e6),
        "max(principal_stresses(Sampled stress)) selects eigs[2] = the largest σ₁"
    );
    assert_eq!(
        reduce("min", wrapper, wrapper_type, pressure_scalar_type()),
        pressure(0.0),
        "min(principal_stresses(Sampled stress)) selects eigs[0] = σ₃ = 0 for a uniaxial window"
    );
}

/// `max`/`min` of `safety_factor` over the Sampled stress fixture.
///
/// safety_factor = yield / von Mises. With yield = 500e6 and von Mises =
/// {100e6, 250e6, 175e6}: {5.0, 2.0, 2.857…}. Only the two exactly
/// representable extremes are asserted — 500/175 is deliberately NOT asserted.
///
/// The codomain is `Type::dimensionless_scalar()`, and `wrap_codomain` maps a
/// dimensionless scalar to `Value::Real`, not `Value::Scalar { DIMENSIONLESS }`.
#[test]
fn max_min_of_safety_factor_over_sampled_field_reduce_to_closed_form() {
    let (field, field_type) = sampled_stress_fixture();
    let wrapper = eval_safety_factor(field, field_type, 500e6);
    assert_ne!(wrapper, Value::Undef, "safety_factor wrapper must construct");
    let wrapper_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };

    assert_eq!(
        reduce(
            "max",
            wrapper.clone(),
            wrapper_type.clone(),
            Type::dimensionless_scalar()
        ),
        Value::Real(5.0),
        "max(safety_factor(.., 500e6)) = 500e6 / 100e6 = 5.0 (the least-stressed window)"
    );
    assert_eq!(
        reduce("min", wrapper, wrapper_type, Type::dimensionless_scalar()),
        Value::Real(2.0),
        "min(safety_factor(.., 500e6)) = 500e6 / 250e6 = 2.0 (the most-stressed window)"
    );
}

/// `argmax`/`argmin` of `von_mises` return DOMAIN COORDINATES, not values.
///
/// Pins that `arg_coord_from_index` decomposes the winning linear index
/// against the inner Sampled field's cloned `axis_grids`: the 250e6 window is
/// at index 1 → coord 1.0, the 100e6 window at index 0 → coord 0.0. The domain
/// is a dimensionless scalar, so `wrap_scalar_coord` yields `Value::Real`.
#[test]
fn argmax_argmin_of_von_mises_over_sampled_field_return_domain_coordinates() {
    let (wrapper, wrapper_type) = wrapper_over_fixture("von_mises", pressure_scalar_type());

    assert_eq!(
        reduce(
            "argmax",
            wrapper.clone(),
            wrapper_type.clone(),
            Type::dimensionless_scalar()
        ),
        Value::Real(1.0),
        "argmax(von_mises(..)) should be the axis coord of the 250e6 window (index 1)"
    );
    assert_eq!(
        reduce("argmin", wrapper, wrapper_type, Type::dimensionless_scalar()),
        Value::Real(0.0),
        "argmin(von_mises(..)) should be the axis coord of the 100e6 window (index 0)"
    );
}

/// Out-of-solid FEA sentinel windows are SKIPPED, not propagated.
///
/// `solve_elastic_static` writes `f64::NAN` for all 9 components at grid points
/// outside the mesh. Those windows project to NaN and are dropped by the
/// `is_finite()` gate in `argmax_argmin_index`, so a real stress field with
/// exterior grid points still reduces to the interior peak.
#[test]
fn von_mises_over_sampled_field_skips_nan_sentinel_windows() {
    let sf = make_sampled_tensor_1d(
        "stress_with_exterior",
        vec![0.0, 1.0, 2.0, 3.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(175e6),
            [f64::NAN; 9], // out-of-solid sentinel
        ],
    );
    let (field, field_type) = wrap_sampled_stress_field(sf);
    let wrapper =
        eval_analysis_wrapper("von_mises", field, field_type, pressure_scalar_type());
    let wrapper_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(pressure_scalar_type()),
    };

    assert_eq!(
        reduce(
            "max",
            wrapper.clone(),
            wrapper_type.clone(),
            pressure_scalar_type()
        ),
        pressure(250e6),
        "the NaN sentinel window must be skipped, not poison the max"
    );
    assert_eq!(
        reduce(
            "argmax",
            wrapper,
            wrapper_type,
            Type::dimensionless_scalar()
        ),
        Value::Real(1.0),
        "argmax must still index the 250e6 window, not the NaN one"
    );
}

/// An all-NaN buffer (every grid point outside the solid) has no finite
/// extremum, so all four wrappers reduce to `Value::Undef` under every
/// reduction rather than returning NaN or panicking.
#[test]
fn analysis_reductions_over_all_nan_sampled_field_return_undef() {
    let nan_fixture = || {
        let sf = make_sampled_tensor_1d(
            "all_exterior",
            vec![0.0, 1.0, 2.0],
            vec![[f64::NAN; 9], [f64::NAN; 9], [f64::NAN; 9]],
        );
        wrap_sampled_stress_field(sf)
    };

    for (op, codomain) in [
        ("von_mises", pressure_scalar_type()),
        ("max_shear", pressure_scalar_type()),
        (
            "principal_stresses",
            Type::List(Box::new(pressure_scalar_type())),
        ),
    ] {
        let (field, field_type) = nan_fixture();
        let wrapper = eval_analysis_wrapper(op, field, field_type, codomain.clone());
        assert_ne!(
            wrapper,
            Value::Undef,
            "{op}: the wrapper still CONSTRUCTS over an all-NaN field — only the reduction is Undef"
        );
        let wrapper_type = Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(codomain),
        };
        for red in ["max", "min", "argmax", "argmin"] {
            assert_eq!(
                reduce(
                    red,
                    wrapper.clone(),
                    wrapper_type.clone(),
                    Type::dimensionless_scalar()
                ),
                Value::Undef,
                "{red}({op}(all-NaN field)) should be Undef — no finite extremum exists"
            );
        }
    }

    let (field, field_type) = nan_fixture();
    let sf_wrapper = eval_safety_factor(field, field_type, 500e6);
    assert_ne!(sf_wrapper, Value::Undef, "safety_factor wrapper constructs");
    let sf_wrapper_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };
    for red in ["max", "min", "argmax", "argmin"] {
        assert_eq!(
            reduce(
                red,
                sf_wrapper.clone(),
                sf_wrapper_type.clone(),
                Type::dimensionless_scalar()
            ),
            Value::Undef,
            "{red}(safety_factor(all-NaN field)) should be Undef"
        );
    }
}

/// REMAINING-GAP PIN: the wrapper now constructs over a Sampled backing, but
/// pointwise `sample()` of that wrapper is still `Value::Undef`.
///
/// Mechanism: `sample_field_at` (`crates/reify-expr/src/lib.rs`) forwards the
/// INNER field's lambda slot — a `Value::SampledField` — into
/// `apply_lambda_with_point_unpacking`, which handles `Value::Lambda` only and
/// returns `Undef` for anything else. Closing it needs the tensor element
/// dimension plumbed through `sample_field_at` (the wrapper's own codomain
/// cannot recover it for `safety_factor`, whose codomain is dimensionless)
/// plus a stride-3 variant for `principal_stresses` — edits well outside this
/// task's file set, tracked separately.
///
/// This is NOT a regression from task 7129: before the fix
/// `von_mises(sampled)` was itself `Undef`, so sampling it was `Undef` too.
/// The observable for the sample path is unchanged; only the reduction path
/// changed. The test is written in two parts so it was genuinely RED before
/// the `validate_tensor_field` relaxation — the first assertion failed.
#[test]
fn sampled_backed_analysis_wrapper_is_constructed_but_pointwise_sample_still_undef() {
    let (field, field_type) = sampled_stress_fixture();
    let wrapper =
        eval_analysis_wrapper("von_mises", field, field_type, pressure_scalar_type());

    // Part 1 — the wrapper IS constructed (this is what task 7129 fixed).
    assert!(
        matches!(
            &wrapper,
            Value::Field {
                source: FieldSourceKind::VonMises,
                ..
            }
        ),
        "von_mises over a Sampled stress field must construct a VonMises wrapper, got {wrapper:?}"
    );

    // Part 2 — pointwise sampling of it is still Undef (the remaining gap).
    let wrapper_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(pressure_scalar_type()),
    };
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(wrapper, wrapper_type),
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
        ],
        pressure_scalar_type(),
    );
    let values = ValueMap::new();
    assert_eq!(
        eval_expr(&sample_expr, &EvalContext::simple(&values)),
        Value::Undef,
        "pointwise sample() of a Sampled-backed analysis wrapper is still Undef — \
         a separate, pre-existing gap in apply_lambda_with_point_unpacking"
    );
}
