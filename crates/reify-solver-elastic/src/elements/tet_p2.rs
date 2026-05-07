//! Second-order tetrahedron (P2) reference element.
//!
//! Quadratic Lagrangian tet with 10 nodes: 4 at the reference vertices
//! `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` and 6 at the midpoints of the
//! edges in the canonical Hughes/Gmsh ordering
//! `(0,1), (1,2), (2,0), (0,3), (1,3), (2,3)`.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// Second-order Lagrangian tetrahedron.
pub struct TetP2;

/// Canonical edge ordering for the P2 reference tet's 6 edge midpoints,
/// as `(a, b)` index pairs into the 4 reference vertices.
///
/// Edge index 0..=5 maps to the corresponding entry here (Hughes/Gmsh
/// ordering: bottom-face edges first, then vertical edges to vertex 3).
/// Both `shape_at` and `shape_grad_at` consult this table so the edge
/// indexing stays single-sourced.
pub const EDGES: [(usize, usize); 6] = [
    (0, 1),
    (1, 2),
    (2, 0),
    (0, 3),
    (1, 3),
    (2, 3),
];

impl ReferenceElement for TetP2 {
    const N_NODES: usize = 10;

    /// Quadratic Lagrangian P2 shape functions evaluated at `coord`.
    ///
    /// Returned in canonical 10-node order: the 4 vertex shapes
    /// `λ_i (2 λ_i − 1)` (where `λ_0 = 1-ξ-η-ζ`, `λ_1 = ξ`, `λ_2 = η`,
    /// `λ_3 = ζ`) followed by the 6 edge shapes `4 λ_a λ_b` for the
    /// edge-pair `(a, b) = EDGES[edge_index]`.
    fn shape_at(&self, coord: ReferenceCoord) -> Vec<f64> {
        let ReferenceCoord { xi, eta, zeta } = coord;
        let lambda = [1.0 - xi - eta - zeta, xi, eta, zeta];

        let mut n = Vec::with_capacity(10);
        // Vertex shapes
        for &lam in &lambda {
            n.push(lam * (2.0 * lam - 1.0));
        }
        // Edge shapes
        for &(a, b) in &EDGES {
            n.push(4.0 * lambda[a] * lambda[b]);
        }
        n
    }

    fn shape_grad_at(&self, _coord: ReferenceCoord) -> Vec<[f64; 3]> {
        todo!("P2 shape-function gradients — task 2914 step-14")
    }

    fn quad_points(&self) -> &'static [QuadraturePoint] {
        todo!("P2 quadrature rule — task 2914 step-16")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    /// All 10 P2 nodes in canonical Hughes/Gmsh ordering:
    /// indices 0..=3 are the four reference vertices,
    /// indices 4..=9 are midpoints of edges
    /// `(0,1), (1,2), (2,0), (0,3), (1,3), (2,3)` in that order.
    const NODES: [ReferenceCoord; 10] = [
        // Vertices
        ReferenceCoord::new(0.0, 0.0, 0.0),
        ReferenceCoord::new(1.0, 0.0, 0.0),
        ReferenceCoord::new(0.0, 1.0, 0.0),
        ReferenceCoord::new(0.0, 0.0, 1.0),
        // Edge midpoints
        ReferenceCoord::new(0.5, 0.0, 0.0), // (0,1)
        ReferenceCoord::new(0.5, 0.5, 0.0), // (1,2)
        ReferenceCoord::new(0.0, 0.5, 0.0), // (2,0)
        ReferenceCoord::new(0.0, 0.0, 0.5), // (0,3)
        ReferenceCoord::new(0.5, 0.0, 0.5), // (1,3)
        ReferenceCoord::new(0.0, 0.5, 0.5), // (2,3)
    ];

    #[test]
    fn shape_at_satisfies_kronecker_delta_at_all_ten_nodes() {
        for (j, node) in NODES.iter().enumerate() {
            let n = TetP2.shape_at(*node);
            assert_eq!(n.len(), 10, "shape_at must return N_NODES=10 entries");
            for (i, &n_i) in n.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (n_i - expected).abs() < TOL,
                    "N_{i}({:?}) = {n_i}, expected {expected}",
                    node,
                );
            }
        }
    }

    /// Reference-coordinate gradients of the barycentric coordinates λ.
    /// `∇λ_0 = (-1,-1,-1)` (since `λ_0 = 1-ξ-η-ζ`), `∇λ_1 = e_x`,
    /// `∇λ_2 = e_y`, `∇λ_3 = e_z`.
    const GRAD_LAMBDA: [[f64; 3]; 4] = [
        [-1.0, -1.0, -1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    #[test]
    fn shape_grad_at_vertex_nodes_match_chain_rule_at_centroid() {
        // At centroid: λ_i = 1/4 for i=0..3, so 4λ_i − 1 = 0 ⇒ all
        // vertex-node gradients vanish. Verify the closed-form value.
        let centroid = ReferenceCoord::new(0.25, 0.25, 0.25);
        let g = TetP2.shape_grad_at(centroid);
        assert_eq!(g.len(), 10);
        for i in 0..4 {
            for k in 0..3 {
                assert!(
                    g[i][k].abs() < TOL,
                    "∇N_{i}(centroid)[{k}] = {} expected 0 (4λ_i−1 = 0 at centroid)",
                    g[i][k],
                );
            }
        }

        // Off-centroid probe: sanity-check the chain rule analytically
        // for vertex node 0. ∇N_0 = (4 λ_0 − 1) ∇λ_0 with λ_0 = 1-ξ-η-ζ.
        let p = ReferenceCoord::new(0.1, 0.2, 0.15);
        let lambda_0 = 1.0 - p.xi - p.eta - p.zeta;
        let scalar = 4.0 * lambda_0 - 1.0;
        let g_p = TetP2.shape_grad_at(p);
        for k in 0..3 {
            let expected = scalar * GRAD_LAMBDA[0][k];
            assert!(
                (g_p[0][k] - expected).abs() < TOL,
                "∇N_0(p)[{k}] = {} expected {expected}",
                g_p[0][k],
            );
        }
    }

    #[test]
    fn shape_grad_at_partition_of_unity_consequence() {
        // Σ_i ∇N_i = 0 at any reference point.
        let probes = [
            ReferenceCoord::new(0.25, 0.25, 0.25),
            ReferenceCoord::new(0.1, 0.2, 0.3),
            ReferenceCoord::new(0.5, 0.0, 0.0),
        ];
        for p in &probes {
            let g = TetP2.shape_grad_at(*p);
            let mut sum = [0.0_f64; 3];
            for row in g {
                for k in 0..3 {
                    sum[k] += row[k];
                }
            }
            for k in 0..3 {
                assert!(
                    sum[k].abs() < TOL,
                    "Σ_i ∇N_i({:?})[{k}] = {}, expected 0",
                    p,
                    sum[k]
                );
            }
        }
    }

    #[test]
    fn shape_grad_at_varies_linearly_in_reference_coords() {
        // P2 shape gradients are linear in (ξ, η, ζ): given two probes
        // p1, p2, the midpoint gradient must equal the average — a
        // direct consequence of linearity.
        let p1 = ReferenceCoord::new(0.1, 0.2, 0.15);
        let p2 = ReferenceCoord::new(0.3, 0.1, 0.25);
        let mid = ReferenceCoord::new(
            0.5 * (p1.xi + p2.xi),
            0.5 * (p1.eta + p2.eta),
            0.5 * (p1.zeta + p2.zeta),
        );
        let g1 = TetP2.shape_grad_at(p1);
        let g2 = TetP2.shape_grad_at(p2);
        let gm = TetP2.shape_grad_at(mid);
        for i in 0..10 {
            for k in 0..3 {
                let avg = 0.5 * (g1[i][k] + g2[i][k]);
                assert!(
                    (gm[i][k] - avg).abs() < TOL,
                    "∇N_{i}(mid)[{k}] = {} avg = {avg}",
                    gm[i][k],
                );
            }
        }
    }

    #[test]
    fn shape_at_partition_of_unity_at_centroid() {
        let centroid = ReferenceCoord::new(0.25, 0.25, 0.25);
        let sum: f64 = TetP2.shape_at(centroid).iter().sum();
        assert!((sum - 1.0).abs() < TOL, "Σ N_i(centroid) = {sum}");
    }
}
