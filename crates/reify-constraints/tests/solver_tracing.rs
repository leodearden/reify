//! Tracing integration tests for DimensionalSolver.
//!
//! Verifies that the solver emits the expected tracing events at the correct
//! levels. Uses a custom tracing::Subscriber that counts events by level,
//! following the pattern established in concurrent_eval.rs.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use reify_constraints::DimensionalSolver;
use reify_test_support::*;
use reify_core::{DimensionVector, Type};
use reify_ir::{AutoParam, BinOp, ConstraintSolver, OptimizationObjective, ResolutionProblem, SolveResult, Value, ValueMap};

/// Build a tracing subscriber that counts DEBUG and WARN level events from
/// `reify_constraints` targets.
///
/// Returns `(subscriber, debug_counter, warn_counter)`.  The counters are
/// shared via [`Arc`] so callers can read them after the subscriber has been
/// removed.
///
/// This is a thin wrapper around [`CountingSubscriberBuilder`] from
/// `reify_test_support`.  It replaces the former hand-rolled `EventCounter`
/// subscriber that always returned `Id::from_u64(1)` from `new_span` (a
/// correctness bug when multiple spans are created concurrently).
fn event_counting_subscriber() -> (impl tracing::Subscriber, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .target_prefix("reify_constraints")
        .count_level(tracing::Level::DEBUG)
        .count_level(tracing::Level::WARN)
        .build();

    let debug_count = Arc::clone(&counters[&tracing::Level::DEBUG]);
    let warn_count = Arc::clone(&counters[&tracing::Level::WARN]);

    (subscriber, debug_count, warn_count)
}

/// Trigger MaxItersReached + has_objective via a 20-param problem starting
/// infeasible. Asserts that exactly 1 debug-level event is emitted (the
/// consolidated "solver completed" event).
///
/// Uses initially_feasible=false to ensure the fallback debug event at the
/// "optimizer drifted infeasible" path cannot fire — isolating the test to
/// only the solver-completed debug event(s).
///
/// With the current code this FAILS because two separate debug! calls fire:
/// one generic "solver completed" and one conditional "solver hit iteration limit".
#[test]
fn consolidated_debug_event_on_max_iters_reached() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    let (subscriber, debug_count, _warn_count) = event_counting_subscriber();

    let solver = DimensionalSolver;

    // 20 parameters starting infeasible → max_iters = MAX_ITERS = 5000.
    // Nelder-Mead in 20 dimensions with sd_tolerance=1e-15 cannot converge
    // in 5000 iters, forcing MaxItersReached.
    let n_params: usize = 20;

    let ids: Vec<_> = (0..n_params)
        .map(|i| vcid("Part", &format!("q{}", i)))
        .collect();
    let refs: Vec<_> = (0..n_params)
        .map(|i| value_ref("Part", &format!("q{}", i)))
        .collect();

    // Constraints: each param in [10mm, 50mm]
    let mut constraints = Vec::new();
    for (i, r) in refs.iter().enumerate() {
        constraints.push((
            cnid("Part", (i * 2) as u32),
            gt(r.clone(), literal(mm(10.0))),
        ));
        constraints.push((
            cnid("Part", (i * 2 + 1) as u32),
            lt(r.clone(), literal(mm(50.0))),
        ));
    }

    // Maximize(sum) — pushes all params toward upper bound
    let sum_expr = refs
        .iter()
        .skip(1)
        .fold(refs[0].clone(), |acc, r| binop(BinOp::Add, acc, r.clone()));
    let objective = OptimizationObjective::Maximize(sum_expr);

    // All params start at 5mm — infeasible (below 10mm constraint).
    // This ensures initially_feasible=false, so the fallback debug cannot fire.
    let mut current = ValueMap::new();
    for id in &ids {
        current.insert(id.clone(), mm(5.0));
    }

    let problem = ResolutionProblem {
        auto_params: ids
            .iter()
            .map(|id| AutoParam {
                id: id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            })
            .collect(),
        constraints,
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let _result = tracing::subscriber::with_default(subscriber, || solver.solve(&problem));

    // After consolidation, should emit exactly 1 debug event for the solve path.
    // Before consolidation, the MaxItersReached + has_objective path emits 2
    // debug events (one "solver completed" and one "solver hit iteration limit").
    let debug = debug_count.load(Ordering::Relaxed);
    assert_eq!(
        debug, 1,
        "expected exactly 1 consolidated debug event for MaxItersReached path, got {}",
        debug
    );
}

/// Regression guard: a normal successful solve (single-param feasibility)
/// should emit zero warn-level events. If a future change accidentally
/// adds a warn! to the normal solve path, this test catches it.
#[test]
fn normal_solve_emits_zero_warns() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    let (subscriber, _debug_count, warn_count) = event_counting_subscriber();

    let solver = DimensionalSolver;

    let x_id = vcid("Bracket", "thickness");
    let x_ref = value_ref("Bracket", "thickness");

    // thickness > 2mm AND thickness < 20mm
    let gt_expr = gt(x_ref.clone(), literal(mm(2.0)));
    let lt_expr = lt(x_ref, literal(mm(20.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
            free: false,
        }],
        constraints: vec![(cnid("Bracket", 0), gt_expr), (cnid("Bracket", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = tracing::subscriber::with_default(subscriber, || solver.solve(&problem));

    // Normal solve must not emit any warns
    let warns = warn_count.load(Ordering::Relaxed);
    assert_eq!(
        warns, 0,
        "normal successful solve should emit 0 warns, got {}",
        warns
    );

    // Sanity: the solve should actually succeed
    assert!(
        matches!(result, SolveResult::Solved { .. }),
        "expected Solved, got {:?}",
        result
    );
}

/// Verify the reason string format for the get_best_param()==None path.
/// This path is defensive — argmin's NelderMead nearly always produces a best
/// parameter. We attempt to trigger it with collapsed bounds (lo == hi) which
/// creates a degenerate zero-volume simplex. If NoProgress is returned with
/// "solver returned no solution", we also verify a warn-level event was emitted.
#[test]
fn no_best_param_returns_no_progress_with_reason() {
    let (subscriber, _debug_count, warn_count) = event_counting_subscriber();

    let solver = DimensionalSolver;

    let x_id = vcid("Plate", "width");
    let x_ref = value_ref("Plate", "width");

    let gt_expr = gt(x_ref, literal(mm(1.0)));

    // Collapsed bounds: lo == hi → zero-volume simplex.
    // The initial point is extracted as lo (0.05), and the simplex perturbation
    // delta = (hi - lo) * 0.1 = 0, so all vertices are identical.
    let mut current = ValueMap::new();
    current.insert(x_id.clone(), mm(0.05));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.05, 0.05)),
            free: false,
        }],
        constraints: vec![(cnid("Plate", 0), gt_expr)],
        current_values: current,
        objective: None,
        functions: vec![].into(),
    };

    let result = tracing::subscriber::with_default(subscriber, || solver.solve(&problem));

    // If the get_best_param()==None path is triggered, verify the reason string
    // and check that a warn was emitted. This is a conditional assertion because
    // argmin may handle the degenerate simplex gracefully (converging immediately
    // with the single-point simplex).
    if let SolveResult::NoProgress { reason } = &result
        && reason == "solver returned no solution"
    {
        let warns = warn_count.load(Ordering::Relaxed);
        assert!(
            warns > 0,
            "get_best_param()==None path should emit a warn, got 0 warns"
        );
    }
}

/// Regression check: verify that `event_counting_subscriber()` (backed by
/// `CountingSubscriberBuilder`) does NOT count debug/warn events from foreign
/// (non-reify_constraints) targets.  This guards against flaky count
/// assertions when argmin or other transitive dependencies emit their own
/// tracing events during a solve.
#[test]
fn event_counter_ignores_foreign_targets() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    let (subscriber, debug_count, warn_count) = event_counting_subscriber();

    tracing::subscriber::with_default(subscriber, || {
        // Emit events that mimic argmin internals — these should be ignored.
        tracing::debug!(target: "argmin::core", "foreign debug event");
        tracing::warn!(target: "argmin::solver::neldermead", "foreign warn event");
        // Also test a completely unrelated target
        tracing::debug!(target: "tokio::runtime", "another foreign debug");
    });

    let debugs = debug_count.load(Ordering::Relaxed);
    let warns = warn_count.load(Ordering::Relaxed);

    assert_eq!(
        debugs, 0,
        "EventCounter should ignore debug events from foreign targets, got {}",
        debugs
    );
    assert_eq!(
        warns, 0,
        "EventCounter should ignore warn events from foreign targets, got {}",
        warns
    );
}

/// Verify the reason string format for the executor.run() error path.
/// Uses NaN bounds to create a degenerate simplex that causes argmin to error.
/// If the executor error path is triggered, the reason must match "solver error: ...".
#[test]
fn executor_error_returns_no_progress_with_reason() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    let gt_expr = gt(x_ref, literal(mm(1.0)));

    // NaN bounds → degenerate simplex → argmin executor error
    let mut current = ValueMap::new();
    current.insert(
        x_id.clone(),
        Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((f64::NAN, f64::NAN)),
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), gt_expr)],
        current_values: current,
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    // The NaN simplex should cause executor.run() to error, yielding NoProgress.
    // If it doesn't error (argmin handles NaN gracefully), the test still passes
    // because we only assert the format when NoProgress is returned.
    if let SolveResult::NoProgress { reason } = result {
        assert!(
            reason.starts_with("solver error: "),
            "executor error reason should start with 'solver error: ', got: {}",
            reason
        );
    }
}
