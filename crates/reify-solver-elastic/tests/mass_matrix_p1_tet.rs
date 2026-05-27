//! δ task user-observable signal (PRD `docs/prds/v0_3/modal-analysis.md` §10
//! Phase 1): P1-tet consistent-mass element kernel + global assembly +
//! uniform-density-block symmetric-PSD signal probed via uᵀMu.
//!
//! ## Split with `mass_matrix.rs` inline tests
//!
//! The per-element pins (12×12 symmetry, ρV total mass, ρ-linearity,
//! V_e ∝ L³ scaling, det.abs() sign-invariance, block-diagonal-in-axes
//! structure) and the in-crate global-pipeline pins (`assemble_global_stiffness`
//! on a 2-tet shared-face mesh — ρV sum, global symmetry) all live in the
//! inline `#[cfg(test)] mod tests` of
//! `crates/reify-solver-elastic/src/mass_matrix.rs`. This file holds only the
//! signal that is genuinely new at the integration vantage:
//!
//! - **Crate-root re-export compile-time pin.** The
//!   `use reify_solver_elastic::consistent_element_mass_tet_p1;` import fails
//!   to compile if the re-export added in `lib.rs` is dropped — same pattern
//!   as `tests/kg_p1_tet.rs`'s import block for
//!   `geometric_element_stiffness_tet_p1`.
//! - **Global PSD via uᵀ M u.** Two nonzero `u` vectors exercise the
//!   assembled `M_global` — an axis-mixed sign-toggle and a sparse
//!   single-DOF probe. The rigid-x translation equality pin (uᵀMu = ρ·ΣV_e)
//!   is *deliberately omitted* here because the inline
//!   `..._total_mass_equals_rho_sum_v_e` test in `src/mass_matrix.rs`
//!   already pins the same equality (sum of axis-0 entries of `M_global`
//!   on the same two-tet shared-face mesh), and the two formulations are
//!   algebraically identical for unit-vector `u`. Mirrors the role
//!   `tests/kg_p1_tet.rs::euler_column_pin_pin_within_ten_percent` plays
//!   for the K_g surface (per the 2026-05-27 "trim foundation β doctest,
//!   move signature pins to integration" convention amend).

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, assemble_global_stiffness, consistent_element_mass_tet_p1,
};

/// uᵀ M u for an `n × n` faer `SparseRowMat` densified to a square matrix.
/// `u` and `mat` must be the same dimension; `mat` is read from its dense
/// projection (sufficient for the small 15×15 fixture here).
fn global_quad_form(mat: &faer::sparse::SparseRowMat<usize, f64>, u: &[f64]) -> f64 {
    assert_eq!(mat.nrows(), u.len(), "mat rows must match u length");
    assert_eq!(mat.ncols(), u.len(), "mat cols must match u length");
    let dense = mat.to_dense();
    let n = u.len();
    let mut acc = 0.0_f64;
    for i in 0..n {
        for j in 0..n {
            acc += u[i] * dense[(i, j)] * u[j];
        }
    }
    acc
}

#[test]
fn consistent_mass_p1_global_m_is_psd_via_quadratic_form() {
    // Two-tet shared-face mesh: conn0 = [0,1,2,3] is the canonical unit tet
    // (V_e0 = 1/6); conn1 = [1,2,3,4] adds node 4 = (1,1,1):
    //   J = phys[1]-phys[0], phys[2]-phys[0], phys[3]-phys[0]
    //     = (-1,1,0), (-1,0,1), (0,1,1)
    //   det = -1·(0·1-1·1) -1·(-1·1-1·0) + 0·… = 1 + 1 = 2
    //   V_e1 = |2|/6 = 1/3
    // Nodes 1, 2, 3 receive contributions from both elements (shared-DOF
    // summation), so the assembler's symmetry contract is exercised here too;
    // the explicit global-symmetry sweep lives in the inline tests.
    let nodes: [[f64; 3]; 5] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
    ];
    let conn0 = [0_usize, 1, 2, 3];
    let conn1 = [1_usize, 2, 3, 4];
    let phys0 = [
        nodes[conn0[0]],
        nodes[conn0[1]],
        nodes[conn0[2]],
        nodes[conn0[3]],
    ];
    let phys1 = [
        nodes[conn1[0]],
        nodes[conn1[1]],
        nodes[conn1[2]],
        nodes[conn1[3]],
    ];

    let density = 1.0_f64;
    let m_e0 = consistent_element_mass_tet_p1(&phys0, density);
    let m_e1 = consistent_element_mass_tet_p1(&phys1, density);

    let elements = [
        AssemblyElement {
            id: 0,
            connectivity: &conn0,
            k_e: &m_e0,
        },
        AssemblyElement {
            id: 1,
            connectivity: &conn1,
            k_e: &m_e1,
        },
    ];
    let m_global = assemble_global_stiffness(5, &elements, AssemblyMode::Deterministic);

    // (i) Axis-mixed sign-toggle: u_i = (-1)^i. Mixes all three axes; the
    //     sign-mixed pattern guarantees no rigid-mode cancellation, so
    //     uᵀ M u must be strictly positive. The rigid-x equality pin is
    //     intentionally omitted — see the module-level doc-comment.
    let mut u_sign = [0.0_f64; 15];
    for i in 0..15 {
        u_sign[i] = if i % 2 == 0 { 1.0 } else { -1.0 };
    }
    let q_sign = global_quad_form(&m_global, &u_sign);
    assert!(q_sign > 0.0, "sign-mixed uᵀMu = {q_sign}, expected > 0");

    // (ii) Sparse single-DOF probe: u_0 = 1, all others 0. Node 0 appears
    //      only in conn0, so uᵀ M u = m_e0[0,0] = ρ·V_e0/10 > 0.
    let mut u_sparse = [0.0_f64; 15];
    u_sparse[0] = 1.0;
    let q_sparse = global_quad_form(&m_global, &u_sparse);
    assert!(q_sparse > 0.0, "sparse uᵀMu = {q_sparse}, expected > 0");
}
