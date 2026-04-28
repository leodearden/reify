use std::collections::HashMap;

use reify_types::{
    CompiledExpr, ConstraintNodeId, ContentHash, DeterminacyState, Freshness, GeometryHandleId,
    OpaqueState, RealizationNodeId, ResolutionNodeId, ResultRef, Satisfaction, Value, ValueCellId,
    ValueMap, VersionId,
};

use crate::deps::DependencyTrace;

/// Unified identifier for any node in the evaluation graph.
/// Used as the key in the cache store.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeId {
    Value(ValueCellId),
    Constraint(ConstraintNodeId),
    Realization(RealizationNodeId),
    Resolution(ResolutionNodeId),
}

impl From<ValueCellId> for NodeId {
    fn from(id: ValueCellId) -> Self {
        NodeId::Value(id)
    }
}

impl From<ConstraintNodeId> for NodeId {
    fn from(id: ConstraintNodeId) -> Self {
        NodeId::Constraint(id)
    }
}

impl From<RealizationNodeId> for NodeId {
    fn from(id: RealizationNodeId) -> Self {
        NodeId::Realization(id)
    }
}

impl From<ResolutionNodeId> for NodeId {
    fn from(id: ResolutionNodeId) -> Self {
        NodeId::Resolution(id)
    }
}

/// Stores different kinds of evaluation results in the cache.
#[derive(Clone, Debug)]
pub enum CachedResult {
    /// A value cell result with its determinacy state.
    Value(Value, DeterminacyState),
    /// A constraint satisfaction result.
    Satisfaction(Satisfaction),
    /// A geometry handle result (proxy for the actual shape).
    GeometryHandle(GeometryHandleId),
    /// A resolution result: resolved auto parameter values.
    Resolution(HashMap<ValueCellId, Value>),
}

impl CachedResult {
    /// Compute a content hash for early cutoff comparison.
    /// Domain-separated with tag bytes [20], [21], [22] per variant.
    pub fn content_hash(&self) -> ContentHash {
        match self {
            CachedResult::Value(val, det) => {
                let tag = ContentHash::of(&[20]);
                let val_hash = val.content_hash();
                let det_hash = ContentHash::of(&[*det as u8]);
                tag.combine(val_hash).combine(det_hash)
            }
            CachedResult::Satisfaction(sat) => {
                let tag = ContentHash::of(&[21]);
                tag.combine(sat.content_hash())
            }
            CachedResult::GeometryHandle(handle_id) => {
                let tag = ContentHash::of(&[22]);
                tag.combine(handle_id.content_hash())
            }
            CachedResult::Resolution(values) => {
                let tag = ContentHash::of(&[23]);
                // Sort entries by ValueCellId Debug repr for deterministic hashing
                let mut entries: Vec<_> = values.iter().collect();
                entries.sort_by_key(|(k, _)| format!("{:?}", k));
                let combined = ContentHash::combine_all(entries.iter().map(|(k, v)| {
                    let key_hash = ContentHash::of_str(&format!("{:?}", k));
                    let val_hash = v.content_hash();
                    key_hash.combine(val_hash)
                }));
                tag.combine(combined)
            }
        }
    }
}

/// Documented placeholder handle ID used by
/// [`Engine::mark_realization_failed`] when a kernel error fires before any
/// successful handle has been allocated for a realization (cold-start path).
///
/// The const exists because the cold-start fallback inserts a `NodeCache`
/// gated by `Freshness::Failed { error }`, but `NodeCache::new` always
/// content-hashes its `result`, so the result must carry *some* concrete
/// value. We deliberately avoid:
///
/// - `GeometryHandleId(0)` — `0` is plausibly a real handle ID in
///   kernels that start their counters at zero (and the project has
///   already had stale-zero export bugs), so consumers that
///   accidentally bypass the freshness gate could conflate this stub
///   with a legitimate first-allocated handle.
/// - `GeometryHandleId::INVALID` (`u64::MAX`) — `GeometryHandleId::content_hash`
///   debug-asserts on the INVALID sentinel, so it cannot be embedded in a
///   `NodeCache::new(...)` result.
///
/// `u64::MAX - 1` sits adjacent to `INVALID` in the same "absurdly high,
/// not-allocated" tail region: kernels in this project allocate from low
/// counters (1, 2, 3, …), so a real allocated handle reaching this value
/// would require ~2^64 sequential allocations. Consumers MUST still gate
/// on `Freshness::Failed` before reading the stored handle — this const
/// is a defence-in-depth, not an escape hatch.
///
/// Pinned by `tests/failed_propagation.rs::failed_realization_stub_handle_is_distinct_from_zero_and_invalid`.
pub const FAILED_REALIZATION_STUB_HANDLE: GeometryHandleId = GeometryHandleId(u64::MAX - 1);

/// Signal indicating whether a node's result changed after re-evaluation.
/// Used to control dirty propagation (early cutoff).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalOutcome {
    /// The result changed compared to the previous cached value.
    Changed,
    /// The result is the same as the previous cached value (early cutoff).
    Unchanged,
}

/// Per-node cache entry storing the evaluation result and metadata.
#[derive(Debug)]
pub struct NodeCache {
    /// The cached evaluation result.
    pub result: CachedResult,
    /// Content hash of the result, for early cutoff comparison.
    pub result_hash: ContentHash,
    /// Freshness of the cached value.
    pub freshness: Freshness,
    /// Statically extracted value cell dependencies for this node.
    pub dependency_trace: DependencyTrace,
    /// The version at which this cache entry was last validated.
    pub basis_version: VersionId,
    /// Optional warm-start state for the evaluator (type-erased).
    /// Transient: not preserved across clones (warm state is an optimization hint).
    pub warm_state: Option<OpaqueState>,
    /// Diagnostic-chain side-table: when `freshness == Pending`, this carries
    /// the upstream `NodeId` that caused this node to be Pending (a Failed
    /// leaf, or another Pending node forwarding its own cause). `None` when
    /// the entry is not Pending or the chain root has not been recorded.
    ///
    /// This lives outside `Freshness::Pending` so the four-variant lifecycle
    /// tag stays untouched (see arch §7.1, plan task #2330 design decision).
    /// Failed entries do NOT set this field — a Failed node is itself the
    /// chain root, not a forwarder.
    pub pending_cause: Option<NodeId>,
}

impl Clone for NodeCache {
    fn clone(&self) -> Self {
        Self {
            result: self.result.clone(),
            result_hash: self.result_hash,
            freshness: self.freshness.clone(),
            dependency_trace: self.dependency_trace.clone(),
            basis_version: self.basis_version,
            warm_state: None, // warm state is transient, not preserved across clones
            pending_cause: self.pending_cause.clone(),
        }
    }
}

impl NodeCache {
    /// Create a new cache entry, automatically computing the result hash.
    ///
    /// `pending_cause` defaults to `None`; the diagnostic chain is populated
    /// by [`CacheStore::mark_pending_with_cause`] at the wire site, not by
    /// this constructor.
    pub fn new(
        result: CachedResult,
        freshness: Freshness,
        dependency_trace: DependencyTrace,
        basis_version: VersionId,
    ) -> Self {
        let result_hash = result.content_hash();
        Self {
            result,
            result_hash,
            freshness,
            dependency_trace,
            basis_version,
            warm_state: None,
            pending_cause: None,
        }
    }
}

/// Store managing per-node cache entries for incremental evaluation.
pub struct CacheStore {
    caches: HashMap<NodeId, NodeCache>,
    /// Nodes that need re-evaluation, with the set of changed ValueCellIds
    /// that caused them to be dirty. A node stays dirty until ALL its dirty
    /// reasons have been resolved (e.g., by early cutoffs on upstream nodes).
    dirty_reasons: HashMap<NodeId, std::collections::HashSet<ValueCellId>>,
    /// Count of successful mark_pending() calls since last reset.
    /// Used to verify that Pending intermediate state is actually applied
    /// during edit_param() evaluation.
    pending_transition_count: usize,
    /// Current version for the cache store. Incremented on each full eval.
    /// Used for fast-path checking: if entry.basis_version == store.version,
    /// the entry is fresh and doesn't need re-evaluation.
    version: VersionId,
}

impl CacheStore {
    /// Create an empty cache store.
    pub fn new() -> Self {
        Self {
            caches: HashMap::new(),
            dirty_reasons: HashMap::new(),
            pending_transition_count: 0,
            version: VersionId(0),
        }
    }

    /// Get the current version of the cache store.
    pub fn version(&self) -> VersionId {
        self.version
    }

    /// Check if a node is fresh (has valid cached result for current version).
    /// Returns true if the entry exists and its basis_version matches the store version.
    pub fn is_fresh(&self, id: &ValueCellId) -> bool {
        let node = NodeId::Value(id.clone());
        if let Some(entry) = self.caches.get(&node) {
            entry.basis_version == self.version
        } else {
            false
        }
    }

    /// Bump the version and return the new version.
    /// Called before incremental evaluation to invalidate stale entries.
    pub fn bump_version(&mut self) -> VersionId {
        self.version = VersionId(self.version.0 + 1);
        self.version
    }

    /// Look up a cached entry by node id.
    pub fn get(&self, node: &NodeId) -> Option<&NodeCache> {
        self.caches.get(node)
    }

    /// Store or overwrite a cache entry.
    pub fn put(&mut self, node: NodeId, cache: NodeCache) {
        self.caches.insert(node, cache);
    }

    /// Remove a cached entry and its dirty state.
    pub fn invalidate(&mut self, node: &NodeId) {
        self.caches.remove(node);
        self.dirty_reasons.remove(node);
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.caches.len()
    }

    /// Whether the store has no entries.
    pub fn is_empty(&self) -> bool {
        self.caches.is_empty()
    }

    /// Remove all cached entries and dirty state.
    pub fn clear(&mut self) {
        self.caches.clear();
        self.dirty_reasons.clear();
    }

    /// Record an evaluation result and determine if it changed (early cutoff).
    ///
    /// Thin wrapper around [`CacheStore::record_evaluation_with_freshness`] that always
    /// writes `Freshness::Final`. All existing call sites outside `evaluate_let_bindings`
    /// (incremental re-eval, engine_edit.rs, concurrent.rs, warm-state seeding) rely on
    /// Final being the implicit freshness — this wrapper preserves byte-identical behaviour
    /// for those sites. See arch §7.2 and task #2328 for the propagation design.
    ///
    /// Compares the new result's content hash with the existing cache entry.
    /// - If same hash: updates basis_version only (early cutoff), returns Unchanged.
    /// - If different or no prior cache: updates full cache entry, returns Changed.
    pub fn record_evaluation(
        &mut self,
        node: NodeId,
        new_result: CachedResult,
        version: VersionId,
        trace: DependencyTrace,
    ) -> EvalOutcome {
        self.record_evaluation_with_freshness(node, new_result, version, trace, Freshness::Final)
    }

    /// Record an evaluation result with a caller-supplied freshness.
    ///
    /// Implements arch §7.2 propagation at the write layer: the wire site in
    /// `evaluate_let_bindings` calls [`CacheStore::record_evaluation_propagating_freshness`]
    /// (which derives freshness and delegates here); all other call sites use
    /// [`CacheStore::record_evaluation`] which hard-codes `Freshness::Final`.
    ///
    /// Compares the new result's content hash with the existing cache entry.
    /// - If same hash: updates `basis_version`, `dependency_trace`, and `freshness`
    ///   (early cutoff), returns `Unchanged`.
    /// - If different or no prior cache: writes full cache entry with supplied `freshness`,
    ///   returns `Changed`.
    ///
    /// **Early-cutoff trace overwrite:** the `dependency_trace = trace` assignment on the
    /// early-cutoff path was already present in `record_evaluation` before task #2328
    /// (verified by reviewing the pre-2328 git history).  It is intentional: even when the
    /// result hash is unchanged, the freshly-computed trace replaces the old one so the
    /// cache stays consistent with the current expression structure.  All existing callers
    /// via [`CacheStore::record_evaluation`] (engine_edit.rs, unfold.rs, concurrent.rs) have
    /// always observed this behaviour and can rely on it.
    ///
    /// Hash-only cutoff is intentional. NaN payloads are canonicalized by
    /// `Value::content_hash` (every NaN bit-pattern collapses to `f64::NAN.to_bits()`),
    /// so two results differing only in NaN payload are treated as Unchanged here.
    /// See `Value::content_hash` doc — 'Known intentional exception — incremental
    /// cache' — in `crates/reify-types/src/value.rs`.
    pub fn record_evaluation_with_freshness(
        &mut self,
        node: NodeId,
        new_result: CachedResult,
        version: VersionId,
        trace: DependencyTrace,
        freshness: Freshness,
    ) -> EvalOutcome {
        let new_hash = new_result.content_hash();

        if let Some(existing) = self.caches.get_mut(&node)
            && existing.result_hash == new_hash
        {
            // Early cutoff: result unchanged, just update version, trace, and freshness.
            // Note: any prior `Pending { last_substantive }` state is discarded here —
            // re-evaluation completed, so Pending is no longer meaningful. Callers that
            // need to preserve Pending state must handle it themselves before calling.
            existing.basis_version = version;
            existing.dependency_trace = trace;
            existing.freshness = freshness;
            existing.warm_state = None; // old warm state is stale after re-evaluation
            return EvalOutcome::Unchanged;
        }

        // Changed or cold start: store full entry (clear any old warm state)
        self.caches.insert(
            node,
            NodeCache {
                result: new_result,
                result_hash: new_hash,
                freshness,
                dependency_trace: trace,
                basis_version: version,
                warm_state: None,
                pending_cause: None,
            },
        );
        EvalOutcome::Changed
    }

    /// Record an evaluation result, deriving freshness from the supplied trace per arch §7.2.
    ///
    /// Convenience combinator: derives the output freshness by calling
    /// [`CacheStore::derive_output_freshness_from_trace`] on the **supplied** `trace`
    /// (so freshness is keyed off the just-computed reads, not a stale cached trace),
    /// then writes the entry via [`CacheStore::record_evaluation_with_freshness`].
    ///
    /// This is the single-call ergonomic entry point for the `evaluate_let_bindings`
    /// wire site and any future caller that evaluates a node with a fresh trace and
    /// wants arch §7.2 propagation atomically.
    ///
    /// The `generation` used for `Freshness::Intermediate` is derived from `version.0`
    /// (arch §7.1: `VersionId` is the single source of truth for the monotonic generation
    /// counter; both `version` and `generation` are the same underlying `u64`).
    ///
    /// Compares the new result's content hash with the existing cache entry.
    /// - If same hash: updates basis_version, dependency_trace, and freshness (early cutoff),
    ///   returns `Unchanged`.
    /// - If different or no prior cache: writes full cache entry with derived `freshness`,
    ///   returns `Changed`.
    pub fn record_evaluation_propagating_freshness(
        &mut self,
        node: NodeId,
        new_result: CachedResult,
        version: VersionId,
        trace: DependencyTrace,
        still_refining: bool,
    ) -> EvalOutcome {
        // Derive generation from version per §7.1 — VersionId is the single source of truth.
        let generation = version.0;
        // Derive freshness from the supplied trace BEFORE passing ownership of `trace`
        // into record_evaluation_with_freshness (borrow-checker: sequential immutable+mutable borrows on self).
        let freshness = self.derive_output_freshness_from_trace(&trace, still_refining, generation);
        self.record_evaluation_with_freshness(node, new_result, version, trace, freshness)
    }

    /// Mark all cached nodes whose dependency trace reads any of the changed
    /// value cells as dirty. They keep their old entries for early cutoff
    /// comparison but will be re-evaluated on the next eval_cached() call.
    ///
    /// Each node's dirty reasons track which specific changed cells caused it
    /// to become dirty. A node only becomes clean when ALL its reasons are
    /// resolved.
    pub fn invalidate_dependents(&mut self, changed: &[ValueCellId]) {
        for (node, entry) in &self.caches {
            let matching_reasons: std::collections::HashSet<ValueCellId> = entry
                .dependency_trace
                .reads
                .iter()
                .filter(|read| changed.contains(read))
                .cloned()
                .collect();
            if !matching_reasons.is_empty() {
                self.dirty_reasons
                    .entry(node.clone())
                    .or_default()
                    .extend(matching_reasons);
            }
        }
    }

    /// Check if a node is marked as dirty (needs re-evaluation).
    pub fn is_dirty(&self, node: &NodeId) -> bool {
        self.dirty_reasons.contains_key(node)
    }

    /// Clear the dirty flag for a node after re-evaluation.
    pub fn clear_dirty(&mut self, node: &NodeId) {
        self.dirty_reasons.remove(node);
    }

    /// Remove a specific dirty reason from nodes whose dependency traces read
    /// the given value cell. Used after early cutoff: if a node's result didn't
    /// change, the `cell` reason is resolved for its dependents. A node is only
    /// fully cleared from dirty when ALL its reasons have been resolved.
    pub fn clear_dependents_dirty(&mut self, cell: &ValueCellId) {
        let mut to_fully_clear = Vec::new();
        for (node, reasons) in self.dirty_reasons.iter_mut() {
            if self
                .caches
                .get(node)
                .map(|entry| entry.dependency_trace.reads.contains(cell))
                .unwrap_or(false)
            {
                reasons.remove(cell);
                if reasons.is_empty() {
                    to_fully_clear.push(node.clone());
                }
            }
        }
        for node in to_fully_clear {
            self.dirty_reasons.remove(&node);
        }
    }

    /// Get the dirty reasons for a node, if any. Returns None if the node
    /// is not dirty.
    pub fn get_dirty_reasons(
        &self,
        node: &NodeId,
    ) -> Option<&std::collections::HashSet<ValueCellId>> {
        self.dirty_reasons.get(node)
    }

    /// Mark a node as Pending before re-evaluation during incremental evaluation.
    ///
    /// Sets the node's freshness to `Pending { last_substantive: Some(result_hash) }`
    /// where `result_hash` is the current cached content hash. This preserves the
    /// previous result for comparison while signaling that re-evaluation is in progress.
    ///
    /// Returns `true` if the node was found and marked, `false` if not cached.
    ///
    /// # Diagnostic chain — pick the right helper
    ///
    /// This no-cause helper **clears any stale `pending_cause` side-table
    /// entry** — the per-node evaluator is responsible for re-attaching a
    /// cause via [`CacheStore::mark_pending_with_cause`] when it detects a
    /// Failed/Pending input on this round. Wiping the chain here is the
    /// invariant that prevents the §9.2 diagnostic chain from leaking
    /// across freshness transitions and pointing at a node that was
    /// already re-evaluated successfully on a later round (regression
    /// pinned by `mark_pending_and_mark_failed_clear_stale_pending_cause`).
    ///
    /// Use [`CacheStore::mark_pending_with_cause`] instead whenever the
    /// transition is driven by a *known upstream* Failed/Pending node;
    /// that is the canonical wire for arch §9.2 propagation (see
    /// `docs/reify-implementation-architecture.md` lines 880-890).
    ///
    /// `mark_pending` is reserved for the **bulk dirty-flag pass** during
    /// incremental re-evaluation, where the "cause" is *the user-driven edit
    /// that bumped the version* rather than any specific upstream NodeId.
    /// Audit of in-tree callers (task #2330 amendment review):
    ///
    /// - `cache.rs::incremental_eval` — top-level dirty walk, no upstream
    ///   node is the cause (the trigger is the bumped `VersionId`).
    /// - `concurrent.rs::concurrent_eval` and `engine_edit.rs::edit_param` /
    ///   `engine_edit.rs::edit_source` — same shape: bulk-mark every member
    ///   of the eval-set Pending before the per-node evaluator runs.
    ///
    /// All four of these intentionally drop the chain because none of them
    /// has Failed or Pending in their input set at the moment they call
    /// `mark_pending`. The §9.2 chain is laid down inside the per-node
    /// evaluator itself (e.g. `evaluate_let_bindings` →
    /// `mark_pending_with_cause`), not by the bulk pre-pass. Future call
    /// sites that *do* have a known cause MUST migrate to
    /// [`CacheStore::mark_pending_with_cause`].
    pub fn mark_pending(&mut self, node: &NodeId) -> bool {
        if let Some(entry) = self.caches.get_mut(node) {
            entry.freshness = Freshness::Pending {
                last_substantive: ResultRef::of_hash(entry.result_hash),
            };
            // Clear any stale chain carried over from a prior round so the
            // per-node evaluator can decide afresh whether to re-attach a
            // cause via mark_pending_with_cause (task #2330 §9.2 invariant).
            entry.pending_cause = None;
            self.pending_transition_count += 1;
            true
        } else {
            false
        }
    }

    /// Mark a node as Failed, recording the supplied [`ErrorRef`].
    ///
    /// Sets `freshness = Freshness::Failed { error }` in place AND clears
    /// the `pending_cause` side-table on the entry. A Failed node is
    /// itself the chain root, not a forwarder — downstream Pending
    /// entries store *this NodeId* in their own `pending_cause`, so the
    /// Failed node's own `pending_cause` must read as `None` per the
    /// [`CacheStore::pending_cause`] reader contract. Clearing here
    /// prevents a stale chain (e.g. left over from a previous round in
    /// which this node was Pending) from leaking through and reporting a
    /// fictitious upstream cause for what is now a Failed root.
    ///
    /// Returns `true` if the node was present and updated, `false` if no
    /// cache entry exists. As with [`CacheStore::mark_pending`], absent-node
    /// auto-creation is intentionally not supported (no value/result/trace
    /// to seed).
    ///
    /// # Warning: do not pass `Freshness::Failed` directly
    ///
    /// `mark_failed` must be used instead of `set_freshness(node,
    /// Freshness::Failed { error })`. This helper centralises the
    /// "Failed nodes are chain roots" invariant (including the chain-clearing
    /// side effect); `set_freshness` would silently allow downstream
    /// callers to skip recording the chain root and to leak a stale
    /// `pending_cause`.
    pub fn mark_failed(&mut self, node: &NodeId, error: reify_types::ErrorRef) -> bool {
        if let Some(entry) = self.caches.get_mut(node) {
            entry.freshness = Freshness::Failed { error };
            // Failed nodes are chain roots — pending_cause() reader contract
            // requires None here. Wipe any stale chain from a prior round so
            // it cannot leak through (task #2330 §9.2 invariant).
            entry.pending_cause = None;
            true
        } else {
            false
        }
    }

    /// Mark a node as Pending with the supplied diagnostic-chain cause.
    ///
    /// Mirrors [`CacheStore::mark_pending`] (sets `freshness = Pending {
    /// last_substantive: ResultRef::of_hash(prev_hash) }`, bumps
    /// `pending_transition_count`) and additionally records `cause` in the
    /// `pending_cause` side-table on the entry.
    ///
    /// `cause` is the upstream `NodeId` that drove the transition — typically
    /// either a Failed leaf or another Pending node forwarding its own cause.
    /// See arch §9.2 lines 880-890 and arch §7.2 line 748.
    ///
    /// Returns `true` if the node was present and updated, `false` if no
    /// cache entry exists.
    pub fn mark_pending_with_cause(&mut self, node: &NodeId, cause: NodeId) -> bool {
        if let Some(entry) = self.caches.get_mut(node) {
            entry.freshness = Freshness::Pending {
                last_substantive: ResultRef::of_hash(entry.result_hash),
            };
            entry.pending_cause = Some(cause);
            self.pending_transition_count += 1;
            true
        } else {
            false
        }
    }

    /// Read the diagnostic-chain cause stored on a node's cache entry.
    ///
    /// Returns the `Option<NodeId>` from the entry's `pending_cause`
    /// side-table when present; returns `None` when the node has no entry
    /// (consistent with the "default to None on absent" pattern).
    ///
    /// Failed entries return `None` here (they are chain roots, not
    /// forwarders); only Pending entries written via
    /// [`CacheStore::mark_pending_with_cause`] populate this field.
    pub fn pending_cause(&self, node: &NodeId) -> Option<NodeId> {
        self.caches
            .get(node)
            .and_then(|e| e.pending_cause.clone())
    }

    /// Canonical reader for cache freshness.
    ///
    /// Returns the cached entry's freshness when present, else
    /// `Freshness::default()` (= `Final`) — see task #2326. Prefer this to
    /// `self.get(node).map(|e| e.freshness.clone())` so the default is
    /// centralized and any future audit of "what is the default freshness"
    /// has a single grep target.
    pub fn freshness(&self, node: &NodeId) -> Freshness {
        self.caches
            .get(node)
            .map(|e| e.freshness.clone())
            .unwrap_or_default()
    }

    /// Canonical writer for cache freshness.
    ///
    /// Updates the cached entry's freshness in place and returns `true`;
    /// returns `false` (no-op) when the node has no cache entry — auto-creation
    /// has no value/result/trace to seed (see task #2326 design decision). Use
    /// `put(node, NodeCache::new(...))` to insert a fresh entry.
    ///
    /// # Precondition: do not pass `Freshness::Pending`
    ///
    /// `mark_pending` or `mark_pending_with_cause` must be used instead of
    /// `set_freshness(node, Freshness::Pending { ... })` when transitioning a node
    /// to the Pending state. Those helpers derive `last_substantive` from the current
    /// cached `result_hash` (ensuring consistency) and increment
    /// `pending_transition_count` (a diagnostic counter). This precondition is
    /// enforced via `assert!` in all builds (task #2451 S1).
    ///
    /// **S2 audit (task #2451):** production write paths in `concurrent.rs` and
    /// `engine_edit.rs` already route all Pending transitions through `mark_pending`
    /// / `mark_pending_with_cause` (tasks #2326, #2335). `CacheStore::caches` is a
    /// private field with no public `get_mut` accessor, so external code cannot
    /// write `freshness` directly. This precondition therefore covers all production
    /// write sites.
    ///
    /// `restore_final` and `mark_pending` continue to coexist as domain-specific
    /// helpers. `mark_pending` additionally captures `result_hash` into
    /// `last_substantive` and bumps `pending_transition_count`; `restore_final`
    /// is today equivalent to `set_freshness(node, Freshness::Final)` but is
    /// retained for readability at its call sites.
    #[must_use = "set_freshness returns false when the node is absent; check or explicitly discard"]
    pub fn set_freshness(&mut self, node: &NodeId, freshness: Freshness) -> bool {
        assert!(
            !matches!(freshness, Freshness::Pending { .. }),
            "set_freshness must not be passed Pending; use mark_pending or mark_pending_with_cause instead"
        );
        if let Some(entry) = self.caches.get_mut(node) {
            entry.freshness = freshness;
            true
        } else {
            false
        }
    }

    /// Restore a node's freshness to Final after early cutoff skips its
    /// re-evaluation. This handles nodes that were pre-marked Pending but
    /// then bypassed because an upstream node produced an unchanged result.
    ///
    /// Returns `true` if the node was found and restored, `false` if not cached.
    pub fn restore_final(&mut self, node: &NodeId) -> bool {
        if let Some(entry) = self.caches.get_mut(node) {
            entry.freshness = Freshness::Final;
            true
        } else {
            false
        }
    }

    /// Get the number of successful mark_pending() calls since last reset.
    pub fn pending_transition_count(&self) -> usize {
        self.pending_transition_count
    }

    /// Reset the pending transition counter to 0.
    pub fn reset_pending_transition_count(&mut self) {
        self.pending_transition_count = 0;
    }

    /// Store warm-start state on an existing cached node.
    ///
    /// Returns `true` if the node was found and warm state was set,
    /// `false` if the node is not in the cache (no-op).
    pub fn donate_warm_state(&mut self, node: &NodeId, state: OpaqueState) -> bool {
        if let Some(entry) = self.caches.get_mut(node) {
            entry.warm_state = Some(state);
            true
        } else {
            false
        }
    }

    /// Take the warm-start state out of a cached node (take semantics).
    ///
    /// Returns the `OpaqueState` if present, leaving `None` in its place.
    /// A second call for the same node will return `None`.
    pub fn get_warm_state(&mut self, node: &NodeId) -> Option<OpaqueState> {
        self.caches.get_mut(node)?.warm_state.take()
    }

    /// Version fast path: if the node is cached and its basis_version matches
    /// the current version, return a clone of the cached result without
    /// re-evaluation.
    pub fn try_fast_path(&self, node: &NodeId, current_version: VersionId) -> Option<CachedResult> {
        let entry = self.caches.get(node)?;
        if entry.basis_version == current_version {
            Some(entry.result.clone())
        } else {
            None
        }
    }

    /// Derive the output freshness from a **supplied** dependency trace.
    ///
    /// Delegates to [`derive_output_freshness_from_trace_with_cause`] and drops the
    /// cause, so the §7.2/§9.2 classification logic lives in exactly one place. See
    /// that method for the full truth table.
    ///
    /// Callers that hold the just-evaluated trace (e.g. the wire site in
    /// `evaluate_let_bindings`) should prefer this over
    /// [`derive_output_freshness_for_node`] to avoid keying off a stale prior trace.
    pub fn derive_output_freshness_from_trace(
        &self,
        trace: &DependencyTrace,
        still_refining: bool,
        generation: u64,
    ) -> Freshness {
        let (f, _) = self.derive_output_freshness_from_trace_with_cause(trace, still_refining, generation);
        f
    }

    /// Derive the output freshness for a cached node by walking its **cached** dependency trace.
    ///
    /// Delegates to [`derive_output_freshness_for_node_with_cause`] and drops the
    /// cause, so the §7.2/§9.2 classification logic lives in exactly one place. See
    /// that method for the full truth table.
    ///
    /// **Old-trace semantics:** this method reads the trace that was stored during the
    /// *prior* evaluation of `node_id`, not a freshly-computed one.  At the wire site in
    /// `evaluate_let_bindings` the just-computed `trace` is available locally; prefer
    /// [`derive_output_freshness_from_trace`] there to avoid keying off a stale trace.
    /// This method is the right choice for callers that only have a `NodeId` and do not
    /// hold the current trace (e.g. diagnostics, tests that verify post-eval state).
    ///
    /// **Absent node fallback:** if `node_id` has no cache entry, returns `Final`
    /// (no inputs ⇒ all-Final ⇒ Final) — consistent with `freshness()`'s
    /// "default Final on absent" contract.
    pub fn derive_output_freshness_for_node(
        &self,
        node_id: &NodeId,
        still_refining: bool,
        generation: u64,
    ) -> Freshness {
        let (f, _) = self.derive_output_freshness_for_node_with_cause(node_id, still_refining, generation);
        f
    }

    /// Cause-bearing variant of [`derive_output_freshness_for_node`].
    ///
    /// Returns both the derived `Freshness` AND the upstream `NodeId` (if any)
    /// that drove the Pending output, by feeding `(NodeId, Freshness, Option<NodeId>)`
    /// triples into [`derive_output_freshness_with_cause`]. The triples are built
    /// from the node's cached `dependency_trace.reads`: each read's freshness is
    /// looked up via [`CacheStore::freshness`] and its `pending_cause` via
    /// [`CacheStore::pending_cause`].
    ///
    /// **Absent node fallback:** if `node_id` has no cache entry, returns
    /// `(Final, None)` (consistent with the pure helper's empty-input case).
    ///
    /// See arch §9.2 lines 880-890 (Failed → Pending carve-out) and arch §7.2
    /// line 748 (Pending forwards the chain).
    pub fn derive_output_freshness_for_node_with_cause(
        &self,
        node_id: &NodeId,
        still_refining: bool,
        generation: u64,
    ) -> (Freshness, Option<NodeId>) {
        let Some(entry) = self.caches.get(node_id) else {
            return derive_output_freshness_with_cause(
                still_refining,
                std::iter::empty(),
                generation,
            );
        };
        self.derive_output_freshness_from_trace_with_cause(
            &entry.dependency_trace,
            still_refining,
            generation,
        )
    }

    /// Cause-bearing variant of [`derive_output_freshness_from_trace`].
    ///
    /// Walks `trace.reads`, looks up each input's freshness and `pending_cause`,
    /// and feeds the triples into [`derive_output_freshness_with_cause`]. Use
    /// this at wire sites that have a freshly-computed trace and need the chain
    /// root — e.g. the pre-eval Pending gate in `evaluate_let_bindings`.
    pub fn derive_output_freshness_from_trace_with_cause(
        &self,
        trace: &DependencyTrace,
        still_refining: bool,
        generation: u64,
    ) -> (Freshness, Option<NodeId>) {
        derive_output_freshness_with_cause(
            still_refining,
            trace.reads.iter().map(|read| {
                let n = NodeId::Value(read.clone());
                let f = self.freshness(&n);
                let c = self.pending_cause(&n);
                (n, f, c)
            }),
            generation,
        )
    }

    /// Insert a synthetic cache entry for a Realization node so that tests can
    /// simulate state that `engine_build.rs` would normally create at
    /// `build()` / `check()` time.
    ///
    /// ## Contract
    ///
    /// **What callers may depend on:**
    /// - The entry exists under `NodeId::Realization(rid)` immediately after
    ///   this call returns.
    /// - `donate_warm_state(&NodeId::Realization(rid), …)` returns `true`
    ///   (the entry is present, so warm state can be attached to it).
    ///
    /// **What callers must NOT depend on:**
    /// - The specific `CachedResult` variant or any field's exact value.
    ///   These are placeholders that may evolve as the cache schema changes.
    ///   Tests that need to inspect the result payload should use the normal
    ///   eval path instead.
    ///
    /// ## Why this exists
    ///
    /// `engine_build.rs` creates Realization cache entries on demand during
    /// `build()` / `check()`, not during `edit_source()`.  Tests that exercise
    /// the warm-state donation hook for Realization nodes must therefore
    /// synthesize an entry before calling `edit_source`.  This helper
    /// centralizes that synthesis so future schema changes (`CachedResult`
    /// gaining a new variant, `NodeCache::new` gaining a parameter, etc.)
    /// produce a single compile error here rather than silent breakage in
    /// scattered test code.
    ///
    /// ## When to retire
    ///
    /// Once `engine_build.rs` or another engine path creates Realization cache
    /// entries during `edit_source`, callers can switch to the normal eval path
    /// and this helper becomes dead code.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn insert_synthetic_realization_entry(&mut self, rid: &RealizationNodeId) {
        let node = NodeId::Realization(rid.clone());
        self.put(
            node,
            NodeCache::new(
                CachedResult::GeometryHandle(GeometryHandleId(0)),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(0),
            ),
        );
    }
}

/// Derive output freshness from `still_refining` flag and input freshnesses.
///
/// Implements arch §7.2 lines 730-749 with the §9.2 line 890 carve-out
/// (`docs/reify-implementation-architecture.md`):
///
/// | `still_refining` | any input Failed | any input Pending | any input Intermediate | output                     |
/// |------------------|------------------|-------------------|------------------------|----------------------------|
/// | `true`           | –                | –                 | –                      | `Intermediate { generation }` |
/// | `false`          | `true`           | –                 | –                      | `Pending { last_substantive: ResultRef::none() }` (§9.2) |
/// | `false`          | `false`          | `true`            | –                      | `Pending { last_substantive: ResultRef::none() }` (§7.2) |
/// | `false`          | `false`          | `false`           | `true`                 | `Intermediate { generation }` |
/// | `false`          | `false`          | `false`           | `false` (all Final)    | `Final`                    |
///
/// Note: Failed and Pending are carved out of the §7.2 "any non-Final → Intermediate"
/// rule per §9.2 line 890 (Failed) and §7.2 line 748 (Pending). Both produce a Pending
/// output so the downstream subtree is naturally quieted. Chain forwarding (the cause
/// NodeId) is handled at the `_with_cause` layer; the pure helper drops the chain and
/// returns `Pending { last_substantive: ResultRef::none() }`.
///
/// See the `derive_output_freshness_classifies_pending_and_failed_inputs_as_non_final`
/// unit test for truth-table coverage across all 4 input variants and the
/// `derive_output_freshness_with_cause_returns_failing_node` test for chain semantics.
pub fn derive_output_freshness(
    still_refining: bool,
    input_freshnesses: impl IntoIterator<Item = Freshness>,
    generation: u64,
) -> Freshness {
    if still_refining {
        return Freshness::Intermediate { generation };
    }
    // Single pass: classify the strongest non-Final input we see.
    let mut saw_failed = false;
    let mut saw_pending = false;
    let mut saw_intermediate = false;
    for f in input_freshnesses {
        match f {
            Freshness::Final => {}
            Freshness::Intermediate { .. } => saw_intermediate = true,
            Freshness::Pending { .. } => saw_pending = true,
            Freshness::Failed { .. } => saw_failed = true,
        }
    }
    // §9.2 carve-out: Failed input → Pending output (chain handled at the _with_cause layer).
    if saw_failed {
        return Freshness::Pending { last_substantive: ResultRef::none() };
    }
    // §7.2 line 748: Pending input → Pending output (chain forwarded at _with_cause layer).
    if saw_pending {
        return Freshness::Pending { last_substantive: ResultRef::none() };
    }
    // §7.2 main rule: Intermediate input → Intermediate output.
    if saw_intermediate {
        return Freshness::Intermediate { generation };
    }
    Freshness::Final
}

/// Cause-bearing variant of [`derive_output_freshness`].
///
/// Returns both the derived `Freshness` AND the upstream `NodeId` (if any) that
/// drove the Pending output. The chain semantics are:
///
/// - **Failed input** contributes its own `NodeId` as the chain root (regardless of
///   whether the input also has a `pending_cause` — Failed nodes are themselves the
///   root, not a forwarder).
/// - **Pending input** forwards the upstream entry's `pending_cause` (or `None` if
///   the upstream chain was never recorded). The Pending node's own NodeId is not
///   used as the cause — only a Failed leaf is a chain root.
/// - All other input combinations return `(freshness, None)`.
///
/// When multiple non-Final inputs are present, the **first Failed** input wins;
/// otherwise the **first Pending** input wins. This deterministic ordering matches
/// the iteration order over `trace.reads` at the wire site.
///
/// See arch §9.2 lines 880-890 and arch §7.2 line 748.
pub fn derive_output_freshness_with_cause(
    still_refining: bool,
    inputs: impl IntoIterator<Item = (NodeId, Freshness, Option<NodeId>)>,
    generation: u64,
) -> (Freshness, Option<NodeId>) {
    if still_refining {
        return (Freshness::Intermediate { generation }, None);
    }
    let mut failed_cause: Option<NodeId> = None;
    let mut pending_cause: Option<NodeId> = None;
    let mut saw_pending = false;
    let mut saw_intermediate = false;
    for (id, freshness, upstream_cause) in inputs {
        match freshness {
            Freshness::Final => {}
            Freshness::Intermediate { .. } => saw_intermediate = true,
            Freshness::Pending { .. } => {
                saw_pending = true;
                if pending_cause.is_none() {
                    // Pending forwards the upstream entry's pending_cause; the Pending
                    // node's own NodeId is NOT used (only Failed leaves are chain roots).
                    pending_cause = upstream_cause;
                }
            }
            Freshness::Failed { .. } => {
                if failed_cause.is_none() {
                    // Failed contributes its OWN NodeId as the chain root.
                    failed_cause = Some(id);
                }
            }
        }
    }
    // §9.2 carve-out: any Failed input → Pending output, with Failed NodeId as chain root.
    if let Some(cause) = failed_cause {
        return (
            Freshness::Pending { last_substantive: ResultRef::none() },
            Some(cause),
        );
    }
    // §7.2 line 748: any Pending input → Pending output, with the upstream cause forwarded.
    // Cause may be `None` if the upstream chain was never recorded — that yields
    // `(Pending, None)` (sentinel for "chain incomplete").
    if saw_pending {
        return (
            Freshness::Pending { last_substantive: ResultRef::none() },
            pending_cause,
        );
    }
    if saw_intermediate {
        return (Freshness::Intermediate { generation }, None);
    }
    (Freshness::Final, None)
}

/// Compute the input hash for a value cell expression.
///
/// Combines the expression's content_hash with dependency value hashes
/// to produce a deterministic input hash. This ensures that if either the
/// expression structure or any dependency value changes, the input hash changes.
pub fn compute_input_hash(
    expr: &CompiledExpr,
    deps: &[ValueCellId],
    values: &ValueMap,
) -> ContentHash {
    // Start with expression's content hash (captures structure)
    let mut combined = expr.content_hash;

    // Collect dependency value hashes (order doesn't matter since we combine them all)
    let dep_hashes: Vec<_> = deps
        .iter()
        .filter_map(|id| values.get(id))
        .map(|v| v.content_hash())
        .collect();

    // Only combine if there are dependency hashes (empty combine_all returns zero hash)
    if !dep_hashes.is_empty() {
        let dep_combined = ContentHash::combine_all(dep_hashes);
        combined = combined.combine(dep_combined);
    }

    combined
}

/// Check if a re-evaluated value has the same content hash as cached.
///
/// Returns true if the new value hash matches the cached entry's result hash,
/// indicating no change occurred (early cutoff should apply).
pub fn check_early_cutoff(
    store: &CacheStore,
    id: &ValueCellId,
    new_value_hash: ContentHash,
) -> bool {
    let node = NodeId::Value(id.clone());
    if let Some(entry) = store.get(&node) {
        entry.result_hash == new_value_hash
    } else {
        false
    }
}

/// Compute the dirty set: all cells that need re-evaluation after input changes.
///
/// Walks dependents in topological order, re-evaluating and checking for early
/// cutoff to prune the propagation frontier. Returns cells that actually
/// changed value.
pub fn dirty_set(
    changed: &[ValueCellId],
    dep_map: &crate::deps::DependencyMap,
    _graph: &crate::graph::EvaluationGraph,
    _values: &ValueMap,
    store: &mut CacheStore,
) -> Vec<ValueCellId> {
    let mut dirty: Vec<ValueCellId> = changed.to_vec();
    let mut result: Vec<ValueCellId> = Vec::new();
    let mut visited: std::collections::HashSet<ValueCellId> = std::collections::HashSet::new();

    // Get topological order for consistent processing
    let topo_order = dep_map.topological_order();

    // Process in topological order
    for cell in &topo_order {
        if !dirty.contains(cell) {
            continue;
        }

        // Skip if already processed
        if visited.contains(cell) {
            continue;
        }
        visited.insert(cell.clone());

        // Check if fresh
        if store.is_fresh(cell) {
            // Clear dirty reasons for this cell since it's fresh
            let node = NodeId::Value(cell.clone());
            store.clear_dirty(&node);
            continue;
        }

        // Mark as changed (needs re-evaluation)
        result.push(cell.clone());

        // Propagate to dependents
        let dependents = dep_map.dependents_of(cell);
        for dependent in dependents {
            if !dirty.contains(dependent) {
                dirty.push(dependent.clone());
            }
        }
    }

    result
}

/// Incremental evaluation: re-evaluate only the cells that changed.
///
/// Returns the list of cells that actually changed value (for reporting).
/// This is the main entry point for incremental evaluation.
pub fn incremental_eval(
    cache: &mut CacheStore,
    _graph: &crate::graph::EvaluationGraph,
    dep_map: &crate::deps::DependencyMap,
    _values: &mut ValueMap,
    changed: &[ValueCellId],
) -> Vec<ValueCellId> {
    // 1. Bump version to invalidate stale entries
    cache.bump_version();

    // 2. Mark all dependents as dirty based on changed cells
    cache.invalidate_dependents(changed);

    // 3. Compute dirty set using topological order
    let mut dirty = Vec::new();
    let topo_order = dep_map.topological_order();

    // Start with directly changed cells
    for cell in changed {
        if !dirty.contains(cell) {
            dirty.push(cell.clone());
        }
    }

    // Walk through topological order, checking freshness
    let mut result: Vec<ValueCellId> = Vec::new();

    for cell in &topo_order {
        // Skip cells that aren't affected
        if !dirty.contains(cell) {
            continue;
        }

        // Check if fresh (same version)
        if cache.is_fresh(cell) {
            // Clear dirty flag and skip
            let node = NodeId::Value(cell.clone());
            cache.clear_dirty(&node);
            continue;
        }

        // Mark as pending before evaluation. Bulk dirty-pass: the "cause"
        // is the user-driven version bump that produced `changed`, not any
        // upstream Failed/Pending NodeId, so the no-cause helper is correct
        // here. The arch §9.2 diagnostic chain is laid down by the per-node
        // evaluator (e.g. `evaluate_let_bindings`'s pre-eval Pending gate)
        // when it actually observes a Failed/Pending input.
        let node = NodeId::Value(cell.clone());
        cache.mark_pending(&node);

        // Re-evaluate this cell
        // For now, we'll use a placeholder - actual evaluation happens in Engine
        // The function returns cells that need re-evaluation
        result.push(cell.clone());

        // Propagate to dependents
        let dependents = dep_map.dependents_of(cell);
        for dependent in dependents {
            if !dirty.contains(dependent) {
                dirty.push(dependent.clone());
            }
        }
    }

    result
}

impl Default for CacheStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{ConstraintNodeId, RealizationNodeId, Type, ValueCellId};

    #[test]
    fn node_id_from_value_cell_id() {
        let vcid = ValueCellId::new("Bracket", "width");
        let node: NodeId = NodeId::from(vcid.clone());
        assert_eq!(node, NodeId::Value(vcid));
    }

    #[test]
    fn node_id_from_constraint_node_id() {
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let node: NodeId = NodeId::from(cnid.clone());
        assert_eq!(node, NodeId::Constraint(cnid));
    }

    #[test]
    fn node_id_from_realization_node_id() {
        let rnid = RealizationNodeId::new("Bracket", 0);
        let node: NodeId = NodeId::from(rnid.clone());
        assert_eq!(node, NodeId::Realization(rnid));
    }

    #[test]
    fn node_id_resolution_variant() {
        use reify_types::ResolutionNodeId;

        let res_id = ResolutionNodeId::new("A", 0);
        let res_node = NodeId::Resolution(res_id.clone());

        // Equality with itself
        assert_eq!(res_node, NodeId::Resolution(ResolutionNodeId::new("A", 0)));

        // Differs from other variants
        assert_ne!(res_node, NodeId::Value(ValueCellId::new("A", "x")));
        assert_ne!(res_node, NodeId::Constraint(ConstraintNodeId::new("A", 0)));
        assert_ne!(
            res_node,
            NodeId::Realization(RealizationNodeId::new("A", 0))
        );

        // From<ResolutionNodeId> conversion
        let from_node: NodeId = NodeId::from(res_id.clone());
        assert_eq!(from_node, res_node);
    }

    #[test]
    fn node_id_variants_not_equal_even_with_overlapping_strings() {
        let vcid = ValueCellId::new("Bracket", "width");
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let rnid = RealizationNodeId::new("Bracket", 0);

        let v = NodeId::Value(vcid);
        let c = NodeId::Constraint(cnid);
        let r = NodeId::Realization(rnid);

        assert_ne!(v, c);
        assert_ne!(v, r);
        assert_ne!(c, r);
    }

    #[test]
    fn node_id_clone_and_debug() {
        let vcid = ValueCellId::new("Bracket", "width");
        let node = NodeId::Value(vcid);
        let cloned = node.clone();
        assert_eq!(node, cloned);

        let debug = format!("{:?}", node);
        assert!(debug.contains("Value"));
    }

    #[test]
    fn node_id_hash_as_map_key() {
        use std::collections::HashMap;
        let vcid = ValueCellId::new("Bracket", "width");
        let cnid = ConstraintNodeId::new("Bracket", 0);

        let mut map = HashMap::new();
        map.insert(NodeId::Value(vcid.clone()), "value");
        map.insert(NodeId::Constraint(cnid.clone()), "constraint");

        assert_eq!(map.get(&NodeId::Value(vcid)), Some(&"value"));
        assert_eq!(map.get(&NodeId::Constraint(cnid)), Some(&"constraint"));
    }

    // --- invalidate_dependents tests ---

    #[test]
    fn invalidate_dependents_removes_dependent_nodes() {
        let mut store = CacheStore::new();
        let a = ValueCellId::new("Bracket", "a");
        let b = ValueCellId::new("Bracket", "b");
        let x_id = ValueCellId::new("Bracket", "x");
        let y_id = ValueCellId::new("Bracket", "y");

        // x depends on a
        let node_x = NodeId::Value(x_id.clone());
        let mut trace_x = DependencyTrace::default();
        trace_x.reads.push(a.clone());
        store.put(
            node_x.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
                Freshness::Final,
                trace_x,
                VersionId(1),
            ),
        );

        // y depends on b (not a)
        let node_y = NodeId::Value(y_id.clone());
        let mut trace_y = DependencyTrace::default();
        trace_y.reads.push(b.clone());
        store.put(
            node_y.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Int(2), DeterminacyState::Determined),
                Freshness::Final,
                trace_y,
                VersionId(1),
            ),
        );

        // Invalidate dependents of a
        store.invalidate_dependents(std::slice::from_ref(&a));

        // x should be marked dirty (depends on a) but entry still exists
        assert!(store.is_dirty(&node_x));
        assert!(store.get(&node_x).is_some()); // entry kept for early cutoff
        // y should NOT be dirty (depends on b, not a)
        assert!(!store.is_dirty(&node_y));
        assert!(store.get(&node_y).is_some());

        // Verify dirty_reasons are correctly populated
        let x_reasons = store.get_dirty_reasons(&node_x).unwrap();
        assert!(x_reasons.contains(&a), "x's dirty reasons should include a");
        assert_eq!(x_reasons.len(), 1, "x should have exactly one dirty reason");

        // y should have no dirty reasons
        assert!(store.get_dirty_reasons(&node_y).is_none());
    }

    // --- record_evaluation tests ---

    #[test]
    fn record_evaluation_early_cutoff_same_hash() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "x"));

        // First evaluation: cold start
        let result1 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let outcome1 = store.record_evaluation(
            node.clone(),
            result1,
            VersionId(1),
            DependencyTrace::default(),
        );
        assert_eq!(outcome1, EvalOutcome::Changed);

        // Second evaluation: same result, new version -> early cutoff
        let result2 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let outcome2 = store.record_evaluation(
            node.clone(),
            result2,
            VersionId(2),
            DependencyTrace::default(),
        );
        assert_eq!(outcome2, EvalOutcome::Unchanged);

        // basis_version should be updated even though result unchanged
        assert_eq!(store.get(&node).unwrap().basis_version, VersionId(2));
    }

    #[test]
    fn record_evaluation_changed_when_different_hash() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "x"));

        // First evaluation
        let result1 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        store.record_evaluation(
            node.clone(),
            result1,
            VersionId(1),
            DependencyTrace::default(),
        );

        // Second evaluation: different result
        let result2 = CachedResult::Value(Value::Int(99), DeterminacyState::Determined);
        let outcome = store.record_evaluation(
            node.clone(),
            result2,
            VersionId(2),
            DependencyTrace::default(),
        );
        assert_eq!(outcome, EvalOutcome::Changed);
        assert_eq!(store.get(&node).unwrap().basis_version, VersionId(2));
    }

    #[test]
    fn record_evaluation_cold_start_is_changed() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "x"));

        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let outcome = store.record_evaluation(
            node.clone(),
            result,
            VersionId(1),
            DependencyTrace::default(),
        );
        assert_eq!(outcome, EvalOutcome::Changed);
        assert!(store.get(&node).is_some());
    }

    /// End-to-end companion to
    /// `reify_types::value::tests::nan_payload_hash_equality_invariant_exception`.
    ///
    /// Locks in the 'Known intentional exception — incremental cache' section of
    /// `Value::content_hash`'s doc comment (in `crates/reify-types/src/value.rs`):
    /// two `Value::Real` payloads that differ only in NaN bit-pattern must hash
    /// identically, so the second evaluation triggers early cutoff (Unchanged) even
    /// though the raw `f64` bits differ.
    ///
    /// If someone ever changes `Value::content_hash` to preserve NaN payloads this
    /// test will fail here in `reify-eval`, not just in `reify-types`, making the
    /// cache-layer regression immediately visible.
    #[test]
    fn record_evaluation_nan_payload_early_cutoff() {
        // Build two f64 NaN values with distinct bit patterns.
        let canonical_nan = f64::NAN;
        let non_canon_nan = f64::from_bits(f64::NAN.to_bits() ^ 1);
        assert!(canonical_nan.is_nan());
        assert!(non_canon_nan.is_nan());
        assert_ne!(canonical_nan.to_bits(), non_canon_nan.to_bits());

        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "x"));

        // First evaluation: cold start with canonical NaN -> Changed
        let result1 = CachedResult::Value(Value::Real(canonical_nan), DeterminacyState::Determined);
        let outcome1 = store.record_evaluation(
            node.clone(),
            result1,
            VersionId(1),
            DependencyTrace::default(),
        );
        assert_eq!(outcome1, EvalOutcome::Changed);

        // Second evaluation: NaN with a different bit pattern -> Unchanged (early cutoff)
        // Value::content_hash canonicalizes all NaN payloads to f64::NAN.to_bits(),
        // so the two results produce the same hash and the cache treats them as equal.
        let result2 = CachedResult::Value(Value::Real(non_canon_nan), DeterminacyState::Determined);
        let outcome2 = store.record_evaluation(
            node.clone(),
            result2,
            VersionId(2),
            DependencyTrace::default(),
        );
        assert_eq!(outcome2, EvalOutcome::Unchanged);

        // basis_version is still bumped even on early cutoff
        assert_eq!(store.get(&node).unwrap().basis_version, VersionId(2));
    }

    // --- Version fast path tests ---

    #[test]
    fn try_fast_path_hit_when_same_version() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.put(node.clone(), make_test_node_cache(42, 1));

        let result = store.try_fast_path(&node, VersionId(1));
        if let Some(CachedResult::Value(v, _)) = result {
            assert_eq!(v, Value::Int(42));
        } else {
            panic!("expected Value variant with Int(42)");
        }
    }

    #[test]
    fn try_fast_path_miss_when_different_version() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.put(node.clone(), make_test_node_cache(42, 1));

        let result = store.try_fast_path(&node, VersionId(2));
        assert!(result.is_none());
    }

    #[test]
    fn try_fast_path_miss_when_not_cached() {
        let store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));

        let result = store.try_fast_path(&node, VersionId(1));
        assert!(result.is_none());
    }

    // --- is_fresh and bump_version tests (Step 11) ---

    #[test]
    fn is_fresh_returns_true_when_entry_version_matches() {
        let mut store = CacheStore::new();
        // Store starts at VersionId(0)
        let node = NodeId::Value(ValueCellId::new("A", "x"));
        let cache = NodeCache::new(
            CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(0), // basis version matches store version
        );
        store.put(node.clone(), cache);

        let id = ValueCellId::new("A", "x");
        assert!(store.is_fresh(&id));
    }

    #[test]
    fn is_fresh_returns_false_after_bump_version() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("A", "x"));
        let cache = NodeCache::new(
            CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(0),
        );
        store.put(node.clone(), cache);

        // Bump version - now entry is stale
        store.bump_version();

        let id = ValueCellId::new("A", "x");
        assert!(!store.is_fresh(&id));
    }

    #[test]
    fn is_fresh_returns_false_for_missing_entry() {
        let store = CacheStore::new();
        let id = ValueCellId::new("A", "nonexistent");
        assert!(!store.is_fresh(&id));
    }

    #[test]
    fn bump_version_increments_version() {
        let mut store = CacheStore::new();
        assert_eq!(store.version(), VersionId(0));

        let new_version = store.bump_version();
        assert_eq!(new_version, VersionId(1));
        assert_eq!(store.version(), VersionId(1));

        let new_version2 = store.bump_version();
        assert_eq!(new_version2, VersionId(2));
        assert_eq!(store.version(), VersionId(2));
    }

    // --- compute_input_hash tests (Step 13) ---

    #[test]
    fn compute_input_hash_deterministic_with_sorted_deps() {
        use reify_types::BinOp;

        // x + y where x < y in id order
        let x = ValueCellId::new("A", "a");
        let y = ValueCellId::new("A", "b");
        let expr = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x.clone(), Type::Real),
            CompiledExpr::value_ref(y.clone(), Type::Real),
            Type::Real,
        );

        let mut values = ValueMap::new();
        values.insert(x.clone(), Value::Real(1.0));
        values.insert(y.clone(), Value::Real(2.0));

        // Compute hash with deps in order [a, b]
        let deps = vec![x.clone(), y.clone()];
        let hash1 = compute_input_hash(&expr, &deps, &values);

        // Same deps same order should produce same hash
        let hash2 = compute_input_hash(&expr, &deps, &values);
        assert_eq!(hash1, hash2, "same deps should produce same hash");
    }

    #[test]
    fn compute_input_hash_different_values_produce_different_hashes() {
        let x = ValueCellId::new("A", "a");
        let expr = CompiledExpr::value_ref(x.clone(), Type::Real);

        // Values: 1.0
        let mut values1 = ValueMap::new();
        values1.insert(x.clone(), Value::Real(1.0));
        let hash1 = compute_input_hash(&expr, std::slice::from_ref(&x), &values1);

        // Values: 2.0
        let mut values2 = ValueMap::new();
        values2.insert(x.clone(), Value::Real(2.0));
        let hash2 = compute_input_hash(&expr, std::slice::from_ref(&x), &values2);

        assert_ne!(
            hash1, hash2,
            "different values should produce different hashes"
        );
    }

    #[test]
    fn compute_input_hash_empty_deps_uses_expr_hash() {
        // Literal expression with no deps
        use std::f64::consts::PI;
        let expr = CompiledExpr::literal(Value::Real(PI), Type::Real);
        let values = ValueMap::new();

        let hash = compute_input_hash(&expr, &[], &values);
        let expr_hash = expr.content_hash;

        assert_eq!(hash, expr_hash, "empty deps should return expr hash");
    }

    #[test]
    fn compute_input_hash_different_expr_produce_different_hashes() {
        use reify_types::BinOp;

        let x = ValueCellId::new("A", "a");

        // a + a
        let expr_add = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x.clone(), Type::Real),
            CompiledExpr::value_ref(x.clone(), Type::Real),
            Type::Real,
        );

        // a * a
        let expr_mul = CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(x.clone(), Type::Real),
            CompiledExpr::value_ref(x.clone(), Type::Real),
            Type::Real,
        );

        let mut values = ValueMap::new();
        values.insert(x.clone(), Value::Real(2.0));

        let hash_add = compute_input_hash(&expr_add, std::slice::from_ref(&x), &values);
        let hash_mul = compute_input_hash(&expr_mul, std::slice::from_ref(&x), &values);

        assert_ne!(
            hash_add, hash_mul,
            "different expressions should produce different hashes"
        );
    }

    // --- CacheStore tests ---

    fn make_test_node_cache(val: i64, version: u64) -> NodeCache {
        NodeCache::new(
            CachedResult::Value(Value::Int(val), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(version),
        )
    }

    #[test]
    fn cache_store_new_is_empty() {
        let store = CacheStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn cache_store_put_and_get() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        let cache = make_test_node_cache(42, 1);
        store.put(node.clone(), cache);

        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
        assert!(store.get(&node).is_some());
        assert_eq!(store.get(&node).unwrap().basis_version, VersionId(1));
    }

    #[test]
    fn cache_store_get_missing() {
        let store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        assert!(store.get(&node).is_none());
    }

    #[test]
    fn cache_store_invalidate() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.put(node.clone(), make_test_node_cache(42, 1));
        assert!(store.get(&node).is_some());

        store.invalidate(&node);
        assert!(store.get(&node).is_none());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn cache_store_invalidate_missing_is_noop() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.invalidate(&node); // no panic
        assert!(store.is_empty());
    }

    #[test]
    fn cache_store_put_overwrites() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.put(node.clone(), make_test_node_cache(42, 1));
        store.put(node.clone(), make_test_node_cache(99, 2));

        assert_eq!(store.len(), 1);
        assert_eq!(store.get(&node).unwrap().basis_version, VersionId(2));
    }

    #[test]
    fn cache_store_len_and_is_empty() {
        let mut store = CacheStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);

        let node1 = NodeId::Value(ValueCellId::new("Bracket", "width"));
        let node2 = NodeId::Value(ValueCellId::new("Bracket", "height"));
        store.put(node1, make_test_node_cache(42, 1));
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());

        store.put(node2, make_test_node_cache(99, 1));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn cache_store_resolution_result() {
        use reify_types::{Freshness, ResolutionNodeId, Value, VersionId};
        use std::collections::HashMap;

        let mut store = CacheStore::new();
        let res_id = ResolutionNodeId::new("A", 0);
        let node = NodeId::Resolution(res_id);

        let mut values = HashMap::new();
        values.insert(ValueCellId::new("A", "x"), Value::Real(1.0));
        let result = CachedResult::Resolution(values);
        let expected_hash = result.content_hash();

        let version = VersionId(1);
        let trace = DependencyTrace {
            reads: vec![ValueCellId::new("A", "x")],
        };

        let outcome = store.record_evaluation(node.clone(), result, version, trace);
        assert_eq!(outcome, EvalOutcome::Changed);

        let entry = store.get(&node).unwrap();
        assert_eq!(entry.freshness, Freshness::Final);
        assert_eq!(entry.result_hash, expected_hash);
    }

    // --- EvalOutcome tests ---

    #[test]
    fn eval_outcome_changed_variant() {
        let outcome = EvalOutcome::Changed;
        assert_eq!(outcome, EvalOutcome::Changed);
        assert_ne!(outcome, EvalOutcome::Unchanged);
    }

    #[test]
    fn eval_outcome_unchanged_variant() {
        let outcome = EvalOutcome::Unchanged;
        assert_eq!(outcome, EvalOutcome::Unchanged);
        assert_ne!(outcome, EvalOutcome::Changed);
    }

    #[test]
    fn eval_outcome_debug_and_copy() {
        let outcome = EvalOutcome::Changed;
        let copied = outcome; // Copy
        assert_eq!(outcome, copied); // original still usable
        let debug = format!("{:?}", outcome);
        assert!(debug.contains("Changed"));
    }

    // --- NodeCache tests ---

    #[test]
    fn node_cache_construction() {
        use crate::deps::DependencyTrace;
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};

        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let expected_hash = result.content_hash();
        let cache = NodeCache::new(
            result.clone(),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        );

        assert_eq!(cache.result_hash, expected_hash);
        assert_eq!(cache.freshness, Freshness::Final);
        assert_eq!(cache.basis_version, VersionId(1));
    }

    #[test]
    fn node_cache_clone_and_debug() {
        use crate::deps::DependencyTrace;
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};

        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let cache = NodeCache::new(
            result,
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        );
        let cloned = cache.clone();
        assert_eq!(cache.result_hash, cloned.result_hash);
        assert_eq!(cache.basis_version, cloned.basis_version);

        let debug = format!("{:?}", cache);
        assert!(debug.contains("NodeCache"));
    }

    #[test]
    fn node_cache_result_hash_matches_content_hash() {
        use crate::deps::DependencyTrace;
        use reify_types::{Freshness, Satisfaction, VersionId};

        let result = CachedResult::Satisfaction(Satisfaction::Violated);
        let expected = result.content_hash();
        let cache = NodeCache::new(
            result,
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(5),
        );
        assert_eq!(cache.result_hash, expected);
    }

    // --- CachedResult tests ---

    #[test]
    fn cached_result_value_variant() {
        use reify_types::{DeterminacyState, Value};
        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("Value"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn cached_result_satisfaction_variant() {
        use reify_types::Satisfaction;
        let result = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("Satisfaction"));
    }

    #[test]
    fn cached_result_geometry_handle_variant() {
        use reify_types::GeometryHandleId;
        let result = CachedResult::GeometryHandle(GeometryHandleId(7));
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("GeometryHandle"));
    }

    #[test]
    fn cached_result_content_hash_value_variant() {
        use reify_types::{DeterminacyState, Value};
        let r1 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let r2 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        assert_eq!(r1.content_hash(), r2.content_hash());

        // Different value -> different hash
        let r3 = CachedResult::Value(Value::Int(99), DeterminacyState::Determined);
        assert_ne!(r1.content_hash(), r3.content_hash());

        // Same value, different determinacy -> different hash
        let r4 = CachedResult::Value(Value::Int(42), DeterminacyState::Undetermined);
        assert_ne!(r1.content_hash(), r4.content_hash());
    }

    #[test]
    fn cached_result_content_hash_satisfaction_variant() {
        use reify_types::Satisfaction;
        let r1 = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let r2 = CachedResult::Satisfaction(Satisfaction::Satisfied);
        assert_eq!(r1.content_hash(), r2.content_hash());

        let r3 = CachedResult::Satisfaction(Satisfaction::Violated);
        assert_ne!(r1.content_hash(), r3.content_hash());
    }

    #[test]
    fn cached_result_content_hash_geometry_variant() {
        use reify_types::GeometryHandleId;
        let r1 = CachedResult::GeometryHandle(GeometryHandleId(7));
        let r2 = CachedResult::GeometryHandle(GeometryHandleId(7));
        assert_eq!(r1.content_hash(), r2.content_hash());

        let r3 = CachedResult::GeometryHandle(GeometryHandleId(8));
        assert_ne!(r1.content_hash(), r3.content_hash());
    }

    #[test]
    fn cached_result_resolution_variant() {
        use reify_types::Value;
        use std::collections::HashMap;

        let mut values1 = HashMap::new();
        values1.insert(ValueCellId::new("A", "x"), Value::Real(1.0));
        let r1 = CachedResult::Resolution(values1.clone());

        let hash1 = r1.content_hash();
        assert_ne!(hash1, ContentHash(0), "resolution hash should be non-zero");

        // Same content → same hash
        let r1b = CachedResult::Resolution(values1);
        assert_eq!(r1.content_hash(), r1b.content_hash());

        // Tag byte [23] — verified by domain separation
        let mut values2 = HashMap::new();
        values2.insert(ValueCellId::new("A", "x"), Value::Real(2.0));
        let r2 = CachedResult::Resolution(values2);
        assert_ne!(
            r1.content_hash(),
            r2.content_hash(),
            "different values → different hash"
        );
    }

    #[test]
    fn cached_result_content_hash_domain_separation() {
        // Ensure different variants produce different hashes even with
        // "similar" inner data
        use reify_types::{DeterminacyState, GeometryHandleId, Satisfaction, Value};
        let val = CachedResult::Value(Value::Int(0), DeterminacyState::Determined);
        let sat = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let geo = CachedResult::GeometryHandle(GeometryHandleId(0));

        assert_ne!(val.content_hash(), sat.content_hash());
        assert_ne!(val.content_hash(), geo.content_hash());
        assert_ne!(sat.content_hash(), geo.content_hash());
    }

    // --- Early cutoff tests (Step 17) ---

    #[test]
    fn check_early_cutoff_returns_true_when_hash_matches() {
        let mut store = CacheStore::new();
        let id = ValueCellId::new("A", "x");
        let node = NodeId::Value(id.clone());

        // Cache entry with value hash
        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let cache = NodeCache::new(
            result,
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        );
        store.put(node.clone(), cache);

        // Same value hash -> early cutoff applies
        let same_hash = store.get(&node).unwrap().result_hash;
        let result = check_early_cutoff(&store, &id, same_hash);
        assert!(result, "should return true when hash matches");
    }

    #[test]
    fn check_early_cutoff_returns_false_when_hash_differs() {
        let mut store = CacheStore::new();
        let id = ValueCellId::new("A", "x");
        let node = NodeId::Value(id.clone());

        // Cache entry with value hash
        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let cache = NodeCache::new(
            result,
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        );
        store.put(node.clone(), cache);

        // Different value hash -> no early cutoff
        let different_hash = ContentHash::of_str("different");
        let result = check_early_cutoff(&store, &id, different_hash);
        assert!(!result, "should return false when hash differs");
    }

    #[test]
    fn check_early_cutoff_returns_false_for_missing_entry() {
        let store = CacheStore::new();
        let id = ValueCellId::new("A", "nonexistent");
        let hash = ContentHash::of_str("any");

        let result = check_early_cutoff(&store, &id, hash);
        assert!(!result, "should return false when entry doesn't exist");
    }

    // --- Integration tests ---

    #[test]
    fn cold_start_cache_miss() {
        use reify_test_support::builders::*;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_types::{BinOp, ModulePath, Type, VersionId};

        let e = "T";
        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("T")
                    .param(e, "a", Type::Int, Some(literal(Value::Int(10))))
                    .param(e, "b", Type::Int, Some(literal(Value::Int(20))))
                    .let_binding(
                        e,
                        "x",
                        Type::Int,
                        binop(
                            BinOp::Add,
                            value_ref_typed(e, "a", Type::Int),
                            literal(Value::Int(1)),
                        ),
                    )
                    .build(),
            )
            .build();

        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), None);
        let result = engine.eval_cached(&module, VersionId(1));

        // All values computed (no cache hits on first run)
        assert_eq!(result.stats.cache_misses, 3); // a, b, x
        assert_eq!(result.stats.cache_hits, 0);

        // Cache should have entries for all 3 value cells
        assert_eq!(engine.cache_store().len(), 3);

        // Values should be correct
        assert_eq!(
            result.eval_result.values.get(&ValueCellId::new(e, "a")),
            Some(&Value::Int(10))
        );
        assert_eq!(
            result.eval_result.values.get(&ValueCellId::new(e, "x")),
            Some(&Value::Int(11))
        );
    }

    #[test]
    fn version_fast_path_100_percent_hits() {
        use reify_test_support::builders::*;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_types::{BinOp, ModulePath, Type, VersionId};

        let e = "T";
        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("T")
                    .param(e, "a", Type::Int, Some(literal(Value::Int(10))))
                    .param(e, "b", Type::Int, Some(literal(Value::Int(20))))
                    .let_binding(
                        e,
                        "x",
                        Type::Int,
                        binop(
                            BinOp::Add,
                            value_ref_typed(e, "a", Type::Int),
                            literal(Value::Int(1)),
                        ),
                    )
                    .build(),
            )
            .build();

        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), None);

        // First call: populates cache
        let result1 = engine.eval_cached(&module, VersionId(1));
        assert_eq!(result1.stats.cache_misses, 3);

        // Second call with same version: 100% cache hits
        let result2 = engine.eval_cached(&module, VersionId(1));
        assert_eq!(result2.stats.cache_hits, 3);
        assert_eq!(result2.stats.cache_misses, 0);

        // Results should be identical
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "a")),
            Some(&Value::Int(10))
        );
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "x")),
            Some(&Value::Int(11))
        );
    }

    #[test]
    fn selective_re_evaluation_on_param_change() {
        use reify_test_support::builders::*;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_types::{BinOp, ModulePath, Type, VersionId};

        let e = "T";
        // param a = 10, param b = 20, let x = a + 1, let y = b + 1
        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("T")
                    .param(e, "a", Type::Int, Some(literal(Value::Int(10))))
                    .param(e, "b", Type::Int, Some(literal(Value::Int(20))))
                    .let_binding(
                        e,
                        "x",
                        Type::Int,
                        binop(
                            BinOp::Add,
                            value_ref_typed(e, "a", Type::Int),
                            literal(Value::Int(1)),
                        ),
                    )
                    .let_binding(
                        e,
                        "y",
                        Type::Int,
                        binop(
                            BinOp::Add,
                            value_ref_typed(e, "b", Type::Int),
                            literal(Value::Int(1)),
                        ),
                    )
                    .build(),
            )
            .build();

        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), None);

        // First eval: populate cache
        let result1 = engine.eval_cached(&module, VersionId(1));
        assert_eq!(result1.stats.cache_misses, 4); // a, b, x, y

        // Change param a -> invalidate its dependents
        engine.set_param_and_invalidate(&ValueCellId::new(e, "a"), Value::Int(15));

        // Re-evaluate with new version
        let result2 = engine.eval_cached(&module, VersionId(2));

        // a should be re-evaluated (always, since param), x should be
        // re-evaluated (depends on a, was invalidated)
        // b and y should be served from cache (not dependent on a)
        assert!(result2.stats.cache_hits >= 2); // b and y from cache
        assert!(result2.stats.cache_misses >= 2); // a and x re-evaluated

        // Results should be correct
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "a")),
            Some(&Value::Int(15))
        );
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "x")),
            Some(&Value::Int(16)) // 15 + 1
        );
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "b")),
            Some(&Value::Int(20))
        );
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "y")),
            Some(&Value::Int(21))
        );
    }

    #[test]
    fn early_cutoff_prevents_downstream_re_evaluation() {
        use reify_test_support::builders::*;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_types::{
            BinOp, CompiledExpr, CompiledExprKind, ContentHash, ModulePath, Type, VersionId,
        };

        let e = "T";

        // Build conditional: if a > 0 then 1 else 1 (always 1)
        let condition = gt(value_ref_typed(e, "a", Type::Int), literal(Value::Int(0)));
        let then_branch = literal(Value::Int(1));
        let else_branch = literal(Value::Int(1));
        let conditional = CompiledExpr {
            kind: CompiledExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch.clone()),
                else_branch: Box::new(else_branch),
            },
            result_type: Type::Int,
            content_hash: ContentHash::of_str("if_a_gt_0_then_1_else_1"),
        };

        // let y = x + 100
        let y_expr = binop(
            BinOp::Add,
            value_ref_typed(e, "x", Type::Int),
            literal(Value::Int(100)),
        );

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("T")
                    .param(e, "a", Type::Int, Some(literal(Value::Int(5))))
                    .let_binding(e, "x", Type::Int, conditional)
                    .let_binding(e, "y", Type::Int, y_expr)
                    .build(),
            )
            .build();

        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), None);

        // First eval: all computed
        let result1 = engine.eval_cached(&module, VersionId(1));
        assert_eq!(result1.stats.cache_misses, 3); // a, x, y
        assert_eq!(
            result1.eval_result.values.get(&ValueCellId::new(e, "x")),
            Some(&Value::Int(1))
        );
        assert_eq!(
            result1.eval_result.values.get(&ValueCellId::new(e, "y")),
            Some(&Value::Int(101))
        );

        // Change a from 5 to 10 (still > 0, so x still = 1)
        engine.set_param_and_invalidate(&ValueCellId::new(e, "a"), Value::Int(10));

        // Re-evaluate
        let result2 = engine.eval_cached(&module, VersionId(2));

        // x should be re-evaluated but result unchanged (early cutoff)
        assert!(
            result2.stats.early_cutoffs >= 1,
            "expected early cutoff for x"
        );

        // y should NOT be re-evaluated (served from cache because x didn't change)
        // The total cache_hits should include y
        assert!(
            result2.stats.cache_hits >= 1,
            "expected y served from cache"
        );

        // Results still correct
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "a")),
            Some(&Value::Int(10))
        );
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "x")),
            Some(&Value::Int(1))
        );
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "y")),
            Some(&Value::Int(101))
        );
    }

    #[test]
    fn diamond_dependency_early_cutoff_correctness() {
        use reify_test_support::builders::*;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_types::{
            BinOp, CompiledExpr, CompiledExprKind, ContentHash, ModulePath, Type, VersionId,
        };

        let e = "T";

        // Build conditional: if a > 0 then 1 else 1 (always 1, reads: [a])
        let condition = gt(value_ref_typed(e, "a", Type::Int), literal(Value::Int(0)));
        let then_branch = literal(Value::Int(1));
        let else_branch = literal(Value::Int(1));
        let conditional = CompiledExpr {
            kind: CompiledExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch.clone()),
                else_branch: Box::new(else_branch),
            },
            result_type: Type::Int,
            content_hash: ContentHash::of_str("if_a_gt_0_then_1_else_1"),
        };

        // let y = a + x (reads: [a, x]) — diamond dependency
        let y_expr = binop(
            BinOp::Add,
            value_ref_typed(e, "a", Type::Int),
            value_ref_typed(e, "x", Type::Int),
        );

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("T")
                    .param(e, "a", Type::Int, Some(literal(Value::Int(5))))
                    .let_binding(e, "x", Type::Int, conditional)
                    .let_binding(e, "y", Type::Int, y_expr)
                    .build(),
            )
            .build();

        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), None);

        // First eval: all computed. a=5, x=1, y=5+1=6
        let result1 = engine.eval_cached(&module, VersionId(1));
        assert_eq!(result1.stats.cache_misses, 3); // a, x, y
        assert_eq!(
            result1.eval_result.values.get(&ValueCellId::new(e, "a")),
            Some(&Value::Int(5))
        );
        assert_eq!(
            result1.eval_result.values.get(&ValueCellId::new(e, "x")),
            Some(&Value::Int(1))
        );
        assert_eq!(
            result1.eval_result.values.get(&ValueCellId::new(e, "y")),
            Some(&Value::Int(6))
        );

        // Change a from 5 to 10 (still > 0, so x still = 1)
        engine.set_param_and_invalidate(&ValueCellId::new(e, "a"), Value::Int(10));

        // Re-evaluate
        let result2 = engine.eval_cached(&module, VersionId(2));

        // x should early-cutoff (result unchanged: still 1)
        assert!(
            result2.stats.early_cutoffs >= 1,
            "expected early cutoff for x"
        );

        // CRITICAL: y MUST be re-evaluated because it reads a directly,
        // even though x early-cutoff'd. y = 10 + 1 = 11 (not stale 6).
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "a")),
            Some(&Value::Int(10))
        );
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "x")),
            Some(&Value::Int(1))
        );
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "y")),
            Some(&Value::Int(11)), // 10 + 1, NOT stale 6
        );
    }

    #[test]
    fn triple_fan_in_dirty_reasons_multiple_independent_reasons() {
        use reify_test_support::builders::*;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_types::{
            BinOp, CompiledExpr, CompiledExprKind, ContentHash, ModulePath, Type, VersionId,
        };

        let e = "T";

        // Build conditional: if a > 0 then 1 else 1 (always 1, reads: [a])
        let condition = gt(value_ref_typed(e, "a", Type::Int), literal(Value::Int(0)));
        let then_branch = literal(Value::Int(1));
        let else_branch = literal(Value::Int(1));
        let conditional = CompiledExpr {
            kind: CompiledExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch.clone()),
                else_branch: Box::new(else_branch),
            },
            result_type: Type::Int,
            content_hash: ContentHash::of_str("if_a_gt_0_then_1_else_1"),
        };

        // let y = (a + b) + x (reads: [a, b, x]) — triple fan-in
        let y_expr = binop(
            BinOp::Add,
            binop(
                BinOp::Add,
                value_ref_typed(e, "a", Type::Int),
                value_ref_typed(e, "b", Type::Int),
            ),
            value_ref_typed(e, "x", Type::Int),
        );

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("T")
                    .param(e, "a", Type::Int, Some(literal(Value::Int(5))))
                    .param(e, "b", Type::Int, Some(literal(Value::Int(100))))
                    .let_binding(e, "x", Type::Int, conditional)
                    .let_binding(e, "y", Type::Int, y_expr)
                    .build(),
            )
            .build();

        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), None);

        // First eval: a=5, b=100, x=1, y=5+100+1=106
        let result1 = engine.eval_cached(&module, VersionId(1));
        assert_eq!(result1.stats.cache_misses, 4); // a, b, x, y
        assert_eq!(
            result1.eval_result.values.get(&ValueCellId::new(e, "y")),
            Some(&Value::Int(106))
        );

        // Change only a from 5 to 10. x early-cutoffs.
        // y reads [a, b, x]. Dirty reason for y: {a}.
        // x early-cutoff removes x-reason (but x wasn't a reason).
        // y stays dirty because reason {a} is unresolved.
        engine.set_param_and_invalidate(&ValueCellId::new(e, "a"), Value::Int(10));

        let result2 = engine.eval_cached(&module, VersionId(2));
        assert!(
            result2.stats.early_cutoffs >= 1,
            "expected early cutoff for x"
        );
        // y must be re-evaluated: y = 10 + 100 + 1 = 111
        assert_eq!(
            result2.eval_result.values.get(&ValueCellId::new(e, "y")),
            Some(&Value::Int(111))
        );

        // Now change b from 100 to 200. x doesn't depend on b.
        // y reads [a, b, x]. Dirty reason for y: {b}.
        engine.set_param_and_invalidate(&ValueCellId::new(e, "b"), Value::Int(200));

        let result3 = engine.eval_cached(&module, VersionId(3));
        // y must be re-evaluated because of b: y = 10 + 200 + 1 = 211
        assert_eq!(
            result3.eval_result.values.get(&ValueCellId::new(e, "y")),
            Some(&Value::Int(211))
        );
    }

    // --- mark_pending tests ---

    #[test]
    fn cache_mark_pending_sets_freshness() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        let cache = make_test_node_cache(42, 1);
        let original_hash = cache.result_hash;
        store.put(node.clone(), cache);

        // Verify initially Final
        assert_eq!(store.get(&node).unwrap().freshness, Freshness::Final);

        // Mark pending
        let marked = store.mark_pending(&node);
        assert!(marked);

        // Freshness should now be Pending with last_substantive
        let entry = store.get(&node).unwrap();
        assert_eq!(
            entry.freshness,
            Freshness::Pending {
                last_substantive: ResultRef::of_hash(original_hash)
            }
        );
    }

    #[test]
    fn cache_mark_pending_preserves_result() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        let cache = make_test_node_cache(42, 1);
        store.put(node.clone(), cache);

        store.mark_pending(&node);

        // Result, result_hash, dependency_trace, and basis_version should be unchanged
        let entry = store.get(&node).unwrap();
        if let CachedResult::Value(val, _) = &entry.result {
            assert_eq!(*val, Value::Int(42));
        } else {
            panic!("expected Value variant");
        }
        assert_eq!(entry.basis_version, VersionId(1));
    }

    #[test]
    fn cache_mark_pending_uncached_returns_false() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "missing"));
        let marked = store.mark_pending(&node);
        assert!(!marked);
    }

    // --- warm_state on NodeCache tests ---

    #[test]
    fn node_cache_new_creates_entry_with_no_warm_state() {
        let cache = NodeCache::new(
            CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        );
        assert!(cache.warm_state.is_none());
    }

    #[test]
    fn node_cache_warm_state_can_be_set() {
        let mut cache = NodeCache::new(
            CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        );
        let state = reify_types::OpaqueState::new(42i32, 4);
        cache.warm_state = Some(state);
        assert!(cache.warm_state.is_some());
        let val = cache.warm_state.unwrap().downcast::<i32>();
        assert_eq!(val, Some(42));
    }

    #[test]
    fn cache_store_put_get_preserves_warm_state() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        let mut cache = NodeCache::new(
            CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        );
        cache.warm_state = Some(reify_types::OpaqueState::new(99i32, 4));
        store.put(node.clone(), cache);

        let entry = store.get(&node).unwrap();
        assert!(entry.warm_state.is_some());
        // Use downcast_ref to check without consuming
        let val = entry.warm_state.as_ref().unwrap().downcast_ref::<i32>();
        assert_eq!(val, Some(&99));
    }

    // --- CacheStore warm-state helper tests ---

    #[test]
    fn donate_warm_state_stores_and_get_retrieves_with_take_semantics() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        store.put(node.clone(), make_test_node_cache(42, 1));

        let state = reify_types::OpaqueState::new(100i32, 4);
        let donated = store.donate_warm_state(&node, state);
        assert!(donated);

        // First get: returns the state
        let retrieved = store.get_warm_state(&node);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().downcast::<i32>(), Some(100));

        // Second get: take semantics — returns None
        let retrieved2 = store.get_warm_state(&node);
        assert!(retrieved2.is_none());
    }

    #[test]
    fn donate_warm_state_on_nonexistent_node_returns_false() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "missing"));
        let state = reify_types::OpaqueState::new(42i32, 4);
        let donated = store.donate_warm_state(&node, state);
        assert!(!donated);
    }

    #[test]
    fn insert_synthetic_realization_entry_creates_donatable_entry() {
        let mut store = CacheStore::new();
        let rid = RealizationNodeId::new("Bracket", 0);
        let node = NodeId::Realization(rid.clone());

        // Verifies the documented contract: entry exists under NodeId::Realization(rid)
        // with a CachedResult::GeometryHandle placeholder, and donate_warm_state returns true.
        store.insert_synthetic_realization_entry(&rid);

        // (a) Entry must exist under NodeId::Realization(rid) with a
        //     CachedResult::GeometryHandle(_) placeholder payload.
        let entry = store
            .get(&node)
            .expect("insert_synthetic_realization_entry must create a cache entry");
        assert!(
            matches!(entry.result, CachedResult::GeometryHandle(_)),
            "synthetic entry result must be CachedResult::GeometryHandle (placeholder)"
        );

        // (b) The entry must accept warm state donation (entry exists → donate returns true).
        let donated =
            store.donate_warm_state(&node, reify_types::OpaqueState::new(0xBEEFu32, 8));
        assert!(
            donated,
            "donate_warm_state must return true for the synthetic realization entry"
        );
    }

    #[test]
    fn record_evaluation_clears_warm_state() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        store.put(node.clone(), make_test_node_cache(42, 1));

        // Donate warm state
        let state = reify_types::OpaqueState::new(100i32, 4);
        store.donate_warm_state(&node, state);
        assert!(store.get(&node).unwrap().warm_state.is_some());

        // Re-evaluate (same result = early cutoff)
        store.record_evaluation(
            node.clone(),
            CachedResult::Value(Value::Int(42), DeterminacyState::Determined),
            VersionId(2),
            DependencyTrace::default(),
        );

        // Warm state should be cleared after re-evaluation
        assert!(store.get(&node).unwrap().warm_state.is_none());
    }

    // --- NodeCache Clone invariant ---

    #[test]
    fn node_cache_clone_drops_warm_state() {
        // NodeCache has a manual Clone impl that intentionally sets warm_state: None.
        // Warm state is a transient optimization hint — it is not preserved across
        // clones. This test documents that invariant and guards against regressions
        // (e.g. if someone replaces the manual impl with #[derive(Clone)], which
        // would fail to compile since OpaqueState is not Clone — but documenting
        // intent is still valuable).
        let mut cache = make_test_node_cache(42, 1);
        cache.warm_state = Some(reify_types::OpaqueState::new(42i32, 4));
        assert!(
            cache.warm_state.is_some(),
            "precondition: warm_state must be Some before cloning"
        );

        let cloned = cache.clone();
        assert!(
            cloned.warm_state.is_none(),
            "NodeCache::clone must drop warm_state (transient hint, not preserved)"
        );
    }

    // --- CacheStore::set_freshness() tests (task #2326, step-5) ---

    #[test]
    fn cache_store_set_freshness_returns_false_for_missing_and_writes_for_present() {
        use reify_types::{ErrorRef, Freshness};

        // (a) Missing node → set_freshness returns false and store stays empty (no auto-create).
        let mut store = CacheStore::new();
        let missing = NodeId::Value(ValueCellId::new("T", "missing"));
        let result = store.set_freshness(&missing, Freshness::Intermediate { generation: 1 });
        assert!(!result, "set_freshness on absent node must return false");
        assert_eq!(
            store.len(),
            0,
            "set_freshness must not auto-create a cache entry"
        );

        // (b) Present node → set_freshness returns true.
        let node = NodeId::Value(ValueCellId::new("T", "present"));
        store.put(node.clone(), make_test_node_cache(42, 1)); // starts with Freshness::Final
        let result = store.set_freshness(
            &node,
            Freshness::Failed {
                error: ErrorRef::new("boom"),
            },
        );
        assert!(result, "set_freshness on present node must return true");

        // (c) Round-trip: canonical reader reflects the written value.
        assert_eq!(
            store.freshness(&node),
            Freshness::Failed {
                error: ErrorRef::new("boom"),
            },
            "freshness() must read back the value written by set_freshness()"
        );
    }

    // --- set_freshness precondition: Pending is forbidden (task #2451, step-1) ---

    /// Pins the `set_freshness` precondition guard (S1): passing `Freshness::Pending`
    /// must panic in all builds. Callers must use `mark_pending` or
    /// `mark_pending_with_cause` instead, which also derive `last_substantive` and
    /// increment `pending_transition_count`.
    #[test]
    #[should_panic(expected = "Pending")]
    fn set_freshness_panics_when_passed_pending() {
        use reify_types::{ContentHash, Freshness, ResultRef};

        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        store.put(node.clone(), make_test_node_cache(1, 1));

        // Must panic — set_freshness must not accept Pending.
        let _ = store.set_freshness(
            &node,
            Freshness::Pending {
                last_substantive: ResultRef::of_hash(ContentHash(0)),
            },
        );
    }

    // --- set_freshness precondition: Failed is forbidden (task #2592, step-1) ---

    /// Pins the `set_freshness` precondition guard (S1): passing `Freshness::Failed`
    /// must panic in all builds. Callers must use `mark_failed` instead, which also
    /// clears `pending_cause` (ensuring Failed nodes are chain roots per arch §9.2).
    /// Tasks #2451 + #2592.
    #[test]
    #[should_panic(expected = "Failed")]
    fn set_freshness_panics_when_passed_failed() {
        use reify_types::{ErrorRef, Freshness};

        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        store.put(node.clone(), make_test_node_cache(1, 1));

        // Must panic — set_freshness must not accept Failed.
        let _ = store.set_freshness(
            &node,
            Freshness::Failed {
                error: ErrorRef::new("boom"),
            },
        );
    }

    // --- derive_output_freshness tests (task #2328, step-1) ---

    /// Arch §7.2 (lines 730-749) truth table:
    ///   still_refining=true  → Intermediate{generation} always
    ///   still_refining=false, any input != Final → Intermediate{generation}
    ///   still_refining=false, all inputs == Final → Final
    #[test]
    fn derive_output_freshness_implements_arch_7_2_truth_table() {
        use reify_types::Freshness;
        let g = 7u64;

        // Row 1: still_refining=true, all inputs Final → Intermediate
        let inputs_all_final = [Freshness::Final, Freshness::Final];
        assert_eq!(
            derive_output_freshness(true, inputs_all_final.iter().cloned(), g),
            Freshness::Intermediate { generation: g },
            "still_refining=true, all-Final inputs → Intermediate"
        );

        // Row 2: still_refining=true, some input non-Final → Intermediate
        let inputs_with_non_final = [
            Freshness::Final,
            Freshness::Intermediate { generation: 3 },
        ];
        assert_eq!(
            derive_output_freshness(true, inputs_with_non_final.iter().cloned(), g),
            Freshness::Intermediate { generation: g },
            "still_refining=true, non-Final inputs → Intermediate"
        );

        // Row 3: still_refining=false, all inputs Final → Final
        assert_eq!(
            derive_output_freshness(false, inputs_all_final.iter().cloned(), g),
            Freshness::Final,
            "still_refining=false, all-Final inputs → Final"
        );

        // Row 4: still_refining=false, any input non-Final → Intermediate
        assert_eq!(
            derive_output_freshness(false, inputs_with_non_final.iter().cloned(), g),
            Freshness::Intermediate { generation: g },
            "still_refining=false, non-Final inputs → Intermediate"
        );

        // Edge case: no inputs, still_refining=false → Final
        assert_eq!(
            derive_output_freshness(false, std::iter::empty::<Freshness>(), g),
            Freshness::Final,
            "still_refining=false, empty inputs → Final"
        );

        // Edge case: no inputs, still_refining=true → Intermediate
        assert_eq!(
            derive_output_freshness(true, std::iter::empty::<Freshness>(), g),
            Freshness::Intermediate { generation: g },
            "still_refining=true, empty inputs → Intermediate"
        );
    }

    // --- record_evaluation_with_freshness tests (task #2328, step-7) ---

    /// Verifies that record_evaluation_with_freshness writes the supplied freshness
    /// (not hardcoded Final) and that early-cutoff still updates freshness in place.
    #[test]
    fn record_evaluation_with_freshness_writes_supplied_freshness_and_preserves_early_cutoff() {
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};

        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));

        // Sub-case 1: cold start with Intermediate freshness
        let result1 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let outcome1 = store.record_evaluation_with_freshness(
            node.clone(),
            result1,
            VersionId(1),
            DependencyTrace::default(),
            Freshness::Intermediate { generation: 5 },
        );
        assert_eq!(outcome1, EvalOutcome::Changed, "cold start must return Changed");
        assert_eq!(
            store.freshness(&node),
            Freshness::Intermediate { generation: 5 },
            "freshness must be the supplied Intermediate, not Final"
        );

        // Sub-case 2: early cutoff (same value, new version, different supplied freshness)
        let result2 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let outcome2 = store.record_evaluation_with_freshness(
            node.clone(),
            result2,
            VersionId(2),
            DependencyTrace::default(),
            Freshness::Intermediate { generation: 9 },
        );
        assert_eq!(
            outcome2,
            EvalOutcome::Unchanged,
            "same value content hash must return Unchanged (early cutoff)"
        );
        // Early-cutoff path must still update freshness to the new supplied value
        assert_eq!(
            store.freshness(&node),
            Freshness::Intermediate { generation: 9 },
            "early-cutoff must overwrite freshness with the newly supplied value"
        );
        // basis_version must be updated even on early cutoff
        assert_eq!(
            store.get(&node).unwrap().basis_version,
            VersionId(2),
            "basis_version must be updated even on early cutoff"
        );
    }

    // --- derive_output_freshness_for_node tests (task #2328, step-5) ---

    /// Verifies that derive_output_freshness_for_node walks the cached dependency_trace.reads
    /// for a let-cell and delegates to derive_output_freshness correctly.
    #[test]
    fn derive_output_freshness_for_node_walks_cached_dependency_trace() {
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};

        let mut store = CacheStore::new();

        let a_id = ValueCellId::new("T", "a");
        let b_id = ValueCellId::new("T", "b");
        let out_id = ValueCellId::new("T", "out");

        // Insert input cell 'a' with Final freshness
        store.put(
            NodeId::Value(a_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Insert input cell 'b' with Intermediate freshness
        store.put(
            NodeId::Value(b_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(2), DeterminacyState::Determined),
                Freshness::Intermediate { generation: 3 },
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Insert output let-cell whose dependency_trace.reads = [a, b]
        let mut out_trace = DependencyTrace::default();
        out_trace.reads.push(a_id.clone());
        out_trace.reads.push(b_id.clone());
        store.put(
            NodeId::Value(out_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(3), DeterminacyState::Determined),
                Freshness::Final,
                out_trace,
                VersionId(1),
            ),
        );

        // Case 1: 'b' is Intermediate → output should be Intermediate{7}
        let result = store.derive_output_freshness_for_node(
            &NodeId::Value(out_id.clone()),
            false,
            7,
        );
        assert_eq!(
            result,
            Freshness::Intermediate { generation: 7 },
            "one non-Final input (b=Intermediate) must yield Intermediate output"
        );

        // Case 2: make 'b' Final → output should be Final
        let _ = store.set_freshness(
            &NodeId::Value(b_id.clone()),
            Freshness::Final,
        );
        let result2 = store.derive_output_freshness_for_node(
            &NodeId::Value(out_id.clone()),
            false,
            7,
        );
        assert_eq!(
            result2,
            Freshness::Final,
            "all Final inputs must yield Final output"
        );
    }

    // --- derive_output_freshness §9.2 carve-out (task #2330 step-5) ---

    /// Pins the §9.2 carve-out (rewritten from the §7.2 truth-table coverage of task
    /// #2328): Failed and Pending inputs now produce a Pending output rather than
    /// Intermediate. Intermediate inputs continue to produce Intermediate.
    ///
    /// The cause-bearing variant is exercised by
    /// `derive_output_freshness_with_cause_returns_failing_node` below.
    #[test]
    fn derive_output_freshness_classifies_pending_and_failed_inputs_as_non_final() {
        use reify_types::{ErrorRef, Freshness, ResultRef};
        let g = 9u64;

        // Intermediate input → Intermediate output (§7.2 unchanged for this row).
        assert_eq!(
            derive_output_freshness(false, [Freshness::Intermediate { generation: 0 }].into_iter(), g),
            Freshness::Intermediate { generation: g },
            "Intermediate input must yield Intermediate output"
        );

        // Pending input → Pending output (§7.2 line 748 + §9.2 line 890 carve-out).
        // Chain forwarding is at the `_with_cause` layer; the pure helper drops the chain.
        assert_eq!(
            derive_output_freshness(false, [Freshness::Pending { last_substantive: ResultRef::none() }].into_iter(), g),
            Freshness::Pending { last_substantive: ResultRef::none() },
            "Pending input must yield Pending output per §7.2 line 748 (downstream subtree quieted)"
        );

        // Failed input → Pending output (§9.2 line 890 carve-out).
        assert_eq!(
            derive_output_freshness(false, [Freshness::Failed { error: ErrorRef::new("type mismatch") }].into_iter(), g),
            Freshness::Pending { last_substantive: ResultRef::none() },
            "Failed input must yield Pending output per §9.2 line 890 carve-out"
        );
    }

    /// Pins the cause-bearing variant `derive_output_freshness_with_cause`:
    /// Failed input contributes its own NodeId as the chain root; Pending input
    /// forwards the upstream entry's `pending_cause`; all-Final inputs return None.
    #[test]
    fn derive_output_freshness_with_cause_returns_failing_node() {
        use reify_types::{ErrorRef, Freshness, ResultRef};
        let g = 9u64;

        let leaf = NodeId::Value(ValueCellId::new("T", "leaf"));
        let mid = NodeId::Value(ValueCellId::new("T", "mid"));

        // (a) Failed input → (Pending{none()}, Some(failing_node))
        let (fresh, cause) = derive_output_freshness_with_cause(
            false,
            [(
                leaf.clone(),
                Freshness::Failed { error: ErrorRef::new("boom") },
                None,
            )],
            g,
        );
        assert_eq!(
            fresh,
            Freshness::Pending { last_substantive: ResultRef::none() },
            "Failed input must yield Pending output per §9.2"
        );
        assert_eq!(
            cause,
            Some(leaf.clone()),
            "Failed input must contribute its own NodeId as the chain root"
        );

        // (b) Pending input with upstream pending_cause → (Pending{...}, Some(leaf))
        // The mid node is itself Pending and carries pending_cause = Some(leaf).
        let (fresh2, cause2) = derive_output_freshness_with_cause(
            false,
            [(
                mid.clone(),
                Freshness::Pending { last_substantive: ResultRef::none() },
                Some(leaf.clone()),
            )],
            g,
        );
        assert!(
            matches!(fresh2, Freshness::Pending { .. }),
            "Pending input must yield Pending output"
        );
        assert_eq!(
            cause2,
            Some(leaf.clone()),
            "Pending input must forward the upstream pending_cause"
        );

        // (c) All-Final inputs → (Final, None)
        let (fresh3, cause3) = derive_output_freshness_with_cause(
            false,
            [(leaf.clone(), Freshness::Final, None)],
            g,
        );
        assert_eq!(fresh3, Freshness::Final, "All-Final inputs must yield Final");
        assert_eq!(cause3, None, "All-Final inputs have no chain cause");
    }

    // --- derive_output_freshness_from_trace tests (task #2328 amendment) ---

    /// Verifies that derive_output_freshness_from_trace uses the *supplied* trace
    /// (not whatever is cached for a node) and delegates to the §7.2 rule correctly.
    #[test]
    fn derive_output_freshness_from_trace_uses_supplied_trace() {
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};

        let mut store = CacheStore::new();

        let a_id = ValueCellId::new("T", "a");
        let b_id = ValueCellId::new("T", "b");

        // Insert `a` with Final freshness and `b` with Intermediate freshness
        store.put(
            NodeId::Value(a_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );
        store.put(
            NodeId::Value(b_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(2), DeterminacyState::Determined),
                Freshness::Intermediate { generation: 3 },
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // A trace that only reads `a` (Final) → should yield Final
        let trace_a_only = DependencyTrace { reads: vec![a_id.clone()] };
        assert_eq!(
            store.derive_output_freshness_from_trace(&trace_a_only, false, 7),
            Freshness::Final,
            "trace reading only Final input must yield Final"
        );

        // A trace that reads `b` (Intermediate) → should yield Intermediate
        let trace_b_only = DependencyTrace { reads: vec![b_id.clone()] };
        assert_eq!(
            store.derive_output_freshness_from_trace(&trace_b_only, false, 7),
            Freshness::Intermediate { generation: 7 },
            "trace reading Intermediate input must yield Intermediate"
        );

        // still_refining=true overrides all: even all-Final trace yields Intermediate
        assert_eq!(
            store.derive_output_freshness_from_trace(&trace_a_only, true, 7),
            Freshness::Intermediate { generation: 7 },
            "still_refining=true must always yield Intermediate regardless of inputs"
        );
    }

    /// Verifies that the early-cutoff path in record_evaluation_with_freshness
    /// overwrites dependency_trace with the newly supplied value.
    /// This behavior was already present before task #2328 (verified by reviewing
    /// the pre-2328 git history) and is intentional: even on early cutoff, the
    /// freshly-computed trace replaces the old one to keep the cache consistent.
    #[test]
    fn record_evaluation_with_freshness_early_cutoff_updates_trace() {
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};

        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        let a_id = ValueCellId::new("T", "a");
        let b_id = ValueCellId::new("T", "b");

        // Cold-start: trace reads `a`
        let trace_a = DependencyTrace { reads: vec![a_id.clone()] };
        store.record_evaluation_with_freshness(
            node.clone(),
            CachedResult::Value(Value::Int(42), DeterminacyState::Determined),
            VersionId(1),
            trace_a,
            Freshness::Final,
        );
        assert_eq!(
            store.get(&node).unwrap().dependency_trace.reads,
            vec![a_id.clone()],
            "after cold start, trace must contain the supplied reads"
        );

        // Early cutoff: same hash, but different trace (reads `b` now)
        let trace_b = DependencyTrace { reads: vec![b_id.clone()] };
        let outcome = store.record_evaluation_with_freshness(
            node.clone(),
            CachedResult::Value(Value::Int(42), DeterminacyState::Determined),
            VersionId(2),
            trace_b,
            Freshness::Final,
        );
        assert_eq!(outcome, EvalOutcome::Unchanged, "same hash must trigger early cutoff");
        // Early-cutoff path must still overwrite the trace with the newly supplied one
        assert_eq!(
            store.get(&node).unwrap().dependency_trace.reads,
            vec![b_id.clone()],
            "early-cutoff must overwrite dependency_trace with the newly supplied trace"
        );
    }

    // --- record_evaluation_propagating_freshness tests (task #2328 amendment) ---

    /// Verifies that record_evaluation_propagating_freshness derives freshness from the
    /// supplied trace (not the old cached trace) and writes it atomically.
    #[test]
    fn record_evaluation_propagating_freshness_derives_from_supplied_trace() {
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};

        let mut store = CacheStore::new();

        let a_id = ValueCellId::new("T", "a");
        let out_id = ValueCellId::new("T", "out");

        // Insert `a` with Intermediate freshness
        store.put(
            NodeId::Value(a_id.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Real(5.0), DeterminacyState::Determined),
                Freshness::Intermediate { generation: 2 },
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Call record_evaluation_propagating_freshness with a trace that reads `a`.
        // version=7 → generation derived as version.0=7 per §7.1 (single source of truth).
        let trace = DependencyTrace { reads: vec![a_id.clone()] };
        let out_node = NodeId::Value(out_id.clone());
        let outcome = store.record_evaluation_propagating_freshness(
            out_node.clone(),
            CachedResult::Value(Value::Real(10.0), DeterminacyState::Determined),
            VersionId(7), // generation = version.0 = 7
            trace,
            false, // still_refining
        );
        assert_eq!(outcome, EvalOutcome::Changed, "cold start must return Changed");
        // Freshness must be derived from `a`'s Intermediate state → Intermediate{generation: 7}
        assert_eq!(
            store.freshness(&out_node),
            Freshness::Intermediate { generation: 7 },
            "freshness must be derived from the supplied trace's input (a=Intermediate); generation=version.0=7"
        );
    }

    // --- CacheStore::freshness() tests (task #2326, step-3) ---

    #[test]
    fn cache_store_freshness_reader_defaults_final_for_missing_and_returns_cached_for_present() {
        use reify_types::{Freshness, VersionId};

        // (a) Absent node → freshness() must return Freshness::Final (via Default).
        let store = CacheStore::new();
        let missing = NodeId::Value(ValueCellId::new("T", "missing"));
        assert_eq!(
            store.freshness(&missing),
            Freshness::Final,
            "freshness() on absent node must return Freshness::Final (the type-level default)"
        );

        // (b) Present node with Intermediate freshness → freshness() returns the cached value.
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "present"));
        store.put(
            node.clone(),
            NodeCache::new(
                CachedResult::Value(
                    reify_types::Value::Int(1),
                    reify_types::DeterminacyState::Determined,
                ),
                Freshness::Intermediate { generation: 7 },
                DependencyTrace::default(),
                VersionId(1),
            ),
        );
        assert_eq!(
            store.freshness(&node),
            Freshness::Intermediate { generation: 7 },
            "freshness() must return the stored Freshness variant for a present node"
        );
    }

    // --- pending_cause / mark_failed / mark_pending_with_cause tests (task #2330 step-3) ---
    //
    // These pin the diagnostic-chain side-table on `NodeCache` and the new
    // mark_failed / mark_pending_with_cause helpers. The chain is stored as a
    // `pending_cause: Option<NodeId>` field on `NodeCache` (NOT on
    // `Freshness::Pending` — see plan §1 design decision).

    fn make_seed_entry() -> NodeCache {
        NodeCache::new(
            CachedResult::Value(
                reify_types::Value::Int(42),
                reify_types::DeterminacyState::Determined,
            ),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        )
    }

    #[test]
    fn node_cache_new_defaults_pending_cause_to_none() {
        let entry = make_seed_entry();
        assert_eq!(
            entry.pending_cause, None,
            "NodeCache::new must default pending_cause to None"
        );
    }

    #[test]
    fn cache_store_pending_cause_returns_none_for_absent_and_reflects_set() {
        let mut store = CacheStore::new();
        let leaf = NodeId::Value(ValueCellId::new("T", "leaf"));
        let mid = NodeId::Value(ValueCellId::new("T", "mid"));

        // (a) Absent → None.
        assert_eq!(
            store.pending_cause(&mid),
            None,
            "pending_cause on absent node must return None"
        );

        // (b) Present but not chained → None.
        store.put(mid.clone(), make_seed_entry());
        assert_eq!(
            store.pending_cause(&mid),
            None,
            "pending_cause on a present-but-unchained entry must return None"
        );

        // (c) After set, reflects the cause NodeId.
        // Use mark_pending_with_cause as the canonical writer.
        let leaf_id = ValueCellId::new("T", "leaf");
        let mid_id = ValueCellId::new("T", "mid");
        // Insert leaf so it has a state to drive the chain (its presence isn't required by
        // mark_pending_with_cause but mirrors realistic call sites).
        store.put(NodeId::Value(leaf_id.clone()), make_seed_entry());
        let _ = leaf;
        let _ = mid;
        let mid_node = NodeId::Value(mid_id);
        let leaf_node = NodeId::Value(leaf_id);
        assert!(
            store.mark_pending_with_cause(&mid_node, leaf_node.clone()),
            "mark_pending_with_cause must return true for an existing entry"
        );
        assert_eq!(
            store.pending_cause(&mid_node),
            Some(leaf_node),
            "pending_cause must reflect the cause NodeId after mark_pending_with_cause"
        );
    }

    #[test]
    fn mark_failed_sets_failed_freshness_and_returns_true_only_for_existing() {
        use reify_types::ErrorRef;

        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "boom"));

        // Absent entry → returns false, no state mutation.
        assert!(
            !store.mark_failed(&node, ErrorRef::new("boom")),
            "mark_failed on absent node must return false"
        );

        // Present entry → flips freshness to Failed and returns true.
        store.put(node.clone(), make_seed_entry());
        assert!(
            store.mark_failed(&node, ErrorRef::new("boom")),
            "mark_failed on existing node must return true"
        );
        assert_eq!(
            store.freshness(&node),
            Freshness::Failed {
                error: ErrorRef::new("boom"),
            },
            "mark_failed must set freshness to Failed {{ error }}"
        );
    }

    #[test]
    fn mark_pending_with_cause_sets_pending_freshness_cause_and_bumps_counter() {
        use reify_types::ResultRef;

        let mut store = CacheStore::new();
        let mid_node = NodeId::Value(ValueCellId::new("T", "mid"));
        let leaf_node = NodeId::Value(ValueCellId::new("T", "leaf"));

        // Absent entry → returns false, no counter bump.
        let before = store.pending_transition_count();
        assert!(
            !store.mark_pending_with_cause(&mid_node, leaf_node.clone()),
            "mark_pending_with_cause on absent node must return false"
        );
        assert_eq!(
            store.pending_transition_count(),
            before,
            "absent-node call must not bump pending_transition_count"
        );

        // Present entry → flips freshness, sets cause, bumps counter.
        store.put(mid_node.clone(), make_seed_entry());
        let entry = store
            .get(&mid_node)
            .expect("seeded entry must be present");
        let prev_hash = entry.result_hash;

        assert!(
            store.mark_pending_with_cause(&mid_node, leaf_node.clone()),
            "mark_pending_with_cause on existing node must return true"
        );
        assert_eq!(
            store.freshness(&mid_node),
            Freshness::Pending {
                last_substantive: ResultRef::of_hash(prev_hash),
            },
            "freshness must be Pending with last_substantive derived from prior result_hash"
        );
        assert_eq!(
            store.pending_cause(&mid_node),
            Some(leaf_node),
            "pending_cause must equal the cause NodeId passed in"
        );
        assert_eq!(
            store.pending_transition_count(),
            before + 1,
            "mark_pending_with_cause must bump pending_transition_count by 1"
        );
    }

    /// Pin that the in-place freshness mutators reset the side-table
    /// `pending_cause` so the diagnostic chain cannot leak across
    /// freshness transitions.
    ///
    /// Per arch §9.2: a Failed node is a chain root, not a forwarder —
    /// the `pending_cause()` reader's documented contract (see
    /// cache.rs around `pub fn pending_cause(...)`) is that Failed
    /// entries return `None`. The bulk dirty-pass `mark_pending` helper
    /// must likewise wipe any stale cause carried over from a prior
    /// round so the per-node evaluator can decide afresh whether to
    /// re-attach a cause via `mark_pending_with_cause`.
    ///
    /// Regression test for task #2330 step-19/step-20 — guards against
    /// a §9.2 chain pointing at a node that has already been
    /// re-evaluated successfully on a later round.
    #[test]
    fn mark_pending_and_mark_failed_clear_stale_pending_cause() {
        use reify_types::ErrorRef;

        let mut store = CacheStore::new();
        let mid = NodeId::Value(ValueCellId::new("T", "mid"));
        let leaf_a = NodeId::Value(ValueCellId::new("T", "leaf_a"));
        let leaf_b = NodeId::Value(ValueCellId::new("T", "leaf_b"));

        // (a) Seed the entry with a real result_hash so mark_pending_with_cause
        //     has something to derive last_substantive from.
        store.put(mid.clone(), make_seed_entry());

        // (b) mark_pending_with_cause(mid, leaf_a) → cause == Some(leaf_a).
        assert!(
            store.mark_pending_with_cause(&mid, leaf_a.clone()),
            "mark_pending_with_cause must succeed on existing entry"
        );
        assert_eq!(
            store.pending_cause(&mid),
            Some(leaf_a.clone()),
            "pending_cause must be Some(leaf_a) after mark_pending_with_cause(leaf_a)"
        );

        // (c) mark_failed(mid, "boom") must transition to Failed AND clear
        //     pending_cause. Failed nodes are chain roots, not forwarders —
        //     their pending_cause must always read as None per the
        //     pending_cause() reader contract.
        assert!(
            store.mark_failed(&mid, ErrorRef::new("boom")),
            "mark_failed must succeed on existing entry"
        );
        assert!(
            matches!(store.freshness(&mid), Freshness::Failed { .. }),
            "freshness must be Failed after mark_failed"
        );
        assert_eq!(
            store.pending_cause(&mid),
            None,
            "mark_failed must clear pending_cause — Failed nodes are chain roots, not forwarders (arch §9.2)"
        );

        // (d) mark_pending_with_cause(mid, leaf_b) re-attaches a fresh cause.
        assert!(
            store.mark_pending_with_cause(&mid, leaf_b.clone()),
            "mark_pending_with_cause must succeed on existing entry"
        );
        assert_eq!(
            store.pending_cause(&mid),
            Some(leaf_b.clone()),
            "pending_cause must be Some(leaf_b) after re-attach"
        );

        // (e) The no-cause bulk-pass helper mark_pending(mid) must wipe
        //     the stale chain so a later per-node evaluator can decide
        //     afresh whether to re-attach a cause via mark_pending_with_cause.
        assert!(
            store.mark_pending(&mid),
            "mark_pending must succeed on existing entry"
        );
        assert_eq!(
            store.pending_cause(&mid),
            None,
            "mark_pending must clear pending_cause — the bulk dirty-pass helper cannot leak a stale chain across rounds (task #2330 §9.2)"
        );
    }

    // --- S3 agreement: no-cause variants == .0 of with_cause variants (task #2451 step-3) ---

    /// Smoke-checks that the no-cause cache methods return exactly `.0` of their
    /// `_with_cause` cousins. After the step-4 refactor the no-cause variants are
    /// `let (f, _) = self._with_cause(...); f` wrappers, so full truth-table
    /// coverage would be a tautology. Two representative rows suffice to catch a
    /// future regression where a method is accidentally hand-rolled again.
    #[test]
    fn derive_output_freshness_no_cause_variants_agree_with_with_cause() {
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};

        let a_id = ValueCellId::new("T", "a");
        let b_id = ValueCellId::new("T", "b");
        let out_id = ValueCellId::new("T", "out");

        // Helper: build a fresh store with `out` having trace = [a, b].
        let make_store = |a_fresh: Freshness, b_fresh: Freshness| -> CacheStore {
            let mut store = CacheStore::new();
            store.put(
                NodeId::Value(a_id.clone()),
                NodeCache::new(
                    CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
                    a_fresh,
                    DependencyTrace::default(),
                    VersionId(1),
                ),
            );
            store.put(
                NodeId::Value(b_id.clone()),
                NodeCache::new(
                    CachedResult::Value(Value::Int(2), DeterminacyState::Determined),
                    b_fresh,
                    DependencyTrace::default(),
                    VersionId(1),
                ),
            );
            let mut out_trace = DependencyTrace::default();
            out_trace.reads.push(a_id.clone());
            out_trace.reads.push(b_id.clone());
            store.put(
                NodeId::Value(out_id.clone()),
                NodeCache::new(
                    CachedResult::Value(Value::Int(3), DeterminacyState::Determined),
                    Freshness::Final,
                    out_trace,
                    VersionId(1),
                ),
            );
            store
        };

        let out = NodeId::Value(out_id.clone());
        let sr = false;
        let g = 5u64;

        macro_rules! assert_agree {
            ($store:expr, $trace:expr, $sr:expr, $g:expr, $label:expr) => {{
                let for_node = $store.derive_output_freshness_for_node(&out, $sr, $g);
                let for_node_cause = $store.derive_output_freshness_for_node_with_cause(&out, $sr, $g).0;
                assert_eq!(
                    for_node, for_node_cause,
                    "derive_output_freshness_for_node vs _with_cause disagree for {}",
                    $label
                );
                let from_trace = $store.derive_output_freshness_from_trace($trace, $sr, $g);
                let from_trace_cause = $store.derive_output_freshness_from_trace_with_cause($trace, $sr, $g).0;
                assert_eq!(
                    from_trace, from_trace_cause,
                    "derive_output_freshness_from_trace vs _with_cause disagree for {}",
                    $label
                );
            }};
        }

        // Row 1: all-Final (simplest path through the classifier)
        {
            let store = make_store(Freshness::Final, Freshness::Final);
            let trace = DependencyTrace { reads: vec![a_id.clone(), b_id.clone()] };
            assert_agree!(store, &trace, sr, g, "all-Final");
        }

        // Row 2: one Pending (non-trivial classifier path; exercises the main divergence risk)
        {
            let mut store = make_store(Freshness::Final, Freshness::Final);
            store.mark_pending(&NodeId::Value(b_id.clone()));
            let trace = DependencyTrace { reads: vec![a_id.clone(), b_id.clone()] };
            assert_agree!(store, &trace, sr, g, "one-Pending");
        }
    }
}
