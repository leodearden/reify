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
    BarMember, BarSection, CgSolverOptions, MemberKind, TensegrityLoadOptions,
    tensegrity_load_analysis,
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
