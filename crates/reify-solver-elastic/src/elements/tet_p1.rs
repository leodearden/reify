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

    /// Barycentric P1 shape functions evaluated at `coord`.
    ///
    /// Returns `[1 - ξ - η - ζ, ξ, η, ζ]` — i.e. shape function `N_i` is
    /// the barycentric coordinate of reference vertex `v_i` in the
    /// canonical ordering `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`.
    fn shape_at(&self, coord: ReferenceCoord) -> Vec<f64> {
        let ReferenceCoord { xi, eta, zeta } = coord;
        vec![1.0 - xi - eta - zeta, xi, eta, zeta]
    }

    /// P1 shape-function gradients in reference coordinates.
    ///
    /// The gradients are constant — independent of `coord` — because P1
    /// shape functions are linear. Returned in the canonical row order
    /// `[∇N_0, ∇N_1, ∇N_2, ∇N_3] = [(-1,-1,-1), (1,0,0), (0,1,0), (0,0,1)]`.
    /// The argument is kept for trait-uniformity with `TetP2`.
    fn shape_grad_at(&self, _coord: ReferenceCoord) -> Vec<[f64; 3]> {
        vec![
            [-1.0, -1.0, -1.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ]
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
    fn shape_grad_at_returns_canonical_constant_gradients() {
        let probes = [
            ReferenceCoord::new(0.25, 0.25, 0.25),
            ReferenceCoord::new(0.0, 0.0, 0.0),
            ReferenceCoord::new(0.1, 0.2, 0.3),
        ];
        let expected = [
            [-1.0, -1.0, -1.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let mut prev: Option<Vec<[f64; 3]>> = None;
        for p in probes {
            let g = TetP1.shape_grad_at(p);
            assert_eq!(g.len(), 4, "shape_grad_at must return N_NODES=4 entries");
            for (i, row) in g.iter().enumerate() {
                for k in 0..3 {
                    assert!(
                        (row[k] - expected[i][k]).abs() < TOL,
                        "∇N_{i}({:?})[{k}] = {} expected {}",
                        p,
                        row[k],
                        expected[i][k],
                    );
                }
            }
            if let Some(prev_g) = &prev {
                for (a, b) in g.iter().zip(prev_g.iter()) {
                    for k in 0..3 {
                        assert!(
                            (a[k] - b[k]).abs() < TOL,
                            "P1 gradients must be constant across reference points",
                        );
                    }
                }
            }
            prev = Some(g);
        }
    }

    #[test]
    fn shape_grad_at_sum_is_zero_partition_of_unity_consequence() {
        let g = TetP1.shape_grad_at(ReferenceCoord::new(0.1, 0.2, 0.3));
        let mut sum = [0.0_f64; 3];
        for row in g {
            for k in 0..3 {
                sum[k] += row[k];
            }
        }
        for k in 0..3 {
            assert!((sum[k]).abs() < TOL, "Σ_i ∇N_i[{k}] = {}, expected 0", sum[k]);
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
