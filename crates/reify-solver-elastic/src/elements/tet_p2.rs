//! Second-order tetrahedron (P2) reference element.
//!
//! Quadratic Lagrangian tet with 10 nodes: 4 at the reference vertices
//! `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` and 6 at the midpoints of the
//! edges in the canonical Hughes/Gmsh ordering
//! `(0,1), (1,2), (2,0), (0,3), (1,3), (2,3)`.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// Second-order Lagrangian tetrahedron.
pub struct TetP2;

/// 4-point Stroud (1971) symmetric Gauss rule for the unit reference
/// tetrahedron — degree-2 exact.
///
/// The two parameters are
///
/// - `a = (5 − √5) / 20 ≈ 0.138196601125010515…`
/// - `b = (5 + 3√5) / 20 ≈ 0.585410196624968454…`
///
/// with quadrature points at the symmetric layout
/// `(a,a,a), (b,a,a), (a,b,a), (a,a,b)` and equal weights `1/24`. The
/// total weight `4/24 = 1/6` matches the reference-tet volume.
///
/// Hard-coded to 17 significant figures rather than computed at runtime:
/// `f64::sqrt` is not `const fn`, and a `OnceLock` for a 4-entry static
/// slice would be needless ceremony. The literals match
/// `(5 ± k √5) / 20` rounded to nearest representable `f64` (within 1 ulp
/// of `5.0_f64.sqrt()` evaluated at runtime — see the
/// `quad_points_is_four_point_stroud_rule` test).
///
/// Degree-2 is sufficient for stiffness assembly with **straight-edge**
/// P2 elements: the geometric Jacobian is constant per element, so the
/// integrand `Bᵀ D B` (with `B = ∇N` linear in reference coords) is a
/// degree-2 polynomial which the Stroud rule integrates exactly. Curved-
/// edge P2 (deferred to v0.4+) would need the 11-point degree-4 rule.
const TET_P2_STROUD_A: f64 = 0.13819660112501052;
const TET_P2_STROUD_B: f64 = 0.585_410_196_624_968_4;
const TET_P2_QUAD: &[QuadraturePoint] = &[
    QuadraturePoint {
        coord: ReferenceCoord::new(TET_P2_STROUD_A, TET_P2_STROUD_A, TET_P2_STROUD_A),
        weight: 1.0 / 24.0,
    },
    QuadraturePoint {
        coord: ReferenceCoord::new(TET_P2_STROUD_B, TET_P2_STROUD_A, TET_P2_STROUD_A),
        weight: 1.0 / 24.0,
    },
    QuadraturePoint {
        coord: ReferenceCoord::new(TET_P2_STROUD_A, TET_P2_STROUD_B, TET_P2_STROUD_A),
        weight: 1.0 / 24.0,
    },
    QuadraturePoint {
        coord: ReferenceCoord::new(TET_P2_STROUD_A, TET_P2_STROUD_A, TET_P2_STROUD_B),
        weight: 1.0 / 24.0,
    },
];

/// Canonical edge ordering for the P2 reference tet's 6 edge midpoints,
/// as `(a, b)` index pairs into the 4 reference vertices.
///
/// Edge index 0..=5 maps to the corresponding entry here (Hughes/Gmsh
/// ordering: bottom-face edges first, then vertical edges to vertex 3).
/// Both `shape_at` and `shape_grad_at` consult this table so the edge
/// indexing stays single-sourced.
pub const EDGES: [(usize, usize); 6] = [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)];

/// Reference-coordinate gradients of the barycentric coordinates λ.
/// `∇λ_0 = (-1,-1,-1)` (since `λ_0 = 1-ξ-η-ζ`), `∇λ_1 = e_x`,
/// `∇λ_2 = e_y`, `∇λ_3 = e_z`.
const GRAD_LAMBDA: [[f64; 3]; 4] = [
    [-1.0, -1.0, -1.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

impl ReferenceElement for TetP2 {
    const N_NODES: usize = 10;

    // `jacobian(...)` is inherited from the trait default: the formula
    // `J_ij = Σ_k phys_nodes[k][i] · shape_grad_at(coord)[k][j]` is
    // N-agnostic via the `Vec<[f64; 3]>` return type, so no per-element
    // override is needed for P2. Callers must supply `phys_nodes` of
    // length `N_NODES = 10` in canonical order (4 vertices in
    // `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` order followed by the 6
    // edge-midpoint nodes in `EDGES` order). Verified by the
    // `jacobian_uniform_scale_is_constant_with_correct_det` and
    // `jacobian_p2_agrees_with_p1_for_affine_map` tests.

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

    /// Quadratic Lagrangian P2 shape-function gradients in reference
    /// coordinates, evaluated at `coord`.
    ///
    /// Computed via the chain rule from the barycentric coordinates
    /// `λ_0 = 1-ξ-η-ζ`, `λ_1 = ξ`, `λ_2 = η`, `λ_3 = ζ`, whose gradients
    /// in `(ξ, η, ζ)` are
    ///
    /// - `∇λ_0 = (-1, -1, -1)`,
    /// - `∇λ_1 = (1, 0, 0)`,
    /// - `∇λ_2 = (0, 1, 0)`,
    /// - `∇λ_3 = (0, 0, 1)`.
    ///
    /// Vertex-node gradient: `∇N_i = (4 λ_i − 1) · ∇λ_i` for `i ∈ 0..=3`.
    /// Edge-node gradient: `∇N = 4 (λ_a ∇λ_b + λ_b ∇λ_a)` for the edge
    /// `(a, b) = EDGES[edge_index]`.
    ///
    /// Gradients are degree-1 polynomials in `(ξ, η, ζ)` — see the
    /// `shape_grad_at_varies_linearly_in_reference_coords` test.
    fn shape_grad_at(&self, coord: ReferenceCoord) -> Vec<[f64; 3]> {
        let ReferenceCoord { xi, eta, zeta } = coord;
        let lambda = [1.0 - xi - eta - zeta, xi, eta, zeta];

        let mut g = Vec::with_capacity(10);
        // Vertex-node gradients: ∇N_i = (4 λ_i − 1) ∇λ_i.
        for i in 0..4 {
            let scale = 4.0 * lambda[i] - 1.0;
            g.push([
                scale * GRAD_LAMBDA[i][0],
                scale * GRAD_LAMBDA[i][1],
                scale * GRAD_LAMBDA[i][2],
            ]);
        }
        // Edge-node gradients: ∇N = 4 (λ_a ∇λ_b + λ_b ∇λ_a).
        for &(a, b) in &EDGES {
            g.push([
                4.0 * (lambda[a] * GRAD_LAMBDA[b][0] + lambda[b] * GRAD_LAMBDA[a][0]),
                4.0 * (lambda[a] * GRAD_LAMBDA[b][1] + lambda[b] * GRAD_LAMBDA[a][1]),
                4.0 * (lambda[a] * GRAD_LAMBDA[b][2] + lambda[b] * GRAD_LAMBDA[a][2]),
            ]);
        }
        g
    }

    fn quad_points(&self) -> &'static [QuadraturePoint] {
        TET_P2_QUAD
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
    fn shape_grad_at_vertex_nodes_match_chain_rule_at_centroid() {
        // At centroid: λ_i = 1/4 for i=0..3, so 4λ_i − 1 = 0 ⇒ all
        // vertex-node gradients vanish. Verify the closed-form value.
        let centroid = ReferenceCoord::new(0.25, 0.25, 0.25);
        let g = TetP2.shape_grad_at(centroid);
        assert_eq!(g.len(), 10);
        for (i, row) in g.iter().enumerate().take(4) {
            for (k, val) in row.iter().enumerate() {
                assert!(
                    val.abs() < TOL,
                    "∇N_{i}(centroid)[{k}] = {val} expected 0 (4λ_i−1 = 0 at centroid)",
                );
            }
        }

        // Off-centroid probe: sanity-check the chain rule analytically
        // for vertex node 0. ∇N_0 = (4 λ_0 − 1) ∇λ_0 with λ_0 = 1-ξ-η-ζ.
        let p = ReferenceCoord::new(0.1, 0.2, 0.15);
        let lambda_0 = 1.0 - p.xi - p.eta - p.zeta;
        let scalar = 4.0 * lambda_0 - 1.0;
        let g_p = TetP2.shape_grad_at(p);
        // Hard-coded literal oracle for ∇λ_0 = (-1,-1,-1) so that a typo in
        // GRAD_LAMBDA[0] is caught rather than silently passed; both sides of
        // the assertion now reference independent sources of truth.
        let grad_lambda_0_oracle = [-1.0_f64, -1.0, -1.0];
        for k in 0..3 {
            let expected = scalar * grad_lambda_0_oracle[k];
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
                for (k, rk) in row.iter().enumerate() {
                    sum[k] += rk;
                }
            }
            for (k, s) in sum.iter().enumerate() {
                assert!(s.abs() < TOL, "Σ_i ∇N_i({p:?})[{k}] = {s}, expected 0",);
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

    /// Quadrature tolerance for rule-property assertions: per-point weight,
    /// total-weight sum, Stroud-point multiset match, and monomial-integration
    /// checks.  The transcription of `TET_P2_STROUD_A`/`B` against the
    /// canonical runtime formula is verified separately (with a tight 4 × ε
    /// bound) in `quad_points_is_four_point_stroud_rule`.
    const QUAD_TOL: f64 = 1e-10;

    #[test]
    fn quad_points_is_four_point_stroud_rule() {
        // Stroud (1971) symmetric degree-2 rule on the unit tet:
        // a = (5 - √5)/20, b = (5 + 3√5)/20, weight 1/24 each.
        let qp = TetP2.quad_points();
        assert_eq!(qp.len(), 4, "P2 quadrature is a 4-point Stroud rule");

        let sqrt5 = 5.0_f64.sqrt();
        let a = (5.0 - sqrt5) / 20.0;
        let b = (5.0 + 3.0 * sqrt5) / 20.0;

        // Transcription guard: check the hard-coded literals against the
        // runtime formula with a 4 × ε budget that covers the rounding chain
        // (sqrt → sub → div).  This is a separate concern from the multiset
        // check below — it locks the source-comment claim ("within 1 ulp of
        // √5 at runtime") into CI without coupling to any single bit pattern.
        assert!(
            (TET_P2_STROUD_A - a).abs() <= 4.0 * f64::EPSILON,
            "TET_P2_STROUD_A ({}) is not within 4 ulp of (5-√5)/20 ({})",
            TET_P2_STROUD_A,
            a
        );
        assert!(
            (TET_P2_STROUD_B - b).abs() <= 4.0 * f64::EPSILON,
            "TET_P2_STROUD_B ({}) is not within 4 ulp of (5+3√5)/20 ({})",
            TET_P2_STROUD_B,
            b
        );

        let expected_pts = [(a, a, a), (b, a, a), (a, b, a), (a, a, b)];

        // Rule-property check: match each expected point to a quadrature
        // entry (ordering unspecified — the rule is symmetric, only the
        // multiset matters).  Uses the loose QUAD_TOL so this check is robust
        // to re-derivation of the constants via a mathematically equivalent
        // formula.
        for (xi_e, eta_e, zeta_e) in expected_pts {
            let found = qp.iter().any(|q| {
                (q.coord.xi - xi_e).abs() < QUAD_TOL
                    && (q.coord.eta - eta_e).abs() < QUAD_TOL
                    && (q.coord.zeta - zeta_e).abs() < QUAD_TOL
                    && (q.weight - 1.0 / 24.0).abs() < QUAD_TOL
            });
            assert!(
                found,
                "Stroud point ({xi_e}, {eta_e}, {zeta_e}) with weight 1/24 not found in {qp:?}"
            );
        }

        // Total weight = reference-tet volume = 1/6.
        let w_sum: f64 = qp.iter().map(|q| q.weight).sum();
        assert!((w_sum - 1.0 / 6.0).abs() < QUAD_TOL);
    }

    #[test]
    fn quad_rule_integrates_constant_to_reference_volume() {
        // ∫_T 1 dV = 1/6.
        let qp = TetP2.quad_points();
        let i: f64 = qp.iter().map(|q| q.weight).sum();
        assert!((i - 1.0 / 6.0).abs() < QUAD_TOL);
    }

    #[test]
    fn quad_rule_integrates_linear_xi_exactly() {
        // ∫_T ξ dV = 1/24 (degree-1 — exact for any rule with degree ≥ 1).
        let qp = TetP2.quad_points();
        let i: f64 = qp.iter().map(|q| q.weight * q.coord.xi).sum();
        assert!((i - 1.0 / 24.0).abs() < QUAD_TOL);
    }

    #[test]
    fn quad_rule_integrates_quadratic_xi_squared_exactly() {
        // ∫_T ξ² dV = 1/60 (analytical — degree-2 Stroud is exact for
        // quadratic monomials).
        let qp = TetP2.quad_points();
        let i: f64 = qp.iter().map(|q| q.weight * q.coord.xi * q.coord.xi).sum();
        assert!(
            (i - 1.0 / 60.0).abs() < QUAD_TOL,
            "∫ ξ² dV = {i}, expected 1/60 = {}",
            1.0 / 60.0
        );
    }

    /// Jacobian tolerance — slightly looser to accommodate the floating-
    /// point cancellation in the 10-term `J_ij = Σ_k phys[k][i] · g[k][j]`
    /// sum at non-trivial probe points.
    const JAC_TOL: f64 = 1e-10;

    /// Build a 10-node physical-node array for a uniformly scaled tet:
    /// 4 vertices at `(0,0,0), (s,0,0), (0,s,0), (0,0,s)` and 6 edge
    /// midpoints in the canonical Hughes/Gmsh edge ordering.
    fn scaled_tet_phys_nodes(s: f64) -> [[f64; 3]; 10] {
        let v: [[f64; 3]; 4] = [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]];
        let mid = |a: usize, b: usize| {
            [
                0.5 * (v[a][0] + v[b][0]),
                0.5 * (v[a][1] + v[b][1]),
                0.5 * (v[a][2] + v[b][2]),
            ]
        };
        [
            v[0],
            v[1],
            v[2],
            v[3],
            mid(0, 1),
            mid(1, 2),
            mid(2, 0),
            mid(0, 3),
            mid(1, 3),
            mid(2, 3),
        ]
    }

    #[test]
    fn jacobian_uniform_scale_is_constant_with_correct_det() {
        // Uniformly scaled (×2) tet: J should be diag(2,2,2) and det = 8
        // at *every* reference point (straight-edge P2 → constant
        // Jacobian).
        let phys = scaled_tet_phys_nodes(2.0);
        let probes = [
            ReferenceCoord::new(0.25, 0.25, 0.25), // centroid
            ReferenceCoord::new(0.5, 0.0, 0.0),    // edge (0,1) midpoint
            ReferenceCoord::new(0.1, 0.2, 0.15),   // interior probe
        ];
        for p in probes {
            let j = TetP2.jacobian(&phys, p);
            for i in 0..3 {
                for k in 0..3 {
                    let expected = if i == k { 2.0 } else { 0.0 };
                    assert!(
                        (j.matrix[i][k] - expected).abs() < JAC_TOL,
                        "J({:?})[{i}][{k}] = {} expected {}",
                        p,
                        j.matrix[i][k],
                        expected,
                    );
                }
            }
            assert!(
                (j.det - 8.0).abs() < JAC_TOL,
                "det J({:?}) = {}, expected 8",
                p,
                j.det,
            );
        }
    }

    #[test]
    fn jacobian_p2_agrees_with_p1_for_affine_map() {
        // For an affine map (straight-edge P2), the Jacobian is independent
        // of the edge-node coordinates and matches P1's Jacobian on the same
        // 4 vertices.
        use crate::elements::tet_p1::TetP1;

        let phys_p2 = scaled_tet_phys_nodes(2.0);
        let phys_p1: [[f64; 3]; 4] = [phys_p2[0], phys_p2[1], phys_p2[2], phys_p2[3]];

        let coord = ReferenceCoord::new(0.25, 0.25, 0.25);
        let j_p2 = TetP2.jacobian(&phys_p2, coord);
        let j_p1 = TetP1.jacobian(&phys_p1, coord);

        for i in 0..3 {
            for k in 0..3 {
                assert!(
                    (j_p2.matrix[i][k] - j_p1.matrix[i][k]).abs() < JAC_TOL,
                    "P2 J[{i}][{k}] = {} disagrees with P1 J[{i}][{k}] = {}",
                    j_p2.matrix[i][k],
                    j_p1.matrix[i][k],
                );
            }
        }
        assert!((j_p2.det - j_p1.det).abs() < JAC_TOL);
    }

    /// Exercises the trait-default `jacobian` length precondition via TetP2
    /// (N_NODES = 10).  Passing P1's 4-vertex layout catches a regression
    /// where a future P2 `jacobian` override forgets the precondition check.
    #[test]
    #[should_panic(expected = "phys_nodes.len() must equal Self::N_NODES")]
    fn jacobian_panics_on_p1_sized_phys_nodes_for_p2() {
        // 4 nodes instead of 10 — the canonical P1 reference vertices
        let phys: &[[f64; 3]] = &[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        TetP2.jacobian(phys, ReferenceCoord::new(0.25, 0.25, 0.25));
    }
}
