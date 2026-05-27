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
/// `solve_cg_warm` call between the two — `Arc::clone` bumps the
/// refcount only (no `Vec` copy), so a single 10⁴–10⁶-DOF solve
/// produces exactly one displacement-vector allocation regardless of
/// whether the caller keeps the `CgResult`, the `CgWarmState`, or
/// both.
pub fn solve_cg_with_warm_state(
    k: &faer::sparse::SparseRowMat<usize, f64>,
    f: &[f64],
    prior: Option<&CgWarmState>,
    opts: crate::solver::CgSolverOptions,
    mode: crate::solver::SolverMode,
) -> (crate::solver::CgResult, CgWarmState) {
    let prior_slice = prior.map(|p| p.u.as_slice());
    let result = crate::solver::solve_cg_warm(k, f, prior_slice, opts, mode);
    // Share `u` between the result and the warm state via Arc::clone —
    // refcount bump only, no Vec copy. Both pointers refer to the same
    // 10⁴–10⁶-DOF `Vec<f64>` allocation.
    let fresh = CgWarmState::from_arc(Arc::clone(&result.u));
    (result, fresh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::{CgSolverOptions, SolverMode};
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
            fresh.u, result.u,
            "fresh warm state must wrap the result's displacement"
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
}
