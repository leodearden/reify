//! End-to-end tests for the per-CG-iteration solver-progress emit path
//! (task 4079, steps 7–8).
//!
//! Two behaviours under test:
//!
//!   **Test A (progress emit)** — `engine.eval()` on the cantilever smoke
//!   fixture emits ≥1 `SolverProgressUpdate` with `iter ≥ 1`, a finite
//!   residual, and `solver_kind == "cg"` to an installed `SolverProgressSink`.
//!
//!   **Test B (cancel interruption)** — pre-cancelling the handle installed via
//!   `engine.set_active_solve_cancel(Some(h))` causes the trampoline to return
//!   `ComputeOutcome::Cancelled`; the `solver::elastic_static` output VC is
//!   left `Freshness::Pending` (NOT Final).
//!
//! Both tests FAIL on the base branch (step-7 RED):
//! - Test A: the sink receives 0 updates (trampoline calls `solve_cg_with_warm_state`,
//!   which has no callback).
//! - Test B: the VC is `Freshness::Final` (trampoline ignores the cancel handle).
//!
//! Both tests pass after step-8 GREEN (trampoline reads the thread-local context
//! and threads a closure into `solve_cg_with_warm_state_progress`).

use std::sync::{Arc, Mutex};

use reify_core::Severity;
use reify_eval::cache::NodeId;
use reify_eval::{CancellationHandle, SolverProgressSink, SolverProgressUpdate};
use reify_ir::Freshness;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── fixture ───────────────────────────────────────────────────────────────────

fn cantilever_source() -> &'static str {
    include_str!("../../../examples/fea_cantilever_smoke.ri")
}

// ── recording sink ────────────────────────────────────────────────────────────

/// `SolverProgressSink` that records every `(solver_kind, iter, residual)` tuple.
struct RecordingSolverProgressSink {
    updates: Arc<Mutex<Vec<(String, u32, f64)>>>,
}

impl SolverProgressSink for RecordingSolverProgressSink {
    fn on_iteration(&self, update: &SolverProgressUpdate) {
        self.updates
            .lock()
            .unwrap()
            .push((update.solver_kind.to_string(), update.iter, update.residual));
    }
}

// ── Test A: progress emit ─────────────────────────────────────────────────────

/// After installing a `RecordingSolverProgressSink` on the engine and calling
/// `engine.eval()` on the cantilever smoke fixture, the sink must have received
/// ≥1 update with `iter ≥ 1`, a finite residual, and `solver_kind == "cg"`.
///
/// RED on the base branch: the trampoline does not yet read the thread-local
/// context → 0 updates recorded.
#[test]
fn solver_progress_sink_receives_cg_iterations_on_cantilever() {
    let compiled = parse_and_compile_with_stdlib(cantilever_source());

    let updates: Arc<Mutex<Vec<(String, u32, f64)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(RecordingSolverProgressSink {
        updates: Arc::clone(&updates),
    });

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    engine.set_solver_progress_sink(sink);

    let eval_result = engine.eval(&compiled);

    // No Error diagnostics — the solve must complete cleanly before testing
    // the progress events.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from cantilever eval, got: {:?}",
        errors
    );

    let recorded = updates.lock().unwrap();

    // At least one progress update must have been emitted.
    assert!(
        !recorded.is_empty(),
        "expected ≥1 SolverProgressUpdate on the cantilever fixture, got 0; \
         the trampoline must read the thread-local context and pass a closure \
         to solve_cg_with_warm_state_progress"
    );

    // Every update must have iter ≥ 1 (1-indexed), a finite residual, and
    // solver_kind == "cg".
    for (kind, iter, residual) in recorded.iter() {
        assert!(
            *iter >= 1,
            "iter must be ≥ 1 (1-indexed), got {}",
            iter
        );
        assert!(
            residual.is_finite(),
            "residual must be finite, got {}",
            residual
        );
        assert_eq!(
            kind.as_str(),
            "cg",
            "solver_kind must be \"cg\", got {:?}",
            kind
        );
    }

    // ── Cadence hardening (task 4366 step-5) ─────────────────────────────────

    // Reference the real PROGRESS_STRIDE constant from reify-eval (re-exported
    // via #[doc(hidden)] pub use in lib.rs for test use) so this value has a
    // single source of truth and cannot silently drift.
    const STRIDE: u32 = reify_eval::PROGRESS_STRIDE as u32;

    // (1) Cadence: every emitted iter must be 1 or a multiple of STRIDE.
    for (_kind, iter, _residual) in recorded.iter() {
        assert!(
            *iter == 1 || *iter % STRIDE == 0,
            "emitted iter {iter} is not in {{1}} ∪ multiples of STRIDE ({STRIDE}); \
             progress throttle may be broken"
        );
    }

    // (2) Stride exercised: the ~2520-DOF cantilever solve runs hundreds of CG
    // iterations, so iter==10 (first stride emit) must occur — len ≥ 2 ⟺ a
    // stride emit happened.  Failing here means the fixture shrank below STRIDE
    // iterations, making the cadence check vacuous.
    assert!(
        recorded.len() >= 2,
        "expected ≥2 progress updates (iter=1 + at least one stride emit at iter={}), \
         got {}; either the fixture runs <{} CG iters or the stride throttle is broken",
        STRIDE,
        recorded.len(),
        STRIDE
    );

    // (3) Net residual decrease (guarded on len ≥ 2 above).
    // CG's unpreconditioned ‖r‖₂ is provably monotonic only in the A-norm of
    // the error, not in ‖r‖₂, so strict pairwise non-increasing is not
    // asserted.  Net decrease (first > last) still catches an emit wired to a
    // stale/constant/garbage residual.
    let first_residual = recorded.first().unwrap().2;
    let last_residual = recorded.last().unwrap().2;
    assert!(
        first_residual >= 0.0,
        "first residual must be ≥ 0, got {first_residual}"
    );
    assert!(
        last_residual >= 0.0,
        "last residual must be ≥ 0, got {last_residual}"
    );
    assert!(
        first_residual > last_residual,
        "first emitted residual ({first_residual:.3e}) must be strictly greater than \
         last ({last_residual:.3e}); an emit wired to a stale/constant/garbage residual \
         would fail this check"
    );
}

// ── Test B: cancel interruption ───────────────────────────────────────────────

/// Installing a pre-cancelled `CancellationHandle` via
/// `engine.set_active_solve_cancel(Some(h))` must cause the
/// `solver::elastic_static` trampoline to return `ComputeOutcome::Cancelled`,
/// leaving every output VC of the ComputeNode `Freshness::Pending` (NOT Final).
///
/// RED on the base branch: the trampoline ignores the thread-local cancel
/// context → the VC ends up `Freshness::Final`.
#[test]
fn pre_cancelled_handle_leaves_elastic_static_vc_pending() {
    let compiled = parse_and_compile_with_stdlib(cantilever_source());

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    // Pre-cancel the handle before installing it so the trampoline sees
    // `is_cancelled() == true` on the very first iteration.
    let handle = CancellationHandle::new();
    handle.cancel();
    engine.set_active_solve_cancel(Some(handle));

    engine.eval(&compiled);

    // Locate the solver::elastic_static ComputeNode.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();

    let elastic_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, d)| d.target == "solver::elastic_static")
        .map(|(_, d)| d)
        .expect(
            "solver::elastic_static ComputeNode must be present in the eval graph; \
             make sure register_compute_fns was called",
        );

    // The node must have at least one output VC.
    assert!(
        !elastic_node.output_value_cells.is_empty(),
        "solver::elastic_static must have at least one output VC"
    );

    // Each output VC must be Freshness::Pending — NOT Final.
    for vc_id in &elastic_node.output_value_cells {
        let node_ref = NodeId::Value(vc_id.clone());
        assert!(
            matches!(engine.freshness(&node_ref), Freshness::Pending { .. }),
            "output VC {:?} must be Freshness::Pending after a pre-cancelled solve, \
             got {:?}; the trampoline must poll the cancel handle and return \
             ComputeOutcome::Cancelled",
            vc_id,
            engine.freshness(&node_ref),
        );
    }
}
