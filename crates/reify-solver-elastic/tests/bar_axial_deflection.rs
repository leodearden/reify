//! Acceptance signal (PRD `docs/prds/v0_6/tensegrity-structures.md` §6 task T3a):
//! single prestressed bar axial and transverse deflection match analytic solutions.
//!
//! # Setup
//!
//! A single bar element from `(0,0,0)` to `(L,0,0)` with `BarSection{E,A}` and
//! pre-stress `N` (tension). Tangent stiffness assembled via
//! `bar_tangent_stiffness` → `assemble_global_stiffness`. Node 0 is fully fixed
//! (3 Dirichlet BCs, homogeneous). A load is applied at node 1.
//!
//! # Analytic solutions
//!
//! 1. **Axial load `P`** (along x): `δ = PL/(EA)` — exact for a single linear bar.
//! 2. **Transverse load `P_t`** (along y, prestressed-string stiffness):
//!    `δ_t = P_t·L/N` — the transverse stiffness `N/L` from `K_g` gives exactly
//!    this displacement for a single linear bar.
//!
//! # Tolerance
//!
//! CG tolerance ≤ 1e-12; deflection assertions ≤ 1e-9 relative
//! (method-bound: one-element bar is nodally exact for K_e; the prestressed-string
//! stiffness `K_g = (N/L)·T` is also exact for a single linear bar).

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, BarSection, CgSolverOptions, DirichletBc, SolverMode,
    apply_dirichlet_row_elimination, apply_point_load, assemble_global_stiffness,
    bar_tangent_stiffness, solve_cg,
};

const L: f64 = 2.5; // bar length [m]
const E: f64 = 200.0e9; // Young's modulus [Pa]
const A: f64 = 1.0e-4; // cross-sectional area [m²]
const N_PRESTRESS: f64 = 5_000.0; // axial prestress (tension) [N]

/// Helper: build the tangent stiffness, apply BCs, solve, and return the
/// displacement vector `u` (length 6: [u0x,u0y,u0z, u1x,u1y,u1z]).
fn solve_single_bar(force: [f64; 3]) -> Vec<f64> {
    let nodes: [[f64; 3]; 2] = [[0.0, 0.0, 0.0], [L, 0.0, 0.0]];
    let section = BarSection { youngs_modulus: E, area: A };

    // Per-member tangent K_t = K_e + K_g
    let kt = bar_tangent_stiffness(&nodes, &section, N_PRESTRESS);

    // Wrap in AssemblyElement and build the 6×6 global stiffness
    let elem = AssemblyElement { id: 0, connectivity: &[0, 1], k_e: &kt };
    let mut k_global = assemble_global_stiffness(2, &[elem], AssemblyMode::Deterministic);
    let mut f = vec![0.0_f64; 6];

    // Apply tip load at node 1
    apply_point_load(&mut f, 1, force);

    // Fix node 0: pin all three DOFs (x, y, z = 0)
    let bcs = [
        DirichletBc { dof: 0, value: 0.0 },
        DirichletBc { dof: 1, value: 0.0 },
        DirichletBc { dof: 2, value: 0.0 },
    ];
    apply_dirichlet_row_elimination(&mut k_global, &mut f, &bcs);

    // Solve with tight CG tolerance
    let opts = CgSolverOptions { tolerance: 1.0e-12, max_iter: 1000 };
    let result = solve_cg(&k_global, &f, opts, SolverMode::Deterministic);
    assert!(result.converged, "CG did not converge: {} iters", result.iterations);
    result.u.to_vec()
}

/// Relative-tolerance assert matching the crate convention.
fn assert_close(lhs: f64, rhs: f64, tol: f64, label: &str) {
    let scale = lhs.abs().max(rhs.abs()).max(1.0);
    assert!(
        (lhs - rhs).abs() < tol * scale,
        "{label}: |{lhs} − {rhs}| = {} ≥ tol·scale = {}",
        (lhs - rhs).abs(),
        tol * scale,
    );
}

/// (1) Axial load P along x: analytic δ = PL/(EA).
///
/// For a single linear bar element, the CG solution is nodally exact.
/// K_e(0,0) = EA/L, so u_x[1] = P/(EA/L) = PL/(EA). No transverse coupling
/// from K_e or K_g (T=(I−cc^T) is zero in the axial direction).
#[test]
fn axial_load_matches_pl_over_ea() {
    let p = 1_000.0; // 1 kN axial load at node 1
    let u = solve_single_bar([p, 0.0, 0.0]);

    let delta_analytic = p * L / (E * A); // δ = PL/(EA)
    // Tip axial displacement
    assert_close(u[3], delta_analytic, 1e-9, "u_x[node1] = PL/(EA)");
    // Transverse and fixed-node DOFs must be zero
    assert_close(u[4], 0.0, 1e-9, "u_y[node1] = 0 under axial load");
    assert_close(u[5], 0.0, 1e-9, "u_z[node1] = 0 under axial load");
    assert_close(u[0], 0.0, 1e-12, "u_x[node0] = 0 (fixed)");
    assert_close(u[1], 0.0, 1e-12, "u_y[node0] = 0 (fixed)");
    assert_close(u[2], 0.0, 1e-12, "u_z[node0] = 0 (fixed)");
}

/// (2) Transverse load P_t along y: analytic δ_t = P_t · L / N.
///
/// With K_g = (N/L)·T and T_yy = 1 for an x-aligned bar, the transverse
/// stiffness at node 1 is N/L. So u_y[1] = P_t / (N/L) = P_t·L/N.
/// This confirms K_g contributes the expected prestressed-string transverse
/// stiffness.
#[test]
fn transverse_load_matches_pt_l_over_n() {
    let p_t = 50.0; // transverse tip load [N]
    let u = solve_single_bar([0.0, p_t, 0.0]);

    let delta_t_analytic = p_t * L / N_PRESTRESS; // δ_t = P_t·L/N
    // Tip transverse displacement
    assert_close(u[4], delta_t_analytic, 1e-9, "u_y[node1] = P_t·L/N");
    // Axial and z DOFs must be zero
    assert_close(u[3], 0.0, 1e-9, "u_x[node1] = 0 under transverse load");
    assert_close(u[5], 0.0, 1e-9, "u_z[node1] = 0 under transverse y-load");
    // Fixed node 0 must not move
    for (i, &u_val) in u[..3].iter().enumerate() {
        assert_close(u_val, 0.0, 1e-12, &format!("u[node0 dof {i}] = 0 (fixed)"));
    }
}
