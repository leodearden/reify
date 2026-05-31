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
//! - Containment uses a linear scan over elements with `barycentric_p1` — O(grid·elems).
//!   Accepted for the v0.4 prismatic slice; a BVH/spatial-index follow-up is
//!   recorded as a non-blocking escalate_info (interpolation.rs:180-188).
//! - Out-of-solid grid points carry `f64::NAN` for all stride components.
//! - Data ordering: row-major axis-0(x) outermost → axis-2(z) innermost,
//!   with stride components contiguous per grid point.

use std::sync::atomic::AtomicBool;

use reify_ir::{InterpolationKind, SampledField, SampledGridKind};

use crate::interpolation::barycentric_p1;

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
    let [nx, ny, nz] = grid.counts;
    // Spacing per axis: (max-min)/count
    let sx = (grid.bounds_max[0] - grid.bounds_min[0]) / nx.max(1) as f64;
    let sy = (grid.bounds_max[1] - grid.bounds_min[1]) / ny.max(1) as f64;
    let sz = (grid.bounds_max[2] - grid.bounds_min[2]) / nz.max(1) as f64;
    let spacing = vec![sx, sy, sz];

    // Build per-axis grid coordinates via linspace_inclusive.
    // Preconditions: finite bounds + spacing > 0 ⇒ always Ok.
    let axis_grids: Vec<Vec<f64>> = (0..3)
        .map(|i| {
            let sp = spacing[i];
            reify_ir::sampled::linspace_inclusive(grid.bounds_min[i], grid.bounds_max[i], sp)
                .expect("resample_nodal_to_grid: linspace_inclusive failed — check that bounds_min < bounds_max and counts > 0")
        })
        .collect();

    let nx1 = axis_grids[0].len();
    let ny1 = axis_grids[1].len();
    let nz1 = axis_grids[2].len();
    let n_grid = nx1 * ny1 * nz1;

    let mut data = Vec::with_capacity(n_grid * stride);

    // Row-major iteration: axis-0(x) outermost → axis-2(z) innermost.
    for ix in 0..nx1 {
        for iy in 0..ny1 {
            for iz in 0..nz1 {
                let p = [axis_grids[0][ix], axis_grids[1][iy], axis_grids[2][iz]];

                // Linear scan: find first element containing p.
                let mut found = false;
                'elem_scan: for conn in elems {
                    let phys4: [[f64; 3]; 4] = [
                        nodes[conn[0]],
                        nodes[conn[1]],
                        nodes[conn[2]],
                        nodes[conn[3]],
                    ];
                    let bary = barycentric_p1(&phys4, p);
                    // Accept if all four barycentric coords in [-tol, 1+tol].
                    if bary.iter().all(|&b| b >= -tol && b <= 1.0 + tol) {
                        // Weighted sum over stride components.
                        for c in 0..stride {
                            let val = bary[0] * nodal_values[conn[0] * stride + c]
                                + bary[1] * nodal_values[conn[1] * stride + c]
                                + bary[2] * nodal_values[conn[2] * stride + c]
                                + bary[3] * nodal_values[conn[3] * stride + c];
                            data.push(val);
                        }
                        found = true;
                        break 'elem_scan;
                    }
                }

                if !found {
                    // Out-of-solid sentinel: NaN per stride component.
                    for _ in 0..stride {
                        data.push(f64::NAN);
                    }
                }
            }
        }
    }

    SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: grid.bounds_min.to_vec(),
        bounds_max: grid.bounds_max.to_vec(),
        spacing,
        axis_grids,
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    }
}

/// Resample **multiple** nodal fields onto the same Regular3D grid in a single
/// geometry pass.
///
/// Semantically equivalent to calling [`resample_nodal_to_grid`] once per entry
/// in `fields`, but the containing-tet + barycentric-weight computation is done
/// **once per grid point** instead of once per *(grid point × field)*.  For the
/// buckling trampoline (~13 k grid points × 61 k tets × 2 fields) this halves
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
    let [nx, ny, nz] = grid.counts;
    let sx = (grid.bounds_max[0] - grid.bounds_min[0]) / nx.max(1) as f64;
    let sy = (grid.bounds_max[1] - grid.bounds_min[1]) / ny.max(1) as f64;
    let sz = (grid.bounds_max[2] - grid.bounds_min[2]) / nz.max(1) as f64;
    let spacing = vec![sx, sy, sz];

    let axis_grids: Vec<Vec<f64>> = (0..3)
        .map(|i| {
            let sp = spacing[i];
            reify_ir::sampled::linspace_inclusive(grid.bounds_min[i], grid.bounds_max[i], sp)
                .expect("resample_multi_nodal_to_grid: linspace_inclusive failed — \
                         check that bounds_min < bounds_max and counts > 0")
        })
        .collect();

    let nx1 = axis_grids[0].len();
    let ny1 = axis_grids[1].len();
    let nz1 = axis_grids[2].len();
    let n_grid = nx1 * ny1 * nz1;

    // Pre-allocate one output buffer per field.
    let mut data_bufs: Vec<Vec<f64>> = fields
        .iter()
        .map(|(_, stride, _)| Vec::with_capacity(n_grid * stride))
        .collect();

    // Single geometry pass: locate the containing tet once per grid point,
    // then apply the barycentric weights to every field simultaneously.
    for ix in 0..nx1 {
        for iy in 0..ny1 {
            for iz in 0..nz1 {
                let p = [axis_grids[0][ix], axis_grids[1][iy], axis_grids[2][iz]];

                let mut hit = false;
                'elem_scan: for conn in elems {
                    let phys4: [[f64; 3]; 4] = [
                        nodes[conn[0]],
                        nodes[conn[1]],
                        nodes[conn[2]],
                        nodes[conn[3]],
                    ];
                    let bary = barycentric_p1(&phys4, p);
                    if bary.iter().all(|&b| b >= -tol && b <= 1.0 + tol) {
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
                        hit = true;
                        break 'elem_scan;
                    }
                }

                if !hit {
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

    // Assemble one SampledField per input field, sharing the same grid metadata.
    fields
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
        .collect()
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
