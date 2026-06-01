//! Grid-based resampling of P1-tet nodal fields onto a regular 3D grid.
//!
//! Implements [`GridSpec`] and [`resample_nodal_to_grid`] — the primitives
//! used by the `elastic_static` and `buckling` trampolines to produce a
//! `Regular3D Sampled Value::Field` from the FEA nodal solution.
//!
//! # Design decisions
//!
//! - Grid resolution mirrors the solve mesh: `GridSpec.counts = (nx,ny,nz)`
//!   element counts → grid nodes = counts+1, spanning body bounds.
//! - Containment uses a [`TetSpatialIndex`] BVH — O(grid·log elems) per call.
//!   The index is built once per `resample_*` call (O(n·log n)) then queried
//!   O(log n) per grid point.  The instrumented entry points return
//!   [`ResampleStats`] with the exact `point_in_tet_p1` evaluation count for
//!   deterministic complexity assertions in tests.
//! - Out-of-solid grid points carry `f64::NAN` for all stride components.
//! - Data ordering: row-major axis-0(x) outermost → axis-2(z) innermost,
//!   with stride components contiguous per grid point.

use std::sync::atomic::AtomicBool;

use reify_ir::{InterpolationKind, SampledField, SampledGridKind};

use crate::interpolation::{TetSpatialIndex, barycentric_p1};

/// Per-call resample statistics returned by the instrumented entry points.
///
/// The `point_in_tet_tests` count is fully deterministic — independent of CPU
/// speed or optimization level — and is suitable for exact complexity assertions
/// in tests (e.g. asserting BVH ≥4× fewer evaluations than the linear scan on
/// the same mesh).
#[derive(Debug, Clone, Default)]
pub struct ResampleStats {
    /// Total number of `point_in_tet_p1` evaluations across all grid points
    /// during the BVH traversal.
    pub point_in_tet_tests: u64,
}

/// Element-count grid specification for a regular 3D axis-aligned grid.
///
/// `counts[i]` is the number of **element** intervals along axis i; the
/// grid has `counts[i]+1` nodes along axis i.  The physical extent of the
/// grid is `[bounds_min[i], bounds_max[i]]` with uniform spacing
/// `spacing[i] = (bounds_max[i] - bounds_min[i]) / counts[i]`.
#[derive(Debug, Clone, Copy)]
pub struct GridSpec {
    /// Lower bound of the grid along each axis (SI units).
    pub bounds_min: [f64; 3],
    /// Upper bound of the grid along each axis (SI units).
    pub bounds_max: [f64; 3],
    /// Number of element intervals along each axis (grid nodes = counts+1).
    pub counts: [usize; 3],
}

/// Resample a nodal field defined on a P1-tet mesh onto a regular 3D grid.
///
/// # Arguments
///
/// - `nodes`: node coordinates (length n_nodes), `nodes[n] = [x, y, z]`.
/// - `elems`: element connectivity (length n_elems), `elems[e] = [n0,n1,n2,n3]`.
/// - `nodal_values`: flat array of length `n_nodes × stride`.  Node `n`'s
///   values are `nodal_values[n*stride .. n*stride+stride]`.
/// - `stride`: number of scalar components per node (3 for displacement, 9 for stress).
/// - `grid`: specifies bounds, element counts (→ node counts = counts+1), and spacing.
/// - `name`: field name embedded in the returned [`SampledField`].
/// - `tol`: absolute barycentric tolerance for point-in-tet containment.
///   A value of `1e-9` accepts points within round-off of an element face.
///
/// # Returns
///
/// A [`SampledField`] with:
/// - `kind = Regular3D`
/// - `interpolation = Linear`
/// - `data.len() == (nx+1)*(ny+1)*(nz+1)*stride`
/// - Row-major ordering: axis-0 (x) outermost, axis-2 (z) innermost;
///   the `stride` components are contiguous per grid point.
/// - Grid points outside all elements carry `f64::NAN` for all `stride` components.
///
/// Prefer [`resample_multi_nodal_to_grid`] when sampling two or more fields
/// (displacement + stress) on the same geometry — it halves the point-location cost.
pub fn resample_nodal_to_grid(
    nodes: &[[f64; 3]],
    elems: &[[usize; 4]],
    nodal_values: &[f64],
    stride: usize,
    grid: &GridSpec,
    name: &str,
    tol: f64,
) -> SampledField {
    resample_nodal_to_grid_instrumented(nodes, elems, nodal_values, stride, grid, name, tol).0
}

/// Like [`resample_nodal_to_grid`] but also returns [`ResampleStats`] with
/// the `point_in_tet_p1` evaluation count.
///
/// Used in tests to assert O(grid·log elems) complexity via deterministic
/// count comparisons.  The public [`resample_nodal_to_grid`] is a thin
/// wrapper that calls this and discards the stats.
pub fn resample_nodal_to_grid_instrumented(
    nodes: &[[f64; 3]],
    elems: &[[usize; 4]],
    nodal_values: &[f64],
    stride: usize,
    grid: &GridSpec,
    name: &str,
    tol: f64,
) -> (SampledField, ResampleStats) {
    let [nx, ny, nz] = grid.counts;
    // Spacing per axis: (max-min)/count
    let sx = (grid.bounds_max[0] - grid.bounds_min[0]) / nx.max(1) as f64;
    let sy = (grid.bounds_max[1] - grid.bounds_min[1]) / ny.max(1) as f64;
    let sz = (grid.bounds_max[2] - grid.bounds_min[2]) / nz.max(1) as f64;
    let spacing = vec![sx, sy, sz];

    // Build per-axis grid coordinates via linspace_inclusive.
    let axis_grids: Vec<Vec<f64>> = (0..3)
        .map(|i| {
            let sp = spacing[i];
            reify_ir::sampled::linspace_inclusive(grid.bounds_min[i], grid.bounds_max[i], sp)
                .expect("resample_nodal_to_grid_instrumented: linspace_inclusive failed — check that bounds_min < bounds_max and counts > 0")
        })
        .collect();

    let nx1 = axis_grids[0].len();
    let ny1 = axis_grids[1].len();
    let nz1 = axis_grids[2].len();
    let n_grid = nx1 * ny1 * nz1;

    // Build the BVH once for all grid points — O(n·log n).
    let idx = TetSpatialIndex::build(nodes, elems, tol);

    let mut data = Vec::with_capacity(n_grid * stride);
    let mut total_tests: u64 = 0;

    // Row-major iteration: axis-0(x) outermost → axis-2(z) innermost.
    for ix in 0..nx1 {
        for iy in 0..ny1 {
            for iz in 0..nz1 {
                let p = [axis_grids[0][ix], axis_grids[1][iy], axis_grids[2][iz]];

                // BVH locate: returns (min-index containing element, point_in_tet_p1 count).
                let (elem_opt, tests) = idx.locate_counted(nodes, elems, p, tol);
                total_tests += tests as u64;

                match elem_opt {
                    Some(e) => {
                        let conn = &elems[e];
                        let phys4: [[f64; 3]; 4] = [
                            nodes[conn[0]],
                            nodes[conn[1]],
                            nodes[conn[2]],
                            nodes[conn[3]],
                        ];
                        // Recompute barycentric weights for the located element;
                        // same arithmetic as the original linear-scan path (bit-identical).
                        let bary = barycentric_p1(&phys4, p);
                        for c in 0..stride {
                            let val = bary[0] * nodal_values[conn[0] * stride + c]
                                + bary[1] * nodal_values[conn[1] * stride + c]
                                + bary[2] * nodal_values[conn[2] * stride + c]
                                + bary[3] * nodal_values[conn[3] * stride + c];
                            data.push(val);
                        }
                    }
                    None => {
                        // Out-of-solid sentinel: NaN per stride component.
                        for _ in 0..stride {
                            data.push(f64::NAN);
                        }
                    }
                }
            }
        }
    }

    let sf = SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: grid.bounds_min.to_vec(),
        bounds_max: grid.bounds_max.to_vec(),
        spacing,
        axis_grids,
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    };
    let stats = ResampleStats { point_in_tet_tests: total_tests };
    (sf, stats)
}

/// Resample **multiple** nodal fields onto the same Regular3D grid in a single
/// geometry pass.
///
/// Semantically equivalent to calling [`resample_nodal_to_grid`] once per entry
/// in `fields`, but the containing-tet + barycentric-weight computation is done
/// **once per grid point** instead of once per *(grid point × field)*.  For the
/// buckling trampoline (~13 k grid points × 61 k tets × 2 fields) this reduces
/// the O(grid·elems) point-location cost — the dominant non-CG step.
///
/// # Arguments
///
/// - `fields`: slice of `(&[f64], usize, &str)` tuples — each is
///   `(nodal_values, stride, name)` for one field.
///   `nodal_values` must have length `n_nodes × stride`.
///
/// All fields share the same `nodes`, `elems`, `grid`, and `tol`.
/// Returns one [`SampledField`] per input entry, in the same order.
pub fn resample_multi_nodal_to_grid(
    nodes: &[[f64; 3]],
    elems: &[[usize; 4]],
    fields: &[(&[f64], usize, &str)],
    grid: &GridSpec,
    tol: f64,
) -> Vec<SampledField> {
    resample_multi_nodal_to_grid_instrumented(nodes, elems, fields, grid, tol).0
}

/// Like [`resample_multi_nodal_to_grid`] but also returns [`ResampleStats`].
///
/// The BVH index is built once and shared across all fields; `point_in_tet_tests`
/// counts locate evaluations per grid point (independent of field count).
/// The public [`resample_multi_nodal_to_grid`] is a thin wrapper that calls
/// this and discards the stats.
pub fn resample_multi_nodal_to_grid_instrumented(
    nodes: &[[f64; 3]],
    elems: &[[usize; 4]],
    fields: &[(&[f64], usize, &str)],
    grid: &GridSpec,
    tol: f64,
) -> (Vec<SampledField>, ResampleStats) {
    let [nx, ny, nz] = grid.counts;
    let sx = (grid.bounds_max[0] - grid.bounds_min[0]) / nx.max(1) as f64;
    let sy = (grid.bounds_max[1] - grid.bounds_min[1]) / ny.max(1) as f64;
    let sz = (grid.bounds_max[2] - grid.bounds_min[2]) / nz.max(1) as f64;
    let spacing = vec![sx, sy, sz];

    let axis_grids: Vec<Vec<f64>> = (0..3)
        .map(|i| {
            let sp = spacing[i];
            reify_ir::sampled::linspace_inclusive(grid.bounds_min[i], grid.bounds_max[i], sp)
                .expect("resample_multi_nodal_to_grid_instrumented: linspace_inclusive failed — \
                         check that bounds_min < bounds_max and counts > 0")
        })
        .collect();

    let nx1 = axis_grids[0].len();
    let ny1 = axis_grids[1].len();
    let nz1 = axis_grids[2].len();
    let n_grid = nx1 * ny1 * nz1;

    // Build the BVH once — shared across all fields and all grid points.
    let idx = TetSpatialIndex::build(nodes, elems, tol);

    // Pre-allocate one output buffer per field.
    let mut data_bufs: Vec<Vec<f64>> = fields
        .iter()
        .map(|(_, stride, _)| Vec::with_capacity(n_grid * stride))
        .collect();

    let mut total_tests: u64 = 0;

    // Single geometry pass: locate once per grid point, apply to all fields.
    for ix in 0..nx1 {
        for iy in 0..ny1 {
            for iz in 0..nz1 {
                let p = [axis_grids[0][ix], axis_grids[1][iy], axis_grids[2][iz]];

                let (elem_opt, tests) = idx.locate_counted(nodes, elems, p, tol);
                total_tests += tests as u64;

                match elem_opt {
                    Some(e) => {
                        let conn = &elems[e];
                        let phys4: [[f64; 3]; 4] = [
                            nodes[conn[0]],
                            nodes[conn[1]],
                            nodes[conn[2]],
                            nodes[conn[3]],
                        ];
                        // Recompute barycentric weights; same arithmetic → bit-identical.
                        let bary = barycentric_p1(&phys4, p);
                        // Grid point is inside this tet — interpolate every field.
                        for (fi, (nodal_vals, stride, _)) in fields.iter().enumerate() {
                            for c in 0..*stride {
                                let val = bary[0] * nodal_vals[conn[0] * stride + c]
                                    + bary[1] * nodal_vals[conn[1] * stride + c]
                                    + bary[2] * nodal_vals[conn[2] * stride + c]
                                    + bary[3] * nodal_vals[conn[3] * stride + c];
                                data_bufs[fi].push(val);
                            }
                        }
                    }
                    None => {
                        // Out-of-solid sentinel: NaN for every stride component of every field.
                        for (fi, (_, stride, _)) in fields.iter().enumerate() {
                            for _ in 0..*stride {
                                data_bufs[fi].push(f64::NAN);
                            }
                        }
                    }
                }
            }
        }
    }

    // Assemble one SampledField per input field, sharing the same grid metadata.
    let sampled_fields: Vec<SampledField> = fields
        .iter()
        .zip(data_bufs)
        .map(|((_, _, name), data)| SampledField {
            name: name.to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: grid.bounds_min.to_vec(),
            bounds_max: grid.bounds_max.to_vec(),
            spacing: spacing.clone(),
            axis_grids: axis_grids.clone(),
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        })
        .collect();

    let stats = ResampleStats { point_in_tet_tests: total_tests };
    (sampled_fields, stats)
}

#[cfg(test)]
mod tests {
    use super::{GridSpec, resample_nodal_to_grid};
    use reify_ir::{InterpolationKind, SampledGridKind};

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Build a single tet (unit tetrahedron with one corner at origin).
    /// Connectivity: [0,1,2,3]; nodes: (0,0,0),(1,0,0),(0,1,0),(0,0,1).
    fn unit_tet() -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
        let nodes = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let elems = vec![[0usize, 1, 2, 3]];
        (nodes, elems)
    }

    // ── Test (a): stride-3 affine field recovers exactly at interior grid points ──

    /// Affine displacement field u(x) = A·x + b where
    ///   A = diag(2,3,4), b = (5,6,7)
    /// Nodal values at the unit tet corners.
    #[test]
    fn resample_stride3_affine_exact_interior() {
        let (nodes, elems) = unit_tet();

        // nodal_values for each node: u(node) = A·node + b
        let a = [2.0_f64, 3.0, 4.0];
        let b = [5.0_f64, 6.0, 7.0];
        let mut nodal_values = vec![0.0_f64; nodes.len() * 3];
        for (i, node) in nodes.iter().enumerate() {
            for c in 0..3 {
                nodal_values[i * 3 + c] = a[c] * node[c] + b[c];
            }
        }

        // Grid: 2 elements along each axis → 3 nodes each; spans the tet's
        // bounding box [0,1]×[0,1]×[0,1].  Interior grid points are at
        // (0.5, 0.0, 0.0), etc. — only the centroid (0.25,0.25,0.25) is
        // strictly inside the tet.
        let grid = GridSpec {
            bounds_min: [0.0, 0.0, 0.0],
            bounds_max: [1.0, 1.0, 1.0],
            counts: [2, 2, 2],
        };

        let sf = resample_nodal_to_grid(&nodes, &elems, &nodal_values, 3, &grid, "u", 1e-9);

        // (b): verify metadata
        assert_eq!(sf.kind, SampledGridKind::Regular3D);
        // Grid nodes = counts+1 = 3 per axis → 27 total; data = 27*3 = 81
        assert_eq!(sf.data.len(), 3 * 3 * 3 * 3, "data len");
        assert_eq!(sf.interpolation, InterpolationKind::Linear);

        // axis_grids: linspace(0,1, spacing=0.5) → [0.0, 0.5, 1.0]
        for ax in 0..3 {
            assert_eq!(sf.axis_grids[ax].len(), 3, "axis {ax} len");
            assert!((sf.axis_grids[ax][0] - 0.0).abs() < 1e-12);
            assert!((sf.axis_grids[ax][1] - 0.5).abs() < 1e-12);
            assert!((sf.axis_grids[ax][2] - 1.0).abs() < 1e-12);
        }

        // bounds_min/max
        assert_eq!(sf.bounds_min, vec![0.0, 0.0, 0.0]);
        assert_eq!(sf.bounds_max, vec![1.0, 1.0, 1.0]);
        // spacing = (max-min)/counts = 0.5 per axis
        for ax in 0..3 {
            assert!((sf.spacing[ax] - 0.5).abs() < 1e-12, "spacing[{ax}]");
        }

        // (a): check the origin node (ix=0,iy=0,iz=0) — exactly at node 0.
        // Row-major: flat index = (ix*(ny+1) + iy)*(nz+1) + iz
        //   origin → 0, data offset = 0*3 = 0
        let origin_data: Vec<f64> = sf.data[0..3].to_vec();
        for c in 0..3 {
            let expected = b[c]; // u(0,0,0) = b
            assert!(
                (origin_data[c] - expected).abs() < 1e-12,
                "origin component {c}: got {}, expected {}",
                origin_data[c],
                expected
            );
        }

        // centroid of tet: (0.25, 0.25, 0.25) — must be INSIDE the unit tet
        // (barycentric: λ0=0.25, λ1=0.25, λ2=0.25, λ3=0.25 — all positive).
        // But it's not a grid node. Instead, check node at (0.5,0.0,0.0):
        // ix=1, iy=0, iz=0 → flat idx = (1*3+0)*3+0 = 9; data offset = 9*3 = 27
        // u(0.5,0,0) = [a[0]*0.5+b[0], b[1], b[2]] = [6.0, 6.0, 7.0]
        // BUT: (0.5,0.0,0.0) is on the edge of the tet — we allow tol=1e-9.
        let node_100 = &sf.data[27..30];
        let expected_100 = [a[0] * 0.5 + b[0], b[1], b[2]];
        for c in 0..3 {
            assert!(
                (node_100[c] - expected_100[c]).abs() < 1e-12,
                "node(1,0,0) component {c}: got {}, expected {}",
                node_100[c],
                expected_100[c]
            );
        }
    }

    // ── Test (c): grid point outside all elements → NaN ──────────────────────

    #[test]
    fn resample_outside_solid_is_nan() {
        let (nodes, elems) = unit_tet();

        // trivial nodal values (constant)
        let nodal_values = vec![1.0_f64; nodes.len() * 3];

        // Grid spanning [0,2]×[0,2]×[0,2] — points at x=1,y=1,z=1 etc.
        // are far outside the unit tet.
        let grid = GridSpec {
            bounds_min: [0.0, 0.0, 0.0],
            bounds_max: [2.0, 2.0, 2.0],
            counts: [1, 1, 1],
        };

        let sf = resample_nodal_to_grid(&nodes, &elems, &nodal_values, 3, &grid, "u", 1e-9);

        // 8 grid nodes (2×2×2). Find at least one that's NaN (the corner at (2,2,2)).
        // Flat index of (ix=1,iy=1,iz=1) = (1*2+1)*2+1 = 5; data offset = 5*3 = 15
        let outside = &sf.data[15..18];
        for (c, &val) in outside.iter().enumerate() {
            assert!(
                val.is_nan(),
                "outside[{c}] should be NaN, got {}",
                val
            );
        }
    }

    // ── Test (d): stride-9 constant tensor round-trips ────────────────────────

    #[test]
    fn resample_stride9_constant_tensor_roundtrip() {
        let (nodes, elems) = unit_tet();

        // Constant stress tensor at every node: identity-like
        // [1,2,3,4,5,6,7,8,9] (row-major 3×3)
        let tensor: [f64; 9] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let nodal_values: Vec<f64> = nodes
            .iter()
            .flat_map(|_| tensor.iter().copied())
            .collect();

        // Grid: 1 element per axis → 2 nodes per axis; only origin [0,0,0] is in tet
        let grid = GridSpec {
            bounds_min: [0.0, 0.0, 0.0],
            bounds_max: [1.0, 1.0, 1.0],
            counts: [1, 1, 1],
        };

        let sf = resample_nodal_to_grid(&nodes, &elems, &nodal_values, 9, &grid, "stress", 1e-9);

        // data.len == 2*2*2*9 = 72
        assert_eq!(sf.data.len(), 72, "stride-9 data len");

        // origin node (ix=0,iy=0,iz=0) → exactly at node 0 → barycentric [1,0,0,0]
        // → should recover tensor exactly
        let origin = &sf.data[0..9];
        for (i, &expected) in tensor.iter().enumerate() {
            assert!(
                (origin[i] - expected).abs() < 1e-12,
                "tensor component {i}: got {}, expected {}",
                origin[i],
                expected
            );
        }
    }

    // ── Test (e): data ordering — row-major x-outer / z-inner ────────────────

    #[test]
    fn resample_data_ordering_row_major_x_outer_z_inner() {
        // Build a 2×2×2 hex that exactly tiles the [0,1]³ cube with 6 tets.
        // We just use the unit tet but with a grid of counts=[1,1,1] (2³ nodes).
        // Since the tet only covers part of the cube, many corners will be NaN.
        // We use a box mesh instead.

        // 2×2×2 box mesh: 8 nodes at corners, split into 6 tets (Freudenthal).
        let nodes: Vec<[f64; 3]> = vec![
            [0.0, 0.0, 0.0], // 0
            [1.0, 0.0, 0.0], // 1
            [1.0, 1.0, 0.0], // 2
            [0.0, 1.0, 0.0], // 3
            [0.0, 0.0, 1.0], // 4
            [1.0, 0.0, 1.0], // 5
            [1.0, 1.0, 1.0], // 6
            [0.0, 1.0, 1.0], // 7
        ];
        let elems: Vec<[usize; 4]> = vec![
            [0, 1, 2, 6],
            [0, 2, 3, 6],
            [0, 5, 1, 6],
            [0, 3, 7, 6],
            [0, 4, 5, 6],
            [0, 7, 4, 6],
        ];

        // Nodal field: f(node) = 10*x + y  (stride 1 for simplicity)
        let nodal_values: Vec<f64> = nodes.iter().map(|n| 10.0 * n[0] + n[1]).collect();

        // Grid: counts=[1,1,1] → 2×2×2 = 8 grid nodes at axis values [0,1]³
        let grid = GridSpec {
            bounds_min: [0.0, 0.0, 0.0],
            bounds_max: [1.0, 1.0, 1.0],
            counts: [1, 1, 1],
        };

        let sf = resample_nodal_to_grid(&nodes, &elems, &nodal_values, 1, &grid, "f", 1e-9);

        // Verify all 8 grid nodes are finite (the box covers all corners).
        for (i, &v) in sf.data.iter().enumerate() {
            assert!(!v.is_nan(), "grid point {i} should be finite, got NaN");
        }

        // Row-major x-outer z-inner: flat index = (ix*(ny+1)+iy)*(nz+1)+iz
        // For counts=[1,1,1], nx=1,ny=1,nz=1:
        //   idx(ix,iy,iz) = (ix*2+iy)*2+iz
        //
        // Grid node (1,0,0) → ix=1,iy=0,iz=0 → flat = (1*2+0)*2+0 = 4
        // Coords = (1.0, 0.0, 0.0) → f = 10*1+0 = 10.0
        assert!(
            (sf.data[4] - 10.0).abs() < 1e-12,
            "node(1,0,0) expected 10.0, got {}",
            sf.data[4]
        );

        // Grid node (0,1,0) → ix=0,iy=1,iz=0 → flat = (0*2+1)*2+0 = 2
        // Coords = (0.0, 1.0, 0.0) → f = 10*0+1 = 1.0
        assert!(
            (sf.data[2] - 1.0).abs() < 1e-12,
            "node(0,1,0) expected 1.0, got {}",
            sf.data[2]
        );

        // Grid node (1,1,0) → ix=1,iy=1,iz=0 → flat = (1*2+1)*2+0 = 6
        // Coords = (1.0, 1.0, 0.0) → f = 10*1+1 = 11.0
        assert!(
            (sf.data[6] - 11.0).abs() < 1e-12,
            "node(1,1,0) expected 11.0, got {}",
            sf.data[6]
        );
    }
}

// ── Step-5/6: BVH-backed resample tests ──────────────────────────────────────
//
// Step-5 RED: imports ResampleStats / instrumented fns which don't exist yet
//             → compile error → RED.
// Step-6 GREEN: those items exist → module compiles → tests pass.

#[cfg(test)]
mod bvh_tests {
    // These imports drive the RED compile error in step 5; they resolve in step 6.
    use super::{
        GridSpec, ResampleStats, resample_multi_nodal_to_grid_instrumented,
        resample_nodal_to_grid_instrumented,
    };
    use crate::interpolation::barycentric_p1;

    // ── Fixtures ─────────────────────────────────────────────────────────────

    /// Build a Freudenthal box-of-tets: M³ hexes → 6·M³ tets tiling [0,1]³.
    ///
    /// Node index: `ix*(M+1)²+iy*(M+1)+iz`; physical coords: `(ix/M, iy/M, iz/M)`.
    /// Per-hex Freudenthal decomposition uses the (1,1,1)-corner diagonal n6,
    /// matching the existing 6-tet fixture in `tests::resample_data_ordering_…`.
    fn make_box_of_tets(m: usize) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
        let m1 = m + 1;
        let mut nodes = Vec::with_capacity(m1 * m1 * m1);
        for ix in 0..=m {
            for iy in 0..=m {
                for iz in 0..=m {
                    nodes.push([
                        ix as f64 / m as f64,
                        iy as f64 / m as f64,
                        iz as f64 / m as f64,
                    ]);
                }
            }
        }
        let node = |ix: usize, iy: usize, iz: usize| ix * m1 * m1 + iy * m1 + iz;
        let mut elems = Vec::with_capacity(6 * m * m * m);
        for cx in 0..m {
            for cy in 0..m {
                for cz in 0..m {
                    let n0 = node(cx, cy, cz);
                    let n1 = node(cx + 1, cy, cz);
                    let n2 = node(cx + 1, cy + 1, cz);
                    let n3 = node(cx, cy + 1, cz);
                    let n4 = node(cx, cy, cz + 1);
                    let n5 = node(cx + 1, cy, cz + 1);
                    let n6 = node(cx + 1, cy + 1, cz + 1);
                    let n7 = node(cx, cy + 1, cz + 1);
                    elems.push([n0, n1, n2, n6]);
                    elems.push([n0, n2, n3, n6]);
                    elems.push([n0, n5, n1, n6]);
                    elems.push([n0, n3, n7, n6]);
                    elems.push([n0, n4, n5, n6]);
                    elems.push([n0, n7, n4, n6]);
                }
            }
        }
        (nodes, elems)
    }

    /// Grid spanning slightly beyond [0,1]³ so it includes interior, shared-face
    /// boundary, AND outside (NaN) grid points.
    fn test_grid() -> GridSpec {
        GridSpec {
            bounds_min: [-0.1, -0.1, -0.1],
            bounds_max: [1.1, 1.1, 1.1],
            counts: [5, 5, 5],
        }
    }

    /// Linear-scan oracle mirroring the old `resample_nodal_to_grid` loop
    /// byte-for-byte (same barycentric check + same weight arithmetic +
    /// same break-on-first-hit = lowest-index hit).
    ///
    /// Returns `(data, point_in_tet_test_count)` so callers can assert both
    /// the bit-identical values AND the O(grid·n_elems) baseline count.
    fn linear_resample_single(
        nodes: &[[f64; 3]],
        elems: &[[usize; 4]],
        nodal_values: &[f64],
        stride: usize,
        grid: &GridSpec,
        tol: f64,
    ) -> (Vec<f64>, u64) {
        let [nx, ny, nz] = grid.counts;
        let sx = (grid.bounds_max[0] - grid.bounds_min[0]) / nx.max(1) as f64;
        let sy = (grid.bounds_max[1] - grid.bounds_min[1]) / ny.max(1) as f64;
        let sz = (grid.bounds_max[2] - grid.bounds_min[2]) / nz.max(1) as f64;
        let ax = reify_ir::sampled::linspace_inclusive(grid.bounds_min[0], grid.bounds_max[0], sx)
            .unwrap();
        let ay = reify_ir::sampled::linspace_inclusive(grid.bounds_min[1], grid.bounds_max[1], sy)
            .unwrap();
        let az = reify_ir::sampled::linspace_inclusive(grid.bounds_min[2], grid.bounds_max[2], sz)
            .unwrap();
        let n_grid = ax.len() * ay.len() * az.len();
        let mut data = Vec::with_capacity(n_grid * stride);
        let mut count = 0u64;
        for ix in 0..ax.len() {
            for iy in 0..ay.len() {
                for iz in 0..az.len() {
                    let p = [ax[ix], ay[iy], az[iz]];
                    let mut found = false;
                    'scan: for conn in elems {
                        let phys4: [[f64; 3]; 4] = [
                            nodes[conn[0]],
                            nodes[conn[1]],
                            nodes[conn[2]],
                            nodes[conn[3]],
                        ];
                        let bary = barycentric_p1(&phys4, p);
                        count += 1;
                        if bary.iter().all(|&b| b >= -tol && b <= 1.0 + tol) {
                            for c in 0..stride {
                                let val = bary[0] * nodal_values[conn[0] * stride + c]
                                    + bary[1] * nodal_values[conn[1] * stride + c]
                                    + bary[2] * nodal_values[conn[2] * stride + c]
                                    + bary[3] * nodal_values[conn[3] * stride + c];
                                data.push(val);
                            }
                            found = true;
                            break 'scan;
                        }
                    }
                    if !found {
                        for _ in 0..stride {
                            data.push(f64::NAN);
                        }
                    }
                }
            }
        }
        (data, count)
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// Step-5 RED / Step-6 GREEN:
    ///
    /// (1) BIT-IDENTICAL — every output element equals the linear oracle via
    ///     `f64::to_bits()` (NaN-aware) for stride 1, 3, and 9, for both
    ///     `resample_nodal_to_grid_instrumented` and
    ///     `resample_multi_nodal_to_grid_instrumented`, on a grid spanning
    ///     slightly beyond [0,1]³ (interior + boundary + outside NaN points).
    ///
    /// (2) QUERY-COUNT — on M=4 (384 tets) the BVH
    ///     `stats.point_in_tet_tests * 4 < linear_count` (≥4× efficiency).
    ///
    /// (3) SCALING — from M=4 to M=8 (8× more tets, same grid):
    ///     - linear count grows >4×  (confirms Θ(grid·n_elems) scaling)
    ///     - BVH count grows <2×     (confirms sub-linear/log query)
    ///
    /// Fails to compile in step 5 (ResampleStats / instrumented fns absent) → RED.
    #[test]
    fn bvh_resample_bit_identical_and_query_count() {
        let (nodes4, elems4) = make_box_of_tets(4);
        let (nodes8, elems8) = make_box_of_tets(8);
        assert_eq!(elems4.len(), 384, "M=4: 6*4³=384 tets");
        assert_eq!(elems8.len(), 3072, "M=8: 6*8³=3072 tets");

        let grid = test_grid(); // 6×6×6 = 216 grid points in [-0.1,1.1]³
        let tol = 1e-9_f64;

        // Non-trivial nodal fields — varied to catch any bary-weight/index bugs.
        let nv_s3_4: Vec<f64> = nodes4
            .iter()
            .flat_map(|n| {
                [
                    2.0 * n[0] + n[1] + 0.5,
                    n[0] + 3.0 * n[1] + n[2],
                    0.5 * n[1] + n[2] + 1.0,
                ]
            })
            .collect();
        let nv_s3_8: Vec<f64> = nodes8
            .iter()
            .flat_map(|n| {
                [
                    2.0 * n[0] + n[1] + 0.5,
                    n[0] + 3.0 * n[1] + n[2],
                    0.5 * n[1] + n[2] + 1.0,
                ]
            })
            .collect();
        let nv_s9_4: Vec<f64> = nodes4
            .iter()
            .flat_map(|n| {
                let (x, y, z) = (n[0], n[1], n[2]);
                [
                    x + y,
                    y + z,
                    x + z,
                    x * 2.0,
                    y * 2.0,
                    z * 2.0,
                    x + y + z,
                    x - y,
                    y - z,
                ]
            })
            .collect();
        let nv_s1_4: Vec<f64> =
            nodes4.iter().map(|n| n[0] + 2.0 * n[1] + 3.0 * n[2]).collect();

        // ── (1a) BIT-IDENTICAL: single fn, stride 3 ──────────────────────────
        let (sf_s3, stats_s3) = resample_nodal_to_grid_instrumented(
            &nodes4, &elems4, &nv_s3_4, 3, &grid, "u_s3", tol,
        );
        let (lin_s3, lin_count_s3) = linear_resample_single(
            &nodes4, &elems4, &nv_s3_4, 3, &grid, tol,
        );
        assert_eq!(sf_s3.data.len(), lin_s3.len(), "stride-3: data lengths must match");
        for (i, (&bvh, &lin)) in sf_s3.data.iter().zip(lin_s3.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "stride-3 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }

        // ── (1b) BIT-IDENTICAL: single fn, stride 9 ──────────────────────────
        let (sf_s9, _) = resample_nodal_to_grid_instrumented(
            &nodes4, &elems4, &nv_s9_4, 9, &grid, "sigma_s9", tol,
        );
        let (lin_s9, _) = linear_resample_single(&nodes4, &elems4, &nv_s9_4, 9, &grid, tol);
        assert_eq!(sf_s9.data.len(), lin_s9.len(), "stride-9: data lengths must match");
        for (i, (&bvh, &lin)) in sf_s9.data.iter().zip(lin_s9.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "stride-9 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }

        // ── (1c) BIT-IDENTICAL: single fn, stride 1 ──────────────────────────
        let (sf_s1, _) = resample_nodal_to_grid_instrumented(
            &nodes4, &elems4, &nv_s1_4, 1, &grid, "f_s1", tol,
        );
        let (lin_s1, _) = linear_resample_single(&nodes4, &elems4, &nv_s1_4, 1, &grid, tol);
        assert_eq!(sf_s1.data.len(), lin_s1.len(), "stride-1: data lengths must match");
        for (i, (&bvh, &lin)) in sf_s1.data.iter().zip(lin_s1.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "stride-1 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }

        // ── (1d) BIT-IDENTICAL: multi fn, stride 3 + stride 9 ───────────────
        let fields_multi: &[(&[f64], usize, &str)] = &[
            (nv_s3_4.as_slice(), 3, "u_multi"),
            (nv_s9_4.as_slice(), 9, "sigma_multi"),
        ];
        let (sf_multi, stats_multi) = resample_multi_nodal_to_grid_instrumented(
            &nodes4, &elems4, fields_multi, &grid, tol,
        );
        assert_eq!(sf_multi.len(), 2, "multi must return 2 fields");
        for (i, (&bvh, &lin)) in sf_multi[0].data.iter().zip(lin_s3.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "multi stride-3 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }
        for (i, (&bvh, &lin)) in sf_multi[1].data.iter().zip(lin_s9.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "multi stride-9 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }

        // ── (2) QUERY-COUNT: BVH ≥4× fewer tests than linear on M=4 ─────────
        assert!(
            stats_s3.point_in_tet_tests * 4 < lin_count_s3,
            "QUERY-COUNT: BVH ({bvh_n}) * 4 = {quad} should be < linear ({lin_n}); \
             BVH must be ≥4× more efficient on M=4 (384 tets)",
            bvh_n = stats_s3.point_in_tet_tests,
            quad = stats_s3.point_in_tet_tests * 4,
            lin_n = lin_count_s3,
        );
        // Also check the multi stats — same locate cost, same assertion.
        assert!(
            stats_multi.point_in_tet_tests * 4 < lin_count_s3,
            "QUERY-COUNT (multi): BVH ({bvh_n}) * 4 = {quad} should be < linear ({lin_n})",
            bvh_n = stats_multi.point_in_tet_tests,
            quad = stats_multi.point_in_tet_tests * 4,
            lin_n = lin_count_s3,
        );

        // ── (3) SCALING: M=4 → M=8 (8× more tets, same grid) ────────────────
        let (_, stats_s3_m8) = resample_nodal_to_grid_instrumented(
            &nodes8, &elems8, &nv_s3_8, 3, &grid, "u_m8", tol,
        );
        let (_, lin_count_s3_m8) =
            linear_resample_single(&nodes8, &elems8, &nv_s3_8, 3, &grid, tol);

        assert!(
            lin_count_s3_m8 > lin_count_s3 * 4,
            "SCALING linear: M=8 count {c8} must be >4× M=4 count {c4} \
             (confirms Θ(grid·n_elems) growth with 8× more tets)",
            c8 = lin_count_s3_m8,
            c4 = lin_count_s3,
        );
        assert!(
            stats_s3_m8.point_in_tet_tests < stats_s3.point_in_tet_tests * 2,
            "SCALING BVH: M=8 count {c8} must be <2× M=4 count {c4} \
             (confirms sub-linear/log growth)",
            c8 = stats_s3_m8.point_in_tet_tests,
            c4 = stats_s3.point_in_tet_tests,
        );
    }
}
