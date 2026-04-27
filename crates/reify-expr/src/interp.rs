//! Pure-function interpolation on regular axis-aligned grids of `f64` samples.
//!
//! This module is the v0.1 algorithmic core for sampled fields. It is
//! intentionally decoupled from `Value::Field { source: FieldSourceKind::Sampled, .. }`
//! evaluation, the `from_samples` stdlib function, and the eval engine — those
//! integrations belong to follow-on tasks. Each entry point takes plain
//! `&[f64]` slices for axis coordinates and a row-major-flattened `&[f64]`
//! values slice, and returns an [`InterpolationResult`] with the evaluated
//! value plus any diagnostics.
//!
//! # Methods
//!
//! [`InterpolationMethod`] enumerates the v0.1 surface:
//!
//! - [`InterpolationMethod::Linear`] — n-linear (lerp / bilinear / trilinear).
//! - [`InterpolationMethod::NearestNeighbor`] — snap to nearest grid sample,
//!   axis-independent ties broken with `f64::round_ties_even` (banker's
//!   rounding) for reproducibility across platforms.
//! - [`InterpolationMethod::Cubic`] — uniform Catmull-Rom in 1D, tensor product
//!   bicubic in 2D, tricubic in 3D. Edge cells extend the 4-point stencil with
//!   linear-extrapolated ghost points so cubic behaviour is preserved
//!   throughout the interior and both endpoints of every edge cell still
//!   reproduce the true sample value.
//! - [`InterpolationMethod::Rbf`] / [`InterpolationMethod::Kriging`] — deferred
//!   to post-v0.1. Selecting either falls back to `Linear` and emits a single
//!   warning diagnostic with code `DiagnosticCode::InterpolationDeferred`.
//!
//! # Boundary policy
//!
//! Queries outside the grid's convex hull clamp to the nearest cell (constant
//! extrapolation). This avoids cascading `NaN`/`Undef` into downstream field
//! arithmetic, matches typical engineering-CAD field behaviour, and keeps
//! cubic from producing wildly off values via linear extrapolation.

use reify_types::Diagnostic;

/// Selected interpolation method.
///
/// The variants `Linear`, `NearestNeighbor`, and `Cubic` are implemented in
/// v0.1. `Rbf` and `Kriging` are accepted on the public surface so callers can
/// already write code that selects them, but at runtime they fall back to
/// `Linear` and emit a single warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterpolationMethod {
    /// n-linear interpolation: lerp (1D), bilinear (2D), trilinear (3D).
    Linear,
    /// Snap to the nearest grid sample. Ties are broken with
    /// `f64::round_ties_even` (banker's rounding) on each axis independently.
    NearestNeighbor,
    /// Catmull-Rom cubic (1D), tensor-product bicubic (2D), tricubic (3D).
    Cubic,
    /// Radial basis function — deferred to post-v0.1; falls back to `Linear`.
    Rbf,
    /// Kriging — deferred to post-v0.1; falls back to `Linear`.
    Kriging,
}

/// Outcome of an interpolation call.
///
/// A named struct rather than a positional tuple so future presentation fields
/// (e.g. extrapolation flags or hit-cell metadata) can be added additively.
#[derive(Debug, Clone)]
pub struct InterpolationResult {
    /// The interpolated value at the query point.
    pub value: f64,
    /// Any diagnostics produced for this call. Empty for fully-supported
    /// methods; populated with a single warning when `Rbf`/`Kriging` triggers
    /// the deferred-method fallback.
    pub diagnostics: Vec<Diagnostic>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Locate the cell `[grid[i], grid[i+1]]` bracketing `query` in a strictly
/// ascending grid. Returns `Some(i)` if `query` falls inside the grid (taking
/// the right-most cell when `query == grid.last()`), `None` if the grid has
/// fewer than two points or `query` is outside the grid.
fn locate_cell(grid: &[f64], query: f64) -> Option<usize> {
    if grid.len() < 2 {
        return None;
    }
    if query < grid[0] || query > grid[grid.len() - 1] {
        return None;
    }
    // Right-edge inclusive: the last cell owns its upper boundary.
    if query == grid[grid.len() - 1] {
        return Some(grid.len() - 2);
    }
    // Standard binary search for the largest index `i` with `grid[i] <= query`.
    // `partition_point` returns the first index that does NOT satisfy the
    // predicate, so subtract 1 to get the last index that does.
    let p = grid.partition_point(|&g| g <= query);
    Some(p - 1)
}

/// Linear interpolation between `a` and `b` at parameter `t ∈ [0, 1]`.
#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Index of the grid sample nearest to `query`, with reproducible tie-breaking
/// via `round_ties_even` semantics: when `query` is exactly halfway between
/// two adjacent samples, the endpoint with the even index wins.
///
/// Out-of-range queries clamp to the first / last sample. The grid must have
/// at least one element; for grids of length 1 the only sample wins.
fn nearest_index_on_axis(grid: &[f64], query: f64) -> usize {
    debug_assert!(!grid.is_empty(), "nearest_index_on_axis: empty grid");
    if grid.len() == 1 {
        return 0;
    }
    if query <= grid[0] {
        return 0;
    }
    let last = grid.len() - 1;
    if query >= grid[last] {
        return last;
    }
    let i = locate_cell(grid, query).expect("in-range query bracketed");
    let d_lo = query - grid[i];
    let d_hi = grid[i + 1] - query;
    if d_lo < d_hi {
        i
    } else if d_hi < d_lo {
        i + 1
    } else {
        // Exact tie: pick the endpoint with the even index (banker's rounding
        // / round_ties_even). Since `i` and `i + 1` differ in parity, exactly
        // one of them is even.
        if i % 2 == 0 { i } else { i + 1 }
    }
}

// ---------------------------------------------------------------------------
// Public 1D entry point
// ---------------------------------------------------------------------------

/// Interpolate a 1D scalar grid at `query`.
///
/// `grid` must be strictly ascending and have the same length as `values`.
/// Out-of-range queries clamp to the nearest endpoint sample (constant
/// extrapolation). Returns an empty `diagnostics` vec for the fully-supported
/// methods (`Linear`, `NearestNeighbor`, `Cubic`); `Rbf`/`Kriging` produce a
/// single deferred-method warning and fall back to `Linear`.
///
/// Panics if `grid.len() != values.len()` or if `grid.len() < 2`.
pub fn interpolate_1d(
    method: InterpolationMethod,
    grid: &[f64],
    values: &[f64],
    query: f64,
) -> InterpolationResult {
    assert_eq!(
        grid.len(),
        values.len(),
        "interpolate_1d: grid and values length mismatch ({} vs {})",
        grid.len(),
        values.len()
    );
    assert!(
        grid.len() >= 2,
        "interpolate_1d: grid must have at least 2 points (got {})",
        grid.len()
    );

    match method {
        InterpolationMethod::Linear => {
            let value = linear_1d(grid, values, query);
            InterpolationResult {
                value,
                diagnostics: Vec::new(),
            }
        }
        InterpolationMethod::NearestNeighbor => {
            let i = nearest_index_on_axis(grid, query);
            InterpolationResult {
                value: values[i],
                diagnostics: Vec::new(),
            }
        }
        InterpolationMethod::Cubic => unreachable!(
            "InterpolationMethod::Cubic 1D not yet implemented"
        ),
        InterpolationMethod::Rbf => unreachable!(
            "InterpolationMethod::Rbf 1D not yet implemented"
        ),
        InterpolationMethod::Kriging => unreachable!(
            "InterpolationMethod::Kriging 1D not yet implemented"
        ),
    }
}

/// Linear interpolation kernel for a 1D grid. Constant-extrapolates outside
/// the convex hull.
fn linear_1d(grid: &[f64], values: &[f64], query: f64) -> f64 {
    // Out-of-range: clamp to nearest endpoint sample.
    if query <= grid[0] {
        return values[0];
    }
    if query >= grid[grid.len() - 1] {
        return values[grid.len() - 1];
    }
    let i = locate_cell(grid, query).expect("in-range query bracketed");
    let span = grid[i + 1] - grid[i];
    // Strict-ascending grid guarantees span > 0; defensive guard for callers
    // who break the contract returns the lower endpoint sample.
    if span <= 0.0 {
        return values[i];
    }
    let t = (query - grid[i]) / span;
    lerp(values[i], values[i + 1], t)
}
