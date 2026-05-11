//! First-order wedge/prism (P1 / wedge6) reference element.
//!
//! Linear 6-node triangular-prism element defined on the **reference prism**
//! = unit triangle × `[-1, +1]`:
//!
//! ```text
//! { (ξ, η, ζ) : ξ ≥ 0, η ≥ 0, ξ + η ≤ 1, ζ ∈ [-1, +1] }
//! ```
//!
//! Shape functions are tensor products of barycentric (triangle base) × linear
//! (sweep direction):
//!
//! ```text
//! N_i(ξ, η, ζ) = L_{a_i}(ξ, η) · (1 + s_i · ζ) / 2
//! ```
//!
//! where `L_0 = 1−ξ−η`, `L_1 = ξ`, `L_2 = η` are the barycentric functions
//! and `(a_i, s_i)` is the (barycentric-index, ζ-sign) pair for node `i`.
//!
//! Canonical Gmsh PRI6 node ordering — bottom face `(ζ = −1)` first, then top
//! face `(ζ = +1)` in the same cyclic barycentric order:
//!
//! | node | bary index | ζ sign | ref coords        |
//! |------|-----------|--------|-------------------|
//! | 0    | 0 (L₀)    | −1     | `(0, 0, −1)`      |
//! | 1    | 1 (L₁=ξ)  | −1     | `(1, 0, −1)`      |
//! | 2    | 2 (L₂=η)  | −1     | `(0, 1, −1)`      |
//! | 3    | 0 (L₀)    | +1     | `(0, 0, +1)`      |
//! | 4    | 1 (L₁=ξ)  | +1     | `(1, 0, +1)`      |
//! | 5    | 2 (L₂=η)  | +1     | `(0, 1, +1)`      |
//!
//! Right-handed orientation — this ordering produces `det J > 0` for an
//! unsheared prism. Reference-prism volume `= (1/2) × 2 = 1`.
//!
//! # API surface
//!
//! `VERTEX_BARY_ZETA` is a crate-internal constant — it is **not** part of
//! the published API:
//!
//! ```compile_fail,E0603
//! use reify_solver_elastic::elements::wedge_p1::VERTEX_BARY_ZETA;
//! let _ = VERTEX_BARY_ZETA;
//! ```

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// Gauss-Legendre 1/√3 coordinate for the 2-point line rule on `[-1, +1]`.
///
/// Written as a literal so the constant is usable in `const` context regardless
/// of MSRV's `f64::sqrt` const-stability status — mirrors the `HEX_P1_GAUSS_PT`
/// pattern in `hex_p1.rs`.
const WEDGE_P1_LINE_GAUSS_PT: f64 = 0.5773502691896257; // ≈ 1/√3

/// 3×2 tensor-product Gauss quadrature rule for the reference prism.
///
/// Triangle base: 3-point Gauss rule (degree-2 exact) at barycentric
/// `(2/3, 1/6, 1/6)` and its 2 cyclic permutations, each with triangle weight
/// `1/6` (sum = `1/2` = unit-triangle area).
///
/// Line sweep: 2-point Gauss-Legendre on `[-1, +1]` at `±1/√3`, each with
/// weight `1` (sum = `2` = line length, degree-3 exact).
///
/// Tensor product → 6 points, all with weight `(1/6)·1 = 1/6`.
/// Total weight = `1` = reference-prism volume `= (1/2)·2`.
/// Exact for degree-2-in-triangle × degree-3-in-line integrands — sufficient
/// for `BᵀDB` on a constant-Jacobian wedge.
const WEDGE_P1_QUAD: &[QuadraturePoint] = &[
    // Triangle point A: (ξ, η) = (2/3, 1/6), ζ = +1/√3
    QuadraturePoint {
        coord: ReferenceCoord::new(2.0 / 3.0, 1.0 / 6.0, WEDGE_P1_LINE_GAUSS_PT),
        weight: 1.0 / 6.0,
    },
    // Triangle point A: (ξ, η) = (2/3, 1/6), ζ = −1/√3
    QuadraturePoint {
        coord: ReferenceCoord::new(2.0 / 3.0, 1.0 / 6.0, -WEDGE_P1_LINE_GAUSS_PT),
        weight: 1.0 / 6.0,
    },
    // Triangle point B: (ξ, η) = (1/6, 2/3), ζ = +1/√3
    QuadraturePoint {
        coord: ReferenceCoord::new(1.0 / 6.0, 2.0 / 3.0, WEDGE_P1_LINE_GAUSS_PT),
        weight: 1.0 / 6.0,
    },
    // Triangle point B: (ξ, η) = (1/6, 2/3), ζ = −1/√3
    QuadraturePoint {
        coord: ReferenceCoord::new(1.0 / 6.0, 2.0 / 3.0, -WEDGE_P1_LINE_GAUSS_PT),
        weight: 1.0 / 6.0,
    },
    // Triangle point C: (ξ, η) = (1/6, 1/6), ζ = +1/√3
    QuadraturePoint {
        coord: ReferenceCoord::new(1.0 / 6.0, 1.0 / 6.0, WEDGE_P1_LINE_GAUSS_PT),
        weight: 1.0 / 6.0,
    },
    // Triangle point C: (ξ, η) = (1/6, 1/6), ζ = −1/√3
    QuadraturePoint {
        coord: ReferenceCoord::new(1.0 / 6.0, 1.0 / 6.0, -WEDGE_P1_LINE_GAUSS_PT),
        weight: 1.0 / 6.0,
    },
];

/// First-order Lagrangian triangular prism (wedge6).
pub struct WedgeP1;

/// `(barycentric-coord-index, ζ-sign)` for each of the 6 nodes in Gmsh
/// PRI6 ordering.
///
/// `bary_idx` ∈ {0, 1, 2} selects which barycentric function
/// (`L_0 = 1−ξ−η`, `L_1 = ξ`, `L_2 = η`) governs the triangle face.
/// `zeta_sign` ∈ {−1.0, +1.0} selects the sweep layer.
///
/// Single-source: used by both `shape_at` and `shape_grad_at` to prevent
/// per-method ordering drift — mirrors the `hex_p1::VERTEX_SIGNS` pattern.
pub(crate) const VERTEX_BARY_ZETA: [(usize, f64); 6] = [
    (0, -1.0), // node 0: L₀, ζ = −1  → (0, 0, −1)
    (1, -1.0), // node 1: L₁, ζ = −1  → (1, 0, −1)
    (2, -1.0), // node 2: L₂, ζ = −1  → (0, 1, −1)
    (0, 1.0),  // node 3: L₀, ζ = +1  → (0, 0, +1)
    (1, 1.0),  // node 4: L₁, ζ = +1  → (1, 0, +1)
    (2, 1.0),  // node 5: L₂, ζ = +1  → (0, 1, +1)
];

impl ReferenceElement for WedgeP1 {
    const N_NODES: usize = 6;

    /// Shape functions at `coord`.
    ///
    /// Returns `[N_0, …, N_5]` where
    /// `N_i(ξ, η, ζ) = L_{a_i}(ξ, η) · (1 + s_i · ζ) / 2`
    /// and `L_0 = 1−ξ−η`, `L_1 = ξ`, `L_2 = η`.
    fn shape_at(&self, coord: ReferenceCoord) -> Vec<f64> {
        let ReferenceCoord { xi, eta, zeta } = coord;
        let lambda = [1.0 - xi - eta, xi, eta];
        let mut n = Vec::with_capacity(6);
        for &(a, s) in &VERTEX_BARY_ZETA {
            n.push(lambda[a] * (1.0 + s * zeta) / 2.0);
        }
        n
    }

    /// Shape-function gradients in reference coordinates.
    ///
    /// Returns `[∇N_0, …, ∇N_5]` where each row is
    /// `[∂N_i/∂ξ, ∂N_i/∂η, ∂N_i/∂ζ]`.  Derived via the product rule:
    ///
    /// ```text
    /// ∂N_i/∂ξ = (∂L_{a_i}/∂ξ) · (1 + s_i ζ) / 2
    /// ∂N_i/∂η = (∂L_{a_i}/∂η) · (1 + s_i ζ) / 2
    /// ∂N_i/∂ζ = L_{a_i} · s_i / 2
    /// ```
    ///
    /// where `∇L_0 = (−1, −1, 0)`, `∇L_1 = (1, 0, 0)`, `∇L_2 = (0, 1, 0)`
    /// in `(ξ, η, ζ)` (barycentric functions are ζ-independent).
    fn shape_grad_at(&self, coord: ReferenceCoord) -> Vec<[f64; 3]> {
        let ReferenceCoord { xi, eta, zeta } = coord;
        let lambda = [1.0 - xi - eta, xi, eta];
        // Gradients of barycentric coordinates in (ξ, η, ζ):
        //   ∇L_0 = (-1, -1, 0),  ∇L_1 = (1, 0, 0),  ∇L_2 = (0, 1, 0)
        const GRAD_LAMBDA: [[f64; 3]; 3] = [[-1.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let mut g = Vec::with_capacity(6);
        for &(a, s) in &VERTEX_BARY_ZETA {
            let half_layer = (1.0 + s * zeta) / 2.0;
            g.push([
                GRAD_LAMBDA[a][0] * half_layer, // ∂N_i/∂ξ
                GRAD_LAMBDA[a][1] * half_layer, // ∂N_i/∂η
                lambda[a] * s / 2.0,            // ∂N_i/∂ζ
            ]);
        }
        g
    }

    fn quad_points(&self) -> &'static [QuadraturePoint] {
        WEDGE_P1_QUAD
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::needless_range_loop)] // matrix and parallel-index loops in test asserts
    use super::*;

    const TOL: f64 = 1e-12;

    /// Canonical reference-prism vertices in Gmsh PRI6 ordering.
    const REF_VERTICES: [ReferenceCoord; 6] = [
        ReferenceCoord::new(0.0, 0.0, -1.0), // v_0: L₀, ζ = −1
        ReferenceCoord::new(1.0, 0.0, -1.0), // v_1: L₁, ζ = −1
        ReferenceCoord::new(0.0, 1.0, -1.0), // v_2: L₂, ζ = −1
        ReferenceCoord::new(0.0, 0.0, 1.0),  // v_3: L₀, ζ = +1
        ReferenceCoord::new(1.0, 0.0, 1.0),  // v_4: L₁, ζ = +1
        ReferenceCoord::new(0.0, 1.0, 1.0),  // v_5: L₂, ζ = +1
    ];

    #[test]
    fn n_nodes_const_is_six() {
        assert_eq!(<WedgeP1 as ReferenceElement>::N_NODES, 6);
    }

    #[test]
    fn shape_at_satisfies_kronecker_delta_at_six_reference_prism_vertices() {
        for (i, v) in REF_VERTICES.iter().enumerate() {
            let n = WedgeP1.shape_at(*v);
            assert_eq!(n.len(), 6, "shape_at must return N_NODES=6 entries");
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
        // Centroid of unit triangle: (1/3, 1/3); mid sweep: ζ = 0.
        // Interior probes: all satisfy ξ > 0, η > 0, ξ+η < 1, |ζ| < 1.
        let probes = [
            ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0), // centroid
            ReferenceCoord::new(0.2, 0.3, 0.5),
            ReferenceCoord::new(0.4, 0.2, -0.6),
        ];
        for p in &probes {
            let sum: f64 = WedgeP1.shape_at(*p).iter().sum();
            assert!(
                (sum - 1.0).abs() < TOL,
                "Σ N_i({:?}) = {sum}, expected 1.0",
                p,
            );
        }
    }

    // ── shape_grad_at tests ──────────────────────────────────────────────────

    #[test]
    fn shape_grad_at_returns_six_rows_each_with_three_components() {
        let probe = ReferenceCoord::new(0.2, 0.3, 0.5);
        let g = WedgeP1.shape_grad_at(probe);
        assert_eq!(g.len(), 6, "shape_grad_at must return N_NODES=6 rows");
        // Row length is enforced by the `[f64; 3]` return type; no runtime check needed.
    }

    #[test]
    fn shape_grad_at_partition_of_unity_consequence() {
        // Σ_i ∇N_i = (0, 0, 0) — consequence of Σ N_i ≡ 1.
        let probes = [
            ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0),
            ReferenceCoord::new(0.2, 0.3, 0.5),
            ReferenceCoord::new(0.4, 0.2, -0.6),
        ];
        for p in &probes {
            let g = WedgeP1.shape_grad_at(*p);
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

    #[test]
    fn shape_grad_at_matches_simple_analytic_form_at_centroid() {
        // At centroid (1/3, 1/3, 0):
        //   λ = [1/3, 1/3, 1/3], (1 + s*0)/2 = 1/2 for all nodes.
        //   ∇L₀ = (-1,-1, 0), ∇L₁ = (1,0,0), ∇L₂ = (0,1,0).
        //   ∂N_i/∂ξ = ∇L_{a_i}[0] * 1/2
        //   ∂N_i/∂η = ∇L_{a_i}[1] * 1/2
        //   ∂N_i/∂ζ = λ[a_i] * s_i / 2  = (1/3) * s_i / 2
        //
        // Node ordering: (a, s) = (0,-1),(1,-1),(2,-1),(0,+1),(1,+1),(2,+1)
        // Expected [∂N/∂ξ, ∂N/∂η, ∂N/∂ζ] for each node:
        #[rustfmt::skip]
        let expected: [[f64; 3]; 6] = [
            [-0.5, -0.5, -1.0 / 6.0], // node 0: a=0,s=-1
            [ 0.5,  0.0, -1.0 / 6.0], // node 1: a=1,s=-1
            [ 0.0,  0.5, -1.0 / 6.0], // node 2: a=2,s=-1
            [-0.5, -0.5,  1.0 / 6.0], // node 3: a=0,s=+1
            [ 0.5,  0.0,  1.0 / 6.0], // node 4: a=1,s=+1
            [ 0.0,  0.5,  1.0 / 6.0], // node 5: a=2,s=+1
        ];
        let centroid = ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0);
        let g = WedgeP1.shape_grad_at(centroid);
        for (i, (grad, exp)) in g.iter().zip(expected.iter()).enumerate() {
            for k in 0..3 {
                assert!(
                    (grad[k] - exp[k]).abs() < TOL,
                    "∇N_{i}(centroid)[{k}] = {}, expected {}",
                    grad[k],
                    exp[k],
                );
            }
        }
    }

    // ── quad_points tests ────────────────────────────────────────────────────

    const QUAD_TOL: f64 = 1e-10;

    #[test]
    fn quad_points_is_three_by_two_gauss_rule() {
        let qps = WedgeP1.quad_points();
        assert_eq!(qps.len(), 6, "3×2 Gauss rule must have 6 points");

        // All weights must be 1/6.
        let expected_weight = 1.0_f64 / 6.0;
        for (i, qp) in qps.iter().enumerate() {
            assert!(
                (qp.weight - expected_weight).abs() < QUAD_TOL,
                "qp[{i}].weight = {}, expected {expected_weight}",
                qp.weight,
            );
        }

        // Triangle base: 3 Gauss points at (ξ, η) ∈ {(2/3,1/6),(1/6,2/3),(1/6,1/6)}.
        // Line sweep: 2 points at ζ = ±1/√3.
        // All 6 combinations (tri_idx, line_idx) must appear exactly once.
        let g = 1.0_f64 / 3.0_f64.sqrt();
        let tri_pts: [(f64, f64); 3] = [
            (2.0 / 3.0, 1.0 / 6.0),
            (1.0 / 6.0, 2.0 / 3.0),
            (1.0 / 6.0, 1.0 / 6.0),
        ];
        let zeta_pts: [f64; 2] = [g, -g];

        let mut seen = [[false; 2]; 3];
        for (qp_i, qp) in qps.iter().enumerate() {
            let c = qp.coord;
            // Find which triangle-point this (ξ,η) matches.
            let ti = tri_pts
                .iter()
                .position(|&(xi, eta)| {
                    (c.xi - xi).abs() < QUAD_TOL && (c.eta - eta).abs() < QUAD_TOL
                })
                .unwrap_or_else(|| {
                    panic!(
                        "qp[{qp_i}] = (ξ={}, η={}) does not match any triangle Gauss point",
                        c.xi, c.eta,
                    )
                });
            // Find which line-point this ζ matches.
            let li = zeta_pts
                .iter()
                .position(|&z| (c.zeta - z).abs() < QUAD_TOL)
                .unwrap_or_else(|| panic!("qp[{qp_i}] ζ={} does not match ±1/√3", c.zeta,));
            assert!(
                !seen[ti][li],
                "(tri={ti}, line={li}) pair matched more than once; second match at qp[{qp_i}]",
            );
            seen[ti][li] = true;
        }
        assert!(
            seen.iter().all(|row| row.iter().all(|&x| x)),
            "not all 6 (tri, line) combinations were covered by the quadrature rule",
        );
    }

    #[test]
    fn quad_points_total_weight_is_prism_volume_one() {
        let total: f64 = WedgeP1.quad_points().iter().map(|q| q.weight).sum();
        assert!(
            (total - 1.0).abs() < QUAD_TOL,
            "Σ weights = {total}, expected 1.0 (reference-prism volume)",
        );
    }

    #[test]
    fn quad_rule_integrates_constant_to_prism_volume() {
        // ∫_{prism} 1 dV = 1 (reference-prism volume = (1/2)·2 = 1).
        let i: f64 = WedgeP1.quad_points().iter().map(|q| q.weight * 1.0).sum();
        assert!((i - 1.0).abs() < QUAD_TOL, "∫ 1 dV = {i}, expected 1.0");
    }

    #[test]
    fn quad_rule_integrates_linear_zeta_to_zero() {
        // ∫ ζ dV = 0 (odd integrand on symmetric ζ ∈ [-1, +1]).
        let i: f64 = WedgeP1
            .quad_points()
            .iter()
            .map(|q| q.weight * q.coord.zeta)
            .sum();
        assert!(i.abs() < QUAD_TOL, "∫ ζ dV = {i}, expected 0.0");
    }

    #[test]
    fn quad_rule_integrates_zeta_squared_to_one_third() {
        // ∫_{prism} ζ² dV = (area of unit triangle) · ∫_{-1}^{1} ζ² dζ
        //                 = (1/2) · (2/3) = 1/3.
        let i: f64 = WedgeP1
            .quad_points()
            .iter()
            .map(|q| q.weight * q.coord.zeta.powi(2))
            .sum();
        assert!(
            (i - 1.0 / 3.0).abs() < QUAD_TOL,
            "∫ ζ² dV = {i}, expected {}",
            1.0 / 3.0,
        );
    }

    #[test]
    fn quad_rule_integrates_xi_to_one_third() {
        // ∫_{prism} ξ dV = ∫_T ξ dA · ∫_{-1}^{1} 1 dζ
        //                = (1/6) · 2 = 1/3.
        // (∫_T ξ dA over the unit triangle = 1/6.)
        let i: f64 = WedgeP1
            .quad_points()
            .iter()
            .map(|q| q.weight * q.coord.xi)
            .sum();
        assert!(
            (i - 1.0 / 3.0).abs() < QUAD_TOL,
            "∫ ξ dV = {i}, expected {}",
            1.0 / 3.0,
        );
    }

    #[test]
    fn quad_rule_integrates_xi_squared_zeta_squared_exactly() {
        // ∫_{prism} ξ²ζ² dV = ∫_T ξ² dA · ∫_{-1}^{1} ζ² dζ
        //                    = (1/12) · (2/3) = 1/18.
        // This is at the exactness limit (degree-2 in triangle, degree-3 in line).
        let i: f64 = WedgeP1
            .quad_points()
            .iter()
            .map(|q| q.weight * q.coord.xi.powi(2) * q.coord.zeta.powi(2))
            .sum();
        assert!(
            (i - 1.0 / 18.0).abs() < QUAD_TOL,
            "∫ ξ²ζ² dV = {i}, expected {}",
            1.0 / 18.0,
        );
    }

    // ── jacobian tests ───────────────────────────────────────────────────────

    const JAC_TOL: f64 = 1e-10;

    /// Build physical-node array by mapping each canonical reference-prism
    /// vertex `(ξ, η, ζ)` through `transform`.  Matches REF_VERTICES order.
    fn prism_phys_nodes(transform: impl Fn([f64; 3]) -> [f64; 3]) -> [[f64; 3]; 6] {
        let mut nodes = [[0.0_f64; 3]; 6];
        for (i, v) in REF_VERTICES.iter().enumerate() {
            nodes[i] = transform([v.xi, v.eta, v.zeta]);
        }
        nodes
    }

    #[test]
    fn jacobian_is_identity_for_reference_prism_phys_nodes() {
        // Physical nodes = reference-prism vertices ⇒ J = I, det = 1.
        let phys = prism_phys_nodes(|v| v);
        for probe in [
            ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0),
            ReferenceCoord::new(0.2, 0.3, 0.5),
        ] {
            let j = WedgeP1.jacobian(&phys, probe);
            for row in 0..3 {
                for col in 0..3 {
                    let expected = if row == col { 1.0_f64 } else { 0.0_f64 };
                    assert!(
                        (j.matrix[row][col] - expected).abs() < JAC_TOL,
                        "J[{row}][{col}] = {}, expected {expected}",
                        j.matrix[row][col],
                    );
                }
            }
            assert!(
                (j.det - 1.0).abs() < JAC_TOL,
                "det J = {}, expected 1.0",
                j.det,
            );
        }
    }

    #[test]
    fn jacobian_uniform_scale_is_constant_with_correct_det() {
        // Scale by s = 2: phys nodes = 2·ref ⇒ J = 2·I, det = 8.
        let s = 2.0_f64;
        let phys = prism_phys_nodes(|v| [s * v[0], s * v[1], s * v[2]]);
        for probe in [
            ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0),
            ReferenceCoord::new(0.2, 0.3, -0.4),
        ] {
            let j = WedgeP1.jacobian(&phys, probe);
            for row in 0..3 {
                for col in 0..3 {
                    let expected = if row == col { s } else { 0.0_f64 };
                    assert!(
                        (j.matrix[row][col] - expected).abs() < JAC_TOL,
                        "J[{row}][{col}] = {}, expected {expected}",
                        j.matrix[row][col],
                    );
                }
            }
            assert!(
                (j.det - s.powi(3)).abs() < JAC_TOL,
                "det J = {}, expected {}",
                j.det,
                s.powi(3),
            );
        }
    }

    #[test]
    fn jacobian_translation_only_yields_identity() {
        // Translate by (a, b, c): Jacobian contribution from translation is zero,
        // so J = I, det = 1.
        let (a, b, c) = (1.5_f64, -0.7, 2.0);
        let phys = prism_phys_nodes(|v| [v[0] + a, v[1] + b, v[2] + c]);
        let j = WedgeP1.jacobian(&phys, ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0));
        for row in 0..3 {
            for col in 0..3 {
                let expected = if row == col { 1.0_f64 } else { 0.0_f64 };
                assert!(
                    (j.matrix[row][col] - expected).abs() < JAC_TOL,
                    "translated J[{row}][{col}] = {}, expected {expected}",
                    j.matrix[row][col],
                );
            }
        }
        assert!(
            (j.det - 1.0).abs() < JAC_TOL,
            "det J of translation = {}",
            j.det
        );
    }

    #[test]
    fn jacobian_45_degree_rotation_in_xz_plane_yields_constant_rotation_matrix_det_one() {
        // Rotate by θ = π/4 in the xz-plane:
        // R = [[cos θ, 0, sin θ], [0, 1, 0], [-sin θ, 0, cos θ]].
        // For an affine map of the reference prism the Jacobian is constant and equals R.
        let theta = std::f64::consts::FRAC_PI_4;
        let (c, s) = (theta.cos(), theta.sin());
        let rotate = |v: [f64; 3]| [c * v[0] + s * v[2], v[1], -s * v[0] + c * v[2]];
        let phys = prism_phys_nodes(rotate);

        // Expected J = R (constant for all reference probes on a P1 prism).
        let r = [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]];

        for probe in [
            ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0),
            ReferenceCoord::new(0.2, 0.3, 0.5),
            ReferenceCoord::new(0.4, 0.1, -0.6),
        ] {
            let j = WedgeP1.jacobian(&phys, probe);
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
    fn jacobian_negative_det_for_swapped_bottom_face_nodes() {
        // Swap bottom-face nodes 1 and 2 — both at ζ = −1.  This reverses the
        // orientation of the bottom triangle, making det J < 0 in the bottom half
        // of the element (ζ < 0).  Unlike a linear tet (where any 2-node swap
        // gives a globally constant det = −1), the prism Jacobian varies with ζ
        // (det J = ζ for the identity-map swap), so we probe at ζ = −1/√3 where
        // det J ≈ −0.577 < 0.
        let mut phys = prism_phys_nodes(|v| v);
        phys.swap(1, 2);
        let probe = ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, -WEDGE_P1_LINE_GAUSS_PT);
        let j = WedgeP1.jacobian(&phys, probe);
        assert!(
            j.det < 0.0,
            "bottom-face swap (nodes 1↔2) must yield det J < 0 at ζ < 0 (got {})",
            j.det,
        );
    }

    /// Non-affine regression: top triangle scaled 2× in xy, bottom canonical.
    ///
    /// The physical map sends:
    ///   bottom (ζ = −1): (0,0,−1), (1,0,−1), (0,1,−1)   ← canonical
    ///   top    (ζ = +1): (0,0,+1), (2,0,+1), (0,2,+1)   ← 2× in xy
    ///
    /// At centroid probe (ξ=1/3, η=1/3, ζ), the layer-blending in
    /// `shape_grad_at` gives J[0][0] = ∂x/∂ξ = (3+ζ)/2, which varies
    /// linearly with ζ.  At ζ = ±0.7 the analytical values are:
    ///   J[0][0](−0.7) = 1.15,  det J = 1.3225
    ///   J[0][0](+0.7) = 1.85,  det J = 3.4225
    ///
    /// Affine-map tests (constant J) cannot detect a bug in the `(1+s·ζ)/2`
    /// blending factor; this test fails if that factor is removed.
    #[test]
    fn jacobian_zeta_varying_for_non_affine_top_scale() {
        // Top layer (v[2] > 0.5 ⇔ ζ = +1) scaled 2× in xy; bottom unchanged.
        // Threshold 0.5 (not 0.0) guards against accidental midplane nodes at ζ=0.
        let phys = prism_phys_nodes(|v| {
            if v[2] > 0.5 {
                [2.0 * v[0], 2.0 * v[1], v[2]]
            } else {
                v
            }
        });

        let j_neg = WedgeP1.jacobian(&phys, ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, -0.7));
        let j_pos = WedgeP1.jacobian(&phys, ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.7));

        assert!(
            j_neg.det > JAC_TOL,
            "det J at ζ=−0.7 must be positive (got {})",
            j_neg.det,
        );
        assert!(
            j_pos.det > JAC_TOL,
            "det J at ζ=+0.7 must be positive (got {})",
            j_pos.det,
        );

        // Tight oracle assertions: exact analytical values at the two probes.
        // J[0][0](ζ) = (3+ζ)/2  ⟹  J[0][0](−0.7)=1.15, J[0][0](+0.7)=1.85.
        // det J(ζ) = ((3+ζ)/2)²  ⟹  det(−0.7)=1.3225, det(+0.7)=3.4225.
        // These subsume the ζ-variation check: the two J[0][0] values differ by 0.7.
        assert!(
            (j_neg.matrix[0][0] - 1.15).abs() < JAC_TOL,
            "J[0][0] at ζ=−0.7: expected 1.15, got {}",
            j_neg.matrix[0][0],
        );
        assert!(
            (j_pos.matrix[0][0] - 1.85).abs() < JAC_TOL,
            "J[0][0] at ζ=+0.7: expected 1.85, got {}",
            j_pos.matrix[0][0],
        );
        assert!(
            (j_neg.det - 1.3225).abs() < JAC_TOL,
            "det J at ζ=−0.7: expected 1.3225, got {}",
            j_neg.det,
        );
        assert!(
            (j_pos.det - 3.4225).abs() < JAC_TOL,
            "det J at ζ=+0.7: expected 3.4225, got {}",
            j_pos.det,
        );
    }

    #[test]
    fn shape_grad_at_matches_finite_difference_oracle_at_off_centroid_probes() {
        // FD oracle: central-difference truncation O(h²) ≈ 1e-12 + roundoff
        // O(ε·|f|/h) ≈ 1e-10; 1e-9 comfortably above.
        //
        // Probes stay strictly inside the reference prism:
        //   ξ,η ≥ 0.1, ξ+η ≤ 0.7, |ζ| ≤ 0.7 — so coord ± h·ê_k
        //   stays inside the domain for h = 1e-6.
        const FD_H: f64 = 1e-6;
        const FD_TOL: f64 = 1e-9;

        let probes = [
            ReferenceCoord::new(0.2, 0.3, 0.5),
            ReferenceCoord::new(0.4, 0.2, -0.6),
            ReferenceCoord::new(0.1, 0.1, 0.7),
        ];

        for probe in &probes {
            let grad = WedgeP1.shape_grad_at(*probe);
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
                let n_plus = WedgeP1.shape_at(coord_plus);
                let n_minus = WedgeP1.shape_at(coord_minus);
                for i in 0..6 {
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
}
