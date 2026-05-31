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
        for c in 0..3 {
            assert!(
                outside[c].is_nan(),
                "outside[{c}] should be NaN, got {}",
                outside[c]
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
