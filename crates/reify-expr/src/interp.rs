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
//!   axis-independent ties broken with the even-grid-index tie-breaker (the
//!   endpoint of the bracketing cell whose grid index is even wins) for
//!   reproducibility across platforms. Note that this is a grid-offset-dependent
//!   policy on the index, not banker's rounding on the value.
//! - [`InterpolationMethod::Cubic`] — 4-point Lagrange cubic in 1D (i.e.
//!   the unique cubic polynomial through four equally-spaced control values),
//!   tensor product bicubic in 2D, tricubic in 3D. Edge cells extend the
//!   4-point stencil with linear-extrapolated ghost points so cubic behaviour
//!   is preserved throughout the interior and both endpoints of every edge
//!   cell still reproduce the true sample value. The Lagrange (rather than
//!   Catmull-Rom) formulation is chosen so cubic polynomials are reproduced
//!   exactly within interior cells.
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
//!
//! NaN queries propagate to a NaN value with no diagnostics; this is the
//! IEEE 754 NaN-poisoning convention and matches the silent treatment of
//! out-of-range queries.

use reify_types::{Diagnostic, DiagnosticCode};

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
    /// Snap to the nearest grid sample. Ties are broken with the
    /// even-grid-index tie-breaker (the endpoint of the bracketing cell whose
    /// grid index is even wins) on each axis independently.
    NearestNeighbor,
    /// 4-point Lagrange cubic (1D), tensor-product bicubic (2D), tricubic (3D).
    /// See module-level docs for the rationale (Lagrange reproduces cubic
    /// polynomials exactly; Catmull-Rom does not).
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

/// Resolve a possibly-deferred [`InterpolationMethod`] to a concrete v0.1
/// implementation, producing a single warning diagnostic when a deferred
/// method is requested.
///
/// `Rbf` and `Kriging` map to [`InterpolationMethod::Linear`] plus a single
/// `Severity::Warning` diagnostic with code
/// [`DiagnosticCode::InterpolationDeferred`] and message of the form
/// `"interpolation method '<RBF|Kriging>' is deferred to post-v0.1; falling back to Linear"`.
/// All other methods pass through unchanged with no diagnostic.
fn resolve_method(method: InterpolationMethod) -> (InterpolationMethod, Option<Diagnostic>) {
    let deferred_name = match method {
        InterpolationMethod::Rbf => "RBF",
        InterpolationMethod::Kriging => "Kriging",
        _ => return (method, None),
    };
    let msg = format!(
        "interpolation method '{deferred_name}' is deferred to post-v0.1; falling back to Linear"
    );
    let diag = Diagnostic::warning(msg).with_code(DiagnosticCode::InterpolationDeferred);
    (InterpolationMethod::Linear, Some(diag))
}

/// Locate the cell `[grid[i], grid[i+1]]` bracketing `query` in a strictly
/// ascending grid. Returns `Some(i)` if `query` falls inside the grid (taking
/// the right-most cell when `query == grid.last()`), `None` if the grid has
/// fewer than two points, `query` is outside the grid, or `query` is NaN
/// (NaN is by definition not in the grid).
fn locate_cell(grid: &[f64], query: f64) -> Option<usize> {
    if grid.len() < 2 {
        return None;
    }
    if query.is_nan() {
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
/// via the even-grid-index policy: when `query` is exactly halfway between
/// two adjacent samples, the endpoint of the bracketing cell whose grid index
/// is even wins. This is reproducible across platforms but is grid-offset
/// dependent — it depends on the index of the bracketing samples, not on the
/// query's value alone, and is therefore not equivalent to
/// `f64::round_ties_even` (banker's rounding) on the value.
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
        // Exact tie: pick the endpoint with the even grid index. Since `i` and
        // `i + 1` differ in parity, exactly one of them is even. This is a
        // reproducible grid-offset-dependent policy; not banker's rounding on
        // the value.
        if i.is_multiple_of(2) { i } else { i + 1 }
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
/// NaN queries propagate to a NaN value with no diagnostics (IEEE 754
/// NaN-poisoning convention).
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
    if query.is_nan() {
        return InterpolationResult {
            value: f64::NAN,
            diagnostics: Vec::new(),
        };
    }

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
        InterpolationMethod::Cubic => {
            let value = cubic_1d(grid, values, query);
            InterpolationResult {
                value,
                diagnostics: Vec::new(),
            }
        }
        InterpolationMethod::Rbf | InterpolationMethod::Kriging => {
            let (resolved, diag) = resolve_method(method);
            let mut result = interpolate_1d(resolved, grid, values, query);
            if let Some(d) = diag {
                result.diagnostics.push(d);
            }
            result
        }
    }
}

/// Evaluate the 4-point Lagrange cubic interpolating `(p0, p1, p2, p3)` at
/// equally-spaced parameters `(-1, 0, 1, 2)` for query parameter `t ∈ [0, 1]`.
///
/// Returns `p1` at `t=0` and `p2` at `t=1`. Reproduces any cubic polynomial
/// exactly when the four control values come from the polynomial at the
/// matching parameters — this is the property required by the v0.1
/// `cubic_1d_reproduces_cubic_polynomial_in_interior` test.
///
/// # Note on naming
///
/// The original task plan referred to this kernel as Catmull-Rom. Standard
/// Catmull-Rom (a cardinal cubic Hermite spline with tangents
/// `(p_{i+1} - p_{i-1})/2`) does *not* reproduce arbitrary cubic polynomials
/// — centred-difference tangent estimates carry an `O(h^2)` error in the
/// cubic term. The 4-point Lagrange formulation does reproduce cubics
/// exactly, which is the binding test contract. Both formulations agree at
/// `t ∈ {0, 0.5, 1}` but diverge at all other parameter values.
#[inline]
fn cubic4_eval(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    // Lagrange basis at parameters (-1, 0, 1, 2):
    //   L_{-1}(t) = -t(t-1)(t-2)/6
    //   L_{0}(t)  =  (t+1)(t-1)(t-2)/2
    //   L_{1}(t)  = -(t+1)t(t-2)/2
    //   L_{2}(t)  =  (t+1)t(t-1)/6
    let tm1 = t - 1.0;
    let tm2 = t - 2.0;
    let tp1 = t + 1.0;
    let l0 = -t * tm1 * tm2 / 6.0;
    let l1 = tp1 * tm1 * tm2 / 2.0;
    let l2 = -tp1 * t * tm2 / 2.0;
    let l3 = tp1 * t * tm1 / 6.0;
    l0 * p0 + l1 * p1 + l2 * p2 + l3 * p3
}

/// 4-point cubic kernel for a 1D grid that reads sample values via a closure.
///
/// This is the canonical cubic implementation; [`cubic_1d`] is a thin wrapper
/// that supplies a slice-indexing accessor. Allowing a closure-based accessor
/// lets the higher-dimensional kernels evaluate a tensor-product cubic without
/// materialising any intermediate row/column/slice `Vec`s — they read only
/// the (≤4) bracketing samples on each axis directly.
///
/// Boundary cells synthesise missing control values via linear extrapolation
/// (`p_{-1} = 2*p1 - p2` / `p_{n} = 2*p2 - p1`), so cubic behaviour is
/// preserved everywhere in the interior and the 2-point degenerate case
/// collapses to linear (an algebraic identity verifiable by substituting the
/// ghost expressions into the Lagrange basis sum). Constant-extrapolates
/// outside the convex hull. Reads at most 4 samples (1 at boundaries).
fn cubic_1d_kernel<F: Fn(usize) -> f64>(grid: &[f64], query: f64, val: F) -> f64 {
    if query <= grid[0] {
        return val(0);
    }
    let last = grid.len() - 1;
    if query >= grid[last] {
        return val(last);
    }
    let i = locate_cell(grid, query).expect("in-range query bracketed");
    let span = grid[i + 1] - grid[i];
    if span <= 0.0 {
        return val(i);
    }
    let t = (query - grid[i]) / span;

    let p1 = val(i);
    let p2 = val(i + 1);
    let p0 = if i == 0 { 2.0 * p1 - p2 } else { val(i - 1) };
    let p3 = if i + 2 > last {
        2.0 * p2 - p1
    } else {
        val(i + 2)
    };
    cubic4_eval(p0, p1, p2, p3, t)
}

/// Slice-based 1D cubic kernel — thin wrapper over [`cubic_1d_kernel`].
fn cubic_1d(grid: &[f64], values: &[f64], query: f64) -> f64 {
    cubic_1d_kernel(grid, query, |i| values[i])
}

/// Row-major flat-index for a 2D grid with `ny` columns: `values[i * ny + j]`
/// is the sample at `(grid_x[i], grid_y[j])`.
#[inline]
fn index_2d(i: usize, j: usize, ny: usize) -> usize {
    i * ny + j
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

// ---------------------------------------------------------------------------
// Public 2D entry point
// ---------------------------------------------------------------------------

/// Interpolate a 2D scalar grid at `query = (x, y)`.
///
/// `grid_x` and `grid_y` must each be strictly ascending. `values` is a
/// row-major flattened slice of shape `(grid_x.len(), grid_y.len())` such
/// that `values[i * grid_y.len() + j]` is the sample at
/// `(grid_x[i], grid_y[j])`.
///
/// Out-of-range queries clamp each axis independently to the nearest
/// endpoint (constant extrapolation). `Rbf`/`Kriging` fall back to `Linear`
/// and emit a single deferred-method warning.
///
/// If any component of `query` is NaN, returns a NaN value with no
/// diagnostics (IEEE 754 NaN-poisoning convention: any-component-NaN poisons
/// the result).
///
/// Panics if `grid_x.len() < 2`, `grid_y.len() < 2`, or
/// `values.len() != grid_x.len() * grid_y.len()`.
pub fn interpolate_2d(
    method: InterpolationMethod,
    grid_x: &[f64],
    grid_y: &[f64],
    values: &[f64],
    query: (f64, f64),
) -> InterpolationResult {
    assert!(
        grid_x.len() >= 2,
        "interpolate_2d: grid_x must have at least 2 points"
    );
    assert!(
        grid_y.len() >= 2,
        "interpolate_2d: grid_y must have at least 2 points"
    );
    assert_eq!(
        values.len(),
        grid_x.len() * grid_y.len(),
        "interpolate_2d: values length {} does not match grid shape ({}, {})",
        values.len(),
        grid_x.len(),
        grid_y.len()
    );
    if query.0.is_nan() || query.1.is_nan() {
        return InterpolationResult {
            value: f64::NAN,
            diagnostics: Vec::new(),
        };
    }

    match method {
        InterpolationMethod::Linear => {
            let value = linear_2d(grid_x, grid_y, values, query);
            InterpolationResult {
                value,
                diagnostics: Vec::new(),
            }
        }
        InterpolationMethod::NearestNeighbor => {
            let i = nearest_index_on_axis(grid_x, query.0);
            let j = nearest_index_on_axis(grid_y, query.1);
            InterpolationResult {
                value: values[index_2d(i, j, grid_y.len())],
                diagnostics: Vec::new(),
            }
        }
        InterpolationMethod::Cubic => {
            let value = cubic_2d(grid_x, grid_y, values, query);
            InterpolationResult {
                value,
                diagnostics: Vec::new(),
            }
        }
        InterpolationMethod::Rbf | InterpolationMethod::Kriging => {
            let (resolved, diag) = resolve_method(method);
            let mut result = interpolate_2d(resolved, grid_x, grid_y, values, query);
            if let Some(d) = diag {
                result.diagnostics.push(d);
            }
            result
        }
    }
}

/// Bilinear kernel for a 2D grid. Reads only the four corner values
/// bracketing the query and applies two `lerp`s along `x` followed by one
/// along `y`. O(1) per query — no allocations.
///
/// Out-of-range queries are handled by [`locate_cell_with_clamp`], which
/// returns a valid cell index `(i, i+1)` and a `t ∈ {0.0, 1.0}` clamped
/// against the boundary; the final `lerp(v_lo, v_hi, ty)` then collapses
/// to whichever row is on the in-range side.
fn linear_2d(
    grid_x: &[f64],
    grid_y: &[f64],
    values: &[f64],
    query: (f64, f64),
) -> f64 {
    let (qx, qy) = query;
    let ny = grid_y.len();
    let (i, tx) = locate_cell_with_clamp(grid_x, qx);
    let (j, ty) = locate_cell_with_clamp(grid_y, qy);
    // Four corner samples bracketing the query cell.
    let v00 = values[index_2d(i, j, ny)];
    let v01 = values[index_2d(i, j + 1, ny)];
    let v10 = values[index_2d(i + 1, j, ny)];
    let v11 = values[index_2d(i + 1, j + 1, ny)];
    // Collapse along x for the two y-rows, then along y.
    let v_lo = lerp(v00, v10, tx);
    let v_hi = lerp(v01, v11, tx);
    lerp(v_lo, v_hi, ty)
}

/// Bicubic kernel for a 2D grid that reads sample values via a closure.
///
/// Computed as a tensor product of 1D cubic interpolations via
/// [`cubic_1d_kernel`]: for each of the (≤4) x-indices in the bracketing
/// 4×4 stencil, evaluate the cubic along `y` to collapse the column to a
/// single value, then evaluate the cubic along `x` over those intermediates.
/// Boundary cells inherit the linear-extrapolated ghost-point convention.
/// Reads at most 16 samples per query — no `Vec` allocations.
fn cubic_2d_kernel<F: Fn(usize, usize) -> f64>(
    grid_x: &[f64],
    grid_y: &[f64],
    qx: f64,
    qy: f64,
    val: F,
) -> f64 {
    let last_x = grid_x.len() - 1;
    // Collapse the y-axis at a fixed x-index to a single cubic-interpolated
    // value. Uses the same closure-driven kernel as the 1D entry point, so
    // boundary clamps and ghost points behave identically.
    let y_collapse = |i_idx: usize| cubic_1d_kernel(grid_y, qy, |j| val(i_idx, j));

    if qx <= grid_x[0] {
        return y_collapse(0);
    }
    if qx >= grid_x[last_x] {
        return y_collapse(last_x);
    }
    let i = locate_cell(grid_x, qx).expect("in-range query bracketed");
    let span_x = grid_x[i + 1] - grid_x[i];
    if span_x <= 0.0 {
        return y_collapse(i);
    }
    let tx = (qx - grid_x[i]) / span_x;

    let p1 = y_collapse(i);
    let p2 = y_collapse(i + 1);
    let p0 = if i == 0 {
        2.0 * p1 - p2
    } else {
        y_collapse(i - 1)
    };
    let p3 = if i + 2 > last_x {
        2.0 * p2 - p1
    } else {
        y_collapse(i + 2)
    };
    cubic4_eval(p0, p1, p2, p3, tx)
}

/// Slice-based 2D cubic kernel — thin wrapper over [`cubic_2d_kernel`].
fn cubic_2d(
    grid_x: &[f64],
    grid_y: &[f64],
    values: &[f64],
    query: (f64, f64),
) -> f64 {
    let (qx, qy) = query;
    let ny = grid_y.len();
    cubic_2d_kernel(grid_x, grid_y, qx, qy, |i, j| values[index_2d(i, j, ny)])
}

// ---------------------------------------------------------------------------
// Public 3D entry point
// ---------------------------------------------------------------------------

/// Row-major flat-index for a 3D grid with `ny` and `nz` columns/depths:
/// `values[i * ny * nz + j * nz + k]` is the sample at
/// `(grid_x[i], grid_y[j], grid_z[k])`.
#[inline]
fn index_3d(i: usize, j: usize, k: usize, ny: usize, nz: usize) -> usize {
    i * ny * nz + j * nz + k
}

/// Interpolate a 3D scalar grid at `query = (x, y, z)`.
///
/// `grid_x`, `grid_y`, `grid_z` must each be strictly ascending. `values` is a
/// row-major flattened slice of shape `(grid_x.len(), grid_y.len(), grid_z.len())`
/// using the layout `values[i * ny * nz + j * nz + k]` — i.e. `z` varies
/// fastest, then `y`, then `x`.
///
/// Out-of-range queries clamp each axis independently to the nearest endpoint
/// (constant extrapolation). `Rbf`/`Kriging` fall back to `Linear` and emit a
/// single deferred-method warning.
///
/// If any component of `query` is NaN, returns a NaN value with no diagnostics
/// (IEEE 754 NaN-poisoning convention: any-component-NaN poisons the result).
///
/// Panics if any axis has fewer than 2 points or `values.len()` does not match
/// `grid_x.len() * grid_y.len() * grid_z.len()`.
pub fn interpolate_3d(
    method: InterpolationMethod,
    grid_x: &[f64],
    grid_y: &[f64],
    grid_z: &[f64],
    values: &[f64],
    query: (f64, f64, f64),
) -> InterpolationResult {
    assert!(
        grid_x.len() >= 2,
        "interpolate_3d: grid_x must have at least 2 points"
    );
    assert!(
        grid_y.len() >= 2,
        "interpolate_3d: grid_y must have at least 2 points"
    );
    assert!(
        grid_z.len() >= 2,
        "interpolate_3d: grid_z must have at least 2 points"
    );
    assert_eq!(
        values.len(),
        grid_x.len() * grid_y.len() * grid_z.len(),
        "interpolate_3d: values length {} does not match grid shape ({}, {}, {})",
        values.len(),
        grid_x.len(),
        grid_y.len(),
        grid_z.len()
    );
    if query.0.is_nan() || query.1.is_nan() || query.2.is_nan() {
        return InterpolationResult {
            value: f64::NAN,
            diagnostics: Vec::new(),
        };
    }

    match method {
        InterpolationMethod::Linear => {
            let value = linear_3d(grid_x, grid_y, grid_z, values, query);
            InterpolationResult {
                value,
                diagnostics: Vec::new(),
            }
        }
        InterpolationMethod::NearestNeighbor => {
            let i = nearest_index_on_axis(grid_x, query.0);
            let j = nearest_index_on_axis(grid_y, query.1);
            let k = nearest_index_on_axis(grid_z, query.2);
            InterpolationResult {
                value: values[index_3d(i, j, k, grid_y.len(), grid_z.len())],
                diagnostics: Vec::new(),
            }
        }
        InterpolationMethod::Cubic => {
            let value = cubic_3d(grid_x, grid_y, grid_z, values, query);
            InterpolationResult {
                value,
                diagnostics: Vec::new(),
            }
        }
        InterpolationMethod::Rbf | InterpolationMethod::Kriging => {
            let (resolved, diag) = resolve_method(method);
            let mut result = interpolate_3d(resolved, grid_x, grid_y, grid_z, values, query);
            if let Some(d) = diag {
                result.diagnostics.push(d);
            }
            result
        }
    }
}

/// Trilinear kernel for a 3D grid. Reads only the eight corner values
/// bracketing the query cell and applies seven `lerp`s in the order
/// `z → y → x` (matching the natural separability identity asserted in the
/// test suite). O(1) per query — no allocations.
///
/// Out-of-range queries are handled by [`locate_cell_with_clamp`], which
/// returns clamped `t` values that collapse the corresponding axis lerp to
/// one of its endpoints — equivalent to the per-axis constant-extrapolation
/// policy of the 1D and 2D kernels.
fn linear_3d(
    grid_x: &[f64],
    grid_y: &[f64],
    grid_z: &[f64],
    values: &[f64],
    query: (f64, f64, f64),
) -> f64 {
    let (qx, qy, qz) = query;
    let ny = grid_y.len();
    let nz = grid_z.len();
    let (i, tx) = locate_cell_with_clamp(grid_x, qx);
    let (j, ty) = locate_cell_with_clamp(grid_y, qy);
    let (k, tz) = locate_cell_with_clamp(grid_z, qz);
    // Eight corner samples bracketing the query cell.
    let v000 = values[index_3d(i, j, k, ny, nz)];
    let v001 = values[index_3d(i, j, k + 1, ny, nz)];
    let v010 = values[index_3d(i, j + 1, k, ny, nz)];
    let v011 = values[index_3d(i, j + 1, k + 1, ny, nz)];
    let v100 = values[index_3d(i + 1, j, k, ny, nz)];
    let v101 = values[index_3d(i + 1, j, k + 1, ny, nz)];
    let v110 = values[index_3d(i + 1, j + 1, k, ny, nz)];
    let v111 = values[index_3d(i + 1, j + 1, k + 1, ny, nz)];
    // Collapse z, then y, then x.
    let v00 = lerp(v000, v001, tz);
    let v01 = lerp(v010, v011, tz);
    let v10 = lerp(v100, v101, tz);
    let v11 = lerp(v110, v111, tz);
    let v0 = lerp(v00, v01, ty);
    let v1 = lerp(v10, v11, ty);
    lerp(v0, v1, tx)
}

/// Tricubic kernel for a 3D grid, computed as the natural tensor product:
/// for each of the (≤4) x-indices in the bracketing 4×4×4 stencil, evaluate
/// the bicubic [`cubic_2d_kernel`] over the (y, z) slice at `(qy, qz)`; then
/// evaluate the 1D cubic along the x-axis over those intermediates. The
/// axis-collapse order is `z → y → x` (since `cubic_2d_kernel` collapses
/// its trailing axis first), matching the separability identity asserted in
/// the test suite. Reads at most 64 samples per query — no `Vec`
/// allocations.
fn cubic_3d(
    grid_x: &[f64],
    grid_y: &[f64],
    grid_z: &[f64],
    values: &[f64],
    query: (f64, f64, f64),
) -> f64 {
    let (qx, qy, qz) = query;
    let ny = grid_y.len();
    let nz = grid_z.len();
    let last_x = grid_x.len() - 1;

    // For a fixed x-index `i_idx`, collapse the (y, z) slice to a single
    // bicubic-interpolated value at (qy, qz). Reuses `cubic_2d_kernel` so
    // ghost-point and boundary-clamp behaviour matches the 2D entry point.
    let yz_collapse = |i_idx: usize| {
        cubic_2d_kernel(grid_y, grid_z, qy, qz, |j, k| {
            values[index_3d(i_idx, j, k, ny, nz)]
        })
    };

    if qx <= grid_x[0] {
        return yz_collapse(0);
    }
    if qx >= grid_x[last_x] {
        return yz_collapse(last_x);
    }
    let i = locate_cell(grid_x, qx).expect("in-range query bracketed");
    let span_x = grid_x[i + 1] - grid_x[i];
    if span_x <= 0.0 {
        return yz_collapse(i);
    }
    let tx = (qx - grid_x[i]) / span_x;

    let p1 = yz_collapse(i);
    let p2 = yz_collapse(i + 1);
    let p0 = if i == 0 {
        2.0 * p1 - p2
    } else {
        yz_collapse(i - 1)
    };
    let p3 = if i + 2 > last_x {
        2.0 * p2 - p1
    } else {
        yz_collapse(i + 2)
    };
    cubic4_eval(p0, p1, p2, p3, tx)
}

/// Locate a cell on a single axis with constant-extrapolation clamping.
/// Returns `(cell_index, t)` where `t ∈ [0, 1]` is the local cell parameter,
/// clamped to `0.0` or `1.0` for out-of-range queries. Returns `(0, f64::NAN)`
/// when `query` is NaN so that any downstream `lerp` propagates NaN rather
/// than silently clamping to a boundary value. The grid must have at least two
/// points.
fn locate_cell_with_clamp(grid: &[f64], query: f64) -> (usize, f64) {
    debug_assert!(grid.len() >= 2);
    if query.is_nan() {
        return (0, f64::NAN);
    }
    let last = grid.len() - 1;
    if query <= grid[0] {
        return (0, 0.0);
    }
    if query >= grid[last] {
        return (last - 1, 1.0);
    }
    let i = locate_cell(grid, query).expect("in-range query bracketed");
    let span = grid[i + 1] - grid[i];
    let t = if span > 0.0 {
        (query - grid[i]) / span
    } else {
        0.0
    };
    (i, t)
}
