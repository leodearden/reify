// Compute-node dispatch registry and associated types.
//
// See `docs/prds/v0_3/compute-node-contract.md` §4 and §8-γ for the full spec.
// Types defined here: `ComputeFn`, `ComputeOutcome`, `RealizationReadHandle`,
// `ComputeDispatchRegistry`.

use std::collections::HashMap;

use reify_types::{Diagnostic, OpaqueState, RealizationNodeId, Value};

use crate::graph::CancellationHandle;

/// Function-pointer type for a synchronous compute trampoline.
///
/// Signature (PRD §4):
/// - `value_inputs`: resolved scalar/tensor inputs for this invocation
/// - `realization_inputs`: resolved geometry inputs (read-only handles)
/// - `options`: per-invocation option map (`Value::Map` or `Value::Undef`)
/// - `prior_warm_state`: warm-start state from the previous invocation, if any
/// - `cancellation`: cooperative-cancellation handle; implementations should
///   poll `is_cancelled()` at coarse-grained intervals
///
/// Returns a [`ComputeOutcome`] describing the result, any new warm state,
/// cost metadata, and diagnostics.
///
/// This is a plain function-pointer (`fn`) type, not a boxed trait object,
/// to keep dispatch registration zero-allocation and enable `Copy` semantics
/// (a registry lookup returns `Option<ComputeFn>` directly without a heap read).
pub type ComputeFn = fn(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome;

/// Outcome of a synchronous [`ComputeFn`] invocation.
///
/// See `docs/prds/v0_3/compute-node-contract.md` §4 and §5.
#[derive(Debug)]
pub enum ComputeOutcome {
    /// The computation completed successfully.
    Completed {
        /// The primary result value written to the output value cell.
        result: Value,
        /// Optional warm-start state to donate for the next invocation.
        /// `None` in γ (warm-state lifecycle is deferred to slice ζ/3425).
        new_warm_state: Option<OpaqueState>,
        /// Optional cost estimate in abstract units per byte of output.
        /// Intended for cache-eviction heuristics; `None` means "unknown".
        cost_per_byte: Option<f64>,
        /// Non-fatal diagnostics generated during computation.
        diagnostics: Vec<Diagnostic>,
    },
    /// The computation was cancelled via the [`CancellationHandle`].
    /// Cancellation lifecycle (`running` field management) is deferred to
    /// slice ε (3424); for γ the cancellation handle is created fresh and
    /// never polled externally.
    Cancelled,
    /// The computation failed; no result value is available.
    Failed {
        /// Diagnostics describing the failure. Should include at least one
        /// `Severity::Error` diagnostic.
        diagnostics: Vec<Diagnostic>,
    },
}

/// Minimal read-only wrapper over a realization node identity.
///
/// Passed to [`ComputeFn`] invocations that declare realization inputs.
/// Content accessors (geometry data, mesh bytes, etc.) are deferred to
/// downstream slices (δ/ε/ζ); for γ, only the node identity is accessible.
/// The wrapper exists so that the `ComputeFn` signature is contract-stable
/// for downstream slices that will read realization content.
///
/// See `docs/prds/v0_3/compute-node-contract.md` §9 Q8.
#[derive(Debug, Clone)]
pub struct RealizationReadHandle {
    /// Identity of the realization node this handle references.
    pub node_id: RealizationNodeId,
}

/// Per-Engine registry mapping `@optimized` target strings to [`ComputeFn`]
/// function pointers.
///
/// Populated via [`Engine::register_compute_fn`]. Consulted by the value-cell
/// eval loop in `engine_eval.rs` when a `UserFunctionCall` whose
/// `CompiledFunction.optimized_target == Some(t)` is encountered — if `t` has
/// a registered entry the engine inserts a `ComputeNode` and invokes the
/// trampoline synchronously instead of body-inlining.
///
/// Keyed by `&'static str` because all registration calls in practice use
/// string literals; this keeps lookup zero-allocation (`get` with `&str`
/// works via the `Borrow<str>` impl on `&'static str`).
///
/// See `docs/prds/v0_3/compute-node-contract.md` §4.
#[derive(Default)]
pub struct ComputeDispatchRegistry {
    pub(crate) fns: HashMap<&'static str, ComputeFn>,
}

impl ComputeDispatchRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self {
            fns: HashMap::new(),
        }
    }
}

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
            node_id: RealizationNodeId::new("test", 0),
        };
    }
}
