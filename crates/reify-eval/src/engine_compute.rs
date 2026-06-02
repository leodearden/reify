// Compute-node dispatch registry and associated types.
//
// See `docs/prds/v0_3/compute-node-contract.md` §4 and §8-γ for the full spec.
// Types defined here: `ComputeFn`, `ComputeOutcome`, `RealizationReadHandle`,
// `ComputeDispatchRegistry`, `DispatchError`.

use std::collections::HashMap;

use reify_core::{ComputeNodeId, Diagnostic, RealizationNodeId, ValueCellId, VersionId};
use reify_ir::{OpaqueState, Value};

use crate::cache::NodeId;
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

        // ζ / task 3425 step-6: source the prior warm state strictly from
        // the canonical at-rest store (cache). When the cache misses, fall
        // back to the warm_pool — this covers the remove-reinsert case
        // where engine_edit step (9) parks the prior state in the pool
        // between source edits and run_compute_dispatch (called from
        // engine_eval's @optimized lowering site) is the natural reinsert
        // seeding point.
        //
        // ζ step-10: `prior_cost` and `prior_warm_state` are consumed by
        // the non-Completed arms below (Cancelled / Failed / unregistered)
        // to restore the prior to the cache via
        // `donate_warm_state_with_cost`. PRD §5 "Idempotent under any
        // number of cancel-and-redispatch cycles" requires this: the prior
        // must survive any non-completing dispatch attempt.
        let compute_node = NodeId::Compute(c_id.clone());
        let prior_cost = self
            .cache
            .cost_per_byte_of(&compute_node)
            .or_else(|| self.warm_pool.cost_per_byte_of(&compute_node))
            .unwrap_or(0.0);
        // Cache-miss → pool-hit fallback. Use `checkout` (which discards
        // the LRU stamp internally) rather than `checkout_with_lru_stamp`
        // + `.map(|(state, _)| state)`: the prior state is restored back
        // to the CACHE on Cancelled/Failed/unregistered (not back to the
        // pool), so the pool's original `last_accessed` stamp has no
        // downstream consumer here. The next engine_edit cycle that re-
        // parks this entry into the pool will assign a fresh
        // `Instant::now()` — the same limitation already documented by
        // `donate_preserving_lru_resets_cost_to_zero_known_limitation` for
        // the (4c)→(14b) round-trip. `checkout` makes the discard explicit
        // rather than hiding it behind an unused `_stamp` binding.
        let mut prior_warm_state: Option<OpaqueState> = self
            .cache
            .get_warm_state(&compute_node)
            .or_else(|| self.warm_pool.checkout(&compute_node));

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
            prior_warm_state.as_ref(),
            cancellation,
        ) {
            Some(ComputeOutcome::Completed {
                result,
                new_warm_state,
                cost_per_byte,
                diagnostics,
            }) => {
                // §9-ζ (#3596) dispatch-complete fold hook: fold derived
                // mid-surface attributes into the topology_attribute_table so
                // downstream selector lookup can find them. Gated on the single
                // target that currently needs a post-dispatch fold; a per-target
                // hook map is the documented future generalization if more
                // targets need this pattern.
                //
                // NOTE: topology_attribute_table is rebuild-derived and is NOT
                // repopulated on a pure in-memory cache hit. Any future path
                // that short-circuits dispatch from a cached result (task ι
                // scope) must also call this fold so the table stays consistent
                // with the result served to callers.
                if target == "shell-extract::extract" {
                    crate::shell_extract_compute::fold_mid_surface_attributes_into_table(
                        &mut self.topology_attribute_table,
                        &result,
                    );
                }

                // Step 3a: atomic completion (write + flip Pending→Final +
                // clear cause + donate warm state). PRD §5 step-3 bundles
                // all four operations into a single critical section.
                let pairs: Vec<(ValueCellId, Value)> = outputs
                    .iter()
                    .map(|o| (o.clone(), result.clone()))
                    .collect();
                // ζ / task 3425 step-8: thread `new_warm_state` and
                // `cost_per_byte.unwrap_or(0.0)` into the extended
                // `complete_compute_dispatch_atomically` so the warm state
                // donation lands atomically with the Pending→Final flip.
                // The prior_warm_state local captured at the top of this
                // function is dropped here — the cache is now authoritative
                // (the Completed path overwrites prior with the new state).
                self.cache.complete_compute_dispatch_atomically(
                    c_id,
                    &pairs,
                    version,
                    new_warm_state,
                    cost_per_byte.unwrap_or(0.0),
                );
                Ok((result, diagnostics))
            }
            // Step 3b: Cancelled — leave VCs in the already-correct Pending
            // state from begin. No mark_failed, no new warm-state donation.
            // PRD §2 / §7.1: "cancelled dispatch leaves prior best on display,
            // Pending until next dispatch completes." ζ / step-10: restore
            // the prior warm state + cost back to the cache so the next
            // dispatch observes the same prior this one would have (PRD §5
            // "Idempotent under any number of cancel-and-redispatch cycles").
            // The donation creates only a Compute-node entry, not a VC entry,
            // so there is no interference with the begin-set Pending state.
            //
            // ζ / step-14: the seed-then-donate sequence is symmetric with
            // the Completed path's auto-seed in
            // `complete_compute_dispatch_atomically` (cache.rs Step 4). On
            // the post-edit pool-only path, the cache-miss → pool-hit fallback
            // above took the prior out of the pool (removing the pool entry)
            // while leaving the cache with NO Compute entry — without the
            // explicit seed call, `donate_warm_state_with_cost` would be a
            // silent no-op and the prior would be dropped on the floor,
            // breaking PRD §5 idempotence on the post-edit path. Keeping the
            // cache as the canonical at-rest store (rather than re-donating
            // back to the pool) means the at-rest topology does not depend
            // on whether the most recent dispatch was Completed vs Cancelled.
            Some(ComputeOutcome::Cancelled) => {
                if let Some(prior) = prior_warm_state.take() {
                    self.cache.seed_compute_entry_if_absent(c_id, version);
                    let donated =
                        self.cache
                            .donate_warm_state_with_cost(&compute_node, prior, prior_cost);
                    debug_assert!(
                        donated,
                        "seed-then-donate is atomic: auto-seed guarantees the entry exists",
                    );
                }
                Err(DispatchError::Cancelled)
            }
            // Step 3c: Failed — also leave VCs Pending; caller owns mark_failed.
            // ζ / step-10: same restore-prior arm as Cancelled.
            // ζ / step-14: same seed-then-donate sequence as Cancelled
            // (post-edit pool-only path symmetry).
            Some(ComputeOutcome::Failed { diagnostics }) => {
                if let Some(prior) = prior_warm_state.take() {
                    self.cache.seed_compute_entry_if_absent(c_id, version);
                    let donated =
                        self.cache
                            .donate_warm_state_with_cost(&compute_node, prior, prior_cost);
                    debug_assert!(
                        donated,
                        "seed-then-donate is atomic: auto-seed guarantees the entry exists",
                    );
                }
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
            //
            // ζ / step-10: restore the prior just like Cancelled / Failed —
            // an unregistered target is morally equivalent to a Failed
            // dispatch from the caller's perspective.
            // ζ / step-14: same seed-then-donate sequence (post-edit
            // pool-only path symmetry).
            None => {
                if let Some(prior) = prior_warm_state.take() {
                    self.cache.seed_compute_entry_if_absent(c_id, version);
                    let donated =
                        self.cache
                            .donate_warm_state_with_cost(&compute_node, prior, prior_cost);
                    debug_assert!(
                        donated,
                        "seed-then-donate is atomic: auto-seed guarantees the entry exists",
                    );
                }
                Err(DispatchError::Failed(vec![Diagnostic::error(format!(
                    "@optimized target {:?}: no registered compute trampoline",
                    target
                ))]))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_core::RealizationNodeId;
    use reify_ir::{OpaqueState, Value};
    use reify_test_support::mocks::MockConstraintChecker;

    use crate::Engine;
    use crate::engine_compute::{ComputeFn, ComputeOutcome, RealizationReadHandle};
    use crate::graph::CancellationHandle;

    /// A minimal identity trampoline: returns the first entry of `value_inputs`
    /// as the result, or `Value::Undef` when the slice is empty (defensive
    /// guard for the case where the engine invokes the trampoline with zero
    /// evaluated arguments — e.g. a zero-argument @optimized call).
    fn identity_fn(
        value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        ComputeOutcome::Completed {
            result: value_inputs.first().cloned().unwrap_or(Value::Undef),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    // ── Test: identity_fn with empty value_inputs ───────────────────────────

    /// Guard test: `identity_fn` called with an empty `value_inputs` slice must
    /// return `ComputeOutcome::Completed { result: Value::Undef }` instead of
    /// panicking with IndexOutOfBounds. The empty-slice path arises when a
    /// zero-argument @optimized call causes the engine to pass an empty
    /// arg_values slice to the trampoline.
    #[test]
    fn identity_fn_empty_value_inputs_returns_undef_without_panic() {
        let result = identity_fn(&[], &[], &Value::Undef, None, &CancellationHandle::new());
        match result {
            ComputeOutcome::Completed {
                result: Value::Undef,
                ..
            } => {}
            other => panic!(
                "expected ComputeOutcome::Completed {{ result: Value::Undef }}, got {:?}",
                other
            ),
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
    use reify_core::{ComputeNodeId, ValueCellId, VersionId};
    use reify_ir::{DeterminacyState, Freshness};

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
            diagnostics: vec![reify_core::Diagnostic::error(
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
            &handle, // NEW param — fails to compile until step-2
            VersionId(2),
        );

        // Must return Err(DispatchError::Failed) with the trampoline's diagnostics.
        match result {
            Err(DispatchError::Failed(diags)) => {
                assert!(
                    !diags.is_empty(),
                    "Failed must carry diagnostics from the trampoline"
                );
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
            &handle, // NEW param — fails to compile until step-2
            VersionId(2),
        );

        // Happy path: Ok with the trampoline's identity result.
        let (value, diags) = result.expect("completed dispatch must return Ok");
        assert_eq!(
            value,
            Value::Int(5),
            "identity must return the input unchanged"
        );
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

    // ── ζ / task 3425 step-5: cache as prior-warm-state source ─────────────
    //
    // The new run_compute_dispatch signature (step-6) drops the dead
    // `prior_warm_state` parameter and sources the prior strictly from
    // `cache.get_warm_state(&NodeId::Compute(c_id))` (with a warm_pool
    // fallback). This test pre-seeds the Compute entry's warm_state and
    // asserts the trampoline observes Some(42) — fails to compile until
    // step-6 lands the new signature + cache-read.

    use std::sync::{Mutex, OnceLock};

    /// Tracer trampoline: stashes its observed `prior_warm_state` (decoded
    /// as `Option<i32>`) into a file-scoped OnceLock so the test body can
    /// assert on it after dispatch returns.
    static ZETA_OBSERVED_PRIOR: OnceLock<Mutex<Option<i32>>> = OnceLock::new();

    fn zeta_tracer_fn(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        let observed = prior_warm_state.and_then(|s| s.downcast_ref::<i32>().copied());
        *ZETA_OBSERVED_PRIOR
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = observed;
        ComputeOutcome::Completed {
            result: Value::Int(0),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    #[test]
    fn run_compute_dispatch_reads_prior_warm_state_from_cache_and_passes_to_trampoline() {
        // Reset the static for cross-process reuse.
        if let Some(m) = ZETA_OBSERVED_PRIOR.get() {
            *m.lock().unwrap() = None;
        }

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::zeta_tracer", zeta_tracer_fn as ComputeFn);

        let cell = ValueCellId::new("T", "ztp");
        let c_id = ComputeNodeId::new("T", 0);

        // Seed an output VC with a Final entry so begin_compute_dispatch
        // has a last_substantive to display.
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Seed a sentinel Compute entry, then donate warm_state(42i32) + cost.
        engine.cache_store_mut().put(
            NodeId::Compute(c_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );
        let donated = engine.cache_store_mut().donate_warm_state_with_cost(
            &NodeId::Compute(c_id.clone()),
            OpaqueState::new(42i32, 4),
            0.25,
        );
        assert!(donated, "seed donate must succeed (entry just inserted)");

        // RED: the new run_compute_dispatch signature drops the
        // `prior_warm_state` parameter — this call has 8 args (no None for
        // prior_warm_state). Fails to compile until step-6.
        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_tracer",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
        );
        let (_value, _diags) = result.expect("dispatch must Ok");

        // The trampoline must have observed Some(42) — the prior warm
        // state was sourced from the cache, not from a (dead) caller-supplied
        // argument.
        let observed = *ZETA_OBSERVED_PRIOR
            .get()
            .expect("trampoline must have set the static")
            .lock()
            .unwrap();
        assert_eq!(
            observed,
            Some(42i32),
            "trampoline must observe prior=Some(42) sourced from cache",
        );
    }

    // ── ζ / task 3425 step-7: donate new warm state on Completed ───────────
    //
    // PRD §5 step-3: the atomic-completion step writes the new value AND
    // donates the trampoline's `new_warm_state` (with `cost_per_byte`) to
    // the cache under `NodeId::Compute(c_id)` in a single critical section.
    //
    // These tests pin the donate-on-Completed wiring. They fail until step-8
    // updates the `run_compute_dispatch` Completed arm to thread
    // `new_warm_state` and `cost_per_byte.unwrap_or(0.0)` into the extended
    // `complete_compute_dispatch_atomically(c_id, &pairs, version, new_warm_state, cost)`.

    /// Trampoline returning Completed{Int(99), warm=Some(7), cost=Some(0.5)}.
    fn donate_7_fn(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        ComputeOutcome::Completed {
            result: Value::Int(99),
            new_warm_state: Some(OpaqueState::new(7i32, 4)),
            cost_per_byte: Some(0.5),
            diagnostics: vec![],
        }
    }

    #[test]
    fn run_compute_dispatch_completed_donates_new_warm_state_and_cost_to_cache() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::zeta_donate_7", donate_7_fn as ComputeFn);

        let cell = ValueCellId::new("T", "donate");
        let c_id = ComputeNodeId::new("T", 0);

        // Seed an output VC at Final so begin_compute_dispatch has a
        // last_substantive to display.
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // NOTE: NO pre-existing entry under NodeId::Compute(c_id) — the
        // donate-on-Completed path must auto-seed the sentinel entry
        // (extended complete_compute_dispatch_atomically contract).

        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_donate_7",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
        );
        let (value, _diags) = result.expect("dispatch must Ok");
        assert_eq!(value, Value::Int(99), "trampoline result must surface");

        // Output VC flipped Pending → Final by atomic-complete.
        assert_eq!(
            engine.freshness(&NodeId::Value(cell.clone())),
            Freshness::Final,
            "output VC must be Final after Completed dispatch",
        );

        // The reported cost_per_byte=0.5 must round-trip through the cache.
        // Read it BEFORE the take below: `get_warm_state` pairs the
        // cost-clear with the warm-state take (cache.rs amendment), so the
        // cost is only observable while the warm state is still present.
        assert_eq!(
            engine
                .cache_store()
                .cost_per_byte_of(&NodeId::Compute(c_id.clone())),
            Some(0.5),
            "cache cost_per_byte must reflect the trampoline's reported cost",
        );

        // The trampoline's new_warm_state must be donated to the cache.
        let observed_warm = engine
            .cache_store_mut()
            .get_warm_state(&NodeId::Compute(c_id.clone()))
            .expect("cache must hold warm state after donate-on-Completed");
        assert_eq!(
            observed_warm.downcast::<i32>(),
            Some(7i32),
            "cache warm_state must contain the donated i32=7",
        );

        // Pairing invariant: `get_warm_state` clears the companion
        // `cost_per_byte`. After the take above, the cost must read 0.0
        // (entry still exists — the cost wasn't dropped to None, it was
        // reset to 0.0).
        assert_eq!(
            engine
                .cache_store()
                .cost_per_byte_of(&NodeId::Compute(c_id)),
            Some(0.0),
            "get_warm_state must pair the take with cost_per_byte = 0.0",
        );
    }

    /// Trampoline returning Completed with NO warm state and NO cost.
    fn no_warm_no_cost_fn(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        ComputeOutcome::Completed {
            result: Value::Int(0),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    #[test]
    fn run_compute_dispatch_completed_with_none_warm_state_creates_no_compute_entry() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn(
            "test::zeta_no_warm_no_cost",
            no_warm_no_cost_fn as ComputeFn,
        );

        let cell = ValueCellId::new("T", "nowarm");
        let c_id = ComputeNodeId::new("T", 0);

        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_no_warm_no_cost",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
        );
        result.expect("dispatch must Ok");

        // No warm state reported → no Compute entry must exist
        // (auto-seed only fires when new_warm_state is Some).
        assert!(
            engine.cache_store().get(&NodeId::Compute(c_id)).is_none(),
            "no Compute entry must exist when trampoline returned new_warm_state=None",
        );
    }

    // ── ζ / task 3425 step-9: restore prior warm state on Cancelled/Failed ──
    //
    // PRD §5: "Idempotent under any number of cancel-and-redispatch cycles."
    // The Cancelled / Failed / unregistered arms must put the prior warm
    // state (and its cost_per_byte) back into the cache before returning,
    // so the next dispatch observes the same prior the cancelled one would
    // have. These tests fail until step-10 wires the restore-prior arm.
    //
    // Note on shared state: get_warm_state has take semantics — at the top
    // of run_compute_dispatch the prior is taken out of the cache; on the
    // Completed path the new warm state replaces it via atomic-complete,
    // but on Cancelled/Failed/None the prior must be re-donated explicitly.
    // Without step-10, the take leaves the cache's warm_state slot empty
    // and the assertions below fail.
    //
    // Each tracer uses its OWN OnceLock<Mutex<Option<i32>>> so the
    // step-9 tests do not race with each other or with the step-5 test's
    // ZETA_OBSERVED_PRIOR static under `cargo test`'s default thread pool.

    /// (1) Cancelled trampoline — prior warm state + cost must be restored.
    #[test]
    fn run_compute_dispatch_cancelled_restores_prior_warm_state_to_cache() {
        use crate::engine_compute::DispatchError;

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::zeta_cancelled_restore", cancellable_fn as ComputeFn);

        let cell = ValueCellId::new("T", "crc");
        let c_id = ComputeNodeId::new("T", 0);

        // Seed an output VC at Final so begin_compute_dispatch has a
        // last_substantive to display.
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Seed a sentinel Compute entry, then donate prior warm_state(99i32)
        // + cost 0.3 — this is the prior that must survive cancellation.
        engine.cache_store_mut().put(
            NodeId::Compute(c_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );
        let donated = engine.cache_store_mut().donate_warm_state_with_cost(
            &NodeId::Compute(c_id.clone()),
            OpaqueState::new(99i32, 4),
            0.3,
        );
        assert!(donated, "seed donate must succeed (entry just inserted)");

        // Pre-cancel the handle so cancellable_fn returns Cancelled.
        let handle = CancellationHandle::new();
        handle.cancel();

        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_cancelled_restore",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &handle,
            VersionId(2),
        );

        // (a) Result is Err(DispatchError::Cancelled).
        assert!(
            matches!(result, Err(DispatchError::Cancelled)),
            "Cancelled trampoline must return Err(DispatchError::Cancelled), got {result:?}",
        );

        // (c) cost_per_byte must be restored to 0.3 (read first — does not
        // consume the warm state slot).
        assert_eq!(
            engine
                .cache_store()
                .cost_per_byte_of(&NodeId::Compute(c_id.clone())),
            Some(0.3),
            "prior cost_per_byte must be restored to cache after Cancelled",
        );

        // (b) warm_state must be restored — the trampoline took 99 out and
        // returned Cancelled; step-10 puts it back. RED until step-10.
        let observed_warm = engine
            .cache_store_mut()
            .get_warm_state(&NodeId::Compute(c_id.clone()))
            .expect("cache must hold restored warm state after Cancelled");
        assert_eq!(
            observed_warm.downcast::<i32>(),
            Some(99i32),
            "prior warm_state must be restored to cache after Cancelled",
        );
    }

    /// (2) Failed trampoline — prior warm state + cost must be restored.
    #[test]
    fn run_compute_dispatch_failed_restores_prior_warm_state_to_cache() {
        use crate::engine_compute::DispatchError;

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::zeta_failed_restore", always_failed_fn as ComputeFn);

        let cell = ValueCellId::new("T", "frc");
        let c_id = ComputeNodeId::new("T", 0);

        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        engine.cache_store_mut().put(
            NodeId::Compute(c_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );
        let donated = engine.cache_store_mut().donate_warm_state_with_cost(
            &NodeId::Compute(c_id.clone()),
            OpaqueState::new(99i32, 4),
            0.3,
        );
        assert!(donated, "seed donate must succeed (entry just inserted)");

        let handle = CancellationHandle::new(); // not cancelled

        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_failed_restore",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &handle,
            VersionId(2),
        );

        // (a) Result is Err(DispatchError::Failed(_)).
        assert!(
            matches!(result, Err(DispatchError::Failed(_))),
            "Failed trampoline must return Err(DispatchError::Failed(_)), got {result:?}",
        );

        // (c) cost_per_byte restored.
        assert_eq!(
            engine
                .cache_store()
                .cost_per_byte_of(&NodeId::Compute(c_id.clone())),
            Some(0.3),
            "prior cost_per_byte must be restored to cache after Failed",
        );

        // (b) warm_state restored — RED until step-10.
        let observed_warm = engine
            .cache_store_mut()
            .get_warm_state(&NodeId::Compute(c_id.clone()))
            .expect("cache must hold restored warm state after Failed");
        assert_eq!(
            observed_warm.downcast::<i32>(),
            Some(99i32),
            "prior warm_state must be restored to cache after Failed",
        );
    }

    /// (3) Cancelled with no pre-seeded prior — no entry must be created.
    /// step-10's `if let Some(prior) = prior_warm_state.take()` guard ensures
    /// the no-prior path is a true no-op (and `donate_warm_state_with_cost`
    /// returns false on a missing entry as a defence-in-depth).
    #[test]
    fn run_compute_dispatch_cancelled_with_no_prior_leaves_cache_warm_state_none() {
        use crate::engine_compute::DispatchError;

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::zeta_cancelled_no_prior", cancellable_fn as ComputeFn);

        let cell = ValueCellId::new("T", "cnp");
        let c_id = ComputeNodeId::new("T", 0);

        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // NOTE: NO pre-existing entry under NodeId::Compute(c_id).

        let handle = CancellationHandle::new();
        handle.cancel();

        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_cancelled_no_prior",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &handle,
            VersionId(2),
        );

        assert!(
            matches!(result, Err(DispatchError::Cancelled)),
            "Cancelled trampoline must return Err(DispatchError::Cancelled), got {result:?}",
        );

        // No Compute entry must exist — there was no prior to restore, so the
        // restore-prior arm must be a true no-op (no phantom entries).
        assert!(
            engine.cache_store().get(&NodeId::Compute(c_id)).is_none(),
            "no Compute entry must exist when no prior was seeded and trampoline cancelled",
        );
    }

    /// (4) Cancel then redispatch — prior warm state survives the round-trip
    /// (PRD §5 "Idempotent under any number of cancel-and-redispatch cycles").
    static ZETA_IDEMPOTENT_OBSERVED_PRIOR: OnceLock<Mutex<Option<i32>>> = OnceLock::new();

    fn zeta_idempotent_tracer_fn(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        let observed = prior_warm_state.and_then(|s| s.downcast_ref::<i32>().copied());
        *ZETA_IDEMPOTENT_OBSERVED_PRIOR
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = observed;
        ComputeOutcome::Completed {
            result: Value::Int(0),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    #[test]
    fn run_compute_dispatch_cancelled_then_redispatched_observes_prior_idempotent() {
        use crate::engine_compute::DispatchError;

        // Reset the static (defensive — other tests don't touch this one).
        if let Some(m) = ZETA_IDEMPOTENT_OBSERVED_PRIOR.get() {
            *m.lock().unwrap() = None;
        }

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn(
            "test::zeta_idempotent_cancellable",
            cancellable_fn as ComputeFn,
        );
        engine.register_compute_fn(
            "test::zeta_idempotent_tracer",
            zeta_idempotent_tracer_fn as ComputeFn,
        );

        let cell = ValueCellId::new("T", "idem");
        let c_id = ComputeNodeId::new("T", 0);

        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Seed the prior warm_state(99i32) at cost 0.3.
        engine.cache_store_mut().put(
            NodeId::Compute(c_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );
        let donated = engine.cache_store_mut().donate_warm_state_with_cost(
            &NodeId::Compute(c_id.clone()),
            OpaqueState::new(99i32, 4),
            0.3,
        );
        assert!(donated, "seed donate must succeed (entry just inserted)");

        // First dispatch: pre-cancelled cancellable_fn → Err(Cancelled).
        // Without step-10 the prior is lost. With step-10 it is restored.
        let cancel_handle = CancellationHandle::new();
        cancel_handle.cancel();
        let first = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_idempotent_cancellable",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &cancel_handle,
            VersionId(2),
        );
        assert!(
            matches!(first, Err(DispatchError::Cancelled)),
            "first dispatch must Cancelled, got {first:?}",
        );

        // Second dispatch: the idempotent tracer must observe Some(99) —
        // the prior survived the cancellation cycle. RED until step-10.
        let second_handle = CancellationHandle::new(); // not cancelled
        let second = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_idempotent_tracer",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &second_handle,
            VersionId(3),
        );
        second.expect("second dispatch must Ok");

        let observed = *ZETA_IDEMPOTENT_OBSERVED_PRIOR
            .get()
            .expect("tracer must have set the static")
            .lock()
            .unwrap();
        assert_eq!(
            observed,
            Some(99i32),
            "after cancel-then-redispatch, the second dispatch's trampoline \
             must observe the prior=Some(99) restored by step-10",
        );
    }

    // ── ζ / task 3425 step-13: pool-only restore-prior path ────────────────
    //
    // After engine_edit step (9) the prior compute-node warm state is parked
    // in `warm_pool` and the cache Compute entry has been invalidated
    // (engine_edit.rs:2275-2303). At the next dispatch, run_compute_dispatch's
    // cache-miss → pool-hit fallback (engine_compute.rs:275-282) TAKES the
    // prior out via `checkout_with_lru_stamp`, which REMOVES the pool entry.
    // So at the moment the trampoline returns Cancelled / Failed / unregistered:
    //   - The cache has NO Compute entry (engine_edit invalidated it).
    //   - The pool has NO entry (checkout took it).
    //   - The prior is held only in the `prior_warm_state` local.
    //
    // The current restore-prior arms call `cache.donate_warm_state_with_cost`,
    // which is a silent no-op when no Compute entry exists — so the prior is
    // dropped on the floor, breaking the PRD §5 idempotence contract on the
    // post-edit path.
    //
    // Step-14 fixes this by auto-seeding a sentinel Compute entry before the
    // donate call. These tests pin that fix. Both are RED until step-14.
    //
    // The two tests cover the two observable surfaces of the bug:
    //   (1) prior visible in the cache after a Cancelled dispatch
    //   (2) prior observable by a redispatched trampoline (PRD §5 idempotence)
    //
    // The existing step-9 tests pre-seed the cache directly, so they bypass
    // this code path — they exercise the "donate succeeds because entry was
    // pre-seeded" branch, not the "donate must auto-seed first" branch.
    //
    // Each test uses a fresh OnceLock to avoid racing with step-5/step-9
    // tracers under cargo test's default thread pool.

    /// (1) Cancelled with pool-only prior — cache must hold the prior after.
    #[test]
    fn run_compute_dispatch_cancelled_with_pool_only_prior_survives_into_cache() {
        use crate::engine_compute::DispatchError;

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn(
            "test::zeta_pool_only_cancelled",
            cancellable_fn as ComputeFn,
        );

        let cell = ValueCellId::new("T", "po1");
        let c_id = ComputeNodeId::new("T", 0);

        // Seed an output VC at Final so begin_compute_dispatch has a
        // last_substantive to display.
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Seed the prior ONLY in the warm_pool — NO cache Compute entry.
        // This mirrors the state engine_edit step (9) leaves behind after a
        // source edit that drops the compute_node from the new snapshot.
        engine.warm_pool_mut().donate_with_cost(
            NodeId::Compute(c_id.clone()),
            OpaqueState::new(99i32, 4),
            0.3,
        );

        // Sanity: no cache entry exists under the Compute node before dispatch.
        assert!(
            engine
                .cache_store()
                .get(&NodeId::Compute(c_id.clone()))
                .is_none(),
            "precondition: no cache Compute entry before dispatch",
        );

        // Pre-cancel the handle so cancellable_fn returns Cancelled.
        let handle = CancellationHandle::new();
        handle.cancel();

        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_pool_only_cancelled",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &handle,
            VersionId(2),
        );

        // (a) Result is Err(DispatchError::Cancelled).
        assert!(
            matches!(result, Err(DispatchError::Cancelled)),
            "Cancelled trampoline must return Err(DispatchError::Cancelled), got {result:?}",
        );

        // (b) cost_per_byte must be restored to 0.3 — read BEFORE the warm
        // state take so the cost assertion does not depend on order.
        assert_eq!(
            engine
                .cache_store()
                .cost_per_byte_of(&NodeId::Compute(c_id.clone())),
            Some(0.3),
            "prior cost_per_byte must be restored to cache after Cancelled \
             on the pool-only path (step-14 auto-seed-then-donate)",
        );

        // (c) warm_state must be restored — the prior survived the cancel
        // through the auto-seed-then-donate fix in step-14.
        let observed_warm = engine
            .cache_store_mut()
            .get_warm_state(&NodeId::Compute(c_id.clone()))
            .expect(
                "cache must hold restored warm state after Cancelled on the \
                 pool-only path (step-14 auto-seed-then-donate)",
            );
        assert_eq!(
            observed_warm.downcast::<i32>(),
            Some(99i32),
            "prior warm_state(99i32) must be restored to cache after Cancelled",
        );
    }

    /// (2) Cancel then redispatch on the pool-only path — prior observable.
    /// PRD §5 "Idempotent under any number of cancel-and-redispatch cycles"
    /// must hold for the post-edit path too, not only the cache-pre-seeded
    /// path covered by step-9.
    static ZETA_POOL_ONLY_IDEM_OBSERVED_PRIOR: OnceLock<Mutex<Option<i32>>> = OnceLock::new();

    fn zeta_pool_only_idem_tracer_fn(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        let observed = prior_warm_state.and_then(|s| s.downcast_ref::<i32>().copied());
        *ZETA_POOL_ONLY_IDEM_OBSERVED_PRIOR
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = observed;
        ComputeOutcome::Completed {
            result: Value::Int(0),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    #[test]
    fn run_compute_dispatch_cancelled_pool_only_then_redispatched_observes_prior_idempotent() {
        use crate::engine_compute::DispatchError;

        // Reset the static (defensive — other tests don't touch this one).
        if let Some(m) = ZETA_POOL_ONLY_IDEM_OBSERVED_PRIOR.get() {
            *m.lock().unwrap() = None;
        }

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn(
            "test::zeta_pool_only_idem_cancellable",
            cancellable_fn as ComputeFn,
        );
        engine.register_compute_fn(
            "test::zeta_pool_only_idem_tracer",
            zeta_pool_only_idem_tracer_fn as ComputeFn,
        );

        let cell = ValueCellId::new("T", "po2");
        let c_id = ComputeNodeId::new("T", 0);

        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Seed the prior ONLY in the warm_pool at cost 0.3, NO cache entry.
        engine.warm_pool_mut().donate_with_cost(
            NodeId::Compute(c_id.clone()),
            OpaqueState::new(99i32, 4),
            0.3,
        );

        // First dispatch: pre-cancelled cancellable_fn → Err(Cancelled).
        // The fallback takes the prior from the pool (removing the pool
        // entry); step-14 must restore it to the cache so the second
        // dispatch can observe it.
        let cancel_handle = CancellationHandle::new();
        cancel_handle.cancel();
        let first = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_pool_only_idem_cancellable",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &cancel_handle,
            VersionId(2),
        );
        assert!(
            matches!(first, Err(DispatchError::Cancelled)),
            "first dispatch must Cancelled, got {first:?}",
        );

        // Second dispatch: the tracer must observe Some(99) — the prior
        // survived the post-edit cancellation cycle through step-14.
        let second_handle = CancellationHandle::new(); // not cancelled
        let second = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::zeta_pool_only_idem_tracer",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &second_handle,
            VersionId(3),
        );
        second.expect("second dispatch must Ok");

        let observed = *ZETA_POOL_ONLY_IDEM_OBSERVED_PRIOR
            .get()
            .expect("tracer must have set the static")
            .lock()
            .unwrap();
        assert_eq!(
            observed,
            Some(99i32),
            "after cancel-then-redispatch on the pool-only path, the second \
             dispatch's trampoline must observe the prior=Some(99) restored \
             by step-14's auto-seed-then-donate",
        );
    }

    // ── task-4079 step-5: run_compute_dispatch installs solver-progress context ──
    //
    // Proves that run_compute_dispatch threads the Engine's solver_progress_sink
    // and active_solve_cancel into the thread-local SolveDispatchContext visible
    // to the trampoline.
    //
    // Fails to COMPILE until step-6 adds Engine::set_solver_progress_sink,
    // Engine::set_active_solve_cancel, and the context-install wiring inside
    // run_compute_dispatch → RED.

    /// Observed state: (sink_present, cancel_present, cancel_was_cancelled).
    static CTX_OBSERVER_RESULT: OnceLock<Mutex<Option<(bool, bool, bool)>>> = OnceLock::new();

    fn ctx_observer_fn(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        use crate::solver_progress::current_solve_dispatch_context;
        let observation = current_solve_dispatch_context().map(|(sink, cancel)| {
            let sink_present = sink.is_some();
            let cancel_present = cancel.is_some();
            let cancel_cancelled = cancel.as_ref().is_some_and(|c| c.is_cancelled());
            (sink_present, cancel_present, cancel_cancelled)
        });
        *CTX_OBSERVER_RESULT
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = observation;
        ComputeOutcome::Completed {
            result: Value::Int(0),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    #[test]
    fn run_compute_dispatch_installs_solver_progress_context_for_trampoline() {
        use std::sync::Arc;

        use crate::solver_progress::{SolverProgressSink, SolverProgressUpdate};

        // Reset static.
        if let Some(m) = CTX_OBSERVER_RESULT.get() {
            *m.lock().unwrap() = None;
        }

        // Minimal no-op recording sink.
        struct NoopSink;
        impl SolverProgressSink for NoopSink {
            fn on_iteration(&self, _update: &SolverProgressUpdate) {}
        }

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::ctx_observer", ctx_observer_fn as ComputeFn);

        // Install sink and a NON-cancelled handle.
        engine.set_solver_progress_sink(Arc::new(NoopSink));
        let handle = CancellationHandle::new();
        engine.set_active_solve_cancel(Some(handle));

        let cell = ValueCellId::new("T", "ctx_obs");
        let c_id = ComputeNodeId::new("T", 0);
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::ctx_observer",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
        ).expect("dispatch must Ok");

        let observed = CTX_OBSERVER_RESULT
            .get()
            .expect("trampoline must have set the static")
            .lock()
            .unwrap()
            .expect("trampoline must observe Some context");
        assert!(observed.0, "sink must be visible to the trampoline");
        assert!(observed.1, "cancel handle must be visible to the trampoline");
        assert!(!observed.2, "cancel handle must NOT be cancelled yet");
    }

    #[test]
    fn run_compute_dispatch_installs_pre_cancelled_handle_in_context() {
        use std::sync::Arc;

        use crate::solver_progress::{SolverProgressSink, SolverProgressUpdate};

        // Reset static (reuse CTX_OBSERVER_RESULT — runs serially in a
        // different test but share the reset guard).
        if let Some(m) = CTX_OBSERVER_RESULT.get() {
            *m.lock().unwrap() = None;
        }

        struct NoopSink2;
        impl SolverProgressSink for NoopSink2 {
            fn on_iteration(&self, _update: &SolverProgressUpdate) {}
        }

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::ctx_observer_cancelled", ctx_observer_fn as ComputeFn);

        engine.set_solver_progress_sink(Arc::new(NoopSink2));
        let cancelled_handle = CancellationHandle::new();
        cancelled_handle.cancel();
        engine.set_active_solve_cancel(Some(cancelled_handle));

        let cell = ValueCellId::new("T", "ctx_can");
        let c_id = ComputeNodeId::new("T", 0);
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // run_compute_dispatch passes a fresh uncancelled handle as the
        // `cancellation` arg — the pre-cancelled state lives in the context.
        engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::ctx_observer_cancelled",
            &[Value::Int(0)],
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
        ).expect("dispatch must Ok");

        let observed = CTX_OBSERVER_RESULT
            .get()
            .expect("trampoline must have set the static")
            .lock()
            .unwrap()
            .expect("trampoline must observe Some context");
        assert!(observed.0, "sink must be visible");
        assert!(observed.1, "cancel handle must be visible");
        assert!(observed.2, "pre-cancelled handle must show is_cancelled==true in context");
    }
}
