//! Gate-resident solver smoke checks (PRD `docs/prds/offline-deep-test-lane.md`
//! task A3).
//!
//! # Scope
//!
//! This thin binary carries the light solver coverage that STAYS on the
//! merge/task gate while the heavy suite (`determinism.rs`,
//! `analytical_validation.rs`, `modal_benchmarks.rs`, and the OCCT FEA
//! binaries) moves to the offline deep-test lane. Two runtime checks, both
//! driven by a single shared 24×24×8 P1-tet cantilever fixture:
//!
//! 1. **Determinism** — solve in `Deterministic` mode at threads ∈ {1, 2},
//!    asserting EXACT bit-stability (`u.to_bits()` byte-identical; no numeric
//!    floor to clear). Exactness holds by identity:
//!    `resolve_execution_modes(true, t, _)` always returns
//!    `(Deterministic, Deterministic)` regardless of `t` (the
//!    `deterministic || …` short-circuit in `solver.rs::resolve_execution_modes`
//!    ignores thread count), and Deterministic mode's fixed pairwise-tree
//!    reduction produces an identical FP-op sequence for any `t`. This is the
//!    `{1, 2}` subset of the already-green
//!    `determinism.rs::deterministic_displacement_bit_stable_across_repeats_and_thread_counts`
//!    (which covers `{1, 4, 16}`).
//! 2. **One analytical benchmark** at a COARSE tolerance pinned to an
//!    already-passing green bound — reproduces
//!    `analytical_validation.rs::cantilever_beam_p1_tip_deflection_within_5pct_of_timoshenko`
//!    verbatim: L/H=2 stocky cantilever, 24×24×8 hex→6-tet Kuhn split,
//!    faithful distributed end shear + face-averaged tip deflection,
//!    `rel_err ≤ 0.05`. DO NOT tighten below 5% or use a slender P1 column —
//!    5% is the exact tolerance of a landed green test, pinned above the
//!    P1-tet bending-lock floor (~9–10%; non-slender L/H=2 never engages it).
//!
//! # Why one fixture serves both checks
//!
//! The 24×24×8 mesh is mandatory for the analytical bound (smaller meshes
//! exceed 5%: 7.9% at 12³). Its `ndof = 16875` also exceeds
//! `PARALLEL_DOF_THRESHOLD` (10 000), so the determinism check genuinely
//! straddles the parallel-mode boundary.
//!
//! # Per-test-file helper-copy convention
//!
//! Integration test binaries are separate crates and cannot import the
//! module-private helpers in `determinism.rs` / `analytical_validation.rs`.
//! Following the established per-test-file pattern (documented in
//! `determinism.rs`), the small set of mesh/BC/solve helpers needed here are
//! reproduced verbatim below.

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness,
    assemble_global_stiffness, apply_dirichlet_row_elimination,
    DirichletBc, IsotropicElastic,
    solve_cg, CgSolverOptions, CgResult, SolverMode,
    resolve_execution_modes,
};

// ─── determinism smoke test ───────────────────────────────────────────────

/// Byte-identical displacement across threads {1, 2} in Deterministic mode.
///
/// `resolve_execution_modes(true, t, ndof)` → `(Deterministic, Deterministic)`
/// for ALL t, so this is the `{1, 2}` subset of the already-green
/// `determinism.rs::deterministic_displacement_bit_stable_across_repeats_and_thread_counts`
/// (which covers `{1, 4, 16}`). Bit-identical u implies an identical FP-op
/// sequence — the exactness guarantee of the pairwise-tree Deterministic path.
#[test]
fn smoke_determinism_p1_cantilever_bit_stable_1_vs_2_threads() {
    // Verify the fixture builder is callable.
    let _ = cantilever_smoke_fixture();

    let out_t1 = solve_cantilever_smoke(true, 1);
    let out_t2 = solve_cantilever_smoke(true, 2);

    assert!(out_t1.converged, "t=1 did not converge (iter={})", out_t1.iterations);
    assert!(out_t2.converged, "t=2 did not converge (iter={})", out_t2.iterations);

    // Sanity: verify the fixture produced a non-trivial solve. If the tip load
    // is accidentally zeroed, solve_cg short-circuits to u = 0 with
    // iterations == 0 and converged == true — the bit-equality check below
    // would then pass trivially.
    assert!(
        out_t1.u.iter().any(|&x| x != 0.0),
        "t=1 displacement is all zero — tip load may be missing",
    );
    assert!(out_t1.iterations > 0, "CG returned 0 iterations — RHS may be zero");

    assert_eq!(
        out_t1.iterations, out_t2.iterations,
        "deterministic iteration count differs between t=1 ({}) and t=2 ({})",
        out_t1.iterations, out_t2.iterations,
    );

    assert_eq!(out_t1.u.len(), out_t2.u.len(), "u length differs between t=1 and t=2");
    for j in 0..out_t1.u.len() {
        assert_eq!(
            out_t1.u[j].to_bits(), out_t2.u[j].to_bits(),
            "u[{j}] differs between t=1 ({}) and t=2 ({})",
            out_t1.u[j], out_t2.u[j],
        );
    }
}
