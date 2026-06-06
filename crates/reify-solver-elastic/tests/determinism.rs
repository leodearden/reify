//! FEA determinism harness — representative ~50K-DOF cantilever model.
//!
//! # Scope (PRD task #19, `docs/prds/v0_3/structural-analysis-fea.md`)
//!
//! This is a CI characterization harness over the already-merged
//! `#deterministic` plumbing (task #2926, PRD task #18). It extends the
//! tiny-scale unit tests in `crates/reify-solver-elastic/src/solver.rs`
//! (`deterministic_back_to_back_bit_stable`,
//! `parallel_shared_dof_k_tolerance_equivalent_and_back_to_back_bit_stable`,
//! `resolve_execution_modes_drives_bit_stable_and_equivalent_solves`) to a
//! representative ~50K-DOF model where:
//!
//! - The parallel SpMV/dot path (PAR_THRESHOLD = 1024) genuinely engages.
//! - The `PARALLEL_DOF_THRESHOLD`-driven mode selection (10 000 DOFs) fires.
//!
//! On this fixture the harness asserts:
//!
//! - **Under `#deterministic`** (`resolve_execution_modes(true, _, _)` →
//!   `(Deterministic, Deterministic)` for all thread counts):
//!   displacement `u`, recovered nodal stress field, and max von Mises are
//!   byte-identical across threads ∈ {1, 4, 16} (simultaneously covering
//!   "repeated runs" and "across thread counts" since Deterministic mode
//!   ignores the thread argument); CG iteration counts are also equal.
//!
//! - **Under default / parallel mode** (`resolve_execution_modes(false, t,
//!   ndof)` → `Parallel{t}` for t > 1 and ndof ≥ `PARALLEL_DOF_THRESHOLD`):
//!   result fields are tolerance-equivalent (relative-L2 ≤ `EQUIV_TOL = 1e-3`)
//!   across 3 repeated runs at fixed threads=4, and across thread counts
//!   {1, 4, 16}. Iteration-count equality is NOT asserted in parallel mode
//!   (round-off shifts the convergence step across thread counts).
//!
//! ## Achievability basis for EQUIV_TOL
//!
//! Two CG solves converged to relative residual `cg_tol = 1e-10` satisfy
//! ‖Δu‖/‖u‖ ≲ cond(K)·cg_tol (backward-error bound). For a stocky 3-D
//! continuum at ~50K DOF cond(K) ~ O(1e3–1e4), giving an expected
//! cross-reduction-order difference ≲ ~1e-6–1e-5 — at least 2 orders below
//! `EQUIV_TOL = 1e-3`. The 1e-3 bound still detects any real non-determinism
//! leak, which manifests as O(1)/NaN differences, not 1e-4-level ones.
//!
//! ## Cross-machine coverage
//!
//! The CI matrix executing this same within-process test on each machine
//! provides the cross-machine dimension. A committed golden u-vector is
//! intentionally omitted (architecture-fragile due to FMA contraction /
//! libm differences; out of scope per PRD task #19).
//!
//! ## Thread count portability
//!
//! `SolverMode::Parallel{threads}` spawns exactly the requested worker count
//! via `std::thread::scope` (chunk = n.div_ceil(threads)) regardless of
//! physical core count, so {1, 4, 16} runs correctly on any CI machine (16
//! merely oversubscribes a 4-core box). `resolve_execution_modes(false, 1, …)`
//! → Deterministic (policy: `threads <= 1`), so the t=1 sweep entry is
//! bit-identical to the deterministic reference.

#[allow(unused_imports)]
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness,
    assemble_global_stiffness, apply_dirichlet_row_elimination,
    DirichletBc, IsotropicElastic,
    StressElement, element_stress_p1, recover_nodal_stress_p1, tet_volume_p1,
    solve_cg, CgSolverOptions, CgResult, SolverMode,
    resolve_execution_modes, PARALLEL_DOF_THRESHOLD,
};

// ─── step-1 RED ──────────────────────────────────────────────────────────────

/// Byte-identical displacement across repeated runs and thread counts in
/// deterministic mode.
///
/// `resolve_execution_modes(true, t, ndof)` → `(Deterministic, Deterministic)`
/// for ALL t, so threads ∈ {1, 4, 16} simultaneously exercises "repeated runs"
/// and "across thread counts". Bit-identical u implies an identical FP-op
/// sequence — the exactness guarantee of the pairwise-tree Deterministic path.
///
/// Note: `solve_box_cantilever` and `box_cantilever_fixture` are not yet
/// defined — this test fails to compile (RED).
#[test]
fn deterministic_displacement_bit_stable_across_repeats_and_thread_counts() {
    // Verify the fixture builder is callable (missing → compile error).
    let _fixture = box_cantilever_fixture();

    let out1 = solve_box_cantilever(true, 1);
    let out4 = solve_box_cantilever(true, 4);
    let out16 = solve_box_cantilever(true, 16);

    // All runs must converge.
    assert!(out1.converged, "deterministic t=1 did not converge (iter={})", out1.iterations);
    assert!(out4.converged, "deterministic t=4 did not converge (iter={})", out4.iterations);
    assert!(out16.converged, "deterministic t=16 did not converge (iter={})", out16.iterations);

    // Iteration counts must be equal (same FP sequence → same convergence step).
    assert_eq!(
        out1.iterations, out4.iterations,
        "deterministic iteration count differs between t=1 ({}) and t=4 ({})",
        out1.iterations, out4.iterations,
    );
    assert_eq!(
        out1.iterations, out16.iterations,
        "deterministic iteration count differs between t=1 ({}) and t=16 ({})",
        out1.iterations, out16.iterations,
    );

    // Displacement vector must be byte-identical (f64::to_bits slot-wise).
    assert_eq!(out1.u.len(), out4.u.len(), "u length differs between t=1 and t=4");
    assert_eq!(out1.u.len(), out16.u.len(), "u length differs between t=1 and t=16");
    for i in 0..out1.u.len() {
        assert_eq!(
            out1.u[i].to_bits(), out4.u[i].to_bits(),
            "u[{i}] differs between t=1 ({}) and t=4 ({})",
            out1.u[i], out4.u[i],
        );
        assert_eq!(
            out1.u[i].to_bits(), out16.u[i].to_bits(),
            "u[{i}] differs between t=1 ({}) and t=16 ({})",
            out1.u[i], out16.u[i],
        );
    }
}
