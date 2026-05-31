//! Field reduction tests.
//!
//! Tests for the eager Field reductions `max`, `min`, `argmax`, `argmin`
//! over `Value::Field` arguments. These collapse a field to a single
//! scalar (or single point) immediately rather than producing a derived
//! lazy field-wrapper (cf. `analysis::compute_von_mises` etc., which
//! return `Value::Field` wrappers).
//!
//! Architectural notes pinned by these tests:
//!
//! 1. **Dispatch gating** — the dispatch arms in `crates/reify-expr/src/lib.rs`
//!    intercept only when `args.len() == 1 && first arg is Value::Field`.
//!    Binary `max(a, b)` / `min(a, b)` (numeric.rs) is preserved.
//! 2. **Sampled-source-only first cut** — `FieldSourceKind::Sampled` is the
//!    only fully-implemented branch. Other source kinds return
//!    `Value::Undef`. See `field_reductions.rs` for the deferred-path comment.
//! 3. **NaN/empty handling** — `data` may contain NaN; reductions skip
//!    non-finite values. Empty / all-non-finite data → `Value::Undef`.

#![allow(clippy::mutable_key_type)]

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ContentHash, DimensionVector, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, FieldSourceKind, InterpolationKind, ResolvedFunction, SampledField, SampledGridKind, Value, ValueMap};

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

/// Construct a 1-D `Value::SampledField` from per-axis grid coords and data.
fn make_sampled_1d(name: &str, axis: Vec<f64>, data: Vec<f64>) -> SampledField {
    let bounds_min = vec![*axis.first().expect("axis must be non-empty")];
    let bounds_max = vec![*axis.last().expect("axis must be non-empty")];
    let spacing = if axis.len() >= 2 {
        vec![axis[1] - axis[0]]
    } else {
        vec![1.0]
    };
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

/// Construct a 2-D `Value::SampledField` from two per-axis grid coords and
/// row-major data (axis-0 outermost: `data[i0 * s1 + i1]`).
fn make_sampled_2d(name: &str, axis0: Vec<f64>, axis1: Vec<f64>, data: Vec<f64>) -> SampledField {
    let bounds_min = vec![
        *axis0.first().expect("axis0 must be non-empty"),
        *axis1.first().expect("axis1 must be non-empty"),
    ];
    let bounds_max = vec![
        *axis0.last().expect("axis0 must be non-empty"),
        *axis1.last().expect("axis1 must be non-empty"),
    ];
    let spacing = vec![
        if axis0.len() >= 2 {
            axis0[1] - axis0[0]
        } else {
            1.0
        },
        if axis1.len() >= 2 {
            axis1[1] - axis1[0]
        } else {
            1.0
        },
    ];
    SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular2D,
        bounds_min,
        bounds_max,
        spacing,
        axis_grids: vec![axis0, axis1],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    }
}

/// Construct a 3-D `Value::SampledField` from three per-axis grid coords
/// and row-major data (axis-0 outermost: `data[i0*s1*s2 + i1*s2 + i2]`).
fn make_sampled_3d(
    name: &str,
    axis0: Vec<f64>,
    axis1: Vec<f64>,
    axis2: Vec<f64>,
    data: Vec<f64>,
) -> SampledField {
    let bounds_min = vec![
        *axis0.first().expect("axis0 must be non-empty"),
        *axis1.first().expect("axis1 must be non-empty"),
        *axis2.first().expect("axis2 must be non-empty"),
    ];
    let bounds_max = vec![
        *axis0.last().expect("axis0 must be non-empty"),
        *axis1.last().expect("axis1 must be non-empty"),
        *axis2.last().expect("axis2 must be non-empty"),
    ];
    let spacing = vec![
        if axis0.len() >= 2 {
            axis0[1] - axis0[0]
        } else {
            1.0
        },
        if axis1.len() >= 2 {
            axis1[1] - axis1[0]
        } else {
            1.0
        },
        if axis2.len() >= 2 {
            axis2[1] - axis2[0]
        } else {
            1.0
        },
    ];
    SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min,
        bounds_max,
        spacing,
        axis_grids: vec![axis0, axis1, axis2],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    }
}

/// Wrap a `SampledField` in a `Value::Field { source: Sampled, .. }` with
/// the supplied domain and codomain types.
fn wrap_sampled_field(sf: SampledField, domain: Type, codomain: Type) -> (Value, Type) {
    let field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    };
    let field_type = Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(codomain),
    };
    (field, field_type)
}

// ── Step 1: max over a 1-D Real-codomain Sampled field ──────────────────────

/// `max(field)` over a Sampled 1-D Real-codomain field returns the maximum
/// value in the data buffer wrapped as `Value::Real`.
#[test]
fn max_sampled_field_1d_real_returns_max_data_value() {
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, Type::Real);

    let expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Real(5.0),
        "max(sampled 1-D Real-codomain field) should equal max of data buffer"
    );
}

/// Regression pin: binary-form `max(a, b)` over two scalar args continues
/// to dispatch through `reify_stdlib::eval_builtin` -> `numeric.rs::max`.
/// The new Field-reduction dispatch gate must NOT intercept this case.
#[test]
fn max_two_arg_scalar_form_unchanged() {
    let expr = make_function_call(
        "max",
        vec![
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
            CompiledExpr::literal(Value::Real(5.0), Type::Real),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Real(5.0),
        "binary max(3.0, 5.0) should still resolve via numeric.rs to 5.0"
    );
}

// ── Step 3: min over a 1-D Real-codomain Sampled field ──────────────────────

/// `min(field)` over a Sampled 1-D Real-codomain field returns the minimum
/// value in the data buffer wrapped as `Value::Real`.
#[test]
fn min_sampled_field_1d_real_returns_min_data_value() {
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, Type::Real);

    let expr = make_function_call(
        "min",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Real(1.0),
        "min(sampled 1-D Real-codomain field) should equal min of data buffer"
    );
}

/// Regression pin: binary-form `min(a, b)` over two scalar args continues
/// to dispatch through `reify_stdlib::eval_builtin` -> `numeric.rs::min`.
#[test]
fn min_two_arg_scalar_form_unchanged() {
    let expr = make_function_call(
        "min",
        vec![
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
            CompiledExpr::literal(Value::Real(5.0), Type::Real),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Real(3.0),
        "binary min(3.0, 5.0) should still resolve via numeric.rs to 3.0"
    );
}

// ── Step 5: max / min over a dimensioned (PRESSURE) codomain ────────────────

/// `max(field)` over a Sampled 1-D Pressure-codomain field returns the
/// maximum value as `Value::Scalar { si_value: <max>, dimension: PRESSURE }`.
#[test]
fn max_sampled_field_with_pressure_codomain_returns_dimensioned_scalar() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let sf = make_sampled_1d("stress", vec![0.0, 1.0, 2.0], vec![100e6, 250e6, 175e6]);
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, pressure.clone());

    let expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field, field_type)],
        pressure.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Scalar {
            si_value: 250e6,
            dimension: DimensionVector::PRESSURE,
        },
        "max of pressure-codomain field should preserve PRESSURE dimension"
    );
}

// ── Step 7: argmax over a 1-D Length-domain Sampled field ───────────────────

/// `argmax(field)` over a Sampled 1-D Length-domain Real-codomain field
/// returns the coord at the index of the data buffer's maximum, wrapped
/// per the field's `domain_type` (here `Type::Scalar { LENGTH }`).
#[test]
fn argmax_sampled_field_1d_length_domain_returns_coord_at_max_index() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    // axis = [0,1,2,3,4]; data = [1,5,3,4,2] -> max at index 1 -> coord 1.0m
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::Real);

    let expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(field, field_type)],
        length.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmax(field) over 1-D LENGTH domain should return the coord of the data max"
    );
}

/// `argmax(field)` over a Sampled 1-D Real-domain field returns the
/// coord as `Value::Real` (no dimension to preserve).
#[test]
fn argmax_sampled_field_1d_real_domain_returns_real_coord() {
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, Type::Real);

    let expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Real(1.0),
        "argmax(field) over 1-D Real domain should return Value::Real(coord)"
    );
}

/// `min(field)` over a Sampled 1-D Pressure-codomain field returns the
/// minimum value as `Value::Scalar { si_value: <min>, dimension: PRESSURE }`.
#[test]
fn min_sampled_field_with_pressure_codomain_returns_dimensioned_scalar() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let sf = make_sampled_1d("stress", vec![0.0, 1.0, 2.0], vec![100e6, 250e6, 175e6]);
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, pressure.clone());

    let expr = make_function_call(
        "min",
        vec![CompiledExpr::literal(field, field_type)],
        pressure.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Scalar {
            si_value: 100e6,
            dimension: DimensionVector::PRESSURE,
        },
        "min of pressure-codomain field should preserve PRESSURE dimension"
    );
}

// ── Step 9: argmin over a 1-D Length-domain Sampled field ───────────────────

/// `argmin(field)` over a Sampled 1-D Length-domain Real-codomain field
/// returns the coord at the index of the data buffer's minimum, wrapped
/// per the field's `domain_type` (here `Type::Scalar { LENGTH }`).
///
/// Mirrors `argmax_sampled_field_1d_length_domain_returns_coord_at_max_index`
/// for the symmetric min case: data `[1, 5, 3, 4, 2]` -> min at index 0
/// -> coord 0.0m.
#[test]
fn argmin_sampled_field_1d_length_domain_returns_coord_at_min_index() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    // axis = [0,1,2,3,4]; data = [1,5,3,4,2] -> min at index 0 -> coord 0.0m
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::Real);

    let expr = make_function_call(
        "argmin",
        vec![CompiledExpr::literal(field, field_type)],
        length.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmin(field) over 1-D LENGTH domain should return the coord of the data min"
    );
}

// ── Step 11: argmax / argmin over a 2-D Length-domain Sampled field ─────────

/// `argmax(field)` over a Sampled 2-D `Point2<Length>`-domain Real-codomain
/// field returns the per-axis coords at the index of the data buffer's
/// maximum, wrapped as `Value::Point` of two `Value::Scalar { LENGTH }`
/// components.
///
/// Shape 3×2 row-major (axis-0 outermost):
///   index   (i0, i1)   data
///     0      (0, 0)     1.0
///     1      (0, 1)     2.0
///     2      (1, 0)     3.0
///     3      (1, 1)     4.0
///     4      (2, 0)     9.0  ← max
///     5      (2, 1)     6.0
/// axis_0 = [0, 1, 2]; axis_1 = [10, 20]. Max at linear index 4 →
/// per-axis (2, 0) → coord (2.0, 10.0).
#[test]
fn argmax_sampled_field_2d_length_domain_returns_point2_at_max_index() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain = Type::point2(length.clone());
    let sf = make_sampled_2d(
        "f",
        vec![0.0, 1.0, 2.0],
        vec![10.0, 20.0],
        vec![1.0, 2.0, 3.0, 4.0, 9.0, 6.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::Real);

    let expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(field, field_type)],
        domain.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Point(vec![
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 10.0,
                dimension: DimensionVector::LENGTH,
            },
        ]),
        "argmax(field) over 2-D Point2<Length> domain should return the per-axis coords at the data max"
    );
}

/// `argmin(field)` over the same 2-D `Point2<Length>` field returns the
/// coord at the data buffer's minimum (linear index 0 → per-axis (0, 0) →
/// coord (0.0, 10.0)).
#[test]
fn argmin_sampled_field_2d_length_domain_returns_point2_at_min_index() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain = Type::point2(length.clone());
    let sf = make_sampled_2d(
        "f",
        vec![0.0, 1.0, 2.0],
        vec![10.0, 20.0],
        vec![1.0, 2.0, 3.0, 4.0, 9.0, 6.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::Real);

    let expr = make_function_call(
        "argmin",
        vec![CompiledExpr::literal(field, field_type)],
        domain.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Point(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 10.0,
                dimension: DimensionVector::LENGTH,
            },
        ]),
        "argmin(field) over 2-D Point2<Length> domain should return the per-axis coords at the data min"
    );
}

// ── Step 13: argmax / argmin over a 3-D Length-domain Sampled field ─────────

/// `argmax(field)` over a Sampled 3-D `Point3<Length>`-domain Real-codomain
/// field returns the per-axis coords at the data buffer's maximum, wrapped
/// as `Value::Point` of three `Value::Scalar { LENGTH }` components.
///
/// Shape (s0, s1, s2) = (2, 2, 3) → 12 cells row-major. We place a unique
/// max at linear index 7. Decomposition (axis-0 outermost, row-major):
///   i_2 = 7 % 3       = 1
///   i_1 = (7 / 3) % 2 = 0
///   i_0 = 7 / (2 * 3) = 1
/// → per-axis (1, 0, 1) → coord (axis_0[1], axis_1[0], axis_2[1])
///                      = (1.0, 0.0, 0.25).
#[test]
fn argmax_sampled_field_3d_length_domain_returns_point3_at_max_index() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain = Type::point3(length.clone());
    let sf = make_sampled_3d(
        "f",
        vec![0.0, 1.0],
        vec![0.0, 0.5],
        vec![0.0, 0.25, 0.5],
        // 12 reals; max at index 7 (= 99.0). All others smaller and unique.
        vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 99.0, 8.0, 9.0, 10.0, 11.0,
        ],
    );
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::Real);

    let expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(field, field_type)],
        domain.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Point(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.25,
                dimension: DimensionVector::LENGTH,
            },
        ]),
        "argmax(field) over 3-D Point3<Length> domain should return the per-axis coords at the data max"
    );
}

/// `argmin(field)` over the same 3-D `Point3<Length>` field — min at
/// linear index 0 (= 1.0) → per-axis (0, 0, 0) → coord (0.0, 0.0, 0.0).
#[test]
fn argmin_sampled_field_3d_length_domain_returns_point3_at_min_index() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain = Type::point3(length.clone());
    let sf = make_sampled_3d(
        "f",
        vec![0.0, 1.0],
        vec![0.0, 0.5],
        vec![0.0, 0.25, 0.5],
        vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 99.0, 8.0, 9.0, 10.0, 11.0,
        ],
    );
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::Real);

    let expr = make_function_call(
        "argmin",
        vec![CompiledExpr::literal(field, field_type)],
        domain.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Point(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]),
        "argmin(field) over 3-D Point3<Length> domain should return the per-axis coords at the data min"
    );
}

// ── Step 15: non-Sampled source kinds return Value::Undef ───────────────────

/// Build a `Value::Field` / `Type::Field` pair with an explicit source kind.
/// Lifted from `field_analysis_tests.rs::make_field_with_source`.
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

/// Build a constant-Real-codomain field over a `Type::Real` domain.
/// The lambda slot carries `Value::Undef` — none of the deferred-path tests
/// sample the field, only check the dispatch outcome (mirrors the Imported case).
fn make_constant_real_analytical_field(source: FieldSourceKind) -> (Value, Type) {
    make_field_with_source(Type::Real, Type::Real, source, Value::Undef)
}

/// Helper: assert all four field reductions return `Value::Undef` on a field
/// constructed with a non-Sampled source kind. Pins the deferred-path
/// contract for the v0.3 staging.
fn assert_all_reductions_undef(field: Value, field_type: Type, label: &str) {
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    for op in ["max", "min", "argmax", "argmin"] {
        let expr = make_function_call(
            op,
            vec![CompiledExpr::literal(field.clone(), field_type.clone())],
            Type::Real,
        );
        let result = eval_expr(&expr, &ctx);
        assert_eq!(
            result,
            Value::Undef,
            "{op}(field) on {label} should return Value::Undef (deferred path)"
        );
    }
}

/// `max`/`min`/`argmax`/`argmin` over an `Analytical`-source field return
/// `Value::Undef` (deferred — would require numerical optimisation over
/// the lambda's bounded domain).
#[test]
fn all_reductions_on_analytical_field_return_undef() {
    let (field, field_type) = make_constant_real_analytical_field(FieldSourceKind::Analytical);
    assert_all_reductions_undef(field, field_type, "Analytical");
}

/// `max`/`min`/`argmax`/`argmin` over a `Composed`-source field return
/// `Value::Undef`.
#[test]
fn all_reductions_on_composed_field_return_undef() {
    let (field, field_type) = make_constant_real_analytical_field(FieldSourceKind::Composed);
    assert_all_reductions_undef(field, field_type, "Composed");
}

/// `max`/`min`/`argmax`/`argmin` over an `Imported`-source field return
/// `Value::Undef`. Imported fields carry `Value::Undef` in the lambda
/// slot (no numeric data buffer at the runtime layer); reductions
/// therefore have nothing to iterate over.
#[test]
fn all_reductions_on_imported_field_return_undef() {
    let (field, field_type) = make_field_with_source(
        Type::Real,
        Type::Real,
        FieldSourceKind::Imported,
        Value::Undef,
    );
    assert_all_reductions_undef(field, field_type, "Imported");
}

/// `max`/`min`/`argmax`/`argmin` over a derived source that is NOT
/// `FieldSourceKind::VonMises` return `Value::Undef` (deferred — PRD §13
/// line 238). Using `MaxShear` as the representative still-deferred source
/// after `VonMises` was promoted to a fully-handled source kind in β.
///
/// This test is the surviving "other derived sources stay deferred" pin
/// after `all_reductions_on_derived_field_return_undef` was repurposed from
/// `VonMises` to `MaxShear` in task 4085 step S5.
#[test]
fn all_reductions_on_derived_non_vonmises_field_return_undef() {
    let (field, field_type) = make_constant_real_analytical_field(FieldSourceKind::MaxShear);
    assert_all_reductions_undef(field, field_type, "derived (MaxShear — still deferred)");
}

// ── Step 17: NaN-skip and empty-data semantics ──────────────────────────────

/// Construct a `SampledField` with empty data and a single-element axis —
/// bypasses the non-empty-data requirement of `build_sampled_field` for the
/// empty-data defense-in-depth pin. The reduction code must remain safe
/// when handed a directly-constructed pathological fixture.
fn make_sampled_empty() -> SampledField {
    SampledField {
        name: "empty".to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![0.0],
        spacing: vec![1.0],
        axis_grids: vec![vec![0.0]],
        interpolation: InterpolationKind::Linear,
        data: vec![],
        oob_emitted: AtomicBool::new(false),
    }
}

/// `max(field)` skips NaN values and returns the maximum of the finite
/// samples — `[1.0, NaN, 5.0, NaN, 3.0]` → `5.0`.
#[test]
fn max_sampled_with_nan_skips_nan_values() {
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, f64::NAN, 5.0, f64::NAN, 3.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, Type::Real);

    let expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Real(5.0),
        "max([1.0, NaN, 5.0, NaN, 3.0]) should skip NaN and return 5.0"
    );
}

/// `argmax(field)` skips NaN values and returns the coord at the index of
/// the maximum of the finite samples — `[1.0, NaN, 5.0, NaN, 3.0]` over
/// axis `[0,1,2,3,4]` → coord at index 2 → 2.0.
#[test]
fn argmax_sampled_with_nan_skips_nan_values() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, f64::NAN, 5.0, f64::NAN, 3.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::Real);

    let expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(field, field_type)],
        length.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmax([1.0, NaN, 5.0, NaN, 3.0]) should skip NaN and return coord at index 2"
    );
}

/// All four reductions return `Value::Undef` over a Sampled field whose
/// entire data buffer is non-finite (all NaN).
#[test]
fn all_reductions_sampled_all_nan_returns_undef() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    for op in ["max", "min", "argmax", "argmin"] {
        let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![f64::NAN, f64::NAN]);
        let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::Real);
        let expected_type = match op {
            "argmax" | "argmin" => length.clone(),
            _ => Type::Real,
        };
        let expr = make_function_call(
            op,
            vec![CompiledExpr::literal(field, field_type)],
            expected_type,
        );
        let result = eval_expr(&expr, &ctx);
        assert_eq!(
            result,
            Value::Undef,
            "{op}(field) over all-NaN Sampled data should return Value::Undef"
        );
    }
}

/// All four reductions return `Value::Undef` over a Sampled field with an
/// empty data buffer. Defense-in-depth pin: `build_sampled_field`'s
/// invariants normally prevent empty data, but the reduction code must
/// remain safe when constructed directly.
#[test]
fn all_reductions_sampled_empty_data_returns_undef() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    for op in ["max", "min", "argmax", "argmin"] {
        let sf = make_sampled_empty();
        let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::Real);
        let expected_type = match op {
            "argmax" | "argmin" => length.clone(),
            _ => Type::Real,
        };
        let expr = make_function_call(
            op,
            vec![CompiledExpr::literal(field, field_type)],
            expected_type,
        );
        let result = eval_expr(&expr, &ctx);
        assert_eq!(
            result,
            Value::Undef,
            "{op}(field) over empty Sampled data should return Value::Undef"
        );
    }
}

// ── Step 19: argcount-gating regression pins ───────────────────────────────

/// Regression pin: `max(field, scalar)` (2 args, first is Field) must NOT
/// be intercepted by our 1-arg-Field gate. The dispatch falls through to
/// `eval_builtin`'s binary `max(a, b)` (`reify-stdlib::numeric.rs:63`),
/// which expects scalar `as_f64()` operands — `Value::Field` has no
/// `as_f64()` mapping, so the binary form returns `Value::Undef`.
///
/// This pins the gating contract: the 1-arg-Field arm in `lib.rs` is the
/// ONLY path that reduces a field. A 2-arg call with a Field first arg
/// falls through to the binary numeric form and produces `Undef`.
#[test]
fn argcount_gating_max_field_then_extra_arg_returns_undef() {
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0]);
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, Type::Real);
    let expr = make_function_call(
        "max",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "max(field, scalar) (2 args) should fall through to binary numeric.rs::max and return Undef (Field has no as_f64)"
    );
}

/// Same regression pin for `min`.
#[test]
fn argcount_gating_min_field_then_extra_arg_returns_undef() {
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0]);
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, Type::Real);
    let expr = make_function_call(
        "min",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "min(field, scalar) (2 args) should fall through to binary numeric.rs::min and return Undef (Field has no as_f64)"
    );
}

/// Same regression pin for `argmax` (no binary form — falls through to
/// `eval_builtin` which has no binding for `argmax`, → `Undef`).
#[test]
fn argcount_gating_argmax_field_then_extra_arg_returns_undef() {
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0]);
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, Type::Real);
    let expr = make_function_call(
        "argmax",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "argmax(field, scalar) (2 args) should fall through to eval_builtin and return Undef (no binding)"
    );
}

/// Same regression pin for `argmin`.
#[test]
fn argcount_gating_argmin_field_then_extra_arg_returns_undef() {
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0]);
    let (field, field_type) = wrap_sampled_field(sf, Type::Real, Type::Real);
    let expr = make_function_call(
        "argmin",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "argmin(field, scalar) (2 args) should fall through to eval_builtin and return Undef (no binding)"
    );
}

// ── Step 20: shape-mismatch defense-in-depth ──────────────────────────────────

/// `argmax(field)` over a Sampled field whose `data.len()` does not equal
/// the product of the axis-grid lengths returns `Value::Undef`.
///
/// Defense-in-depth pin: `build_sampled_field`'s shape-equality invariant
/// (`engine_eval.rs`: rejects fields where `data.len() != prod(axis_lengths)`)
/// normally prevents this case. The `make_sampled_1d` helper enforces axis
/// non-emptiness but not the shape-product equality, so it transparently
/// produces the malformed fixture without modification — the same pattern
/// that `make_sampled_empty` uses for the empty-data test.
///
/// Pre-fix: returns `Value::Scalar { si_value: 0.0, dimension: LENGTH }`
/// because `argmax_argmin_index` returns `Some(4)` (linear index of the max
/// value `100.0`), then `decompose_index(4, &[2])` wraps `4 % 2 = 0` back
/// into bounds, yielding `axis_grids[0][0] = 0.0`.
#[test]
fn argmax_sampled_field_with_shape_mismatch_returns_undef() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    // axis length = 2, prod = 2; data length = 5 — shape mismatch (5 ≠ 2)
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0, 3.0, 4.0, 100.0]);
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::Real);

    let expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(field, field_type)],
        length.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Undef,
        "argmax(field) with data.len() != prod(axis_lengths) should return Value::Undef"
    );
}

/// `argmin(field)` over a Sampled field whose `data.len()` does not equal
/// the product of the axis-grid lengths returns `Value::Undef`.
///
/// Defense-in-depth pin: mirrors `argmax_sampled_field_with_shape_mismatch_returns_undef`.
///
/// Pre-fix: returns `Value::Scalar { si_value: 0.0, dimension: LENGTH }`
/// because `argmax_argmin_index` returns `Some(0)` (linear index of the min
/// value `1.0`), `decompose_index(0, &[2])` yields `per_axis = [0]`, and
/// `axis_grids[0][0] = 0.0`.
#[test]
fn argmin_sampled_field_with_shape_mismatch_returns_undef() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    // axis length = 2, prod = 2; data length = 5 — shape mismatch (5 ≠ 2)
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0, 3.0, 4.0, 100.0]);
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::Real);

    let expr = make_function_call(
        "argmin",
        vec![CompiledExpr::literal(field, field_type)],
        length.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Undef,
        "argmin(field) with data.len() != prod(axis_lengths) should return Value::Undef"
    );
}

/// `argmax(field)` over a 2-D Sampled field whose `data.len()` does not equal
/// the product of the axis-grid lengths returns `Value::Undef`.
///
/// Defense-in-depth pin: exercises the multi-axis product branch
/// (`axis_lengths[..n].iter().product()` for N=2) of the shape-mismatch guard
/// in `arg_coord_from_index`. axis0 = [0.0, 1.0] (length 2), axis1 =
/// [0.0, 1.0, 2.0] (length 3), prod = 2 × 3 = 6; data length = 5 — shape
/// mismatch (5 ≠ 6). The 1-D test reduces the product to a single term, so
/// this 2-D case is the smallest fixture that exercises the N>1 code path.
#[test]
fn argmax_sampled_field_2d_with_shape_mismatch_returns_undef() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain = Type::point2(length.clone());
    // axis0 length = 2, axis1 length = 3, prod = 6; data length = 5 — shape mismatch (5 ≠ 6)
    let sf = make_sampled_2d(
        "f",
        vec![0.0, 1.0],
        vec![0.0, 1.0, 2.0],
        vec![1.0, 2.0, 3.0, 4.0, 100.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::Real);

    let expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(field, field_type)],
        domain.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Undef,
        "argmax(field) over 2-D field with data.len() != prod(axis_lengths) should return Value::Undef"
    );
}

// ── Step S1: max / min reduce a VonMises-derived Sampled field ──────────────
//
// These tests are RED before the VonMises arm is implemented in
// `field_reductions.rs` (both return Value::Undef at the VonMises catch-all).
// After S2 they become GREEN.
//
// Uniaxial window convention: window_i = [σ_i, 0, 0, 0, 0, 0, 0, 0, 0]
// (σ_xx only, all off-diagonals and other diagonals zero).  For a uniaxial
// stress tensor von Mises == σ_xx exactly, which gives exact expected values
// without any approximation.

/// Build a 1-D `SampledField` with stride-9 row-major tensor data.
///
/// Each element `windows[i]` is a 9-float symmetric 3×3 matrix stored
/// row-major: [s_xx, s_xy, s_xz, s_yx, s_yy, s_yz, s_zx, s_zy, s_zz].
/// Axis coords are `axis[0], axis[1], ..., axis[K-1]`.
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

/// Wrap a stride-9 `SampledField` in a `Value::Field { source: Sampled }`
/// with the supplied domain and a Matrix3x3<PRESSURE> codomain.
fn wrap_sampled_tensor_field(sf: SampledField, domain: Type) -> Value {
    let codomain = Type::Matrix {
        m: 3,
        n: 3,
        quantity: Box::new(Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        }),
    };
    Value::Field {
        domain_type: domain,
        codomain_type: codomain,
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Uniaxial window for a single principal stress σ: [σ,0,0, 0,0,0, 0,0,0].
/// Von Mises == σ_xx == σ exactly for this window.
fn uniaxial_window(sigma: f64) -> [f64; 9] {
    [sigma, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
}

/// `max` / `min` over a VonMises-source field whose lambda is a 1-D Sampled
/// tensor field (directly constructed, bypassing `compute_von_mises` wrapping).
///
/// Uniaxial windows: σ_xx = {100e6, 250e6, 175e6} → von Mises = {100e6,
/// 250e6, 175e6}. Expected: max = 250e6 Pa, min = 100e6 Pa.
///
/// **RED before S2**: VonMises arm returns `Value::Undef`.
#[test]
fn max_min_von_mises_derived_sampled_field_returns_correct_extremum() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };

    // Build the inner Sampled tensor field (stride-9 uniaxial windows).
    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(175e6),
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, Type::Real);

    // Directly construct the VonMises-source field (lambda = inner tensor field).
    let (vonmises_field, vonmises_field_type) = make_field_with_source(
        Type::Real,
        pressure.clone(),
        FieldSourceKind::VonMises,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // max(vonmises_field) should be 250e6 Pa
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(vonmises_field.clone(), vonmises_field_type.clone())],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&max_expr, &ctx),
        Value::Scalar {
            si_value: 250e6,
            dimension: DimensionVector::PRESSURE,
        },
        "max(VonMises-derived field) should return 250e6 Pa (the projected maximum)"
    );

    // min(vonmises_field) should be 100e6 Pa
    let min_expr = make_function_call(
        "min",
        vec![CompiledExpr::literal(vonmises_field, vonmises_field_type)],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&min_expr, &ctx),
        Value::Scalar {
            si_value: 100e6,
            dimension: DimensionVector::PRESSURE,
        },
        "min(VonMises-derived field) should return 100e6 Pa (the projected minimum)"
    );
}

/// B3 composition: `max` evaluated via `eval_expr` over a VonMises-source
/// field whose lambda is a Sampled tensor field — exercises the real
/// `eval_expr` dispatch path for the `max` → VonMises reduction chain.
///
/// Same tensor data as the test above; the VonMises field is pre-built and
/// passed as a literal argument to the `max` function call so the full
/// `eval_expr` dispatch fires (matching the "directly-constructed Sampled
/// tensor field" requirement in the design decisions).
///
/// **RED before S2**: VonMises arm returns `Value::Undef`.
#[test]
fn b3_max_of_von_mises_field_via_eval_expr_dispatch() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };

    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(175e6),
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, Type::Real);

    let (vonmises_field, vonmises_field_type) = make_field_with_source(
        Type::Real,
        pressure.clone(),
        FieldSourceKind::VonMises,
        inner_tensor_field,
    );

    // max(vonmises_field) — tested through the real eval_expr dispatch.
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(vonmises_field, vonmises_field_type)],
        pressure.clone(),
    );

    let values = ValueMap::new();
    let result = eval_expr(&max_expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Scalar {
            si_value: 250e6,
            dimension: DimensionVector::PRESSURE,
        },
        "B3: max(VonMises field over sampled tensor data) must be non-Undef Scalar<Pressure>"
    );
}

// ── Step S3: argmax / argmin reduce a VonMises-derived Sampled field ─────────
//
// RED before S4: compute_argextremum's VonMises path returns Value::Undef.
// After S4 these become GREEN.

/// `argmax` / `argmin` over a 1-D VonMises field with Scalar<LENGTH> domain.
///
/// Inner tensor field: 1-D Sampled, axis = [0.0, 1.0, 2.0] (LENGTH),
/// uniaxial windows σ_xx = {100e6, 250e6, 175e6} → vM = {100e6, 250e6, 175e6}.
/// argmax → coord at index 1 → 1.0 m; argmin → coord at index 0 → 0.0 m.
///
/// **RED before S4**.
#[test]
fn argmax_argmin_von_mises_field_1d_length_domain_returns_coord() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(175e6),
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, length.clone());

    let (vonmises_field, vonmises_field_type) = make_field_with_source(
        length.clone(),
        pressure.clone(),
        FieldSourceKind::VonMises,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // argmax → index 1 → coord 1.0 m
    let argmax_expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(vonmises_field.clone(), vonmises_field_type.clone())],
        length.clone(),
    );
    assert_eq!(
        eval_expr(&argmax_expr, &ctx),
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmax(VonMises 1-D field) should return coord at projected max (index 1 → 1.0 m)"
    );

    // argmin → index 0 → coord 0.0 m
    let argmin_expr = make_function_call(
        "argmin",
        vec![CompiledExpr::literal(vonmises_field, vonmises_field_type)],
        length.clone(),
    );
    assert_eq!(
        eval_expr(&argmin_expr, &ctx),
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmin(VonMises 1-D field) should return coord at projected min (index 0 → 0.0 m)"
    );
}

/// `argmax` over a 3-D VonMises field with Point3<LENGTH> domain.
///
/// Inner tensor field: 3-D Sampled, axes = [0.0, 1.0] × [0.0] × [0.0, 0.5].
/// Shape (2, 1, 2) → 4 grid points, row-major linear indices 0-3.
/// Uniaxial windows: σ_xx = {100e6, 175e6, 50e6, 250e6} → vM = same.
/// Max at linear index 3 → per-axis (1, 0, 1) → coord (1.0, 0.0, 0.5).
/// Min at linear index 2 → per-axis (1, 0, 0) → coord (1.0, 0.0, 0.0).
///
/// Decomposition (axis-0 outermost, row-major):
///   i_2 = 3 % 2 = 1
///   i_1 = (3 / 2) % 1 = 0
///   i_0 = 3 / (1 * 2) = 1
/// → axis_0[1]=1.0, axis_1[0]=0.0, axis_2[1]=0.5.
///
/// **RED before S4**.
#[test]
fn argmax_von_mises_field_3d_length_domain_returns_point3_at_max() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain = Type::point3(length.clone());

    // Shape (2, 1, 2): axis0=[0,1], axis1=[0], axis2=[0,0.5].
    let inner_sf = {
        let bounds_min = vec![0.0, 0.0, 0.0];
        let bounds_max = vec![1.0, 0.0, 0.5];
        let spacing = vec![1.0, 1.0, 0.5];
        let axis0 = vec![0.0, 1.0];
        let axis1 = vec![0.0];
        let axis2 = vec![0.0, 0.5];
        // Row-major linear ordering (axis-0 outermost):
        //   idx 0: (0,0,0) → σ=100e6
        //   idx 1: (0,0,1) → σ=175e6
        //   idx 2: (1,0,0) → σ=50e6
        //   idx 3: (1,0,1) → σ=250e6  ← max
        let windows = vec![
            uniaxial_window(100e6),
            uniaxial_window(175e6),
            uniaxial_window(50e6),
            uniaxial_window(250e6),
        ];
        let mut data: Vec<f64> = Vec::with_capacity(4 * 9);
        for w in &windows {
            data.extend_from_slice(w);
        }
        SampledField {
            name: "stress_3d".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min,
            bounds_max,
            spacing,
            axis_grids: vec![axis0, axis1, axis2],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    };

    let inner_tensor_field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(pressure.clone()),
        },
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(inner_sf)),
    };

    let (vonmises_field, vonmises_field_type) = make_field_with_source(
        domain.clone(),
        pressure.clone(),
        FieldSourceKind::VonMises,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // argmax → linear index 3 → (1, 0, 1) → (1.0, 0.0, 0.5)
    let argmax_expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(vonmises_field.clone(), vonmises_field_type.clone())],
        domain.clone(),
    );
    assert_eq!(
        eval_expr(&argmax_expr, &ctx),
        Value::Point(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.5,
                dimension: DimensionVector::LENGTH,
            },
        ]),
        "argmax(VonMises 3-D field) should return Point3 at projected max (linear idx 3)"
    );

    // argmin → linear index 2 → (1, 0, 0) → (1.0, 0.0, 0.0)
    let argmin_expr = make_function_call(
        "argmin",
        vec![CompiledExpr::literal(vonmises_field, vonmises_field_type)],
        domain.clone(),
    );
    assert_eq!(
        eval_expr(&argmin_expr, &ctx),
        Value::Point(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]),
        "argmin(VonMises 3-D field) should return Point3 at projected min (linear idx 2)"
    );
}

// ── Step S5: defensive / negative contract pins for the VonMises path ────────
//
// These tests verify the guards delivered in S2/S4 (`project_von_mises_sampled`
// returning None on malformed inputs) and the NaN-skip behaviour.

/// All four reductions return `Value::Undef` when the VonMises field's lambda
/// is NOT a Sampled `Value::Field` (e.g. `Value::Undef`).
///
/// This pins the `project_von_mises_sampled` level-1 defensive arm and
/// preserves a VonMises negative pin after the stale
/// `all_reductions_on_derived_field_return_undef` test was repurposed to
/// `MaxShear` in this step.
#[test]
fn all_reductions_on_vonmises_field_with_non_sampled_lambda_return_undef() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    // VonMises field whose lambda is Value::Undef (not a Sampled Field).
    let (field, field_type) = make_field_with_source(
        Type::Real,
        pressure,
        FieldSourceKind::VonMises,
        Value::Undef,
    );
    assert_all_reductions_undef(field, field_type, "VonMises with non-Sampled lambda (Undef)");
}

/// All four reductions return `Value::Undef` when the VonMises field's backing
/// `SampledField` violates the stride-9 contract (`data.len() != grid_count * 9`).
///
/// Pins the `grid_count == 0 || sf.data.len() != grid_count * 9` guard in
/// `project_von_mises_sampled`.
#[test]
fn all_reductions_on_vonmises_field_with_stride_violation_return_undef() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    // Stride violation: axis has 3 points (grid_count=3), data has 3*8=24
    // values instead of 3*9=27 — one short per window.
    let bad_sf = {
        let axis = vec![0.0, 1.0, 2.0];
        let mut data = Vec::with_capacity(24);
        for _ in 0..3 {
            data.extend_from_slice(&[1.0_f64; 8]); // 8-float windows, not 9
        }
        SampledField {
            name: "bad_stride".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![2.0],
            spacing: vec![1.0],
            axis_grids: vec![axis],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    };
    let inner_tensor_field = Value::Field {
        domain_type: Type::Real,
        codomain_type: Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(pressure.clone()),
        },
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(bad_sf)),
    };
    let (field, field_type) = make_field_with_source(
        Type::Real,
        pressure,
        FieldSourceKind::VonMises,
        inner_tensor_field,
    );
    assert_all_reductions_undef(
        field,
        field_type,
        "VonMises with stride-contract violation (data.len() != grid_count*9)",
    );
}

/// All four reductions return `Value::Undef` when every projected window is
/// NaN (all-out-of-solid sentinel).
///
/// Pins the NaN-skip + all-finite-absent → Undef chain:
/// `compute_von_mises_3x3` of an all-NaN window returns NaN,
/// `argmax_argmin_index` skips NaN and returns `None`, → `Value::Undef`.
#[test]
fn all_reductions_on_vonmises_field_with_all_nan_windows_return_undef() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    // All-NaN windows — every window is [NaN; 9], simulating fully
    // out-of-solid sentinel values from the FEA elaborator.
    let nan_sf = make_sampled_tensor_1d(
        "all_nan",
        vec![0.0, 1.0],
        vec![
            [f64::NAN; 9],
            [f64::NAN; 9],
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(nan_sf, Type::Real);
    let (field, field_type) = make_field_with_source(
        Type::Real,
        pressure,
        FieldSourceKind::VonMises,
        inner_tensor_field,
    );
    assert_all_reductions_undef(
        field,
        field_type,
        "VonMises with all-NaN projected windows (all-out-of-solid)",
    );
}

/// Positive NaN-skip: when SOME windows are NaN (out-of-solid sentinel)
/// and others are finite, the reduction operates only over the finite-projected
/// windows.
///
/// Setup: 3 windows — NaN, σ=250e6, NaN — projected: NaN, 250e6, NaN.
/// max → 250e6 Pa (only finite window), argmax → coord 1.0 m (axis[1]).
#[test]
fn reductions_on_vonmises_field_with_partial_nan_windows_skip_nan() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    let sf = make_sampled_tensor_1d(
        "partial_nan",
        vec![0.0, 1.0, 2.0],
        vec![
            [f64::NAN; 9],        // out-of-solid sentinel
            uniaxial_window(250e6), // finite: σ=250e6 Pa
            [f64::NAN; 9],        // out-of-solid sentinel
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(sf, length.clone());
    let (field, field_type) = make_field_with_source(
        length.clone(),
        pressure.clone(),
        FieldSourceKind::VonMises,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // max → the single finite projected window: 250e6 Pa
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field.clone(), field_type.clone())],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&max_expr, &ctx),
        Value::Scalar {
            si_value: 250e6,
            dimension: DimensionVector::PRESSURE,
        },
        "max(VonMises field with partial NaN) should skip NaN and return 250e6 Pa"
    );

    // argmax → the finite window at axis index 1 → coord 1.0 m
    let argmax_expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(field, field_type)],
        length.clone(),
    );
    assert_eq!(
        eval_expr(&argmax_expr, &ctx),
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmax(VonMises field with partial NaN) should skip NaN and return coord 1.0 m"
    );
}
