//! Runtime sample-dispatch for `Value::SampledField`.
//!
//! v0.2 sampled-field semantics (task 2341):
//! * Out-of-bounds queries return `Value::Undef` and emit
//!   `W_FIELD_OUT_OF_BOUNDS` once per field per session, suppressed by an
//!   `AtomicBool` on the `SampledField` itself.
//! * Linear / NearestNeighbor / Cubic methods dispatch to
//!   [`crate::interp::interpolate_1d`]/`_2d`/`_3d`. RBF / Kriging fall back
//!   to Linear and emit `W_INTERPOLATION_DEFERRED` (delegated to interp's
//!   own resolve-method path).
//!
//! `EvalContext` carries an optional `RefCell<Vec<Diagnostic>>` sink. When
//! present, OOB and interpolation-deferred warnings are pushed into it for
//! the surrounding `Engine::eval` to drain. When absent, warnings are
//! silently dropped — preserving the old `EvalContext::simple` semantics
//! used by ad-hoc tests.
//!
//! End-to-end behaviour of these contracts is pinned by the integration
//! tests in `crates/reify-eval/tests/field_eval_tests.rs`:
//! * `sample_sampled_field_out_of_bounds_returns_undef_and_emits_warning_once`
//!   — OOB → `Value::Undef` and exactly one `W_FIELD_OUT_OF_BOUNDS` across
//!   N OOB calls on the same field.
//! * `sample_sampled_field_with_rbf_emits_interpolation_deferred_warning_and_falls_back_to_linear`
//!   — RBF / Kriging emit `W_INTERPOLATION_DEFERRED` and the value matches
//!   the Linear-fallback baseline.
//! * 1D / 2D / 3D positive-path tests confirm the fully-supported methods
//!   (Linear / NearestNeighbor / Cubic) leave `result.diagnostics` empty.

use std::sync::atomic::Ordering;

use reify_types::{
    Diagnostic, DiagnosticCode, InterpolationKind, SampledField, SampledGridKind, Type, Value,
};

use crate::EvalContext;
use crate::interp::{InterpolationMethod, InterpolationResult, interpolate_1d, interpolate_2d, interpolate_3d};

/// Map a language-level [`InterpolationKind`] to the algorithmic-core
/// [`InterpolationMethod`]. RBF and Kriging map directly so `interp::resolve_method`
/// triggers the deferred-method fallback and emits `W_INTERPOLATION_DEFERRED`.
impl From<InterpolationKind> for InterpolationMethod {
    fn from(kind: InterpolationKind) -> Self {
        match kind {
            InterpolationKind::Linear => InterpolationMethod::Linear,
            InterpolationKind::NearestNeighbor => InterpolationMethod::NearestNeighbor,
            InterpolationKind::Cubic => InterpolationMethod::Cubic,
            InterpolationKind::Rbf => InterpolationMethod::Rbf,
            InterpolationKind::Kriging => InterpolationMethod::Kriging,
        }
    }
}

/// Sample a `SampledField` at the given query point.
///
/// `point` is the user-facing sample arg (whatever the user passed as the
/// second arg to `sample(field, point)`). For `Regular1D`, accepts
/// `Value::Real`, `Value::Int`, or `Value::Scalar` (any dimension; we
/// extract `si_value`). For `Regular2D`/`3D`, accepts `Value::Point`
/// or `Value::Vector` of matching arity, with each component a Real / Int
/// / Scalar.
///
/// `codomain_type` is the field's declared codomain type (from
/// `Value::Field.codomain_type`). Dimensionless codomain → return
/// `Value::Real`; dimensioned codomain → return `Value::Scalar`.
///
/// Behaviour:
/// 1. Coord-extraction failure → `Value::Undef` (no diagnostic; matches
///    the existing analytical-field arm's silent-Undef on shape mismatch).
/// 2. OOB on any axis → atomically swap `oob_emitted` from `false` →
///    `true`; on the winning swap (we set it from false to true), push a
///    `W_FIELD_OUT_OF_BOUNDS` warning to `ctx.diagnostics` if present.
///    Return `Value::Undef`.
/// 3. Else dispatch to `interpolate_1d`/`2d`/`3d` based on `field.kind`,
///    forward the call's `result.diagnostics` to `ctx.diagnostics`, and
///    wrap the f64 in `Value::Real` (dimensionless codomain) or
///    `Value::Scalar` (dimensioned codomain).
pub fn sample_at_point(
    field: &SampledField,
    point: &Value,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    let coords = match extract_coords(point, field.kind) {
        Some(c) => c,
        None => return Value::Undef,
    };

    if is_out_of_bounds(&coords, &field.bounds_min, &field.bounds_max) {
        if field
            .oob_emitted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            && let Some(sink) = ctx.diagnostics
        {
            let diag = Diagnostic::warning(format!(
                "sampled field '{}' query is out of bounds; returning Undef",
                field.name
            ))
            .with_code(DiagnosticCode::FieldOutOfBounds);
            sink.borrow_mut().push(diag);
        }
        return Value::Undef;
    }

    // 1D / 2D / 3D dispatch: the per-axis flat-data layout follows
    // `interp.rs`'s row-major convention (axis-0 outermost). The elaborator
    // (`engine_eval::build_sampled_field`) enforces three runtime invariants
    // before constructing the `SampledField` reached by this dispatch:
    //
    // 1. each axis spacing is strictly positive and finite,
    // 2. each axis grid has at least 2 nodes (i.e.
    //    `axis_grids[i].len() >= 2`),
    // 3. `data.len() == product(axis_grids[i].len())` — row-major flatten,
    //    axis-0 outermost.
    //
    // Any violation poisons the field to `Value::Undef` at elaboration time
    // and emits a `DiagnosticCode::FieldSampledInvalidConfig` warning, so by
    // the time control reaches this dispatch the `interp::interpolate_Nd`
    // primitives' internal `assert!`s on grid length, axis-grid length, and
    // data length cannot fire from sampled-field input.
    let method: InterpolationMethod = field.interpolation.into();
    let result: InterpolationResult = match field.kind {
        SampledGridKind::Regular1D => {
            interpolate_1d(method, &field.axis_grids[0], &field.data, coords[0])
        }
        SampledGridKind::Regular2D => interpolate_2d(
            method,
            &field.axis_grids[0],
            &field.axis_grids[1],
            &field.data,
            (coords[0], coords[1]),
        ),
        SampledGridKind::Regular3D => interpolate_3d(
            method,
            &field.axis_grids[0],
            &field.axis_grids[1],
            &field.axis_grids[2],
            &field.data,
            (coords[0], coords[1], coords[2]),
        ),
    };

    // Forward any interpolation diagnostics (e.g. RBF/Kriging deferral) to
    // the runtime sink. Like OOB above, silent-drop when no sink is wired.
    if !result.diagnostics.is_empty()
        && let Some(sink) = ctx.diagnostics
    {
        let mut borrow = sink.borrow_mut();
        for d in result.diagnostics {
            borrow.push(d);
        }
    }

    wrap_result(result.value, codomain_type)
}

/// Extract per-axis SI scalar coordinates from a sample-point `Value`,
/// projecting to the arity required by `kind`.
fn extract_coords(point: &Value, kind: SampledGridKind) -> Option<Vec<f64>> {
    match kind {
        SampledGridKind::Regular1D => {
            // 1D: accept a single scalar (Real / Int / Scalar). Reject
            // Point/Vector — those are ambiguous for a 1D field.
            scalar_si(point).map(|v| vec![v])
        }
        SampledGridKind::Regular2D => extract_arity_n(point, 2),
        SampledGridKind::Regular3D => extract_arity_n(point, 3),
    }
}

/// Extract `n` per-axis SI scalars from a `Value::Point` or `Value::Vector`
/// whose components are each Real / Int / Scalar.
fn extract_arity_n(point: &Value, n: usize) -> Option<Vec<f64>> {
    let items = match point {
        Value::Point(items) | Value::Vector(items) => items,
        _ => return None,
    };
    if items.len() != n {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for item in items {
        out.push(scalar_si(item)?);
    }
    Some(out)
}

/// Extract the SI scalar value from a `Value::Real`, `Value::Int`, or
/// `Value::Scalar`. Any other shape returns `None`.
fn scalar_si(v: &Value) -> Option<f64> {
    match v {
        Value::Real(r) => Some(*r),
        Value::Int(n) => Some(*n as f64),
        Value::Scalar { si_value, .. } => Some(*si_value),
        _ => None,
    }
}

/// True if any axis coord is strictly outside the field's `[min, max]`
/// bounds. Inclusive on both endpoints (matches `interp::locate_cell`'s
/// right-edge-inclusive contract).
fn is_out_of_bounds(coords: &[f64], min: &[f64], max: &[f64]) -> bool {
    debug_assert_eq!(coords.len(), min.len());
    debug_assert_eq!(coords.len(), max.len());
    coords
        .iter()
        .zip(min.iter())
        .zip(max.iter())
        .any(|((q, lo), hi)| q.is_nan() || q < lo || q > hi)
}

/// Wrap an interpolated f64 in the field's codomain shape. Dimensionless
/// codomain (`Type::Real` or `Type::Int`) → `Value::Real`; otherwise
/// `Value::Scalar { si_value, dimension: codomain.dim }`.
fn wrap_result(v: f64, codomain_type: &Type) -> Value {
    match codomain_type {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Value::Scalar {
            si_value: v,
            dimension: *dimension,
        },
        _ => Value::Real(v),
    }
}
