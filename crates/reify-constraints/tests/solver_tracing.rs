//! Tracing integration tests for DimensionalSolver.
//!
//! Verifies that the solver emits the expected tracing events at the correct
//! levels. Uses a custom tracing::Subscriber that counts events by level,
//! following the pattern established in concurrent_eval.rs.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use reify_constraints::DimensionalSolver;
use reify_test_support::*;
use reify_types::{
    AutoParam, BinOp, ConstraintSolver, OptimizationObjective, ResolutionProblem, SolveResult,
    Type, ValueMap,
};

/// Build a tracing subscriber that counts DEBUG and WARN level events.
/// Returns the subscriber and clones of both counters for assertions.
fn event_counting_subscriber() -> (
    impl tracing::Subscriber,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let debug_count = Arc::new(AtomicUsize::new(0));
    let warn_count = Arc::new(AtomicUsize::new(0));
    let dc = Arc::clone(&debug_count);
    let wc = Arc::clone(&warn_count);

    struct EventCounter {
        debug_count: Arc<AtomicUsize>,
        warn_count: Arc<AtomicUsize>,
    }

    impl tracing::Subscriber for EventCounter {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.level() <= &tracing::Level::DEBUG
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(
            &self,
            _span: &tracing::span::Id,
            _follows: &tracing::span::Id,
        ) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let level = event.metadata().level();
            if level == &tracing::Level::DEBUG {
                self.debug_count.fetch_add(1, Ordering::Relaxed);
            } else if level == &tracing::Level::WARN {
                self.warn_count.fetch_add(1, Ordering::Relaxed);
            }
        }

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    (
        EventCounter {
            debug_count: dc,
            warn_count: wc,
        },
        debug_count,
        warn_count,
    )
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
    for i in 0..n_params {
        constraints.push((
            cnid("Part", (i * 2) as u32),
            gt(refs[i].clone(), literal(mm(10.0))),
        ));
        constraints.push((
            cnid("Part", (i * 2 + 1) as u32),
            lt(refs[i].clone(), literal(mm(50.0))),
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
            })
            .collect(),
        constraints,
        current_values: current,
        objective: Some(objective),
        functions: vec![],
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
