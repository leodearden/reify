//! P2 (quadratic, 10-node) tetrahedron consistent mass-matrix kernel.
//!
//! Task 4066 — "P2-tet modal frequencies": the P1 constant-strain tet
//! (`crate::mass_matrix::consistent_element_mass_tet_p1`) locks in bending,
//! flooring slender-beam modal-frequency error several percent above the 2%
//! aspirational target. The fix mirrors the P2 buckling path (task 4052,
//! `solve_buckling_kernel_p2`): quadratic shape functions resolve bending
//! curvature, so this module supplies the one missing primitive — the P2
//! **consistent mass** `M_e` — to pair with the existing P2 stiffness
//! (`crate::element_stiffness` at `ElementOrder::P2`) in the modal eigenproblem
//! `K φ = λ M φ`.
//!
//! The element matrix shares the row-major `(3·node + axis)` DOF layout of
//! [`crate::assembly::ElementStiffness`] (here 30 DOFs = 10 nodes × 3 axes), so
//! the global mass matrix `M` is assembled by handing each element `M_e` to the
//! existing [`crate::assemble_global_stiffness`] scatter primitive — the
//! assembler is agnostic to `K` vs `K_g` vs `M`.
//!
//! # Why exact degree-4 integration (the central technical point)
//!
//! The mass integrand is `N_a · N_b`. P2 shape functions are quadratic, so the
//! product is a **degree-4** polynomial — unlike the P2 *stiffness* integrand
//! `∇N · ∇N` (degree-2), for which the 4-point Stroud rule on
//! [`crate::elements::tet_p2::TetP2`] is exact. Re-using that degree-2 rule for
//! the mass would make the 10×10 reference Gram matrix rank ≤ 4, hence the
//! 30×30 `M_e` rank ≤ 12 < 30 — singular and **not** positive-definite. The
//! generalized modal eigensolve (`crate::solve_eigen_dense` /
//! `solve_eigen_shift_invert`) factors `M` via Cholesky and therefore requires
//! `M` SPD. So this kernel integrates `N_a · N_b` **exactly** to degree 4 via
//! closed-form barycentric monomial integration
//! `∫_T λ0^i λ1^j λ2^k λ3^l dV = V · (i! j! k! l! · 3!) / ((i+j+k+l+3)!)`,
//! which is exact-by-construction for an affine (straight-edge) tet and mirrors
//! the P1 closed-form precedent. The **quadratic**-velocity kinetic-energy unit
//! test (`vᵀ M v = ρ ∫ v² dV`) is the gate that fails on any under-degree rule
//! (see the test module for why a *linear* field is insufficient).

use crate::assembly::ElementStiffness;
use crate::elements::tet_p2::{EDGES, TetP2};
use crate::elements::{ReferenceCoord, ReferenceElement};
use crate::math::MIN_JACOBIAN_DET;

/// Factorial of a small non-negative integer. The largest argument reached
/// here is 7 — the maximum total barycentric degree of a product of two
/// quadratic P2 shapes (4) plus the spatial-dimension offset (3) — so every
/// value (≤ 5040) is exactly representable in `f64`.
fn factorial(n: usize) -> f64 {
    (1..=n).map(|k| k as f64).product()
}

/// Exact integral of the barycentric monomial `λ0^e0 · λ1^e1 · λ2^e2 · λ3^e3`
/// over the **reference** tetrahedron (volume `1/6`):
///
/// ```text
/// ∫_ref Π λ_i^{e_i} dV = (e0! e1! e2! e3!) / (e0+e1+e2+e3+3)!
/// ```
///
/// This is the classical simplex formula
/// `∫_T Π λ_i^{e_i} dV = (Π e_i!) · d! / (Σe_i + d)! · V` with spatial
/// dimension `d = 3`, where the reference volume `V = 1/6` cancels the
/// `d! = 3! = 6`. Exact-by-construction — no quadrature, so it integrates the
/// degree-4 mass integrand `N_a · N_b` exactly (the requirement that the
/// degree-2 Stroud rule on [`TetP2`] cannot meet).
fn ref_monomial_integral(e: [usize; 4]) -> f64 {
    let num = factorial(e[0]) * factorial(e[1]) * factorial(e[2]) * factorial(e[3]);
    let denom = factorial(e[0] + e[1] + e[2] + e[3] + 3);
    num / denom
}

/// Barycentric monomial terms `(coefficient, exponent-vector)` of the P2 shape
/// function for local node `node` (canonical Hughes/Gmsh ordering):
///
/// - vertex `i ∈ 0..4`: `N_i = λ_i (2 λ_i − 1) = 2 λ_i² − λ_i` → two terms;
/// - edge `4 + e`: `N = 4 λ_a λ_b` for `(a, b) = EDGES[e]` → one term.
///
/// Returned as a fixed `[_; 2]` so the kernel stays allocation-free; an edge
/// shape's unused second slot is the zero term `(0.0, …)`, which contributes
/// nothing to any product.
fn shape_monomials(node: usize) -> [(f64, [usize; 4]); 2] {
    if node < 4 {
        let mut e_sq = [0usize; 4];
        e_sq[node] = 2;
        let mut e_lin = [0usize; 4];
        e_lin[node] = 1;
        [(2.0, e_sq), (-1.0, e_lin)]
    } else {
        let (a, b) = EDGES[node - 4];
        let mut e = [0usize; 4];
        e[a] += 1;
        e[b] += 1;
        [(4.0, e), (0.0, [0; 4])]
    }
}

/// Compute the 30×30 **consistent mass matrix** `M_e` for a P2 (quadratic,
/// 10-node) tetrahedron with constant density `density`.
///
/// `phys_nodes` are the 10 node positions in canonical Hughes/Gmsh ordering: 4
/// vertices `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` followed by the 6 edge-midpoint
/// nodes for [`EDGES`]`[0..=5]` — the same convention as
/// [`crate::element_stiffness`] at `ElementOrder::P2` and `promote_tets_to_p2`.
///
/// The returned matrix shares the row-major `(3·node_idx + axis)` layout of
/// [`ElementStiffness`] (30 DOFs = 10 nodes × 3 axes), so it can be fed into
/// [`crate::assemble_global_stiffness`] without repacking (the assembler treats
/// `k_e` opaquely — K vs K_g vs M).
///
/// # Formula
///
/// For a straight-edge P2 tet with constant density `ρ` and constant Jacobian
/// determinant `det J`,
///
/// ```text
/// M_e[3a+α, 3b+α] = ρ · |det J| · G_ref[a, b]      α ∈ {0,1,2}
/// M_e[3a+α, 3b+β] = 0                               α ≠ β
/// ```
///
/// where `G_ref[a, b] = ∫_ref N_a N_b dV` is integrated **exactly** to degree 4
/// via [`ref_monomial_integral`] over the P2 shapes' barycentric monomials.
/// Block-diagonal in axes (off-axis blocks are 0); total mass per axis sums to
/// `ρ · |det J| · Σ_{a,b} G_ref = ρ · |det J| · ∫_ref (Σ N_a)² = ρ · |det J| / 6
/// = ρ · V_e` by partition of unity.
///
/// # Panics
///
/// Panics under `debug_assertions` when:
/// - `density` is non-finite or non-positive (NaN, ±∞, 0.0, negative) — the
///   same density guard as [`crate::consistent_element_mass_tet_p1`];
/// - `|det J| <= MIN_JACOBIAN_DET` or `det J` is non-finite/subnormal — the
///   shared degeneracy guard.
///
/// Uses `|det J|` so left-handed (mirror-flipped) node orderings still produce
/// the physically correct positive `V_e` and a positive-mass `M_e`.
#[allow(clippy::needless_range_loop)]
pub fn consistent_element_mass_tet_p2(
    phys_nodes: &[[f64; 3]; 10],
    density: f64,
) -> ElementStiffness {
    const N_NODES: usize = 10;
    const N_DOFS: usize = 30;
    debug_assert!(
        density.is_finite() && density > 0.0,
        "density must be finite and positive, got {density}",
    );

    // Constant straight-edge Jacobian determinant via the P2 shape gradients
    // (evaluated at the centroid; constant over the element for straight
    // edges). `det.abs()` so left-handed (mirror) orderings still yield a
    // positive volume factor — mirrors the P1 mass kernel.
    let det = TetP2
        .jacobian(&phys_nodes[..], ReferenceCoord::new(0.25, 0.25, 0.25))
        .det;
    debug_assert!(
        det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
        "degenerate element: |det J| = {} (must be > {} and finite)",
        det.abs(),
        MIN_JACOBIAN_DET,
    );
    let abs_det = det.abs();

    let mut m_e = ElementStiffness::zeros(N_DOFS);
    for a in 0..N_NODES {
        let terms_a = shape_monomials(a);
        for b in 0..N_NODES {
            let terms_b = shape_monomials(b);
            // G_ref[a,b] = ∫_ref N_a N_b dV, integrated term-by-term over the
            // barycentric monomial products (degree ≤ 4 — exact).
            let mut g = 0.0_f64;
            for &(ca, ea) in &terms_a {
                for &(cb, eb) in &terms_b {
                    let e = [
                        ea[0] + eb[0],
                        ea[1] + eb[1],
                        ea[2] + eb[2],
                        ea[3] + eb[3],
                    ];
                    g += ca * cb * ref_monomial_integral(e);
                }
            }
            let coef = density * abs_det * g;
            // Write coef · I_3 into the (a,b) 3×3 block; off-axis (α ≠ β) slots
            // stay 0.0 from ElementStiffness::zeros — block-diagonal-in-axes.
            for alpha in 0..3 {
                let row = 3 * a + alpha;
                let col = 3 * b + alpha;
                m_e.data[row * N_DOFS + col] += coef;
            }
        }
    }

    m_e
}

#[cfg(test)]
mod tests {
    use crate::assembly::ElementStiffness;
    use crate::assembly::test_support::scaled_p2_phys_nodes;
    use crate::elements::tet_p2::EDGES;
    use crate::p2_tet::consistent_element_mass_tet_p2;

    const N_DOFS: usize = 30;

    /// Read the `(i, j)` entry of a row-major `ElementStiffness`.
    fn read(m: &ElementStiffness, i: usize, j: usize) -> f64 {
        m.data[i * m.n_dofs + j]
    }

    /// uᵀ M u for a 30-DOF element matrix.
    fn quad_form(m: &ElementStiffness, u: &[f64; N_DOFS]) -> f64 {
        let mut acc = 0.0_f64;
        for i in 0..N_DOFS {
            for j in 0..N_DOFS {
                acc += u[i] * read(m, i, j) * u[j];
            }
        }
        acc
    }

    /// Build the canonical 10-node P2 phys layout from 4 vertices, with the 6
    /// edge-midpoint nodes in the production [`EDGES`] order. Used by the
    /// left-handed-orientation fixture (where the vertex swap means
    /// `scaled_p2_phys_nodes` cannot be reused). Driven off the production
    /// `EDGES` table so an edge-order change can never silently desync.
    fn p2_phys_from_vertices(v: [[f64; 3]; 4]) -> [[f64; 3]; 10] {
        let mid = |a: usize, b: usize| {
            [
                0.5 * (v[a][0] + v[b][0]),
                0.5 * (v[a][1] + v[b][1]),
                0.5 * (v[a][2] + v[b][2]),
            ]
        };
        let mut nodes = [[0.0_f64; 3]; 10];
        nodes[..4].copy_from_slice(&v);
        for (i, &(a, b)) in EDGES.iter().enumerate() {
            nodes[4 + i] = mid(a, b);
        }
        nodes
    }

    #[test]
    fn consistent_mass_tet_p2_returns_30_by_30_element_stiffness() {
        // (a) shape: 30 DOFs (10 nodes × 3 axes), 900 row-major entries.
        let phys = scaled_p2_phys_nodes(1.0);
        let m_e = consistent_element_mass_tet_p2(&phys, 1.0);
        assert_eq!(m_e.n_dofs, 30, "P2 tet M_e must be 30-DOF (10 nodes × 3 axes)");
        assert_eq!(m_e.data.len(), 900, "row-major 30×30 storage must have 900 entries");
    }

    #[test]
    fn consistent_mass_p2_is_symmetric_within_fp_tolerance() {
        // (c) M_e symmetry — G_ref[a,b] = G_ref[b,a] by construction (Gram
        // matrix). The Φᵀ M Φ-diagonalisation precondition the modal Lanczos
        // /dense eigensolve relies on.
        let phys = scaled_p2_phys_nodes(1.0);
        let m_e = consistent_element_mass_tet_p2(&phys, 2.5);
        for i in 0..N_DOFS {
            for j in 0..N_DOFS {
                let lhs = read(&m_e, i, j);
                let rhs = read(&m_e, j, i);
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-12 * scale,
                    "asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    #[test]
    fn consistent_mass_p2_total_mass_equals_rho_v_on_unit_reference_tet_within_1e_12() {
        // (b) Total mass per axis = Σ_{a,b} M_e[3a, 3b] = ρ · V_e. On the unit
        // reference P2 tet V = 1/6. Guaranteed by partition-of-unity Σ N_a ≡ 1:
        // Σ_{a,b} ∫ N_a N_b = ∫ (Σ N_a)² = ∫ 1 = V. Does NOT distinguish exact
        // from under-integrated mass (any rule with total weight = V_ref passes)
        // — the kinetic-energy test (j) is the degree-4 gate.
        let phys = scaled_p2_phys_nodes(1.0);

        // ρ = 1.0: absolute check.
        let m_e = consistent_element_mass_tet_p2(&phys, 1.0);
        let mut total = 0.0_f64;
        for a in 0..10 {
            for b in 0..10 {
                total += read(&m_e, 3 * a, 3 * b);
            }
        }
        let expected = 1.0_f64 / 6.0;
        assert!(
            (total - expected).abs() < 1e-12,
            "ρ=1 total axis-0 mass = {total}, expected {expected}",
        );

        // ρ = 7850.0 (steel-like): relative check.
        let m_e_steel = consistent_element_mass_tet_p2(&phys, 7850.0);
        let mut total_steel = 0.0_f64;
        for a in 0..10 {
            for b in 0..10 {
                total_steel += read(&m_e_steel, 3 * a, 3 * b);
            }
        }
        let expected_steel = 7850.0_f64 / 6.0;
        assert!(
            (total_steel - expected_steel).abs() < 1e-12 * expected_steel,
            "ρ=7850 total axis-0 mass = {total_steel}, expected {expected_steel}",
        );
    }

    #[test]
    fn consistent_mass_p2_linear_in_density_doubles_every_entry() {
        // (g) M_e is linear in density — doubling ρ doubles every entry.
        let phys = scaled_p2_phys_nodes(1.0);
        let m1 = consistent_element_mass_tet_p2(&phys, 1.0);
        let m2 = consistent_element_mass_tet_p2(&phys, 2.0);
        for i in 0..900 {
            let want = 2.0 * m1.data[i];
            let got = m2.data[i];
            let scale = want.abs().max(1.0);
            assert!(
                (got - want).abs() < 1e-12 * scale,
                "linearity at idx {i}: got {got}, expected 2·{} = {want}",
                m1.data[i],
            );
        }
    }

    #[test]
    fn consistent_mass_p2_volume_scaling_octuples_mass_when_edge_length_doubles() {
        // (h) V_e ∝ L³, so a uniform 2× scale yields M_e' = 8 · M_e — the
        // canonical mass-vs-stiffness scaling difference (P2 stiffness scales
        // as L, mass as L³).
        let m_unit = consistent_element_mass_tet_p2(&scaled_p2_phys_nodes(1.0), 1.0);
        let m_scaled = consistent_element_mass_tet_p2(&scaled_p2_phys_nodes(2.0), 1.0);
        for i in 0..900 {
            let want = 8.0 * m_unit.data[i];
            let got = m_scaled.data[i];
            let scale = want.abs().max(1.0);
            assert!(
                (got - want).abs() < 1e-12 * scale,
                "volume scaling at idx {i}: got {got}, expected 8·{} = {want}",
                m_unit.data[i],
            );
        }
    }

    #[test]
    fn consistent_mass_p2_left_handed_orientation_yields_positive_mass_equal_to_rho_v() {
        // (i) Swap vertices 2 ↔ 3 ⇒ det J < 0; physical V is still positive, so
        // total mass must be +ρV = 1/6 and every diagonal entry > 0. Pins the
        // det.abs() choice — a regression re-introducing signed det would yield
        // total mass = −1/6 and negative diagonals.
        let flipped = p2_phys_from_vertices([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0],
        ]);
        let m_e = consistent_element_mass_tet_p2(&flipped, 1.0);
        let mut total = 0.0_f64;
        for a in 0..10 {
            for b in 0..10 {
                total += read(&m_e, 3 * a, 3 * b);
            }
        }
        let expected = 1.0_f64 / 6.0;
        assert!(
            (total - expected).abs() < 1e-12,
            "left-handed total axis-0 mass = {total}, expected {expected} (det.abs() must be used)",
        );
        for i in 0..N_DOFS {
            let d = read(&m_e, i, i);
            assert!(d > 0.0, "diagonal entry M[{i},{i}] = {d}, expected > 0");
        }
    }

    #[test]
    fn consistent_mass_p2_is_positive_definite_via_quadratic_form() {
        // (f) M is a Gram matrix (ρ ∫ N Nᵀ), so PD is structural for exact
        // integration. Sign-mixed and sparse-load patterns are positivity
        // checks (no kernel mode). The full-PD gate is the modal Cholesky in
        // tests/modal_benchmarks.rs; here we pin necessary positivity.
        let phys = scaled_p2_phys_nodes(1.0);
        let m_e = consistent_element_mass_tet_p2(&phys, 1.0);

        // Sign-mixed pattern: u_i = (-1)^i.
        let mut u_sign = [0.0_f64; N_DOFS];
        for (i, val) in u_sign.iter_mut().enumerate() {
            *val = if i % 2 == 0 { 1.0 } else { -1.0 };
        }
        let q_sign = quad_form(&m_e, &u_sign);
        assert!(q_sign > 0.0, "sign-mixed uᵀMu = {q_sign}, expected > 0");

        // Sparse-load: single nonzero DOF.
        let mut u_sparse = [0.0_f64; N_DOFS];
        u_sparse[0] = 1.0;
        let q_sparse = quad_form(&m_e, &u_sparse);
        assert!(q_sparse > 0.0, "sparse uᵀMu = {q_sparse}, expected > 0");
    }

    #[test]
    fn consistent_mass_p2_off_axis_blocks_are_zero_block_diagonal_3x3_structure() {
        // (d) Each (a, b) node-pair block is `coef · I_3` — diagonal in
        // axis-axis indexing. α ≠ β entries must be exactly 0.
        let phys = scaled_p2_phys_nodes(1.0);
        let m_e = consistent_element_mass_tet_p2(&phys, 1.0);
        for a in 0..10 {
            for b in 0..10 {
                for alpha in 0..3 {
                    for beta in 0..3 {
                        if alpha == beta {
                            continue;
                        }
                        let v = read(&m_e, 3 * a + alpha, 3 * b + beta);
                        assert_eq!(v, 0.0, "(a,b,α,β) = ({a},{b},{alpha},{beta}) must be 0");
                    }
                }
            }
        }
    }

    #[test]
    fn consistent_mass_p2_kinetic_energy_exact_for_constant_and_quadratic_velocity() {
        // (j) THE DEGREE-4-EXACTNESS GATE.
        //
        // M is axis-block-diagonal, so for a nodal velocity vector u whose
        // α-component samples a scalar field v_α at the nodes,
        //   uᵀ M u = ρ Σ_α ∫_Ω v_{α,h}² dV
        // where v_{α,h} = Σ_a v_α(x_a) N_a is the P2 interpolant. P2 reproduces
        // any field in its space (degree ≤ 2) exactly, so v_h ≡ v there.
        //
        // Why a *quadratic* field, not merely linear: for linear v, v² is
        // degree-2, which the existing degree-2 Stroud rule integrates exactly
        // — so a linear field gives uᵀMu = ∫v² for BOTH the exact mass and a
        // degree-2 under-integrated mass, and does NOT bite. A quadratic v
        // makes v² degree-4, which only the exact (closed-form / degree-4)
        // integration reproduces; the degree-2 Stroud rule under-integrates it
        // and this assert fails. That failure is what forces exact integration.
        let phys = scaled_p2_phys_nodes(1.0);
        let density = 3.0;
        let m_e = consistent_element_mass_tet_p2(&phys, density);

        // --- constant field v_x ≡ 2 (degree-0): uᵀMu = ρ V c² ---
        let mut u_const = [0.0_f64; N_DOFS];
        for a in 0..10 {
            u_const[3 * a] = 2.0;
        }
        let q_const = quad_form(&m_e, &u_const);
        let expected_const = density * (1.0 / 6.0) * 2.0 * 2.0; // ρ · V · c²
        assert!(
            (q_const - expected_const).abs() < 1e-12 * expected_const,
            "constant-velocity uᵀMu = {q_const}, expected ρVc² = {expected_const}",
        );

        // --- quadratic field v_x(x) = x² (degree-2): uᵀMu = ρ ∫_T (x²)² dV ---
        // On the unit reference tet x ≡ λ_1, so ∫_T x⁴ dV = ∫ λ_1⁴ dV =
        // 4!/(4+3)! = 24/5040 = 1/210 (independent analytical oracle).
        let mut u_quad = [0.0_f64; N_DOFS];
        for (a, node) in phys.iter().enumerate() {
            u_quad[3 * a] = node[0] * node[0]; // x_a²
        }
        let q_quad = quad_form(&m_e, &u_quad);
        let expected_quad = density * (1.0 / 210.0); // ρ ∫ x⁴ dV
        assert!(
            (q_quad - expected_quad).abs() < 1e-12 * expected_quad.max(1.0),
            "quadratic-velocity uᵀMu = {q_quad}, expected ρ·∫x⁴ = {expected_quad} \
             (degree-4 integration required — a degree-2 rule under-integrates x⁴)",
        );
    }

    // ----- debug-only density-guard tests -----
    // The guard `debug_assert!(density.is_finite() && density > 0.0, ...)` is
    // compiled in only under `debug_assertions`, so these tests are gated
    // identically. The `#[should_panic]` string must match the guard message.

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "density must be finite and positive")]
    fn consistent_mass_p2_panics_on_nan_density() {
        let _ = consistent_element_mass_tet_p2(&scaled_p2_phys_nodes(1.0), f64::NAN);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "density must be finite and positive")]
    fn consistent_mass_p2_panics_on_positive_infinite_density() {
        let _ = consistent_element_mass_tet_p2(&scaled_p2_phys_nodes(1.0), f64::INFINITY);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "density must be finite and positive")]
    fn consistent_mass_p2_panics_on_negative_infinite_density() {
        let _ = consistent_element_mass_tet_p2(&scaled_p2_phys_nodes(1.0), f64::NEG_INFINITY);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "density must be finite and positive")]
    fn consistent_mass_p2_panics_on_zero_density() {
        let _ = consistent_element_mass_tet_p2(&scaled_p2_phys_nodes(1.0), 0.0);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "density must be finite and positive")]
    fn consistent_mass_p2_panics_on_negative_density() {
        let _ = consistent_element_mass_tet_p2(&scaled_p2_phys_nodes(1.0), -1.0);
    }
}
