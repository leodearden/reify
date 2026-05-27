//! First-order tetrahedron (P1) reference element.
//!
//! Linear (constant-strain) tet with 4 nodes located at the reference
//! vertices `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`. Shape functions are the
//! barycentric coordinates `[1-ξ-η-ζ, ξ, η, ζ]`; gradients are constant.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// First-order Lagrangian tetrahedron.
pub struct TetP1;

/// Single-entry centroid Gauss rule for the unit reference tetrahedron.
///
/// One point at `(1/4, 1/4, 1/4)` with weight `1/6`. Degree-1 exact —
/// sufficient for stiffness assembly with constant-strain P1 elements:
/// the integrand `B^T D B` is constant per element because P1 gradients
/// are constant, so a 1-point rule exactly captures the integral.
const TET_P1_QUAD: &[QuadraturePoint] = &[QuadraturePoint {
    coord: ReferenceCoord::new(0.25, 0.25, 0.25),
    weight: 1.0 / 6.0,
}];

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
        TET_P1_QUAD
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
    fn quad_points_is_one_point_centroid_rule() {
        let qp = TetP1.quad_points();
        assert_eq!(qp.len(), 1, "P1 quadrature is a 1-point centroid rule");
        let q = qp[0];
        assert!((q.coord.xi - 0.25).abs() < TOL);
        assert!((q.coord.eta - 0.25).abs() < TOL);
        assert!((q.coord.zeta - 0.25).abs() < TOL);
        assert!((q.weight - 1.0 / 6.0).abs() < TOL);
    }

    #[test]
    fn quad_rule_integrates_constant_to_reference_volume() {
        // ∫_T 1 dV = 1/6 (reference-tet volume)
        let qp = TetP1.quad_points();
        let i: f64 = qp.iter().map(|q| q.weight * 1.0).sum();
        assert!((i - 1.0 / 6.0).abs() < TOL);
    }

    #[test]
    fn quad_rule_integrates_linear_xi_exactly() {
        // ∫_T ξ dV = 1/24 (analytical, exact for 1-point Gauss on linears).
        let qp = TetP1.quad_points();
        let i: f64 = qp.iter().map(|q| q.weight * q.coord.xi).sum();
        assert!((i - 1.0 / 24.0).abs() < TOL);
    }

    #[test]
    fn jacobian_is_identity_for_reference_vertices() {
        // Physical nodes coincide with reference vertices ⇒ J = I, det = 1.
        let phys = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let j = TetP1.jacobian(&phys, ReferenceCoord::new(0.25, 0.25, 0.25));
        for i in 0..3 {
            for k in 0..3 {
                let expected = if i == k { 1.0 } else { 0.0 };
                assert!(
                    (j.matrix[i][k] - expected).abs() < TOL,
                    "J[{i}][{k}] = {} expected {}",
                    j.matrix[i][k],
                    expected,
                );
            }
        }
        assert!((j.det - 1.0).abs() < TOL);
    }

    #[test]
    fn jacobian_uniform_scale_doubles_diagonal_and_cubes_det() {
        // Physical nodes at 2 × reference ⇒ J = 2 I, det = 8.
        let phys = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
        ];
        let j = TetP1.jacobian(&phys, ReferenceCoord::new(0.25, 0.25, 0.25));
        for i in 0..3 {
            for k in 0..3 {
                let expected = if i == k { 2.0 } else { 0.0 };
                assert!((j.matrix[i][k] - expected).abs() < TOL);
            }
        }
        assert!((j.det - 8.0).abs() < TOL);
    }

    #[test]
    fn jacobian_negative_det_for_left_handed_node_ordering() {
        // Swap v_2 and v_3 — flips orientation, det should be negative.
        let phys = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0],
        ];
        let j = TetP1.jacobian(&phys, ReferenceCoord::new(0.25, 0.25, 0.25));
        assert!(
            j.det < 0.0,
            "swapped node ordering must yield a negative det (got {})",
            j.det
        );
        assert!(
            (j.det - (-1.0)).abs() < TOL,
            "swapped node ordering must yield det = -1 exactly (got {})",
            j.det,
        );
    }

    #[test]
    fn shape_grad_at_sum_is_zero_partition_of_unity_consequence() {
        let g = TetP1.shape_grad_at(ReferenceCoord::new(0.1, 0.2, 0.3));
        let mut sum = [0.0_f64; 3];
        for row in g {
            for (k, rk) in row.iter().enumerate() {
                sum[k] += rk;
            }
        }
        for (k, s) in sum.iter().enumerate() {
            assert!(s.abs() < TOL, "Σ_i ∇N_i[{k}] = {s}, expected 0");
        }
    }

    #[test]
    #[should_panic(expected = "phys_nodes.len() must equal Self::N_NODES")]
    fn jacobian_panics_on_too_short_phys_nodes() {
        // 3 nodes instead of 4 — one row short
        let phys: &[[f64; 3]] = &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        TetP1.jacobian(phys, ReferenceCoord::new(0.25, 0.25, 0.25));
    }

    #[test]
    #[should_panic(expected = "phys_nodes.len() must equal Self::N_NODES")]
    fn jacobian_panics_on_too_long_phys_nodes() {
        // 5 nodes instead of 4 — one row extra (duplicate of first)
        let phys: &[[f64; 3]] = &[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        ];
        TetP1.jacobian(phys, ReferenceCoord::new(0.25, 0.25, 0.25));
    }

    /// `GRADS_REF` must equal the canonical P1 reference-gradient table.
    ///
    /// The const is the single source of truth shared with
    /// `crate::mass_matrix` and `crate::geometric_stiffness::tet`.  This
    /// test asserts its value against the exact-representable float literals
    /// so a future typo in the const definition is caught immediately.
    #[test]
    fn grads_ref_const_matches_canonical_p1_gradients() {
        let expected: [[f64; 3]; 4] = [
            [-1.0, -1.0, -1.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        assert_eq!(GRADS_REF, expected);
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
