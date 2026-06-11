//! Eager Field reductions: `max`, `min`, `argmax`, `argmin`.
//!
//! Architecturally distinct from `analysis.rs` (which produces lazy
//! field-wrapper Values via `FieldSourceKind::VonMises`/etc.):
//! these reductions **collapse** a field to a single scalar (or a
//! single point) immediately. The dispatch arms in `lib.rs` invoke
//! these helpers on a `Value::Field` argument and return the resulting
//! `Value` directly to the caller.
//!
//! # Source-kind support (staged per task description)
//!
//! `FieldSourceKind::Sampled` and `FieldSourceKind::VonMises` are fully
//! implemented:
//!
//! - **Sampled** — data buffer reduced directly (stride-1 scalar data).
//! - **VonMises** — the backing Sampled tensor field is unwrapped from
//!   the lambda slot, each 9-float window is projected to a scalar via
//!   `reify_stdlib::compute_von_mises_3x3`, and the resulting stride-1
//!   scalar buffer is delegated to the existing Sampled reduction path.
//!   This mirrors `reify_stdlib::fea::envelope_tensor_projection` (the
//!   proven two-pass pattern in this codebase).
//!
//! All other source kinds (`Analytical`, `Composed`, `Imported`, and
//! the derived wrappers `Gradient`/`Divergence`/`Curl`/`Laplacian`/
//! `PrincipalStresses`/`MaxShear`/`SafetyFactor`) return
//! `Value::Undef`.
//!
//! The deferred path for those kinds requires either numerical
//! optimisation over an analytical lambda's bounded domain (Nelder-Mead /
//! golden-section / coordinate descent) or sampled-subfield reduction —
//! see `docs/prds/v0_3/structural-analysis-fea.md` task #6.  The PRD
//! task description authorises this staging:
//! "Implementation can be staged — `sampled` first (FEA produces
//! sampled fields)."
//!
//! # NaN / empty data semantics
//!
//! `SampledField.data` is `Vec<f64>` and the elaborator
//! (`engine_eval::build_sampled_field`) does not reject NaN data values
//! — only NaN/inf spacings and degenerate axis grids. A reduction
//! must therefore handle NaN-bearing data: skip non-finite values
//! when reducing; if all values are non-finite (or `data.is_empty()`),
//! return `Value::Undef`. This matches the `safety_factor` poison
//! convention and the `sanitize_value` discipline elsewhere in stdlib.
//!
//! For VonMises projections, NaN tensor windows (out-of-solid sentinel
//! values used by the FEA elaborator) project to NaN via
//! `compute_von_mises_3x3` and are then skipped by the existing
//! `is_finite()` reduction, matching
//! `solve_elastic_static_e2e.rs`'s window-skip logic.

use std::sync::atomic::AtomicBool;

use reify_core::Type;
use reify_ir::{FieldSourceKind, SampledField, Value};

/// Compute `max(field)` — return the maximum codomain value of a
/// `Sampled`- or `VonMises`-source field, wrapped per the field's
/// `codomain_type`.
///
/// For `VonMises` fields the backing Sampled tensor field is projected
/// per 9-float window via `reify_stdlib::compute_von_mises_3x3` before
/// the reduction (see [`project_von_mises_sampled`]).
///
/// Other source kinds return `Value::Undef` (deferred — see module
/// doc-comment for the staging rationale).
pub(crate) fn compute_max(field_val: &Value) -> Value {
    compute_extremum(field_val, false)
}

/// Compute `min(field)` — return the minimum codomain value of a
/// `Sampled`- or `VonMises`-source field, wrapped per the field's
/// `codomain_type`.
///
/// For `VonMises` fields the backing Sampled tensor field is projected
/// per 9-float window via `reify_stdlib::compute_von_mises_3x3` before
/// the reduction (see [`project_von_mises_sampled`]).
///
/// Other source kinds return `Value::Undef` (deferred — see module
/// doc-comment for the staging rationale).
pub(crate) fn compute_min(field_val: &Value) -> Value {
    compute_extremum(field_val, true)
}

/// Compute `argmax(field)` — return the domain coord at which a
/// `Sampled`- or `VonMises`-source field attains its maximum value,
/// wrapped per the field's `domain_type`.
///
/// For `VonMises` fields the backing Sampled tensor field is projected
/// per 9-float window via `reify_stdlib::compute_von_mises_3x3` before
/// the index search (see [`project_von_mises_sampled`]).
///
/// Tie-break: lowest linear index wins (the `total_cmp` reduce keeps
/// the first-seen extremum on equal values).
///
/// Other source kinds return `Value::Undef` (deferred).
pub(crate) fn compute_argmax(field_val: &Value) -> Value {
    compute_argextremum(field_val, false)
}

/// Compute `argmin(field)` — return the domain coord at which a
/// `Sampled`- or `VonMises`-source field attains its minimum value,
/// wrapped per the field's `domain_type`.
///
/// For `VonMises` fields the backing Sampled tensor field is projected
/// per 9-float window via `reify_stdlib::compute_von_mises_3x3` before
/// the index search (see [`project_von_mises_sampled`]).
///
/// Tie-break: lowest linear index wins (mirrors `compute_argmax`).
///
/// Other source kinds return `Value::Undef` (deferred).
pub(crate) fn compute_argmin(field_val: &Value) -> Value {
    compute_argextremum(field_val, true)
}

// ─── Bounded reductions: max/min/argmax/argmin(field, bounds: BoundingBox) ───

/// Compute `max(field, bounds)` — return the maximum codomain value of a
/// `Sampled`-source field restricted to grid nodes inside `bounds`.
///
/// `Analytical`/`Composed` grid-sampling is implemented in step-4 of
/// task 4561.  All other source kinds return `Value::Undef`.
pub(crate) fn compute_max_bounded(
    field: &Value,
    bounds: &Value,
    ctx: &crate::EvalContext<'_>,
) -> Value {
    compute_bounded_extremum(field, bounds, false, ctx)
}

/// Compute `min(field, bounds)` — symmetric with [`compute_max_bounded`].
pub(crate) fn compute_min_bounded(
    field: &Value,
    bounds: &Value,
    ctx: &crate::EvalContext<'_>,
) -> Value {
    compute_bounded_extremum(field, bounds, true, ctx)
}

/// Compute `argmax(field, bounds)` — return the domain coord at the maximum
/// within `bounds` for a `Sampled`-source field.
pub(crate) fn compute_argmax_bounded(
    field: &Value,
    bounds: &Value,
    ctx: &crate::EvalContext<'_>,
) -> Value {
    compute_bounded_argextremum(field, bounds, false, ctx)
}

/// Compute `argmin(field, bounds)` — symmetric with [`compute_argmax_bounded`].
pub(crate) fn compute_argmin_bounded(
    field: &Value,
    bounds: &Value,
    ctx: &crate::EvalContext<'_>,
) -> Value {
    compute_bounded_argextremum(field, bounds, true, ctx)
}

/// Shared body for `compute_max_bounded` / `compute_min_bounded`.
///
/// # Sampled sub-region
///
/// Clips the grid to nodes whose per-axis coord ∈ [lo[k], hi[k]] inclusive
/// (raw SI f64, first `n = axis_grids.len()` bbox axes used).  Non-finite
/// data values are skipped via [`argmax_argmin_index`].  Empty sub-region
/// (no in-bounds nodes, or all non-finite) → `Value::Undef`.
///
/// # Analytical / Composed
///
/// Grid-sampler over the bounding box (task 4561 step-4, not yet
/// implemented here).  Returns `Value::Undef` until step-4 lands.
///
/// # Other sources
///
/// `VonMises`/`Imported`/derived sources → `Value::Undef`.
fn compute_bounded_extremum(
    field: &Value,
    bounds: &Value,
    find_min: bool,
    _ctx: &crate::EvalContext<'_>,
) -> Value {
    let (codomain_type, source, lambda) = match field {
        Value::Field { codomain_type, source, lambda, .. } => (codomain_type, source, lambda),
        _ => return Value::Undef,
    };

    let (lo, hi) = match bbox_coords(bounds) {
        Some(pair) => pair,
        None => return Value::Undef,
    };

    match source {
        FieldSourceKind::Sampled => match lambda.as_ref() {
            Value::SampledField(sf) => {
                reduce_sampled_extremum_bounded(sf, &lo, &hi, codomain_type, find_min)
            }
            _ => Value::Undef,
        },
        // Analytical/Composed: grid-sampler deferred to step-4.
        // 1-arg form remains honest-Undef (compute_extremum above).
        FieldSourceKind::Analytical | FieldSourceKind::Composed => Value::Undef,
        _ => Value::Undef,
    }
}

/// Shared body for `compute_argmax_bounded` / `compute_argmin_bounded`.
fn compute_bounded_argextremum(
    field: &Value,
    bounds: &Value,
    find_min: bool,
    _ctx: &crate::EvalContext<'_>,
) -> Value {
    let (domain_type, source, lambda) = match field {
        Value::Field { domain_type, source, lambda, .. } => (domain_type, source, lambda),
        _ => return Value::Undef,
    };

    let (lo, hi) = match bbox_coords(bounds) {
        Some(pair) => pair,
        None => return Value::Undef,
    };

    match source {
        FieldSourceKind::Sampled => match lambda.as_ref() {
            Value::SampledField(sf) => {
                reduce_sampled_argextremum_bounded(sf, &lo, &hi, domain_type, find_min)
            }
            _ => Value::Undef,
        },
        // Analytical/Composed: deferred to step-4.
        FieldSourceKind::Analytical | FieldSourceKind::Composed => Value::Undef,
        _ => Value::Undef,
    }
}

/// Extract `(lo_coords, hi_coords)` as `Vec<f64>` from a `Value::BoundingBox`.
///
/// The BoundingBox min/max corners are `Value::Point` of 3 components;
/// each component is `Value::Real` (dimensionless) or
/// `Value::Scalar { .. }` (dimensioned).  SI f64 is extracted via
/// [`Value::as_f64`].
///
/// Returns `None` if `bounds` is not a `BoundingBox`, or if any component
/// fails `as_f64()`.
fn bbox_coords(bounds: &Value) -> Option<(Vec<f64>, Vec<f64>)> {
    let (min_pt, max_pt) = match bounds {
        Value::BoundingBox { min, max } => (min.as_ref(), max.as_ref()),
        _ => return None,
    };

    let extract = |pt: &Value| -> Option<Vec<f64>> {
        match pt {
            Value::Point(components) => components.iter().map(|c| c.as_f64()).collect(),
            _ => None,
        }
    };

    Some((extract(min_pt)?, extract(max_pt)?))
}

/// Clip a `SampledField` to nodes within `[lo, hi]` (inclusive, per-axis)
/// and return the extremum codomain value.
///
/// `n = sf.axis_grids.len()` axes are checked; only the first `n` entries
/// of `lo`/`hi` are used.  Returns `Value::Undef` on empty sub-region or
/// shape mismatch.
fn reduce_sampled_extremum_bounded(
    sf: &SampledField,
    lo: &[f64],
    hi: &[f64],
    codomain_type: &reify_core::Type,
    find_min: bool,
) -> Value {
    let n = sf.axis_grids.len();
    if lo.len() < n || hi.len() < n {
        return Value::Undef;
    }

    let mut axis_lengths = [0usize; MAX_AXES];
    for (k, g) in sf.axis_grids.iter().enumerate().take(n) {
        axis_lengths[k] = g.len();
    }
    let expected_len: usize = axis_lengths[..n].iter().product();
    if sf.data.len() != expected_len {
        return Value::Undef;
    }

    let mut in_bounds_values: Vec<f64> = Vec::new();
    for linear in 0..sf.data.len() {
        let per_axis = decompose_index(linear, &axis_lengths[..n]);
        let mut ok = true;
        for k in 0..n {
            let coord = sf.axis_grids[k][per_axis[k]];
            if coord < lo[k] || coord > hi[k] {
                ok = false;
                break;
            }
        }
        if ok {
            in_bounds_values.push(sf.data[linear]);
        }
    }

    match argmax_argmin_index(&in_bounds_values, find_min) {
        Some(best_idx) => wrap_codomain(in_bounds_values[best_idx], codomain_type),
        None => Value::Undef,
    }
}

/// Clip a `SampledField` to nodes within `[lo, hi]` (inclusive, per-axis)
/// and return the domain coord at the extremum.
fn reduce_sampled_argextremum_bounded(
    sf: &SampledField,
    lo: &[f64],
    hi: &[f64],
    domain_type: &reify_core::Type,
    find_min: bool,
) -> Value {
    let n = sf.axis_grids.len();
    if lo.len() < n || hi.len() < n {
        return Value::Undef;
    }

    let mut axis_lengths = [0usize; MAX_AXES];
    for (k, g) in sf.axis_grids.iter().enumerate().take(n) {
        axis_lengths[k] = g.len();
    }
    let expected_len: usize = axis_lengths[..n].iter().product();
    if sf.data.len() != expected_len {
        return Value::Undef;
    }

    let mut in_bounds_values: Vec<f64> = Vec::new();
    let mut in_bounds_coords: Vec<[f64; MAX_AXES]> = Vec::new();

    for linear in 0..sf.data.len() {
        let per_axis = decompose_index(linear, &axis_lengths[..n]);
        let mut ok = true;
        let mut coords_si = [0.0f64; MAX_AXES];
        for k in 0..n {
            let coord = sf.axis_grids[k][per_axis[k]];
            coords_si[k] = coord;
            if coord < lo[k] || coord > hi[k] {
                ok = false;
                break;
            }
        }
        if ok {
            in_bounds_values.push(sf.data[linear]);
            in_bounds_coords.push(coords_si);
        }
    }

    match argmax_argmin_index(&in_bounds_values, find_min) {
        Some(best_idx) => {
            wrap_coord_for_domain(&in_bounds_coords[best_idx][..n], domain_type)
        }
        None => Value::Undef,
    }
}

/// Shared body for `compute_max` / `compute_min`. `find_min == true`
/// selects the minimum, `false` selects the maximum.
fn compute_extremum(field_val: &Value, find_min: bool) -> Value {
    let (codomain_type, source, lambda) = match field_val {
        Value::Field {
            codomain_type,
            source,
            lambda,
            ..
        } => (codomain_type, source, lambda),
        _ => return Value::Undef,
    };

    match source {
        FieldSourceKind::Sampled => match lambda.as_ref() {
            Value::SampledField(sf) => reduce_sampled_extremum(sf, codomain_type, find_min),
            // Defensive: a Sampled source must carry a SampledField in its
            // lambda slot. Anything else is a malformed runtime value;
            // return Undef rather than panicking.
            _ => Value::Undef,
        },
        // VonMises: project each 9-float tensor window in the backing Sampled
        // field to a scalar via `compute_von_mises_3x3`, then delegate to the
        // existing Sampled reduction path. Mirrors fea.rs::envelope_tensor_projection.
        // NaN windows (out-of-solid sentinel) project to NaN and are skipped
        // by the is_finite() gate in argmax_argmin_index.
        FieldSourceKind::VonMises => match project_von_mises_sampled(lambda.as_ref()) {
            Some(sf) => reduce_sampled_extremum(&sf, codomain_type, find_min),
            None => Value::Undef,
        },
        // TODO(future): numerical optimisation over Analytical/Composed lambda
        // domains (Nelder-Mead / golden-section / coordinate descent); sampled-
        // subfield reduction for Gradient/Divergence/Curl/Laplacian/MaxShear/
        // PrincipalStresses/SafetyFactor — see
        // PRD docs/prds/v0_3/structural-analysis-fea.md task #6 and §13 line 238
        // (deferred per task description's "Implementation can be staged —
        // sampled first"). Imported fields carry Value::Undef in their lambda
        // slot and cannot be reduced without a backing data buffer.
        //
        // Pinned by the step-15 / S5 negative-path tests:
        // - all_reductions_on_analytical_field_return_undef
        // - all_reductions_on_composed_field_return_undef
        // - all_reductions_on_imported_field_return_undef
        // - all_reductions_on_derived_non_vonmises_field_return_undef
        _ => Value::Undef,
    }
}

/// Project the backing Sampled tensor field stored in a VonMises field's
/// lambda slot into a fresh stride-1 scalar `SampledField`.
///
/// # Unwrap path
///
/// The lambda slot of a `VonMisesField` holds the ORIGINAL tensor
/// `Value::Field { source: Sampled, lambda: Arc<Value::SampledField(_)>, .. }`.
/// This helper performs two levels of unwrapping:
/// 1. `lambda` as `Value::Field { source: Sampled, lambda: inner, .. }`
/// 2. `inner.as_ref()` as `Value::SampledField(sf)` (the actual data buffer)
///
/// Returns `None` defensively for any other shape, mirroring the
/// `compute_extremum` Sampled defensive arm.
///
/// # Projection
///
/// Computes `grid_count = ∏ axis_grid lengths`, guards that
/// `sf.axis_grids` is non-empty, `grid_count > 0`, and
/// `sf.data.len() == grid_count * 9` (stride contract — mirrors
/// `fea.rs::extract_per_case_sampled_field`), then for each `i` in
/// `0..grid_count` pushes
/// `reify_stdlib::compute_von_mises_3x3(&sf.data[i*9..i*9+9])` into a new
/// scalar `Vec<f64>`.
///
/// Note: the `axis_grids.is_empty()` guard is technically redundant given
/// the `SampledGridKind` invariant (`Regular1D`/`Regular2D`/`Regular3D` all
/// carry at least one axis), but it prevents the empty-iterator identity
/// (`product() == 1`) from producing `grid_count == 1` on a structurally
/// impossible empty-axis field — symmetry with the documented stride
/// contract.
///
/// # Result
///
/// Returns a fresh `SampledField` copying `sf`'s grid metadata (`name`,
/// `kind`, `bounds_min`, `bounds_max`, `spacing`, `axis_grids`,
/// `interpolation`) with `data = projected_scalars` and
/// `oob_emitted: AtomicBool::new(false)` (fresh flag — the projected field
/// is an internal intermediary, so there is no user-visible duplicate-warning
/// surface to suppress, mirroring fea.rs line 804–813 rationale).
fn project_von_mises_sampled(lambda: &Value) -> Option<SampledField> {
    // Level 1: unwrap the Sampled tensor field from the VonMises lambda slot.
    let inner = match lambda {
        Value::Field {
            source: FieldSourceKind::Sampled,
            lambda: inner,
            ..
        } => inner,
        _ => return None,
    };

    // Level 2: unwrap the SampledField from the Sampled field's lambda slot.
    let sf = match inner.as_ref() {
        Value::SampledField(sf) => sf,
        _ => return None,
    };

    // Shape + stride contract: axis_grids must be non-empty (SampledGridKind
    // invariant guarantees this for Regular1D/2D/3D, but checked defensively
    // for directly-constructed fields bypassing that gate; an empty axis_grids
    // vec would yield product()==1, not 0, so it must be guarded separately),
    // grid_count must be non-zero, and data must be exactly grid_count * 9 floats.
    let grid_count: usize = sf.axis_grids.iter().map(|g| g.len()).product();
    if sf.axis_grids.is_empty() || grid_count == 0 || sf.data.len() != grid_count * 9 {
        return None;
    }

    // Project each 9-float window to a scalar von Mises value.
    let mut projected: Vec<f64> = Vec::with_capacity(grid_count);
    for i in 0..grid_count {
        projected.push(reify_stdlib::compute_von_mises_3x3(&sf.data[i * 9..i * 9 + 9]));
    }

    Some(SampledField {
        name: sf.name.clone(),
        kind: sf.kind,
        bounds_min: sf.bounds_min.clone(),
        bounds_max: sf.bounds_max.clone(),
        spacing: sf.spacing.clone(),
        axis_grids: sf.axis_grids.clone(),
        interpolation: sf.interpolation,
        data: projected,
        oob_emitted: AtomicBool::new(false),
    })
}

/// Reduce a `SampledField`'s data buffer to a single extremum value,
/// wrapped per the codomain type.
///
/// `find_min == false` → maximum; `find_min == true` → minimum.
///
/// # Single source of truth for scan / tie-break / NaN-skip semantics
///
/// Delegates the scan to [`argmax_argmin_index`] and indexes back into
/// `sf.data` to recover the extremum value. This keeps the
/// NaN-skip + `total_cmp` + first-occurrence-wins semantics in one
/// place — the doc-pinned invariants live on `argmax_argmin_index`
/// alone, and `compute_max` / `compute_min` cannot drift from
/// `compute_argmax` / `compute_argmin` on equal-valued samples.
///
/// # NaN / non-finite / empty handling
///
/// Non-finite values (NaN and ±∞) are skipped via `is_finite()` —
/// stricter than `!is_nan()` and matching the `sanitize_value`
/// discipline in `crates/reify-stdlib/src/helpers.rs`. An empty
/// data buffer or all-non-finite buffer returns `Value::Undef`.
///
/// Pinned by `max_sampled_with_nan_skips_nan_values`,
/// `all_reductions_sampled_all_nan_returns_undef`, and
/// `all_reductions_sampled_empty_data_returns_undef` in
/// `tests/field_reductions_tests.rs` (step-17 of plan 2913).
fn reduce_sampled_extremum(sf: &SampledField, codomain_type: &Type, find_min: bool) -> Value {
    match argmax_argmin_index(&sf.data, find_min) {
        Some(linear) => wrap_codomain(sf.data[linear], codomain_type),
        None => Value::Undef,
    }
}

/// Shared body for `compute_argmax` / `compute_argmin`. Locates the
/// extremum's linear index in the Sampled data buffer, decomposes it
/// into per-axis coords via `axis_grids`, and wraps the result per
/// the field's `domain_type`.
fn compute_argextremum(field_val: &Value, find_min: bool) -> Value {
    let (domain_type, source, lambda) = match field_val {
        Value::Field {
            domain_type,
            source,
            lambda,
            ..
        } => (domain_type, source, lambda),
        _ => return Value::Undef,
    };

    match source {
        FieldSourceKind::Sampled => match lambda.as_ref() {
            Value::SampledField(sf) => match argmax_argmin_index(&sf.data, find_min) {
                Some(linear) => arg_coord_from_index(sf, linear, domain_type),
                None => Value::Undef,
            },
            // Defensive: see compute_extremum's matching defensive arm.
            _ => Value::Undef,
        },
        // VonMises: project the backing tensor field per 9-float window, then
        // locate the extremum index in the projected scalar buffer and
        // decompose it against the inner field's axis_grids.
        // `project_von_mises_sampled` clones the inner grid metadata (including
        // axis_grids), so `arg_coord_from_index` operates on the same shape —
        // its shape guard (`data.len() == prod(axis_grid lengths)`) holds
        // because data.len() == grid_count == prod(axis_grid lengths) after
        // projection.
        FieldSourceKind::VonMises => match project_von_mises_sampled(lambda.as_ref()) {
            Some(sf) => match argmax_argmin_index(&sf.data, find_min) {
                Some(linear) => arg_coord_from_index(&sf, linear, domain_type),
                None => Value::Undef,
            },
            None => Value::Undef,
        },
        // TODO(future): see compute_extremum for the full deferred-path note.
        // Same staging rationale applies — argmax/argmin over Analytical/
        // Composed/Gradient/Divergence/Curl/Laplacian/MaxShear/
        // PrincipalStresses/SafetyFactor sources requires numerical optimisation
        // or sampled-subfield reduction, not yet in scope (PRD §13 line 238).
        // Pinned by the same step-15 / S5 negative-path tests as compute_extremum.
        _ => Value::Undef,
    }
}

/// Locate the linear index of the maximum (or minimum, when
/// `find_min`) finite value in `data`. Uses `total_cmp` for the
/// IEEE 754 totalOrder consistency (matches `Value::Real`/`Scalar`
/// `Ord` impls).
///
/// Non-finite values (NaN and ±∞) are skipped via `is_finite()` —
/// stricter than `!is_nan()` and matching the `sanitize_value`
/// discipline elsewhere. Returns `None` when `data` is empty or
/// contains no finite values. Tie-break: lowest linear index wins
/// (strict `<`/`>` rather than `<=`/`>=` keeps the first-seen
/// extremum on equal values).
fn argmax_argmin_index(data: &[f64], find_min: bool) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, &v) in data.iter().enumerate() {
        if !v.is_finite() {
            continue;
        }
        match best {
            None => best = Some((i, v)),
            Some((_, b)) => {
                let cmp = v.total_cmp(&b);
                let take = if find_min { cmp.is_lt() } else { cmp.is_gt() };
                if take {
                    best = Some((i, v));
                }
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Maximum number of axes a `SampledField` can carry.
///
/// Bounded by the `SampledGridKind` invariant (`Regular1D` / `Regular2D` /
/// `Regular3D`); used as the stack-array size for axis-length and per-axis
/// scratch buffers below to avoid heap allocation on the argmax/argmin path.
const MAX_AXES: usize = 3;

/// Look up the per-axis SI coords at `linear_index` in `sf.axis_grids`
/// and wrap them per `domain_type`.
///
/// The N-D loop below is fully generic across 1/2/3 axes — the
/// `SampledGridKind` invariant (`Regular1D`/`Regular2D`/`Regular3D`)
/// is reinforced by the `debug_assert!` here and in `decompose_index`
/// below. Pinned by the 1-D / 2-D / 3-D test suites in
/// `tests/field_reductions_tests.rs` (`argmax|argmin_sampled_field_*d_*`).
///
/// # Shape-mismatch guard
///
/// If `sf.data.len() != prod(axis_grid lengths)`, this function returns
/// `Value::Undef`. `engine_eval::build_sampled_field` enforces this
/// shape-equality invariant at construction; this guard is defense-in-depth
/// for SampledFields constructed directly (test fixtures, future ingest
/// paths) that bypass that gate. It mirrors the "malformed runtime value →
/// Undef" convention in `compute_extremum`'s defensive Sampled arm,
/// `compute_argextremum`'s matching arm, and `wrap_coord_for_domain`'s
/// catch-all Undef arm.
///
/// # Allocation
///
/// All scratch buffers (`axis_lengths`, `per_axis`, `coords_si`) are
/// stack-allocated `[_; MAX_AXES]` arrays sliced down to the actual axis
/// count. No heap allocation on the argmax/argmin path.
fn arg_coord_from_index(sf: &SampledField, linear_index: usize, domain_type: &Type) -> Value {
    let n = sf.axis_grids.len();
    debug_assert!(
        matches!(n, 1..=MAX_AXES),
        "SampledGridKind invariant: 1/2/3 axes only, got {n}"
    );

    // Decompose the linear index into per-axis indices (axis-0 outermost,
    // row-major). Stack-allocated buffers — no heap allocation here.
    let mut axis_lengths = [0usize; MAX_AXES];
    for (k, g) in sf.axis_grids.iter().enumerate().take(n) {
        axis_lengths[k] = g.len();
    }

    // Defense-in-depth: a malformed SampledField with data.len() != prod(axis_lengths)
    // would otherwise either (a) panic on division-by-zero in decompose_index when an
    // axis_grid is empty (axis_lengths[k] == 0, rem % 0), or (b) silently return a wrong
    // coord because decompose_index's modulo-at-every-level wraps an out-of-range linear
    // index back into bounds. `engine_eval::build_sampled_field` rejects this shape
    // mismatch at construction, but direct construction bypasses that gate.
    let expected_len: usize = axis_lengths[..n].iter().product();
    if sf.data.len() != expected_len {
        return Value::Undef;
    }

    let per_axis = decompose_index(linear_index, &axis_lengths[..n]);

    // Look up SI coords from axis_grids.
    let mut coords_si = [0.0f64; MAX_AXES];
    for k in 0..n {
        coords_si[k] = sf.axis_grids[k][per_axis[k]];
    }

    wrap_coord_for_domain(&coords_si[..n], domain_type)
}

/// Decompose a row-major linear index into per-axis indices.
///
/// Convention: axis-0 outermost (matches `engine_eval::build_sampled_field`
/// and `interp::interpolate_Nd`'s row-major layout).
///
/// For shape `(s0, s1, ..., s_{N-1})` and linear index `i`:
/// ```text
/// i_{N-1} = i % s_{N-1}
/// i_{N-2} = (i / s_{N-1}) % s_{N-2}
/// ...
/// i_0    = i / (s_1 * s_2 * ... * s_{N-1})
/// ```
///
/// # Return shape
///
/// Returns a fixed-size `[usize; MAX_AXES]` (stack-allocated) — the caller
/// reads only the first `axis_lengths.len()` entries; the remainder is
/// zero-padded. This avoids the per-call heap allocation that a `Vec`
/// would incur and matches the SampledGridKind invariant (1..=3 axes).
fn decompose_index(linear: usize, axis_lengths: &[usize]) -> [usize; MAX_AXES] {
    debug_assert!(
        matches!(axis_lengths.len(), 1..=MAX_AXES),
        "SampledGridKind invariant: 1/2/3 axes only"
    );
    let mut out = [0usize; MAX_AXES];
    let mut rem = linear;
    for k in (0..axis_lengths.len()).rev() {
        let s = axis_lengths[k];
        out[k] = rem % s;
        rem /= s;
    }
    out
}

/// Wrap per-axis SI coords as a `Value` per the field's `domain_type`.
///
/// Supported domains:
/// - **1-D scalar domain** (`Type::Real`, `Type::Scalar { dim }`):
///   returns a single `Value::Real` (dimensionless) or `Value::Scalar`
///   (dimensioned). Requires `coords_si.len() == 1`.
/// - **N-D Point domain** (`Type::Point { n, quantity }` where
///   `quantity ∈ { Type::Real, Type::Scalar { .. } }`): returns
///   `Value::Point(per-axis-coords)` where each component follows the
///   same per-quantity wrap rule. Requires `coords_si.len() == n`.
///
/// Unsupported domains (return `Value::Undef`):
/// - `Type::Int` — `axis_grids` are stored as `f64` and there is no
///   precise integer round-trip; an Int domain is unsupported rather
///   than silently coerced to `Value::Real`.
/// - `Type::Point { quantity }` where `quantity` is `Type::Int` (or
///   any other non-Real / non-Scalar type) — same rationale.
/// - Mismatches between `coords_si.len()` and the domain's expected
///   dimensionality (e.g., 3-D grid wrapped as a 1-D-domain field, or
///   vice versa) — user-driven via field type/source mistypes.
/// - Any other domain type.
///
/// The eval engine's diagnostic channel is not reachable from here, so
/// the `Undef` return is the only signal — matching `analysis::*` /
/// `sampled::wrap_result` conventions.
fn wrap_coord_for_domain(coords_si: &[f64], domain_type: &Type) -> Value {
    match domain_type {
        Type::Point { n, quantity } if coords_si.len() == *n => {
            // Reject Point<Int> (and any other unsupported quantity) up
            // front so the result is uniformly Undef rather than a
            // Point of silently-coerced Reals.
            if !is_supported_scalar_quantity(quantity) {
                return Value::Undef;
            }
            let components: Vec<Value> = coords_si
                .iter()
                .map(|&c| wrap_scalar_coord(c, quantity))
                .collect();
            Value::Point(components)
        }
        // 1-D scalar/dimensionless domain: single coord. `Type::Int` is
        // intentionally NOT in this arm — see doc-comment above.
        Type::Real | Type::Scalar { .. } if coords_si.len() == 1 => {
            wrap_scalar_coord(coords_si[0], domain_type)
        }
        _ => Value::Undef,
    }
}

/// Predicate: is `quantity` a supported per-axis scalar quantity for
/// `Point`-domain wrapping?
///
/// Returns true only for `Type::Real` and `Type::Scalar { .. }`.
/// `Type::Int` and other types are rejected — see [`wrap_coord_for_domain`]
/// for the rationale (no precise integer round-trip from `axis_grids`'
/// `f64` storage).
fn is_supported_scalar_quantity(ty: &Type) -> bool {
    matches!(ty, Type::Real | Type::Scalar { .. })
}

/// Wrap a single SI coord per a scalar quantity type.
///
/// Contract:
/// - `Type::Scalar { dimension }` with non-dimensionless `dimension`
///   → `Value::Scalar { si_value, dimension }`.
/// - `Type::Real` and `Type::Scalar` with dimensionless `dimension`
///   → `Value::Real(coord_si)`.
///
/// Callers MUST pre-filter `quantity` via [`is_supported_scalar_quantity`]
/// — passing any other type (e.g. `Type::Int`) hits the catch-all arm
/// and silently returns `Value::Real`, which is incorrect for the caller's
/// contract. The `wrap_coord_for_domain` Point arm performs this check.
/// The 1-D scalar arm only routes `Type::Real` / `Type::Scalar` here, so
/// it is also safe.
fn wrap_scalar_coord(coord_si: f64, quantity: &Type) -> Value {
    match quantity {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Value::Scalar {
            si_value: coord_si,
            dimension: *dimension,
        },
        _ => Value::Real(coord_si),
    }
}

/// Wrap an SI f64 in the field's codomain shape.
///
/// Mirrors `crate::sampled::wrap_result` exactly:
/// - `Type::Scalar { dimension }` with non-dimensionless `dimension`
///   (e.g. `PRESSURE`, `LENGTH`) → `Value::Scalar { si_value, dimension }`,
///   preserving the field's codomain dimension on the reduction result so
///   `max(von_mises(stress)) < yield_stress` etc. unify dimensionally.
/// - `Type::Real`, `Type::Int`, dimensionless `Type::Scalar`, and any
///   other codomain → `Value::Real(v)` (the `_` arm is the dimensionless
///   default; the codomain type is otherwise unused for max/min).
fn wrap_codomain(v: f64, codomain_type: &Type) -> Value {
    match codomain_type {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Value::Scalar {
            si_value: v,
            dimension: *dimension,
        },
        _ => Value::Real(v),
    }
}
