//! Acceptance signal (PRD `docs/prds/v0_6/tensegrity-structures.md` §6 / §8.2,
//! Tier-3 leaf T3b): tensegrity load analysis with a tension-only active set.
//!
//! # Cases
//!
//! 1. **No-slack prestressed string** — a collinear two-cable string
//!    `anchor(0) — free(1) — anchor(2)`, each cable carrying tension prestress
//!    `N0`. A transverse tip load `P_t` at the free node produces
//!    `u_y[1] = P_t·L / (2·N0)` (the combined prestressed-string transverse
//!    stiffness is `2·N0/L`), with member forces unchanged to first order and
//!    no cable slackening.
//!
//! 2. **Slackening cable** (added in step-5) — an axial tip load drives one
//!    cable compressive; the tension-only active set drops it and re-solves to
//!    the reduced single-cable system.
//!
//! # Tolerance
//!
//! Inner CG tolerance ≤ 1e-12; deflection assertions ≤ 1e-9 relative. The
//! single/double pin-jointed bar is nodally exact for `K_e`, and
//! `K_g = (N/L)·(I − cc^T)` is the exact transverse prestressed-string
//! stiffness, so the analytic references are method-exact (same basis as T3a's
//! `tests/bar_axial_deflection.rs`).

use reify_solver_elastic::assembly::test_support::assert_close;
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, BarMember, BarSection, CgSolverOptions, DirichletBc, MemberKind,
    SolverMode, TensegrityLoadOptions, apply_dirichlet_row_elimination, apply_point_load,
    assemble_global_stiffness, bar_tangent_stiffness, solve_cg, tensegrity_load_analysis,
};

/// Tight inner-CG options shared across the goldens.
fn tight_options() -> TensegrityLoadOptions {
    TensegrityLoadOptions {
        cg: CgSolverOptions {
            tolerance: 1.0e-12,
            max_iter: 1000,
        },
        ..TensegrityLoadOptions::default()
    }
}

/// (1) No-slack prestressed string under a transverse tip load.
///
/// Nodes `0=(0,0,0)` fixed, `1=(L,0,0)` free, `2=(2L,0,0)` fixed; two collinear
/// cables `(0,1)` and `(1,2)`, each `BarSection{E,A}` with tension prestress
/// `N0`. A transverse load `P_t` at node 1 deflects it by `P_t·L/(2·N0)`:
/// each cable contributes a transverse stiffness `N0/L`, summing to `2·N0/L`.
/// The displacement is purely transverse (the free 3×3 block is diagonal:
/// `diag(2EA/L, 2N0/L, 2N0/L)`), so the axial force delta is exactly zero and
/// each member force stays at `N0`.
#[test]
fn no_slack_prestressed_string_transverse_load() {
    let l = 2.0_f64;
    let e = 200.0e9_f64;
    let a = 1.0e-4_f64;
    let n0 = 5_000.0_f64; // tension prestress [N]
    let p_t = 50.0_f64; // transverse tip load at node 1 [N]

    let nodes = vec![
        [0.0, 0.0, 0.0],     // node 0 — fixed
        [l, 0.0, 0.0],       // node 1 — free
        [2.0 * l, 0.0, 0.0], // node 2 — fixed
    ];
    let members = vec![
        BarMember {
            nodes: (0, 1),
            kind: MemberKind::Cable,
            section: BarSection { youngs_modulus: e, area: a },
            prestress: n0,
        },
        BarMember {
            nodes: (1, 2),
            kind: MemberKind::Cable,
            section: BarSection { youngs_modulus: e, area: a },
            prestress: n0,
        },
    ];
    let loads = vec![[0.0, 0.0, 0.0], [0.0, p_t, 0.0], [0.0, 0.0, 0.0]];
    let fixed_nodes = vec![0, 2];
    let options = tight_options();

    let solve = tensegrity_load_analysis(&nodes, &members, &loads, &fixed_nodes, &options)
        .expect("no-slack prestressed string must be feasible");

    // Combined transverse string stiffness 2·N0/L ⇒ u_y[1] = P_t·L/(2·N0).
    let uy_expected = p_t * l / (2.0 * n0); // = 0.01
    assert_close(
        solve.displacements[1][1],
        uy_expected,
        1e-9,
        "u_y[1] = P_t·L/(2·N0)",
    );
    // Pure transverse response: no axial or out-of-plane motion at node 1.
    assert_close(solve.displacements[1][0], 0.0, 1e-9, "u_x[1] = 0");
    assert_close(solve.displacements[1][2], 0.0, 1e-9, "u_z[1] = 0");
    // Anchored nodes do not move.
    for axis in 0..3 {
        assert_close(solve.displacements[0][axis], 0.0, 1e-12, "node 0 fixed");
        assert_close(solve.displacements[2][axis], 0.0, 1e-12, "node 2 fixed");
    }

    // Transverse motion has zero axial projection ⇒ dN_i = 0 ⇒ N_i = N0.
    assert_close(solve.member_forces[0], n0, 1e-9, "cable (0,1) force ≈ N0");
    assert_close(solve.member_forces[1], n0, 1e-9, "cable (1,2) force ≈ N0");
    assert_close(solve.member_force_deltas[0], 0.0, 1e-9, "dN[0] ≈ 0");
    assert_close(solve.member_force_deltas[1], 0.0, 1e-9, "dN[1] ≈ 0");

    // No cable goes compressive ⇒ the active set converges in a single pass.
    assert_eq!(solve.slack, vec![false, false], "no slack cables");
    assert!(solve.converged, "solve must converge");
    assert_eq!(
        solve.active_set_iterations, 1,
        "no drop ⇒ exactly one active-set pass",
    );
}

/// Independent reduced-topology reference: assemble ONLY cable `(0,1)` (the one
/// that stays taut after the slack cable is dropped), pin nodes 0 and 2, apply
/// the axial load `P` at node 1, and solve with the public assembly/CG
/// primitives. Returns `u_x[1]`. This is the exact linear system the
/// tension-only active set must converge to after dropping cable `(1,2)` —
/// computed without touching `tensegrity_load_analysis`, so it is a genuine
/// cross-check of the kernel's post-drop deflection.
fn reduced_single_cable_ux1(l: f64, e: f64, a: f64, n0: f64, p: f64) -> f64 {
    let nodes = [[0.0, 0.0, 0.0], [l, 0.0, 0.0], [2.0 * l, 0.0, 0.0]];
    let section = BarSection { youngs_modulus: e, area: a };
    let kt = bar_tangent_stiffness(&[nodes[0], nodes[1]], &section, n0);
    let conn = [0usize, 1];
    let elem = AssemblyElement { id: 0, connectivity: &conn, k_e: &kt };
    let mut k = assemble_global_stiffness(3, &[elem], AssemblyMode::Deterministic);

    let mut f = vec![0.0_f64; 9];
    apply_point_load(&mut f, 1, [p, 0.0, 0.0]);

    // Pin nodes 0 and 2 in all three axes.
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for &node in &[0usize, 2usize] {
        for axis in 0..3 {
            bcs.push(DirichletBc { dof: 3 * node + axis, value: 0.0 });
        }
    }
    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    let result = solve_cg(
        &k,
        &f,
        CgSolverOptions { tolerance: 1.0e-12, max_iter: 1000 },
        SolverMode::Deterministic,
    );
    assert!(result.converged, "reduced single-cable solve must converge");
    result.u()[3] // u_x at node 1
}

/// (2) Slackening cable under an axial tip load.
///
/// Same collinear two-cable string, but the tip load `P = 3·N0` is **axial**
/// (toward node 2). Before any drop the two-cable axial stiffness is `2·EA/L`,
/// giving `u_x = P·L/(2EA)` and cable `(1,2)` force `N0 − P/2 = −N0/2 < 0` — it
/// has gone slack and must be dropped. After the drop only cable `(0,1)`
/// resists, so `u_x[1] = P·L/(EA)` (the reduced single-cable analytic, NOT the
/// two-cable `P·L/(2EA)`), cable `(0,1)` carries `N0 + P`, and cable `(1,2)`
/// reports zero force with `slack == true`. The active set takes two passes
/// (drop, then confirm the fixed point), and the post-drop deflection matches
/// an independent reduced-topology assemble/solve.
#[test]
fn slackening_cable_axial_load() {
    let l = 2.0_f64;
    let e = 200.0e9_f64;
    let a = 1.0e-4_f64;
    let n0 = 5_000.0_f64;
    let p = 3.0 * n0; // axial tip load toward node 2 [N]

    let nodes = vec![
        [0.0, 0.0, 0.0],
        [l, 0.0, 0.0],
        [2.0 * l, 0.0, 0.0],
    ];
    let members = vec![
        BarMember {
            nodes: (0, 1),
            kind: MemberKind::Cable,
            section: BarSection { youngs_modulus: e, area: a },
            prestress: n0,
        },
        BarMember {
            nodes: (1, 2),
            kind: MemberKind::Cable,
            section: BarSection { youngs_modulus: e, area: a },
            prestress: n0,
        },
    ];
    let loads = vec![[0.0, 0.0, 0.0], [p, 0.0, 0.0], [0.0, 0.0, 0.0]];
    let fixed_nodes = vec![0, 2];
    let options = tight_options();

    let solve = tensegrity_load_analysis(&nodes, &members, &loads, &fixed_nodes, &options)
        .expect("slackening string must still be feasible (one cable stays taut)");

    // Cable (1,2) goes compressive and is dropped: zero force, slack flagged,
    // and its force delta cancels the prestress exactly.
    assert!(solve.slack[1], "cable (1,2) must be flagged slack");
    assert!(!solve.slack[0], "cable (0,1) stays taut");
    assert_eq!(solve.member_forces[1], 0.0, "slack cable reports zero force");
    assert_close(
        solve.member_force_deltas[1],
        -n0,
        1e-9,
        "slack cable dN = −prestress (total force falls to 0)",
    );

    // Reduced single-cable deflection: u_x[1] = P·L/(EA), NOT P·L/(2EA).
    let ux_expected = p * l / (e * a); // = 0.0015
    assert_close(solve.displacements[1][0], ux_expected, 1e-9, "u_x[1] = P·L/(EA)");
    // Independent reduced-topology cross-check (no active-set logic involved).
    let ux_cross = reduced_single_cable_ux1(l, e, a, n0, p);
    assert_close(
        solve.displacements[1][0],
        ux_cross,
        1e-9,
        "u_x[1] matches independent reduced-topology solve",
    );

    // Taut cable (0,1) carries N0 + P; its delta is the full P.
    assert_close(solve.member_forces[0], n0 + p, 1e-9, "cable (0,1) force = N0 + P");
    assert_close(solve.member_force_deltas[0], p, 1e-9, "cable (0,1) dN = P");

    // Two passes: pass 1 drops (1,2); pass 2 confirms the fixed point.
    assert!(solve.converged, "post-drop solve must converge");
    assert_eq!(
        solve.active_set_iterations, 2,
        "one drop ⇒ two active-set passes",
    );
}
