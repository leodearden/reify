use std::collections::HashMap;
use std::fmt;

use reify_core::{ComputeNodeId, ConstraintNodeId, ContentHash, RealizationNodeId, ResolutionNodeId, ValueCellId, VersionId};
use reify_ir::{CompiledExpr, DeterminacyState, Freshness, GeometryHandleId, OpaqueState, ResultRef, Satisfaction, Value, ValueMap};

use crate::deps::DependencyTrace;

/// Unified identifier for any node in the evaluation graph.
/// Used as the key in the cache store.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeId {
    Value(ValueCellId),
    Constraint(ConstraintNodeId),
    Realization(RealizationNodeId),
    Resolution(ResolutionNodeId),
    /// P3.3: a ComputeNode (e.g. an @optimized FEA/solver computation).
    /// Added so the reverse-dependency index can register VC→Compute and
    /// Realization→Compute edges as `Set<NodeId>` dependents, and so the
    /// dirty-cone / freshness walks can propagate through ComputeNodes.
    Compute(ComputeNodeId),
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

impl From<ComputeNodeId> for NodeId {
    fn from(id: ComputeNodeId) -> Self {
        NodeId::Compute(id)
    }
}

/// Bridge from [`NodeId`] to the canonical [`reify_types::NodeKind`] discriminant.
///
/// This impl lives in `reify-eval` (not `reify-runtime` or `reify-types`) because
/// it is the unique orphan-rule-clean host: `NodeId` is local to this crate, and
/// RFC 2451 permits `impl From<&LocalType> for ForeignType` when the local type
/// appears in the `From` argument. See `docs/prds/v0_3/node-traits-unification.md §4`.
impl From<&NodeId> for reify_ir::NodeKind {
    fn from(node_id: &NodeId) -> Self {
        match node_id {
            NodeId::Value(_) => Self::Value,
            NodeId::Constraint(_) => Self::Constraint,
            NodeId::Realization(_) => Self::Realization,
            NodeId::Resolution(_) => Self::Resolution,
            NodeId::Compute(_) => Self::Compute,
        }
    }
}

/// Project a [`NodeId`] to its [`reify_types::NodeKind`] discriminant via the
/// [`reify_types::HasNodeKind`] trait.
///
/// Sibling bridge to the `From<&NodeId> for NodeKind` impl above; both live in
/// `reify-eval` for the same orphan-rule reason (`NodeId` is local to this crate,
/// the destination trait/type is foreign). The body delegates to that existing `From`
/// impl to avoid duplicating match arms. See PRD §5 B1.
impl reify_ir::HasNodeKind for NodeId {
    fn node_kind(&self) -> reify_ir::NodeKind {
        reify_ir::NodeKind::from(self)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeId::Value(v) => v.fmt(f),
            NodeId::Constraint(c) => c.fmt(f),
            NodeId::Realization(r) => r.fmt(f),
            NodeId::Resolution(s) => s.fmt(f),
            NodeId::Compute(c) => c.fmt(f),
        }
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
    /// Estimated `cost_per_byte` companion of `warm_state`
    /// (= `estimated_cold_compute_time_secs / size_bytes`, arch §4.3).
    ///
    /// Paired with `warm_state` — both are reset together on `Clone`, in
    /// `record_evaluation_with_freshness`, AND in
    /// [`CacheStore::get_warm_state`] (take semantics) since the cost is
    /// meaningful only while the warm state it describes is still present.
    /// Sanitised by [`CacheStore::donate_warm_state_with_cost`] so a future
    /// cost-weighted LRU eviction policy can `partial_cmp` safely on the
    /// field.
    pub cost_per_byte: f64,
    /// Diagnostic-chain side-table: when `freshness == Pending`, this carries
    /// the upstream `NodeId` that caused this node to be Pending. Valid
    /// chain-root variants (per `docs/prds/v0_3/compute-node-contract.md §3`):
    ///
    /// - `NodeId::Value(_)` — a Failed leaf whose error gated a downstream cell.
    /// - `NodeId::Compute(_)` — an in-flight ComputeNode that is itself the
    ///   chain root (admitted by PRD §3 "Chain-root contract extension").
    ///
    /// `None` when the entry is not Pending or the chain root has not been
    /// recorded.
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
            // Cost is meaningful only while the warm_state it describes is
            // present; resetting both together preserves the "audit-clean"
            // pairing invariant called out on the field.
            cost_per_byte: 0.0,
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
            cost_per_byte: 0.0,
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
    /// Per-path content-hash cache for imported field source files (PRD task 4 / task 2668).
    ///
    /// Stores the most-recently-observed `ContentHash` for each user-supplied path string.
    /// Used by PRD task 5's wire site in `elaborate_field` to detect file-content changes
    /// between evaluations via `imported_file_hash_changed`. Hashes are byte-only — see
    /// `engine_eval::hash_imported_file_content`. Keys are literal path strings as written
    /// by the user (not canonicalised); see design decision in plan 2668 for rationale.
    ///
    /// **Growth policy:** this map grows monotonically with each distinct path string
    /// observed, and shrinks only when [`CacheStore::clear`] is called. In normal
    /// interactive use the set of active import paths is bounded by the design tree, so
    /// the map stays small. Stale entries from edited-away `import` declarations will
    /// linger until the next `clear()` call; a future eviction hook (keyed on the
    /// active import set at the start of each eval cycle) could reclaim them if memory
    /// pressure from long-lived sessions proves to be an issue.
    ///
    /// **Non-UTF-8 paths:** keys are `String`, so paths that are not valid UTF-8 cannot
    /// be recorded in this side-table. See `record_imported_file_hash` for details.
    imported_file_hashes: HashMap<String, ContentHash>,
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
            imported_file_hashes: HashMap::new(),
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

    /// Remove all cached entries, dirty state, and imported-file content hashes.
    ///
    /// Clears `caches`, `dirty_reasons`, and the `imported_file_hashes` side-table
    /// (added in PRD task 4 / task 2668) so the cache-reset surface stays unified.
    pub fn clear(&mut self) {
        self.caches.clear();
        self.dirty_reasons.clear();
        self.imported_file_hashes.clear();
    }

    /// Record the most-recently-observed content hash for an imported file path.
    ///
    /// Per-path content-hash side-table for imported field source files (PRD task 4 / task 2668).
    /// Overwrites any prior recording for `path`. The key is the literal user-supplied path
    /// string — not canonicalised (see design decision in plan 2668).
    ///
    /// **Non-UTF-8 paths:** `path` is a `&str`, so callers that have a `PathBuf` or `OsStr`
    /// must convert to UTF-8 first. Paths that are not valid UTF-8 cannot be recorded in this
    /// side-table. In practice the surface-language `imported { path = "..." }` literal is
    /// always UTF-8, so this is not expected to be a practical limitation. If non-UTF-8 support
    /// becomes necessary, the key type should change to `OsString` / `PathBuf`.
    ///
    /// Companion to [`CacheStore::get_imported_file_hash`] and
    /// [`CacheStore::imported_file_hash_changed`]. PRD task 5's wire site in `elaborate_field`
    /// calls this after reading a file with `engine_eval::hash_imported_file_content` to
    /// update the recorded hash.
    pub fn record_imported_file_hash(&mut self, path: &str, hash: ContentHash) {
        self.imported_file_hashes.insert(path.to_string(), hash);
    }

    /// Retrieve the most-recently-recorded content hash for an imported file path.
    ///
    /// Returns `None` when no hash has been recorded yet for `path` (cold start or after
    /// [`CacheStore::clear`]). Returns `Some(hash)` once
    /// [`CacheStore::record_imported_file_hash`] has been called for that path.
    ///
    /// `ContentHash` is `Copy`, so this returns the value directly rather than a reference.
    pub fn get_imported_file_hash(&self, path: &str) -> Option<ContentHash> {
        self.imported_file_hashes.get(path).copied()
    }

    /// Invalidation predicate for PRD task 5's wire site in `elaborate_field`.
    ///
    /// Returns `true` (i.e. "invalidate") in two cases:
    /// - The path has no recorded hash yet (cold start — `None` in the side-table).
    /// - The recorded hash differs from `new_hash` (file content changed).
    ///
    /// Returns `false` (i.e. "cache hit") only when an exact match is recorded.
    ///
    /// ## Three branches
    ///
    /// 1. **No prior recording** → `true` (cold start; must re-read).
    /// 2. **Recorded hash == `new_hash`** → `false` (content unchanged; cache hits).
    /// 3. **Recorded hash != `new_hash`** → `true` (content changed; must invalidate).
    ///
    /// ## PRD acceptance properties
    ///
    /// - File-content change → different hash → returns `true` → invalidation signal.
    /// - File-path change with same content → same hash (from `engine_eval::hash_imported_file_content`,
    ///   which hashes bytes only) → returns `false` → cache hit.
    ///
    /// Companion to [`CacheStore::record_imported_file_hash`] and
    /// [`CacheStore::get_imported_file_hash`].
    pub fn imported_file_hash_changed(&self, path: &str, new_hash: ContentHash) -> bool {
        self.imported_file_hashes
            .get(path)
            .is_none_or(|h| *h != new_hash)
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
            existing.cost_per_byte = 0.0; // paired-reset with warm_state (ζ/3425)
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
                cost_per_byte: 0.0,
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
    /// - `concurrent.rs::concurrent_eval` and `engine_edit.rs::edit_param` /
    ///   `engine_edit.rs::edit_source` — same shape: bulk-mark every member
    ///   of the eval-set Pending before the per-node evaluator runs.
    ///
    /// - `freshness_walk.rs` — the `None` arm of the `cause` match in the
    ///   Pending-write branch: the walk's compute helper returned `Pending`
    ///   but with `cause = None`, so there is no upstream `NodeId` to
    ///   record. The chain is dropped not because the input set is clean,
    ///   but because the helper produced no traceable upstream.
    ///
    /// All four of these intentionally drop the chain, but for different
    /// reasons: the three bulk pre-pass callers have no Failed or Pending
    /// nodes in their input set at call time; the freshness-walk caller has
    /// no upstream `NodeId` to record. The §9.2 chain is laid down inside
    /// the per-node evaluator itself (e.g. `evaluate_let_bindings` →
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
    /// # Precondition: do not pass `Freshness::Failed`
    ///
    /// `mark_failed` must be used instead of `set_freshness(node,
    /// Freshness::Failed { error })`. This helper centralises the
    /// "Failed nodes are chain roots" invariant (including the chain-clearing
    /// side effect); `set_freshness` would silently allow downstream
    /// callers to skip recording the chain root and to leak a stale
    /// `pending_cause`. This precondition is enforced via `assert!` in all
    /// builds (task #2592, parity with the Pending guard from task #2451).
    pub fn mark_failed(&mut self, node: &NodeId, error: reify_ir::ErrorRef) -> bool {
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
    /// `cause` is the upstream `NodeId` that drove the transition. Valid
    /// chain-root variants (per `docs/prds/v0_3/compute-node-contract.md §3`):
    ///
    /// - `NodeId::Value(_)` — a Failed leaf whose error gated a downstream cell.
    /// - `NodeId::Compute(_)` — an in-flight ComputeNode that is itself the
    ///   chain root (admitted by PRD §3 "Chain-root contract extension").
    /// - Another Pending node forwarding its own upstream cause.
    ///
    /// The `cause` field accepts any `NodeId` variant — no per-variant guard
    /// is applied. See arch §9.2 lines 880-890 and arch §7.2 line 748.
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

    /// Begin the in-flight ComputeNode dispatch lifecycle for `c_id`'s output
    /// ValueCells — PRD §3 "Atomic completion" step 0 (begin) / §8 task δ
    /// (`docs/prds/v0_3/compute-node-contract.md`).
    ///
    /// Handles both branches uniformly so the engine call site stays simple:
    ///
    /// - **Existing entry** (re-dispatch): routes through
    ///   [`CacheStore::mark_pending_with_cause`] so `last_substantive` is
    ///   captured from the prior `result_hash` (the previous best stays on
    ///   display) and the chain root `NodeId::Compute(c_id)` is recorded.
    /// - **Absent entry** (first-time dispatch): seeds a fresh Pending entry
    ///   with `last_substantive: ResultRef::none()` (no prior result exists —
    ///   the documented sentinel per `Freshness::Pending`'s field doc),
    ///   `pending_cause = Some(NodeId::Compute(c_id))`, an empty
    ///   `DependencyTrace`, and `basis_version: VersionId(0)`.
    ///
    /// `NodeId::Compute(_)` is admitted as a valid chain root by PRD §3
    /// "Chain-root contract extension" (see task 3420/α). Every marked or
    /// seeded output bumps `pending_transition_count`.
    ///
    /// Returns the count of output VCs marked or seeded Pending.
    //
    // `clippy::map_entry` would prefer the Entry API here, but the existing-VC
    // branch calls `self.mark_pending_with_cause(...)` which borrows all of
    // `self` mutably and so cannot coexist with `self.caches.entry(...)`. We
    // could inline `mark_pending_with_cause`'s body, but that duplicates the
    // chain-root contract logic and decouples this site from regression tests
    // that pin `mark_pending_with_cause` directly. Allowing the lint locally
    // is the minimal change.
    #[allow(clippy::map_entry)]
    pub fn begin_compute_dispatch(
        &mut self,
        c_id: &ComputeNodeId,
        outputs: &[ValueCellId],
    ) -> usize {
        let mut marked = 0;
        for out in outputs {
            let node = NodeId::Value(out.clone());
            if self.caches.contains_key(&node) {
                // Re-dispatch: preserve the prior best as last_substantive
                // via the chain-root helper.
                if self.mark_pending_with_cause(&node, NodeId::Compute(c_id.clone())) {
                    marked += 1;
                }
            } else {
                // First-time dispatch: no prior result → seed Pending with
                // the ResultRef::none() sentinel.
                //
                // Compute `result_hash` from the seeded `CachedResult` (rather
                // than a `ContentHash(0)` sentinel) so this entry obeys the
                // global invariant `result_hash == result.content_hash()` that
                // `record_evaluation_with_freshness`'s early-cutoff path relies
                // on. Without this, a same-hash check against the bogus 0 would
                // happen to take the changed-path (correct by accident) and a
                // trampoline that legitimately returned a value hashing to 0
                // would mis-route through the early-cutoff branch.
                //
                // The placeholder shape is shared with
                // `seed_compute_entry_if_absent` via
                // `Self::dispatch_placeholder_result` — see that helper's
                // docstring for the rationale.
                let result = Self::dispatch_placeholder_result();
                let result_hash = result.content_hash();
                self.caches.insert(
                    node,
                    NodeCache {
                        result,
                        result_hash,
                        freshness: Freshness::Pending {
                            last_substantive: ResultRef::none(),
                        },
                        dependency_trace: DependencyTrace::default(),
                        basis_version: VersionId(0),
                        warm_state: None,
                        cost_per_byte: 0.0,
                        pending_cause: Some(NodeId::Compute(c_id.clone())),
                    },
                );
                self.pending_transition_count += 1;
                marked += 1;
            }
        }
        marked
    }

    /// Complete the in-flight ComputeNode dispatch lifecycle atomically —
    /// PRD §3 "Atomic completion" steps 1-3 / §8 task δ
    /// (`docs/prds/v0_3/compute-node-contract.md`).
    ///
    /// For each `(out, value)` this performs, in a single critical section
    /// (one function call — no public sub-step API exists in between, so no
    /// consumer can observe an incoherent intermediate state):
    ///
    /// 1. **Write** the new value: `CachedResult::Value(value, Determined)`.
    /// 2. **Flip** freshness Pending → Final.
    /// 3. **Clear** `pending_cause` on the output entry.
    ///
    /// Steps 1-2 go through [`CacheStore::record_evaluation_with_freshness`]
    /// with `Freshness::Final`, which covers both the changed-hash path
    /// (re-insert with `pending_cause = None`) and the early-cutoff path
    /// (same hash as the prior Pending entry — fields updated but
    /// `pending_cause` is PRESERVED per that helper's docstring). Step 3 is
    /// the explicit side-table clear that closes the early-cutoff gap and is
    /// what makes the same-hash-as-prior case atomic.
    ///
    /// `_with_cause` is deliberately NOT called here — `Freshness::Final`
    /// has no diagnostic chain root.
    ///
    /// **Warm-state donation (PRD §5 step 4 / ζ — task 3425).** When
    /// `new_warm_state` is `Some`, an entry under `NodeId::Compute(c_id)`
    /// is auto-seeded with a sentinel `CachedResult::Value(Undef, Determined)`
    /// placeholder (Compute entries exist only to carry warm_state + cost;
    /// they never hold authoritative results — those live on output VCs)
    /// and the warm state + `cost_per_byte` are donated into it via
    /// [`CacheStore::donate_warm_state_with_cost`].
    ///
    /// The donation runs in the same call as the output-VC flips, so any
    /// caller observing a Final output through this `CacheStore` reference
    /// will also see the donated warm state — there is no intervening
    /// release of the `&mut self` borrow. This is NOT atomicity in the
    /// strict cross-thread sense: `&mut self` only excludes simultaneous
    /// mutators, not interleaved readers between two `&mut` calls. Within
    /// a single call, the multi-output loop also flips later VCs only
    /// after earlier ones, so a hypothetical multi-output consumer could
    /// still observe partial progress (today this is moot: multi-output
    /// dispatch is `debug_assert_eq!`-pinned to a single output).
    ///
    /// When `new_warm_state` is `None`, no Compute entry is created — the
    /// trampoline reported no state worth preserving, and the at-rest
    /// surface is the cache alone.
    ///
    /// Returns the count of output entries written.
    pub fn complete_compute_dispatch_atomically(
        &mut self,
        c_id: &ComputeNodeId,
        outputs: &[(ValueCellId, Value)],
        version: VersionId,
        new_warm_state: Option<OpaqueState>,
        cost_per_byte: f64,
    ) -> usize {
        let mut updated = 0;
        for (out, value) in outputs {
            let node = NodeId::Value(out.clone());
            let cached_result = CachedResult::Value(value.clone(), DeterminacyState::Determined);
            // Steps 1-2: write value + flip freshness to Final.
            self.record_evaluation_with_freshness(
                node.clone(),
                cached_result,
                version,
                DependencyTrace::default(),
                Freshness::Final,
            );
            // Step 3: explicitly clear pending_cause. The early-cutoff path
            // of record_evaluation_with_freshness preserves the side-table,
            // so a same-hash-as-prior-Pending update would otherwise leak the
            // stale Compute chain root — clearing here is the single
            // critical-section atomicity guarantee.
            if let Some(entry) = self.caches.get_mut(&node) {
                entry.pending_cause = None;
            }
            updated += 1;
        }

        // Step 4 (ζ): donate the trampoline's new warm state + cost to the
        // canonical at-rest store. The Compute entry is auto-seeded as a
        // sentinel; it has no authoritative result. See design decision 3
        // in `.task/plan.json`.
        if let Some(state) = new_warm_state {
            self.seed_compute_entry_if_absent(c_id, version);
            // donate_warm_state_with_cost sanitises non-finite/negative cost
            // to 0.0; the no-op (entry-absent) branch cannot fire here
            // because seed_compute_entry_if_absent just guaranteed the entry.
            let compute_node = NodeId::Compute(c_id.clone());
            self.donate_warm_state_with_cost(&compute_node, state, cost_per_byte);
        }

        updated
    }

    /// Auto-seed a sentinel Compute entry under `NodeId::Compute(c_id)` if absent.
    ///
    /// Compute entries exist only to carry `warm_state` + `cost_per_byte` —
    /// they never hold authoritative results (those live on output VCs). The
    /// sentinel is `CachedResult::Value(Undef, Determined)` at `Freshness::Final`
    /// with a default `DependencyTrace`. No-op when an entry already exists.
    ///
    /// Used by [`CacheStore::complete_compute_dispatch_atomically`] on the
    /// Completed-Some path AND by `run_compute_dispatch`'s Cancelled / Failed /
    /// unregistered arms when restoring the prior to a cache with no Compute
    /// entry (the post-edit pool-only path, PRD §5 "Idempotent under any number
    /// of cancel-and-redispatch cycles"). Keeping the sentinel construction in
    /// one place ensures both call sites stay in sync if the placeholder shape
    /// ever changes.
    pub(crate) fn seed_compute_entry_if_absent(
        &mut self,
        c_id: &ComputeNodeId,
        version: VersionId,
    ) {
        let compute_node = NodeId::Compute(c_id.clone());
        self.caches.entry(compute_node).or_insert_with(|| {
            NodeCache::new(
                Self::dispatch_placeholder_result(),
                Freshness::Final,
                DependencyTrace::default(),
                version,
            )
        });
    }

    /// Shared placeholder `CachedResult` used by the in-flight dispatch
    /// lifecycle when no authoritative result is yet known.
    ///
    /// Used by:
    ///
    /// - [`CacheStore::begin_compute_dispatch`] when seeding a first-time
    ///   Pending entry on a Value cell (the entry will be overwritten by
    ///   the trampoline's output, but until then we need a content-hash-
    ///   correct placeholder so the early-cutoff path of
    ///   `record_evaluation_with_freshness` stays sound — see the
    ///   in-line comment at that site).
    /// - [`CacheStore::seed_compute_entry_if_absent`] when seeding a
    ///   sentinel Compute entry to carry warm_state + cost_per_byte
    ///   (Compute entries never hold authoritative results — those live
    ///   on output VCs — so the placeholder is permanent for Compute
    ///   nodes).
    ///
    /// Centralising the placeholder shape ensures both seed paths stay
    /// in sync if `CachedResult` or its sentinel ever evolves.
    fn dispatch_placeholder_result() -> CachedResult {
        CachedResult::Value(Value::Undef, DeterminacyState::Determined)
    }

    /// Read the diagnostic-chain cause stored on a node's cache entry.
    ///
    /// Returns the `Option<NodeId>` from the entry's `pending_cause`
    /// side-table when present; returns `None` when the node has no entry
    /// (consistent with the "default to None on absent" pattern).
    ///
    /// The returned `NodeId` may be any of the valid chain-root variants
    /// (per `docs/prds/v0_3/compute-node-contract.md §3`):
    ///
    /// - `NodeId::Value(_)` — a Failed leaf whose error gated a downstream cell.
    /// - `NodeId::Compute(_)` — an in-flight ComputeNode that is itself the
    ///   chain root (admitted by PRD §3 "Chain-root contract extension").
    ///
    /// Failed entries return `None` here (they are chain roots, not
    /// forwarders); only Pending entries written via
    /// [`CacheStore::mark_pending_with_cause`] populate this field.
    pub fn pending_cause(&self, node: &NodeId) -> Option<NodeId> {
        self.caches.get(node).and_then(|e| e.pending_cause.clone())
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
    /// # Precondition: do not pass `Freshness::Pending` or `Freshness::Failed`
    ///
    /// `mark_pending` or `mark_pending_with_cause` must be used instead of
    /// `set_freshness(node, Freshness::Pending { ... })` when transitioning a node
    /// to the Pending state. Those helpers derive `last_substantive` from the current
    /// cached `result_hash` (ensuring consistency) and increment
    /// `pending_transition_count` (a diagnostic counter). This precondition is
    /// enforced via `assert!` in all builds (task #2451 S1).
    ///
    /// `mark_failed` must be used instead of `set_freshness(node,
    /// Freshness::Failed { ... })` when transitioning a node to the Failed state.
    /// That helper centralises the "Failed nodes are chain roots" invariant (clearing
    /// `pending_cause` so no stale diagnostic chain leaks through). This precondition
    /// is enforced via `assert!` in all builds (task #2592, parity with the Pending
    /// guard above).
    ///
    /// **S2 audit (task #2451):** production write paths in `concurrent.rs` and
    /// `engine_edit.rs` already route all Pending transitions through `mark_pending`
    /// / `mark_pending_with_cause` (tasks #2326, #2335) and all Failed transitions
    /// through `mark_failed`. `CacheStore::caches` is a private field with no public
    /// `get_mut` accessor, so external code cannot write `freshness` directly. These
    /// preconditions therefore cover all production write sites.
    ///
    /// `restore_final` and `mark_pending` continue to coexist as domain-specific
    /// helpers. `mark_pending` additionally captures `result_hash` into
    /// `last_substantive` and bumps `pending_transition_count`; `restore_final`
    /// is today equivalent to `set_freshness(node, Freshness::Final)` but is
    /// retained for readability at its call sites.
    #[must_use = "set_freshness returns false when the node is absent; check or explicitly discard"]
    pub fn set_freshness(&mut self, node: &NodeId, freshness: Freshness) -> bool {
        assert!(
            !matches!(
                freshness,
                Freshness::Pending { .. } | Freshness::Failed { .. }
            ),
            "set_freshness must not be passed Pending or Failed; use mark_pending/mark_pending_with_cause or mark_failed instead"
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
    ///
    /// Back-compat wrapper over [`CacheStore::donate_warm_state_with_cost`]
    /// with `cost_per_byte = 0.0` — mirrors the same wrapper pattern
    /// [`WarmStatePool::donate`](crate::warm_pool::WarmStatePool::donate)
    /// uses over `donate_with_cost`. Existing callers and tests are
    /// unaffected; cost-aware callers use the explicit method.
    pub fn donate_warm_state(&mut self, node: &NodeId, state: OpaqueState) -> bool {
        self.donate_warm_state_with_cost(node, state, 0.0)
    }

    /// Store warm-start state on an existing cached node along with its
    /// estimated `cost_per_byte` (= `estimated_cold_compute_time_secs /
    /// size_bytes`, arch §4.3).
    ///
    /// Returns `true` if the node was found and both fields were written,
    /// `false` if the node is not in the cache (no-op).
    ///
    /// `cost_per_byte` is sanitised to `0.0` when not finite (`NaN`, `±inf`)
    /// or negative, mirroring [`crate::warm_pool::WarmStatePool::insert_entry`]
    /// so a future cost-weighted-LRU comparator can safely call `partial_cmp`
    /// without panicking on non-finite values or mishandling negative costs.
    pub fn donate_warm_state_with_cost(
        &mut self,
        node: &NodeId,
        state: OpaqueState,
        cost_per_byte: f64,
    ) -> bool {
        if let Some(entry) = self.caches.get_mut(node) {
            // Sanitise: clamp NaN / ±inf / negatives to 0.0 so a future
            // cost-weighted-LRU comparator can `partial_cmp` safely.
            let sanitised = if cost_per_byte.is_finite() && cost_per_byte >= 0.0 {
                cost_per_byte
            } else {
                0.0
            };
            entry.warm_state = Some(state);
            entry.cost_per_byte = sanitised;
            true
        } else {
            false
        }
    }

    /// Return the stored `cost_per_byte` for a cached node, or `None` if the
    /// node is not present.
    ///
    /// Companion reader to [`CacheStore::donate_warm_state_with_cost`] /
    /// [`CacheStore::get_warm_state`]: callers driving the in-flight
    /// dispatch lifecycle (e.g. `engine_compute::run_compute_dispatch`)
    /// capture the prior cost before taking the warm state so the cost can
    /// be restored alongside the warm state on `Cancelled` / `Failed`.
    pub fn cost_per_byte_of(&self, node: &NodeId) -> Option<f64> {
        self.caches.get(node).map(|e| e.cost_per_byte)
    }

    /// Take the warm-start state out of a cached node (take semantics).
    ///
    /// Returns the `OpaqueState` if present, leaving `None` in its place.
    /// A second call for the same node will return `None`.
    ///
    /// **Pairing invariant (ζ amendment).** The companion `cost_per_byte`
    /// field is reset to `0.0` here whenever an entry exists, so a take
    /// leaves the cache in a paired state: an entry with `warm_state: None`
    /// also has `cost_per_byte: 0.0`. This prevents a stale cost from being
    /// observed by a future cost-weighted-LRU comparator (or by
    /// [`CacheStore::cost_per_byte_of`]) after the warm state it described
    /// has already been taken. Callers that need to restore the prior cost
    /// alongside the prior state (`engine_compute::run_compute_dispatch`)
    /// must read [`CacheStore::cost_per_byte_of`] BEFORE calling this method.
    pub fn get_warm_state(&mut self, node: &NodeId) -> Option<OpaqueState> {
        let entry = self.caches.get_mut(node)?;
        let taken = entry.warm_state.take();
        // Pair the take: cost_per_byte is meaningful only while the warm
        // state is still present. Reset to 0.0 unconditionally so the
        // pairing invariant holds even on a second (None) call.
        entry.cost_per_byte = 0.0;
        taken
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
        let (f, _) =
            self.derive_output_freshness_from_trace_with_cause(trace, still_refining, generation);
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
        let (f, _) =
            self.derive_output_freshness_for_node_with_cause(node_id, still_refining, generation);
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
    /// Walks `trace.reads` (VC inputs) **and** `trace.realization_reads`
    /// (GHR-δ: the upstream Realization a `Value::GeometryHandle` cell depends
    /// on), looks up each input's freshness and `pending_cause`, and feeds the
    /// combined triples into [`derive_output_freshness_with_cause`]. The core
    /// classifier is NodeId-variant-agnostic, so a `NodeId::Realization` input
    /// participates in the meet and chain-root forwarding exactly like a VC
    /// input. Use this at wire sites that have a freshly-computed trace and need
    /// the chain root — e.g. the pre-eval Pending gate in `evaluate_let_bindings`.
    pub fn derive_output_freshness_from_trace_with_cause(
        &self,
        trace: &DependencyTrace,
        still_refining: bool,
        generation: u64,
    ) -> (Freshness, Option<NodeId>) {
        derive_output_freshness_with_cause(
            still_refining,
            trace
                .reads
                .iter()
                .map(|read| {
                    let n = NodeId::Value(read.clone());
                    let f = self.freshness(&n);
                    let c = self.pending_cause(&n);
                    (n, f, c)
                })
                .chain(trace.realization_reads.iter().map(|rid| {
                    // GHR-δ: a GH cell implicitly reads its backing Realization;
                    // fold that node's freshness into the same meet.
                    let n = NodeId::Realization(rid.clone());
                    let f = self.freshness(&n);
                    let c = self.pending_cause(&n);
                    (n, f, c)
                })),
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
        return Freshness::Pending {
            last_substantive: ResultRef::none(),
        };
    }
    // §7.2 line 748: Pending input → Pending output (chain forwarded at _with_cause layer).
    if saw_pending {
        return Freshness::Pending {
            last_substantive: ResultRef::none(),
        };
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
///   used as the cause. The forwarded cause may be any valid chain-root variant,
///   including `NodeId::Compute(_)` (in-flight ComputeNode — admitted by
///   `docs/prds/v0_3/compute-node-contract.md §3`); the forwarder branch is
///   variant-agnostic.
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
                    // The forwarded cause may be any NodeId variant, including
                    // NodeId::Compute(_) (PRD §3 chain-root contract extension).
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
            Freshness::Pending {
                last_substantive: ResultRef::none(),
            },
            Some(cause),
        );
    }
    // §7.2 line 748: any Pending input → Pending output, with the upstream cause forwarded.
    // Cause may be `None` if the upstream chain was never recorded — that yields
    // `(Pending, None)` (sentinel for "chain incomplete").
    if saw_pending {
        return (
            Freshness::Pending {
                last_substantive: ResultRef::none(),
            },
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

impl Default for CacheStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{ConstraintNodeId, RealizationNodeId, Type, ValueCellId};

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
        use reify_core::ResolutionNodeId;

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

    /// P3.3 step-1: Pin the new `NodeId::Compute(ComputeNodeId)` variant.
    ///
    /// Mirrors `node_id_resolution_variant`. Asserts:
    ///   (a) construction + equality/hash round-trip via `HashMap` key
    ///   (b) `From<ComputeNodeId> for NodeId` produces `NodeId::Compute(_)`
    ///   (c) the variant differs from every existing variant even with
    ///       overlapping entity strings
    #[test]
    fn node_id_compute_variant() {
        use reify_core::{ComputeNodeId, ResolutionNodeId};

        let cn_id = ComputeNodeId::new("E", 0);
        let cn_node = NodeId::Compute(cn_id.clone());

        // (a) Equality with itself
        assert_eq!(cn_node, NodeId::Compute(ComputeNodeId::new("E", 0)));

        // (a) Hash round-trip through HashMap (mirrors node_id_hash_as_map_key)
        let mut map = HashMap::new();
        map.insert(cn_node.clone(), "compute");
        assert_eq!(map.get(&NodeId::Compute(cn_id.clone())), Some(&"compute"));

        // (c) Differs from other variants
        assert_ne!(cn_node, NodeId::Value(ValueCellId::new("E", "x")));
        assert_ne!(cn_node, NodeId::Constraint(ConstraintNodeId::new("E", 0)));
        assert_ne!(cn_node, NodeId::Realization(RealizationNodeId::new("E", 0)));
        assert_ne!(cn_node, NodeId::Resolution(ResolutionNodeId::new("E", 0)));

        // (b) From<ComputeNodeId> conversion
        let from_node: NodeId = NodeId::from(cn_id.clone());
        assert_eq!(from_node, cn_node);
    }

    /// P3.3 step-1: Pin `Display for NodeId::Compute(id)` forwarding to
    /// `id.fmt(f)`. The contract this test enforces is delegation: the
    /// wrapper's Display impl must produce the same bytes as the inner
    /// `ComputeNodeId`'s impl. We deliberately do NOT pin the literal
    /// format produced by `ComputeNodeId::fmt` here — that lives in
    /// reify_types and may be retuned independently of NodeId's
    /// pass-through contract.
    #[test]
    fn node_id_display_compute_forwards_to_inner_variant() {
        use reify_core::ComputeNodeId;

        let inner = ComputeNodeId::new("E", 0);
        let node = NodeId::Compute(inner.clone());
        assert_eq!(format!("{}", node), format!("{}", inner));
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

    #[test]
    fn node_id_display_forwards_to_inner_variant() {
        use reify_core::ResolutionNodeId;

        // Value variant
        let inner_v = ValueCellId::new("Bracket", "x");
        let node_v = NodeId::Value(inner_v.clone());
        assert_eq!(format!("{}", node_v), format!("{}", inner_v));

        // Constraint variant
        let inner_c = ConstraintNodeId::new("Bracket", 0);
        let node_c = NodeId::Constraint(inner_c.clone());
        assert_eq!(format!("{}", node_c), format!("{}", inner_c));

        // Realization variant
        let inner_r = RealizationNodeId::new("Bracket", 1);
        let node_r = NodeId::Realization(inner_r.clone());
        assert_eq!(format!("{}", node_r), format!("{}", inner_r));

        // Resolution variant
        let inner_s = ResolutionNodeId::new("Bracket", 2);
        let node_s = NodeId::Resolution(inner_s.clone());
        assert_eq!(format!("{}", node_s), format!("{}", inner_s));
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
        use reify_ir::BinOp;

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
        use reify_ir::BinOp;

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
        use reify_core::{ResolutionNodeId, VersionId};
        use reify_ir::{Freshness, Value};
        use std::collections::HashMap;

        let mut store = CacheStore::new();
        let res_id = ResolutionNodeId::new("A", 0);
        let node = NodeId::Resolution(res_id);

        let mut values = HashMap::new();
        values.insert(ValueCellId::new("A", "x"), Value::Real(1.0));
        let result = CachedResult::Resolution(values);
        let expected_hash = result.content_hash();

        let version = VersionId(1);
        let trace = DependencyTrace { realization_reads: Vec::new(),
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
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

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
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

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
        use reify_core::VersionId;
        use reify_ir::{Freshness, Satisfaction};

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
        use reify_ir::{DeterminacyState, Value};
        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("Value"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn cached_result_satisfaction_variant() {
        use reify_ir::Satisfaction;
        let result = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("Satisfaction"));
    }

    #[test]
    fn cached_result_geometry_handle_variant() {
        use reify_ir::GeometryHandleId;
        let result = CachedResult::GeometryHandle(GeometryHandleId(7));
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("GeometryHandle"));
    }

    #[test]
    fn cached_result_content_hash_value_variant() {
        use reify_ir::{DeterminacyState, Value};
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
        use reify_ir::Satisfaction;
        let r1 = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let r2 = CachedResult::Satisfaction(Satisfaction::Satisfied);
        assert_eq!(r1.content_hash(), r2.content_hash());

        let r3 = CachedResult::Satisfaction(Satisfaction::Violated);
        assert_ne!(r1.content_hash(), r3.content_hash());
    }

    #[test]
    fn cached_result_content_hash_geometry_variant() {
        use reify_ir::GeometryHandleId;
        let r1 = CachedResult::GeometryHandle(GeometryHandleId(7));
        let r2 = CachedResult::GeometryHandle(GeometryHandleId(7));
        assert_eq!(r1.content_hash(), r2.content_hash());

        let r3 = CachedResult::GeometryHandle(GeometryHandleId(8));
        assert_ne!(r1.content_hash(), r3.content_hash());
    }

    #[test]
    fn cached_result_resolution_variant() {
        use reify_ir::Value;
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
        use reify_ir::{DeterminacyState, GeometryHandleId, Satisfaction, Value};
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
        use reify_core::{ModulePath, Type, VersionId};
        use reify_ir::BinOp;

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
        use reify_core::{ModulePath, Type, VersionId};
        use reify_ir::BinOp;

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
        use reify_core::{ModulePath, Type, VersionId};
        use reify_ir::BinOp;

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
        use reify_core::{ContentHash, ModulePath, Type, VersionId};
        use reify_ir::{BinOp, CompiledExpr, CompiledExprKind};

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
        use reify_core::{ContentHash, ModulePath, Type, VersionId};
        use reify_ir::{BinOp, CompiledExpr, CompiledExprKind};

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
        use reify_core::{ContentHash, ModulePath, Type, VersionId};
        use reify_ir::{BinOp, CompiledExpr, CompiledExprKind};

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
        let state = reify_ir::OpaqueState::new(42i32, 4);
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
        cache.warm_state = Some(reify_ir::OpaqueState::new(99i32, 4));
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

        let state = reify_ir::OpaqueState::new(100i32, 4);
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

    /// ζ amendment: `get_warm_state` pairs the take with a
    /// `cost_per_byte` reset to `0.0` so a stale cost cannot outlive the
    /// warm state it describes (PRD §4.3 cost-weighted-LRU readiness).
    #[test]
    fn get_warm_state_pairs_take_with_cost_reset_to_zero() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "pair"));
        store.put(node.clone(), make_test_node_cache(42, 1));

        // Donate warm state + cost.
        let donated = store.donate_warm_state_with_cost(
            &node,
            reify_ir::OpaqueState::new(7i32, 4),
            0.75,
        );
        assert!(donated);
        assert_eq!(store.cost_per_byte_of(&node), Some(0.75));

        // Take the warm state — entry still exists, but BOTH fields must
        // be cleared (pairing invariant).
        let taken = store.get_warm_state(&node);
        assert!(taken.is_some(), "first take returns the donated state");
        assert_eq!(
            store.cost_per_byte_of(&node),
            Some(0.0),
            "get_warm_state must reset cost_per_byte to 0.0 alongside the take",
        );

        // Second take returns None (warm state already taken) and cost
        // stays at 0.0 (idempotent reset).
        let taken_again = store.get_warm_state(&node);
        assert!(taken_again.is_none(), "second take returns None");
        assert_eq!(
            store.cost_per_byte_of(&node),
            Some(0.0),
            "second take leaves cost_per_byte at 0.0 (idempotent)",
        );
    }

    #[test]
    fn donate_warm_state_on_nonexistent_node_returns_false() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "missing"));
        let state = reify_ir::OpaqueState::new(42i32, 4);
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
        let donated = store.donate_warm_state(&node, reify_ir::OpaqueState::new(0xBEEFu32, 8));
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
        let state = reify_ir::OpaqueState::new(100i32, 4);
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
        cache.warm_state = Some(reify_ir::OpaqueState::new(42i32, 4));
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
        use reify_ir::Freshness;

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
        let result = store.set_freshness(&node, Freshness::Intermediate { generation: 1 });
        assert!(result, "set_freshness on present node must return true");

        // (c) Round-trip: canonical reader reflects the written value.
        assert_eq!(
            store.freshness(&node),
            Freshness::Intermediate { generation: 1 },
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
        use reify_core::ContentHash;
        use reify_ir::{Freshness, ResultRef};

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
        use reify_ir::{ErrorRef, Freshness};

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
        use reify_ir::Freshness;
        let g = 7u64;

        // Row 1: still_refining=true, all inputs Final → Intermediate
        let inputs_all_final = [Freshness::Final, Freshness::Final];
        assert_eq!(
            derive_output_freshness(true, inputs_all_final.iter().cloned(), g),
            Freshness::Intermediate { generation: g },
            "still_refining=true, all-Final inputs → Intermediate"
        );

        // Row 2: still_refining=true, some input non-Final → Intermediate
        let inputs_with_non_final = [Freshness::Final, Freshness::Intermediate { generation: 3 }];
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
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

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
        assert_eq!(
            outcome1,
            EvalOutcome::Changed,
            "cold start must return Changed"
        );
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
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

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
        let result =
            store.derive_output_freshness_for_node(&NodeId::Value(out_id.clone()), false, 7);
        assert_eq!(
            result,
            Freshness::Intermediate { generation: 7 },
            "one non-Final input (b=Intermediate) must yield Intermediate output"
        );

        // Case 2: make 'b' Final → output should be Final
        let _ = store.set_freshness(&NodeId::Value(b_id.clone()), Freshness::Final);
        let result2 =
            store.derive_output_freshness_for_node(&NodeId::Value(out_id.clone()), false, 7);
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
        use reify_ir::{ErrorRef, Freshness, ResultRef};
        let g = 9u64;

        // Intermediate input → Intermediate output (§7.2 unchanged for this row).
        assert_eq!(
            derive_output_freshness(
                false,
                [Freshness::Intermediate { generation: 0 }].into_iter(),
                g
            ),
            Freshness::Intermediate { generation: g },
            "Intermediate input must yield Intermediate output"
        );

        // Pending input → Pending output (§7.2 line 748 + §9.2 line 890 carve-out).
        // Chain forwarding is at the `_with_cause` layer; the pure helper drops the chain.
        assert_eq!(
            derive_output_freshness(
                false,
                [Freshness::Pending {
                    last_substantive: ResultRef::none()
                }]
                .into_iter(),
                g
            ),
            Freshness::Pending {
                last_substantive: ResultRef::none()
            },
            "Pending input must yield Pending output per §7.2 line 748 (downstream subtree quieted)"
        );

        // Failed input → Pending output (§9.2 line 890 carve-out).
        assert_eq!(
            derive_output_freshness(
                false,
                [Freshness::Failed {
                    error: ErrorRef::new("type mismatch")
                }]
                .into_iter(),
                g
            ),
            Freshness::Pending {
                last_substantive: ResultRef::none()
            },
            "Failed input must yield Pending output per §9.2 line 890 carve-out"
        );
    }

    /// Pins the cause-bearing variant `derive_output_freshness_with_cause`:
    /// Failed input contributes its own NodeId as the chain root; Pending input
    /// forwards the upstream entry's `pending_cause`; all-Final inputs return None.
    #[test]
    fn derive_output_freshness_with_cause_returns_failing_node() {
        use reify_ir::{ErrorRef, Freshness, ResultRef};
        let g = 9u64;

        let leaf = NodeId::Value(ValueCellId::new("T", "leaf"));
        let mid = NodeId::Value(ValueCellId::new("T", "mid"));

        // (a) Failed input → (Pending{none()}, Some(failing_node))
        let (fresh, cause) = derive_output_freshness_with_cause(
            false,
            [(
                leaf.clone(),
                Freshness::Failed {
                    error: ErrorRef::new("boom"),
                },
                None,
            )],
            g,
        );
        assert_eq!(
            fresh,
            Freshness::Pending {
                last_substantive: ResultRef::none()
            },
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
                Freshness::Pending {
                    last_substantive: ResultRef::none(),
                },
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
        let (fresh3, cause3) =
            derive_output_freshness_with_cause(false, [(leaf.clone(), Freshness::Final, None)], g);
        assert_eq!(
            fresh3,
            Freshness::Final,
            "All-Final inputs must yield Final"
        );
        assert_eq!(cause3, None, "All-Final inputs have no chain cause");
    }

    // --- derive_output_freshness_from_trace tests (task #2328 amendment) ---

    /// Verifies that derive_output_freshness_from_trace uses the *supplied* trace
    /// (not whatever is cached for a node) and delegates to the §7.2 rule correctly.
    #[test]
    fn derive_output_freshness_from_trace_uses_supplied_trace() {
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

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
        let trace_a_only = DependencyTrace { realization_reads: Vec::new(),
            reads: vec![a_id.clone()],
        };
        assert_eq!(
            store.derive_output_freshness_from_trace(&trace_a_only, false, 7),
            Freshness::Final,
            "trace reading only Final input must yield Final"
        );

        // A trace that reads `b` (Intermediate) → should yield Intermediate
        let trace_b_only = DependencyTrace { realization_reads: Vec::new(),
            reads: vec![b_id.clone()],
        };
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
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        let a_id = ValueCellId::new("T", "a");
        let b_id = ValueCellId::new("T", "b");

        // Cold-start: trace reads `a`
        let trace_a = DependencyTrace { realization_reads: Vec::new(),
            reads: vec![a_id.clone()],
        };
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
        let trace_b = DependencyTrace { realization_reads: Vec::new(),
            reads: vec![b_id.clone()],
        };
        let outcome = store.record_evaluation_with_freshness(
            node.clone(),
            CachedResult::Value(Value::Int(42), DeterminacyState::Determined),
            VersionId(2),
            trace_b,
            Freshness::Final,
        );
        assert_eq!(
            outcome,
            EvalOutcome::Unchanged,
            "same hash must trigger early cutoff"
        );
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
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

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
        let trace = DependencyTrace { realization_reads: Vec::new(),
            reads: vec![a_id.clone()],
        };
        let out_node = NodeId::Value(out_id.clone());
        let outcome = store.record_evaluation_propagating_freshness(
            out_node.clone(),
            CachedResult::Value(Value::Real(10.0), DeterminacyState::Determined),
            VersionId(7), // generation = version.0 = 7
            trace,
            false, // still_refining
        );
        assert_eq!(
            outcome,
            EvalOutcome::Changed,
            "cold start must return Changed"
        );
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
        use reify_core::VersionId;
        use reify_ir::Freshness;

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
                    reify_ir::Value::Int(1),
                    reify_ir::DeterminacyState::Determined,
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
                reify_ir::Value::Int(42),
                reify_ir::DeterminacyState::Determined,
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
        use reify_ir::ErrorRef;

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
        use reify_ir::ResultRef;

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
        let entry = store.get(&mid_node).expect("seeded entry must be present");
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
        use reify_ir::ErrorRef;

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
    /// `let (f, _) = self._with_cause(...); f` wrappers, so these rows are
    /// tautological by construction. Four rows pin the agreement at distinct
    /// branches of the classifier: all-Final (simplest path), one-Pending
    /// (non-trivial path), Failed §9.2 carve-out, and still_refining short-circuit.
    /// If a method is ever accidentally hand-rolled again, at least one row will
    /// diverge.
    #[test]
    fn derive_output_freshness_no_cause_variants_agree_with_with_cause() {
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, ErrorRef, Freshness, Value};

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
                let for_node_cause = $store
                    .derive_output_freshness_for_node_with_cause(&out, $sr, $g)
                    .0;
                assert_eq!(
                    for_node, for_node_cause,
                    "derive_output_freshness_for_node vs _with_cause disagree for {}",
                    $label
                );
                let from_trace = $store.derive_output_freshness_from_trace($trace, $sr, $g);
                let from_trace_cause = $store
                    .derive_output_freshness_from_trace_with_cause($trace, $sr, $g)
                    .0;
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
            let trace = DependencyTrace { realization_reads: Vec::new(),
                reads: vec![a_id.clone(), b_id.clone()],
            };
            assert_agree!(store, &trace, sr, g, "all-Final");
        }

        // Row 2: one Pending (non-trivial classifier path; exercises the main divergence risk)
        {
            let mut store = make_store(Freshness::Final, Freshness::Final);
            store.mark_pending(&NodeId::Value(b_id.clone()));
            let trace = DependencyTrace { realization_reads: Vec::new(),
                reads: vec![a_id.clone(), b_id.clone()],
            };
            assert_agree!(store, &trace, sr, g, "one-Pending");
        }

        // Row 3: Failed input (exercises §9.2 carve-out — most likely divergence point)
        {
            let mut store = make_store(Freshness::Final, Freshness::Final);
            store.mark_failed(&NodeId::Value(b_id.clone()), ErrorRef::new("x"));
            let trace = DependencyTrace { realization_reads: Vec::new(),
                reads: vec![a_id.clone(), b_id.clone()],
            };
            assert_agree!(store, &trace, sr, g, "one-Failed");
        }

        // Row 4: still_refining=true (exercises the Pending-when-still-refining branch)
        {
            let store = make_store(Freshness::Final, Freshness::Final);
            let trace = DependencyTrace { realization_reads: Vec::new(),
                reads: vec![a_id.clone(), b_id.clone()],
            };
            assert_agree!(store, &trace, true, g, "all-Final-still-refining");
        }
    }

    /// GHR-δ S5: `derive_output_freshness_from_trace_with_cause` folds
    /// `realization_reads` into the freshness meet alongside `reads`, and the
    /// cached-trace variant `derive_output_freshness_for_node_with_cause` agrees.
    ///
    /// A trace with `reads=[]`, `realization_reads=[R0]` must derive:
    ///   R0 Pending (chain root) → Pending, cause Some(Realization(R0))
    ///   R0 Intermediate{g}      → Intermediate{g}, no cause
    ///   R0 Final                → Final, no cause
    ///
    /// RED until S6 chains the realization triples into the meet (today only
    /// `trace.reads` is consulted, so an empty-`reads` trace always yields Final).
    #[test]
    fn derive_output_freshness_folds_realization_reads() {
        const GEN: u64 = 5;
        let r0 = RealizationNodeId::new("Widget", 0);
        let gh = ValueCellId::new("Widget", "body");
        let r0_node = NodeId::Realization(r0.clone());
        let gh_node = NodeId::Value(gh.clone());

        // Forward trace reads only the realization (no VC reads).
        let trace = DependencyTrace {
            reads: vec![],
            realization_reads: vec![r0.clone()],
        };

        // Seed: R0 present (Final) + a GH cell whose CACHED trace carries
        // realization_reads=[R0] (exercises the for-node / cached-trace variant).
        let seed = || {
            let mut store = CacheStore::new();
            store.put(
                r0_node.clone(),
                NodeCache::new(
                    CachedResult::GeometryHandle(GeometryHandleId(7)),
                    Freshness::Final,
                    DependencyTrace::default(),
                    VersionId(1),
                ),
            );
            store.put(
                gh_node.clone(),
                NodeCache::new(
                    CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                    Freshness::Final,
                    DependencyTrace {
                        reads: vec![],
                        realization_reads: vec![r0.clone()],
                    },
                    VersionId(1),
                ),
            );
            store
        };

        // (a) R0 Pending with cause = itself → Pending output, cause forwards.
        {
            let mut store = seed();
            store.mark_pending_with_cause(&r0_node, r0_node.clone());
            let (f, cause) =
                store.derive_output_freshness_from_trace_with_cause(&trace, false, GEN);
            assert!(
                matches!(f, Freshness::Pending { .. }),
                "Pending realization input must yield Pending output, got {:?}",
                f
            );
            assert_eq!(
                cause,
                Some(r0_node.clone()),
                "Pending cause must forward to the realization node"
            );
            let (f2, cause2) =
                store.derive_output_freshness_for_node_with_cause(&gh_node, false, GEN);
            assert!(
                matches!(f2, Freshness::Pending { .. }),
                "for-node (cached-trace) variant must agree on Pending"
            );
            assert_eq!(cause2, Some(r0_node.clone()), "for-node variant cause must agree");
        }

        // (b) R0 Intermediate{GEN} → Intermediate{GEN}, no cause.
        {
            let mut store = seed();
            store.set_freshness(&r0_node, Freshness::Intermediate { generation: GEN });
            let (f, cause) =
                store.derive_output_freshness_from_trace_with_cause(&trace, false, GEN);
            assert_eq!(f, Freshness::Intermediate { generation: GEN });
            assert_eq!(cause, None);
            let (f2, _) =
                store.derive_output_freshness_for_node_with_cause(&gh_node, false, GEN);
            assert_eq!(
                f2,
                Freshness::Intermediate { generation: GEN },
                "for-node variant must agree on Intermediate"
            );
        }

        // (c) R0 Final (as seeded) → Final, no cause.
        {
            let store = seed();
            let (f, cause) =
                store.derive_output_freshness_from_trace_with_cause(&trace, false, GEN);
            assert_eq!(f, Freshness::Final);
            assert_eq!(cause, None);
            let (f2, _) =
                store.derive_output_freshness_for_node_with_cause(&gh_node, false, GEN);
            assert_eq!(f2, Freshness::Final, "for-node variant must agree on Final");
        }
    }

    // ── Imported-file content-hash side-table tests (PRD task 4 / task 2668) ─────

    /// Verifies the record/get API for the imported-file content-hash side-table.
    ///
    /// Asserts:
    /// (a) Fresh store → `get_imported_file_hash` returns `None`.
    /// (b) After recording, `get_imported_file_hash` returns `Some(hash)`.
    /// (c) Overwriting with a different hash updates the stored value.
    /// (d) An unrelated path is untouched.
    #[test]
    fn cache_store_records_and_retrieves_imported_file_hashes() {
        let mut store = CacheStore::new();

        // (a) Fresh store has no records.
        assert_eq!(
            store.get_imported_file_hash("/any/path"),
            None,
            "fresh store must return None for any path"
        );

        // (b) Record hash A for "foo.vdb" → retrieve it.
        store.record_imported_file_hash("foo.vdb", ContentHash::of_str("A"));
        assert_eq!(
            store.get_imported_file_hash("foo.vdb"),
            Some(ContentHash::of_str("A")),
            "get must return the recorded hash"
        );

        // (c) Overwrite with hash B → new value visible.
        store.record_imported_file_hash("foo.vdb", ContentHash::of_str("B"));
        assert_eq!(
            store.get_imported_file_hash("foo.vdb"),
            Some(ContentHash::of_str("B")),
            "second record must overwrite the first"
        );

        // (d) Unrelated path is untouched.
        assert_eq!(
            store.get_imported_file_hash("bar.vdb"),
            None,
            "unrelated path must remain None"
        );
    }

    /// Verifies that `CacheStore::clear` drops the `imported_file_hashes` side-table.
    ///
    /// Asserts:
    /// (a) Record a hash for "foo.vdb".
    /// (b) Sanity-check: `get_imported_file_hash` returns `Some(...)` before clear.
    /// (c) Call `store.clear()`.
    /// (d) Post-clear: `get_imported_file_hash("foo.vdb") == None`.
    /// (e) Post-clear: `imported_file_hash_changed("foo.vdb", hash_A) == true`
    ///     (prior recording is gone; predicate signals cold-start invalidation).
    #[test]
    fn cache_store_clear_drops_imported_file_hashes() {
        let mut store = CacheStore::new();
        let hash_a = ContentHash::of_str("A");

        // (a) Record a hash.
        store.record_imported_file_hash("foo.vdb", hash_a);

        // (b) Sanity-check before clear.
        assert_eq!(
            store.get_imported_file_hash("foo.vdb"),
            Some(hash_a),
            "sanity: hash must be visible before clear"
        );

        // (c) Clear.
        store.clear();

        // (d) Post-clear: no recording.
        assert_eq!(
            store.get_imported_file_hash("foo.vdb"),
            None,
            "clear must drop the imported_file_hashes side-table"
        );

        // (e) Post-clear: predicate signals invalidation (cold start).
        assert!(
            store.imported_file_hash_changed("foo.vdb", hash_a),
            "post-clear, imported_file_hash_changed must return true (no prior recording)"
        );
    }

    /// Verifies the three branches of the `imported_file_hash_changed` invalidation predicate.
    ///
    /// (a) No prior recording → returns `true` (cold start → must invalidate).
    /// (b) Recorded hash matches candidate → returns `false` (cache hit).
    /// (c) Recorded hash differs from candidate → returns `true` (content changed → invalidate).
    #[test]
    fn cache_store_imported_file_hash_changed_returns_true_on_first_seen_or_differing_hash() {
        let mut store = CacheStore::new();

        // (a) No recording → changed == true.
        assert!(
            store.imported_file_hash_changed("foo.vdb", ContentHash::of_str("A")),
            "no prior recording must signal invalidation (changed == true)"
        );

        // Record hash A.
        store.record_imported_file_hash("foo.vdb", ContentHash::of_str("A"));

        // (b) Same hash → changed == false (cache hit).
        assert!(
            !store.imported_file_hash_changed("foo.vdb", ContentHash::of_str("A")),
            "matching recorded hash must signal cache hit (changed == false)"
        );

        // (c) Different hash → changed == true (content changed).
        assert!(
            store.imported_file_hash_changed("foo.vdb", ContentHash::of_str("B")),
            "differing hash must signal invalidation (changed == true)"
        );
    }

    /// Pins direct admission of `NodeId::Compute(_)` as a `pending_cause`
    /// chain root — PRD §3 "Chain-root contract extension". A
    /// `NodeId::Compute(N)` may be stored as the `pending_cause` of a
    /// downstream `NodeId::Value(V)` entry; reading back via `pending_cause`
    /// must return `Some(NodeId::Compute(N))`. Contract-pinning of
    /// already-correct behaviour (the underlying `Option<NodeId>` field is
    /// variant-agnostic); made explicit per PRD §8 task α.
    #[test]
    fn cache_store_pending_cause_admits_compute_chain_root() {
        use reify_core::ComputeNodeId;

        let compute_node = NodeId::Compute(ComputeNodeId::new("T", 0));

        let mut store = CacheStore::new();
        let v_id = ValueCellId::new("T", "v");
        store.put(NodeId::Value(v_id.clone()), make_seed_entry());

        assert!(
            store.mark_pending_with_cause(&NodeId::Value(v_id.clone()), compute_node.clone()),
            "mark_pending_with_cause must return true for an existing entry \
             (PRD §3 chain-root contract extension)"
        );
        assert_eq!(
            store.pending_cause(&NodeId::Value(v_id.clone())),
            Some(compute_node),
            "pending_cause must admit NodeId::Compute(_) as chain root \
             (PRD §3 chain-root contract extension)"
        );
    }

    /// Pins forwarder semantics for a `NodeId::Compute(_)` chain root through
    /// `derive_output_freshness_with_cause` — PRD §3 "Chain-root contract
    /// extension". A Pending Value input whose `upstream_cause =
    /// Some(NodeId::Compute(N))` must forward the Compute chain root unchanged
    /// through the Pending-forwarding branch (cache.rs:1080-1135). The
    /// forwarder branch is variant-agnostic; made explicit per PRD §8 task α.
    #[test]
    fn derive_output_freshness_with_cause_forwards_compute_chain_root() {
        use reify_core::ComputeNodeId;
        use reify_ir::{Freshness, ResultRef};

        let compute_node = NodeId::Compute(ComputeNodeId::new("T", 0));
        let mid_id = ValueCellId::new("T", "mid");

        let (fresh, cause) = derive_output_freshness_with_cause(
            false,
            [(
                NodeId::Value(mid_id),
                Freshness::Pending {
                    last_substantive: ResultRef::none(),
                },
                Some(compute_node.clone()),
            )],
            0u64,
        );
        assert!(
            matches!(fresh, Freshness::Pending { .. }),
            "Pending input must yield Pending output (cache.rs:1080-1135)"
        );
        assert_eq!(
            cause,
            Some(compute_node),
            "Pending-forwarding branch must carry NodeId::Compute(_) cause unchanged \
             (PRD §3 chain-root contract extension; cache.rs:1080-1135)"
        );
    }

    /// RED (task 3423/δ step-2): `begin_compute_dispatch` on an EXISTING
    /// output-VC entry routes through `mark_pending_with_cause`, capturing
    /// `last_substantive` from the prior `result_hash` and recording the
    /// `NodeId::Compute(c_id)` chain root. Pins the existing-entry branch of
    /// the begin lifecycle (PRD §3 "Atomic completion" step 0 / §8 task δ).
    #[test]
    fn begin_compute_dispatch_marks_existing_output_pending_with_compute_cause() {
        use reify_core::ComputeNodeId;
        use reify_ir::{Freshness, ResultRef};

        let mut store = CacheStore::new();
        let b = ValueCellId::new("T", "b");

        // Existing Final entry: Value::Int(42) @ VersionId(1) (via make_seed_entry).
        store.put(NodeId::Value(b.clone()), make_seed_entry());
        let prior_hash = store
            .get(&NodeId::Value(b.clone()))
            .expect("seeded entry must be present")
            .result_hash;

        let c_id = ComputeNodeId::new("T", 0);
        let before = store.pending_transition_count();

        let marked = store.begin_compute_dispatch(&c_id, std::slice::from_ref(&b));

        assert_eq!(
            marked, 1,
            "begin_compute_dispatch must report 1 output marked"
        );
        assert_eq!(
            store.freshness(&NodeId::Value(b.clone())),
            Freshness::Pending {
                last_substantive: ResultRef::of_hash(prior_hash),
            },
            "existing-entry begin must capture last_substantive from prior result_hash"
        );
        assert_eq!(
            store.pending_cause(&NodeId::Value(b.clone())),
            Some(NodeId::Compute(c_id)),
            "begin must record NodeId::Compute(c_id) as the chain root (PRD §3)"
        );
        assert_eq!(
            store.pending_transition_count(),
            before + 1,
            "begin_compute_dispatch must bump pending_transition_count by 1 per marked output"
        );
    }

    /// RED (task 3423/δ step-4): `begin_compute_dispatch` on an ABSENT output
    /// VC (first-time dispatch — no prior cache entry) seeds a fresh Pending
    /// entry with `last_substantive: ResultRef::none()` (no prior result to
    /// display), `pending_cause = Some(NodeId::Compute(c_id))`, and an empty
    /// dependency trace. Pins the first-time-dispatch path that step-3's
    /// GREEN does NOT yet handle — fails until step-5 extends the impl.
    #[test]
    fn begin_compute_dispatch_seeds_pending_entry_for_absent_output() {
        use reify_core::ComputeNodeId;
        use reify_ir::{Freshness, ResultRef};

        let mut store = CacheStore::new();
        let b = ValueCellId::new("T", "b");

        // No entry for `b` — first-time dispatch.
        assert!(
            store.get(&NodeId::Value(b.clone())).is_none(),
            "precondition: output VC must be absent"
        );

        let c_id = ComputeNodeId::new("T", 0);
        let marked = store.begin_compute_dispatch(&c_id, std::slice::from_ref(&b));

        assert_eq!(
            marked, 1,
            "begin_compute_dispatch must report 1 (absent VC seeded)"
        );
        let entry = store
            .get(&NodeId::Value(b.clone()))
            .expect("absent output VC must be seeded with a fresh entry");
        assert_eq!(
            entry.freshness,
            Freshness::Pending {
                last_substantive: ResultRef::none(),
            },
            "first-time dispatch has no prior result — last_substantive must be ResultRef::none()"
        );
        assert!(
            entry.dependency_trace.reads.is_empty(),
            "seeded entry must carry an empty dependency trace"
        );
        assert_eq!(
            store.pending_cause(&NodeId::Value(b.clone())),
            Some(NodeId::Compute(c_id)),
            "seeded entry must record NodeId::Compute(c_id) as the chain root (PRD §3)"
        );
    }

    /// RED (task 3423/δ step-6): the FULL begin→complete atomicity cycle.
    /// After `begin_compute_dispatch` the cache holds (Pending, prior value,
    /// Compute cause); the SINGLE call to
    /// `complete_compute_dispatch_atomically` transitions it to (Final, new
    /// value, no cause) — no public API exists to observe an incoherent
    /// (Final, prior) or (Pending, new) intermediate (PRD §3 atomic
    /// completion / §8 task δ).
    #[test]
    fn complete_compute_dispatch_atomically_writes_value_flips_freshness_clears_cause() {
        use reify_core::{ComputeNodeId, VersionId};
        use reify_ir::{DeterminacyState, Freshness, ResultRef, Value};

        let mut store = CacheStore::new();
        let b = ValueCellId::new("T", "b");
        let c_id = ComputeNodeId::new("T", 0);

        // (a) Existing Final entry: Value::Int(42) @ VersionId(1).
        store.put(NodeId::Value(b.clone()), make_seed_entry());
        let prior_hash = store
            .get(&NodeId::Value(b.clone()))
            .expect("seeded entry must be present")
            .result_hash;

        // (b) begin → Pending.
        assert_eq!(store.begin_compute_dispatch(&c_id, std::slice::from_ref(&b)), 1);

        // (c) mid-state: (Pending{last_substantive: prior}, Compute cause,
        //     prior value still on display).
        assert_eq!(
            store.freshness(&NodeId::Value(b.clone())),
            Freshness::Pending {
                last_substantive: ResultRef::of_hash(prior_hash),
            },
            "mid-dispatch freshness must be Pending with prior last_substantive"
        );
        assert_eq!(
            store.pending_cause(&NodeId::Value(b.clone())),
            Some(NodeId::Compute(c_id.clone())),
            "mid-dispatch pending_cause must be the Compute chain root"
        );
        match &store.get(&NodeId::Value(b.clone())).unwrap().result {
            CachedResult::Value(v, d) => {
                assert_eq!(
                    *v,
                    Value::Int(42),
                    "prior value must still be on display mid-dispatch"
                );
                assert_eq!(*d, DeterminacyState::Determined);
            }
            other => panic!("expected CachedResult::Value, got {other:?}"),
        }

        // (d) complete — single atomic call (does not yet exist — RED).
        let updated = store.complete_compute_dispatch_atomically(
            &c_id,
            &[(b.clone(), Value::Int(99))],
            VersionId(2),
            None,
            0.0,
        );

        // (e) post-state: (Final, new value, no cause).
        assert_eq!(updated, 1, "complete must report 1 output updated");
        assert_eq!(
            store.freshness(&NodeId::Value(b.clone())),
            Freshness::Final,
            "complete must flip freshness Pending → Final"
        );
        assert_eq!(
            store.pending_cause(&NodeId::Value(b.clone())),
            None,
            "complete must clear pending_cause (single-critical-section guarantee)"
        );
        let entry = store.get(&NodeId::Value(b.clone())).unwrap();
        match &entry.result {
            CachedResult::Value(v, d) => {
                assert_eq!(*v, Value::Int(99), "complete must write the new value");
                assert_eq!(*d, DeterminacyState::Determined);
            }
            other => panic!("expected CachedResult::Value, got {other:?}"),
        }
        assert_eq!(
            entry.result_hash,
            CachedResult::Value(Value::Int(99), DeterminacyState::Determined).content_hash(),
            "result_hash must match the new value's content hash"
        );
        assert_eq!(
            entry.basis_version,
            VersionId(2),
            "complete must stamp the supplied version"
        );
    }

    // ── ζ / task 3425 step-2: cost_per_byte field on NodeCache + accessors ──
    //
    // These pin the new `cost_per_byte` field (paired with `warm_state`) and
    // the cost-aware `donate_warm_state_with_cost` / `cost_per_byte_of` API.

    /// `NodeCache::new` defaults `cost_per_byte` to `0.0` (the entry has no
    /// warm state yet, so the cost is undefined → `0.0`).
    #[test]
    fn node_cache_new_defaults_cost_per_byte_to_zero() {
        let entry = make_seed_entry();
        assert_eq!(
            entry.cost_per_byte, 0.0,
            "NodeCache::new must default cost_per_byte to 0.0",
        );
    }

    /// `NodeCache::clone` resets `cost_per_byte` to `0.0` alongside
    /// `warm_state = None` — both fields are transient and paired.
    #[test]
    fn node_cache_clone_drops_cost_per_byte_to_zero() {
        let mut entry = make_seed_entry();
        entry.warm_state = Some(OpaqueState::new(7i32, 4));
        entry.cost_per_byte = 0.75;

        let cloned = entry.clone();
        assert!(
            cloned.warm_state.is_none(),
            "Clone must drop warm_state (transient hint)",
        );
        assert_eq!(
            cloned.cost_per_byte, 0.0,
            "Clone must reset cost_per_byte to 0.0 (paired with warm_state)",
        );
    }

    /// `record_evaluation_with_freshness` clears `cost_per_byte` on both the
    /// early-cutoff path and the changed/cold-start path (paired with the
    /// existing `warm_state = None` reset).
    #[test]
    fn record_evaluation_clears_cost_per_byte() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "rec"));
        let cached = CachedResult::Value(Value::Int(1), DeterminacyState::Determined);

        // Insert an entry directly (cold-start) and stamp warm_state + cost.
        store.put(
            node.clone(),
            NodeCache {
                result: cached.clone(),
                result_hash: cached.content_hash(),
                freshness: Freshness::Final,
                dependency_trace: DependencyTrace::default(),
                basis_version: VersionId(1),
                warm_state: Some(OpaqueState::new(7i32, 4)),
                cost_per_byte: 0.9,
                pending_cause: None,
            },
        );

        // Same hash → early-cutoff path: must reset both fields.
        let outcome = store.record_evaluation(
            node.clone(),
            cached.clone(),
            VersionId(2),
            DependencyTrace::default(),
        );
        assert_eq!(outcome, EvalOutcome::Unchanged);
        let entry = store.get(&node).expect("entry must still exist");
        assert!(entry.warm_state.is_none(), "early-cutoff must clear warm_state");
        assert_eq!(
            entry.cost_per_byte, 0.0,
            "early-cutoff must clear cost_per_byte (paired with warm_state)",
        );

        // Different hash → changed/cold-start path: also resets both.
        store
            .donate_warm_state_with_cost(&node, OpaqueState::new(8i32, 4), 0.4);
        let new_cached =
            CachedResult::Value(Value::Int(2), DeterminacyState::Determined);
        let outcome = store.record_evaluation(
            node.clone(),
            new_cached,
            VersionId(3),
            DependencyTrace::default(),
        );
        assert_eq!(outcome, EvalOutcome::Changed);
        let entry = store.get(&node).expect("entry must still exist");
        assert!(entry.warm_state.is_none(), "changed path must clear warm_state");
        assert_eq!(
            entry.cost_per_byte, 0.0,
            "changed path must clear cost_per_byte (paired with warm_state)",
        );
    }

    /// `donate_warm_state_with_cost` writes both fields and
    /// `cost_per_byte_of` reads the stored cost back.
    #[test]
    fn donate_warm_state_with_cost_stores_and_cost_per_byte_of_reads() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "dwc"));
        store.put(node.clone(), make_seed_entry());

        assert_eq!(
            store.cost_per_byte_of(&node),
            Some(0.0),
            "fresh entry's cost defaults to 0.0",
        );

        let donated = store.donate_warm_state_with_cost(
            &node,
            OpaqueState::new(123i32, 4),
            0.625,
        );
        assert!(donated, "donate must report true when the node exists");
        assert_eq!(
            store.cost_per_byte_of(&node),
            Some(0.625),
            "cost_per_byte_of must reflect the donated cost",
        );

        // Absent node → reader returns None.
        let absent = NodeId::Value(ValueCellId::new("T", "absent"));
        assert_eq!(
            store.cost_per_byte_of(&absent),
            None,
            "cost_per_byte_of on an absent node must return None",
        );
    }

    /// The 2-arg `donate_warm_state` wrapper defaults `cost_per_byte` to
    /// `0.0` (no behaviour change for legacy callers).
    #[test]
    fn donate_warm_state_two_arg_keeps_cost_at_zero() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "two_arg"));
        store.put(node.clone(), make_seed_entry());

        let donated = store.donate_warm_state(&node, OpaqueState::new(9i32, 4));
        assert!(donated, "donate must report true when the node exists");
        assert_eq!(
            store.cost_per_byte_of(&node),
            Some(0.0),
            "2-arg donate_warm_state must keep cost_per_byte at 0.0",
        );
    }

    /// ζ / task 3425 step-3: when `new_warm_state` is `Some`, the extended
    /// `complete_compute_dispatch_atomically` auto-seeds a sentinel entry
    /// under `NodeId::Compute(c_id)` (placeholder
    /// `CachedResult::Value(Value::Undef, Determined)`) and donates the
    /// warm state + cost in the same critical section as the output-VC
    /// Pending→Final flip. RED until step-4 extends the helper signature.
    #[test]
    fn complete_compute_dispatch_atomically_with_some_new_warm_state_seeds_compute_entry() {
        use reify_core::{ComputeNodeId, VersionId};
        use reify_ir::Value;

        let mut store = CacheStore::new();
        let vc = ValueCellId::new("T", "out");
        let c_id = ComputeNodeId::new("T", 0);

        // begin → mark output Pending so atomic-complete has a Pending state
        // to flip (matches production lifecycle).
        store.begin_compute_dispatch(&c_id, std::slice::from_ref(&vc));

        // Pre-condition: no entry exists under NodeId::Compute(c_id).
        assert!(
            store.get(&NodeId::Compute(c_id.clone())).is_none(),
            "precondition: NodeId::Compute(c_id) must be absent before complete",
        );

        // RED: the new signature accepts (c_id, outputs, version,
        // new_warm_state, cost_per_byte). Today the method takes only
        // (c_id, outputs, version) — this call fails to compile until
        // step-4 widens the signature.
        let updated = store.complete_compute_dispatch_atomically(
            &c_id,
            &[(vc.clone(), Value::Int(7))],
            VersionId(2),
            Some(OpaqueState::new(99i32, 4)),
            0.75,
        );
        assert_eq!(updated, 1, "complete must report 1 output updated");

        // (a) Output VC is Final with the new value and no pending_cause.
        assert_eq!(
            store.freshness(&NodeId::Value(vc.clone())),
            Freshness::Final,
            "output VC freshness must be Final after complete",
        );
        assert_eq!(
            store.pending_cause(&NodeId::Value(vc.clone())),
            None,
            "output VC pending_cause must be cleared after complete",
        );

        // (b) NodeId::Compute(c_id) entry was auto-seeded.
        assert!(
            store.get(&NodeId::Compute(c_id.clone())).is_some(),
            "complete with Some warm state must auto-seed a Compute entry",
        );

        // (c) cost_per_byte_of reads 0.75 on the Compute entry.
        assert_eq!(
            store.cost_per_byte_of(&NodeId::Compute(c_id.clone())),
            Some(0.75),
            "Compute entry must carry the donated cost",
        );

        // (d) get_warm_state returns Some(OpaqueState(99i32)) (take semantics).
        let took = store
            .get_warm_state(&NodeId::Compute(c_id.clone()))
            .expect("warm state must be donated to the Compute entry");
        assert_eq!(
            took.downcast::<i32>(),
            Some(99),
            "donated warm state must round-trip i32(99)",
        );
    }

    /// When `new_warm_state` is `None`, the extended
    /// `complete_compute_dispatch_atomically` MUST NOT seed a `Compute`
    /// entry (the entry exists only to carry warm_state + cost; absent
    /// warm state means no seed). RED until step-4.
    #[test]
    fn complete_compute_dispatch_atomically_with_none_warm_state_does_not_seed_compute_entry() {
        use reify_core::{ComputeNodeId, VersionId};
        use reify_ir::Value;

        let mut store = CacheStore::new();
        let vc = ValueCellId::new("T", "out2");
        let c_id = ComputeNodeId::new("T", 1);

        store.begin_compute_dispatch(&c_id, std::slice::from_ref(&vc));

        let updated = store.complete_compute_dispatch_atomically(
            &c_id,
            &[(vc.clone(), Value::Int(11))],
            VersionId(2),
            None,
            0.0,
        );
        assert_eq!(updated, 1, "complete must report 1 output updated");

        // Output VC still flipped to Final as before.
        assert_eq!(
            store.freshness(&NodeId::Value(vc.clone())),
            Freshness::Final,
        );

        // NodeId::Compute(c_id) entry is NOT seeded when warm state is None.
        assert!(
            store.get(&NodeId::Compute(c_id)).is_none(),
            "complete with None warm state must NOT seed a Compute entry",
        );
    }

    /// `donate_warm_state_with_cost` sanitises non-finite (`NaN`, `±inf`)
    /// and negative `cost_per_byte` to `0.0` (so a future cost-weighted-LRU
    /// `partial_cmp` is safe).
    #[test]
    fn donate_warm_state_with_cost_sanitises_nan_and_negative_to_zero() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("T", "san"));
        store.put(node.clone(), make_seed_entry());

        store.donate_warm_state_with_cost(&node, OpaqueState::new(1i32, 4), f64::NAN);
        assert_eq!(
            store.cost_per_byte_of(&node),
            Some(0.0),
            "NaN cost must sanitise to 0.0",
        );

        store.donate_warm_state_with_cost(&node, OpaqueState::new(1i32, 4), f64::INFINITY);
        assert_eq!(
            store.cost_per_byte_of(&node),
            Some(0.0),
            "+inf cost must sanitise to 0.0",
        );

        store.donate_warm_state_with_cost(&node, OpaqueState::new(1i32, 4), f64::NEG_INFINITY);
        assert_eq!(
            store.cost_per_byte_of(&node),
            Some(0.0),
            "-inf cost must sanitise to 0.0",
        );

        store.donate_warm_state_with_cost(&node, OpaqueState::new(1i32, 4), -1.5);
        assert_eq!(
            store.cost_per_byte_of(&node),
            Some(0.0),
            "negative cost must sanitise to 0.0",
        );
    }
}
