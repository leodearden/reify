//! Second-order tetrahedron (P2) reference element.
//!
//! Quadratic Lagrangian tet with 10 nodes: 4 at the reference vertices
//! `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` and 6 at the midpoints of the
//! edges in the canonical Hughes/Gmsh ordering
//! `(0,1), (1,2), (2,0), (0,3), (1,3), (2,3)`.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// Second-order Lagrangian tetrahedron.
pub struct TetP2;

impl ReferenceElement for TetP2 {
    const N_NODES: usize = 10;

    fn shape_at(&self, _coord: ReferenceCoord) -> Vec<f64> {
        todo!("P2 shape functions — task 2914 step-12")
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
