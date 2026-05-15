// Compute-node dispatch registry and associated types.
//
// See `docs/prds/v0_3/compute-node-contract.md` §4 and §8-γ for the full spec.
// Types defined here: `ComputeFn`, `ComputeOutcome`, `RealizationReadHandle`,
// `ComputeDispatchRegistry`.

#[cfg(test)]
mod tests {
    use reify_test_support::mocks::MockConstraintChecker;
    use reify_types::{OpaqueState, RealizationNodeId, Value};

    use crate::Engine;
    use crate::engine_compute::{
        ComputeFn, ComputeOutcome, RealizationReadHandle,
    };
    use crate::graph::CancellationHandle;

    /// A minimal identity trampoline: returns `value_inputs[0]` as the result.
    fn identity_fn(
        value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        ComputeOutcome::Completed {
            result: value_inputs[0].clone(),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    // ── Test: register then dispatch returns Some ───────────────────────────

    #[test]
    fn register_and_dispatch_registered_target_returns_some() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::identity", identity_fn as ComputeFn);
        assert!(
            engine.compute_dispatch("test::identity").is_some(),
            "expected Some after register_compute_fn"
        );
    }

    // ── Test: unregistered target returns None ──────────────────────────────

    #[test]
    fn dispatch_unregistered_target_returns_none() {
        let engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        assert!(
            engine.compute_dispatch("nonexistent::target").is_none(),
            "expected None for unregistered target"
        );
    }

    // ── Test: duplicate registration panics naming the target ───────────────

    #[test]
    #[should_panic(expected = "test::identity")]
    fn register_duplicate_target_panics_naming_target() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::identity", identity_fn as ComputeFn);
        // Second registration with the same target must panic.
        engine.register_compute_fn("test::identity", identity_fn as ComputeFn);
    }

    // ── Test: ComputeOutcome variants and RealizationReadHandle are nameable ─
    // (compile-time surface pin — ensures the type shapes are as expected)

    #[allow(dead_code)]
    fn _coerce_types() {
        // ComputeFn is a plain fn-pointer type
        let _f: ComputeFn = identity_fn;

        // ComputeOutcome::Completed
        let _c = ComputeOutcome::Completed {
            result: Value::Int(0),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        };

        // ComputeOutcome::Cancelled
        let _d = ComputeOutcome::Cancelled;

        // ComputeOutcome::Failed
        let _e = ComputeOutcome::Failed {
            diagnostics: vec![],
        };

        // RealizationReadHandle construction
        let _h = RealizationReadHandle {
            node_id: RealizationNodeId::new("test", "r"),
        };
    }
}
