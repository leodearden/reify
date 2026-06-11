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
    let dims = axis_dims(sf);
    let n_axes = dims.len();
    let grid_count: usize = dims.iter().product();
    let in_stride = if grid_count > 0 { sf.data.len() / grid_count } else { 1 };

    match op {
        DifferentialOp::Gradient => {
            // Scalar input (stride 1) → vector output (stride = axis count n).
            debug_assert_eq!(in_stride, 1, "Gradient: expected scalar input (stride 1)");
            let out_stride = n_axes;
            let mut data = vec![0.0f64; grid_count * out_stride];
            for g in 0..grid_count {
                let mi = decode_index(g, &dims);
                for c in 0..n_axes {
                    data[g * out_stride + c] =
                        first_diff_along_axis(&sf.data, &dims, &sf.spacing, &mi, c, 1, 0);
                }
            }
            clone_geometry(sf, data)
        }
        DifferentialOp::Laplacian => {
            // Scalar input (stride 1) → scalar output (stride 1).
            // Boundary: one-sided 3-point second difference (f[0]-2f[1]+f[2])/h²
            // which equals 2a for f=ax², matching the quadratic exactness contract
            // (PRD §6). Higher-order boundary treatment deferred to η per PRD §10.
            debug_assert_eq!(in_stride, 1, "Laplacian: expected scalar input (stride 1)");
            let mut data = vec![0.0f64; grid_count];
            for g in 0..grid_count {
                let mi = decode_index(g, &dims);
                let mut lap = 0.0;
                for axis in 0..n_axes {
                    lap += second_diff_along_axis(&sf.data, &dims, &sf.spacing, &mi, axis, 1, 0);
                }
                data[g] = lap;
            }
            clone_geometry(sf, data)
        }
        DifferentialOp::Divergence => {
            todo!("sampled_differential Divergence: not yet implemented — see plan step-6")
        }
        DifferentialOp::Curl => {
            todo!("sampled_differential Curl: not yet implemented — see plan step-8")
        }
    }
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

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

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
        for i in 0..nx {
            for j in 0..ny {
                data.push(f(xs[i], ys[j]));
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
        for i in 0..nx {
            for j in 0..ny {
                for k in 0..nz {
                    data.push(f(xs[i], ys[j], zs[k]));
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
        for i in 0..nx {
            for j in 0..ny {
                let v = f(xs[i], ys[j]);
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
        for i in 0..nx {
            for j in 0..ny {
                for k in 0..nz {
                    let v = f(xs[i], ys[j], zs[k]);
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
}
