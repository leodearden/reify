// Compute-node dispatch registry and associated types.
//
// See `docs/prds/v0_3/compute-node-contract.md` §4 and §8-γ for the full spec.
// Types defined here: `ComputeFn`, `ComputeOutcome`, `RealizationReadHandle`,
// `ComputeDispatchRegistry`, `DispatchError`.

use std::collections::HashMap;

use reify_types::{
    ComputeNodeId, Diagnostic, OpaqueState, RealizationNodeId, Value, ValueCellId, VersionId,
};

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

/// Error returned by [`Engine::run_compute_dispatch`] when a dispatch does not
/// complete successfully.
///
/// Distinguishes the two terminal non-success outcomes so the lowering site
/// (and tests) can apply the correct cache transition:
///
/// - [`DispatchError::Cancelled`] — the trampoline observed cancellation via
///   its [`CancellationHandle`] and returned [`ComputeOutcome::Cancelled`].
///   The output VCs are **left [`Freshness::Pending`][reify_types::Freshness::Pending]**
///   (prior best on display, cache untouched) per PRD §2 / §7.1.  Callers
///   must NOT call `mark_failed` on this path.
///
/// - [`DispatchError::Failed`] — the trampoline returned
///   [`ComputeOutcome::Failed`], or the target string had no registered
///   trampoline.  The output VCs are also left `Pending` (from
///   `begin_compute_dispatch`); the caller owns the `mark_failed` transition.
///
/// See `docs/prds/v0_3/compute-node-contract.md` §2 / §7.1 / §8-ε.
#[derive(Debug)]
pub enum DispatchError {
    /// The trampoline observed [`CancellationHandle::is_cancelled`] and
    /// returned [`ComputeOutcome::Cancelled`].  Output VCs stay Pending; prior
    /// cache and warm-state are untouched.  The lowering site must NOT call
    /// `mark_failed`; it should journal a non-Changed event and `continue`.
    Cancelled,
    /// The trampoline returned [`ComputeOutcome::Failed`] or the target string
    /// had no registered trampoline.  The contained `Vec<Diagnostic>` carries
    /// the trampoline's error diagnostics (or the "no registered trampoline"
    /// synthesised diagnostic).  The lowering site owns `mark_failed`.
    Failed(Vec<Diagnostic>),
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

impl crate::Engine {
    /// Invoke the registered compute trampoline for `target` with the supplied
    /// inputs and the **caller-provided** cancellation handle.
    ///
    /// Returns `Some(outcome)` when a trampoline is registered, `None` when
    /// the target string has no registered entry.  The raw [`ComputeOutcome`]
    /// is returned so that [`run_compute_dispatch`](Self::run_compute_dispatch)
    /// can discriminate `Cancelled` from `Failed` without collapsing them into
    /// a common `Err` path (the root cause of the ε defect fixed here).
    ///
    /// Passing the handle in (rather than creating a fresh one here) lets the
    /// lowering site thread the same `Arc<AtomicBool>` that it stores in the
    /// node's `running` slot, so a future async driver cancelling via `running`
    /// propagates to the trampoline's poll.
    fn invoke_compute_trampoline(
        &self,
        target: &str,
        value_inputs: &[Value],
        realization_inputs: &[RealizationReadHandle],
        options: &Value,
        prior_warm_state: Option<&OpaqueState>,
        cancellation: &CancellationHandle,
    ) -> Option<ComputeOutcome> {
        let f = self.compute_registry.fns.get(target).copied()?;
        Some(f(
            value_inputs,
            realization_inputs,
            options,
            prior_warm_state,
            cancellation,
        ))
    }

    /// Run the full in-flight ComputeNode dispatch lifecycle for `c_id` —
    /// begin → invoke trampoline → atomic-complete-or-leave-Pending.
    ///
    /// PRD §3 "Atomic completion" / §8 tasks δ and ε
    /// (`docs/prds/v0_3/compute-node-contract.md`):
    ///
    /// 1. [`CacheStore::begin_compute_dispatch`][crate::cache::CacheStore::begin_compute_dispatch]
    ///    pre-marks every output VC `Freshness::Pending` with
    ///    `pending_cause = NodeId::Compute(c_id)` (the prior best stays on
    ///    display while recomputation is in flight).
    /// 2. [`Engine::invoke_compute_trampoline`](Self::invoke_compute_trampoline)
    ///    calls the registered trampoline synchronously with the PASSED
    ///    `cancellation` handle and returns the raw [`ComputeOutcome`].
    /// 3. On `Some(Completed{result, diagnostics})` —
    ///    [`CacheStore::complete_compute_dispatch_atomically`][crate::cache::CacheStore::complete_compute_dispatch_atomically]
    ///    writes the new value, flips Pending → Final, and clears
    ///    `pending_cause` in a single critical section. Returns
    ///    `Ok((result, diagnostics))`.
    /// 4. On `Some(Cancelled)` — the output VCs are **left Pending** (begin
    ///    set them; complete is NOT called). Per PRD §2 / §7.1 a cancelled
    ///    dispatch must leave the prior best on display and the prior cache
    ///    untouched. Returns `Err(DispatchError::Cancelled)`. The lowering
    ///    site must NOT call `mark_failed` on this path.
    /// 5. On `Some(Failed{diagnostics})` or `None` (unregistered target) —
    ///    output VCs are also left Pending; returns
    ///    `Err(DispatchError::Failed(diagnostics))`. The caller owns the
    ///    `mark_failed` transition (it has the `ErrorRef` context).
    ///
    /// The `cancellation` handle is the **same `Arc<AtomicBool>`** the
    /// lowering site stores in the node's `running` slot, so a future async
    /// driver cancelling via `running` propagates directly to the trampoline's
    /// poll (PRD §5 / design decision in task ε/3424).
    ///
    /// `c_id` is forwarded to `complete_compute_dispatch_atomically`, where it
    /// is reserved for ζ-scope warm-state donation (task 3425).
    ///
    /// ## Freshness on completion is unconditionally Final
    ///
    /// On `Ok`, `complete_compute_dispatch_atomically` stamps the output VCs
    /// `Freshness::Final` regardless of the input cells' freshness. Restoring
    /// derived-Intermediate propagation when inputs are partial is deferred to
    /// a future slice (the upstream Pending gate already short-circuits
    /// Failed/Pending inputs before reaching here).
    ///
    /// ## Multi-output dispatch is NOT yet defined
    ///
    /// The `outputs` parameter is a slice for forward-compatibility, but the
    /// trampoline returns a single `Value`. Today the only caller (the
    /// `@optimized` lowering site in `engine_eval.rs`) passes
    /// `slice::from_ref(cell_id)`, i.e. a single output. A `debug_assert_eq!`
    /// pins this contract: if a future caller passes more than one output, the
    /// helper would silently broadcast the single trampoline result to every
    /// cell rather than fan out per-component. Multi-output semantics require
    /// a trampoline signature extension; until then, this assertion catches
    /// accidental misuse.
    #[allow(clippy::too_many_arguments)]
    pub fn run_compute_dispatch(
        &mut self,
        c_id: &ComputeNodeId,
        outputs: &[ValueCellId],
        target: &str,
        value_inputs: &[Value],
        realization_inputs: &[RealizationReadHandle],
        options: &Value,
        prior_warm_state: Option<&OpaqueState>,
        cancellation: &CancellationHandle,
        version: VersionId,
    ) -> Result<(Value, Vec<Diagnostic>), DispatchError> {
        // Multi-output dispatch is not yet defined — see docstring.
        debug_assert_eq!(
            outputs.len(),
            1,
            "run_compute_dispatch only supports single-output dispatch today; \
             trampoline returns a single Value and would be broadcast to every \
             cell. Multi-output semantics require a trampoline signature change.",
        );

        // Step 1: pre-mark output VCs Pending (the in-flight state).
        // begin_compute_dispatch already leaves VCs Pending{last_substantive: prior}
        // with pending_cause = Compute(c_id). The cancelled path simply does NOT
        // call complete — that already-correct Pending state IS the contract
        // (PRD §2 / design decision recorded in task ε/3424).
        self.cache.begin_compute_dispatch(c_id, outputs);

        // Step 2: invoke the trampoline via the new helper (ε: threads the
        // PASSED handle rather than creating a fresh throwaway one).
        match self.invoke_compute_trampoline(
            target,
            value_inputs,
            realization_inputs,
            options,
            prior_warm_state,
            cancellation,
        ) {
            Some(ComputeOutcome::Completed {
                result,
                diagnostics,
                ..
            }) => {
                // Step 3a: atomic completion (write + flip Pending→Final + clear cause).
                let pairs: Vec<(ValueCellId, Value)> = outputs
                    .iter()
                    .map(|o| (o.clone(), result.clone()))
                    .collect();
                // Warm-state threading lands in step-8 (task 3425/ζ); the
                // call deliberately passes `None, 0.0` here so step-7's RED
                // test can pin the missing wiring before step-8 fixes it.
                self.cache.complete_compute_dispatch_atomically(
                    c_id,
                    &pairs,
                    version,
                    None,
                    0.0,
                );
                Ok((result, diagnostics))
            }
            // Step 3b: Cancelled — leave VCs in the already-correct Pending
            // state from begin. No warm-state donation, no mark_failed.
            // PRD §2 / §7.1: "cancelled dispatch leaves prior best on display,
            // prior cache untouched, Pending until next dispatch completes."
            Some(ComputeOutcome::Cancelled) => Err(DispatchError::Cancelled),
            // Step 3c: Failed — also leave VCs Pending; caller owns mark_failed.
            Some(ComputeOutcome::Failed { diagnostics }) => {
                Err(DispatchError::Failed(diagnostics))
            }
            // Step 3d: Unregistered target — synthesise a Failed diagnostic.
            //
            // NOTE: the production caller (`engine_eval.rs`) pre-gates on
            // registration — it body-inlines the unregistered-target path and
            // emits its own diagnostic (PRD §9 Q1) before ever reaching this
            // function.  This arm is therefore unreachable from production code
            // and exists as a defensive fallback for direct test calls and any
            // future caller that does not pre-gate.  The synthesised diagnostic
            // text intentionally matches the `dispatch_compute_node` wording so
            // the two helper surfaces stay consistent.
            None => Err(DispatchError::Failed(vec![Diagnostic::error(format!(
                "@optimized target {:?}: no registered compute trampoline",
                target
            ))])),
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

    // ── Step-ε-1: RED — Cancelled/Failed/Completed discrimination ──────────────
    // These tests call the NEW run_compute_dispatch signature:
    //   (c_id, outputs, target, value_inputs, realization_inputs, options,
    //    prior_warm_state, cancellation: &CancellationHandle, version)
    //   -> Result<(Value, Vec<Diagnostic>), DispatchError>
    //
    // They fail to COMPILE until step-2 adds DispatchError + the cancellation
    // param, making them RED as required by the TDD protocol.

    use crate::cache::{CachedResult, NodeCache, NodeId};
    use crate::deps::DependencyTrace;
    use reify_types::{ComputeNodeId, DeterminacyState, Freshness, ValueCellId, VersionId};

    /// Trampoline (a): polls is_cancelled() and returns Cancelled if set,
    /// otherwise Completed{Int(0)}.
    fn cancellable_fn(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        if cancellation.is_cancelled() {
            ComputeOutcome::Cancelled
        } else {
            ComputeOutcome::Completed {
                result: Value::Int(0),
                new_warm_state: None,
                cost_per_byte: None,
                diagnostics: vec![],
            }
        }
    }

    /// Trampoline (b): always returns Failed with one error diagnostic.
    fn always_failed_fn(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        ComputeOutcome::Failed {
            diagnostics: vec![reify_types::Diagnostic::error(
                "test trampoline always fails",
            )],
        }
    }

    /// (a) A pre-cancelled handle → Err(DispatchError::Cancelled) and the
    /// output VC is left Pending (begin set it; complete was NOT called).
    #[test]
    fn run_compute_dispatch_pre_cancelled_returns_dispatch_error_cancelled() {
        use crate::engine_compute::DispatchError;

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::cancellable_eps1a", cancellable_fn as ComputeFn);

        let cell = ValueCellId::new("T", "b");
        let c_id = ComputeNodeId::new("T", 0);

        // Seed a Final entry so begin_compute_dispatch has a last_substantive.
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(99), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Pre-cancel the handle before the call.
        let handle = CancellationHandle::new();
        handle.cancel();

        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::cancellable_eps1a",
            &[Value::Int(99)],
            &[],
            &Value::Undef,
            None,
            &handle, // NEW param — fails to compile until step-2
            VersionId(2),
        );

        // Must return Err(DispatchError::Cancelled) — not Err(DispatchError::Failed).
        assert!(
            matches!(result, Err(DispatchError::Cancelled)),
            "pre-cancelled dispatch must return Err(DispatchError::Cancelled), got {result:?}",
        );

        // VC is left Pending (begin was called; complete was not).
        let node = NodeId::Value(cell.clone());
        assert!(
            matches!(engine.freshness(&node), Freshness::Pending { .. }),
            "post-cancel VC freshness must be Pending, not Failed or Final",
        );

        // pending_cause == Some(NodeId::Compute(c_id)).
        assert_eq!(
            engine.pending_cause(&node),
            Some(NodeId::Compute(c_id)),
            "pending_cause must point at the in-flight ComputeNode",
        );
    }

    /// (b) A Failed trampoline → Err(DispatchError::Failed(diags)) with the
    /// diagnostics preserved; output VC stays Pending.
    #[test]
    fn run_compute_dispatch_failed_trampoline_returns_dispatch_error_failed_with_diags() {
        use crate::engine_compute::DispatchError;

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::always_failed_eps1b", always_failed_fn as ComputeFn);

        let cell = ValueCellId::new("T", "b");
        let c_id = ComputeNodeId::new("T", 0);

        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(7), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        let handle = CancellationHandle::new(); // not cancelled

        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::always_failed_eps1b",
            &[Value::Int(7)],
            &[],
            &Value::Undef,
            None,
            &handle, // NEW param — fails to compile until step-2
            VersionId(2),
        );

        // Must return Err(DispatchError::Failed) with the trampoline's diagnostics.
        match result {
            Err(DispatchError::Failed(diags)) => {
                assert!(!diags.is_empty(), "Failed must carry diagnostics from the trampoline");
            }
            other => panic!("expected Err(DispatchError::Failed(…)), got {other:?}"),
        }

        // VC is left Pending (begin was called; complete was not).
        let node = NodeId::Value(cell.clone());
        assert!(
            matches!(engine.freshness(&node), Freshness::Pending { .. }),
            "post-failed-trampoline VC freshness must be Pending, not Final or Failed",
        );
    }

    /// (c) Completed trampoline → Ok((result, diags)) and VC flipped to Final
    /// (regression: the happy path survives the signature change).
    #[test]
    fn run_compute_dispatch_completed_trampoline_returns_ok_and_flips_vc_to_final() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::identity_eps1c", identity_fn as ComputeFn);

        let cell = ValueCellId::new("T", "b");
        let c_id = ComputeNodeId::new("T", 0);

        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(5), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        let handle = CancellationHandle::new(); // not cancelled

        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::identity_eps1c",
            &[Value::Int(5)],
            &[],
            &Value::Undef,
            None,
            &handle, // NEW param — fails to compile until step-2
            VersionId(2),
        );

        // Happy path: Ok with the trampoline's identity result.
        let (value, diags) = result.expect("completed dispatch must return Ok");
        assert_eq!(value, Value::Int(5), "identity must return the input unchanged");
        assert!(diags.is_empty(), "identity emits no diagnostics");

        // VC flipped to Final by complete_compute_dispatch_atomically.
        let node = NodeId::Value(cell.clone());
        assert_eq!(
            engine.freshness(&node),
            Freshness::Final,
            "post-completed VC freshness must be Final",
        );
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
