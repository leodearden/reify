//! Consistent mass-matrix kernels.
//!
//! See PRD `docs/prds/v0_3/modal-analysis.md` §10 Phase 1 task δ:
//! "consistent mass for tet4 elements; lumped variant deferred to Open
//! Question §12.3". v0.3 ships a single P1 tetrahedron kernel; hex/wedge/
//! shell mass kernels and the row-sum-lumped variant are out of scope for
//! this task.
//!
//! The element matrix shares the row-major `(3·node + axis)` DOF layout of
//! [`crate::assembly::ElementStiffness`], so the global mass matrix `M` is
//! assembled by handing each element `M_e` to the existing
//! [`crate::assemble_global_stiffness`] scatter primitive — no new
//! global-API surface needed (the assembler is agnostic to `K` vs `K_g`
//! vs `M`).

use crate::assembly::ElementStiffness;
use crate::elements::{ReferenceCoord, ReferenceElement, tet_p1::TetP1};
use crate::math::MIN_JACOBIAN_DET;

/// Compute the 12×12 **consistent mass matrix** `M_e` for a P1 (linear,
/// 4-node) tetrahedron with constant density `density`.
///
/// `phys_nodes` are the 4 vertex positions in the canonical reference-vertex
/// ordering `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` — same convention as
/// [`crate::element_stiffness_p1`] and [`crate::geometric_element_stiffness_tet_p1`].
///
/// The returned matrix shares the row-major `(3·node_idx + axis)` layout of
/// [`ElementStiffness`], so it can be fed into [`crate::assemble_global_stiffness`]
/// without any repacking (the assembler treats `k_e` opaquely — K vs K_g vs M).
///
/// # Formula
///
/// For a P1 (straight-edge) tet with constant density `ρ`, the consistent
/// mass entries are
///
/// ```text
/// M_e[3a+α, 3b+β] = ρ · ∫_Ω N_a · N_b · δ_αβ dV
/// ```
///
/// Using the classical closed form `∫_T N_a · N_b dV = V_e · (1 + δ_{a,b})/20`
/// (exact under an affine reference→physical map, so degree-2 quadrature
/// is unnecessary), with `V_e = |det J| / 6`,
///
/// ```text
/// M_e[3a+α, 3b+α] = ρ · V_e · (1 + δ_{a,b}) / 20         α ∈ {0,1,2}
/// M_e[3a+α, 3b+β] = 0                                     α ≠ β
/// ```
///
/// Total mass per axis sums to `ρ · V_e · (4 · 1/10 + 12 · 1/20) = ρ · V_e`.
///
/// # Panics
///
/// Panics under `debug_assertions` when `|det J| <= MIN_JACOBIAN_DET` or
/// when `det J` is non-finite/subnormal — the same degeneracy-guard
/// convention as [`crate::element_stiffness_p1`] and
/// [`crate::geometric_element_stiffness_tet_p1`].
///
/// Uses `|det J|` so left-handed (mirror-flipped) node orderings still
/// produce the physically correct positive `V_e` and a positive-mass `M_e`.
#[allow(clippy::needless_range_loop)]
pub fn consistent_element_mass_tet_p1(
    phys_nodes: &[[f64; 3]; 4],
    density: f64,
) -> ElementStiffness {
    const N_NODES: usize = 4;
    const N_DOFS: usize = 12;
    let mut m_e = ElementStiffness::zeros(N_DOFS);

    // P1 has constant gradients — evaluating at the centroid is just as
    // valid as any other reference point; the centroid is the canonical
    // 1-point Gauss location. We only need gradients to build J (for V_e);
    // the shape-function values N_a are absorbed into the closed-form
    // integral V_e·(1+δ_{a,b})/20.
    let centroid = ReferenceCoord::new(0.25, 0.25, 0.25);
    let grads_ref = TetP1.shape_grad_at(centroid);
    debug_assert_eq!(grads_ref.len(), N_NODES);

    // Forward Jacobian J_ij = Σ_k phys_nodes[k][i] · grads_ref[k][j].
    // Built as a 3×3 stack array — we only need `det` here, so there is
    // no benefit to constructing the full `ReferenceElement::jacobian()`
    // `Jacobian` struct (which also computes the inverse transpose). Note
    // that `grads_ref` from `shape_grad_at` is still a heap-allocated Vec
    // per call; the same heap traffic exists in `element_stiffness_p1`
    // and `geometric_element_stiffness_tet_p1`, and could be eliminated
    // by a future sweep that replaces these with a const `[[f64;3];4]`
    // across all three tet-P1 kernels (gradients are compile-time
    // constants for P1).
    let mut j_mat = [[0.0_f64; 3]; 3];
    for k in 0..N_NODES {
        for i in 0..3 {
            for jj in 0..3 {
                j_mat[i][jj] += phys_nodes[k][i] * grads_ref[k][jj];
            }
        }
    }
    let det = j_mat[0][0] * (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1])
        - j_mat[0][1] * (j_mat[1][0] * j_mat[2][2] - j_mat[1][2] * j_mat[2][0])
        + j_mat[0][2] * (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]);

    debug_assert!(
        det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
        "degenerate element: |det J| = {} (must be > {} and finite)",
        det.abs(),
        MIN_JACOBIAN_DET,
    );

    let v_e = det.abs() / 6.0; // P1 tet physical volume

    // For each (a, b) node-pair, compute the closed-form coefficient and
    // write coef · I_3 into the 3×3 block at rows [3a..3a+3], cols
    // [3b..3b+3]. Off-axis (α ≠ β) slots remain 0.0 from
    // ElementStiffness::zeros(12) — block-diagonal-in-axes physics.
    for a in 0..N_NODES {
        for b in 0..N_NODES {
            let kron = if a == b { 1.0 } else { 0.0 };
            let coef = density * v_e * (1.0 + kron) / 20.0;
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
    use super::*;

    /// Canonical unit reference tet — vertices `(0,0,0), (1,0,0), (0,1,0),
    /// (0,0,1)` with reference volume `1/6`. Mirrors the constant in
    /// `geometric_stiffness/tet.rs::tests::UNIT_TET`.
    const UNIT_TET: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    fn read(m: &ElementStiffness, i: usize, j: usize) -> f64 {
        m.data[i * m.n_dofs + j]
    }

    #[test]
    fn consistent_mass_tet_p1_returns_12_by_12_element_stiffness() {
        let m_e = consistent_element_mass_tet_p1(&UNIT_TET, 1.0);
        assert_eq!(m_e.n_dofs, 12, "P1 tet M_e must be 12-DOF (4 nodes × 3 axes)");
        assert_eq!(
            m_e.data.len(),
            144,
            "row-major 12×12 storage must have 144 entries"
        );
    }

    #[test]
    fn consistent_mass_p1_is_symmetric_within_fp_tolerance() {
        // M_e symmetry — coef(a,b) = coef(b,a) by construction. This is the
        // Φᵀ M Φ-diagonalisation precondition that Lanczos (task ε) relies on.
        // Regression-guard: a future refactor that uses a half-block write loop
        // (e.g. only a ≤ b) would fail here.
        let m_e = consistent_element_mass_tet_p1(&UNIT_TET, 2.5);
        for i in 0..12 {
            for j in 0..12 {
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
    fn consistent_mass_p1_total_mass_equals_rho_v_on_unit_reference_tet_within_1e_12() {
        // Total mass per axis = Σ_{a,b} M_e[3a, 3b] = ρ · V_e. On the unit
        // reference tet V = 1/6. Pins the closed-form V·(1+δ_{a,b})/20
        // coefficient choice — 4·(V/10) + 12·(V/20) = V.

        // ρ = 1.0: absolute check
        let m_e = consistent_element_mass_tet_p1(&UNIT_TET, 1.0);
        let mut total: f64 = 0.0;
        for a in 0..4 {
            for b in 0..4 {
                total += read(&m_e, 3 * a, 3 * b);
            }
        }
        let expected = 1.0_f64 / 6.0;
        assert!(
            (total - expected).abs() < 1e-12,
            "ρ=1 total axis-0 mass = {total}, expected {expected}",
        );

        // ρ = 7850.0 (steel-like): relative check
        let m_e_steel = consistent_element_mass_tet_p1(&UNIT_TET, 7850.0);
        let mut total_steel: f64 = 0.0;
        for a in 0..4 {
            for b in 0..4 {
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
    fn consistent_mass_p1_linear_in_density_doubles_every_entry() {
        // M_e is linear in density — doubling ρ doubles every entry.
        // Mirrors geometric_stiffness/tet.rs::linear_in_stress_magnitude.
        let m1 = consistent_element_mass_tet_p1(&UNIT_TET, 1.0);
        let m2 = consistent_element_mass_tet_p1(&UNIT_TET, 2.0);
        for i in 0..144 {
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
    fn consistent_mass_p1_volume_scaling_octuples_mass_when_edge_length_doubles() {
        // V_e ∝ L³, so a uniform 2× scale yields M_e' = 8 · M_e. This is the
        // canonical mass-vs-stiffness scaling difference (stiffness scales as
        // L for P1, mass scales as L³). Pinned alongside
        // `assembly/tet.rs::p1_volume_scaling_doubles_stiffness_when_edge_length_doubles`.
        const SCALED_TET: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
        ];
        let m_unit = consistent_element_mass_tet_p1(&UNIT_TET, 1.0);
        let m_scaled = consistent_element_mass_tet_p1(&SCALED_TET, 1.0);
        for i in 0..144 {
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
    fn consistent_mass_p1_left_handed_orientation_yields_positive_mass_equal_to_rho_v() {
        // Swap nodes 2 ↔ 3 ⇒ det J < 0; physical V is still positive,
        // so total mass must be positive (= ρV = 1/6) and every diagonal
        // entry must be > 0. Pins the det.abs() choice — a regression that
        // re-introduces signed det would yield total mass = −1/6 and
        // negative diagonal entries.
        const FLIPPED: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0],
        ];
        let m_e = consistent_element_mass_tet_p1(&FLIPPED, 1.0);
        let mut total: f64 = 0.0;
        for a in 0..4 {
            for b in 0..4 {
                total += read(&m_e, 3 * a, 3 * b);
            }
        }
        let expected = 1.0_f64 / 6.0;
        assert!(
            (total - expected).abs() < 1e-12,
            "left-handed total axis-0 mass = {total}, expected {expected} (det.abs() must be used)",
        );
        for i in 0..12 {
            let d = read(&m_e, i, i);
            assert!(d > 0.0, "diagonal entry M[{i},{i}] = {d}, expected > 0");
        }
    }

    /// uᵀ M u for a 12-DOF element matrix.
    fn quad_form(m: &ElementStiffness, u: &[f64; 12]) -> f64 {
        let mut acc = 0.0_f64;
        for i in 0..12 {
            for j in 0..12 {
                acc += u[i] * read(m, i, j) * u[j];
            }
        }
        acc
    }

    #[test]
    fn consistent_mass_p1_two_tets_via_assemble_global_stiffness_total_mass_equals_rho_sum_v_e() {
        // Build a 2-tet shared-face mesh: conn0 = [0,1,2,3] is UNIT_TET (V=1/6),
        // conn1 = [1,2,3,4] is a second tet with node 4 = (1,1,1) (V = 1/3
        // computed below from the actual det.abs()/6). Wrap each M_e in
        // AssemblyElement and feed the existing assemble_global_stiffness
        // primitive. Pins the "FEA assembly pipeline" hook: the assembler
        // treats k_e opaquely (K vs K_g vs M).
        use crate::assembly::{AssemblyElement, AssemblyMode, assemble_global_stiffness};

        let nodes: [[f64; 3]; 5] = [
            [0.0, 0.0, 0.0], // 0
            [1.0, 0.0, 0.0], // 1
            [0.0, 1.0, 0.0], // 2
            [0.0, 0.0, 1.0], // 3
            [1.0, 1.0, 1.0], // 4
        ];
        let conn0 = [0_usize, 1, 2, 3];
        let conn1 = [1_usize, 2, 3, 4];
        let phys0 = [nodes[conn0[0]], nodes[conn0[1]], nodes[conn0[2]], nodes[conn0[3]]];
        let phys1 = [nodes[conn1[0]], nodes[conn1[1]], nodes[conn1[2]], nodes[conn1[3]]];

        let density = 1.0;
        let m_e0 = consistent_element_mass_tet_p1(&phys0, density);
        let m_e1 = consistent_element_mass_tet_p1(&phys1, density);

        let elements = [
            AssemblyElement { id: 0, connectivity: &conn0, k_e: &m_e0 },
            AssemblyElement { id: 1, connectivity: &conn1, k_e: &m_e1 },
        ];
        let m_global = assemble_global_stiffness(5, &elements, AssemblyMode::Deterministic);
        assert_eq!(m_global.nrows(), 15, "5 nodes × 3 axes = 15 rows");
        assert_eq!(m_global.ncols(), 15, "5 nodes × 3 axes = 15 cols");

        // Sum every axis-0 entry of M_global (the M_global[3i, 3j] entries for
        // i, j ∈ 0..5). Since M is axis-block-diagonal, this is the total mass
        // along axis 0, which equals ρ · (V_e0 + V_e1).
        let dense = m_global.to_dense();
        let mut total: f64 = 0.0;
        for i in 0..5 {
            for j in 0..5 {
                total += dense[(3 * i, 3 * j)];
            }
        }

        // V_e0 = 1/6; for V_e1 compute via the same det as the kernel.
        // phys1 = [(1,0,0), (0,1,0), (0,0,1), (1,1,1)]
        // J = phys[1]-phys[0], phys[2]-phys[0], phys[3]-phys[0]
        //   = (-1,1,0), (-1,0,1), (0,1,1)
        // det = -1·(0·1 - 1·1) - 1·(-1·1 - 1·0) + 0·…
        //     = -1·(-1) -1·(-1)  + 0 = 1 + 1 = 2
        // V_e1 = |2|/6 = 1/3
        let v_e0 = 1.0 / 6.0;
        let v_e1 = 1.0 / 3.0;
        let expected = density * (v_e0 + v_e1);
        assert!(
            (total - expected).abs() < 1e-12,
            "global axis-0 mass = {total}, expected ρ·(V_e0+V_e1) = {expected}",
        );
    }

    #[test]
    fn consistent_mass_p1_assembled_global_m_is_symmetric_within_fp_tolerance() {
        // Same two-tet shared-face mesh as step-17 — nodes 1, 2, 3 receive
        // contributions from both elements (shared-DOF summation occurs).
        // Asserts the global M is symmetric within FP tolerance, mirroring the
        // K-stiffness pin `assembly/global.rs::global_k_is_symmetric_within_fp_tolerance`.
        use crate::assembly::{AssemblyElement, AssemblyMode, assemble_global_stiffness};

        let nodes: [[f64; 3]; 5] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        let conn0 = [0_usize, 1, 2, 3];
        let conn1 = [1_usize, 2, 3, 4];
        let phys0 = [nodes[conn0[0]], nodes[conn0[1]], nodes[conn0[2]], nodes[conn0[3]]];
        let phys1 = [nodes[conn1[0]], nodes[conn1[1]], nodes[conn1[2]], nodes[conn1[3]]];

        let m_e0 = consistent_element_mass_tet_p1(&phys0, 1.0);
        let m_e1 = consistent_element_mass_tet_p1(&phys1, 1.0);

        let elements = [
            AssemblyElement { id: 0, connectivity: &conn0, k_e: &m_e0 },
            AssemblyElement { id: 1, connectivity: &conn1, k_e: &m_e1 },
        ];
        let m_global = assemble_global_stiffness(5, &elements, AssemblyMode::Deterministic);
        let dense = m_global.to_dense();
        for i in 0..15 {
            for j in i..15 {
                let lhs = dense[(i, j)];
                let rhs = dense[(j, i)];
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-9 * scale,
                    "global asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    #[test]
    fn consistent_mass_p1_is_positive_semidefinite_via_quadratic_form() {
        // M is a Gram matrix (integral of ρ·N·Nᵀ), so PSD is structural.
        // The strongest pin is (a) rigid-translation along an axis: the
        // kinetic-energy invariant uᵀ M u = ρV for the unit reference tet
        // (V = 1/6) is an *equality*, so an off-by-constant error in the
        // coef formula would fire here. (b) sign-mixed and (c) sparse-load
        // are positivity-only (no kernel mode).
        let m_e = consistent_element_mass_tet_p1(&UNIT_TET, 1.0);

        // (a) Rigid translation along x: u = (1,0,0, 1,0,0, 1,0,0, 1,0,0)
        let mut u_trans = [0.0_f64; 12];
        for node in 0..4 {
            u_trans[3 * node] = 1.0;
        }
        let q_trans = quad_form(&m_e, &u_trans);
        let expected = 1.0_f64 / 6.0;
        assert!(
            (q_trans - expected).abs() < 1e-12,
            "rigid-x uᵀMu = {q_trans}, expected ρV = {expected}",
        );

        // (b) Sign-mixed pattern: u_i = (-1)^i
        let mut u_sign = [0.0_f64; 12];
        for i in 0..12 {
            u_sign[i] = if i % 2 == 0 { 1.0 } else { -1.0 };
        }
        let q_sign = quad_form(&m_e, &u_sign);
        assert!(q_sign > 0.0, "sign-mixed uᵀMu = {q_sign}, expected > 0");

        // (c) Sparse-load: single nonzero entry at DOF 0
        let mut u_sparse = [0.0_f64; 12];
        u_sparse[0] = 1.0;
        let q_sparse = quad_form(&m_e, &u_sparse);
        assert!(q_sparse > 0.0, "sparse uᵀMu = {q_sparse}, expected > 0");
    }

    #[test]
    fn consistent_mass_p1_off_axis_blocks_are_zero_block_diagonal_3x3_structure() {
        // Each (a, b) node-pair block in M_e is `coef · I_3` — diagonal in
        // axis-axis indexing. α ≠ β entries must be exactly 0. Mirrors the
        // K_g pin `geometric_stiffness/tet.rs::off_axis_blocks_are_zero_block_diagonal_3x3_structure`.
        let m_e = consistent_element_mass_tet_p1(&UNIT_TET, 1.0);
        for a in 0..4 {
            for b in 0..4 {
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
}
