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
//!
//! # API surface
//!
//! `VERTEX_SIGNS` is a crate-internal constant — it is **not** part of the
//! published API:
//!
//! ```compile_fail,E0603
//! use reify_solver_elastic::elements::hex_p1::VERTEX_SIGNS;
//! let _ = VERTEX_SIGNS;
//! ```

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
pub(crate) const VERTEX_SIGNS: [[f64; 3]; 8] = [
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
    fn shape_grad_at_matches_simple_analytic_form_at_centroid() {
        // At the centroid (0,0,0) the trilinear product-rule terms all simplify
        // to 1·1 factors, giving ∇N_i = (ξ_i, η_i, ζ_i) / 8.  This is a
        // genuinely independent check — no product-rule terms are involved — and
        // it pins the simplest case explicitly.
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
    }

    #[test]
    fn shape_grad_at_matches_finite_difference_oracle_at_off_centroid_probes() {
        // FD oracle: central-difference truncation O(h²) ≈ 1e-12 + roundoff
        // O(ε·|f|/h) ≈ 1e-10; 1e-9 is comfortably above the sum.
        //
        // Why FD rather than the closed form: the closed form mirrors
        // shape_grad_at's product expression bit-for-bit and can only catch
        // refactor typos — any analytic-derivation error propagates identically
        // to both sides.  FD compares against shape_at (a different function
        // whose correctness is pinned by Kronecker-delta and partition-of-unity
        // tests), so the oracle is independent and will detect wrong math.
        const FD_H: f64 = 1e-6;
        const FD_TOL: f64 = 1e-9;

        // All probes are strictly interior: |coord_k| ≤ 0.7, so coord ± h·ê_k
        // stays safely inside [-1, 1]³ for h = 1e-6.
        let probes = [
            ReferenceCoord::new(0.3, -0.4, 0.2),
            ReferenceCoord::new(-0.7, 0.5, -0.1),
            ReferenceCoord::new(0.5, 0.5, 0.5),
        ];

        for probe in &probes {
            let grad = HexP1.shape_grad_at(*probe);
            for k in 0..3 {
                let coord_plus = match k {
                    0 => ReferenceCoord::new(probe.xi + FD_H, probe.eta, probe.zeta),
                    1 => ReferenceCoord::new(probe.xi, probe.eta + FD_H, probe.zeta),
                    2 => ReferenceCoord::new(probe.xi, probe.eta, probe.zeta + FD_H),
                    _ => unreachable!(),
                };
                let coord_minus = match k {
                    0 => ReferenceCoord::new(probe.xi - FD_H, probe.eta, probe.zeta),
                    1 => ReferenceCoord::new(probe.xi, probe.eta - FD_H, probe.zeta),
                    2 => ReferenceCoord::new(probe.xi, probe.eta, probe.zeta - FD_H),
                    _ => unreachable!(),
                };
                let n_plus = HexP1.shape_at(coord_plus);
                let n_minus = HexP1.shape_at(coord_minus);
                for i in 0..8 {
                    let fd_approx = (n_plus[i] - n_minus[i]) / (2.0 * FD_H);
                    assert!(
                        (fd_approx - grad[i][k]).abs() < FD_TOL,
                        "FD∇N_{i}({:?})[{k}]: fd={fd_approx}, analytic={}, diff={}",
                        probe,
                        grad[i][k],
                        (fd_approx - grad[i][k]).abs(),
                    );
                }
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
        // Verify each of the 8 sign-patterns is covered by exactly one qp.
        // Using a `seen` bitfield guarantees no duplicate coverage (a degenerate
        // rule where all 8 points land on the same vertex would fail here even if
        // every point passes the `any(...)` check above).
        let mut seen = [false; 8];
        for (i, qp) in qps.iter().enumerate() {
            let c = qp.coord;
            let idx = expected_signs
                .iter()
                .position(|s| {
                    (c.xi   - s[0] * g).abs() < QUAD_TOL &&
                    (c.eta  - s[1] * g).abs() < QUAD_TOL &&
                    (c.zeta - s[2] * g).abs() < QUAD_TOL
                })
                .unwrap_or_else(|| {
                    panic!(
                        "qp[{i}] = ({}, {}, {}) does not match any ±1/√3 sign-pattern",
                        c.xi, c.eta, c.zeta,
                    )
                });
            assert!(
                !seen[idx],
                "sign-pattern {:?} (index {idx}) matched more than once; second match at qp[{i}]",
                expected_signs[idx],
            );
            seen[idx] = true;
        }
        assert!(
            seen.iter().all(|&x| x),
            "not all 8 ±1/√3 sign-patterns were covered by the quadrature rule",
        );
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
        // Verifies the rule is exact when every axis is at its degree-2 limit —
        // a strictly harder integrand than any monomial that actually appears in
        // BᵀDB on a constant-Jacobian hex (each B = ∇N component is bilinear in
        // only the two axes orthogonal to its differentiation axis, so no BᵀDB
        // monomial reaches degree 2 on all three axes simultaneously).
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

    // ── jacobian tests ───────────────────────────────────────────────────────

    const JAC_TOL: f64 = 1e-10;

    /// Build physical-node array by mapping each canonical reference cube
    /// vertex `(ξ_i, η_i, ζ_i)` through `transform`.
    fn cube_phys_nodes(transform: impl Fn([f64; 3]) -> [f64; 3]) -> [[f64; 3]; 8] {
        let mut nodes = [[0.0_f64; 3]; 8];
        for (i, s) in VERTEX_SIGNS.iter().enumerate() {
            nodes[i] = transform([s[0], s[1], s[2]]);
        }
        nodes
    }

    #[test]
    fn jacobian_is_identity_for_reference_cube_phys_nodes() {
        // Physical nodes = reference-cube corners ⇒ J = I, det = 1.
        let phys = cube_phys_nodes(|v| v);
        for probe in [
            ReferenceCoord::new(0.0, 0.0, 0.0),
            ReferenceCoord::new(0.3, -0.4, 0.2),
        ] {
            let j = HexP1.jacobian(&phys, probe);
            for row in 0..3 {
                for col in 0..3 {
                    let expected = if row == col { 1.0_f64 } else { 0.0_f64 };
                    assert!(
                        (j.matrix[row][col] - expected).abs() < JAC_TOL,
                        "J[{row}][{col}] = {}, expected {}",
                        j.matrix[row][col],
                        expected,
                    );
                }
            }
            assert!((j.det - 1.0).abs() < JAC_TOL, "det J = {}, expected 1.0", j.det);
        }
    }

    #[test]
    fn jacobian_uniform_scale_is_constant_with_correct_det() {
        // Scale by s = 2: phys nodes at 2·(±1,±1,±1) ⇒ J = 2·I, det = 8.
        let s = 2.0_f64;
        let phys = cube_phys_nodes(|v| [s * v[0], s * v[1], s * v[2]]);
        for probe in [
            ReferenceCoord::new(0.0, 0.0, 0.0),
            ReferenceCoord::new(-0.5, 0.3, 0.7),
        ] {
            let j = HexP1.jacobian(&phys, probe);
            for row in 0..3 {
                for col in 0..3 {
                    let expected = if row == col { s } else { 0.0_f64 };
                    assert!(
                        (j.matrix[row][col] - expected).abs() < JAC_TOL,
                        "J[{row}][{col}] = {}, expected {}",
                        j.matrix[row][col],
                        expected,
                    );
                }
            }
            assert!((j.det - s.powi(3)).abs() < JAC_TOL, "det J = {}, expected {}", j.det, s.powi(3));
        }
    }

    #[test]
    fn jacobian_translation_only_yields_identity() {
        // Translate by (a, b, c): J = I (translation has zero Jacobian contribution).
        let (a, b, c) = (1.5_f64, -0.7, 2.0);
        let phys = cube_phys_nodes(|v| [v[0] + a, v[1] + b, v[2] + c]);
        let j = HexP1.jacobian(&phys, ReferenceCoord::new(0.0, 0.0, 0.0));
        for row in 0..3 {
            for col in 0..3 {
                let expected = if row == col { 1.0_f64 } else { 0.0_f64 };
                assert!(
                    (j.matrix[row][col] - expected).abs() < JAC_TOL,
                    "translated J[{row}][{col}] = {}, expected {}",
                    j.matrix[row][col],
                    expected,
                );
            }
        }
        assert!((j.det - 1.0).abs() < JAC_TOL);
    }

    #[test]
    fn jacobian_45_degree_rotation_in_xz_plane_yields_constant_rotation_matrix_det_one() {
        // Rotate by θ = π/4 in the xz-plane:
        // R = [[cos θ, 0, sin θ], [0, 1, 0], [-sin θ, 0, cos θ]].
        // For a straight-edge hex the Jacobian is constant and equals R.
        let theta = std::f64::consts::FRAC_PI_4;
        let (c, s) = (theta.cos(), theta.sin());
        // Rotation matrix R:
        //   [c  0  s]
        //   [0  1  0]
        //   [-s 0  c]
        let rotate = |v: [f64; 3]| [c * v[0] + s * v[2], v[1], -s * v[0] + c * v[2]];
        let phys = cube_phys_nodes(rotate);

        // Expected J = R (constant for all reference probes on a straight-edge hex).
        let r = [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]];

        for probe in [
            ReferenceCoord::new(0.0, 0.0, 0.0),
            ReferenceCoord::new(0.3, -0.4, 0.2),
            ReferenceCoord::new(-0.7, 0.5, -0.1),
        ] {
            let j = HexP1.jacobian(&phys, probe);
            for row in 0..3 {
                for col in 0..3 {
                    assert!(
                        (j.matrix[row][col] - r[row][col]).abs() < JAC_TOL,
                        "rotated J[{row}][{col}] = {}, expected {}",
                        j.matrix[row][col],
                        r[row][col],
                    );
                }
            }
            assert!(
                (j.det - 1.0).abs() < JAC_TOL,
                "det J of rotation = {}, expected 1.0",
                j.det,
            );
        }
    }

    #[test]
    fn jacobian_negative_det_for_swapped_node_ordering() {
        // Swap v_0 (-1,-1,-1) and v_6 (+1,+1,+1) — diagonally opposite
        // vertices that differ in ALL three coordinate signs.  This creates
        // a "twisted" hex that is inside-out, producing det J < 0.
        //
        // Note: swapping adjacent nodes (e.g. v_0 ↔ v_1) that differ in
        // only one coordinate gives det J = 1/2 > 0 for the bilinear hex8
        // (unlike for linear tets where any node swap negates the det).
        // Only opposite-corner swaps reverse orientation.
        let mut phys = cube_phys_nodes(|v| v);
        phys.swap(0, 6);
        let j = HexP1.jacobian(&phys, ReferenceCoord::new(0.0, 0.0, 0.0));
        assert!(
            j.det < 0.0,
            "opposite-corner swap must yield det J < 0 (got {})",
            j.det,
        );
    }
}
