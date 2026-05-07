//! First-order tetrahedron (P1) reference element.
//!
//! Linear (constant-strain) tet with 4 nodes located at the reference
//! vertices `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`. Shape functions are the
//! barycentric coordinates `[1-ξ-η-ζ, ξ, η, ζ]`; gradients are constant.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// First-order Lagrangian tetrahedron.
pub struct TetP1;

impl ReferenceElement for TetP1 {
    const N_NODES: usize = 4;

    fn shape_at(&self, _coord: ReferenceCoord) -> Vec<f64> {
        todo!("P1 shape functions — task 2914 step-4")
    }

    fn shape_grad_at(&self, _coord: ReferenceCoord) -> Vec<[f64; 3]> {
        todo!("P1 shape-function gradients — task 2914 step-6")
    }

    fn quad_points(&self) -> &'static [QuadraturePoint] {
        todo!("P1 quadrature rule — task 2914 step-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    /// Reference vertices `v_0, v_1, v_2, v_3` in canonical ordering.
    const REF_VERTICES: [ReferenceCoord; 4] = [
        ReferenceCoord::new(0.0, 0.0, 0.0),
        ReferenceCoord::new(1.0, 0.0, 0.0),
        ReferenceCoord::new(0.0, 1.0, 0.0),
        ReferenceCoord::new(0.0, 0.0, 1.0),
    ];

    #[test]
    fn shape_at_satisfies_kronecker_delta_at_reference_vertices() {
        for (i, v) in REF_VERTICES.iter().enumerate() {
            let n = TetP1.shape_at(*v);
            assert_eq!(n.len(), 4, "shape_at must return N_NODES=4 entries");
            for (j, &n_j) in n.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (n_j - expected).abs() < TOL,
                    "N_{j}({:?}) = {n_j}, expected {expected}",
                    v,
                );
            }
        }
    }

    #[test]
    fn shape_at_partition_of_unity_at_centroid_and_interior() {
        let probes = [
            ReferenceCoord::new(0.25, 0.25, 0.25),
            ReferenceCoord::new(0.1, 0.2, 0.3),
        ];
        for p in &probes {
            let sum: f64 = TetP1.shape_at(*p).iter().sum();
            assert!(
                (sum - 1.0).abs() < TOL,
                "Σ N_i({:?}) = {sum}, expected 1.0",
                p,
            );
        }
    }
}
