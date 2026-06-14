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
//! # Totality
//!
//! Every operator is **total**: when the input stride does not match the
//! operator's expectation (e.g. a vector field passed to `Gradient`), the
//! function returns a zero-filled degenerate field with the correct output
//! stride rather than panicking.  The ζ dispatch layer validates strides before
//! calling; the totality guarantee is a defence-in-depth backstop.
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

// All items in this module are pub(crate) producers for future consumers ε/ζ
// (calculus.rs dispatch) and are exercised only via in-crate unit tests until
// those tasks land.  Suppress dead-code lint for the whole module.
#![allow(dead_code)]

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
    /// Returns a zero-filled degenerate field if the input stride ≠ 1.
    Gradient,
    /// Σ_c ∂F_c/∂x_c.
    /// Input: vector (stride = axis count n).  Output: scalar (stride 1).
    /// Returns a zero-filled degenerate field if the input stride ≠ axis count.
    Divergence,
    /// ∇×F (Regular3D + stride-3 only).
    /// Input: vector stride 3.  Output: vector stride 3.
    /// Non-Regular3D or non-stride-3 input returns a defined zero-filled field.
    Curl,
    /// Σ_c ∂²f/∂x_c².
    /// Input: scalar (stride 1).  Output: scalar (stride 1).
    /// Returns a zero-filled degenerate field if the input stride ≠ 1.
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
/// # Stride contract and totality
///
/// Each operator expects a specific input stride (see the per-operator stride
/// table in the module doc).  When the contract is violated the operator
/// returns a zero-filled degenerate `SampledField` with the output stride
/// appropriate for the requested operator — it never panics.  The ζ dispatch
/// layer validates strides before calling; this totality is a
/// defence-in-depth backstop.
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
///
/// # Performance
///
/// The hot-path loop maintains a running multi-index (odometer increment via
/// [`increment_mi`]) and addresses neighbours via precomputed per-axis grid
/// strides ([`compute_axis_strides`]), avoiding all per-node heap allocations.
pub(crate) fn sampled_differential(sf: &SampledField, op: DifferentialOp) -> SampledField {
    let dims = axis_dims(sf);
    let n_axes = dims.len();
    let grid_count: usize = dims.iter().product();
    let in_stride = sf.data.len().checked_div(grid_count).unwrap_or(1);

    match op {
        DifferentialOp::Gradient => {
            // Scalar input (stride 1) → vector output (stride = axis count n).
            // Totality: return zero-filled degenerate field on stride mismatch.
            let out_stride = n_axes;
            if in_stride != 1 {
                return clone_geometry(sf, vec![0.0f64; grid_count * out_stride]);
            }
            let ax_strides = compute_axis_strides(&dims);
            let mut mi = vec![0usize; n_axes];
            let mut data = vec![0.0f64; grid_count * out_stride];
            for g in 0..grid_count {
                for c in 0..n_axes {
                    data[g * out_stride + c] = first_diff_flat(
                        &sf.data,
                        dims[c],
                        sf.spacing[c],
                        mi[c],
                        g,
                        ax_strides[c],
                        1,
                        0,
                    );
                }
                increment_mi(&mut mi, &dims);
            }
            clone_geometry(sf, data)
        }
        DifferentialOp::Laplacian => {
            // Scalar input (stride 1) → scalar output (stride 1).
            // Boundary: one-sided 3-point second difference (f[0]-2f[1]+f[2])/h²
            // which equals 2a for f=ax², matching the quadratic exactness contract
            // (PRD §6). Higher-order boundary treatment deferred to η per PRD §10.
            // Totality: return zero-filled degenerate field on stride mismatch.
            if in_stride != 1 {
                return clone_geometry(sf, vec![0.0f64; grid_count]);
            }
            let ax_strides = compute_axis_strides(&dims);
            let mut mi = vec![0usize; n_axes];
            let mut data = vec![0.0f64; grid_count];
            for (g, slot) in data.iter_mut().enumerate() {
                let mut lap = 0.0;
                for axis in 0..n_axes {
                    lap += second_diff_flat(
                        &sf.data,
                        dims[axis],
                        sf.spacing[axis],
                        mi[axis],
                        g,
                        ax_strides[axis],
                        1,
                        0,
                    );
                }
                *slot = lap;
                increment_mi(&mut mi, &dims);
            }
            clone_geometry(sf, data)
        }
        DifferentialOp::Divergence => {
            // Vector input (stride = axis count n) → scalar output (stride 1).
            // out[g] = Σ_c ∂F_c/∂x_c, where F_c is extracted from interleaved
            // input via first_diff_flat(..., data_stride=in_stride, comp=c).
            // Totality: return zero-filled degenerate field on stride mismatch.
            if in_stride != n_axes {
                return clone_geometry(sf, vec![0.0f64; grid_count]);
            }
            let ax_strides = compute_axis_strides(&dims);
            let mut mi = vec![0usize; n_axes];
            let mut data = vec![0.0f64; grid_count];
            for (g, slot) in data.iter_mut().enumerate() {
                let mut div = 0.0;
                for c in 0..n_axes {
                    div += first_diff_flat(
                        &sf.data,
                        dims[c],
                        sf.spacing[c],
                        mi[c],
                        g,
                        ax_strides[c],
                        in_stride,
                        c,
                    );
                }
                *slot = div;
                increment_mi(&mut mi, &dims);
            }
            clone_geometry(sf, data)
        }
        DifferentialOp::Curl => {
            // Curl is only defined for Regular3D + stride-3 vector fields.
            // For other inputs return a defined degenerate field (all-zero, stride 3)
            // rather than panicking — the primitive is total (PRD §5/design §5).
            if sf.kind != SampledGridKind::Regular3D || in_stride != 3 {
                // Degenerate: non-Regular3D or wrong component stride.
                // Return a zero-filled stride-3 field — the primitive is total
                // (PRD §5/design §5); callers must not get a panic here.
                // ζ will call Curl only on validated Regular3D + stride-3 inputs;
                // any other caller is a caller-side contract violation surfaced
                // at the ζ dispatch layer, not here.
                return clone_geometry(sf, vec![0.0f64; grid_count * 3]);
            }

            // curl = ∇×F with components:
            //   curl_x = ∂F_z/∂y  − ∂F_y/∂z  = ∂F2/∂x1 − ∂F1/∂x2
            //   curl_y = ∂F_x/∂z  − ∂F_z/∂x  = ∂F0/∂x2 − ∂F2/∂x0
            //   curl_z = ∂F_y/∂x  − ∂F_x/∂y  = ∂F1/∂x0 − ∂F0/∂x1
            //
            // Each partial ∂F_c/∂x_a is computed by first_diff_flat with
            // axis=a, data_stride=3, comp=c.  Output is interleaved node-major: stride 3.
            let ax_strides = compute_axis_strides(&dims);
            let mut mi = vec![0usize; 3]; // Regular3D has exactly 3 axes
            let mut data = vec![0.0f64; grid_count * 3];
            for g in 0..grid_count {
                // axis indices: 0=x, 1=y, 2=z; component indices: 0=F_x, 1=F_y, 2=F_z
                // curl_x = ∂F2/∂x1 − ∂F1/∂x2
                data[g * 3] =
                    first_diff_flat(&sf.data, dims[1], sf.spacing[1], mi[1], g, ax_strides[1], 3, 2)
                        - first_diff_flat(
                            &sf.data, dims[2], sf.spacing[2], mi[2], g, ax_strides[2], 3, 1,
                        );
                // curl_y = ∂F0/∂x2 − ∂F2/∂x0
                data[g * 3 + 1] =
                    first_diff_flat(&sf.data, dims[2], sf.spacing[2], mi[2], g, ax_strides[2], 3, 0)
                        - first_diff_flat(
                            &sf.data, dims[0], sf.spacing[0], mi[0], g, ax_strides[0], 3, 2,
                        );
                // curl_z = ∂F1/∂x0 − ∂F0/∂x1
                data[g * 3 + 2] =
                    first_diff_flat(&sf.data, dims[0], sf.spacing[0], mi[0], g, ax_strides[0], 3, 1)
                        - first_diff_flat(
                            &sf.data, dims[1], sf.spacing[1], mi[1], g, ax_strides[1], 3, 0,
                        );
                increment_mi(&mut mi, &dims);
            }
            clone_geometry(sf, data)
        }
    }
}

// ─── grid helpers ────────────────────────────────────────────────────────────

/// Returns per-axis node counts from `axis_grids`.
fn axis_dims(sf: &SampledField) -> Vec<usize> {
    sf.axis_grids.iter().map(|g| g.len()).collect()
}

/// Precompute per-axis row-major grid strides.
///
/// `axis_strides[a] = ∏_{b > a} dims[b]`.  Combined with the flat node index
/// `g`, a neighbour at `mi[a] ± 1` on axis `a` sits at flat index
/// `g ± axis_strides[a]`.  This lets the FD kernels address neighbours without
/// rebuilding a multi-index Vec on every call.
fn compute_axis_strides(dims: &[usize]) -> Vec<usize> {
    let n = dims.len();
    if n == 0 {
        return vec![];
    }
    let mut strides = vec![1usize; n];
    for axis in (0..n - 1).rev() {
        strides[axis] = strides[axis + 1] * dims[axis + 1];
    }
    strides
}

/// Advance a row-major running multi-index by one step (odometer-style).
///
/// Axis 0 is outermost (slowest-varying).  Overflow past the last grid node
/// wraps all components to zero, consistent with the next iteration of the
/// outer `for g in 0..grid_count` loop.
fn increment_mi(mi: &mut [usize], dims: &[usize]) {
    for axis in (0..dims.len()).rev() {
        mi[axis] += 1;
        if mi[axis] < dims[axis] {
            return;
        }
        mi[axis] = 0;
    }
}

/// Row-major flat index from a multi-index `mi` over dimensions `dims`.
/// Axis 0 is the outermost (slowest-varying) index.
///
/// Matches `SampledField::data` layout: for 3-D [i, j, k] with dims [nx, ny, nz]
/// `flat_index = i * ny * nz + j * nz + k`.
///
/// Kept as a reference implementation; the hot-path kernels use
/// [`compute_axis_strides`] + direct flat arithmetic instead.
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
///
/// Kept as a reference implementation; the hot-path maintains a running
/// multi-index via [`increment_mi`] instead.
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

/// `flat_index` with one axis component overridden to `new_val`.
///
/// Kept as a reference implementation; the hot-path kernels address neighbours
/// by adding/subtracting a precomputed `axis_stride` from the flat node index
/// instead.
fn flat_index_with(mi: &[usize], dims: &[usize], axis: usize, new_val: usize) -> usize {
    let mut mi2 = mi.to_vec();
    mi2[axis] = new_val;
    flat_index(&mi2, dims)
}

// ─── finite-difference kernels ───────────────────────────────────────────────

/// Central/one-sided first difference along `axis` — **allocation-free
/// hot-path version** using precomputed flat addressing.
///
/// `g` is the flat node index, `axis_stride` is `compute_axis_strides()[axis]`
/// (the row-major grid stride for this axis), `mi_axis` is the current
/// position of the node on this axis (from the running multi-index).
/// `data_stride` and `comp` index the component in an interleaved buffer:
/// `data[node * data_stride + comp]`.
///
/// Returns 0.0 when `n < 2` (mirrors `medial::gradient_at_index` for
/// singleton axes).
#[allow(clippy::too_many_arguments)]
fn first_diff_flat(
    data: &[f64],
    n: usize,
    h: f64,
    mi_axis: usize,
    g: usize,
    axis_stride: usize,
    data_stride: usize,
    comp: usize,
) -> f64 {
    if n < 2 {
        return 0.0;
    }
    if mi_axis == 0 {
        // forward one-sided
        (data[(g + axis_stride) * data_stride + comp] - data[g * data_stride + comp]) / h
    } else if mi_axis == n - 1 {
        // backward one-sided
        (data[g * data_stride + comp] - data[(g - axis_stride) * data_stride + comp]) / h
    } else {
        // central
        (data[(g + axis_stride) * data_stride + comp]
            - data[(g - axis_stride) * data_stride + comp])
            / (2.0 * h)
    }
}

/// Central/one-sided second difference along `axis` — **allocation-free
/// hot-path version** using precomputed flat addressing.
///
/// Returns 0.0 when `n < 3`.
///
/// Boundary: one-sided 3-point form `(f[0] - 2·f[1] + f[2]) / h²` at the
/// lower boundary and `(f[n-1] - 2·f[n-2] + f[n-3]) / h²` at the upper
/// boundary, which equals `2a` for `f = ax²` (Laplacian exactness per PRD §6).
#[allow(clippy::too_many_arguments)]
fn second_diff_flat(
    data: &[f64],
    n: usize,
    h: f64,
    mi_axis: usize,
    g: usize,
    axis_stride: usize,
    data_stride: usize,
    comp: usize,
) -> f64 {
    if n < 3 {
        return 0.0;
    }
    let h2 = h * h;
    if mi_axis == 0 {
        // one-sided lower: (f[0] - 2·f[1] + f[2]) / h²
        let p0 = g * data_stride + comp;
        let p1 = (g + axis_stride) * data_stride + comp;
        let p2 = (g + 2 * axis_stride) * data_stride + comp;
        (data[p0] - 2.0 * data[p1] + data[p2]) / h2
    } else if mi_axis == n - 1 {
        // one-sided upper: (f[n-1] - 2·f[n-2] + f[n-3]) / h²
        let pc = g * data_stride + comp;
        let pb = (g - axis_stride) * data_stride + comp;
        let pa = (g - 2 * axis_stride) * data_stride + comp;
        (data[pc] - 2.0 * data[pb] + data[pa]) / h2
    } else {
        // central: (f[i+1] - 2·f[i] + f[i-1]) / h²
        let pm = (g - axis_stride) * data_stride + comp;
        let p0 = g * data_stride + comp;
        let pp = (g + axis_stride) * data_stride + comp;
        (data[pp] - 2.0 * data[p0] + data[pm]) / h2
    }
}

/// Central/one-sided first difference along `axis`, reading component `comp`
/// from interleaved `data[node * stride + comp]`.
///
/// **Reference implementation** using multi-index Vec addressing — kept for
/// documentation.  The hot-path uses [`first_diff_flat`] instead.
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
/// **Reference implementation** using multi-index Vec addressing — kept for
/// documentation.  The hot-path uses [`second_diff_flat`] instead.
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

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use reify_ir::{InterpolationKind, SampledField, SampledGridKind};

    use super::{DifferentialOp, sampled_differential};

    // ── fixture builders ─────────────────────────────────────────────────────
    // Mirror the medial.rs:991/1041 fixture-builder style: construct SampledField
    // literal with oob_emitted: AtomicBool::new(false).

    /// Build a uniform Regular1D scalar field over `n` nodes with spacing `h`,
    /// where `data[i] = f(x_i)` and `x_i = i as f64 * h`.
    fn make_1d_scalar(n: usize, h: f64, f: impl Fn(f64) -> f64) -> SampledField {
        let axis: Vec<f64> = (0..n).map(|i| i as f64 * h).collect();
        let data: Vec<f64> = axis.iter().map(|&x| f(x)).collect();
        SampledField {
            name: "test-1d".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![(n - 1) as f64 * h],
            spacing: vec![h],
            axis_grids: vec![axis],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Build a uniform Regular2D scalar field over `nx × ny` nodes with spacing
    /// `hx` / `hy`, where `data[i*ny + j] = f(x_i, y_j)`.
    fn make_2d_scalar(
        nx: usize,
        ny: usize,
        hx: f64,
        hy: f64,
        f: impl Fn(f64, f64) -> f64,
    ) -> SampledField {
        let xs: Vec<f64> = (0..nx).map(|i| i as f64 * hx).collect();
        let ys: Vec<f64> = (0..ny).map(|j| j as f64 * hy).collect();
        let mut data = Vec::with_capacity(nx * ny);
        for &x in &xs {
            for &y in &ys {
                data.push(f(x, y));
            }
        }
        SampledField {
            name: "test-2d".to_string(),
            kind: SampledGridKind::Regular2D,
            bounds_min: vec![0.0, 0.0],
            bounds_max: vec![(nx - 1) as f64 * hx, (ny - 1) as f64 * hy],
            spacing: vec![hx, hy],
            axis_grids: vec![xs, ys],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Build a uniform Regular3D scalar field over `nx × ny × nz` nodes with
    /// uniform spacing `h`, where `data[i*ny*nz + j*nz + k] = f(x,y,z)`.
    fn make_3d_scalar(
        nx: usize,
        ny: usize,
        nz: usize,
        h: f64,
        f: impl Fn(f64, f64, f64) -> f64,
    ) -> SampledField {
        let xs: Vec<f64> = (0..nx).map(|i| i as f64 * h).collect();
        let ys: Vec<f64> = (0..ny).map(|j| j as f64 * h).collect();
        let zs: Vec<f64> = (0..nz).map(|k| k as f64 * h).collect();
        let mut data = Vec::with_capacity(nx * ny * nz);
        for &x in &xs {
            for &y in &ys {
                for &z in &zs {
                    data.push(f(x, y, z));
                }
            }
        }
        SampledField {
            name: "test-3d".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![(nx - 1) as f64 * h, (ny - 1) as f64 * h, (nz - 1) as f64 * h],
            spacing: vec![h, h, h],
            axis_grids: vec![xs, ys, zs],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    // ── step-1: gradient of an affine scalar field is exact ──────────────────

    /// 1D gradient: f(x) = 2x + 3 ⟹ ∂f/∂x = 2 everywhere.
    /// Central and first-order one-sided first differences are both
    /// algebraically exact for affine functions on a uniform grid.
    ///
    /// Also verifies that `clone_geometry` preserves `name` and resets
    /// `oob_emitted` (suggestion 4 — single test locks the full constructor
    /// contract).
    #[test]
    fn gradient_1d_affine_exact() {
        let sf = make_1d_scalar(5, 1.0, |x| 2.0 * x + 3.0);
        let out = sampled_differential(&sf, DifferentialOp::Gradient);

        // Output shape: out_stride = 1 (axis count), data.len() == grid_count * 1
        assert_eq!(out.data.len(), 5, "data.len() == grid_count * n_axes");

        // Grid geometry preserved bit-for-bit
        assert_eq!(out.kind, SampledGridKind::Regular1D);
        assert_eq!(out.axis_grids, sf.axis_grids);
        assert_eq!(out.spacing, sf.spacing);
        assert_eq!(out.bounds_min, sf.bounds_min);
        assert_eq!(out.bounds_max, sf.bounds_max);
        assert_eq!(out.interpolation, sf.interpolation);

        // clone_geometry contract: name preserved, oob_emitted reset (suggestion 4)
        assert_eq!(out.name, sf.name, "clone_geometry must preserve name");
        assert!(
            !out.oob_emitted.load(Ordering::Relaxed),
            "clone_geometry must reset oob_emitted to false"
        );

        // Gradient exact at every node
        for (g, &val) in out.data.iter().enumerate() {
            assert!(
                (val - 2.0).abs() < 1e-12,
                "node {g}: gradient = {val}, expected 2.0"
            );
        }
    }

    /// 2D gradient: f(x,y) = 3x + 5y + 1 ⟹ ∇f = (3, 5) everywhere.
    #[test]
    fn gradient_2d_affine_exact() {
        let nx = 4;
        let ny = 3;
        let sf = make_2d_scalar(nx, ny, 0.5, 0.5, |x, y| 3.0 * x + 5.0 * y + 1.0);
        let out = sampled_differential(&sf, DifferentialOp::Gradient);

        // out_stride = 2 (axis count)
        let grid_count = nx * ny;
        assert_eq!(out.data.len(), grid_count * 2);

        // Geometry preserved
        assert_eq!(out.kind, SampledGridKind::Regular2D);
        assert_eq!(out.axis_grids, sf.axis_grids);
        assert_eq!(out.spacing, sf.spacing);

        for g in 0..grid_count {
            let gx = out.data[g * 2];
            let gy = out.data[g * 2 + 1];
            assert!(
                (gx - 3.0).abs() < 1e-12,
                "node {g}: grad_x = {gx}, expected 3.0"
            );
            assert!(
                (gy - 5.0).abs() < 1e-12,
                "node {g}: grad_y = {gy}, expected 5.0"
            );
        }
    }

    /// 3D gradient: f(x,y,z) = 2x + 3y + 4z + 1 ⟹ ∇f = (2, 3, 4) everywhere.
    #[test]
    fn gradient_3d_affine_exact() {
        let n = 4;
        let sf = make_3d_scalar(n, n, n, 1.0, |x, y, z| 2.0 * x + 3.0 * y + 4.0 * z + 1.0);
        let out = sampled_differential(&sf, DifferentialOp::Gradient);

        let grid_count = n * n * n;
        // out_stride = 3 (axis count)
        assert_eq!(out.data.len(), grid_count * 3);
        assert_eq!(out.kind, SampledGridKind::Regular3D);
        assert_eq!(out.axis_grids, sf.axis_grids);

        for g in 0..grid_count {
            let gx = out.data[g * 3];
            let gy = out.data[g * 3 + 1];
            let gz = out.data[g * 3 + 2];
            assert!((gx - 2.0).abs() < 1e-12, "node {g}: grad_x = {gx}");
            assert!((gy - 3.0).abs() < 1e-12, "node {g}: grad_y = {gy}");
            assert!((gz - 4.0).abs() < 1e-12, "node {g}: grad_z = {gz}");
        }
    }

    /// Build a Regular2D vector field with in_stride=2 over `nx × ny` nodes.
    /// `f(x, y)` returns `[F_x, F_y]`; data is node-major interleaved:
    /// `data[g*2 + c]` where `g = i*ny + j`.
    fn make_2d_vector(
        nx: usize,
        ny: usize,
        hx: f64,
        hy: f64,
        f: impl Fn(f64, f64) -> [f64; 2],
    ) -> SampledField {
        let xs: Vec<f64> = (0..nx).map(|i| i as f64 * hx).collect();
        let ys: Vec<f64> = (0..ny).map(|j| j as f64 * hy).collect();
        let mut data = Vec::with_capacity(nx * ny * 2);
        for &x in &xs {
            for &y in &ys {
                let v = f(x, y);
                data.push(v[0]);
                data.push(v[1]);
            }
        }
        SampledField {
            name: "test-2d-vec".to_string(),
            kind: SampledGridKind::Regular2D,
            bounds_min: vec![0.0, 0.0],
            bounds_max: vec![(nx - 1) as f64 * hx, (ny - 1) as f64 * hy],
            spacing: vec![hx, hy],
            axis_grids: vec![xs, ys],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Build a Regular3D vector field with in_stride=3 over `nx × ny × nz` nodes.
    /// `f(x, y, z)` returns `[F_x, F_y, F_z]`; data is node-major interleaved.
    fn make_3d_vector(
        nx: usize,
        ny: usize,
        nz: usize,
        h: f64,
        f: impl Fn(f64, f64, f64) -> [f64; 3],
    ) -> SampledField {
        let xs: Vec<f64> = (0..nx).map(|i| i as f64 * h).collect();
        let ys: Vec<f64> = (0..ny).map(|j| j as f64 * h).collect();
        let zs: Vec<f64> = (0..nz).map(|k| k as f64 * h).collect();
        let mut data = Vec::with_capacity(nx * ny * nz * 3);
        for &x in &xs {
            for &y in &ys {
                for &z in &zs {
                    let v = f(x, y, z);
                    data.push(v[0]);
                    data.push(v[1]);
                    data.push(v[2]);
                }
            }
        }
        SampledField {
            name: "test-3d-vec".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![(nx - 1) as f64 * h, (ny - 1) as f64 * h, (nz - 1) as f64 * h],
            spacing: vec![h, h, h],
            axis_grids: vec![xs, ys, zs],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    // ── step-3: laplacian of a quadratic scalar field is exact ───────────────

    /// 1D laplacian: f(x) = x² ⟹ ∇²f = 2 everywhere.
    /// Central second difference is exact for quadratics; one-sided 3-point
    /// form (f[0]-2f[1]+f[2])/h² also equals 2 for f=x², so all nodes pass.
    #[test]
    fn laplacian_1d_quadratic_exact() {
        // ≥4 nodes so we exercise both interior and boundary nodes
        let sf = make_1d_scalar(5, 1.0, |x| x * x);
        let out = sampled_differential(&sf, DifferentialOp::Laplacian);

        // out_stride = 1 (scalar), data.len() == grid_count
        assert_eq!(out.data.len(), 5);

        // Grid geometry preserved bit-for-bit
        assert_eq!(out.kind, SampledGridKind::Regular1D);
        assert_eq!(out.axis_grids, sf.axis_grids);
        assert_eq!(out.spacing, sf.spacing);
        assert_eq!(out.bounds_min, sf.bounds_min);
        assert_eq!(out.bounds_max, sf.bounds_max);

        for (g, &val) in out.data.iter().enumerate() {
            assert!(
                (val - 2.0).abs() < 1e-12,
                "node {g}: laplacian = {val}, expected 2.0"
            );
        }
    }

    /// 2D laplacian: f(x,y) = x² + 2y² ⟹ ∇²f = 2 + 4 = 6 everywhere,
    /// including all four boundary rows/columns.
    #[test]
    fn laplacian_2d_quadratic_exact() {
        let nx = 5;
        let ny = 4;
        let sf = make_2d_scalar(nx, ny, 0.5, 0.5, |x, y| x * x + 2.0 * y * y);
        let out = sampled_differential(&sf, DifferentialOp::Laplacian);

        let grid_count = nx * ny;
        assert_eq!(out.data.len(), grid_count);
        assert_eq!(out.kind, SampledGridKind::Regular2D);
        assert_eq!(out.axis_grids, sf.axis_grids);

        for (g, &val) in out.data.iter().enumerate() {
            assert!(
                (val - 6.0).abs() < 1e-12,
                "node {g}: laplacian = {val}, expected 6.0"
            );
        }
    }

    // ── step-5: divergence of an affine vector field is exact ────────────────

    /// 2D divergence: F = (2x, 3y) ⟹ div F = 2 + 3 = 5 everywhere.
    /// Input is stride-2 interleaved: data[g*2+0] = F_x, data[g*2+1] = F_y.
    #[test]
    fn divergence_2d_affine_exact() {
        let nx = 4;
        let ny = 3;
        let sf = make_2d_vector(nx, ny, 0.5, 0.5, |x, y| [2.0 * x, 3.0 * y]);
        let out = sampled_differential(&sf, DifferentialOp::Divergence);

        let grid_count = nx * ny;
        // out_stride = 1 (scalar)
        assert_eq!(out.data.len(), grid_count);

        // Grid geometry preserved
        assert_eq!(out.kind, SampledGridKind::Regular2D);
        assert_eq!(out.axis_grids, sf.axis_grids);
        assert_eq!(out.spacing, sf.spacing);
        assert_eq!(out.bounds_min, sf.bounds_min);
        assert_eq!(out.bounds_max, sf.bounds_max);

        for (g, &val) in out.data.iter().enumerate() {
            assert!(
                (val - 5.0).abs() < 1e-12,
                "node {g}: div = {val}, expected 5.0"
            );
        }
    }

    /// 3D divergence: F = (x, 2y, 3z) ⟹ div F = 1 + 2 + 3 = 6 everywhere.
    #[test]
    fn divergence_3d_affine_exact() {
        let n = 4;
        let sf = make_3d_vector(n, n, n, 1.0, |x, y, z| [x, 2.0 * y, 3.0 * z]);
        let out = sampled_differential(&sf, DifferentialOp::Divergence);

        let grid_count = n * n * n;
        assert_eq!(out.data.len(), grid_count);
        assert_eq!(out.kind, SampledGridKind::Regular3D);
        assert_eq!(out.axis_grids, sf.axis_grids);

        for (g, &val) in out.data.iter().enumerate() {
            assert!(
                (val - 6.0).abs() < 1e-12,
                "node {g}: div = {val}, expected 6.0"
            );
        }
    }

    // ── step-7: curl of an affine 3D vector field is exact ───────────────────

    /// Curl of F = (-y, x, 0) ⟹ curl F = (0, 0, 2) everywhere.
    /// Central and one-sided first differences are both exact for affine inputs.
    #[test]
    fn curl_3d_affine_exact() {
        let n = 4;
        // F_x = -y, F_y = x, F_z = 0
        let sf = make_3d_vector(n, n, n, 1.0, |x, y, _z| [-y, x, 0.0]);
        let out = sampled_differential(&sf, DifferentialOp::Curl);

        let grid_count = n * n * n;
        // out_stride = 3
        assert_eq!(out.data.len(), grid_count * 3);

        // Grid geometry preserved
        assert_eq!(out.kind, SampledGridKind::Regular3D);
        assert_eq!(out.axis_grids, sf.axis_grids);
        assert_eq!(out.spacing, sf.spacing);
        assert_eq!(out.bounds_min, sf.bounds_min);
        assert_eq!(out.bounds_max, sf.bounds_max);

        for g in 0..grid_count {
            let cx = out.data[g * 3];
            let cy = out.data[g * 3 + 1];
            let cz = out.data[g * 3 + 2];
            assert!((cx - 0.0).abs() < 1e-12, "node {g}: curl_x = {cx}, expected 0");
            assert!((cy - 0.0).abs() < 1e-12, "node {g}: curl_y = {cy}, expected 0");
            assert!((cz - 2.0).abs() < 1e-12, "node {g}: curl_z = {cz}, expected 2");
        }
    }

    /// Curl on a non-Regular3D input returns a defined degenerate field
    /// (out_stride=3, all-zero data) rather than panicking.
    /// Exercises the `kind != Regular3D` guard.
    #[test]
    fn curl_degenerate_non_3d_returns_zero() {
        // Use a 2D vector field — not Regular3D, so curl should return zeros
        let sf = make_2d_vector(4, 3, 1.0, 1.0, |x, y| [x, y]);
        // Pad data to stride-3 manually to avoid the in_stride != 3 guard
        // Actually, for a 2D field with stride-2 data, the in_stride check fires.
        // The non-Regular3D guard fires first (kind != Regular3D), so we get zeros.
        let out = sampled_differential(&sf, DifferentialOp::Curl);

        // Must return a SampledField (not panic), with out_stride=3, all zeros
        let grid_count = 4 * 3;
        assert_eq!(out.data.len(), grid_count * 3);
        assert!(out.data.iter().all(|&v| v == 0.0), "degenerate curl should be all-zero");
    }

    /// Curl on a Regular3D field with in_stride ≠ 3 returns a zero-filled
    /// stride-3 field rather than panicking.
    /// Exercises the `in_stride != 3` guard (the kind guard passes; stride fails).
    /// Regression for the second branch of the Curl degenerate path.
    #[test]
    fn curl_degenerate_regular3d_wrong_stride_returns_zero() {
        // make_3d_scalar produces a Regular3D field with in_stride=1 (scalar).
        // Curl expects in_stride=3, so the stride guard fires.
        let sf = make_3d_scalar(3, 3, 3, 1.0, |x, y, _z| x + y);
        let out = sampled_differential(&sf, DifferentialOp::Curl);

        let grid_count = 3 * 3 * 3;
        assert_eq!(
            out.data.len(),
            grid_count * 3,
            "degenerate curl must have stride-3 output"
        );
        assert!(
            out.data.iter().all(|&v| v == 0.0),
            "degenerate curl (wrong stride) must be all-zero"
        );
        // Grid geometry still preserved
        assert_eq!(out.kind, SampledGridKind::Regular3D);
        assert_eq!(out.axis_grids, sf.axis_grids);
    }

    // ── step-9: second-order convergence control + degenerate-axis handling ──

    /// (a) Convergence guard: for f(x) = sin(x) sampled on two grids over the
    /// same interval, halving the spacing should reduce the interior gradient
    /// error by ≥ 3× (O(h²) central-difference scheme).
    ///
    /// This test validates the O(h²) interior scheme — if the implementation
    /// accidentally used a first-order formula everywhere, the error ratio would
    /// be ~2×, not ≥ 3×.
    #[test]
    fn gradient_sin_convergence_rate() {
        // Coarse: 10 nodes over [0, π]
        let n_coarse = 10usize;
        let h_coarse = std::f64::consts::PI / (n_coarse - 1) as f64;
        let sf_coarse = make_1d_scalar(n_coarse, h_coarse, |x| x.sin());

        // Fine: 20 nodes over [0, π] (half spacing)
        let n_fine = 20usize;
        let h_fine = std::f64::consts::PI / (n_fine - 1) as f64;
        let sf_fine = make_1d_scalar(n_fine, h_fine, |x| x.sin());

        let grad_coarse = sampled_differential(&sf_coarse, DifferentialOp::Gradient);
        let grad_fine = sampled_differential(&sf_fine, DifferentialOp::Gradient);

        // Exact gradient: cos(x)
        // Measure max error on interior nodes only (boundary nodes use 1st-order
        // one-sided, giving only O(h) convergence; interior uses O(h²) central).
        let coarse_err: f64 = (1..n_coarse - 1)
            .map(|i| {
                let x = i as f64 * h_coarse;
                (grad_coarse.data[i] - x.cos()).abs()
            })
            .fold(0.0f64, f64::max);

        let fine_err: f64 = (1..n_fine - 1)
            .map(|i| {
                let x = i as f64 * h_fine;
                (grad_fine.data[i] - x.cos()).abs()
            })
            .fold(0.0f64, f64::max);

        // Ratio ≥ 3× confirms O(h²) convergence rate (actual ~4× for central diff)
        assert!(
            fine_err <= coarse_err / 3.0,
            "gradient convergence rate too low: coarse_err={coarse_err:.3e}, \
             fine_err={fine_err:.3e}, ratio={:.2} (expected ≥ 3×)",
            coarse_err / fine_err
        );
    }

    /// (b) Degenerate axis: a Regular2D field with a singleton first axis
    /// (axis_grids[0].len() == 1) yields gradient component 0 = 0 at every node.
    #[test]
    fn gradient_singleton_axis_yields_zero() {
        // Build a 1×4 2-D field (singleton first axis)
        let xs: Vec<f64> = vec![0.0]; // singleton
        let ys: Vec<f64> = (0..4).map(|j| j as f64 * 1.0).collect();
        let mut data = Vec::with_capacity(4);
        for y in &ys {
            data.push(3.0 * y + 1.0); // f = 3y + 1
        }
        let sf = SampledField {
            name: "test-singleton".to_string(),
            kind: SampledGridKind::Regular2D,
            bounds_min: vec![0.0, 0.0],
            bounds_max: vec![0.0, 3.0],
            spacing: vec![1.0, 1.0],
            axis_grids: vec![xs, ys],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        };
        let out = sampled_differential(&sf, DifferentialOp::Gradient);

        // out_stride = 2, grid_count = 4
        assert_eq!(out.data.len(), 8);
        for g in 0..4 {
            let gx = out.data[g * 2];     // ∂f/∂x along singleton axis → 0
            let gy = out.data[g * 2 + 1]; // ∂f/∂y = 3
            assert!(
                gx.abs() < 1e-12,
                "node {g}: grad_x = {gx}, expected 0 (singleton axis)"
            );
            assert!(
                (gy - 3.0).abs() < 1e-12,
                "node {g}: grad_y = {gy}, expected 3.0"
            );
        }
    }

    /// Laplacian with <3 nodes on an axis: that axis contributes 0.
    #[test]
    fn laplacian_under_resolved_axis_contributes_zero() {
        // 2×5 field: first axis has 2 nodes (< 3 → second_diff = 0 for that axis)
        // f = x² + y² → ∇²f = 2 + 2 = 4, but since axis-0 has only 2 nodes,
        // second_diff along axis 0 = 0, so only axis-1 contributes: ∇²f = 2
        let nx = 2;
        let ny = 5;
        let sf = make_2d_scalar(nx, ny, 1.0, 1.0, |x, y| x * x + y * y);
        let out = sampled_differential(&sf, DifferentialOp::Laplacian);

        assert_eq!(out.data.len(), nx * ny);
        for (g, &val) in out.data.iter().enumerate() {
            assert!(
                (val - 2.0).abs() < 1e-12,
                "node {g}: laplacian = {val}, expected 2.0 (under-resolved axis-0 → 0)"
            );
        }
    }
}
