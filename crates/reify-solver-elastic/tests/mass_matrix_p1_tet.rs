//! δ task user-observable signal (PRD `docs/prds/v0_3/modal-analysis.md` §10
//! Phase 1): P1-tet consistent-mass element kernel + global assembly +
//! uniform-density-block ρV / symmetric-PSD signal.
//!
//! ## What this file pins
//!
//! The PRD δ signal is "unit test confirms total mass of a uniform-density
//! block equals ρV within 1e-12; assembled mass matrix is symmetric PSD".
//! The test `consistent_mass_p1_uniform_density_block_total_mass_equals_rho_v_and_global_m_is_symmetric_psd`
//! exercises all three halves of that signal on a two-tet shared-face mesh
//! that runs through the existing `assemble_global_stiffness` pipeline (the
//! "exposed via the existing FEA assembly pipeline" hook the PRD requests):
//!
//! - (a) **ρV invariant**: sum of all axis-0 entries of `M_global` equals
//!   `ρ · (V_e0 + V_e1)` within 1e-12.
//! - (b) **global symmetry**: `|M[i,j] − M[j,i]| < 1e-9 · max(|M[i,j]|,
//!   |M[j,i]|, 1.0)` over the full 15×15 entries (shared-DOF summation could
//!   in principle perturb FP symmetry; this pin shows the existing assembler
//!   preserves it).
//! - (c) **global PSD**: `uᵀ M_global u > 0` for three nonzero `u` — rigid
//!   translation along x (equality pin to `ρ · (V_e0 + V_e1)`), an
//!   axis-mixed sign-toggle, and a sparse single-DOF probe.
//!
//! ## Why an integration test in addition to inline unit tests
//!
//! `crates/reify-solver-elastic/src/mass_matrix.rs` already covers the
//! per-element and in-crate-global pins. This file replays the most important
//! global-pipeline signals from the *downstream-consumer vantage* — its `use
//! reify_solver_elastic::consistent_element_mass_tet_p1;` import block is the
//! implicit compile-time pin for the crate-root re-export. Mirrors
//! `tests/kg_p1_tet.rs`'s role for the K_g surface (per the 2026-05-27
//! "trim foundation β doctest, move signature pins to integration"
//! convention amend).

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, ElementStiffness, assemble_global_stiffness,
    consistent_element_mass_tet_p1,
};

/// Canonical unit reference tet — vertices `(0,0,0), (1,0,0), (0,1,0),
/// (0,0,1)` with reference volume `V_e0 = 1/6`. Matches the in-crate
/// constant in `mass_matrix.rs::tests::UNIT_TET`.
const UNIT_TET_NODES: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

/// uᵀ M u for an `n × n` faer `SparseRowMat` densified to a square matrix.
/// `u` and `mat` must be the same dimension; `mat` is read from its dense
/// projection (sufficient for the small 15×15 fixture here — mirrors the
/// `quad_form` helper in `mass_matrix.rs::tests`).
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
fn consistent_mass_p1_uniform_density_block_total_mass_equals_rho_v_and_global_m_is_symmetric_psd()
{
    // ---- 1. Two-tet shared-face mesh ----------------------------------------
    // conn0 = [0,1,2,3] is UNIT_TET, V_e0 = 1/6.
    // conn1 = [1,2,3,4] adds node 4 = (1,1,1):
    //   J = phys[1]-phys[0], phys[2]-phys[0], phys[3]-phys[0]
    //     = (-1,1,0), (-1,0,1), (0,1,1)
    //   det = -1·(0·1-1·1) -1·(-1·1-1·0) + 0·… = 1 + 1 = 2
    //   V_e1 = |2|/6 = 1/3
    // Nodes 1, 2, 3 receive contributions from both elements (shared-DOF
    // summation), so step-19's symmetry guarantee is exercised here too.
    let nodes: [[f64; 3]; 5] = [
        UNIT_TET_NODES[0],
        UNIT_TET_NODES[1],
        UNIT_TET_NODES[2],
        UNIT_TET_NODES[3],
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
    let m_e0: ElementStiffness = consistent_element_mass_tet_p1(&phys0, density);
    let m_e1: ElementStiffness = consistent_element_mass_tet_p1(&phys1, density);

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
    assert_eq!(m_global.nrows(), 15, "5 nodes × 3 axes = 15 rows");
    assert_eq!(m_global.ncols(), 15, "5 nodes × 3 axes = 15 cols");

    let v_e0 = 1.0_f64 / 6.0;
    let v_e1 = 1.0_f64 / 3.0;
    let expected_total_mass = density * (v_e0 + v_e1);

    // ---- 2. (a) ρV invariant: sum of all axis-0 entries = ρ·ΣV_e ------------
    let dense = m_global.to_dense();
    let mut total: f64 = 0.0;
    for i in 0..5 {
        for j in 0..5 {
            total += dense[(3 * i, 3 * j)];
        }
    }
    assert!(
        (total - expected_total_mass).abs() < 1e-12,
        "axis-0 total mass = {total}, expected ρ·(V_e0+V_e1) = {expected_total_mass}",
    );

    // ---- 3. (b) global symmetry: |M[i,j] − M[j,i]| < 1e-9·scale ------------
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

    // ---- 4. (c) global PSD via uᵀ M u > 0 over three probes ----------------
    // (c.i) Rigid translation along x: u_{3i} = 1 for all 5 nodes,
    //       u_{3i+1} = u_{3i+2} = 0. Kinetic-energy-of-rigid-translation
    //       invariant: uᵀ M u = ρ · ΣV_e (equality pin — strongest of the
    //       three since an off-by-constant coef error would fire here).
    let mut u_trans = [0.0_f64; 15];
    for node in 0..5 {
        u_trans[3 * node] = 1.0;
    }
    let q_trans = global_quad_form(&m_global, &u_trans);
    assert!(
        (q_trans - expected_total_mass).abs() < 1e-12,
        "rigid-x uᵀMu = {q_trans}, expected ρ·ΣV_e = {expected_total_mass}",
    );

    // (c.ii) Axis-mixed sign-toggle: u_i = (-1)^i. Mixes all three axes and
    //       the sign-mixed pattern guarantees no rigid-mode cancellation —
    //       uᵀ M u must be strictly positive.
    let mut u_sign = [0.0_f64; 15];
    for i in 0..15 {
        u_sign[i] = if i % 2 == 0 { 1.0 } else { -1.0 };
    }
    let q_sign = global_quad_form(&m_global, &u_sign);
    assert!(q_sign > 0.0, "sign-mixed uᵀMu = {q_sign}, expected > 0");

    // (c.iii) Sparse single-DOF probe: u_0 = 1, all others 0. The diagonal
    //       block at node 0 picks up only the m_e0 contribution (node 0 is
    //       not in conn1), so uᵀ M u = m_e0[0,0] = ρ·V_e0/10 > 0.
    let mut u_sparse = [0.0_f64; 15];
    u_sparse[0] = 1.0;
    let q_sparse = global_quad_form(&m_global, &u_sparse);
    assert!(q_sparse > 0.0, "sparse uᵀMu = {q_sparse}, expected > 0");
}
