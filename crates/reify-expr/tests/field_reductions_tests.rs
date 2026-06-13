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
use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, FieldSourceKind, InterpolationKind, ResolvedFunction, SampledField, SampledGridKind, Value, ValueMap};

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

// ── Bounded-reduction test fixtures ─────────────────────────────────────────

/// Build a `Value::BoundingBox` whose three corner components are either
/// `Value::Real` (dimensionless) or `Value::Scalar { dimension: dim }`
/// (dimensioned).
///
/// Pass `DimensionVector::DIMENSIONLESS` to get Real components; pass e.g.
/// `DimensionVector::LENGTH` to get dimensioned Scalar components.
///
/// The box is always a 3-component `Value::Point`, matching the canonical
/// `bounding_box(solid)` shape. For n-D field tests (n < 3), only the first
/// n axes are inspected by the bounded reductions.
fn make_bbox(min: [f64; 3], max: [f64; 3], dim: DimensionVector) -> Value {
    let make_comp = |v: f64| -> Value {
        if dim.is_dimensionless() {
            Value::Real(v)
        } else {
            Value::Scalar { si_value: v, dimension: dim }
        }
    };
    Value::BoundingBox {
        min: Box::new(Value::Point(vec![
            make_comp(min[0]),
            make_comp(min[1]),
            make_comp(min[2]),
        ])),
        max: Box::new(Value::Point(vec![
            make_comp(max[0]),
            make_comp(max[1]),
            make_comp(max[2]),
        ])),
    }
}

/// Build a `Value::Lambda` with (name, id) param pairs.
///
/// Lifted directly from `gradient_tests.rs:69`.  Used for constructing
/// `Analytical`-source field lambdas in bounded-reduction tests.
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

/// Build a `Value::Field` with an explicit source kind and a lambda value.
///
/// Alias for the local `make_field_with_source` helper (which appears later
/// in this file).  Provided here for semantic clarity in bounded-reduction
/// test construction, before `make_field_with_source` is visible to the reader.
///
/// SAFETY: no code here references `make_field_with_source` at declaration time;
/// Rust resolves all function names at call time within the same module.
fn make_analytical_field(
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

/// Build a 2-arg `FunctionCall` CompiledExpr for `op(field, bbox)`.
///
/// Used as the primary fixture factory for bounded-reduction dispatch tests:
/// the result is passed to `eval_expr` to exercise the full dispatch chain.
fn make_bounded_call(
    op: &str,
    field: Value,
    field_type: Type,
    bounds: Value,
    result_type: Type,
) -> CompiledExpr {
    make_function_call(
        op,
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(bounds, Type::BoundingBox),
        ],
        result_type,
    )
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
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());

    let expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field, field_type)],
        Type::dimensionless_scalar(),
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
            CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(5.0), Type::dimensionless_scalar()),
        ],
        Type::dimensionless_scalar(),
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
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());

    let expr = make_function_call(
        "min",
        vec![CompiledExpr::literal(field, field_type)],
        Type::dimensionless_scalar(),
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
            CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(5.0), Type::dimensionless_scalar()),
        ],
        Type::dimensionless_scalar(),
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
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), pressure.clone());

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
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::dimensionless_scalar());

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
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());

    let expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(field, field_type)],
        Type::dimensionless_scalar(),
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
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), pressure.clone());

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
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::dimensionless_scalar());

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
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::dimensionless_scalar());

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
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::dimensionless_scalar());

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
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::dimensionless_scalar());

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
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::dimensionless_scalar());

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

/// Build a constant-Real-codomain field over a `Type::dimensionless_scalar()` domain.
/// The lambda slot carries `Value::Undef` — none of the deferred-path tests
/// sample the field, only check the dispatch outcome (mirrors the Imported case).
fn make_constant_real_analytical_field(source: FieldSourceKind) -> (Value, Type) {
    make_field_with_source(Type::dimensionless_scalar(), Type::dimensionless_scalar(), source, Value::Undef)
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
            Type::dimensionless_scalar(),
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
        Type::dimensionless_scalar(),
        Type::dimensionless_scalar(),
        FieldSourceKind::Imported,
        Value::Undef,
    );
    assert_all_reductions_undef(field, field_type, "Imported");
}

/// `max`/`min`/`argmax`/`argmin` over a derived source that is NOT yet
/// fully handled (still deferred) return `Value::Undef`. Using
/// `Gradient` as the representative still-deferred source after
/// `VonMises` (task 4085), `MaxShear` (task 4543), and `PrincipalStresses`
/// (task 4562) were promoted to fully-handled source kinds.
///
/// This test is the surviving "other derived sources stay deferred" pin.
/// Retarget history:
///   - VonMises → MaxShear (task 4085 step S5)
///   - MaxShear → PrincipalStresses (task 4543 step S5)
///   - PrincipalStresses → Gradient (task 4562 step-5)
///
/// Gradient (a differential operator) is deferred to the
/// differential-field-reductions PRD; it is and remains Undef in
/// `compute_extremum`/`compute_argextremum` via the `_ => Undef` fall-through.
#[test]
fn all_reductions_on_deferred_differential_field_return_undef() {
    let (field, field_type) =
        make_constant_real_analytical_field(FieldSourceKind::Gradient);
    assert_all_reductions_undef(
        field,
        field_type,
        "derived (Gradient — still deferred → differential-field-reductions PRD)",
    );
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
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());

    let expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field, field_type)],
        Type::dimensionless_scalar(),
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
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::dimensionless_scalar());

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
        let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::dimensionless_scalar());
        let expected_type = match op {
            "argmax" | "argmin" => length.clone(),
            _ => Type::dimensionless_scalar(),
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
        let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::dimensionless_scalar());
        let expected_type = match op {
            "argmax" | "argmin" => length.clone(),
            _ => Type::dimensionless_scalar(),
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
//
// Contract (updated in task 4561): a `Value::BoundingBox` 2nd argument now
// triggers the bounded reduction path (`compute_*_bounded`).  A non-BoundingBox
// 2nd arg (e.g. a scalar `Real`) does NOT match the new gate and falls
// through as before — `max`/`min` to `eval_builtin` → binary numeric form
// → Undef (Field has no `as_f64`); `argmax`/`argmin` to `eval_builtin` →
// no binding → Undef.  The positive counterpart (`max(field, bbox)` reduces)
// is exercised by the new bounded-Sampled tests added in step-1 above.

/// Regression pin: `max(field, scalar)` (2 args, first is Field, second is
/// a non-BoundingBox scalar) falls through to the binary numeric form and
/// returns `Value::Undef` (Field has no `as_f64` mapping).
///
/// The updated dispatch contract: a `Value::BoundingBox` 2nd arg triggers the
/// bounded reduction; a non-BoundingBox scalar 2nd arg falls through to
/// `eval_builtin`'s binary `max(a,b)` which cannot coerce a Field → Undef.
#[test]
fn argcount_gating_max_field_then_non_bbox_arg_returns_undef() {
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0]);
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
    let expr = make_function_call(
        "max",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar()),
        ],
        Type::dimensionless_scalar(),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "max(field, scalar) (non-BoundingBox 2nd arg) should fall through to binary numeric.rs::max → Undef"
    );
}

/// Same regression pin for `min`: non-BoundingBox scalar 2nd arg → Undef.
#[test]
fn argcount_gating_min_field_then_non_bbox_arg_returns_undef() {
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0]);
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
    let expr = make_function_call(
        "min",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar()),
        ],
        Type::dimensionless_scalar(),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "min(field, scalar) (non-BoundingBox 2nd arg) should fall through to binary numeric.rs::min → Undef"
    );
}

/// Same regression pin for `argmax`: non-BoundingBox scalar 2nd arg → Undef
/// (no binary `argmax` form; `eval_builtin` has no binding for `argmax`).
#[test]
fn argcount_gating_argmax_field_then_non_bbox_arg_returns_undef() {
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0]);
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
    let expr = make_function_call(
        "argmax",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar()),
        ],
        Type::dimensionless_scalar(),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "argmax(field, scalar) (non-BoundingBox 2nd arg) falls through to eval_builtin → Undef (no binding)"
    );
}

/// Same regression pin for `argmin`: non-BoundingBox scalar 2nd arg → Undef.
#[test]
fn argcount_gating_argmin_field_then_non_bbox_arg_returns_undef() {
    let sf = make_sampled_1d("f", vec![0.0, 1.0], vec![1.0, 2.0]);
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
    let expr = make_function_call(
        "argmin",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar()),
        ],
        Type::dimensionless_scalar(),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "argmin(field, scalar) (non-BoundingBox 2nd arg) falls through to eval_builtin → Undef (no binding)"
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
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::dimensionless_scalar());

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
    let (field, field_type) = wrap_sampled_field(sf, length.clone(), Type::dimensionless_scalar());

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
    let (field, field_type) = wrap_sampled_field(sf, domain.clone(), Type::dimensionless_scalar());

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

// ── Bounded Sampled sub-region tests (step-1 RED / step-2 GREEN) ────────────
//
// Fixture: 1-D SampledField with Real domain and Real codomain.
//   axis  = [0, 1, 2, 3, 4]
//   data  = [1, 5, 3, 4, 2]
//   bbox  x ∈ [2, 4] → in-bounds nodes: {x=2→3, x=3→4, x=4→2}
//
// Exact expectations (data values, no interpolation):
//   max   = 4.0  (data at x=3)
//   min   = 2.0  (data at x=4)
//   argmax = Value::Real(3.0)  (coord at the max)
//   argmin = Value::Real(4.0)  (coord at the min)
//
// Empty sub-region (bbox x ∈ [10, 20]) — no grid nodes → Undef.
//
// **RED before step-2 (Sampled arm not yet implemented).**

/// `max(sampled_1d_field, bbox)` clips to the sub-region and returns the maximum
/// data value within the bounding box.
///
/// axis [0..4] / data [1,5,3,4,2], bbox x∈[2,4]:
/// in-bounds {x=2→3, x=3→4, x=4→2} → max = 4.0.
#[test]
fn max_sampled_field_bounded_subregion_returns_max_in_bbox() {
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
    let bbox = make_bbox([2.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("max", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(4.0),
        "max(sampled 1-D, bbox x∈[2,4]) should be 4.0 (data at x=3)"
    );
}

/// `min(sampled_1d_field, bbox)` clips to the sub-region and returns the minimum
/// data value within the bounding box.
///
/// axis [0..4] / data [1,5,3,4,2], bbox x∈[2,4]:
/// in-bounds {x=2→3, x=3→4, x=4→2} → min = 2.0.
#[test]
fn min_sampled_field_bounded_subregion_returns_min_in_bbox() {
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
    let bbox = make_bbox([2.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("min", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(2.0),
        "min(sampled 1-D, bbox x∈[2,4]) should be 2.0 (data at x=4)"
    );
}

/// `argmax(sampled_1d_field, bbox)` clips to the sub-region and returns the
/// domain coordinate at the maximum.
///
/// axis [0..4] / data [1,5,3,4,2], bbox x∈[2,4]:
/// in-bounds {x=2→3, x=3→4, x=4→2} → argmax → x=3 → Value::Real(3.0).
#[test]
fn argmax_sampled_field_bounded_subregion_returns_coord_at_max() {
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
    let bbox = make_bbox([2.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("argmax", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(3.0),
        "argmax(sampled 1-D, bbox x∈[2,4]) should be coord 3.0 (data[3]=4 is the max)"
    );
}

/// `argmin(sampled_1d_field, bbox)` clips to the sub-region and returns the
/// domain coordinate at the minimum.
///
/// axis [0..4] / data [1,5,3,4,2], bbox x∈[2,4]:
/// in-bounds {x=2→3, x=3→4, x=4→2} → argmin → x=4 → Value::Real(4.0).
#[test]
fn argmin_sampled_field_bounded_subregion_returns_coord_at_min() {
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
    let bbox = make_bbox([2.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("argmin", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(4.0),
        "argmin(sampled 1-D, bbox x∈[2,4]) should be coord 4.0 (data[4]=2 is the min)"
    );
}

/// `max(sampled_1d_field, bbox)` where the bounding box excludes ALL grid nodes
/// returns `Value::Undef` (empty sub-region).
///
/// axis [0..4], bbox x∈[10,20] — no grid points in range → Undef.
#[test]
fn max_sampled_field_bounded_empty_subregion_returns_undef() {
    let sf = make_sampled_1d(
        "f",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![1.0, 5.0, 3.0, 4.0, 2.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
    let bbox = make_bbox([10.0, 0.0, 0.0], [20.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("max", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "max(sampled 1-D, bbox outside all nodes) should return Undef (empty sub-region)"
    );
}

// ── Bounded Analytical/Composed grid-sampling tests (step-3 RED / step-4 GREEN) ──
//
// The Analytical/Composed arm in compute_bounded_extremum / compute_bounded_argextremum
// is NOT yet implemented (step-4).  All positive tests below currently return
// Value::Undef — they become GREEN in step-4.
//
// Negative pins (tests 5-8 below) already expect Value::Undef and pass now;
// they remain GREEN after step-4 lands.

// ── step-3 helper: build an Analytical 1-D lambda ──────────────────────────
//
// Lambda `|x| scale * x` over Type::dimensionless_scalar() domain — evaluates to a Real.
//
// Body = BinOp::Mul(literal(scale), value_ref(x_id)).
fn make_linear_lambda(scale: f64) -> Value {
    let x_id = ValueCellId::new("$lambda_linear.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(scale), Type::dimensionless_scalar()),
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    make_value_lambda(vec![("x", x_id)], body, ValueMap::new())
}

// ── step-3 helper: build a quadratic-peak lambda ────────────────────────────
//
// Lambda `|x| apex - (x - center)^2` — evaluated via `(x-center)*(x-center)`.
//
// Body = Sub(literal(apex), Mul(Sub(x_ref, literal(center)), Sub(x_ref, literal(center)))).
fn make_quadratic_peak_lambda(apex: f64, center: f64) -> Value {
    let x_id = ValueCellId::new("$lambda_peak.S", "x");
    let x_ref = || CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar());
    let diff = CompiledExpr::binop(
        BinOp::Sub,
        x_ref(),
        CompiledExpr::literal(Value::Real(center), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    let diff2 = CompiledExpr::binop(
        BinOp::Sub,
        x_ref(),
        CompiledExpr::literal(Value::Real(center), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    let sq = CompiledExpr::binop(BinOp::Mul, diff, diff2, Type::dimensionless_scalar());
    let body = CompiledExpr::binop(
        BinOp::Sub,
        CompiledExpr::literal(Value::Real(apex), Type::dimensionless_scalar()),
        sq,
        Type::dimensionless_scalar(),
    );
    make_value_lambda(vec![("x", x_id)], body, ValueMap::new())
}

// ── step-3 helper: build a 2-D additive lambda ──────────────────────────────
//
// Lambda `|x, y| x + y` — parameters are individually bound; called via
// `apply_lambda_with_point_unpacking` which auto-unpacks Point(x,y) into (x, y).
fn make_sum2d_lambda() -> Value {
    let x_id = ValueCellId::new("$lambda_sum2d.S", "x");
    let y_id = ValueCellId::new("$lambda_sum2d.S", "y");
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::value_ref(y_id.clone(), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    make_value_lambda(vec![("x", x_id), ("y", y_id)], body, ValueMap::new())
}

// ── step-3 helper: build a NaN-returning lambda ─────────────────────────────
//
// Lambda `|x| NaN` — body is a literal `Value::Real(f64::NAN)`.
// Used to verify that all-non-finite results → Undef (the grid-sampler
// skips non-finite values via `as_f64()` + `is_finite()`).
fn make_nan_lambda() -> Value {
    let x_id = ValueCellId::new("$lambda_nan.S", "x");
    let body = CompiledExpr::literal(Value::Real(f64::NAN), Type::dimensionless_scalar());
    make_value_lambda(vec![("x", x_id)], body, ValueMap::new())
}

// ── step-3 helper: build a Vector-returning lambda ──────────────────────────
//
// Lambda `|x| [1, 2, 3]` — body is a literal Vector.
// `as_f64()` on a Vector returns None → result is skipped by the grid-sampler → Undef.
fn make_vector_returning_lambda() -> Value {
    let x_id = ValueCellId::new("$lambda_vec.S", "x");
    let vec_val = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
    let body = CompiledExpr::literal(vec_val, Type::vec3(Type::dimensionless_scalar()));
    make_value_lambda(vec![("x", x_id)], body, ValueMap::new())
}

/// `max(analytical_1d, bbox)` over a monotonic linear field `|x| 3*x`
/// with bbox x∈[0,4].
///
/// Grid: 11 nodes on [0,4] → {0.0, 0.4, 0.8, ..., 4.0}.
/// Max at x=4 (endpoint, always a grid node) → 3*4 = 12.0 EXACT.
///
/// **RED before step-4.**
#[test]
fn max_analytical_1d_linear_returns_max_at_endpoint() {
    let lambda = make_linear_lambda(3.0);
    let (field, field_type) = make_analytical_field(Type::dimensionless_scalar(), Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("max", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(12.0),
        "max(|x| 3x, bbox x∈[0,4]) = 12.0 at x=4 (endpoint node, exact)"
    );
}

/// `min(analytical_1d, bbox)` over `|x| 3*x` with bbox x∈[0,4].
///
/// Min at x=0 (endpoint) → 0.0 EXACT.
///
/// **RED before step-4.**
#[test]
fn min_analytical_1d_linear_returns_min_at_endpoint() {
    let lambda = make_linear_lambda(3.0);
    let (field, field_type) = make_analytical_field(Type::dimensionless_scalar(), Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("min", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(0.0),
        "min(|x| 3x, bbox x∈[0,4]) = 0.0 at x=0 (endpoint node, exact)"
    );
}

/// `argmax(analytical_1d, bbox)` over `|x| 3*x` with bbox x∈[0,4].
///
/// argmax at x=4 → Value::Real(4.0) EXACT.
///
/// **RED before step-4.**
#[test]
fn argmax_analytical_1d_linear_returns_coord_at_endpoint() {
    let lambda = make_linear_lambda(3.0);
    let (field, field_type) = make_analytical_field(Type::dimensionless_scalar(), Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("argmax", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(4.0),
        "argmax(|x| 3x, bbox x∈[0,4]) = 4.0 (endpoint node, exact)"
    );
}

/// `argmin(analytical_1d, bbox)` over `|x| 3*x` with bbox x∈[0,4].
///
/// argmin at x=0 → Value::Real(0.0) EXACT.
///
/// **RED before step-4.**
#[test]
fn argmin_analytical_1d_linear_returns_coord_at_endpoint() {
    let lambda = make_linear_lambda(3.0);
    let (field, field_type) = make_analytical_field(Type::dimensionless_scalar(), Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("argmin", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(0.0),
        "argmin(|x| 3x, bbox x∈[0,4]) = 0.0 (endpoint node, exact)"
    );
}

/// `max(analytical_1d, bbox)` over a quadratic-peak field `|x| 10-(x-2)^2`
/// with bbox x∈[0,4].
///
/// Grid: 11 nodes on [0,4] → node 5 is x=2.0 (box center, exact because
/// count is ODD).  f(2.0) = 10 - 0 = 10.0 EXACT.
///
/// **RED before step-4.**
#[test]
fn max_analytical_1d_interior_peak_returns_peak_value() {
    let lambda = make_quadratic_peak_lambda(10.0, 2.0);
    let (field, field_type) = make_analytical_field(Type::dimensionless_scalar(), Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("max", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(10.0),
        "max(|x| 10-(x-2)^2, bbox x∈[0,4]) = 10.0 (box-center node x=2, exact)"
    );
}

/// `argmax(analytical_1d, bbox)` over `|x| 10-(x-2)^2` with bbox x∈[0,4].
///
/// argmax at x=2.0 (box-center node) → Value::Real(2.0) EXACT.
///
/// **RED before step-4.**
#[test]
fn argmax_analytical_1d_interior_peak_returns_center_coord() {
    let lambda = make_quadratic_peak_lambda(10.0, 2.0);
    let (field, field_type) = make_analytical_field(Type::dimensionless_scalar(), Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("argmax", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(2.0),
        "argmax(|x| 10-(x-2)^2, bbox x∈[0,4]) = 2.0 (box-center node, exact)"
    );
}

/// `max(analytical_2d, bbox)` over `|x,y| x+y` with Point2<Real> domain,
/// bbox [0,2]×[0,3].
///
/// Grid: 11×11 nodes.  Max at corner (2.0, 3.0) → f(2,3) = 5.0 EXACT.
///
/// **RED before step-4.**
#[test]
fn max_analytical_2d_additive_returns_max_at_corner() {
    let lambda = make_sum2d_lambda();
    let domain = Type::Point { n: 2, quantity: Box::new(Type::dimensionless_scalar()) };
    let (field, field_type) = make_analytical_field(domain, Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [2.0, 3.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("max", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(5.0),
        "max(|x,y| x+y, bbox [0,2]×[0,3]) = 5.0 at corner (2,3), exact"
    );
}

/// `argmax(analytical_2d, bbox)` over `|x,y| x+y` with Point2<Real> domain,
/// bbox [0,2]×[0,3].
///
/// argmax at corner (2.0, 3.0) → Value::Point([Real(2.0), Real(3.0)]) EXACT.
///
/// **RED before step-4.**
#[test]
fn argmax_analytical_2d_additive_returns_corner_point() {
    let lambda = make_sum2d_lambda();
    let domain = Type::Point { n: 2, quantity: Box::new(Type::dimensionless_scalar()) };
    let (field, field_type) = make_analytical_field(domain, Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [2.0, 3.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("argmax", field, field_type, bbox, Type::Point { n: 2, quantity: Box::new(Type::dimensionless_scalar()) });
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Point(vec![Value::Real(2.0), Value::Real(3.0)]),
        "argmax(|x,y| x+y, bbox [0,2]×[0,3]) = Point(2.0, 3.0) at corner, exact"
    );
}

/// `max(composed_field, bbox)` over a Composed-source field with `|x| 2*x`
/// lambda, bbox x∈[0,5].
///
/// Composed source shares the Analytical grid-sampler path.
/// Max at x=5 (endpoint) → 2*5 = 10.0 EXACT.
///
/// **RED before step-4.**
#[test]
fn max_composed_field_bounded_shares_analytical_path() {
    let lambda = make_linear_lambda(2.0);
    let (field, field_type) = make_analytical_field(Type::dimensionless_scalar(), Type::dimensionless_scalar(), FieldSourceKind::Composed, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [5.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("max", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(10.0),
        "max(|x| 2x composed, bbox x∈[0,5]) = 10.0 at x=5 (endpoint node, exact)"
    );
}

// ── Negative pins: edge-case and source-kind guards ─────────────────────────
//
// These tests expect Value::Undef now AND after step-4.

/// `max(analytical, bbox)` where the lambda always returns NaN → Undef.
///
/// The grid-sampler skips non-finite values (`as_f64()` + `is_finite()`);
/// all 11 nodes skipped → Undef.
///
/// GREEN now (Analytical → Undef) and GREEN after step-4 (all-NaN → Undef).
#[test]
fn max_analytical_all_nan_lambda_returns_undef() {
    let lambda = make_nan_lambda();
    let (field, field_type) = make_analytical_field(Type::dimensionless_scalar(), Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("max", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "max(analytical, bbox) where lambda always returns NaN should be Undef"
    );
}

/// `max(analytical, bbox)` where the lambda returns a non-orderable Vector → Undef.
///
/// The grid-sampler calls `as_f64()` on each lambda result; Vector returns None → skipped.
/// All nodes skipped → Undef.
///
/// GREEN now (Analytical → Undef) and GREEN after step-4 (non-orderable → Undef).
#[test]
fn max_analytical_vector_codomain_returns_undef() {
    let lambda = make_vector_returning_lambda();
    let vec3_type = Type::vec3(Type::dimensionless_scalar());
    let (field, field_type) = make_analytical_field(Type::dimensionless_scalar(), vec3_type, FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("max", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "max(analytical, bbox) where lambda returns Vector should be Undef (non-orderable)"
    );
}

/// `max(analytical_4d, bbox_3d)` where the domain requires 4 coords but the
/// BoundingBox only provides 3 → Undef.
///
/// GREEN now (Analytical → Undef) and GREEN after step-4 (n > bbox_dim → Undef).
#[test]
fn max_analytical_domain_exceeds_bbox_dim_returns_undef() {
    let lambda = make_linear_lambda(1.0);
    // Point4<Real> domain — needs 4 bbox coords; our 3-coord bbox only has lo.len()==3.
    let domain = Type::Point { n: 4, quantity: Box::new(Type::dimensionless_scalar()) };
    let (field, field_type) = make_analytical_field(domain, Type::dimensionless_scalar(), FieldSourceKind::Analytical, lambda);
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 4.0, 4.0], DimensionVector::DIMENSIONLESS);
    let expr = make_bounded_call("max", field, field_type, bbox, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "max(Point4 domain, 3-coord bbox) should be Undef (domain dim > bbox coord count)"
    );
}

/// 2-arg `max|min|argmax|argmin(field, bbox)` over a VonMises-source field with a
/// **malformed lambda** (`Value::Undef`) returns `Value::Undef` defensively.
///
/// The VonMises bounded arm calls `project_von_mises_sampled(lambda)`.  When
/// `lambda` is `Value::Undef` (not a valid inner tensor `Value::Field { source:
/// Sampled, .. }`), `project_von_mises_sampled` returns `None` and the bounded
/// reduction returns `Undef` — the same defensive fallback as the 1-arg VonMises
/// path for a malformed field.  This test pins that behaviour so the defensive
/// `None => Undef` arm is not accidentally removed.
///
/// For the positive case (well-formed VonMises + real tensor data), see
/// `bounded_reductions_on_vonmises_field_clips_to_subregion`.
#[test]
fn bounded_reductions_on_vonmises_field_with_malformed_lambda_returns_undef() {
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        Type::dimensionless_scalar(),
        FieldSourceKind::VonMises,
        Value::Undef,
    );
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    for op in ["max", "min", "argmax", "argmin"] {
        let expr = make_bounded_call(op, field.clone(), field_type.clone(), bbox.clone(), Type::dimensionless_scalar());
        assert_eq!(
            eval_expr(&expr, &ctx),
            Value::Undef,
            "{op}(VonMises field with Undef lambda, bbox) should be Undef (malformed inner field)"
        );
    }
}

/// 2-arg `max|min|argmax|argmin(field, bbox)` over a **well-formed** VonMises-source
/// field clips the projected scalar sub-region correctly.
///
/// Inner tensor field: 1-D axis = [0.0, 1.0, 2.0, 3.0, 4.0], uniaxial windows:
/// σ_xx = {100e6, 250e6, 175e6, 200e6, 50e6} → vM = {100e6, 250e6, 175e6, 200e6, 50e6}.
///
/// Bounding box: x ∈ [1.0, 3.0] → in-bounds nodes at indices 1, 2, 3
/// → projected values {250e6, 175e6, 200e6}.
///
/// Expected:
/// - max = 250e6 Pa (at x = 1.0)
/// - min = 175e6 Pa (at x = 2.0)
/// - argmax = Real(1.0)
/// - argmin = Real(2.0)
#[test]
fn bounded_reductions_on_vonmises_field_clips_to_subregion() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };

    // Build the inner Sampled tensor field (stride-9 uniaxial windows).
    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(175e6),
            uniaxial_window(200e6),
            uniaxial_window(50e6),
        ],
    );
    // Domain of inner tensor field is Real (raw float coords).
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, Type::dimensionless_scalar());

    // Outer VonMises field: domain=Real, codomain=PRESSURE, lambda=inner tensor field.
    let (vonmises_field, vonmises_field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        pressure.clone(),
        FieldSourceKind::VonMises,
        inner_tensor_field,
    );

    // Bounding box: x ∈ [1.0, 3.0] (clips to indices 1, 2, 3).
    let bbox = make_bbox([1.0, 0.0, 0.0], [3.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // max(vonmises_field, bbox) — should return 250e6 Pa (projected max in sub-region)
    let max_expr = make_bounded_call(
        "max",
        vonmises_field.clone(),
        vonmises_field_type.clone(),
        bbox.clone(),
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&max_expr, &ctx),
        Value::Scalar { si_value: 250e6, dimension: DimensionVector::PRESSURE },
        "max(VonMises field, bbox=[1,3]) should return 250e6 Pa (vM max in sub-region)"
    );

    // min(vonmises_field, bbox) — should return 175e6 Pa
    let min_expr = make_bounded_call(
        "min",
        vonmises_field.clone(),
        vonmises_field_type.clone(),
        bbox.clone(),
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&min_expr, &ctx),
        Value::Scalar { si_value: 175e6, dimension: DimensionVector::PRESSURE },
        "min(VonMises field, bbox=[1,3]) should return 175e6 Pa (vM min in sub-region)"
    );

    // argmax(vonmises_field, bbox) — should return Real(1.0) (coord at index 1)
    let argmax_expr = make_bounded_call(
        "argmax",
        vonmises_field.clone(),
        vonmises_field_type.clone(),
        bbox.clone(),
        Type::dimensionless_scalar(),
    );
    assert_eq!(
        eval_expr(&argmax_expr, &ctx),
        Value::Real(1.0),
        "argmax(VonMises field, bbox=[1,3]) should return Real(1.0) (coord of vM max)"
    );

    // argmin(vonmises_field, bbox) — should return Real(2.0) (coord at index 2)
    let argmin_expr = make_bounded_call(
        "argmin",
        vonmises_field.clone(),
        vonmises_field_type.clone(),
        bbox.clone(),
        Type::dimensionless_scalar(),
    );
    assert_eq!(
        eval_expr(&argmin_expr, &ctx),
        Value::Real(2.0),
        "argmin(VonMises field, bbox=[1,3]) should return Real(2.0) (coord of vM min)"
    );
}

/// 2-arg `max|min|argmax|argmin(field, bbox)` over an Imported-source field → Undef.
#[test]
fn bounded_reductions_on_imported_field_return_undef() {
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        Type::dimensionless_scalar(),
        FieldSourceKind::Imported,
        Value::Undef,
    );
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    for op in ["max", "min", "argmax", "argmin"] {
        let expr = make_bounded_call(op, field.clone(), field_type.clone(), bbox.clone(), Type::dimensionless_scalar());
        assert_eq!(
            eval_expr(&expr, &ctx),
            Value::Undef,
            "{op}(Imported field, bbox) should be Undef (bounded form not supported for Imported)"
        );
    }
}

/// 2-arg `max|min|argmax|argmin(field, bbox)` over a MaxShear-source field → Undef.
#[test]
fn bounded_reductions_on_derived_maxshear_field_return_undef() {
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        Type::dimensionless_scalar(),
        FieldSourceKind::MaxShear,
        Value::Undef,
    );
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    for op in ["max", "min", "argmax", "argmin"] {
        let expr = make_bounded_call(op, field.clone(), field_type.clone(), bbox.clone(), Type::dimensionless_scalar());
        assert_eq!(
            eval_expr(&expr, &ctx),
            Value::Undef,
            "{op}(MaxShear field, bbox) should be Undef (bounded form not supported for derived)"
        );
    }
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
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, Type::dimensionless_scalar());

    // Directly construct the VonMises-source field (lambda = inner tensor field).
    let (vonmises_field, vonmises_field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
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
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, Type::dimensionless_scalar());

    let (vonmises_field, vonmises_field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
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
        Type::dimensionless_scalar(),
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
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(pressure.clone()),
        },
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(bad_sf)),
    };
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
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
    let inner_tensor_field = wrap_sampled_tensor_field(nan_sf, Type::dimensionless_scalar());
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
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

// ── Step S1 (MaxShear): max / min reduce a MaxShear-derived Sampled field ───
//
// These tests are RED before the MaxShear arm is implemented in
// `field_reductions.rs` (returns Value::Undef at the catch-all).
// After step-4 they become GREEN.
//
// Uniaxial window convention: window_i = [σ_i, 0, 0, 0, 0, 0, 0, 0, 0].
// For a uniaxial tensor the only non-zero principal stress is σ_xx,
// so max_shear = σ_xx / 2 exactly.

/// `max` / `min` over a MaxShear-source field whose lambda is a 1-D Sampled
/// tensor field (directly constructed, bypassing `compute_max_shear` wrapping).
///
/// Uniaxial windows: σ_xx = {100e6, 250e6, 175e6} → τ_max = σ/2 =
/// {50e6, 125e6, 87.5e6}. Expected: max = 125e6 Pa, min = 50e6 Pa.
///
/// **RED before step-4**: MaxShear arm returns `Value::Undef`.
#[test]
fn max_min_max_shear_derived_sampled_field_returns_correct_extremum() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
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
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, length.clone());

    // Directly construct the MaxShear-source field (lambda = inner tensor field).
    let (maxshear_field, maxshear_field_type) = make_field_with_source(
        length.clone(),
        pressure.clone(),
        FieldSourceKind::MaxShear,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // max(maxshear_field) should be 125e6 Pa (= 250e6 / 2)
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(maxshear_field.clone(), maxshear_field_type.clone())],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&max_expr, &ctx),
        Value::Scalar {
            si_value: 125e6,
            dimension: DimensionVector::PRESSURE,
        },
        "max(MaxShear field) should return 125e6 Pa (τ_max of σ=250e6 window)"
    );

    // min(maxshear_field) should be 50e6 Pa (= 100e6 / 2)
    let min_expr = make_function_call(
        "min",
        vec![CompiledExpr::literal(maxshear_field, maxshear_field_type)],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&min_expr, &ctx),
        Value::Scalar {
            si_value: 50e6,
            dimension: DimensionVector::PRESSURE,
        },
        "min(MaxShear field) should return 50e6 Pa (τ_max of σ=100e6 window)"
    );
}

/// `argmax` / `argmin` over a MaxShear-source 1-D field with LENGTH domain.
///
/// Same inner field as above: σ = {100e6, 250e6, 175e6} → τ = {50e6, 125e6, 87.5e6}.
/// argmax → index 1 → coord 1.0 m; argmin → index 0 → coord 0.0 m.
///
/// **RED before step-4**.
#[test]
fn argmax_argmin_max_shear_field_1d_returns_coord() {
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
    let (maxshear_field, maxshear_field_type) = make_field_with_source(
        length.clone(),
        pressure.clone(),
        FieldSourceKind::MaxShear,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // argmax → index 1 → coord 1.0 m
    let argmax_expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(maxshear_field.clone(), maxshear_field_type.clone())],
        length.clone(),
    );
    assert_eq!(
        eval_expr(&argmax_expr, &ctx),
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmax(MaxShear 1-D field) should return coord at projected max (index 1 → 1.0 m)"
    );

    // argmin → index 0 → coord 0.0 m
    let argmin_expr = make_function_call(
        "argmin",
        vec![CompiledExpr::literal(maxshear_field, maxshear_field_type)],
        length.clone(),
    );
    assert_eq!(
        eval_expr(&argmin_expr, &ctx),
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmin(MaxShear 1-D field) should return coord at projected min (index 0 → 0.0 m)"
    );
}

/// Partial-NaN skip: windows [NaN×9, uniaxial(250e6), NaN×9] →
/// projected τ = [NaN, 125e6, NaN] → max = 125e6 Pa, argmax → coord 1.0 m.
///
/// **RED before step-4**.
#[test]
fn reductions_on_max_shear_field_with_partial_nan_windows_skip_nan() {
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
            [f64::NAN; 9],           // out-of-solid sentinel
            uniaxial_window(250e6),  // finite: τ_max = 125e6 Pa
            [f64::NAN; 9],           // out-of-solid sentinel
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(sf, length.clone());
    let (field, field_type) = make_field_with_source(
        length.clone(),
        pressure.clone(),
        FieldSourceKind::MaxShear,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // max → the single finite projected window: 125e6 Pa
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field.clone(), field_type.clone())],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&max_expr, &ctx),
        Value::Scalar {
            si_value: 125e6,
            dimension: DimensionVector::PRESSURE,
        },
        "max(MaxShear field with partial NaN) should skip NaN and return 125e6 Pa"
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
        "argmax(MaxShear field with partial NaN) should skip NaN and return coord 1.0 m"
    );
}

/// Defensive: all four reductions return `Value::Undef` when the MaxShear
/// field's lambda is NOT a Sampled `Value::Field` (e.g. `Value::Undef`).
///
/// Pins the `project_max_shear_sampled` level-1 defensive arm (non-Sampled
/// lambda → None → Undef).
#[test]
fn all_reductions_on_max_shear_field_with_non_sampled_lambda_return_undef() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        pressure,
        FieldSourceKind::MaxShear,
        Value::Undef,
    );
    assert_all_reductions_undef(field, field_type, "MaxShear with non-Sampled lambda (Undef)");
}

/// All four reductions return `Value::Undef` when every MaxShear window is NaN
/// (all-out-of-solid FEA sentinel).
///
/// Pins the FEA out-of-solid path: `compute_max_shear_3x3([NaN; 9])` returns
/// `f64::NAN` (eigenvalues → None → NAN), the `is_finite()` gate in
/// `argmax_argmin_index` skips every window, and all reductions return Undef.
/// Also validates that the `debug_assert` NaN short-circuit added in step-2 does
/// NOT panic on all-NaN input (regression pin for the symmetry-assert fix).
#[test]
fn reductions_on_max_shear_field_all_nan_windows_return_undef() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let nan_sf = make_sampled_tensor_1d(
        "all_nan",
        vec![0.0, 1.0],
        vec![[f64::NAN; 9], [f64::NAN; 9]],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(nan_sf, Type::dimensionless_scalar());
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        pressure,
        FieldSourceKind::MaxShear,
        inner_tensor_field,
    );
    assert_all_reductions_undef(
        field,
        field_type,
        "MaxShear with all-NaN windows (all-out-of-solid → Undef)",
    );
}

// ── Step S1 (SafetyFactor): max / min reduce a SafetyFactor-derived field ───
//
// These tests are RED before the SafetyFactor arm is implemented in
// `field_reductions.rs` (returns Value::Undef at the catch-all).
// After step-6 they become GREEN.
//
// SafetyFactor lambda = Value::List[inner_tensor_field, yield_val].
// For uniaxial σ: vM = σ exactly → SF = yield / σ.

/// `max` / `min` over a SafetyFactor-source field.
///
/// Inner tensor field: uniaxial σ = {100e6, 250e6, 50e6} → vM = σ →
/// SF = 250e6 / {100e6, 250e6, 50e6} = {2.5, 1.0, 5.0} (dimensionless Real).
/// Expected: max = Real(5.0), min = Real(1.0).
///
/// **RED before step-6**.
#[test]
fn max_min_safety_factor_derived_sampled_field_returns_correct_extremum() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Build the inner Sampled tensor field (stride-9 uniaxial windows).
    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(50e6),
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, length.clone());

    // SafetyFactor lambda = Value::List[tensor_field, yield_val]
    let yield_val = Value::Scalar {
        si_value: 250e6,
        dimension: DimensionVector::PRESSURE,
    };
    let lambda = Value::List(vec![inner_tensor_field, yield_val]);

    // SafetyFactor field: domain=Length, codomain=Real (dimensionless)
    let (sf_field, sf_field_type) = make_field_with_source(
        length.clone(),
        Type::dimensionless_scalar(),
        FieldSourceKind::SafetyFactor,
        lambda,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // max → Real(5.0) (SF of σ=50e6 window: 250e6/50e6=5.0)
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(sf_field.clone(), sf_field_type.clone())],
        Type::dimensionless_scalar(),
    );
    assert_eq!(
        eval_expr(&max_expr, &ctx),
        Value::Real(5.0),
        "max(SafetyFactor field) should return Real(5.0) (SF of σ=50e6 window)"
    );

    // min → Real(1.0) (SF of σ=250e6 window: 250e6/250e6=1.0)
    let min_expr = make_function_call(
        "min",
        vec![CompiledExpr::literal(sf_field, sf_field_type)],
        Type::dimensionless_scalar(),
    );
    assert_eq!(
        eval_expr(&min_expr, &ctx),
        Value::Real(1.0),
        "min(SafetyFactor field) should return Real(1.0) (SF of σ=250e6 window)"
    );
}

/// `argmax` / `argmin` over a SafetyFactor-source 1-D field.
///
/// σ = {100e6, 250e6, 50e6} → SF = {2.5, 1.0, 5.0}.
/// argmax → index 2 → coord 2.0 m; argmin → index 1 → coord 1.0 m.
///
/// **RED before step-6**.
#[test]
fn argmax_argmin_safety_factor_field_1d_returns_coord() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![
            uniaxial_window(100e6),
            uniaxial_window(250e6),
            uniaxial_window(50e6),
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, length.clone());
    let yield_val = Value::Scalar {
        si_value: 250e6,
        dimension: DimensionVector::PRESSURE,
    };
    let lambda = Value::List(vec![inner_tensor_field, yield_val]);
    let (sf_field, sf_field_type) = make_field_with_source(
        length.clone(),
        Type::dimensionless_scalar(),
        FieldSourceKind::SafetyFactor,
        lambda,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // argmax → index 2 → coord 2.0 m
    let argmax_expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(sf_field.clone(), sf_field_type.clone())],
        length.clone(),
    );
    assert_eq!(
        eval_expr(&argmax_expr, &ctx),
        Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmax(SafetyFactor 1-D field) should return coord at SF max (index 2 → 2.0 m)"
    );

    // argmin → index 1 → coord 1.0 m
    let argmin_expr = make_function_call(
        "argmin",
        vec![CompiledExpr::literal(sf_field, sf_field_type)],
        length.clone(),
    );
    assert_eq!(
        eval_expr(&argmin_expr, &ctx),
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmin(SafetyFactor 1-D field) should return coord at SF min (index 1 → 1.0 m)"
    );
}

/// All-hydrostatic windows: vM = 0, SF = yield/0 = +∞.
/// The is_finite() reduction gate skips +∞, so all reductions return Undef.
///
/// Pins the hydrostatic-poison convention (matches the `safety_factor` builtin:
/// yield/0 → +∞ → `sanitize_value` → Undef; the is_finite() gate in
/// `argmax_argmin_index` enforces the same outcome for the field reduction).
#[test]
fn reductions_on_safety_factor_field_with_hydrostatic_windows_return_undef() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Hydrostatic window: [p, 0, 0, 0, p, 0, 0, 0, p] — vM = 0.
    let p = 100e6_f64;
    let hydrostatic_window: [f64; 9] = [p, 0.0, 0.0, 0.0, p, 0.0, 0.0, 0.0, p];

    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0],
        vec![hydrostatic_window, hydrostatic_window],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, length.clone());
    let yield_val = Value::Scalar {
        si_value: 250e6,
        dimension: DimensionVector::PRESSURE,
    };
    let lambda = Value::List(vec![inner_tensor_field, yield_val]);
    let (field, field_type) = make_field_with_source(
        length.clone(),
        Type::dimensionless_scalar(),
        FieldSourceKind::SafetyFactor,
        lambda,
    );

    assert_all_reductions_undef(
        field,
        field_type,
        "SafetyFactor with hydrostatic windows (vM=0 → SF=+∞ → skipped → Undef)",
    );
}

/// All four reductions return `Value::Undef` when every SafetyFactor window is
/// NaN (all-out-of-solid FEA sentinel).
///
/// Pins the out-of-solid path: `yield / compute_von_mises_3x3([NaN; 9])` =
/// `yield / NaN` = NaN, which is non-finite and skipped by `is_finite()`,
/// so all reductions return Undef.
#[test]
fn reductions_on_safety_factor_field_all_nan_windows_return_undef() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let nan_sf = make_sampled_tensor_1d(
        "all_nan",
        vec![0.0, 1.0],
        vec![[f64::NAN; 9], [f64::NAN; 9]],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(nan_sf, length.clone());
    let yield_val = Value::Scalar {
        si_value: 250e6,
        dimension: DimensionVector::PRESSURE,
    };
    let lambda = Value::List(vec![inner_tensor_field, yield_val]);
    let (field, field_type) = make_field_with_source(
        length.clone(),
        Type::dimensionless_scalar(),
        FieldSourceKind::SafetyFactor,
        lambda,
    );
    assert_all_reductions_undef(
        field,
        field_type,
        "SafetyFactor with all-NaN windows (all-out-of-solid → Undef)",
    );
}

/// Defensive: all four reductions return `Value::Undef` for malformed
/// SafetyFactor lambda — pins the `project_safety_factor_sampled` defensive arms.
///
/// Tests three malformed cases:
/// (1) non-List lambda (Value::Undef) — level-1 guard rejects non-List.
/// (2) wrong-arity List (3 elements) — level-1 guard rejects `len != 2`.
/// (3) non-numeric yield (Value::Undef in list position 1) — `yield_val.as_f64()` returns None.
#[test]
fn all_reductions_on_safety_factor_field_with_malformed_lambda_return_undef() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };

    // Case 1: non-List lambda
    let (field1, field_type1) = make_field_with_source(
        Type::dimensionless_scalar(),
        Type::dimensionless_scalar(),
        FieldSourceKind::SafetyFactor,
        Value::Undef,
    );
    assert_all_reductions_undef(
        field1,
        field_type1,
        "SafetyFactor with non-List lambda (Undef)",
    );

    // Build a valid inner tensor field for cases 2 and 3.
    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0],
        vec![uniaxial_window(100e6), uniaxial_window(200e6)],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, Type::dimensionless_scalar());

    // Case 2: wrong-arity List (3 elements instead of 2)
    let yield_val = Value::Scalar {
        si_value: 250e6,
        dimension: DimensionVector::PRESSURE,
    };
    let (field2, field_type2) = make_field_with_source(
        Type::dimensionless_scalar(),
        Type::dimensionless_scalar(),
        FieldSourceKind::SafetyFactor,
        Value::List(vec![
            inner_tensor_field.clone(),
            yield_val.clone(),
            Value::Real(0.0),
        ]),
    );
    assert_all_reductions_undef(
        field2,
        field_type2,
        "SafetyFactor with wrong-arity List (3 elements)",
    );

    // Case 3: non-numeric yield (Value::Undef in position 1)
    let (field3, field_type3) = make_field_with_source(
        Type::dimensionless_scalar(),
        pressure,
        FieldSourceKind::SafetyFactor,
        Value::List(vec![inner_tensor_field, Value::Undef]),
    );
    assert_all_reductions_undef(
        field3,
        field_type3,
        "SafetyFactor with non-numeric yield (Undef in position 1)",
    );
}

/// 2-arg `max|min|argmax|argmin(field, bbox)` over a SafetyFactor-source field → Undef.
///
/// Mirrors `bounded_reductions_on_derived_maxshear_field_return_undef`:
/// the bounded form is not supported for derived SafetyFactor fields.
#[test]
fn bounded_reductions_on_derived_safetyfactor_field_return_undef() {
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        Type::dimensionless_scalar(),
        FieldSourceKind::SafetyFactor,
        Value::Undef,
    );
    let bbox = make_bbox([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    for op in ["max", "min", "argmax", "argmin"] {
        let expr = make_bounded_call(op, field.clone(), field_type.clone(), bbox.clone(), Type::dimensionless_scalar());
        assert_eq!(
            eval_expr(&expr, &ctx),
            Value::Undef,
            "{op}(SafetyFactor field, bbox) should be Undef (bounded form not supported for derived)"
        );
    }
}

// ── Robustness: non-positive yield strength ──────────────────────────────────

/// All four reductions return `Value::Undef` when the SafetyFactor yield
/// strength is zero or negative (physically meaningless values).
///
/// Pins the `project_safety_factor_sampled` non-positive-yield guard:
/// `yield_f64 <= 0.0 → None → Undef`.  Without this guard, `0.0 / vM = 0.0`
/// and `negative / vM < 0` would silently produce finite extrema that are
/// nonsensical as safety factors.
///
/// Uses a valid inner tensor field (uniaxial windows) so the projection
/// path would yield finite results if the yield guard were absent.
#[test]
fn all_reductions_on_safety_factor_field_with_non_positive_yield_return_undef() {
    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0],
        vec![uniaxial_window(100e6), uniaxial_window(200e6)],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, Type::dimensionless_scalar());

    // Case 1: zero yield — 0.0 / vM = 0.0 (finite but meaningless without guard).
    let (field1, field_type1) = make_field_with_source(
        Type::dimensionless_scalar(),
        Type::dimensionless_scalar(),
        FieldSourceKind::SafetyFactor,
        Value::List(vec![inner_tensor_field.clone(), Value::Real(0.0)]),
    );
    assert_all_reductions_undef(
        field1,
        field_type1,
        "SafetyFactor with zero yield (0.0 → non-positive guard → Undef)",
    );

    // Case 2: negative yield — physically impossible yield strength.
    let (field2, field_type2) = make_field_with_source(
        Type::dimensionless_scalar(),
        Type::dimensionless_scalar(),
        FieldSourceKind::SafetyFactor,
        Value::List(vec![inner_tensor_field, Value::Real(-250e6)]),
    );
    assert_all_reductions_undef(
        field2,
        field_type2,
        "SafetyFactor with negative yield (-250e6 → non-positive guard → Undef)",
    );
}

// ── Task 4562: PrincipalStresses reductions ──────────────────────────────────
//
// PrincipalStresses is a pointwise tensor→LIST projection: each stride-9
// stress-tensor window eigen-decomposes to a List of 3 principal stresses
// (ascending: eigs[0]=σ₃ min, eigs[2]=σ₁ max).
//
// Diagonal windows hit the `p1 ≤ 1e-30` short-circuit in
// `compute_eigenvalues_3x3` (off-diagonal entries are 0), so eigenvalues
// equal the sorted diagonal entries EXACTLY — no floating-point error,
// so `assert_eq!` on exact `Value::Scalar` values is sound.
//
// Tests are RED before step-2 / step-4 implement the PrincipalStresses arm.

/// Diagonal 3×3 symmetric window with principal stresses (a, b, c):
/// [a,0,0, 0,b,0, 0,0,c] (row-major, off-diagonal = 0).
///
/// `compute_eigenvalues_3x3` hits the `p1 ≤ 1e-30` diagonal short-circuit
/// and returns `sort([a, b, c])` with NO floating arithmetic — eigenvalues
/// equal the diagonal entries exactly, so `assert_eq!` on `Value::Scalar`
/// values is safe.
fn diagonal_window(a: f64, b: f64, c: f64) -> [f64; 9] {
    [a, 0.0, 0.0, 0.0, b, 0.0, 0.0, 0.0, c]
}

/// `max` / `min` over a PrincipalStresses-source field whose lambda is a 1-D
/// Sampled tensor field with diagonal windows.
///
/// Windows: diag(100e6, 20e6, 30e6), diag(50e6, 40e6, 60e6), diag(10e6, -70e6, 5e6).
/// Eigenvalues (ascending) per window:
///   - w0: [20e6, 30e6, 100e6]  → min_entry=20e6,  max_entry=100e6
///   - w1: [40e6, 50e6,  60e6]  → min_entry=40e6,  max_entry=60e6
///   - w2: [-70e6, 5e6,  10e6]  → min_entry=-70e6, max_entry=10e6
///
/// Global max = max(100e6, 60e6, 10e6) = 100e6 Pa.
/// Global min = min(20e6, 40e6, -70e6) = -70e6 Pa.
///
/// The PrincipalStresses field has `codomain = List(Scalar<PRESSURE>)`;
/// the reduction unwraps to the element type and returns `Value::Scalar<PRESSURE>`.
///
/// **RED before step-2**: PrincipalStresses arm returns `Value::Undef`.
#[test]
fn max_min_principal_stresses_derived_sampled_field_returns_correct_extremum() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let list_pressure = Type::List(Box::new(pressure.clone()));

    // Build the inner Sampled tensor field (stride-9 diagonal windows).
    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![
            diagonal_window(100e6, 20e6, 30e6),
            diagonal_window(50e6, 40e6, 60e6),
            diagonal_window(10e6, -70e6, 5e6),
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, length.clone());

    // Construct the PrincipalStresses-source field (lambda = inner tensor field,
    // codomain = List(Scalar<PRESSURE>)).
    let (ps_field, ps_field_type) = make_field_with_source(
        length.clone(),
        list_pressure,
        FieldSourceKind::PrincipalStresses,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // max(ps_field) should be 100e6 Pa (global max principal stress)
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(ps_field.clone(), ps_field_type.clone())],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&max_expr, &ctx),
        Value::Scalar {
            si_value: 100e6,
            dimension: DimensionVector::PRESSURE,
        },
        "max(PrincipalStresses field) should return 100e6 Pa (global max eigenvalue)"
    );

    // min(ps_field) should be -70e6 Pa (global min principal stress)
    let min_expr = make_function_call(
        "min",
        vec![CompiledExpr::literal(ps_field, ps_field_type)],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&min_expr, &ctx),
        Value::Scalar {
            si_value: -70e6,
            dimension: DimensionVector::PRESSURE,
        },
        "min(PrincipalStresses field) should return -70e6 Pa (global min eigenvalue)"
    );
}

/// Partial-NaN skip: windows [NaN×9, diag(15e6, 25e6, 8e6), NaN×9].
/// Eigenvalues for the finite window (diagonal short-circuit):
///   sort([15e6, 25e6, 8e6]) = [8e6, 15e6, 25e6] → max_entry = 25e6.
/// NaN windows eigen-decompose to NaN and are skipped by the `is_finite()` gate.
/// Expected: max = 25e6 Pa.
///
/// **RED before step-2**.
#[test]
fn max_principal_stresses_field_with_partial_nan_windows_skips_nan() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let list_pressure = Type::List(Box::new(pressure.clone()));

    let sf = make_sampled_tensor_1d(
        "partial_nan",
        vec![0.0, 1.0, 2.0],
        vec![
            [f64::NAN; 9],                     // out-of-solid sentinel
            diagonal_window(15e6, 25e6, 8e6),  // finite: max_entry = 25e6 Pa
            [f64::NAN; 9],                     // out-of-solid sentinel
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(sf, length.clone());
    let (field, field_type) = make_field_with_source(
        length.clone(),
        list_pressure,
        FieldSourceKind::PrincipalStresses,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // max → the single finite projected window: eigs[2] = 25e6 Pa
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field, field_type)],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&max_expr, &ctx),
        Value::Scalar {
            si_value: 25e6,
            dimension: DimensionVector::PRESSURE,
        },
        "max(PrincipalStresses field with partial NaN) should skip NaN and return 25e6 Pa"
    );
}

/// `argmax` / `argmin` over a PrincipalStresses-source 1-D field with LENGTH domain.
///
/// Same three diagonal windows as the value-extremum test:
///   w0 at coord 0.0 m: diag(100e6, 20e6, 30e6) → max_entry=100e6, min_entry=20e6
///   w1 at coord 1.0 m: diag(50e6,  40e6, 60e6) → max_entry=60e6,  min_entry=40e6
///   w2 at coord 2.0 m: diag(10e6, -70e6,  5e6) → max_entry=10e6,  min_entry=-70e6
///
/// argmax → projected max entries = {100e6, 60e6, 10e6} → index 0 → coord 0.0 m.
/// argmin → projected min entries = {20e6, 40e6, -70e6} → index 2 → coord 2.0 m.
///
/// Only the domain coord is surfaced (not the winning entry index) — mirrors
/// the scalar argextremum path and all sibling derived kinds.
///
/// **RED before step-4**: PrincipalStresses arm in compute_argextremum returns Undef.
#[test]
fn argmax_argmin_principal_stresses_field_1d_returns_coord() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let list_pressure = Type::List(Box::new(pressure.clone()));

    let inner_sf = make_sampled_tensor_1d(
        "stress",
        vec![0.0, 1.0, 2.0],
        vec![
            diagonal_window(100e6, 20e6, 30e6),
            diagonal_window(50e6, 40e6, 60e6),
            diagonal_window(10e6, -70e6, 5e6),
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(inner_sf, length.clone());
    let (ps_field, ps_field_type) = make_field_with_source(
        length.clone(),
        list_pressure,
        FieldSourceKind::PrincipalStresses,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // argmax → projected max-entries = {100e6, 60e6, 10e6} → index 0 → coord 0.0 m
    let argmax_expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(ps_field.clone(), ps_field_type.clone())],
        length.clone(),
    );
    assert_eq!(
        eval_expr(&argmax_expr, &ctx),
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmax(PrincipalStresses 1-D field) should return coord 0.0 m (index 0, max entry=100e6)"
    );

    // argmin → projected min-entries = {20e6, 40e6, -70e6} → index 2 → coord 2.0 m
    let argmin_expr = make_function_call(
        "argmin",
        vec![CompiledExpr::literal(ps_field, ps_field_type)],
        length.clone(),
    );
    assert_eq!(
        eval_expr(&argmin_expr, &ctx),
        Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        },
        "argmin(PrincipalStresses 1-D field) should return coord 2.0 m (index 2, min entry=-70e6)"
    );
}

/// Partial-NaN skip for argmax: windows [NaN×9, diag(15e6, 25e6, 8e6), NaN×9]
/// over axis [0, 1, 2].  Only window at index 1 is finite; max_entry = eigs[2]
/// = 25e6 → the only candidate → argmax coord = 1.0 m.
///
/// **RED before step-4**.
#[test]
fn argmax_principal_stresses_field_with_partial_nan_skips_nan() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let list_pressure = Type::List(Box::new(pressure.clone()));

    let sf = make_sampled_tensor_1d(
        "partial_nan",
        vec![0.0, 1.0, 2.0],
        vec![
            [f64::NAN; 9],                     // out-of-solid sentinel
            diagonal_window(15e6, 25e6, 8e6),  // finite: max_entry=25e6, min_entry=8e6
            [f64::NAN; 9],                     // out-of-solid sentinel
        ],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(sf, length.clone());
    let (field, field_type) = make_field_with_source(
        length.clone(),
        list_pressure,
        FieldSourceKind::PrincipalStresses,
        inner_tensor_field,
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // argmax → only finite window at index 1 → coord 1.0 m
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
        "argmax(PrincipalStresses field with partial NaN) should skip NaN and return coord 1.0 m"
    );
}

/// All four reductions return `Value::Undef` when every PrincipalStresses
/// window is all-NaN (all-out-of-solid FEA sentinel).
///
/// Pins the NaN-skip + all-finite-absent → Undef chain:
/// `compute_eigenvalues_3x3([NaN; 9])` returns `Some([NaN, NaN, NaN])`,
/// the selected entry is NaN, the `is_finite()` gate in
/// `argmax_argmin_index` / `reduce_sampled_extremum` skips every window,
/// and all four reductions return `Value::Undef`.
///
/// Mirrors `reductions_on_max_shear_field_all_nan_windows_return_undef`.
#[test]
fn reductions_on_principal_stresses_field_all_nan_windows_return_undef() {
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let list_pressure = Type::List(Box::new(pressure.clone()));
    let nan_sf = make_sampled_tensor_1d(
        "all_nan",
        vec![0.0, 1.0],
        vec![[f64::NAN; 9], [f64::NAN; 9]],
    );
    let inner_tensor_field = wrap_sampled_tensor_field(nan_sf, Type::dimensionless_scalar());
    let (field, field_type) = make_field_with_source(
        Type::dimensionless_scalar(),
        list_pressure,
        FieldSourceKind::PrincipalStresses,
        inner_tensor_field,
    );
    assert_all_reductions_undef(
        field,
        field_type,
        "PrincipalStresses with all-NaN windows (all-out-of-solid → Undef)",
    );
}

// ── Task 4566 γ — vector/tensor-magnitude Sampled reduction ──────────────────
//
// Steps 1–6: add `max/min/argmax/argmin` over vector/tensor-codomain Sampled
// fields by pointwise Euclidean (Frobenius for tensors) magnitude.

// ── γ step 1: vector-codomain magnitude max/min ─────────────────────────────

/// `max` / `min` over a 1-D `Length`-domain, `Vector3<Real>`-codomain Sampled
/// field reduce by pointwise Euclidean magnitude.
///
/// Flat stride-3 buffer: node 0 = (3,4,0), node 1 = (0,0,0), node 2 = (6,8,0).
/// Magnitudes: [5, 0, 10] → max Value::Real(10.0), min Value::Real(0.0).
///
/// **RED before step-2**: current Sampled arm flat-reduces the buffer,
/// returning the max *component* (8.0) instead of max magnitude (10.0).
#[test]
fn max_min_vector3_sampled_field_reduces_by_magnitude() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain = Type::vec3(Type::dimensionless_scalar());

    // Flat stride-3 buffer: nodes 0/1/2 = (3,4,0)|(0,0,0)|(6,8,0).
    let sf = make_sampled_1d(
        "vec3",
        vec![0.0, 1.0, 2.0],
        vec![3.0, 4.0, 0.0, 0.0, 0.0, 0.0, 6.0, 8.0, 0.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, length, codomain);

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field.clone(), field_type.clone())],
        Type::dimensionless_scalar(),
    );
    assert_eq!(
        eval_expr(&max_expr, &ctx),
        Value::Real(10.0),
        "max(Vector3 Sampled) should return max Euclidean magnitude 10.0"
    );

    let min_expr = make_function_call(
        "min",
        vec![CompiledExpr::literal(field, field_type)],
        Type::dimensionless_scalar(),
    );
    assert_eq!(
        eval_expr(&min_expr, &ctx),
        Value::Real(0.0),
        "min(Vector3 Sampled) should return min Euclidean magnitude 0.0"
    );
}

/// Dimensioned `Vector3<Pressure>` codomain: max magnitude preserves the
/// element dimension.
///
/// Windows: (3e6,4e6,0)|(0,0,0)|(6e6,8e6,0) → magnitudes [5e6, 0, 10e6] Pa.
/// max → Value::Scalar{si_value:10e6, dimension:PRESSURE}.
///
/// **RED before step-2**.
#[test]
fn max_vector3_dimensioned_sampled_field_preserves_dimension() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let codomain = Type::vec3(pressure.clone());

    let sf = make_sampled_1d(
        "vec3_pa",
        vec![0.0, 1.0, 2.0],
        vec![3e6, 4e6, 0.0, 0.0, 0.0, 0.0, 6e6, 8e6, 0.0],
    );
    let (field, field_type) = wrap_sampled_field(sf, length, codomain);

    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field, field_type)],
        pressure.clone(),
    );
    assert_eq!(
        eval_expr(&max_expr, &EvalContext::simple(&ValueMap::new())),
        Value::Scalar {
            si_value: 10e6,
            dimension: DimensionVector::PRESSURE,
        },
        "max(Vector3<Pressure> Sampled) should preserve PRESSURE dimension on magnitude result"
    );
}

/// NaN window in a `Vector3<Real>`-codomain field is skipped by the existing
/// `is_finite()` gate; an all-NaN buffer returns `Value::Undef`.
///
/// Partial-NaN: (3,4,0)|(NaN,NaN,NaN)|(6,8,0) → max magnitude = 10.0.
/// All-NaN: NaN×9 per node → Value::Undef.
///
/// **RED before step-2**.
#[test]
fn max_vector3_sampled_field_skips_nan_window() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain = Type::vec3(Type::dimensionless_scalar());

    // Partial NaN: middle window is all-NaN.
    let sf_partial = make_sampled_1d(
        "vec3_nan",
        vec![0.0, 1.0, 2.0],
        vec![
            3.0,      4.0,      0.0,
            f64::NAN, f64::NAN, f64::NAN,
            6.0,      8.0,      0.0,
        ],
    );
    let (field, field_type) =
        wrap_sampled_field(sf_partial, length.clone(), codomain.clone());
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field, field_type)],
        Type::dimensionless_scalar(),
    );
    assert_eq!(
        eval_expr(&max_expr, &EvalContext::simple(&ValueMap::new())),
        Value::Real(10.0),
        "max(partial-NaN Vector3 Sampled) should skip NaN window and return 10.0"
    );

    // All-NaN: every element of the flat buffer is NaN.
    let sf_all_nan = make_sampled_1d(
        "vec3_all_nan",
        vec![0.0, 1.0, 2.0],
        vec![
            f64::NAN, f64::NAN, f64::NAN,
            f64::NAN, f64::NAN, f64::NAN,
            f64::NAN, f64::NAN, f64::NAN,
        ],
    );
    let (field2, field_type2) =
        wrap_sampled_field(sf_all_nan, length, codomain);
    let max_expr2 = make_function_call(
        "max",
        vec![CompiledExpr::literal(field2, field_type2)],
        Type::dimensionless_scalar(),
    );
    assert_eq!(
        eval_expr(&max_expr2, &EvalContext::simple(&ValueMap::new())),
        Value::Undef,
        "max(all-NaN Vector3 Sampled) should return Value::Undef"
    );
}

/// REGRESSION: `Real`-codomain (stride-1) Sampled field stays sign-preserving
/// after the magnitude-path change. data = [-5, -1, -3] → max = -1.0, NOT 5.0.
///
/// Confirms the scalar/divergence path bypasses the magnitude branch.
/// This test MUST be GREEN even before step-2 (it pins the existing behavior).
#[test]
fn max_scalar_sampled_field_stays_sign_preserving_regression() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let sf = make_sampled_1d(
        "scalar_neg",
        vec![0.0, 1.0, 2.0],
        vec![-5.0, -1.0, -3.0],
    );
    let (field, field_type) =
        wrap_sampled_field(sf, length, Type::dimensionless_scalar());

    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(field, field_type)],
        Type::dimensionless_scalar(),
    );
    assert_eq!(
        eval_expr(&max_expr, &EvalContext::simple(&ValueMap::new())),
        Value::Real(-1.0),
        "max(negative scalar Sampled) should return -1.0 (sign-preserving, NOT abs)"
    );
}
