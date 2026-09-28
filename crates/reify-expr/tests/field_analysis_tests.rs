//! Field-level analysis wrapper tests.
//!
//! Tests for stress analysis field operators (von_mises, principal_stresses,
//! max_shear, safety_factor) that wrap tensor fields and apply pointwise
//! analysis when sampled.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_expr::{EvalContext, eval_expr};
use reify_core::{
    ContentHash, Diagnostic, DiagnosticCode, DimensionVector, Severity, Type, ValueCellId,
};
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
// The fixtures below are copied from `field_reductions_tests.rs:1968/2000/2018`.
// That duplication is an accepted trade-off, not a language constraint: a
// `tests/common/mod.rs` module (as `crates/reify-eval/tests/common/` does) or
// the `reify-test-support` crate could share them. Hoisting would mean editing
// `field_reductions_tests.rs`, which this task does not own, and the fixtures
// are three small constructors whose numeric contracts are pinned there.

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

/// [`sampled_stress_fixture`] plus an out-of-solid sentinel window at x = 3:
/// all nine components `f64::NAN`, as `solve_elastic_static` writes at grid
/// points outside the mesh.
fn sampled_stress_with_exterior_fixture() -> (Value, Type) {
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
/// lambda slot (the Imported arm of `sample_field_at`,
/// `crates/reify-expr/src/lib.rs`), so a relaxation phrased as "accept whenever
/// the lambda is a SampledField" would wrongly admit it.
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

// The other three wrapper kinds are NOT re-reduced here. `max_shear`,
// `principal_stresses` and `safety_factor` own their per-kernel numeric oracles
// in `field_reductions_tests.rs`
// (`max_min_max_shear_derived_sampled_field_returns_correct_extremum`,
// `max_min_principal_stresses_...`, `max_min_safety_factor_...`) — over this
// same uniaxial fixture, against the same expected numbers. Those tests
// hand-build the `Field { source: Sampled, lambda: SampledField }` wrapper;
// what THIS file adds is that `analysis.rs` now PRODUCES that exact shape, and
// the section-(b) construction tests assert it directly (`source` plus
// `lambda.as_ref() == &expected_inner`). The von Mises pair above is kept as
// the one end-to-end seam check, and the all-NaN test below still drives all
// four kinds through `max`/`min`/`argmax`/`argmin`.

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
    let (field, field_type) = sampled_stress_with_exterior_fixture();
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

// ── Task 7131: pointwise sample() of a Sampled-backed analysis wrapper ──────
//
// Over a Sampled backing, `sample(W, p)` projects every node's stride-9 window
// with the same `reify_stdlib` kernel the reductions scan, then interpolates
// the projected node values with the backing field's own method. At a grid
// node the sample therefore IS the node value `max`/`min`/`argmax`/`argmin`
// see, and a Linear sample is a convex combination of node values.
//
// Every expected value below is exact in binary: the inputs are integers or
// dyadics, `lerp(a, b, t) = a + (b − a)·t` is exact at t ∈ {0, 0.5, 1} for
// them, and at t = 0 it returns `a` bit for bit whenever the right neighbour is
// finite. `Value::eq` compares f64 bits, so whole values are compared with
// `assert_eq!`; every zero here is +0.0.

/// Build `sample(<field>, <at>)`, typed as the field's codomain.
fn sample_call((field, field_type): &(Value, Type), at: Value, at_type: Type) -> CompiledExpr {
    let Type::Field { codomain, .. } = field_type else {
        panic!("sample_call: expected a Type::Field, got {field_type:?}");
    };
    make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field.clone(), field_type.clone()),
            CompiledExpr::literal(at, at_type),
        ],
        (**codomain).clone(),
    )
}

/// Evaluate `sample(<field>, <at>)`.
fn sample_at(field: &(Value, Type), at: Value, at_type: Type) -> Value {
    let values = ValueMap::new();
    eval_expr(
        &sample_call(field, at, at_type),
        &EvalContext::simple(&values),
    )
}

/// Evaluate `sample(<field>, x)` over a dimensionless 1-D domain.
fn sample_1d(field: &(Value, Type), x: f64) -> Value {
    sample_at(field, Value::Real(x), Type::dimensionless_scalar())
}

/// A `Value::Scalar` carrying LENGTH.
fn length(si_value: f64) -> Value {
    Value::Scalar {
        si_value,
        dimension: DimensionVector::LENGTH,
    }
}

/// Build the `kind` analysis wrapper over a stress field with a dimensionless
/// 1-D domain, paired with its `Type::Field`, asserting that it constructs.
/// `safety_factor` takes a 500e6 Pa yield.
fn analysis_wrapper(kind: &str, (field, field_type): (Value, Type)) -> (Value, Type) {
    let codomain = match kind {
        "principal_stresses" => Type::List(Box::new(pressure_scalar_type())),
        "safety_factor" => Type::dimensionless_scalar(),
        _ => pressure_scalar_type(),
    };
    let wrapper = if kind == "safety_factor" {
        eval_safety_factor(field, field_type, 500e6)
    } else {
        eval_analysis_wrapper(kind, field, field_type, codomain.clone())
    };
    assert_ne!(wrapper, Value::Undef, "{kind} must construct a wrapper");
    let wrapper_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(codomain),
    };
    (wrapper, wrapper_type)
}

/// `principal_stresses` of a uniaxial window σ > 0, in the builtin's
/// ascending order: the two zero principal stresses first, then σ.
fn uniaxial_principal_stresses(sigma: f64) -> Value {
    Value::List(vec![pressure(0.0), pressure(0.0), pressure(sigma)])
}

/// A grid-backed wrapper's node values ARE what the reductions scan, so
/// sampling `von_mises(stress)` where `argmax`/`argmin` of it point returns
/// exactly its `max`/`min`. `argmin` is the first node, which reaches the
/// interpolator's lower clamp branch.
///
/// Part 1 pins that the wrapper is constructed over a Sampled backing at all:
/// before task 7129 relaxed `validate_tensor_field` it was not, and this
/// test's first assertion failed.
#[test]
fn sampled_backed_von_mises_wrapper_samples_to_its_reduction_extrema_at_their_arg_coordinates() {
    let (field, field_type) = sampled_stress_fixture();
    let wrapper = eval_analysis_wrapper("von_mises", field, field_type, pressure_scalar_type());

    // Part 1 — the wrapper IS constructed.
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

    // Part 2 — sampled at its arg-extrema, it returns its extrema.
    let wrapper_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(pressure_scalar_type()),
    };
    let wrapper = (wrapper, wrapper_type);
    for (arg_reduction, reduction, extremum) in [
        ("argmax", "max", pressure(250e6)),
        ("argmin", "min", pressure(100e6)),
    ] {
        let (field, field_type) = wrapper.clone();
        let at = reduce(
            arg_reduction,
            field.clone(),
            field_type.clone(),
            Type::dimensionless_scalar(),
        );
        assert_eq!(
            reduce(reduction, field, field_type, pressure_scalar_type()),
            extremum,
            "{reduction}(von_mises(stress))"
        );
        assert_eq!(
            sample_at(&wrapper, at.clone(), Type::dimensionless_scalar()),
            extremum,
            "sample(von_mises(stress), {arg_reduction}(..) = {at:?}) must equal {reduction}(..)"
        );
    }
}

/// Node equality holds for every wrapper kind: sampled at its own
/// `argmax`/`argmin`, each wrapper returns exactly its `max`/`min`. The sample
/// path and the reductions apply the per-kind kernel in two separate places —
/// the `sample_*_at_point` closures in `analysis.rs` and the `project_*_sampled`
/// functions in `field_reductions.rs` — so this is the pin that fails when one
/// side's kernel changes without the other.
///
/// `principal_stresses` samples to its ascending List but reduces to a single
/// principal stress — the largest for `max`, the smallest for `min` — so the
/// matching end of the sampled List is compared.
#[test]
fn every_wrapper_kind_samples_to_its_reduction_extrema_at_their_arg_coordinates() {
    for kind in [
        "von_mises",
        "max_shear",
        "principal_stresses",
        "safety_factor",
    ] {
        let wrapper = analysis_wrapper(kind, sampled_stress_fixture());
        for (arg_reduction, reduction) in [("argmax", "max"), ("argmin", "min")] {
            let (field, field_type) = wrapper.clone();
            let at = reduce(
                arg_reduction,
                field.clone(),
                field_type.clone(),
                Type::dimensionless_scalar(),
            );
            let extremum = reduce(reduction, field, field_type, Type::dimensionless_scalar());
            assert_ne!(
                extremum,
                Value::Undef,
                "{reduction}({kind}(stress)) must reduce"
            );

            let sampled = match sample_at(&wrapper, at.clone(), Type::dimensionless_scalar()) {
                Value::List(items) if reduction == "max" => items[items.len() - 1].clone(),
                Value::List(items) => items[0].clone(),
                scalar => scalar,
            };
            assert_eq!(
                sampled, extremum,
                "sample({kind}(stress), {arg_reduction}(..) = {at:?}) must equal {reduction}(..)"
            );
        }
    }
}

/// Sampling interpolates the node PROJECTIONS, not the tensors. The two nodes
/// hold σ_xx = +100e6 and −100e6, both of von Mises value exactly 100e6, so
/// the mid-cell sample is 100e6 too — equal to `min`, as a convex combination
/// of node values must be. Interpolating the tensors first would sample the
/// zero tensor there, whose von Mises value 0 lies below `min`.
#[test]
fn sampled_backed_wrapper_interpolates_node_projections_not_tensors() {
    let sf = make_sampled_tensor_1d(
        "sign_flip",
        vec![0.0, 1.0],
        vec![uniaxial_window(100e6), uniaxial_window(-100e6)],
    );
    let wrapper = analysis_wrapper("von_mises", wrap_sampled_stress_field(sf));

    let mid_cell = sample_1d(&wrapper, 0.5);
    assert_eq!(mid_cell, pressure(100e6));
    let (field, field_type) = wrapper;
    assert_eq!(
        mid_cell,
        reduce("min", field, field_type, pressure_scalar_type()),
        "the mid-cell sample must equal min(von_mises(..)), not undercut it"
    );
}

/// `max_shear`, `principal_stresses` and `safety_factor` sample their node
/// projections too, at the node x = 1 (σ_xx = 250e6) and mid-cell x = 0.5
/// (between σ_xx = 100e6 and 250e6):
///
/// - `max_shear` = σ/2: 125e6 at the node, lerp(50e6, 125e6, 0.5) = 87.5e6
///   mid-cell.
/// - `principal_stresses` = [0, 0, σ]: σ = 250e6 at the node and
///   lerp(100e6, 250e6, 0.5) = 175e6 mid-cell, every element typed by the
///   wrapper's `List<Scalar<P>>` codomain.
/// - `safety_factor` = 500e6 / σ: 2 at the node and lerp(5, 2, 0.5) = 3.5
///   mid-cell, where interpolating the tensor first would give 500/175.
#[test]
fn max_shear_principal_stresses_and_safety_factor_sample_their_node_projections() {
    for (kind, at_node, mid_cell) in [
        ("max_shear", pressure(125e6), pressure(87.5e6)),
        (
            "principal_stresses",
            uniaxial_principal_stresses(250e6),
            uniaxial_principal_stresses(175e6),
        ),
        ("safety_factor", Value::Real(2.0), Value::Real(3.5)),
    ] {
        let wrapper = analysis_wrapper(kind, sampled_stress_fixture());
        assert_eq!(
            sample_1d(&wrapper, 1.0),
            at_node,
            "{kind} at the grid node x = 1"
        );
        assert_eq!(
            sample_1d(&wrapper, 0.5),
            mid_cell,
            "{kind} mid-cell at x = 0.5"
        );
    }
}

/// A sample whose interpolation stencil touches an out-of-solid (all-NaN)
/// window is `Undef` for every wrapper kind — never NaN — while a sample over
/// a fully finite cell of the same field keeps its value.
///
/// Nothing is asserted AT x = 2, the finite node beside the sentinel: a t = 0
/// lerp against a NaN right neighbour yields NaN there, an interpolation
/// artefact rather than a contract worth pinning.
#[test]
fn sampled_backed_wrapper_samples_undef_where_the_stencil_touches_an_out_of_solid_window() {
    for (kind, at_node) in [
        ("von_mises", pressure(250e6)),
        ("max_shear", pressure(125e6)),
        ("principal_stresses", uniaxial_principal_stresses(250e6)),
        ("safety_factor", Value::Real(2.0)),
    ] {
        let wrapper = analysis_wrapper(kind, sampled_stress_with_exterior_fixture());
        assert_eq!(
            sample_1d(&wrapper, 1.0),
            at_node,
            "{kind}: the cell [1, 2] is finite"
        );
        assert_eq!(
            sample_1d(&wrapper, 2.5),
            Value::Undef,
            "{kind}: the cell [2, 3] holds the out-of-solid sentinel"
        );
    }
}

/// A hydrostatic node — equal normal stresses, so von Mises 0 — has an
/// infinite safety factor. A sample whose stencil reaches it is `Undef`, never
/// +∞, as the pointwise `safety_factor` builtin sanitizes it; a cell between
/// two loaded nodes keeps its value, lerp(5, 2, 0.5) = 3.5.
#[test]
fn safety_factor_sample_is_undef_where_the_stencil_reaches_a_hydrostatic_node() {
    let hydrostatic = [100e6, 0.0, 0.0, 0.0, 100e6, 0.0, 0.0, 0.0, 100e6];
    let sf = make_sampled_tensor_1d(
        "stress_with_hydrostatic_node",
        vec![0.0, 1.0, 2.0],
        vec![uniaxial_window(100e6), uniaxial_window(250e6), hydrostatic],
    );
    let wrapper = analysis_wrapper("safety_factor", wrap_sampled_stress_field(sf));

    assert_eq!(
        sample_1d(&wrapper, 0.5),
        Value::Real(3.5),
        "the cell [0, 1] is loaded at both ends"
    );
    assert_eq!(
        sample_1d(&wrapper, 1.5),
        Value::Undef,
        "the cell [1, 2] reaches the hydrostatic node"
    );
    assert_eq!(
        sample_1d(&wrapper, 2.0),
        Value::Undef,
        "the hydrostatic node itself"
    );
}

/// An out-of-bounds sample of a grid-backed wrapper is `Undef` and warns
/// through the BACKING field's once-per-session latch. Every clone of the
/// wrapper shares that `SampledField` through the `Arc`s in the lambda slots,
/// so repeated samples warn exactly once.
#[test]
fn out_of_bounds_sample_of_a_sampled_backed_wrapper_warns_once_per_backing_field() {
    let wrapper = analysis_wrapper("von_mises", sampled_stress_fixture());
    let values = ValueMap::new();
    let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
    let ctx = EvalContext::simple(&values).with_runtime_diagnostics(&sink);
    let outside = sample_call(&wrapper, Value::Real(5.0), Type::dimensionless_scalar());

    for _ in 0..2 {
        assert_eq!(eval_expr(&outside, &ctx), Value::Undef);
    }
    let out_of_bounds_warnings = sink
        .borrow()
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.code == Some(DiagnosticCode::FieldOutOfBounds)
        })
        .count();
    assert_eq!(
        out_of_bounds_warnings, 1,
        "two out-of-bounds samples of one backing field must warn exactly once"
    );
}

/// Pointwise sampling reads a Regular3D stress grid in its row-major node
/// order — x outer, z inner, as `solve_elastic_static`'s resample writes it —
/// at `Point3<Length>` coordinates, the production domain.
///
/// Node (i, j, k) of the 2×2×2 unit grid is window n = 4i + 2j + k and holds a
/// uniaxial (n + 1)e6 Pa tensor. Node (1, 1, 0) is n = 6 → 7e6 Pa, where a
/// transposed layout would read n = 3 → 4e6 Pa. The cell centre is the mean of
/// the eight corners, 4.5e6 Pa, exactly.
#[test]
fn sampled_backed_von_mises_wrapper_samples_a_regular3d_grid_at_point3_length_coordinates() {
    let unit_axis = vec![0.0, 1.0];
    let sf = SampledField {
        name: "stress_3d".to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0; 3],
        bounds_max: vec![1.0; 3],
        spacing: vec![1.0; 3],
        axis_grids: vec![unit_axis.clone(), unit_axis.clone(), unit_axis],
        interpolation: InterpolationKind::Linear,
        data: (1..=8)
            .flat_map(|m| uniaxial_window(m as f64 * 1e6))
            .collect(),
        oob_emitted: AtomicBool::new(false),
    };
    let domain = Type::point3(Type::length());
    let (field, field_type) = make_field_with_source(
        domain.clone(),
        pressure_tensor_type(),
        FieldSourceKind::Sampled,
        Value::SampledField(sf),
    );
    let wrapper_type = Type::Field {
        domain: Box::new(domain.clone()),
        codomain: Box::new(pressure_scalar_type()),
    };
    let von_mises = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        wrapper_type.clone(),
    );
    let values = ValueMap::new();
    let wrapper = (
        eval_expr(&von_mises, &EvalContext::simple(&values)),
        wrapper_type,
    );

    let point = |x: f64, y: f64, z: f64| Value::Point(vec![length(x), length(y), length(z)]);
    assert_eq!(
        sample_at(&wrapper, point(1.0, 1.0, 0.0), domain.clone()),
        pressure(7e6),
        "node (1, 1, 0) is window 4·1 + 2·1 + 0 = 6"
    );
    assert_eq!(
        sample_at(&wrapper, point(0.5, 0.5, 0.5), domain),
        pressure(4.5e6),
        "the cell centre is the mean of the eight corners"
    );
}
