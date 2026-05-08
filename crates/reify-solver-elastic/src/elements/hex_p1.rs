//! First-order hexahedron (P1 / hex8) reference element.
//!
//! Trilinear 8-node element defined on the **reference cube** `[-1, 1]³`
//! with vertices at the 8 corners `{±1}³`. Shape functions are tensor
//! products of linear 1D Lagrange basis functions:
//!
//! ```text
//! N_i(ξ, η, ζ) = (1/8)(1 + ξ_i ξ)(1 + η_i η)(1 + ζ_i ζ)
//! ```
//!
//! where `(ξ_i, η_i, ζ_i) ∈ {±1}³` is the sign-pattern triple for node `i`
//! in the canonical Hughes/Gmsh hex8 ordering:
//!
//! | node | ξ  | η  | ζ  |
//! |------|----|----|----|
//! | 0    | −1 | −1 | −1 |
//! | 1    | +1 | −1 | −1 |
//! | 2    | +1 | +1 | −1 |
//! | 3    | −1 | +1 | −1 |
//! | 4    | −1 | −1 | +1 |
//! | 5    | +1 | −1 | +1 |
//! | 6    | +1 | +1 | +1 |
//! | 7    | −1 | +1 | +1 |
//!
//! Bottom face (ζ = −1) traversed counter-clockwise when viewed from +ζ;
//! top face (ζ = +1) in the same cyclic order.  Right-handed orientation —
//! the canonical ordering produces a positive `det J` for an unsheared cube.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// Gauss-Legendre 1/√3 coordinate (within 1 ulp of `(3.0_f64).sqrt().recip()`).
///
/// Hard-coded literal because `f64::sqrt` is not `const fn` — mirrors the
/// `TET_P2_STROUD_A`/`B` pattern in `tet_p2.rs`.
const HEX_P1_GAUSS_PT: f64 = 0.5773502691896257; // ≈ 1/√3

/// 2×2×2 Gauss-Legendre quadrature rule for the reference cube `[-1, 1]³`.
///
/// 8 points at `(±1/√3, ±1/√3, ±1/√3)`, all with weight 1.  Total weight
/// 8 = reference-cube volume.  Tensor product of the 1D 2-point
/// Gauss-Legendre rule on `[-1, 1]`, which is degree-3 exact per axis —
/// sufficient for the trilinear stiffness integrand `Bᵀ D B` on a
/// constant-Jacobian hex (each `B = ∇N` component is bilinear in the two
/// remaining reference coordinates, so `Bᵀ D B` has per-axis degree ≤ 2,
/// well within the rule's degree-3-per-axis exactness).
const HEX_P1_QUAD: &[QuadraturePoint] = &[
    QuadraturePoint { coord: ReferenceCoord::new(-HEX_P1_GAUSS_PT, -HEX_P1_GAUSS_PT, -HEX_P1_GAUSS_PT), weight: 1.0 },
    QuadraturePoint { coord: ReferenceCoord::new( HEX_P1_GAUSS_PT, -HEX_P1_GAUSS_PT, -HEX_P1_GAUSS_PT), weight: 1.0 },
    QuadraturePoint { coord: ReferenceCoord::new(-HEX_P1_GAUSS_PT,  HEX_P1_GAUSS_PT, -HEX_P1_GAUSS_PT), weight: 1.0 },
    QuadraturePoint { coord: ReferenceCoord::new( HEX_P1_GAUSS_PT,  HEX_P1_GAUSS_PT, -HEX_P1_GAUSS_PT), weight: 1.0 },
    QuadraturePoint { coord: ReferenceCoord::new(-HEX_P1_GAUSS_PT, -HEX_P1_GAUSS_PT,  HEX_P1_GAUSS_PT), weight: 1.0 },
    QuadraturePoint { coord: ReferenceCoord::new( HEX_P1_GAUSS_PT, -HEX_P1_GAUSS_PT,  HEX_P1_GAUSS_PT), weight: 1.0 },
    QuadraturePoint { coord: ReferenceCoord::new(-HEX_P1_GAUSS_PT,  HEX_P1_GAUSS_PT,  HEX_P1_GAUSS_PT), weight: 1.0 },
    QuadraturePoint { coord: ReferenceCoord::new( HEX_P1_GAUSS_PT,  HEX_P1_GAUSS_PT,  HEX_P1_GAUSS_PT), weight: 1.0 },
];

/// First-order Lagrangian hexahedron (trilinear hex8).
pub struct HexP1;

/// Sign-pattern triples `(ξ_i, η_i, ζ_i) ∈ {±1}³` for each of the 8 nodes
/// in the canonical Hughes/Gmsh hex8 ordering.
///
/// Single-source: used by both `shape_at` and `shape_grad_at` to prevent
/// per-method ordering drift.
pub const VERTEX_SIGNS: [[f64; 3]; 8] = [
    [-1.0, -1.0, -1.0], // v_0
    [ 1.0, -1.0, -1.0], // v_1
    [ 1.0,  1.0, -1.0], // v_2
    [-1.0,  1.0, -1.0], // v_3
    [-1.0, -1.0,  1.0], // v_4
    [ 1.0, -1.0,  1.0], // v_5
    [ 1.0,  1.0,  1.0], // v_6
    [-1.0,  1.0,  1.0], // v_7
];

impl ReferenceElement for HexP1 {
    const N_NODES: usize = 8;

    /// Trilinear shape functions evaluated at `coord`.
    ///
    /// Returns `[N_0, …, N_7]` where
    /// `N_i(ξ, η, ζ) = (1/8)(1 + ξ_i ξ)(1 + η_i η)(1 + ζ_i ζ)`.
    fn shape_at(&self, coord: ReferenceCoord) -> Vec<f64> {
        let ReferenceCoord { xi, eta, zeta } = coord;
        let mut n = Vec::with_capacity(8);
        for s in &VERTEX_SIGNS {
            n.push((1.0 + s[0] * xi) * (1.0 + s[1] * eta) * (1.0 + s[2] * zeta) / 8.0);
        }
        n
    }

    /// Shape-function gradients in reference coordinates.
    ///
    /// Returns `[∇N_0, …, ∇N_7]` where each row is
    /// `[∂N_i/∂ξ, ∂N_i/∂η, ∂N_i/∂ζ]`.  Derived via the product rule:
    ///
    /// ```text
    /// ∇N_i = (1/8) [ξ_i (1 + η_i η)(1 + ζ_i ζ),
    ///               η_i (1 + ξ_i ξ)(1 + ζ_i ζ),
    ///               ζ_i (1 + ξ_i ξ)(1 + η_i η)]
    /// ```
    fn shape_grad_at(&self, coord: ReferenceCoord) -> Vec<[f64; 3]> {
        let ReferenceCoord { xi, eta, zeta } = coord;
        let mut g = Vec::with_capacity(8);
        for s in &VERTEX_SIGNS {
            let (sx, sy, sz) = (s[0], s[1], s[2]);
            g.push([
                (sx / 8.0) * (1.0 + sy * eta)  * (1.0 + sz * zeta),
                (sy / 8.0) * (1.0 + sx * xi)   * (1.0 + sz * zeta),
                (sz / 8.0) * (1.0 + sx * xi)   * (1.0 + sy * eta),
            ]);
        }
        g
    }

    fn quad_points(&self) -> &'static [QuadraturePoint] {
        HEX_P1_QUAD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    /// Reference cube vertices `v_0..v_7` in canonical Hughes/Gmsh ordering:
    /// bottom face (ζ = −1) counter-clockwise when viewed from +ζ, then
    /// top face in the same cyclic order.  Matches `VERTEX_SIGNS` in the
    /// outer module.
    const REF_VERTICES: [ReferenceCoord; 8] = [
        ReferenceCoord::new(-1.0, -1.0, -1.0), // v_0
        ReferenceCoord::new( 1.0, -1.0, -1.0), // v_1
        ReferenceCoord::new( 1.0,  1.0, -1.0), // v_2
        ReferenceCoord::new(-1.0,  1.0, -1.0), // v_3
        ReferenceCoord::new(-1.0, -1.0,  1.0), // v_4
        ReferenceCoord::new( 1.0, -1.0,  1.0), // v_5
        ReferenceCoord::new( 1.0,  1.0,  1.0), // v_6
        ReferenceCoord::new(-1.0,  1.0,  1.0), // v_7
    ];

    #[test]
    fn n_nodes_const_is_eight() {
        assert_eq!(<HexP1 as ReferenceElement>::N_NODES, 8);
    }

    #[test]
    fn shape_at_satisfies_kronecker_delta_at_eight_reference_cube_vertices() {
        for (i, v) in REF_VERTICES.iter().enumerate() {
            let n = HexP1.shape_at(*v);
            assert_eq!(n.len(), 8, "shape_at must return N_NODES=8 entries");
            for (j, &n_j) in n.iter().enumerate() {
                let expected = if i == j { 1.0_f64 } else { 0.0_f64 };
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
            ReferenceCoord::new(0.0, 0.0, 0.0),
            ReferenceCoord::new(0.3, -0.4, 0.2),
            ReferenceCoord::new(0.5, 0.5, 0.5),
        ];
        for p in &probes {
            let sum: f64 = HexP1.shape_at(*p).iter().sum();
            assert!(
                (sum - 1.0).abs() < TOL,
                "Σ N_i({:?}) = {sum}, expected 1.0",
                p,
            );
        }
    }

    // ── shape_grad_at tests ──────────────────────────────────────────────────

    #[test]
    fn shape_grad_at_returns_eight_rows_each_with_three_components() {
        let probe = ReferenceCoord::new(0.3, -0.4, 0.2);
        let g = HexP1.shape_grad_at(probe);
        assert_eq!(g.len(), 8, "shape_grad_at must return N_NODES=8 rows");
        for row in &g {
            assert_eq!(row.len(), 3, "each gradient row must have 3 components");
        }
    }

    #[test]
    fn shape_grad_at_matches_analytic_form_at_centroid_and_probes() {
        // At centroid (0,0,0): ∇N_i = (ξ_i, η_i, ζ_i) / 8.
        let centroid = ReferenceCoord::new(0.0, 0.0, 0.0);
        let g = HexP1.shape_grad_at(centroid);
        for (i, (grad, sign)) in g.iter().zip(VERTEX_SIGNS.iter()).enumerate() {
            for k in 0..3 {
                let expected = sign[k] / 8.0;
                assert!(
                    (grad[k] - expected).abs() < TOL,
                    "∇N_{i}(centroid)[{k}] = {}, expected {}",
                    grad[k],
                    expected,
                );
            }
        }

        // At off-centroid probe (0.3, -0.4, 0.2): verify analytic formula for
        // selected nodes.
        let probe = ReferenceCoord::new(0.3, -0.4, 0.2);
        let g2 = HexP1.shape_grad_at(probe);
        let xi = 0.3_f64;
        let eta = -0.4_f64;
        let zeta = 0.2_f64;
        for (i, (grad, s)) in g2.iter().zip(VERTEX_SIGNS.iter()).enumerate() {
            let (sx, sy, sz) = (s[0], s[1], s[2]);
            let expected = [
                (sx / 8.0) * (1.0 + sy * eta) * (1.0 + sz * zeta),
                (sy / 8.0) * (1.0 + sx * xi)  * (1.0 + sz * zeta),
                (sz / 8.0) * (1.0 + sx * xi)  * (1.0 + sy * eta),
            ];
            for k in 0..3 {
                assert!(
                    (grad[k] - expected[k]).abs() < TOL,
                    "∇N_{i}(probe)[{k}] = {}, expected {}",
                    grad[k],
                    expected[k],
                );
            }
        }
    }

    #[test]
    fn shape_grad_at_partition_of_unity_consequence() {
        // Σ_i ∇N_i = (0, 0, 0) — consequence of Σ N_i ≡ 1.
        let probes = [
            ReferenceCoord::new(0.0, 0.0, 0.0),
            ReferenceCoord::new(0.3, -0.4, 0.2),
            ReferenceCoord::new(-0.7, 0.5, -0.1),
        ];
        for p in &probes {
            let g = HexP1.shape_grad_at(*p);
            let mut sum = [0.0_f64; 3];
            for row in &g {
                for k in 0..3 {
                    sum[k] += row[k];
                }
            }
            for k in 0..3 {
                assert!(
                    sum[k].abs() < TOL,
                    "Σ_i ∇N_i({:?})[{k}] = {}, expected 0",
                    p,
                    sum[k],
                );
            }
        }
    }

    // ── quad_points tests ────────────────────────────────────────────────────

    const QUAD_TOL: f64 = 1e-10;

    #[test]
    fn quad_points_is_two_by_two_by_two_gauss_legendre_rule() {
        let qps = HexP1.quad_points();
        assert_eq!(qps.len(), 8, "2×2×2 Gauss rule must have 8 points");

        // All weights must be 1.0.
        for (i, qp) in qps.iter().enumerate() {
            assert!(
                (qp.weight - 1.0).abs() < QUAD_TOL,
                "qp[{i}].weight = {}, expected 1.0",
                qp.weight,
            );
        }

        // Each point must sit at one of the 8 sign-combinations of ±1/√3.
        let g = 1.0_f64 / 3.0_f64.sqrt();
        let expected_signs: [[f64; 3]; 8] = [
            [-1.0, -1.0, -1.0], [ 1.0, -1.0, -1.0],
            [-1.0,  1.0, -1.0], [ 1.0,  1.0, -1.0],
            [-1.0, -1.0,  1.0], [ 1.0, -1.0,  1.0],
            [-1.0,  1.0,  1.0], [ 1.0,  1.0,  1.0],
        ];
        for (i, qp) in qps.iter().enumerate() {
            let c = qp.coord;
            // Find a matching sign pattern by absolute-difference search.
            let found = expected_signs.iter().any(|s| {
                (c.xi   - s[0] * g).abs() < QUAD_TOL &&
                (c.eta  - s[1] * g).abs() < QUAD_TOL &&
                (c.zeta - s[2] * g).abs() < QUAD_TOL
            });
            assert!(
                found,
                "qp[{i}] = ({}, {}, {}) does not match any ±1/√3 sign-pattern",
                c.xi, c.eta, c.zeta,
            );
        }
    }

    #[test]
    fn quad_points_total_weight_is_cube_volume_eight() {
        let total: f64 = HexP1.quad_points().iter().map(|q| q.weight).sum();
        assert!(
            (total - 8.0).abs() < QUAD_TOL,
            "Σ weights = {total}, expected 8.0 (reference-cube volume)",
        );
    }

    #[test]
    fn quad_rule_integrates_constant_to_cube_volume() {
        // ∫_{[-1,1]³} 1 dV = 8.
        let i: f64 = HexP1.quad_points().iter().map(|q| q.weight * 1.0).sum();
        assert!((i - 8.0).abs() < QUAD_TOL, "∫ 1 dV = {i}, expected 8.0");
    }

    #[test]
    fn quad_rule_integrates_linear_xi_to_zero() {
        // ∫_{[-1,1]³} ξ dV = 0  (odd integrand on symmetric domain).
        let i: f64 = HexP1.quad_points().iter().map(|q| q.weight * q.coord.xi).sum();
        assert!(i.abs() < QUAD_TOL, "∫ ξ dV = {i}, expected 0.0");
    }

    #[test]
    fn quad_rule_integrates_xi_squared_to_eight_thirds() {
        // ∫_{[-1,1]³} ξ² dV = (2/3)·2·2 = 8/3.
        let i: f64 = HexP1.quad_points().iter().map(|q| q.weight * q.coord.xi.powi(2)).sum();
        assert!(
            (i - 8.0 / 3.0).abs() < QUAD_TOL,
            "∫ ξ² dV = {i}, expected {}",
            8.0 / 3.0,
        );
    }

    #[test]
    fn quad_rule_integrates_xi_eta_cross_term_to_zero() {
        // ∫_{[-1,1]³} ξη dV = 0  (odd in ξ and η independently).
        let i: f64 = HexP1
            .quad_points()
            .iter()
            .map(|q| q.weight * q.coord.xi * q.coord.eta)
            .sum();
        assert!(i.abs() < QUAD_TOL, "∫ ξη dV = {i}, expected 0.0");
    }

    #[test]
    fn quad_rule_integrates_xi_squared_eta_squared_zeta_squared_exactly() {
        // ∫_{[-1,1]³} ξ²η²ζ² dV = (2/3)³ = 8/27.
        // Verifies the rule is exact for the highest-degree monomial in
        // the trilinear Bᵀ D B stiffness integrand.
        let i: f64 = HexP1
            .quad_points()
            .iter()
            .map(|q| q.weight * q.coord.xi.powi(2) * q.coord.eta.powi(2) * q.coord.zeta.powi(2))
            .sum();
        assert!(
            (i - 8.0 / 27.0).abs() < QUAD_TOL,
            "∫ ξ²η²ζ² dV = {i}, expected {}",
            8.0 / 27.0,
        );
    }

    #[test]
    fn shape_grad_at_d_by_d_xi_is_independent_of_xi() {
        // ∂N_i/∂ξ = (ξ_i/8)(1 + η_i η)(1 + ζ_i ζ) has no ξ dependency.
        // Two probes that share (η, ζ) but differ in ξ must give the same
        // [0] components.
        let p1 = ReferenceCoord::new(0.0, 0.3, -0.4);
        let p2 = ReferenceCoord::new(0.5, 0.3, -0.4);
        let g1 = HexP1.shape_grad_at(p1);
        let g2 = HexP1.shape_grad_at(p2);
        for i in 0..8 {
            assert!(
                (g1[i][0] - g2[i][0]).abs() < TOL,
                "∂N_{i}/∂ξ should not depend on ξ: p1 gives {}, p2 gives {}",
                g1[i][0],
                g2[i][0],
            );
        }
    }
}
