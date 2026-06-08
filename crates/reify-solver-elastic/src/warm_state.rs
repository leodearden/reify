//! FEA-solver warm-state shape and OpaqueState conversion (PRD task #14).
//!
//! Per PRD `docs/prds/v0_3/structural-analysis-fea.md` task #14, the
//! Jacobi-CG warm state is "just `u_0` — preconditioner is trivial to
//! recompute" (full direct-solve symbolic-factorization caching is
//! explicitly out of scope for v0.3). This module owns the engine-
//! integration surface (`OpaqueState` conversion) so the pure-numerics
//! `solver.rs` stays free of `reify-types` dependency surface.

use std::sync::Arc;

/// Warm-start state for the Jacobi-preconditioned CG solver.
///
/// Carries the displacement vector `u` from a previous solve so it can be
/// passed as the CG initial guess `u_0` on the next solve. The Jacobi
/// preconditioner itself is trivially recomputed from `K`, so it is not
/// part of the state.
///
/// `u` is wrapped in `Arc<Vec<f64>>` so [`solve_cg_with_warm_state`] can
/// share the single allocation produced by the solver between the
/// returned [`crate::CgResult`] and this warm state — avoiding a
/// 10⁴–10⁶-DOF `Vec<f64>` copy on every solve. All read paths work
/// through `Deref`: `state.u[i]`, `state.u.len()`, `state.u.iter()`,
/// `&state.u[..]`. Cloning a `CgWarmState` bumps the refcount instead
/// of deep-copying `u`, which is the desired behaviour for the
/// `WarmStatePool` donate→checkout cycle (logically immutable state
/// that may be observed by multiple consumers concurrently).
#[derive(Debug, Clone, PartialEq)]
pub struct CgWarmState {
    /// Displacement vector from the previous solve, used as `u_0` on the
    /// next call. Shared via `Arc` with the originating
    /// [`crate::CgResult`] when constructed by [`solve_cg_with_warm_state`].
    pub u: Arc<Vec<f64>>,
}

impl CgWarmState {
    /// Construct a warm state from an owned displacement vector. Wraps
    /// `u` in a fresh `Arc<Vec<f64>>`. Use [`Self::from_arc`] if the
    /// caller already holds an `Arc` (e.g. shared with a `CgResult`).
    pub fn from_displacement(u: Vec<f64>) -> Self {
        Self { u: Arc::new(u) }
    }

    /// Construct a warm state from an `Arc<Vec<f64>>` directly, sharing
    /// the existing allocation. This is the zero-copy path used by
    /// [`solve_cg_with_warm_state`] to share `u` with the originating
    /// [`crate::CgResult`] (refcount bump only — no `Vec` copy).
    pub fn from_arc(u: Arc<Vec<f64>>) -> Self {
        Self { u }
    }

    /// Estimated heap-payload size in bytes for `WarmStatePool` budget
    /// enforcement. Counts only the `Vec<f64>` payload (`u.len() *
    /// size_of::<f64>()`); the constant ~24-byte `Vec` heap header, the
    /// `Arc` heap node, and the struct's own stack overhead are
    /// negligible relative to typical FEA solution vectors (10⁴ – 10⁶ DOFs).
    pub fn estimated_size_bytes(&self) -> usize {
        self.u.len() * std::mem::size_of::<f64>()
    }

    /// Wrap this warm state in an `OpaqueState` for storage in
    /// `WarmStatePool`. The size hint is `estimated_size_bytes()`.
    pub fn into_opaque_state(self) -> reify_ir::OpaqueState {
        let bytes = self.estimated_size_bytes();
        reify_ir::OpaqueState::new(self, bytes)
    }

    /// Attempt to recover a `CgWarmState` from an `OpaqueState`. Returns
    /// `None` if the inner type is not `CgWarmState` (the caller should
    /// silently treat that as a cold start, per the `WarmStartable`
    /// best-effort contract).
    pub fn from_opaque_state(state: reify_ir::OpaqueState) -> Option<Self> {
        state.downcast::<Self>()
    }
}

/// High-level Jacobi-CG producer wrapper with per-iteration progress callback
/// (task #4079).
///
/// Identical to [`solve_cg_with_warm_state`] but forwards a `progress`
/// callback to [`crate::solver::solve_cg_with_progress`], enabling
/// per-iteration emit and `CgIterationControl::Cancel` interruption.
///
/// # Allocation sharing
///
/// Same zero-copy Arc-share contract as [`solve_cg_with_warm_state`]:
/// `CgResult.u` and `CgWarmState.u` are the same `Arc<Vec<f64>>`
/// (refcount bump only, no Vec copy).
pub fn solve_cg_with_warm_state_progress(
    k: &faer::sparse::SparseRowMat<usize, f64>,
    f: &[f64],
    prior: Option<&CgWarmState>,
    opts: crate::solver::CgSolverOptions,
    mode: crate::solver::SolverMode,
    progress: &mut dyn FnMut(usize, f64) -> crate::solver::CgIterationControl,
) -> (crate::solver::CgResult, CgWarmState) {
    let prior_slice = prior.map(|p| p.u.as_slice());
    let result = crate::solver::solve_cg_with_progress(k, f, prior_slice, opts, mode, progress);
    // `CgResult.u` is private (accessor-only); `shared_u()` returns the
    // `Arc<Vec<f64>>` with a refcount bump only (no Vec copy), preserving the
    // single-allocation donate contract.
    let fresh = CgWarmState::from_arc(result.shared_u());
    (result, fresh)
}

/// High-level Jacobi-CG producer wrapper for engine wiring (PRD task #14).
///
/// Solves `K·u = f` with optional prior warm state, and emits both the
/// `CgResult` and a fresh `CgWarmState` containing the new solution `u`.
/// This is the producer-side template that ComputeNode wiring (PRD task
/// #16, `solve_elastic_static @optimized`) will call: it returns BOTH
/// the result and the next-call warm state, so the engine never has to
/// peek inside `CgResult` to wrap `u`.
///
/// Returning `CgWarmState` (not `OpaqueState`) lets callers inspect the
/// result before deciding whether to donate; conversion to `OpaqueState`
/// is one further `into_opaque_state()` call away.
///
/// # Allocation sharing
///
/// `CgResult.u` and `CgWarmState.u` are both `Arc<Vec<f64>>`. This
/// function shares the single allocation produced by the underlying
/// solver call between the two — `Arc::clone` bumps the refcount only
/// (no `Vec` copy), so a single 10⁴–10⁶-DOF solve produces exactly one
/// displacement-vector allocation regardless of whether the caller keeps
/// the `CgResult`, the `CgWarmState`, or both.
///
/// Delegates to [`solve_cg_with_warm_state_progress`] with a no-op
/// closure so the Arc-share/donate contract is maintained in one place.
pub fn solve_cg_with_warm_state(
    k: &faer::sparse::SparseRowMat<usize, f64>,
    f: &[f64],
    prior: Option<&CgWarmState>,
    opts: crate::solver::CgSolverOptions,
    mode: crate::solver::SolverMode,
) -> (crate::solver::CgResult, CgWarmState) {
    solve_cg_with_warm_state_progress(k, f, prior, opts, mode, &mut |_, _| {
        crate::solver::CgIterationControl::Continue
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::{CgIterationControl, CgSolverOptions, SolverMode};
    use faer::sparse::{SparseRowMat, Triplet};

    /// `CgWarmState::from_opaque_state` returns `None` (rather than
    /// panicking) when the inner `OpaqueState` value is not a
    /// `CgWarmState`. Pins the `WarmStartable` best-effort contract
    /// referenced in the doc comment of `from_opaque_state`: a caller
    /// that gets `None` should silently treat the state as a cold start.
    #[test]
    fn cg_warm_state_from_opaque_state_returns_none_on_type_mismatch() {
        // OpaqueState wrapping an i32 — clearly not a CgWarmState.
        let opaque = reify_ir::OpaqueState::new(42_i32, std::mem::size_of::<i32>());
        let restored = CgWarmState::from_opaque_state(opaque);
        assert!(
            restored.is_none(),
            "from_opaque_state must return None on type mismatch (got Some)"
        );
    }

    /// `CgWarmState::from_displacement(u)` → `into_opaque_state()` →
    /// `from_opaque_state()` round-trips the displacement vector unchanged.
    /// Also pins the `estimated_size_bytes` formula
    /// (`u.len() * size_of::<f64>()`).
    #[test]
    fn cg_warm_state_round_trips_through_opaque_state() {
        let u = vec![1.0_f64, 2.0, 3.0];
        let ws = CgWarmState::from_displacement(u.clone());
        let opaque = ws.into_opaque_state();
        let restored = CgWarmState::from_opaque_state(opaque).expect("downcast");
        // restored.u is Arc<Vec<f64>>; compare its dereferenced contents
        // to the original Vec<f64>.
        assert_eq!(*restored.u, u);

        assert_eq!(
            CgWarmState::from_displacement(vec![0.0_f64; 5]).estimated_size_bytes(),
            5 * std::mem::size_of::<f64>(),
        );
    }

    /// `solve_cg_with_warm_state(k, f, None, opts, mode)` returns a
    /// `(CgResult, CgWarmState)` pair where:
    /// - the result converged,
    /// - `fresh.u == result.u` (the producer wrapped the result's
    ///   displacement),
    /// - calling again with `Some(&fresh)` against the same `(k, f)`
    ///   returns iterations == 0 (warm at the exact solution — pinned
    ///   by `solver::tests::warm_start_at_exact_solution_returns_in_zero_iterations`,
    ///   exercised here through the high-level wrapper).
    #[test]
    fn solve_cg_with_warm_state_returns_result_and_fresh_state() {
        // 2×2 SPD fixture: same triplets as
        // solver::tests::hand_computed_2x2_spd_within_tolerance — rebuilt
        // here because that helper is private to solver.rs.
        let k = SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 3.0_f64),
            ],
        )
        .unwrap();
        let f = [1.0_f64, 2.0];
        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 100,
        };

        // Cold call: prior = None.
        let (result, fresh) =
            solve_cg_with_warm_state(&k, &f, None, opts.clone(), SolverMode::Deterministic);
        assert!(
            result.converged,
            "cold solve_cg_with_warm_state must converge"
        );
        assert_eq!(
            result.u(), fresh.u.as_slice(),
            "fresh warm state must wrap the result's displacement"
        );
        assert!(
            Arc::ptr_eq(&result.shared_u(), &fresh.u),
            "fresh warm state must share the same Arc allocation as result (zero-copy)"
        );

        // Re-solve with prior = Some(&fresh) on the same (k, f) — already at
        // the exact solution, so the early-exit fires.
        let (result_warm, _fresh2) =
            solve_cg_with_warm_state(&k, &f, Some(&fresh), opts, SolverMode::Deterministic);
        assert_eq!(
            result_warm.iterations, 0,
            "warm at exact solution must return 0 iterations, got {}",
            result_warm.iterations
        );
        assert!(
            result_warm.converged,
            "warm at exact solution must report converged"
        );
    }

    // ── step-1 tests for solve_cg_with_warm_state_progress ──────────────────

    /// Helper: build the 2×2 SPD fixture [[4,1],[1,3]] with RHS [1,2].
    fn make_2x2_fixture() -> (SparseRowMat<usize, f64>, Vec<f64>, CgSolverOptions) {
        let k = SparseRowMat::try_new_from_triplets(
            2,
            2,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, 1.0_f64),
                Triplet::new(1_usize, 0_usize, 1.0_f64),
                Triplet::new(1_usize, 1_usize, 3.0_f64),
            ],
        )
        .unwrap();
        let f = vec![1.0_f64, 2.0];
        let opts = CgSolverOptions { tolerance: 1e-10, max_iter: 100 };
        (k, f, opts)
    }

    /// Test A: cold solve with a recording progress closure.
    /// - result.converged == true
    /// - recorded iters are 1-indexed (start at 1) with len == result.iterations
    /// - last residual is finite and small
    /// - fresh.u == result.u (Arc-shared donate)
    #[test]
    fn solve_cg_with_warm_state_progress_records_iters_and_donates() {
        let (k, f, opts) = make_2x2_fixture();
        let mut records: Vec<(usize, f64)> = Vec::new();

        let (result, fresh) = solve_cg_with_warm_state_progress(
            &k,
            &f,
            None,
            opts,
            SolverMode::Deterministic,
            &mut |iter, residual| {
                records.push((iter, residual));
                CgIterationControl::Continue
            },
        );

        assert!(result.converged, "cold solve must converge");
        assert!(
            !records.is_empty(),
            "progress closure must have been called at least once"
        );
        // iters must be 1-indexed
        assert_eq!(
            records[0].0, 1,
            "first recorded iter must be 1 (1-indexed), got {}",
            records[0].0
        );
        assert_eq!(
            records.len(),
            result.iterations,
            "recorded count must equal result.iterations: {} vs {}",
            records.len(),
            result.iterations
        );
        let last_residual = records.last().unwrap().1;
        assert!(
            last_residual.is_finite(),
            "last residual must be finite"
        );
        assert!(
            last_residual < 1e-5,
            "last residual must be small, got {}",
            last_residual
        );
        assert!(
            Arc::ptr_eq(&fresh.u, &result.shared_u()),
            "fresh warm state must share result.u (Arc donate)"
        );
    }

    /// Test B: warm re-solve at the exact solution → 0 iterations, closure not invoked.
    #[test]
    fn solve_cg_with_warm_state_progress_warm_resolves_in_zero_iters() {
        let (k, f, opts) = make_2x2_fixture();

        // Cold solve to get the warm state.
        let (_, fresh) = solve_cg_with_warm_state_progress(
            &k,
            &f,
            None,
            opts.clone(),
            SolverMode::Deterministic,
            &mut |_, _| CgIterationControl::Continue,
        );

        let mut records: Vec<(usize, f64)> = Vec::new();
        let (result_warm, _) = solve_cg_with_warm_state_progress(
            &k,
            &f,
            Some(&fresh),
            opts,
            SolverMode::Deterministic,
            &mut |iter, residual| {
                records.push((iter, residual));
                CgIterationControl::Continue
            },
        );

        assert_eq!(
            result_warm.iterations, 0,
            "warm at exact solution must return 0 iterations, got {}",
            result_warm.iterations
        );
        assert!(result_warm.converged, "warm at exact solution must be converged");
        assert!(
            records.is_empty(),
            "closure must NOT be invoked when 0 iterations are needed"
        );
    }

    /// Test C: closure returning Cancel on first call → converged==false, iterations==1.
    #[test]
    fn solve_cg_with_warm_state_progress_cancel_on_first_iter() {
        let (k, f, opts) = make_2x2_fixture();

        let (result, _fresh) = solve_cg_with_warm_state_progress(
            &k,
            &f,
            None,
            opts,
            SolverMode::Deterministic,
            &mut |_, _| CgIterationControl::Cancel,
        );

        assert!(!result.converged, "cancelled solve must not converge");
        assert_eq!(
            result.iterations, 1,
            "cancelled on first iteration must show iterations==1, got {}",
            result.iterations
        );
    }

    // ── step-3 (task 4366): equivalence test for both entry points ─────────────

    /// A multi-iteration COLD solve yields the SAME iteration count and
    /// displacement vector via both `solve_cg_with_warm_state` (no-op closure)
    /// and `solve_cg_with_warm_state_progress` (recording closure).
    ///
    /// Both entry points share `cg_loop`, so equality is guaranteed by
    /// construction. This test is a regression guard — it passes on current
    /// correct code and locks the no-op-closure equivalence so a future
    /// option-(b) fast-path (a direct `solve_cg_warm` bypassing the closure
    /// indirection) cannot silently diverge from the progress variant.
    ///
    /// The 3×3 tridiagonal SPD fixture [[4,-1,0],[-1,4,-1],[0,-1,4]] with
    /// RHS [1,2,3] provably needs >1 CG iteration (verified by
    /// `solver::tests::solve_cg_with_progress_multi_iteration_callback_sequence`),
    /// so `iterations > 1` is non-vacuous here.
    #[test]
    fn cold_solve_same_iteration_count_via_both_entry_points() {
        // 3×3 tridiagonal SPD: same fixture as
        // solver::tests::solve_cg_with_progress_multi_iteration_callback_sequence.
        let k = SparseRowMat::try_new_from_triplets(
            3,
            3,
            &[
                Triplet::new(0_usize, 0_usize, 4.0_f64),
                Triplet::new(0_usize, 1_usize, -1.0_f64),
                Triplet::new(1_usize, 0_usize, -1.0_f64),
                Triplet::new(1_usize, 1_usize, 4.0_f64),
                Triplet::new(1_usize, 2_usize, -1.0_f64),
                Triplet::new(2_usize, 1_usize, -1.0_f64),
                Triplet::new(2_usize, 2_usize, 4.0_f64),
            ],
        )
        .unwrap();
        let f = vec![1.0_f64, 2.0_f64, 3.0_f64];
        let opts = CgSolverOptions { tolerance: 1e-12, max_iter: 100 };

        // Entry point A: plain no-op-closure path.
        let (result_a, _) = solve_cg_with_warm_state(
            &k,
            &f,
            None,
            opts.clone(),
            SolverMode::Deterministic,
        );

        // Entry point B: progress-recording path.
        let mut records: Vec<(usize, f64)> = Vec::new();
        let (result_b, _) = solve_cg_with_warm_state_progress(
            &k,
            &f,
            None,
            opts,
            SolverMode::Deterministic,
            &mut |i, r| {
                records.push((i, r));
                CgIterationControl::Continue
            },
        );

        // Both must converge.
        assert!(result_a.converged, "entry-point-A cold solve must converge");
        assert!(result_b.converged, "entry-point-B cold solve must converge");

        // Iteration counts must be identical (both share cg_loop).
        assert_eq!(
            result_a.iterations,
            result_b.iterations,
            "both entry points must perform the same number of CG iterations: {} vs {}",
            result_a.iterations,
            result_b.iterations
        );

        // The solve is genuinely multi-iteration (locks the gap current tests left).
        assert!(
            result_a.iterations > 1,
            "3×3 tridiagonal fixture must need >1 CG iteration, got {}",
            result_a.iterations
        );

        // The recording closure was invoked once per iteration.
        assert_eq!(
            records.len(),
            result_b.iterations,
            "progress closure must be called once per iteration: {} records vs {} iters",
            records.len(),
            result_b.iterations
        );

        // Displacement vectors are byte-identical.
        assert_eq!(
            result_a.u(),
            result_b.u(),
            "both entry points must produce identical displacement vectors"
        );
    }
}
