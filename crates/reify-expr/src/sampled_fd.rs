//! Sampled finite-difference (FD) differential operators.
//!
//! This module provides [`sampled_differential`], a generic FD primitive that
//! computes gradient, laplacian, divergence, or curl from a uniformly-sampled
//! [`SampledField`] using central differences on the interior and first-order
//! one-sided differences at boundaries.
//!
//! # Precedent
//!
//! The central/one-sided scheme is copied and generalised from
//! `reify-shell-extract::medial::gradient_at_index` (line 666), which operates
//! on the same `SampledField` type with a hard-coded 3-D layout.  δ extends
//! that to an n-axis helper parameterised by `(axis, stride, comp)` so that a
//! single code path serves all four operators.  There is no shared-code
//! dependency — `reify-shell-extract` is a different crate (PRD §8/§10).
//!
//! # Output data layout
//!
//! Vector outputs use **stride-N interleaved node-major** layout:
//! `out[g * out_stride + comp]`, identical to the multi-component convention
//! from `reify-solver-elastic::resample::resample_multi_nodal_to_grid` (e.g.
//! displacement stride-3, stress stride-9), so ζ's future `stride-n
//! sample_at_point` and the Phase-1 `sampled_*_field` channels can read δ
//! output with a single code path.
//!
//! # Per-operator stride table
//!
//! | Op          | in stride  | out stride       |
//! |-------------|------------|------------------|
//! | Gradient    | 1 (scalar) | n (axis count)   |
//! | Laplacian   | 1 (scalar) | 1 (scalar)       |
//! | Divergence  | n (vector) | 1 (scalar)       |
//! | Curl        | 3 (vec)    | 3 (vec, 3-D only)|
//!
//! # Numeric contract (PRD §6)
//!
//! * Gradient/Divergence/Curl on **affine** inputs: exact to < 1e-12 at every
//!   node (central + first-order one-sided; both are algebraically exact for
//!   degree-1 polynomials on a uniform grid).
//! * Laplacian on **quadratic** inputs: exact to < 1e-12 at every node,
//!   including boundaries (one-sided 3-point second difference equals `2a`
//!   for `ax²`).
//! * Smooth non-polynomial (e.g. `sin(x)`): interior central-difference error
//!   is O(h²) — halving spacing reduces it ≥ 3× (convergence-rate assertion).

use std::sync::atomic::AtomicBool;

use reify_ir::{SampledField, SampledGridKind};

/// Finite-difference operation to apply to a [`SampledField`].
///
/// See [`sampled_differential`] for operator semantics, stride contracts, and
/// numeric exactness guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DifferentialOp {
    /// ∂f/∂x_c for each axis c.
    /// Input: scalar (stride 1).  Output: vector (stride = axis count n).
    Gradient,
    /// Σ_c ∂F_c/∂x_c.
    /// Input: vector (stride = axis count n).  Output: scalar (stride 1).
    Divergence,
    /// ∇×F (Regular3D + stride-3 only).
    /// Input: vector stride 3.  Output: vector stride 3.
    /// Non-Regular3D or non-stride-3 input returns a defined zero-filled field.
    Curl,
    /// Σ_c ∂²f/∂x_c².
    /// Input: scalar (stride 1).  Output: scalar (stride 1).
    Laplacian,
}

/// Compute a finite-difference differential operator on a uniformly-sampled
/// field.
///
/// Returns a new `SampledField` sharing the input's physical grid (same
/// `kind`, `bounds_min`, `bounds_max`, `spacing`, `axis_grids`,
/// `interpolation`) but with a freshly-computed data buffer and a reset
/// `oob_emitted` flag.
///
/// # Boundary treatment
///
/// First derivatives use first-order one-sided differences at the two boundary
/// nodes of each axis (mirrors `medial::gradient_at_index` exactly).
/// Second derivatives (Laplacian) use the nearest-interior 3-point one-sided
/// second difference at boundaries: `(f[0] - 2·f[1] + f[2]) / h²` (lower) /
/// `(f[n-1] - 2·f[n-2] + f[n-3]) / h²` (upper), which reproduces `2a` for
/// `f = ax²` on any uniform grid.
///
/// Under-resolved axes contribute zero: < 2 nodes for 1st-derivative ops;
/// < 3 nodes for the Laplacian.  Higher-order boundary treatment is deferred
/// to η per PRD §10.
pub(crate) fn sampled_differential(sf: &SampledField, op: DifferentialOp) -> SampledField {
    todo!("sampled_differential: not yet implemented — see plan step-2/4/6/8")
}

// ─── grid helpers ────────────────────────────────────────────────────────────

/// Returns per-axis node counts from `axis_grids`.
fn axis_dims(sf: &SampledField) -> Vec<usize> {
    sf.axis_grids.iter().map(|g| g.len()).collect()
}

/// Row-major flat index from a multi-index `mi` over dimensions `dims`.
/// Axis 0 is the outermost (slowest-varying) index.
///
/// Matches `SampledField::data` layout: for 3-D [i, j, k] with dims [nx, ny, nz]
/// `flat_index = i * ny * nz + j * nz + k`.
fn flat_index(mi: &[usize], dims: &[usize]) -> usize {
    let mut idx = 0usize;
    let mut stride = 1usize;
    for axis in (0..dims.len()).rev() {
        idx += mi[axis] * stride;
        stride *= dims[axis];
    }
    idx
}

/// Decode a flat grid index `g` back into a multi-index over `dims`.
fn decode_index(g: usize, dims: &[usize]) -> Vec<usize> {
    let n = dims.len();
    let mut mi = vec![0usize; n];
    let mut remaining = g;
    for axis in (0..n).rev() {
        mi[axis] = remaining % dims[axis];
        remaining /= dims[axis];
    }
    mi
}

/// `flat_index` with one axis overridden to `new_val`.
fn flat_index_with(mi: &[usize], dims: &[usize], axis: usize, new_val: usize) -> usize {
    let mut mi2 = mi.to_vec();
    mi2[axis] = new_val;
    flat_index(&mi2, dims)
}

// ─── finite-difference kernels ───────────────────────────────────────────────

/// Central/one-sided first difference along `axis`, reading component `comp`
/// from interleaved `data[node * stride + comp]`.
///
/// Returns 0.0 when `dims[axis] < 2` (mirrors `medial::gradient_at_index` for
/// singleton axes).
fn first_diff_along_axis(
    data: &[f64],
    dims: &[usize],
    spacing: &[f64],
    mi: &[usize],
    axis: usize,
    stride: usize,
    comp: usize,
) -> f64 {
    let n = dims[axis];
    if n < 2 {
        return 0.0;
    }
    let h = spacing[axis];
    let i = mi[axis];

    if i == 0 {
        // forward (one-sided)
        let p0 = flat_index_with(mi, dims, axis, 0) * stride + comp;
        let p1 = flat_index_with(mi, dims, axis, 1) * stride + comp;
        (data[p1] - data[p0]) / h
    } else if i == n - 1 {
        // backward (one-sided)
        let pm = flat_index_with(mi, dims, axis, n - 2) * stride + comp;
        let pp = flat_index_with(mi, dims, axis, n - 1) * stride + comp;
        (data[pp] - data[pm]) / h
    } else {
        // central
        let pm = flat_index_with(mi, dims, axis, i - 1) * stride + comp;
        let pp = flat_index_with(mi, dims, axis, i + 1) * stride + comp;
        (data[pp] - data[pm]) / (2.0 * h)
    }
}

/// Central/one-sided second difference along `axis`, reading component `comp`
/// from interleaved `data[node * stride + comp]`.
///
/// Returns 0.0 when `dims[axis] < 3`.
///
/// Boundary: one-sided 3-point form `(f[0] - 2·f[1] + f[2]) / h²` at the
/// lower boundary and `(f[n-1] - 2·f[n-2] + f[n-3]) / h²` at the upper
/// boundary.  For `f = ax²` this equals `2a` everywhere, making the Laplacian
/// exact on quadratic inputs (see PRD §6).  Higher-order boundary treatment
/// (ghost-node) is deferred to η per PRD §10.
fn second_diff_along_axis(
    data: &[f64],
    dims: &[usize],
    spacing: &[f64],
    mi: &[usize],
    axis: usize,
    stride: usize,
    comp: usize,
) -> f64 {
    let n = dims[axis];
    if n < 3 {
        return 0.0;
    }
    let h = spacing[axis];
    let h2 = h * h;
    let i = mi[axis];

    if i == 0 {
        // one-sided lower: (f[0] - 2·f[1] + f[2]) / h²
        let p0 = flat_index_with(mi, dims, axis, 0) * stride + comp;
        let p1 = flat_index_with(mi, dims, axis, 1) * stride + comp;
        let p2 = flat_index_with(mi, dims, axis, 2) * stride + comp;
        (data[p0] - 2.0 * data[p1] + data[p2]) / h2
    } else if i == n - 1 {
        // one-sided upper: (f[n-1] - 2·f[n-2] + f[n-3]) / h²
        let pa = flat_index_with(mi, dims, axis, n - 3) * stride + comp;
        let pb = flat_index_with(mi, dims, axis, n - 2) * stride + comp;
        let pc = flat_index_with(mi, dims, axis, n - 1) * stride + comp;
        (data[pc] - 2.0 * data[pb] + data[pa]) / h2
    } else {
        // central: (f[i+1] - 2·f[i] + f[i-1]) / h²
        let pm = flat_index_with(mi, dims, axis, i - 1) * stride + comp;
        let p0 = flat_index_with(mi, dims, axis, i) * stride + comp;
        let pp = flat_index_with(mi, dims, axis, i + 1) * stride + comp;
        (data[pp] - 2.0 * data[p0] + data[pm]) / h2
    }
}

// ─── output-field construction ───────────────────────────────────────────────

/// Clone the grid geometry from `sf` and substitute a new data buffer.
///
/// Reuses the projection pattern from `field_reductions::project_von_mises_sampled`:
/// clone `name/kind/bounds_min/bounds_max/spacing/axis_grids/interpolation`,
/// substitute `data`, fresh `oob_emitted: AtomicBool::new(false)`.
fn clone_geometry(sf: &SampledField, data: Vec<f64>) -> SampledField {
    SampledField {
        name: sf.name.clone(),
        kind: sf.kind,
        bounds_min: sf.bounds_min.clone(),
        bounds_max: sf.bounds_max.clone(),
        spacing: sf.spacing.clone(),
        axis_grids: sf.axis_grids.clone(),
        interpolation: sf.interpolation,
        data,
        oob_emitted: AtomicBool::new(false),
    }
}
