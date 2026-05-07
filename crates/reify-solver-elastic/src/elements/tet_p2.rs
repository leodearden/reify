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

    #[test]
    fn shape_at_partition_of_unity_at_centroid() {
        let centroid = ReferenceCoord::new(0.25, 0.25, 0.25);
        let sum: f64 = TetP2.shape_at(centroid).iter().sum();
        assert!((sum - 1.0).abs() < TOL, "Σ N_i(centroid) = {sum}");
    }
}
