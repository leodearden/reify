// Compute-node dispatch registry and associated types.
//
// See `docs/prds/v0_3/compute-node-contract.md` §4 and §8-γ for the full spec.
// Types defined here: `ComputeFn`, `ComputeOutcome`, `RealizationReadHandle`,
// `ComputeDispatchRegistry`, `DispatchError`.

use std::collections::HashMap;
use std::sync::Arc;

use reify_core::{
    ComputeNodeId, ContentHash, Diagnostic, RealizationNodeId, ValueCellId, VersionId,
};
use reify_ir::{Mesh, OpaqueState, SampledField, Value, VolumeMesh};

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

/// The content of a realized geometry node.
///
/// Wraps the three concrete content kinds the realization pipeline can produce,
/// each held behind an `Arc` for cheap cloning and multi-consumer sharing.
/// `Arc<T>: Clone` is unconditional, so this enum derives `Clone` even though
/// `SampledField` itself is not `Clone` (PRD §3.1 realization-read-api.md).
///
/// `None` content on a [`RealizationReadHandle`] (BRep-only or honest degradation)
/// means every accessor returns `None` — no content is fabricated, no panic
/// occurs (invariants §3.2-5 of the PRD).
///
/// See `docs/prds/v0_6/realization-read-api.md` task α §3.1.
#[derive(Debug, Clone)]
pub enum RealizedContent {
    /// A signed-distance field (volumetric scalar field).
    Sdf(Arc<SampledField>),
    /// A tessellated surface mesh.
    SurfaceMesh(Arc<Mesh>),
    /// A tetrahedral volume mesh.
    VolumeMesh(Arc<VolumeMesh>),
}

/// Minimal read-only wrapper over a realization node identity and its optional content.
///
/// Passed to [`ComputeFn`] invocations that declare realization inputs.
/// `content_hash` identifies the content (matches the compute-cache key's
/// realization hash). The `content()` accessor returns the payload when content
/// is available; `None` when BRep-only or not yet hydrated (honest-degradation
/// invariant §3.2-5, realization-read-api.md §3.2).
///
/// The `content` field is private: `new()` is the sole construction path,
/// honouring PRD §3.1 "only the Engine-side constructor builds handles".
///
/// See `docs/prds/v0_6/realization-read-api.md` task α §3.1/§3.2.
#[derive(Debug, Clone)]
pub struct RealizationReadHandle {
    /// Identity of the realization node this handle references.
    pub node_id: RealizationNodeId,
    /// Content hash of the realization (mirrors the compute-cache key).
    pub content_hash: ContentHash,
    /// Optional content payload.  Private so `new()` is the sole construction
    /// path (PRD §3.1).
    content: Option<RealizedContent>,
}

impl RealizationReadHandle {
    /// Construct a handle from its three components.
    ///
    /// `pub` (not `pub(crate)`) because external integration tests and future
    /// η two-way boundary tests construct handles from outside this crate.
    pub fn new(
        node_id: RealizationNodeId,
        content_hash: ContentHash,
        content: Option<RealizedContent>,
    ) -> Self {
        Self {
            node_id,
            content_hash,
            content,
        }
    }

    /// Return a reference to the content payload, or `None` when absent.
    pub fn content(&self) -> Option<&RealizedContent> {
        self.content.as_ref()
    }

    /// Return a reference to the inner [`SampledField`] when the content is
    /// [`RealizedContent::Sdf`]; `None` otherwise.
    pub fn sdf(&self) -> Option<&SampledField> {
        match self.content.as_ref() {
            Some(RealizedContent::Sdf(a)) => Some(a),
            _ => None,
        }
    }

    /// Return a reference to the inner [`Mesh`] when the content is
    /// [`RealizedContent::SurfaceMesh`]; `None` otherwise.
    pub fn surface_mesh(&self) -> Option<&Mesh> {
        match self.content.as_ref() {
            Some(RealizedContent::SurfaceMesh(a)) => Some(a),
            _ => None,
        }
    }

    /// Return a reference to the inner [`VolumeMesh`] when the content is
    /// [`RealizedContent::VolumeMesh`]; `None` otherwise.
    pub fn volume_mesh(&self) -> Option<&VolumeMesh> {
        match self.content.as_ref() {
            Some(RealizedContent::VolumeMesh(a)) => Some(a),
            _ => None,
        }
    }
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
        // task #3428 step-6: persistent-cache input key (from compute_cache_key).
        // `ContentHash(0)` is inert (no-op when cache_dir is None, which is the
        // default for all existing tests).
        cache_key: ContentHash,
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

        // Step 1b: task #3428 step-8 — persistent lookup (before invoke).
        //
        // If a cache dir is configured AND the target is in the persistable
        // allowlist, attempt to read a prior result from the on-disk cache.
        // On a HIT: run the fold hook (mirrors the Completed arm so
        // topology_attribute_table stays consistent — per the NOTE at line 407),
        // atomically complete the dispatch (Pending → Final), bump the hit
        // counter, and return without ever invoking the trampoline.
        // On a MISS: bump the miss counter and fall through to invoke unchanged.
        if let Some(cache_dir) = self.persistent_cache_dir.as_deref()
            && crate::compute_persist::is_persistable_target(target)
        {
            match crate::compute_persist::persistent_lookup(cache_dir, target, cache_key) {
                Some(result) => {
                    // Fold hook — mirrors the Completed arm.
                    if target == "shell-extract::extract" {
                        crate::shell_extract_compute::fold_mid_surface_attributes_into_table(
                            &mut self.topology_attribute_table,
                            &result,
                        );
                    }
                    // Atomic completion (write + Pending→Final + clear cause).
                    // No warm state donation (no solve ran); cost = 0.
                    let pairs: Vec<(ValueCellId, Value)> = outputs
                        .iter()
                        .map(|o| (o.clone(), result.clone()))
                        .collect();
                    self.cache.complete_compute_dispatch_atomically(
                        c_id,
                        &pairs,
                        version,
                        None, // new_warm_state — no solve, no warm state
                        0.0,  // cost_per_byte unknown for a cache hit
                    );
                    self.persistent_hit_count += 1;
                    return Ok((result, vec![]));
                }
                None => {
                    self.persistent_miss_count += 1;
                    // Fall through to invoke_compute_trampoline below.
                }
            }
        }

        // Step 2: install the solver-progress dispatch context (task #4079),
        // then invoke the trampoline.  The RAII guard clears the thread-local
        // slot on drop — even on panic or early return.
        let _ctx_guard = crate::solver_progress::install_solve_dispatch_context(
            self.solver_progress_sink.clone(),
            self.active_solve_cancel.clone(),
        );

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

                // θ / task 3427: significance-filter suppression at the
                // output-VC boundary. Suppression is only applied when
                // outputs.len() == 1 (the single-output contract enforced by
                // debug_assert_eq! above). When outputs.len() != 1 (empty OR
                // >1), the filter is skipped entirely and the new result is
                // written as-is — so a future multi-output caller degrades to
                // correct-but-unsuppressed behavior in release builds rather
                // than silently broadcasting outputs[0]'s suppression decision
                // to every output cell.
                // On SignificanceOutcome::Equivalent the prior cached value
                // (bundled in the outcome) is written instead of `result`,
                // preserving the VC's content hash bit-identically →
                // record_evaluation_with_freshness takes the same-hash
                // early-cutoff → EvalOutcome::Unchanged → downstream consumers
                // are NOT invalidated or recomputed.
                //
                // Determinacy semantics: `complete_compute_dispatch_atomically`
                // always writes `DeterminacyState::Determined` for output VCs,
                // regardless of the prior entry's determinacy or the new result.
                // The Equivalent arm therefore adopts `Determined` — the new
                // dispatch's determinacy (warm-state-advances semantics). A
                // completed dispatch is always determined; output_significance_outcome
                // asserts the prior is also Determined so the content hash is
                // fully preserved on the Equivalent path.
                //
                // Warm state + cost always advance to the new state (Completed
                // is semantically a full re-run regardless of output proximity).
                let effective_value = if outputs.len() == 1 {
                    // output_significance_outcome bundles the prior Value in the
                    // Equivalent arm — no second cache lookup needed here.
                    // `result` is moved in the NotSuppressed arm; the &result
                    // borrow passed to output_significance_outcome ends before
                    // the match arm executes, so the move is sound.
                    // SAFETY: checked outputs.len() == 1 above.
                    match self.output_significance_outcome(target, &outputs[0], &result) {
                        SignificanceOutcome::Equivalent(prior) => prior,
                        SignificanceOutcome::NotSuppressed => result,
                    }
                } else {
                    // outputs.len() != 1 (empty OR >1): debug_assert_eq! above
                    // fires in debug builds; in release skip suppression and
                    // fall through with the new result so behavior is
                    // correct-but-unsuppressed rather than a silent wrong broadcast.
                    result
                };

                // Step 3a: atomic completion (write + flip Pending→Final +
                // clear cause + donate warm state). PRD §5 step-3 bundles
                // all four operations into a single critical section.
                //
                // NOTE: significance suppression is single-output-only — the
                // decision is derived from outputs[0]'s prior value and
                // tolerance (mirroring the debug_assert above). When
                // outputs.len() != 1, the effective_value block above skips
                // suppression entirely (correct-but-unsuppressed degradation).
                // If multi-output dispatch is ever enabled, `effective_value`
                // must become per-output and `output_significance_outcome` must
                // be called once per VC with its own prior and tolerance.
                let pairs: Vec<(ValueCellId, Value)> = outputs
                    .iter()
                    .map(|o| (o.clone(), effective_value.clone()))
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
                // task #3428 step-6: best-effort persistent write.
                // Gated on cache_dir.is_some() AND the persistable-target allowlist
                // so that (a) no write occurs when no cache dir is configured
                // (the default — all existing tests are unaffected), and (b)
                // only opted-in targets are persisted.
                //
                // MERGE (#3427 θ × #3428): persist `effective_value`, NOT the raw
                // trampoline `result`. `effective_value` is exactly what
                // `complete_compute_dispatch_atomically` wrote to the in-memory
                // cache above; on SignificanceOutcome::Equivalent it is the prior
                // value (content-hash preserved) and differs from `result`. The
                // persistable allowlist mirrors the significance opt-in allowlist
                // (both are {solver::elastic_static, solver::buckling}), so a
                // persisted target is ALWAYS significance-filtered and the two can
                // genuinely diverge here. The persistent-hit path (Step 1b) writes
                // the stored value straight into the in-memory cache WITHOUT
                // re-running the significance filter, so storing `effective_value`
                // keeps a future hit consistent with the suppressed in-memory state
                // and avoids a spurious downstream invalidation that would defeat
                // the significance filter. This preserves #3428's invariant that the
                // on-disk cache mirrors the in-memory cache for (target, cache_key).
                if let Some(cache_dir) = self.persistent_cache_dir.as_deref()
                    && crate::compute_persist::is_persistable_target(target)
                {
                    crate::compute_persist::persistent_write(
                        cache_dir,
                        target,
                        cache_key,
                        &effective_value,
                    );
                }
                // θ / task 3427 step-4: return effective_value (prior on
                // Equivalent, new result otherwise) so the engine_eval values
                // map is consistent with what the cache holds.
                Ok((effective_value, diagnostics))
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

/// Outcome of [`Engine::output_significance_outcome`] — richer than
/// [`crate::significance_filter::FilterOutcome`] because the `Equivalent` arm
/// bundles the already-fetched prior [`reify_ir::Value`].  This lets the caller
/// avoid a second cache lookup and eliminates any need for a conservative
/// `unwrap_or_else` fallback: if the prior was absent, the helper returns
/// `NotSuppressed` before ever returning `Equivalent`.
enum SignificanceOutcome {
    /// The new result is tolerance-equivalent to the prior cached value.
    /// The prior [`reify_ir::Value`] is carried here so the caller can write
    /// it directly, preserving the output VC's content hash bit-identically.
    Equivalent(reify_ir::Value),
    /// The new result differs beyond tolerance, or there is no prior / no
    /// active tolerance / the target is not opted-in. Write the new dispatch
    /// result — normal (today's) cache-update behavior.
    NotSuppressed,
}

impl crate::Engine {
    /// Determine the output significance outcome for a single dispatch output VC.
    ///
    /// Called from `run_compute_dispatch`'s Completed arm (θ / task 3427) to
    /// decide whether the new trampoline result is tolerance-equivalent to the
    /// prior cached value. When [`SignificanceOutcome::Equivalent`] is returned,
    /// the prior `Value` is bundled so the caller can write it without a second
    /// cache lookup, preserving the VC's content hash bit-identically.
    ///
    /// # Lookup chain
    ///
    /// 1. Read the prior cached `Value` for `NodeId::Value(out)`.  This is
    ///    available even after `begin_compute_dispatch` because that helper
    ///    only changes `freshness` (Pending), not `result`.
    /// 2. Resolve `self.active_tolerance_for(out.entity)`.
    /// 3. Delegate to `crate::significance_filter::significance_filter`.
    ///    On `FilterOutcome::Equivalent`, return
    ///    `SignificanceOutcome::Equivalent(prior)` with the already-fetched
    ///    prior value; all other outcomes map to `NotSuppressed`.
    ///
    /// # Conservative fallbacks → `NotSuppressed`
    ///
    /// Returns [`SignificanceOutcome::NotSuppressed`] (normal-invalidation) when:
    /// - The VC cache entry is absent (first-time dispatch, no prior).
    /// - The cached result is not a `CachedResult::Value` (unexpected variant).
    /// - `active_tolerance_for` returns `None` (no active purpose).
    /// - The target is not in the significance-filter opt-in allowlist.
    /// - The result shape is malformed for this target.
    ///
    /// All of these fall through to normal (today's) cache-update behavior —
    /// the significance filter is a strict no-op for every dispatch path that
    /// does not activate a tolerance-bearing purpose on an opted-in target.
    fn output_significance_outcome(
        &self,
        target: &str,
        out: &ValueCellId,
        new: &reify_ir::Value,
    ) -> SignificanceOutcome {
        use crate::significance_filter::FilterOutcome;

        // Fast-path: only two targets are opted in to significance filtering
        // ('solver::elastic_static', 'solver::buckling').  For the overwhelming
        // majority of dispatches the guard below short-circuits before the
        // cache.get + clone + active_tolerance_for lookups, keeping the
        // dispatch completion hot-path free of that overhead.
        if !crate::significance_filter::is_opted_in(target) {
            return SignificanceOutcome::NotSuppressed;
        }

        // Read the prior cached Value (preserved through begin_compute_dispatch).
        // Both this immutable borrow of self.cache and the active_tolerance_for
        // call below are &self — they coexist without borrow conflict.
        let prev = match self.cache.get(&crate::cache::NodeId::Value(out.clone())) {
            Some(entry) => match &entry.result {
                crate::cache::CachedResult::Value(v, det) => {
                    // `complete_compute_dispatch_atomically` always writes
                    // `Determined` for dispatch-completed VCs, so the Equivalent
                    // arm's hash-preservation holds. If a prior entry were ever
                    // non-Determined, writing (prior_value, Determined) would
                    // produce a DIFFERENT hash — silently defeating suppression.
                    // Assert the invariant in debug builds to surface any future
                    // violation rather than leaving it as an unverified comment.
                    debug_assert_eq!(
                        *det,
                        reify_ir::DeterminacyState::Determined,
                        "dispatch-completed prior cache entry must be Determined; \
                         a non-Determined prior would produce a different content \
                         hash on the Equivalent path, silently defeating suppression",
                    );
                    v
                }
                _ => return SignificanceOutcome::NotSuppressed,
            },
            None => return SignificanceOutcome::NotSuppressed,
        };

        // Resolve the active tolerance for the output VC's entity.
        let tol = self.active_tolerance_for(out.entity.as_str());

        match crate::significance_filter::significance_filter(target, prev, new, tol) {
            FilterOutcome::Equivalent => SignificanceOutcome::Equivalent(prev.clone()),
            FilterOutcome::Different | FilterOutcome::NotOptedIn => {
                SignificanceOutcome::NotSuppressed
            }
        }
    }
}

impl crate::Engine {
    /// Build the per-dispatch `realization_inputs` and `RealizationReadHandle` vec
    /// by scanning `arg_values` for `Value::GeometryHandle` entries.
    ///
    /// For each `Value::GeometryHandle { realization_ref, .. }` encountered (in
    /// argument order, first-occurrence wins), the method:
    /// 1. Pushes `realization_ref` into the returned `inputs` vec.
    /// 2. Calls [`project_realization_read_handle`](crate::Engine::project_realization_read_handle)
    ///    to obtain the matching handle (possibly degraded to `None` content in β).
    /// 3. Accumulates any degradation diagnostics.
    ///
    /// Non-`GeometryHandle` args are skipped; their values flow through
    /// `value_inputs` unchanged (value identity via `Value`, content identity via
    /// handle).
    ///
    /// ## Dedup (first-occurrence, arg order preserved)
    ///
    /// Duplicate `RealizationNodeId`s are deduplicated: if the same id appears
    /// more than once in `arg_values`, only the first occurrence is contributed to
    /// `inputs`.  This is required by `compute_cache_key`, which asserts that
    /// `realization_inputs` contains no duplicates.
    ///
    /// The returned `inputs` and `handles` are 1-to-1 (same length); each
    /// `handles[i]` corresponds to `inputs[i]`.
    pub(crate) fn build_compute_realization_inputs(
        &mut self,
        arg_values: &[Value],
        graph: &crate::graph::EvaluationGraph,
    ) -> (
        Vec<reify_core::RealizationNodeId>,
        Vec<RealizationReadHandle>,
        Vec<reify_core::Diagnostic>,
    ) {
        let mut inputs = Vec::new();
        let mut handles = Vec::new();
        let mut diags = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for arg in arg_values {
            if let Value::GeometryHandle {
                realization_ref, ..
            } = arg
                && seen.insert(realization_ref)
            {
                let (handle, arg_diags) =
                    self.project_realization_read_handle(realization_ref, graph);
                inputs.push(realization_ref.clone());
                handles.push(handle);
                diags.extend(arg_diags);
            }
        }

        (inputs, handles, diags)
    }
}

#[cfg(test)]
mod tests {
    use reify_core::RealizationNodeId;
    use reify_ir::{OpaqueState, Value};
    use reify_test_support::mocks::MockConstraintChecker;

    use crate::Engine;
    use crate::engine_compute::{
        ComputeFn, ComputeOutcome, RealizationReadHandle, RealizedContent,
    };
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
    use reify_core::{ComputeNodeId, ContentHash, ValueCellId, VersionId};
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
        let _h = RealizationReadHandle::new(
            RealizationNodeId::new("test", 0),
            reify_core::ContentHash(0),
            None,
        );
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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

    // Each observer uses its OWN OnceLock<Mutex<..>> static AND its OWN
    // trampoline fn so the two tests do not race under `cargo test`'s default
    // multi-thread pool.  ComputeFn is a bare fn-pointer (cannot capture),
    // so per-test static + per-test fn is the only zero-shared-state option —
    // matching the ZETA-tracer convention at engine_compute.rs:1065-1067.

    /// Observed state for the progress-sink test: (sink_present, cancel_present, cancel_was_cancelled).
    static CTX_OBSERVER_RESULT_PROGRESS: OnceLock<Mutex<Option<(bool, bool, bool)>>> =
        OnceLock::new();

    fn ctx_observer_progress_fn(
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
        *CTX_OBSERVER_RESULT_PROGRESS
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

    /// Observed state for the pre-cancelled-handle test: (sink_present, cancel_present, cancel_was_cancelled).
    static CTX_OBSERVER_RESULT_CANCEL: OnceLock<Mutex<Option<(bool, bool, bool)>>> =
        OnceLock::new();

    fn ctx_observer_cancel_fn(
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
        *CTX_OBSERVER_RESULT_CANCEL
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

        // Reset this test's dedicated static.
        if let Some(m) = CTX_OBSERVER_RESULT_PROGRESS.get() {
            *m.lock().unwrap() = None;
        }

        // Minimal no-op recording sink.
        struct NoopSink;
        impl SolverProgressSink for NoopSink {
            fn on_iteration(&self, _update: &SolverProgressUpdate) {}
        }

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("test::ctx_observer", ctx_observer_progress_fn as ComputeFn);

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

        engine
            .run_compute_dispatch(
                &c_id,
                std::slice::from_ref(&cell),
                "test::ctx_observer",
                &[Value::Int(0)],
                &[],
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(2),
                ContentHash(0), // inert: no cache dir in tests
            )
            .expect("dispatch must Ok");

        let observed = CTX_OBSERVER_RESULT_PROGRESS
            .get()
            .expect("trampoline must have set the static")
            .lock()
            .unwrap()
            .expect("trampoline must observe Some context");
        assert!(observed.0, "sink must be visible to the trampoline");
        assert!(
            observed.1,
            "cancel handle must be visible to the trampoline"
        );
        assert!(!observed.2, "cancel handle must NOT be cancelled yet");
    }

    #[test]
    fn run_compute_dispatch_installs_pre_cancelled_handle_in_context() {
        use std::sync::Arc;

        use crate::solver_progress::{SolverProgressSink, SolverProgressUpdate};

        // Reset this test's dedicated static.
        if let Some(m) = CTX_OBSERVER_RESULT_CANCEL.get() {
            *m.lock().unwrap() = None;
        }

        struct NoopSink2;
        impl SolverProgressSink for NoopSink2 {
            fn on_iteration(&self, _update: &SolverProgressUpdate) {}
        }

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn(
            "test::ctx_observer_cancelled",
            ctx_observer_cancel_fn as ComputeFn,
        );

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
        engine
            .run_compute_dispatch(
                &c_id,
                std::slice::from_ref(&cell),
                "test::ctx_observer_cancelled",
                &[Value::Int(0)],
                &[],
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(2),
                ContentHash(0), // inert: no cache dir in tests
            )
            .expect("dispatch must Ok");

        let observed = CTX_OBSERVER_RESULT_CANCEL
            .get()
            .expect("trampoline must have set the static")
            .lock()
            .unwrap()
            .expect("trampoline must observe Some context");
        assert!(observed.0, "sink must be visible");
        assert!(observed.1, "cancel handle must be visible");
        assert!(
            observed.2,
            "pre-cancelled handle must show is_cancelled==true in context"
        );
    }

    // ── α: RealizedContent + RealizationReadHandle content accessors ─────────
    // Step-1 RED: RealizedContent, new(), content_hash field, content() do not
    // yet exist — this block fails to compile until step-2 lands them.

    #[test]
    fn handle_new_stores_content_hash_and_content_accessor_returns_it() {
        use reify_core::ContentHash;
        use reify_ir::Mesh;
        use std::sync::Arc;

        let h = RealizationReadHandle::new(
            RealizationNodeId::new("n", 0),
            ContentHash(7),
            Some(RealizedContent::SurfaceMesh(Arc::new(Mesh {
                vertices: vec![],
                indices: vec![],
                normals: None,
            }))),
        );
        assert_eq!(
            h.content_hash,
            ContentHash(7),
            "content_hash field must be stored"
        );
        assert!(
            matches!(h.content(), Some(RealizedContent::SurfaceMesh(_))),
            "content() must return the stored variant",
        );
    }

    // ── α step-3 RED: typed accessors + honest-None + Clone Arc-sharing ──────
    // sdf(), surface_mesh(), volume_mesh() exist after step-4 impl.

    #[test]
    fn typed_accessor_volume_mesh_returns_some_others_none() {
        use reify_core::ContentHash;
        use reify_ir::{ElementOrderTag, VolumeMesh};
        use std::sync::Arc;

        let h = RealizationReadHandle::new(
            RealizationNodeId::new("v", 0),
            ContentHash(1),
            Some(RealizedContent::VolumeMesh(Arc::new(VolumeMesh {
                vertices: vec![],
                tet_indices: vec![],
                element_order: ElementOrderTag::P1,
                normals: None,
            }))),
        );
        assert!(
            h.volume_mesh().is_some(),
            "volume_mesh() must return Some for VolumeMesh content"
        );
        assert!(
            h.sdf().is_none(),
            "sdf() must return None for VolumeMesh content"
        );
        assert!(
            h.surface_mesh().is_none(),
            "surface_mesh() must return None for VolumeMesh content"
        );
    }

    #[test]
    fn typed_accessor_surface_mesh_returns_some_others_none() {
        use reify_core::ContentHash;
        use reify_ir::Mesh;
        use std::sync::Arc;

        let h = RealizationReadHandle::new(
            RealizationNodeId::new("s", 0),
            ContentHash(2),
            Some(RealizedContent::SurfaceMesh(Arc::new(Mesh {
                vertices: vec![],
                indices: vec![],
                normals: None,
            }))),
        );
        assert!(
            h.surface_mesh().is_some(),
            "surface_mesh() must return Some for SurfaceMesh content"
        );
        assert!(
            h.sdf().is_none(),
            "sdf() must return None for SurfaceMesh content"
        );
        assert!(
            h.volume_mesh().is_none(),
            "volume_mesh() must return None for SurfaceMesh content"
        );
    }

    #[test]
    fn typed_accessor_sdf_returns_some_others_none() {
        use reify_core::ContentHash;
        use reify_ir::{InterpolationKind, SampledField, SampledGridKind};
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        let sf = SampledField {
            name: "test".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![1.0],
            spacing: vec![1.0],
            axis_grids: vec![vec![0.0, 1.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![0.0, 1.0],
            oob_emitted: AtomicBool::new(false),
        };
        let h = RealizationReadHandle::new(
            RealizationNodeId::new("f", 0),
            ContentHash(3),
            Some(RealizedContent::Sdf(Arc::new(sf))),
        );
        assert!(h.sdf().is_some(), "sdf() must return Some for Sdf content");
        assert!(
            matches!(h.content(), Some(RealizedContent::Sdf(_))),
            "content() must return Some(Sdf) for Sdf content",
        );
        assert!(
            h.surface_mesh().is_none(),
            "surface_mesh() must return None for Sdf content"
        );
        assert!(
            h.volume_mesh().is_none(),
            "volume_mesh() must return None for Sdf content"
        );
    }

    #[test]
    fn typed_accessors_all_none_when_content_is_none() {
        use reify_core::ContentHash;

        let h = RealizationReadHandle::new(RealizationNodeId::new("x", 0), ContentHash(0), None);
        assert!(
            h.content().is_none(),
            "content() must be None for None-content handle"
        );
        assert!(
            h.sdf().is_none(),
            "sdf() must be None for None-content handle"
        );
        assert!(
            h.surface_mesh().is_none(),
            "surface_mesh() must be None for None-content handle"
        );
        assert!(
            h.volume_mesh().is_none(),
            "volume_mesh() must be None for None-content handle"
        );
    }

    #[test]
    fn clone_shares_arc_allocation_ptr_eq() {
        use reify_core::ContentHash;
        use reify_ir::{ElementOrderTag, VolumeMesh};
        use std::sync::Arc;

        let h = RealizationReadHandle::new(
            RealizationNodeId::new("c", 0),
            ContentHash(42),
            Some(RealizedContent::VolumeMesh(Arc::new(VolumeMesh {
                vertices: vec![],
                tet_indices: vec![],
                element_order: ElementOrderTag::P1,
                normals: None,
            }))),
        );
        let c = h.clone();
        // Clone must share the inner Arc allocation (ptr_eq invariant §8).
        assert!(
            std::ptr::eq(h.volume_mesh().unwrap(), c.volume_mesh().unwrap()),
            "cloned handle must share the same Arc allocation (ptr_eq)",
        );
        assert_eq!(
            h.content_hash, c.content_hash,
            "content_hash must match after clone"
        );
    }

    // ── β / task 4508 step-5: RED — build_compute_realization_inputs ──────────
    //
    // These tests verify the lowering rule: only Value::GeometryHandle args
    // contribute to realization_inputs (in arg order, no dedup).
    // step-6 (impl) makes them pass by implementing build_compute_realization_inputs.

    mod beta_lowering {
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, ValueCellId, VersionId};
        use reify_ir::{DeterminacyState, Freshness, ReprKind, Value};
        use reify_test_support::mocks::MockConstraintChecker;

        use crate::Engine;
        use crate::cache::{CachedResult, NodeCache, NodeId};
        use crate::compute_cache_key::compute_cache_key;
        use crate::deps::DependencyTrace;
        use crate::engine_compute::{ComputeFn, ComputeOutcome, RealizationReadHandle};
        use crate::graph::{
            CancellationHandle, ComputeNodeData, EvaluationGraph, RealizationNodeData,
        };

        fn make_engine() -> Engine {
            Engine::new(Box::new(MockConstraintChecker::new()), None)
        }

        fn make_geometry_handle_value(realization_ref: RealizationNodeId) -> Value {
            Value::GeometryHandle {
                realization_ref,
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(reify_ir::GeometryHandleId(0)),
            }
        }

        fn seed_realization(
            graph: &mut EvaluationGraph,
            id: RealizationNodeId,
            content_hash: ContentHash,
            produced_repr: ReprKind,
        ) {
            graph.realizations.insert(
                id.clone(),
                RealizationNodeData {
                    id,
                    operations: vec![],
                    content_hash,
                    produced_repr,
                    geometry_cell: None,
                    produced_kernel: None,
                },
            );
        }

        // ── step-5: build_compute_realization_inputs lowering rule tests ──────

        #[test]
        fn lowering_extracts_geometry_handle_args_in_order_deduped() {
            let mut engine = make_engine();
            let mut graph = EvaluationGraph::default();

            let r0 = RealizationNodeId::new("E", 0);
            let r1 = RealizationNodeId::new("E", 1);
            let h0 = ContentHash::of_str("mesh-h0");
            let h1 = ContentHash::of_str("brep-h1");
            seed_realization(&mut graph, r0.clone(), h0, ReprKind::Mesh);
            seed_realization(&mut graph, r1.clone(), h1, ReprKind::BRep);

            // Mixed arg_values: leading non-geometry, two distinct geometry args,
            // then R0 REPEATED to verify first-occurrence dedup.
            let arg_values = vec![
                Value::Int(7),
                make_geometry_handle_value(r0.clone()),
                Value::Int(42), // non-geometry interleaved
                make_geometry_handle_value(r1.clone()),
                make_geometry_handle_value(r0.clone()), // duplicate — must be dropped
            ];

            let (inputs, handles, diags) =
                engine.build_compute_realization_inputs(&arg_values, &graph);

            // (a) inputs in arg order, first-occurrence dedup, non-geometry skipped
            assert_eq!(
                inputs,
                vec![r0.clone(), r1.clone()],
                "realization_inputs must preserve arg order and dedup by first occurrence"
            );

            // (b) handles parallel to inputs (2, not 3 — duplicate dropped)
            assert_eq!(
                handles.len(),
                2,
                "handles must be 1:1 with deduplicated inputs"
            );
            assert_eq!(handles[0].node_id, r0, "handles[0] must reference R0");
            assert_eq!(
                handles[0].content_hash, h0,
                "handles[0].content_hash must be H0"
            );
            assert_eq!(handles[1].node_id, r1, "handles[1] must reference R1");
            assert_eq!(
                handles[1].content_hash, h1,
                "handles[1].content_hash must be H1"
            );

            // (c) BRep (handles[1]) emits no diagnostic; Mesh (handles[0]) emits one
            //     warning; duplicate R0 is dropped so not processed twice.
            //     Total diags = 1.
            assert_eq!(
                diags.len(),
                1,
                "one Mesh handle (duplicate dropped) × one warning = 1 total diag"
            );
        }

        #[test]
        fn lowering_all_non_geometry_returns_empty_triple() {
            let mut engine = make_engine();
            let graph = EvaluationGraph::default();

            let arg_values = vec![Value::Int(7), Value::Bool(true), Value::Undef];
            let (inputs, handles, diags) =
                engine.build_compute_realization_inputs(&arg_values, &graph);

            assert!(inputs.is_empty(), "no geometry args → empty inputs");
            assert!(handles.is_empty(), "no geometry args → empty handles");
            assert!(diags.is_empty(), "no geometry args → no diags");
        }

        #[test]
        fn lowering_brep_repr_emits_no_diagnostic() {
            let mut engine = make_engine();
            let mut graph = EvaluationGraph::default();
            let r0 = RealizationNodeId::new("E", 0);
            let h0 = ContentHash::of_str("brep-h");
            seed_realization(&mut graph, r0.clone(), h0, ReprKind::BRep);

            let arg_values = vec![make_geometry_handle_value(r0.clone())];
            let (inputs, handles, diags) =
                engine.build_compute_realization_inputs(&arg_values, &graph);

            assert_eq!(inputs, vec![r0.clone()]);
            assert_eq!(handles.len(), 1);
            assert_eq!(handles[0].content_hash, h0);
            assert!(diags.is_empty(), "BRep repr must emit no diagnostic");
        }

        // ── step-7: end-to-end dispatch data-path + cache key ─────────────────
        //
        // Three user-observable β signals:
        // (a) inserted ComputeNode.realization_inputs == [R0]
        // (b) probe captures realization_inputs[0].content_hash == H0
        // (c) cache key changes when R0.content_hash mutates

        use std::sync::{Mutex, OnceLock};

        /// Probe trampoline: captures realization_inputs.len() and, when
        /// len>0, realization_inputs[0].content_hash into a file-scoped OnceLock.
        static BETA_PROBE_OBSERVED: OnceLock<Mutex<Option<(usize, ContentHash)>>> = OnceLock::new();

        fn beta_probe_fn(
            _value_inputs: &[Value],
            realization_inputs: &[RealizationReadHandle],
            _options: &Value,
            _prior_warm_state: Option<&reify_ir::OpaqueState>,
            _cancellation: &CancellationHandle,
        ) -> ComputeOutcome {
            let capture = if realization_inputs.is_empty() {
                None
            } else {
                Some((realization_inputs.len(), realization_inputs[0].content_hash))
            };
            *BETA_PROBE_OBSERVED
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap() = capture;
            ComputeOutcome::Completed {
                result: Value::Int(0),
                new_warm_state: None,
                cost_per_byte: None,
                diagnostics: vec![],
            }
        }

        fn seed_output_cell(engine: &mut Engine, cell: &ValueCellId) {
            engine.cache_store_mut().put(
                NodeId::Value(cell.clone()),
                NodeCache::new(
                    CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                    Freshness::Final,
                    DependencyTrace::default(),
                    VersionId(1),
                ),
            );
        }

        #[test]
        fn dispatch_e2e_realization_handle_reaches_trampoline_and_cache_key_changes() {
            // Reset the static for cross-test reuse safety.
            if let Some(m) = BETA_PROBE_OBSERVED.get() {
                *m.lock().unwrap() = None;
            }

            let mut engine = make_engine();
            engine.register_compute_fn("test::beta_probe", beta_probe_fn as ComputeFn);

            // Seed graph realization R0 (repr Mesh so it produces a warning
            // in the degradation path; irrelevant to dispatch correctness).
            let mut graph = EvaluationGraph::default();
            let r0 = RealizationNodeId::new("BP", 0);
            let h0 = ContentHash::of_str("beta-h0");
            seed_realization(&mut graph, r0.clone(), h0, ReprKind::Mesh);

            let arg_values = vec![make_geometry_handle_value(r0.clone())];
            let (inputs, handles, diags) =
                engine.build_compute_realization_inputs(&arg_values, &graph);

            // (c) projection-precedes-invoke: the Mesh degradation Warning is
            // present in diags BEFORE dispatch (not after).
            assert_eq!(
                diags.len(),
                1,
                "Mesh repr must emit exactly one degradation warning"
            );

            let cell = ValueCellId::new("BP", "out");
            let c_id = ComputeNodeId::new("BP", 0);
            seed_output_cell(&mut engine, &cell);

            let node = ComputeNodeData {
                computation_id: c_id.clone(),
                target: "test::beta_probe".to_string(),
                value_inputs: vec![],
                realization_inputs: inputs.clone(),
                options_hash: ContentHash(0),
                cache_key: ContentHash(0),
                cached_result: None,
                result_content_hash: None,
                opaque_state: None,
                running: Some(CancellationHandle::new()),
                output_value_cells: vec![cell.clone()],
            };

            // Record the initial cache key (realization_inputs=[R0] with H0
            // feeds into the key).
            let key_before = compute_cache_key(&node, &graph);

            graph.compute_nodes.insert(c_id.clone(), node);

            // (a) Verify graph actually stored realization_inputs=[R0].
            assert_eq!(
                graph.compute_nodes.get(&c_id).unwrap().realization_inputs,
                vec![r0.clone()],
                "(a) graph-stored node.realization_inputs must be [R0]"
            );

            let result = engine.run_compute_dispatch(
                &c_id,
                std::slice::from_ref(&cell),
                "test::beta_probe",
                &arg_values,
                &handles,
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(2),
                ContentHash(0), // inert: no cache dir in tests
            );
            result.expect("dispatch must Ok");

            // (b) probe captured exactly one realization handle with content_hash==H0
            let observed = *BETA_PROBE_OBSERVED
                .get()
                .expect("trampoline must have set the static")
                .lock()
                .unwrap();
            let (obs_len, obs_hash) = observed.expect("probe must have observed Some((len, hash))");
            assert_eq!(
                obs_len, 1,
                "(b) probe must see exactly one realization handle"
            );
            assert_eq!(obs_hash, h0, "(b) probe must see content_hash == H0");

            // (d) CACHE KEY: update R0.content_hash and recompute the key via a
            // freshly-built node with the same realization_inputs.  The key must
            // change because realization_inputs=[R0] carries R0's hash into the
            // sorted-bucket fold.
            let h0_prime = ContentHash::of_str("beta-h0-prime");
            seed_realization(&mut graph, r0.clone(), h0_prime, ReprKind::Mesh);
            // Build a fresh node (same realization_inputs; running=None is fine
            // because compute_cache_key only inspects target + value_inputs +
            // realization_inputs + options_hash).
            let node_for_key = ComputeNodeData {
                computation_id: c_id.clone(),
                target: "test::beta_probe".to_string(),
                value_inputs: vec![],
                realization_inputs: inputs, // owns the Vec — no borrow conflict
                options_hash: ContentHash(0),
                cache_key: ContentHash(0),
                cached_result: None,
                result_content_hash: None,
                opaque_state: None,
                running: None,
                output_value_cells: vec![cell.clone()],
            };
            let key_after = compute_cache_key(&node_for_key, &graph);
            assert_ne!(
                key_before, key_after,
                "(d) cache key must change when R0.content_hash changes"
            );
        }
    }

    // ── θ / task 3427: significance-filter wiring tests ──────────────────────
    //
    // Tests for the dispatch-completion significance-filter integration:
    // step-1: RED test (output VC hash preserved on Equivalent re-dispatch)
    // step-2: impl wires significance_filter → step-1 goes GREEN
    // step-3: RED test (returned value is prior on Equivalent)
    // step-4: impl makes run_compute_dispatch return prior value → step-3 GREEN
    // step-5: RED guard tests (non-Equivalent paths write new value)
    // step-6: finalize conservative no-op paths → step-5 GREEN

    // ── Helpers for building ElasticResult-shaped Values ─────────────────────

    use reify_core::Type;
    use reify_ir::{FieldSourceKind, InterpolationKind, SampledField, SampledGridKind};
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    /// Build a Sampled Field value for use in ElasticResult test fixtures.
    fn make_sampled_field_ec(name: &str, data: &[f64]) -> Value {
        Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::SampledField(SampledField {
                name: name.to_string(),
                kind: SampledGridKind::Regular1D,
                bounds_min: vec![0.0],
                bounds_max: vec![1.0],
                spacing: vec![0.5],
                axis_grids: vec![(0..data.len()).map(|i| i as f64).collect()],
                interpolation: InterpolationKind::Linear,
                data: data.to_vec(),
                oob_emitted: AtomicBool::new(false),
            })),
        }
    }

    /// Build an ElasticResult-shaped `Value::Map` for significance-filter tests.
    ///
    /// Matches the stdlib fea.rs output shape:
    /// - `"displacement"` → `Value::Field { source: Sampled, lambda: Value::SampledField }`
    /// - `"stress"` → `Value::Field { source: Sampled, lambda: Value::SampledField }`
    /// - `"max_von_mises"` → `Value::Real`
    /// - `"converged"` → `Value::Bool`
    /// - `"iterations"` → `Value::Int`
    fn make_elastic_result_ec(
        displacement_data: &[f64],
        stress_data: &[f64],
        max_vm: f64,
        converged: bool,
        iters: u32,
    ) -> Value {
        let mut map = BTreeMap::new();
        map.insert(
            Value::String("displacement".to_string()),
            make_sampled_field_ec("displacement", displacement_data),
        );
        map.insert(
            Value::String("stress".to_string()),
            make_sampled_field_ec("stress", stress_data),
        );
        map.insert(
            Value::String("max_von_mises".to_string()),
            Value::Real(max_vm),
        );
        map.insert(
            Value::String("converged".to_string()),
            Value::Bool(converged),
        );
        map.insert(
            Value::String("iterations".to_string()),
            Value::Int(iters as i64),
        );
        Value::Map(map)
    }

    // ── Step-1 trampoline: returns displacement perturbed +1e-12 (sub-tol) ──

    /// Trampoline returning an ElasticResult whose `displacement` samples are
    /// each shifted by +1e-12 m relative to the seeded prior.  All other fields
    /// (`stress`, `max_von_mises`, `converged`, `iterations`) are bit-identical
    /// to the prior.  With tolerance = 1e-6 m, the delta (1e-12 m) is
    /// sub-tolerance → `significance_filter` returns Equivalent.
    fn elastic_static_sub_tol_fn_s1(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        ComputeOutcome::Completed {
            result: make_elastic_result_ec(
                &[0.0_f64 + 1e-12, 0.001_f64 + 1e-12], // sub-tol displacement
                &[0.0_f64, 0.001_f64],                   // stress: bit-identical
                1e8_f64,                                 // max_von_mises: bit-identical
                true,                                    // converged: bit-identical
                5,                                       // iterations: bit-identical
            ),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    /// Step-1 (RED before step-2 impl): a sub-tolerance re-dispatch for an
    /// opted-in target with an active tolerance must preserve the output VC's
    /// content hash, so downstream consumers see an Unchanged input and are
    /// NOT recomputed.
    ///
    /// Currently FAILS because `run_compute_dispatch` writes the new
    /// bit-different value, changing the hash.  GREEN after step-2 wires
    /// `significance_filter` into the Completed arm.
    #[test]
    fn run_compute_dispatch_equivalent_redispatch_preserves_output_vc_hash() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn(
            "solver::elastic_static",
            elastic_static_sub_tol_fn_s1 as ComputeFn,
        );

        let entity = "EntityStep1";
        let cell = ValueCellId::new(entity, "result");
        let c_id = ComputeNodeId::new(entity, 0);

        // Build the prior ElasticResult (displacement = [0.0, 0.001]).
        let prior_result = CachedResult::Value(
            make_elastic_result_ec(
                &[0.0_f64, 0.001_f64],
                &[0.0_f64, 0.001_f64],
                1e8_f64,
                true,
                5,
            ),
            DeterminacyState::Determined,
        );
        let prior_hash = prior_result.content_hash();

        // Seed the output VC as Final with the prior value.
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                prior_result,
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Seed active_tolerance_scope so significance_filter receives Some(1e-6).
        engine
            .active_tolerance_scope
            .insert(entity.to_string(), 1e-6_f64);

        // Dispatch: trampoline returns displacement perturbed +1e-12 (sub-tol).
        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "solver::elastic_static",
            &[], // value_inputs: not used by this trampoline
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
            ContentHash(0), // inert: no cache dir in tests
        );
        result.expect("Completed dispatch must succeed");

        // Assert: output VC result_hash must equal the PRIOR hash (prior value
        // retained → record_evaluation_with_freshness Unchanged → downstream
        // cache-hits NOT recomputed).
        //
        // RED today: the new bit-different value is written and hash changes.
        // GREEN after step-2 wires significance_filter into the Completed arm.
        let after_hash = engine
            .cache_store()
            .get(&NodeId::Value(cell))
            .expect("output VC must exist after dispatch")
            .result_hash;
        assert_eq!(
            after_hash,
            prior_hash,
            "sub-tolerance re-dispatch (Equivalent) must preserve the output \
             VC content hash so downstream inputs are bit-identical (Unchanged)",
        );
    }

    /// Step-3 (RED before step-4 impl): on an Equivalent re-dispatch,
    /// `run_compute_dispatch` must return the PRIOR value so the engine_eval
    /// `values` map stays consistent with the cache entry (both hold the prior).
    ///
    /// Currently FAILS: step-2 writes the prior into the cache but still returns
    /// `result` (the new bit-different value). GREEN after step-4 changes
    /// `Ok((result, diagnostics))` to `Ok((effective_value, diagnostics))`.
    #[test]
    fn run_compute_dispatch_equivalent_redispatch_returns_prior_value() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn(
            "solver::elastic_static",
            elastic_static_sub_tol_fn_s1 as ComputeFn,
        );

        let entity = "EntityStep3";
        let cell = ValueCellId::new(entity, "result");
        let c_id = ComputeNodeId::new(entity, 0);

        let prior_value = make_elastic_result_ec(
            &[0.0_f64, 0.001_f64],
            &[0.0_f64, 0.001_f64],
            1e8_f64,
            true,
            5,
        );
        let prior_cache_result =
            CachedResult::Value(prior_value.clone(), DeterminacyState::Determined);

        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                prior_cache_result,
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        engine
            .active_tolerance_scope
            .insert(entity.to_string(), 1e-6_f64);

        let (returned_value, _diags) = engine
            .run_compute_dispatch(
                &c_id,
                std::slice::from_ref(&cell),
                "solver::elastic_static",
                &[],
                &[],
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(2),
                ContentHash(0), // inert: no cache dir in tests
            )
            .expect("Completed dispatch must succeed");

        // Assert: returned value must bit-equal the PRIOR (not the new result).
        // RED today: step-2 still returns `result` (the new perturbed value).
        // GREEN after step-4 changes the return to `effective_value`.
        assert_eq!(
            returned_value,
            prior_value,
            "Equivalent re-dispatch must return the prior value so the \
             engine_eval values map is consistent with the cache",
        );
    }

    // ── Step-5: guard tests — non-Equivalent paths write new value ────────────
    //
    // Three scenarios that must write/return the NEW result (no suppression):
    // (a) opted-in target + active tolerance + beyond-tol displacement delta
    // (b) non-opted-in target ("test::identity") — filter returns NotOptedIn
    // (c) opted-in target + NO active tolerance — active_tolerance_for None

    // Trampoline (a): opted-in target, displacement delta 1.0 m >> tol 1e-6 m.
    fn elastic_static_over_tol_fn_s5(
        _value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        ComputeOutcome::Completed {
            result: make_elastic_result_ec(
                &[0.0_f64 + 1.0, 0.001_f64 + 1.0], // 1.0 m >> tol 1e-6
                &[0.0_f64, 0.001_f64],
                1e8_f64,
                true,
                5,
            ),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    /// Step-5 guards: only Equivalent suppresses. Pins that over-tolerance,
    /// non-opted-in, and no-tolerance paths all write/return the new result.
    #[test]
    fn run_compute_dispatch_non_equivalent_paths_write_new_value() {
        // ── (a) opted-in + active tolerance + beyond-tol delta → new value ───
        {
            let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
            engine.register_compute_fn(
                "solver::elastic_static",
                elastic_static_over_tol_fn_s5 as ComputeFn,
            );

            let entity = "Step5EntityA";
            let cell = ValueCellId::new(entity, "result");
            let c_id = ComputeNodeId::new(entity, 0);

            let prior_value = make_elastic_result_ec(
                &[0.0_f64, 0.001_f64],
                &[0.0_f64, 0.001_f64],
                1e8_f64,
                true,
                5,
            );
            let prior_result =
                CachedResult::Value(prior_value.clone(), DeterminacyState::Determined);
            let prior_hash = prior_result.content_hash();

            engine.cache_store_mut().put(
                NodeId::Value(cell.clone()),
                NodeCache::new(
                    prior_result,
                    Freshness::Final,
                    DependencyTrace::default(),
                    VersionId(1),
                ),
            );
            engine
                .active_tolerance_scope
                .insert(entity.to_string(), 1e-6_f64);

            let (returned, _) = engine
                .run_compute_dispatch(
                    &c_id,
                    std::slice::from_ref(&cell),
                    "solver::elastic_static",
                    &[],
                    &[],
                    &Value::Undef,
                    &CancellationHandle::new(),
                    VersionId(2),
                    ContentHash(0), // inert: no cache dir in tests
                )
                .expect("dispatch must succeed");

            // (a) Over-tolerance → new result written and returned.
            let after_hash = engine
                .cache_store()
                .get(&NodeId::Value(cell))
                .unwrap()
                .result_hash;
            assert_ne!(
                after_hash, prior_hash,
                "(a) over-tolerance must change the VC hash (new value written)"
            );
            assert_ne!(
                returned, prior_value,
                "(a) over-tolerance must return the new result, not the prior"
            );
        }

        // ── (b) non-opted-in target → new value (NotOptedIn) ─────────────────
        {
            let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
            // "test::identity" is not in the significance-filter opt-in allowlist.
            engine.register_compute_fn("test::identity_s5b", identity_fn as ComputeFn);

            let entity = "Step5EntityB";
            let cell = ValueCellId::new(entity, "result");
            let c_id = ComputeNodeId::new(entity, 0);

            let prior_value = Value::Int(42);
            let prior_result =
                CachedResult::Value(prior_value.clone(), DeterminacyState::Determined);
            let prior_hash = prior_result.content_hash();

            engine.cache_store_mut().put(
                NodeId::Value(cell.clone()),
                NodeCache::new(
                    prior_result,
                    Freshness::Final,
                    DependencyTrace::default(),
                    VersionId(1),
                ),
            );
            engine
                .active_tolerance_scope
                .insert(entity.to_string(), 1e-6_f64);

            // Trampoline returns 99 (different from prior 42).
            let (returned, _) = engine
                .run_compute_dispatch(
                    &c_id,
                    std::slice::from_ref(&cell),
                    "test::identity_s5b",
                    &[Value::Int(99)],
                    &[],
                    &Value::Undef,
                    &CancellationHandle::new(),
                    VersionId(2),
                    ContentHash(0), // inert: no cache dir in tests
                )
                .expect("dispatch must succeed");

            let after_hash = engine
                .cache_store()
                .get(&NodeId::Value(cell))
                .unwrap()
                .result_hash;
            assert_ne!(
                after_hash, prior_hash,
                "(b) NotOptedIn target must change the VC hash (new value written)"
            );
            assert_eq!(
                returned,
                Value::Int(99),
                "(b) NotOptedIn target must return the new result"
            );
        }

        // ── (c) opted-in + NO active tolerance → new value (Different) ───────
        {
            let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
            engine.register_compute_fn(
                "solver::elastic_static",
                elastic_static_sub_tol_fn_s1 as ComputeFn,
            );

            let entity = "Step5EntityC";
            let cell = ValueCellId::new(entity, "result");
            let c_id = ComputeNodeId::new(entity, 0);

            let prior_value = make_elastic_result_ec(
                &[0.0_f64, 0.001_f64],
                &[0.0_f64, 0.001_f64],
                1e8_f64,
                true,
                5,
            );
            let prior_result =
                CachedResult::Value(prior_value.clone(), DeterminacyState::Determined);
            let prior_hash = prior_result.content_hash();

            engine.cache_store_mut().put(
                NodeId::Value(cell.clone()),
                NodeCache::new(
                    prior_result,
                    Freshness::Final,
                    DependencyTrace::default(),
                    VersionId(1),
                ),
            );
            // active_tolerance_scope is NOT seeded → active_tolerance_for returns None.

            let (returned, _) = engine
                .run_compute_dispatch(
                    &c_id,
                    std::slice::from_ref(&cell),
                    "solver::elastic_static",
                    &[],
                    &[],
                    &Value::Undef,
                    &CancellationHandle::new(),
                    VersionId(2),
                    ContentHash(0), // inert: no cache dir in tests
                )
                .expect("dispatch must succeed");

            let after_hash = engine
                .cache_store()
                .get(&NodeId::Value(cell))
                .unwrap()
                .result_hash;
            assert_ne!(
                after_hash, prior_hash,
                "(c) no active tolerance (None) must change the VC hash (new value written)"
            );
            assert_ne!(
                returned, prior_value,
                "(c) no active tolerance must return the new result, not the prior"
            );
        }
    }

    // ── Step-7: downstream observable — Equivalent re-dispatch does NOT ───────
    //            recompute/invalidate a consumer; beyond-tol DOES. ────────────
    //
    // The plan permits an in-crate test seeding `active_tolerance_scope` over a
    // hand-built output-VC → consumer topology as an acceptable equivalent to a
    // full through-engine `.ri` e2e test (preferred — far less thrash than
    // compiling a `.ri` module).
    //
    // `run_compute_dispatch` itself does NOT emit journal events or invalidate
    // dependents — it writes the output VC via `complete_compute_dispatch_
    // atomically`. In production the downstream invalidation happens one layer
    // up: the consumer re-evaluates and routes through
    // `record_evaluation_with_freshness`, which early-cuts (EvalOutcome::
    // Unchanged) when the value it derives from the output VC is bit-identical,
    // and reports EvalOutcome::Changed otherwise. That same-hash early-cutoff IS
    // the suppression signal a downstream consumer observes.
    //
    // This test models exactly that consumer: it builds a consumer VC whose
    // result is derived 1-to-1 from the output VC's value, seeds it Final with
    // the prior output value, runs the dispatch, then re-evaluates the consumer
    // off the POST-dispatch output VC value via `record_evaluation_with_
    // freshness`. The returned EvalOutcome is the downstream observable:
    //   - Equivalent (sub-tol) re-dispatch → output VC preserved → Unchanged
    //     (consumer NOT recomputed / NOT invalidated).
    //   - Beyond-tol re-dispatch          → output VC changed   → Changed
    //     (consumer IS recomputed / invalidated).
    //
    // Helper: run the dispatch for a given trampoline, then return the
    // (downstream EvalOutcome, output-VC-hash-preserved?) pair.
    fn dispatch_then_consumer_outcome(
        compute_fn: ComputeFn,
        entity: &str,
    ) -> (crate::cache::EvalOutcome, bool) {
        use crate::cache::EvalOutcome;

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn("solver::elastic_static", compute_fn);

        let out_cell = ValueCellId::new(entity, "result");
        let consumer_cell = ValueCellId::new(entity, "downstream");
        let c_id = ComputeNodeId::new(entity, 0);

        // Prior output value (the seeded "best on display").
        let prior_output = make_elastic_result_ec(
            &[0.0_f64, 0.001_f64],
            &[0.0_f64, 0.001_f64],
            1e8_f64,
            true,
            5,
        );
        let prior_output_result =
            CachedResult::Value(prior_output.clone(), DeterminacyState::Determined);
        let prior_output_hash = prior_output_result.content_hash();

        // Seed the OUTPUT VC Final with the prior value.
        engine.cache_store_mut().put(
            NodeId::Value(out_cell.clone()),
            NodeCache::new(
                prior_output_result,
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Seed the CONSUMER VC Final, with a result derived 1-to-1 from the
        // prior output value and a DependencyTrace that reads the output VC.
        // (The downstream signal is the consumer's re-eval EvalOutcome, which
        // hashes whatever value it derives from the output VC.)
        let mut consumer_trace = DependencyTrace::default();
        consumer_trace.reads.push(out_cell.clone());
        engine.cache_store_mut().put(
            NodeId::Value(consumer_cell.clone()),
            NodeCache::new(
                CachedResult::Value(prior_output.clone(), DeterminacyState::Determined),
                Freshness::Final,
                consumer_trace.clone(),
                VersionId(1),
            ),
        );

        // Activate the tolerance purpose for the output VC's entity.
        engine
            .active_tolerance_scope
            .insert(entity.to_string(), 1e-6_f64);

        // Dispatch.
        engine
            .run_compute_dispatch(
                &c_id,
                std::slice::from_ref(&out_cell),
                "solver::elastic_static",
                &[],
                &[],
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(2),
                ContentHash(0), // inert: no cache dir in tests
            )
            .expect("Completed dispatch must succeed");

        // The post-dispatch output VC value — this is exactly what a downstream
        // consumer would read as its input.
        let post_output = match &engine
            .cache_store()
            .get(&NodeId::Value(out_cell.clone()))
            .expect("output VC must exist after dispatch")
            .result
        {
            CachedResult::Value(v, _) => v.clone(),
            other => panic!("output VC must hold a Value, got {other:?}"),
        };
        let post_output_hash = engine
            .cache_store()
            .get(&NodeId::Value(out_cell))
            .unwrap()
            .result_hash;
        let output_vc_preserved = post_output_hash == prior_output_hash;

        // Re-evaluate the consumer off the post-dispatch output value. This is
        // the production downstream re-eval path: `record_evaluation_with_
        // freshness` early-cuts (Unchanged) iff the derived value is hash-
        // identical to the consumer's prior cached result.
        let downstream_outcome: EvalOutcome = engine.cache_store_mut().record_evaluation_with_freshness(
            NodeId::Value(consumer_cell),
            CachedResult::Value(post_output, DeterminacyState::Determined),
            VersionId(3),
            consumer_trace,
            Freshness::Final,
        );

        (downstream_outcome, output_vc_preserved)
    }

    /// Step-7: pin the DOWNSTREAM observable of the significance filter.
    ///
    /// Sub-tolerance-equivalent re-dispatch of an opted-in target with an active
    /// tolerance leaves a downstream consumer UNCHANGED (not recomputed); a
    /// beyond-tolerance perturbation flips the consumer to CHANGED.
    #[test]
    fn run_compute_dispatch_equivalent_redispatch_does_not_recompute_downstream_consumer() {
        use crate::cache::EvalOutcome;

        // Equivalent (sub-tol +1e-12, tol 1e-6): output VC preserved → the
        // downstream consumer early-cuts (Unchanged) → NOT recomputed.
        let (eq_outcome, eq_output_preserved) = dispatch_then_consumer_outcome(
            elastic_static_sub_tol_fn_s1 as ComputeFn,
            "Step7EntityEquivalent",
        );
        assert!(
            eq_output_preserved,
            "precondition: sub-tolerance re-dispatch must preserve the output VC hash",
        );
        assert_eq!(
            eq_outcome,
            EvalOutcome::Unchanged,
            "Equivalent (sub-tolerance) re-dispatch must leave the downstream \
             consumer Unchanged (early cutoff) — NOT recomputed or invalidated",
        );

        // Beyond-tol (+1.0 m, tol 1e-6): output VC changes → the downstream
        // consumer re-derives a new value → Changed (recomputed/invalidated).
        let (over_outcome, over_output_preserved) = dispatch_then_consumer_outcome(
            elastic_static_over_tol_fn_s5 as ComputeFn,
            "Step7EntityBeyond",
        );
        assert!(
            !over_output_preserved,
            "precondition: beyond-tolerance re-dispatch must change the output VC hash",
        );
        assert_eq!(
            over_outcome,
            EvalOutcome::Changed,
            "Beyond-tolerance re-dispatch must change the downstream consumer's \
             input → consumer is recomputed/invalidated (EvalOutcome::Changed)",
        );
    }

    // ── Amendment 2 (reviewer): pin determinacy semantics of the Equivalent arm ─
    //
    // `complete_compute_dispatch_atomically` always writes
    // `DeterminacyState::Determined` for output VCs (hardcoded at cache.rs:1123),
    // regardless of the prior entry's DeterminacyState. The Equivalent arm
    // therefore adopts `Determined` — the new dispatch's determinacy
    // (warm-state-advances semantics). This test pins that contract so a future
    // change that threads DeterminacyState through the dispatch path cannot
    // silently produce a mismatched (prior value, new determinacy) entry.

    /// Pin that the Equivalent arm produces a cache entry with
    /// `DeterminacyState::Determined`, and that the returned value bit-equals
    /// the prior value.
    ///
    /// Scenario: prior entry is `Determined` (the normal post-dispatch state).
    /// Sub-tolerance re-dispatch → Equivalent → prior value retained →
    /// cache entry has `Determined`. The content hash is fully preserved
    /// (same value + same determinacy), so downstream stays Unchanged.
    #[test]
    fn run_compute_dispatch_equivalent_cache_entry_determinacy_is_determined() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_compute_fn(
            "solver::elastic_static",
            elastic_static_sub_tol_fn_s1 as ComputeFn,
        );

        let entity = "EntityDeterminacyAmend2";
        let cell = ValueCellId::new(entity, "result");
        let c_id = ComputeNodeId::new(entity, 0);

        let prior_value = make_elastic_result_ec(
            &[0.0_f64, 0.001_f64],
            &[0.0_f64, 0.001_f64],
            1e8_f64,
            true,
            5,
        );

        // Seed the prior as Determined (the normal state after any prior dispatch
        // completion — complete_compute_dispatch_atomically always writes
        // Determined).
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(prior_value.clone(), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        engine
            .active_tolerance_scope
            .insert(entity.to_string(), 1e-6_f64);

        let (returned_value, _diags) = engine
            .run_compute_dispatch(
                &c_id,
                std::slice::from_ref(&cell),
                "solver::elastic_static",
                &[],
                &[],
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(2),
                ContentHash(0), // inert: no cache dir in tests
            )
            .expect("Completed dispatch must succeed");

        // The returned value must be the prior (Equivalent arm).
        assert_eq!(
            returned_value, prior_value,
            "Equivalent arm must return the prior value",
        );

        // The cache entry must have DeterminacyState::Determined.
        // `complete_compute_dispatch_atomically` hardcodes Determined (cache.rs:1123),
        // so the Equivalent arm adopts the new dispatch's determinacy (Determined),
        // NOT the prior entry's determinacy. Since prior was also Determined, the
        // content hash is fully preserved — no spurious downstream invalidation.
        let after_entry = engine
            .cache_store()
            .get(&NodeId::Value(cell))
            .expect("output VC must exist after dispatch");
        match &after_entry.result {
            CachedResult::Value(v, det) => {
                assert_eq!(
                    *det,
                    DeterminacyState::Determined,
                    "Equivalent arm: DeterminacyState must be Determined \
                     (warm-state-advances semantics — complete_compute_dispatch_atomically \
                     always writes Determined)",
                );
                assert_eq!(
                    v, &prior_value,
                    "Equivalent arm: cached value must be the prior value",
                );
            }
            other => panic!("expected CachedResult::Value, got {other:?}"),
        }
    }
}
