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
use reify_types::{
    CompiledExpr, CompiledExprKind, ContentHash, DimensionVector, FieldSourceKind,
    InterpolationKind, ResolvedFunction, SampledField, SampledGridKind, Type, Value, ValueMap,
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
fn make_sampled_2d(
    name: &str,
    axis0: Vec<f64>,
    axis1: Vec<f64>,
    data: Vec<f64>,
) -> SampledField {
    let bounds_min = vec![
        *axis0.first().expect("axis0 must be non-empty"),
        *axis1.first().expect("axis1 must be non-empty"),
    ];
    let bounds_max = vec![
        *axis0.last().expect("axis0 must be non-empty"),
        *axis1.last().expect("axis1 must be non-empty"),
    ];
    let spacing = vec![
        if axis0.len() >= 2 { axis0[1] - axis0[0] } else { 1.0 },
        if axis1.len() >= 2 { axis1[1] - axis1[0] } else { 1.0 },
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
    let sf = make_sampled_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![100e6, 250e6, 175e6],
    );
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
    let sf = make_sampled_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![100e6, 250e6, 175e6],
    );
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
