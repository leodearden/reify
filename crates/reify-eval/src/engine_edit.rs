//! Edit methods for the Reify evaluation engine (split from lib.rs, task 2032).
//!
//! # Canonical Auto-cell lifecycle rule
//!
//! **Auto cell lifecycle is owned by the constraint solver, not by guard
//! activation/deactivation.**
//!
//! Inactive-branch Auto cells retain their solver-resolved value across guard
//! transitions in BOTH of the following code paths:
//!
//! * **`Engine::eval` — post-solver guard re-evaluation** (`engine_eval.rs`):
//!   the inactive-branch `Value::Undef` write is gated with
//!   `if !cell.kind.is_auto()`, so Auto cells on the inactive side keep the
//!   value and determinacy that the solver wrote.
//!
//! * **`Engine::edit_param` — Phase 1 guard re-elaboration and post-wave2
//!   cleanup** (this file): the helper [`deactivate_if_not_auto`] writes
//!   `Undef / Undetermined` for non-Auto cells and skips Auto cells entirely.
//!
//! This was originally asymmetric (task 2143): the post-solver pass in
//! `engine_eval.rs` used to overwrite inactive-branch Auto cells with
//! `(Undef, Auto)`, destroying solver work and causing `eval(guard=T)` to
//! diverge from `eval(guard=¬T) → edit_param(guard, T)` for the same final
//! configuration.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;
use std::time::Instant;

use reify_compiler::{CompiledFunction, CompiledModule};
use reify_types::{
    AutoParam, ConstraintNodeId, ContentHash, DeterminacyState, Diagnostic, PersistentMap,
    RealizationNodeId, ResolutionProblem, SnapshotId, SnapshotProvenance, SolveResult, Value,
    ValueCellId, ValueMap, VersionId,
};

use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
use crate::deps::{DependencyTrace, extract_dependency_trace};
use crate::engine_admin::{ParamOverrideRejection, validate_param_override};
use crate::graph::{EvaluationGraph, GuardedGroupInfo};
use crate::journal::{EvalEvent, EventKind, EventPayload};
use crate::warm_pool::WarmStatePool;
use crate::{
    CheckResult, Engine, EngineError, EvalResult, EvaluationState, GuardLookup, build_meta_map,
    eval_ctx_with_meta, guard_state_fingerprint, merge_functions,
};

/// Deactivate a guarded-group member by writing `Undef` into both the working
/// `values` map and the snapshot's `values` map — UNLESS the member is an
/// `Auto` cell, whose lifecycle is owned by the constraint solver rather than
/// guard activation/deactivation. Missing cells are treated as non-Auto
/// (i.e. they get deactivated), preserving the prior `is_some_and` semantics.
///
/// See the [module-level doc](self) for the canonical Auto-cell lifecycle rule
/// shared with `engine_eval.rs`'s post-solver guard re-evaluation pass.
pub(crate) fn deactivate_if_not_auto(
    graph: &EvaluationGraph,
    id: &ValueCellId,
    values: &mut ValueMap,
    snapshot_values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
) {
    if !graph.is_auto_cell(id) {
        values.insert(id.clone(), Value::Undef);
        snapshot_values.insert(id.clone(), (Value::Undef, DeterminacyState::Undetermined));
    }
}

/// Re-elaborate the active and inactive branches of a single guarded group
/// given the already-computed guard value.
///
/// - **Active branch** (`is_true` for `members`, `is_false` for `else_members`):
///   each cell's `default_expr` is evaluated with
///   `eval_ctx_with_meta(values, functions, meta_map)` and written into
///   both `values` and `snapshot_values` with `DeterminacyState::Determined`.
///   Cells without a `default_expr` (or absent from the graph) are left
///   unchanged.
/// - **Inactive branch**: each cell is passed to `deactivate_if_not_auto`,
///   which writes `Undef / Undetermined` for non-Auto cells and skips Auto
///   cells (whose lifecycle is owned by the constraint solver).
///
/// The caller is responsible for computing and inserting the guard cell value
/// itself — this helper takes `guard_val` as input and handles only the member
/// propagation step.
fn reelaborate_guarded_group(
    graph: &EvaluationGraph,
    group: &GuardedGroupInfo,
    guard_val: &Value,
    values: &mut ValueMap,
    snapshot_values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
) {
    let is_true = matches!(guard_val, Value::Bool(true));
    let is_false = matches!(guard_val, Value::Bool(false));

    for (cells, is_active) in [(&group.members, is_true), (&group.else_members, is_false)] {
        for mid in cells {
            if is_active {
                if let Some(node) = graph.value_cells.get(mid)
                    && let Some(ref expr) = node.default_expr
                {
                    let val = reify_expr::eval_expr(
                        expr,
                        &eval_ctx_with_meta(values, functions, meta_map),
                    );
                    values.insert(mid.clone(), val.clone());
                    snapshot_values.insert(mid.clone(), (val, DeterminacyState::Determined));
                }
            } else {
                deactivate_if_not_auto(graph, mid, values, snapshot_values);
            }
        }
    }
}

/// Re-deactivate inactive-branch members for **all** guarded groups after wave2.
///
/// ## Why all groups, not just `phase1_reelaborated` (task 2144)
///
/// Wave2 re-evaluates every cell in the dirty cone of resolved auto-param IDs.
/// An inactive-branch member whose `default_expr` reads a resolved auto param
/// is in that dirty cone regardless of whether its *guard* was in Phase 1's
/// dirty-guard trigger.  Three categories of groups may be affected:
///
/// (a) Groups Phase 1 **re-elaborated** — guard flipped, Phase 1 deactivated the
///     inactive branch, wave2 then overwrote it.  Previously covered; now
///     covered as a natural subset of "all groups".
///
/// (b) Groups Phase 1 **skipped via per-group unchanged-guard short-circuit** —
///     guard is in Phase 1's iteration but its value is unchanged, so the group
///     takes the per-group unchanged-guard short-circuit (`continue`) without entering `phase1_reelaborated`.
///     Wave2 can still overwrite the inactive-branch member.  Previously *not*
///     covered — this was the task 2144 bug.
///
/// (c) Groups **outside any dirty-guard trigger** — `has_dirty_guards` was false
///     and Phase 1 never ran at all.  The pre-edit guard value seeded from
///     `new_snapshot.values` at edit-start is still valid in `values`.
///
/// The cleanup is idempotent: `deactivate_if_not_auto` writes `Undef` over
/// `Undef` for groups wave2 did not touch, and Auto cells are always skipped.
///
/// ## Guard-value source
///
/// Phase 1 writes every group's current guard value into `values` even when it
/// takes the per-group unchanged-guard short-circuit (`continue`).
/// For groups outside Phase 1's dirty-guard trigger, `values` holds the pre-edit
/// guard value seeded from `new_snapshot.values` at edit-start (the
/// edit-start snapshot-to-values seeding pass),
/// which is non-empty for every guard cell that `eval()` has populated.
/// A `debug_assert!` verifies that the guard cell is present in `values`; a
/// missing entry indicates a real invariant violation (the cell was neither
/// seeded by Phase 1 nor by the edit-start snapshot pass) and should be caught
/// early in debug builds.  In release builds the `unwrap_or(Value::Undef)`
/// fallback mirrors how Phase 1 itself handles a missing guard node (Phase 1's missing-guard-node fallback),
/// treating both branches as inactive rather than panicking.
///
/// ## Note on `phase1_reelaborated`
///
/// This helper no longer takes `phase1_reelaborated` as a parameter — the set
/// was only used to gate which groups to iterate, and that gate is now removed.
/// Phase 3's `phase1_reelaborated.contains(...)` dedup gates in `edit_param` and
/// `edit_source` are unaffected; those sets remain in the caller.
///
/// Called from both `edit_param` post-wave2 and `edit_source` post-wave2.
/// Field-level borrow splitting still applies: `graph` and `snapshot_values`
/// are passed separately so no `.clone()` of `guarded_groups` is required.
fn reapply_guard_deactivations_post_wave2(
    graph: &EvaluationGraph,
    values: &mut ValueMap,
    snapshot_values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
) {
    for group in &graph.guarded_groups {
        debug_assert!(
            values.contains(&group.guard_cell),
            "guard cell {:?} has no value in `values` before post-wave2 cleanup — \
             this indicates an invariant violation: every guard cell must be seeded \
             either by Phase 1 or by the edit-start snapshot-to-values pass",
            group.guard_cell
        );
        let guard_val = values
            .get(&group.guard_cell)
            .cloned()
            .unwrap_or(Value::Undef);
        let is_true = matches!(&guard_val, Value::Bool(true));
        let is_false = matches!(&guard_val, Value::Bool(false));
        for (cells, is_active) in [(&group.members, is_true), (&group.else_members, is_false)] {
            if !is_active {
                for mid in cells {
                    deactivate_if_not_auto(graph, mid, values, snapshot_values);
                }
            }
        }
    }
}

/// Returns `true` iff Phase 3 **must** re-elaborate a guarded group
/// (tasks 2140, 2146).
///
/// Used for **both** the `.any()` early-exit predicate (`guard_changed`) and
/// the per-group `continue` inside the Phase 3 loop so both sites share a
/// single source of truth.
///
/// Four cases, in priority order:
///
/// (absent) Guard cell is **absent** from `values` (post-eval invariant
///     violation, defended against in `edit_source` Phase 3) → treat as
///     needing Phase 3 iff there was a prior value (structural change).
/// (a) Phase 1 processed this group **with the same guard value** as the
///     current one → Phase 1's work is still valid → skip (return false).
/// (b) Phase 1 processed this group **with a different guard value** (wave2
///     flipped the guard after Phase 1 ran) → the group is in an intermediate
///     state; must re-elaborate unconditionally, regardless of what the old
///     snapshot says (task 2146) → return true.
/// (c) Phase 1 **did not** touch this group → apply the standard old-vs-new
///     check: re-elaborate only when `old_guard_val != Some(current)`.
fn group_needs_phase3(
    group: &GuardedGroupInfo,
    values: &ValueMap,
    old_guard_val: Option<&Value>,
    phase1: &HashMap<ValueCellId, Value>,
) -> bool {
    match values.get(&group.guard_cell) {
        None => old_guard_val.is_some(),
        Some(new_val) => match phase1.get(&group.guard_cell) {
            Some(p1_val) if p1_val == new_val => false, // (a) Phase 1 still valid → skip
            Some(_) => true,                            // (b) wave2 flip → must re-elaborate
            None => old_guard_val != Some(new_val),     // (c) old-vs-new skip
        },
    }
}

/// Shared guard-cell lookup used by Phase 3 in both `Engine::edit_param` and
/// `Engine::edit_source` (one call site each).
///
/// Returns `Some(v.clone())` when present; on `None`, emits `tracing::warn!`,
/// fires `debug_assert!(false)` in debug builds, and returns `None`.  The
/// absent-guard arm is unreachable today (Phase 1 seeds every guard cell) but
/// becomes reachable after a future refactor that narrows
/// `structure_controlling` (task 2229).
///
/// **Warn-before-assert ordering is load-bearing**: the WARN must fire before
/// the `debug_assert!` so a `CountingSubscriber` can observe the counter
/// increment even when the `debug_assert!` subsequently unwinds the thread.
fn phase3_get_guard_val(values: &ValueMap, guard_cell: &ValueCellId) -> Option<Value> {
    match values.get(guard_cell) {
        Some(v) => Some(v.clone()),
        None => {
            // Warn first (observable to CountingSubscriber), debug_assert! second, return None third — ordering is load-bearing; see fn doc.
            tracing::warn!(
                target: "reify_eval::engine_edit",
                guard_cell = %guard_cell,
                "Phase 3 guard cell absent from `values` after group_needs_phase3 \
                 returned true — unreachable in current flow but possible after a \
                 future refactor that narrows structure_controlling (task 2229); \
                 skipping this group"
            );
            debug_assert!(
                false,
                "phase3_get_guard_val: guard cell {:?} absent from `values` after \
                 group_needs_phase3 returned true — unreachable in current flow but \
                 possible after a future refactor that narrows structure_controlling \
                 (task 2229)",
                guard_cell
            );
            None
        }
    }
}

/// Look up the pre-edit snapshot guard value for `gc` from the engine's
/// `eval_state`, returning a borrowed `&Value` (lifetime tied to `eval_state`).
///
/// Used at the outer `.any(|group| group_needs_phase3(...))` predicate **and**
/// the per-group `continue` check inside the Phase 3 loop in both `edit_param`
/// and `edit_source` — four call sites total — so the extraction lives in one
/// place rather than being repeated verbatim each time.
fn old_guard_for<'a>(
    eval_state: Option<&'a EvaluationState>,
    gc: &ValueCellId,
) -> Option<&'a Value> {
    eval_state
        .and_then(|s| s.snapshot.values.get(gc))
        .map(|(v, _)| v)
}

/// Build a role map from a slice of `GuardedGroupInfo` for the role-flip
/// probe in `Engine::edit_source`.
///
/// The returned map is keyed by `ValueCellId` and maps to
/// `(guard_cell, branch_tag)` where `branch_tag` is `0u8` for `members`
/// (guard = true) and `1u8` for `else_members` (guard = false).
///
/// When a `ValueCellId` appears in both `members` and `else_members` of the
/// **same** group, the `else_members` entry wins (last-write semantics); this
/// is an observable pattern in valid compiled modules (e.g. a cell that is the
/// "effective" output regardless of which branch is active).
///
/// # Panics (debug builds only)
///
/// In debug builds the function panics if any `ValueCellId` appears in two
/// groups that have **different** `guard_cell`s, i.e. the cell is claimed by
/// two distinct guards.  Intra-group duplicates (same `guard_cell`) are
/// permitted and resolved by last-write-wins.
fn build_role_map(groups: &[GuardedGroupInfo]) -> HashMap<ValueCellId, (ValueCellId, u8)> {
    let capacity: usize = groups
        .iter()
        .map(|g| g.members.len() + g.else_members.len())
        .sum();
    let mut roles: HashMap<ValueCellId, (ValueCellId, u8)> = HashMap::with_capacity(capacity);
    for group in groups.iter() {
        for mid in &group.members {
            let prev = roles.insert(mid.clone(), (group.guard_cell.clone(), 0u8));
            debug_assert!(
                prev.is_none_or(|(prev_guard, _)| prev_guard == group.guard_cell),
                "ValueCellId {:?} appeared in multiple guarded-group roles",
                mid
            );
        }
        for mid in &group.else_members {
            let prev = roles.insert(mid.clone(), (group.guard_cell.clone(), 1u8));
            debug_assert!(
                prev.is_none_or(|(prev_guard, _)| prev_guard == group.guard_cell),
                "ValueCellId {:?} appeared in multiple guarded-group roles",
                mid
            );
        }
    }
    roles
}

/// Detect whether any guarded-group member has changed its role (guard cell or
/// branch) between the old and new evaluation graph.
///
/// Returns `true` if a role-flip is detected; `false` if the guard membership
/// is structurally unchanged.
///
/// ## Correctness
///
/// Both `old_groups` and `new_groups` are reduced through the same
/// [`build_role_map`] helper before comparison.  This applies the same
/// last-write-wins semantics to both sides, so intra-group duplicates (a cell
/// appearing in both `members` and `else_members` of the same group — an
/// affirmatively supported pattern documented in `build_role_map`'s
/// docstring) resolve identically on both sides.  `HashMap` equality then
/// covers both the per-element role check **and** the dedup'd key-count check
/// in a single comparison, eliminating the spurious mismatch that the old
/// inline walk produced for that shape.
///
/// ## Performance
///
/// The empty-case fast path avoids allocating two empty `HashMap`s in the
/// common no-guarded-groups case.  Otherwise two O(N) passes over the group
/// lists are performed (one per side), followed by an O(N) map-equality check —
/// same asymptotic cost as the previous short-circuit walk, with simpler code.
/// The previous inline walk short-circuited on the first mismatch; this
/// implementation always materialises both maps.  For the typical case of
/// no-flip (maps equal) the cost is unchanged; for the flip case it loses the
/// early exit.  If guarded-group counts grow large in practice, revisit: build
/// `build_role_map(old_groups)` first, then iterate `new_groups` with an early
/// exit against that map plus a running count check (symmetric for duplicates).
fn detect_role_flip(old_groups: &[GuardedGroupInfo], new_groups: &[GuardedGroupInfo]) -> bool {
    if old_groups.is_empty() && new_groups.is_empty() {
        return false;
    }
    build_role_map(old_groups) != build_role_map(new_groups)
}

/// Invoke [`detect_role_flip`] and bump the probe counter in one place.
///
/// Both call sites in `Engine::edit_source` Phase 1 — the outer short-circuit
/// block and the per-group lazy-memo `None` arm — share the same two-line
/// pattern: call `detect_role_flip`, increment the counter.  This free function
/// ensures the counter increment cannot be omitted if a third call site ever
/// appears.
///
/// Accepts explicit disjoint borrows rather than `&mut Engine` so it can be
/// called while [`PendingWarmSeedsGuard`] holds `&mut warm_pool` (which would
/// prevent a whole-self `&mut self` method call).
fn probe_role_flip(
    old_groups: &[GuardedGroupInfo],
    new_groups: &[GuardedGroupInfo],
    counter: &mut usize,
) -> bool {
    let result = detect_role_flip(old_groups, new_groups);
    *counter += 1;
    result
}

/// Returns `true` when the guard cell's value in the pre-edit snapshot equals
/// `new_val`, meaning the guard has not changed and member re-elaboration can
/// be skipped.
///
/// Accepts the snapshot `values` map directly (rather than the full
/// `EvaluationState`) to keep callers and unit tests dependency-free.
fn guard_value_unchanged(
    snapshot_values: Option<&PersistentMap<ValueCellId, (Value, DeterminacyState)>>,
    guard_cell: &ValueCellId,
    new_val: &Value,
) -> bool {
    snapshot_values
        .and_then(|vs| vs.get(guard_cell))
        .map(|(v, _)| v)
        == Some(new_val)
}

/// Generic identity/equivalence diff between two `PersistentMap<Id, Node>`
/// collections.
///
/// Classifies every `Id` across the two maps into three disjoint sets by
/// comparing per-node content hashes (extracted via `content_hash_fn`):
///
/// - `changed`: present in both maps, content hash differs.
/// - `added`: present only in the new map.
/// - `removed`: present only in the old map.
///
/// A match signals "equivalent node; cached value is still valid"; a
/// mismatch signals "re-evaluate". This is the shared kernel of the three
/// graph-level diffs (`diff_value_cells`, `diff_constraints`,
/// `diff_realizations`) — every one of them wants the same three-set
/// classification, so any future tweak (e.g. returning counts, emitting a
/// Modified variant, handling content_hash collisions) lives in one place.
fn diff_nodes<Id, Node, F>(
    old_map: &PersistentMap<Id, Node>,
    new_map: &PersistentMap<Id, Node>,
    content_hash_fn: F,
) -> (HashSet<Id>, HashSet<Id>, HashSet<Id>)
where
    Id: Clone + Eq + Hash,
    Node: Clone,
    F: Fn(&Node) -> ContentHash,
{
    let mut changed = HashSet::new();
    let mut added = HashSet::new();
    for (id, new_node) in new_map.iter() {
        match old_map.get(id) {
            Some(old_node) => {
                if content_hash_fn(old_node) != content_hash_fn(new_node) {
                    changed.insert(id.clone());
                }
            }
            None => {
                added.insert(id.clone());
            }
        }
    }
    let mut removed = HashSet::new();
    for (id, _) in old_map.iter() {
        if !new_map.contains_key(id) {
            removed.insert(id.clone());
        }
    }
    (changed, added, removed)
}

/// The `(changed, added, removed)` triple returned by [`diff_value_cells`]
/// and captured by [`crate::Engine::last_diff_value_cells`].
///
/// - `0` (`changed`): present in both graphs with differing `content_hash`.
/// - `1` (`added`):   present only in the new graph.
/// - `2` (`removed`): present only in the old graph.
pub(crate) type ValueCellDiff = (
    HashSet<ValueCellId>,
    HashSet<ValueCellId>,
    HashSet<ValueCellId>,
);

/// Classify every `ValueCellId` across a pair of graphs into three disjoint
/// sets by comparing per-node `ValueCellNode::content_hash`:
///
/// - `changed`: present in both graphs with differing `content_hash`.
/// - `added`: present only in the new graph.
/// - `removed`: present only in the old graph.
///
/// The content_hash already combines the cell's ID hash and expression
/// content_hash (see `EvaluationGraph::from_templates`), so a match signals
/// "equivalent node; cached value is still valid" while a mismatch signals
/// "re-evaluate". This is the identity/equivalence key used by
/// `Engine::edit_source`.
pub(crate) fn diff_value_cells(
    old_graph: &EvaluationGraph,
    new_graph: &EvaluationGraph,
) -> ValueCellDiff {
    diff_nodes(&old_graph.value_cells, &new_graph.value_cells, |n| {
        n.content_hash
    })
}

/// Constraint-node analogue of [`diff_value_cells`]: classify every
/// `ConstraintNodeId` across a pair of graphs into `(changed, added, removed)`
/// by comparing per-node `ConstraintNodeData::content_hash`.
///
/// `ConstraintNodeId` is positional (`entity, index`) within its template, so a
/// re-ordering of constraint declarations in source surfaces here as a
/// `changed` diff at the shifted indexes — not as add+remove. This matches
/// `EvaluationGraph::from_templates`, which assigns indexes from the
/// constraint's declaration order.
pub(crate) fn diff_constraints(
    old_graph: &EvaluationGraph,
    new_graph: &EvaluationGraph,
) -> (
    HashSet<ConstraintNodeId>,
    HashSet<ConstraintNodeId>,
    HashSet<ConstraintNodeId>,
) {
    diff_nodes(&old_graph.constraints, &new_graph.constraints, |n| {
        n.content_hash
    })
}

/// Realization-node analogue of [`diff_value_cells`]: classify every
/// `RealizationNodeId` across a pair of graphs into `(changed, added, removed)`
/// by comparing per-node `RealizationNodeData::content_hash`.
///
/// Uses the same positional-identity convention as constraints.
pub(crate) fn diff_realizations(
    old_graph: &EvaluationGraph,
    new_graph: &EvaluationGraph,
) -> (
    HashSet<RealizationNodeId>,
    HashSet<RealizationNodeId>,
    HashSet<RealizationNodeId>,
) {
    diff_nodes(&old_graph.realizations, &new_graph.realizations, |n| {
        n.content_hash
    })
}

/// Drop-guard for the `pending_warm_seeds` staging map used in `Engine::edit_source`
/// between steps (4c) and (14b).
///
/// # Safety contract
///
/// `WarmStatePool::checkout` has take semantics — once an entry is removed from the
/// pool it cannot be recovered except by re-donation.  Today no `?` or
/// `return Err(...)` exists between steps (4c) and (14b); the guard primarily
/// protects against panics (e.g. a failed `unwrap()` on `eval_state` or a snapshot
/// operation), and against any `?` / early-return added by a future refactor.
///
/// This guard fixes the hazard: it holds both the staging map **and** a
/// `&mut WarmStatePool` reborrow.  On `Drop` it drains `self.map` and re-donates
/// every surviving entry to `self.pool`, ensuring recoverability regardless of how
/// the enclosing scope exits.
///
/// On the success path, call [`drain_into_cache_or_repool`](Self::drain_into_cache_or_repool)
/// before the guard goes out of scope.  That method empties `self.map` so the natural
/// `Drop` is a no-op (no double-donation).
///
/// # Borrow-checker note
///
/// The guard holds `&'a mut WarmStatePool` but no other `Engine` field.  Rust's
/// disjoint-field borrow rules therefore allow the caller to hold simultaneous
/// `&mut self.cache`, `&mut self.solver`, etc. while the guard is live.  Step (9)'s
/// `donate_warm_state_and_invalidate` calls use [`pool_mut`](Self::pool_mut) to obtain a
/// re-borrow of the pool through the guard rather than accessing `self.warm_pool`
/// directly.
struct PendingWarmSeedsGuard<'a> {
    map: HashMap<NodeId, (reify_types::OpaqueState, std::time::Instant)>,
    pool: &'a mut WarmStatePool,
}

impl<'a> PendingWarmSeedsGuard<'a> {
    /// Create a new guard wrapping `pool`.  The staging map starts empty.
    fn new(pool: &'a mut WarmStatePool) -> Self {
        Self {
            map: HashMap::new(),
            pool,
        }
    }

    /// Insert a checked-out entry into the staging map together with its
    /// original `last_accessed` timestamp.
    ///
    /// The `last_accessed` value should be the stamp returned by
    /// [`WarmStatePool::checkout_with_lru_stamp`] so that the guard can
    /// later pass it to [`WarmStatePool::donate_preserving_lru`] on the
    /// cache-miss path, preserving the entry's LRU position through the
    /// (4c)→(14b) round-trip (arch §4.3).
    fn insert(
        &mut self,
        nid: NodeId,
        state: reify_types::OpaqueState,
        last_accessed: std::time::Instant,
    ) {
        self.map.insert(nid, (state, last_accessed));
    }

    /// Re-borrow the pool for callers that need `&mut WarmStatePool` while
    /// the guard is live (e.g. step (9)'s `donate_warm_state_and_invalidate`).
    fn pool_mut(&mut self) -> &mut WarmStatePool {
        self.pool
    }

    /// Drain the staging map: route each entry to `cache.donate_warm_state`
    /// when the cache holds a matching entry, else re-donate to `self.pool`
    /// **preserving the original `last_accessed` stamp**.
    ///
    /// - **cache hit** → `cache.donate_warm_state(&nid, state)` (seeds the
    ///   cache's warm-state slot for the upcoming evaluation round).  The
    ///   `last_accessed` stamp is discarded on this path because the entry
    ///   leaves the pool; its LRU position no longer matters.
    /// - **cache miss** → `pool.donate_preserving_lru(nid, state, stamp)`
    ///   (re-donates the entry so it remains recoverable on a subsequent
    ///   topology event, preserving the original LRU ordering instead of
    ///   refreshing the stamp to `Instant::now()`).
    ///
    /// After this method returns, `self.map` is empty, so the natural `Drop`
    /// is a no-op (no double-donation).  On any early-return / panic between
    /// step (4c) and this call, `Drop` fires with a non-empty map and
    /// re-donates all surviving entries to the pool — early-return preservation
    /// is enforced by the guard's [`Drop`] impl.
    ///
    /// Cross-reference: see the block comment at step (14b) in `edit_source`
    /// for the full rationale for LRU-stamp preservation on the cache-miss path.
    fn drain_into_cache_or_repool(&mut self, cache: &mut CacheStore) {
        for (nid, (state, stamp)) in self.map.drain() {
            if cache.get(&nid).is_some() {
                cache.donate_warm_state(&nid, state);
            } else {
                self.pool.donate_preserving_lru(nid, state, stamp);
            }
        }
    }
}

impl Drop for PendingWarmSeedsGuard<'_> {
    /// Re-donate all remaining staged entries back to the pool, preserving
    /// each entry's original `last_accessed` stamp via
    /// [`WarmStatePool::donate_preserving_lru`].
    ///
    /// This is the panic safety net: if `drain_into_cache_or_repool` was already
    /// called (success path), `self.map` is empty and this is a no-op.  Otherwise
    /// every surviving entry is re-donated so the pool can recover it on the next
    /// `edit_source` call.
    ///
    /// Using `donate_preserving_lru` (rather than `donate`) ensures that an entry
    /// which panics out between steps (4c) and (14b) does not unfairly reset its
    /// LRU clock — the entry returns to the pool with the same age it had when it
    /// was originally checked out.
    ///
    /// When the safety net fires (non-empty map), a single `WARN`-level event is
    /// emitted with the entry count.  This makes panic-induced re-donations
    /// observable in production logs, distinguishing them from the normal
    /// (silent) success path.
    ///
    /// **Diagnostic:** when fired from inside `edit_source`, this WARN typically
    /// indicates a panic or early-return between steps (4c) and (14b).  Other
    /// call sites that drop without draining (e.g. unit tests) trigger the same
    /// WARN benignly.
    ///
    /// # Double-panic note
    ///
    /// When the safety net fires, this `Drop` impl calls `tracing::warn!`
    /// **before** calling `pool.donate_preserving_lru`.  Both are on the
    /// double-panic hot path: if `Drop` fires during stack-unwinding from
    /// another panic, either the `warn!` dispatch or `donate_preserving_lru`
    /// could theoretically panic, triggering an unconditional
    /// `std::process::abort`.
    ///
    /// `tracing::warn!` is unlikely to panic in well-behaved subscribers, but
    /// does dispatch (and may allocate) when a subscriber is attached — a
    /// buggy subscriber could in principle panic on the unwind path.
    /// `donate_preserving_lru` only panics in debug builds when the events
    /// buffer is at its cap (65 536 entries).  Both risks are documented here
    /// for completeness.
    fn drop(&mut self) {
        let len = self.map.len();
        if len == 0 {
            return;
        }
        tracing::warn!(
            target: "reify_eval::engine_edit",
            count = len,
            "PendingWarmSeedsGuard safety-net fired: re-donating staged \
             warm-pool entries from Drop (panic between edit_source steps 4c–14b \
             in production; benign in unit tests)"
        );
        for (nid, (state, stamp)) in self.map.drain() {
            self.pool.donate_preserving_lru(nid, state, stamp);
        }
    }
}

/// For each per-variant ID in `ids`, wrap it as a [`NodeId`] via `wrap` and
/// attempt to checkout warm state from `pending.pool_mut()`. On `Some(state)`,
/// insert into `pending`; on `None` (absent or LRU-evicted), the node falls
/// through with no seed — observable equivalence to a cold-only run, per arch
/// §4.3 lines 539-540.
///
/// Used by `edit_source` step (4c) to drive the checkout half of the
/// WarmStatePool round-trip uniformly across the `added` /
/// `added_constraints` / `added_realizations` sets. The pool API itself
/// is variant-agnostic, so a future `NodeId` variant (e.g. `Resolution`
/// once a `diff_resolutions` exists, or `ComputeNode` once it becomes a
/// variant) drops in as a single additional call.
///
/// The `pending` guard owns the pool borrow, so callers do not need to pass
/// both a `&mut WarmStatePool` and a separate sink map — both are accessed
/// through the guard, which also ensures re-donation on early-return / panic.
fn checkout_added_warm_seeds<'a, I, T, F>(
    pending: &mut PendingWarmSeedsGuard<'_>,
    ids: I,
    wrap: F,
) where
    I: IntoIterator<Item = &'a T>,
    T: Clone + 'a,
    F: Fn(T) -> NodeId,
{
    for id in ids {
        let nid = wrap(id.clone());
        if let Some((state, last_accessed)) = pending.pool_mut().checkout_with_lru_stamp(&nid) {
            pending.insert(nid, state, last_accessed);
        }
    }
}

/// For each per-variant ID in `ids`, wrap it as a [`NodeId`] via `wrap`:
/// (1) donates any cached warm state to `pool` (when present), then
/// (2) invalidates the cache entry.
///
/// Used by `edit_source` step (9) for the `removed` / `removed_constraints` /
/// `removed_realizations` sets. A future `NodeId` variant slots in as a single
/// additional call.
fn donate_warm_state_and_invalidate<'a, I, T, F>(
    pool: &mut WarmStatePool,
    cache: &mut CacheStore,
    ids: I,
    wrap: F,
) where
    I: IntoIterator<Item = &'a T>,
    T: Clone + 'a,
    F: Fn(T) -> NodeId,
{
    for id in ids {
        let nid = wrap(id.clone());
        if let Some(state) = cache.get_warm_state(&nid) {
            pool.donate(nid.clone(), state);
        }
        cache.invalidate(&nid);
    }
}

impl Engine {
    /// Set a parameter override and invalidate cache entries that depend on it.
    pub fn set_param_and_invalidate(&mut self, param: &ValueCellId, value: reify_types::Value) {
        self.param_overrides.insert(param.clone(), value);
        // Mark the param's own cache entry as dirty
        let param_node = NodeId::Value(param.clone());
        self.cache.invalidate(&param_node);
        // Mark all nodes that depend on this param as dirty
        self.cache
            .invalidate_dependents(std::slice::from_ref(param));
    }

    /// Incrementally re-evaluate after changing a parameter value.
    ///
    /// Requires a prior call to eval() to establish the baseline snapshot
    /// and dependency structures. Creates a child snapshot with Edit provenance,
    /// computes dirty∩demand cone intersection, evaluates only Value nodes in
    /// the eval set (topologically sorted). Constraint/Realization nodes are
    /// tracked in the eval set but not evaluated (deferred to check()/build()).
    ///
    /// Returns EvalResult with all current values (both changed and unchanged).
    pub fn edit_param(
        &mut self,
        cell: ValueCellId,
        new_value: reify_types::Value,
    ) -> Result<EvalResult, EngineError> {
        // Arc::clone is O(1) — a refcount bump. The merged table (user functions +
        // prelude) was sealed into Arc<Vec<CompiledFunction>> by eval() or edit_source().
        // The local binding satisfies the borrow checker: evaluate_let_bindings and
        // other callers take &mut self, which would conflict with an immutable borrow
        // of self.functions. The Arc keeps the table alive for the binding's scope.
        let functions = Arc::clone(&self.functions);
        // Reset the per-edit guard-phase group evaluation counter before Phase 1.
        self.last_guard_phase_group_evals = 0;
        // Reset the test-instrumentation diff snapshot. The "most recent
        // edit_source call" invariant on `Engine::last_diff_value_cells()`
        // is enforced rather than documented — a subsequent edit_param
        // clears the field so callers cannot observe a stale diff (task 2265).
        // Gated to match the writer site in this same file (and to avoid
        // touching the production hot path).
        #[cfg(any(test, feature = "test-instrumentation"))]
        {
            self.last_diff_value_cells = None;
        }
        let state = self
            .eval_state
            .as_ref()
            .ok_or(EngineError::NotInitialized)?;

        // Single lookup: validate existence and retrieve the node in one traversal.
        // This eliminates the earlier double-lookup (contains_key + get().unwrap()).
        let cell_node = match state.snapshot.graph.value_cells.get(&cell) {
            Some(node) => node,
            None => return Err(EngineError::CellNotFound { cell }),
        };

        // Validate type-kind + Scalar-dimension compatibility via the shared
        // `validate_param_override` helper (see `engine_admin.rs`).  Kept in
        // one place so a future third guard (Tensor shape, List element-type)
        // lands once and is picked up by both the cold-start path in
        // `Engine::eval` and the incremental path here.  `Value::Undef` is
        // accepted as the Auto/no-value sentinel — `value_type_kind_matches`
        // inside the helper handles that.
        match validate_param_override(&new_value, &cell_node.cell_type) {
            Ok(()) => {}
            Err(ParamOverrideRejection::TypeKindMismatch) => {
                return Err(EngineError::TypeKindMismatch {
                    cell,
                    expected: Box::new(cell_node.cell_type.clone()),
                    got: Box::new(new_value),
                });
            }
            Err(ParamOverrideRejection::ScalarDimensionMismatch { expected, got }) => {
                return Err(EngineError::DimensionMismatch {
                    cell,
                    expected,
                    got,
                });
            }
        }

        // Clone snapshot and extract references (O(1) via PersistentMap)
        let parent_id = state.snapshot.id;
        let mut new_snapshot = state.snapshot.clone();

        // Compute dirty cone and eval set while state borrow is active
        let mut changed_set = std::collections::HashSet::new();
        changed_set.insert(cell.clone());
        let dirty_cone = crate::dirty::compute_dirty_cone(&changed_set, &state.reverse_index);
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &self.demand, &state.trace_map);

        // Seed has_changed_parent from dependents of the changed param
        let mut has_changed_parent: std::collections::HashSet<NodeId> =
            std::collections::HashSet::new();
        for dependent in state.reverse_index.dependents_of(&cell) {
            has_changed_parent.insert(dependent.clone());
        }
        // Release the immutable borrow of eval_state so we can mutate later
        let _ = state;

        // Update snapshot ID, version, and provenance
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;
        new_snapshot.id = SnapshotId(snapshot_id);
        new_snapshot.version = VersionId(version_id);

        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: changed_set.clone(),
            parent: parent_id,
        };

        // Update the changed cell's value in snapshot
        new_snapshot.values.insert(
            cell.clone(),
            (new_value.clone(), DeterminacyState::Determined),
        );

        // Update the param's cache entry to match the snapshot.
        // The param is a source node (not in dirty_cone / eval_set), so its
        // cache entry would otherwise retain the stale value from initial eval().
        self.cache.record_evaluation(
            NodeId::Value(cell.clone()),
            CachedResult::Value(new_value.clone(), DeterminacyState::Determined),
            VersionId(version_id),
            crate::deps::DependencyTrace::default(),
        );

        // Build the full ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in new_snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }
        // Overwrite with the new param value
        values.insert(cell.clone(), new_value);

        // Mark all nodes in the eval set as Pending before re-evaluation.
        // This transitions Final → Pending{last_substantive: hash}.
        self.cache.reset_pending_transition_count();
        for node_id in &eval_set {
            self.cache.mark_pending(node_id);
        }

        // Evaluate only Value nodes in the eval set (topo-sorted order).
        // Track nodes to skip due to early cutoff of upstream nodes.
        let mut skipped: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        let mut actual_eval_set: Vec<NodeId> = Vec::with_capacity(eval_set.len());

        for node_id in &eval_set {
            if skipped.contains(node_id) {
                continue;
            }
            actual_eval_set.push(node_id.clone());

            if let NodeId::Value(vcid) = node_id
                && let Some(node) = new_snapshot.graph.value_cells.get(vcid)
                && let Some(ref expr) = node.default_expr
            {
                let start = Instant::now();
                self.journal.record(EvalEvent {
                    timestamp: start,
                    node_id: node_id.clone(),
                    kind: EventKind::Started,
                    version: VersionId(version_id),
                    payload: None,
                });

                let val = reify_expr::eval_expr(
                    expr,
                    &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                );
                values.insert(vcid.clone(), val.clone());
                new_snapshot
                    .values
                    .insert(vcid.clone(), (val.clone(), DeterminacyState::Determined));

                // Record in cache and check for early cutoff
                let trace = extract_dependency_trace(expr);
                let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
                let outcome = self.cache.record_evaluation(
                    node_id.clone(),
                    cached_result,
                    VersionId(version_id),
                    trace,
                );

                self.journal.record(EvalEvent {
                    timestamp: Instant::now(),
                    node_id: node_id.clone(),
                    kind: EventKind::Completed { outcome },
                    version: VersionId(version_id),
                    payload: Some(EventPayload::Duration(start.elapsed())),
                });

                // Early cutoff with mixed fan-in protection:
                // - Changed: propagate has_changed_parent to dependents,
                //   remove them from skipped (in case an earlier Unchanged
                //   parent added them prematurely).
                // - Unchanged: only add dependents to skipped if they do NOT
                //   have a Changed parent (i.e., not in has_changed_parent).
                {
                    let dependents = self
                        .eval_state
                        .as_ref()
                        .unwrap()
                        .reverse_index
                        .dependents_of(vcid);
                    if outcome == EvalOutcome::Changed {
                        for dependent in dependents {
                            has_changed_parent.insert(dependent.clone());
                            skipped.remove(dependent);
                        }
                    } else {
                        // Unchanged
                        for dependent in dependents {
                            if !has_changed_parent.contains(dependent) {
                                skipped.insert(dependent.clone());
                            }
                        }
                    }
                }
            }
            // Constraint/Realization nodes: tracked in eval set but not evaluated
            // (deferred to check()/build())
        }

        // Restore freshness to Final for nodes that were pre-marked Pending
        // but then skipped by early cutoff (they were never re-evaluated).
        for node_id in &skipped {
            self.cache.restore_final(node_id);
        }

        // ── Composed-field re-elaboration (task 2343 step-8) ───────────
        // Composed fields can capture other field cells via the augmented
        // `Lambda { captures, .. }` injected by the compiler post-pass
        // `phase_augment_composed_captures`. Those captures are sealed at
        // initial elaboration time, so a downstream change in the values
        // map (e.g. an upstream cell modified by this edit) would otherwise
        // leave the cached `Value::Lambda` with stale captures — breaking
        // the cache invariant. Iterate the snapshot of compiled fields and,
        // for any composed field whose NodeId is in the dirty cone, rebuild
        // its `Value::Field` against the post-eval-loop `values` map. The
        // rebuilt lambda has fresh captures sourced from the current values,
        // restoring the invariant. Mirrors the cold-start path in
        // `engine_eval.rs::Engine::eval` via the shared `elaborate_field`
        // helper. Sampled / Imported fields are skipped: their source has
        // no callable lambda and therefore no captures to refresh.
        //
        // Precision gate (task 2343 step-10): the `dirty_cone.contains`
        // check below is the precision contract — a composed field is
        // re-elaborated ONLY when one of its registered deps (captures from
        // step-4) appears in `changed_set`, since `compute_dirty_cone`
        // never adds a node whose deps did not change. Pinned by
        // `eval_composed_field_invalidates_only_when_dep_changes` in
        // `crates/reify-eval/tests/field_eval_tests.rs` (step-9), which
        // edits a param NOT captured by any field and verifies via
        // `Arc::ptr_eq` that no field's lambda Arc is rebuilt. Removing
        // or weakening this gate (e.g. always re-elaborating every field)
        // would re-elaborate fields whose captures haven't changed,
        // wasting work and breaking the step-9 test.
        let compiled_fields = Arc::clone(&self.compiled_fields);
        for field in compiled_fields.iter() {
            let reify_compiler::CompiledFieldSource::Composed { expr } = &field.source else {
                continue;
            };
            let field_cell = ValueCellId::new(reify_types::FIELD_ENTITY_PREFIX, &field.name);
            let field_node = NodeId::Value(field_cell.clone());
            if !dirty_cone.contains(&field_node) {
                continue;
            }
            let new_field_value =
                crate::engine_eval::elaborate_field(field, &values, &functions, &self.meta_map);
            values.insert(field_cell.clone(), new_field_value.clone());
            new_snapshot.values.insert(
                field_cell,
                (new_field_value.clone(), DeterminacyState::Determined),
            );
            // Refresh the cache entry so a subsequent demand-driven fetch
            // picks up the rebuilt lambda rather than the stale one. We
            // record_evaluation rather than mark_pending — the field is
            // freshly computed at this point and downstream consumers can
            // treat it as Final.
            //
            // Record the static dependency trace (matching the cold-start
            // contract) rather than `DependencyTrace::default()`. Without
            // this, `CacheStore::invalidate_dependents` (cache.rs) would
            // see an empty `reads` set on the rebuilt entry and silently
            // skip propagation when one of the field's actual deps later
            // changes — leaving the cache invariant 'entries carry the
            // static trace of their reads' broken on this code path. The
            // reverse-index drives invalidation via `compute_dirty_cone`
            // today, but the cache trace is the durable per-entry record
            // and must stay consistent with the cold-start path.
            self.cache.record_evaluation(
                field_node,
                CachedResult::Value(new_field_value, DeterminacyState::Determined),
                VersionId(version_id),
                extract_dependency_trace(expr),
            );
        }

        // ── Guard re-elaboration phase ────────────────────────────────
        // If any structure_controlling cell changed, re-evaluate guarded groups
        // to flip which branch is active/inactive, and recompute fingerprint.
        //
        // Cross-phase dedup (task 2140, 2146): the map is non-empty only when
        // Phase 1 fires; the else arm returns an empty HashMap so no allocation
        // is wasted when no guards are dirty. Phase 3 consults the map to skip
        // groups already re-elaborated here when the guard value is unchanged
        // since Phase 1 — but falls through to full re-elaboration if wave2 has
        // flipped the guard value after Phase 1 recorded it (task 2146 fix).
        let phase1_reelaborated: HashMap<ValueCellId, Value> = {
            let graph = &new_snapshot.graph;
            let has_dirty_guards = graph.structure_controlling.iter().any(|sc_id| {
                dirty_cone.contains(&NodeId::Value(sc_id.clone())) || changed_set.contains(sc_id)
            });

            if has_dirty_guards {
                let mut set = HashMap::new();
                for group in &graph.guarded_groups {
                    // Re-evaluate the guard cell's expression
                    let guard_val = if let Some(node) = graph.value_cells.get(&group.guard_cell) {
                        if let Some(ref expr) = node.default_expr {
                            reify_expr::eval_expr(
                                expr,
                                &eval_ctx_with_meta(&values, &functions, &self.meta_map)
                                    .with_determinacy(&new_snapshot.values),
                            )
                        } else {
                            Value::Undef
                        }
                    } else {
                        Value::Undef
                    };
                    // Per-group skip: if this group's guard value is unchanged vs.
                    // the pre-edit snapshot, its activation state has not flipped
                    // and its members don't need re-elaboration. edit_param has no
                    // structural-add or role-flip trigger, so the skip condition is
                    // purely "guard value unchanged".
                    // Always write the guard cell value before the skip check.
                    // Phase 1 re-evaluates guards with a determinacy context that
                    // the main eval loop lacks; DeterminacyPredicate guards (e.g.
                    // `determined(x)`) evaluate to Undef in the main loop and must
                    // be corrected here — even when we skip member re-elaboration.
                    let guard_det = if matches!(&guard_val, Value::Bool(_)) {
                        DeterminacyState::Determined
                    } else {
                        DeterminacyState::Undetermined
                    };
                    values.insert(group.guard_cell.clone(), guard_val.clone());
                    new_snapshot
                        .values
                        .insert(group.guard_cell.clone(), (guard_val.clone(), guard_det));
                    if guard_value_unchanged(
                        self.eval_state.as_ref().map(|s| &s.snapshot.values),
                        &group.guard_cell,
                        &guard_val,
                    ) {
                        continue;
                    }
                    // Skipped here ⇒ no entry in `set` (a.k.a. `phase1_reelaborated`),
                    // so Phase 3 reaches case (c): the standard `old_guard_val != current`
                    // check still catches any later wave2 flip that reverts the guard.
                    self.last_guard_phase_group_evals += 1;
                    reelaborate_guarded_group(
                        graph,
                        group,
                        &guard_val,
                        &mut values,
                        &mut new_snapshot.values,
                        &functions,
                        &self.meta_map,
                    );
                    // Record guard_cell → guard_val so Phase 3 can detect a
                    // wave2 flip: if the current guard value differs from the
                    // recorded value, Phase 3 falls through to full re-elaboration
                    // (task 2146 fix). The `.insert` sits after the skip-continue
                    // so only actually-processed groups land in the map.
                    // guard_val is moved here (not cloned) — reelaborate_guarded_group
                    // only borrows it by reference.
                    set.insert(group.guard_cell.clone(), guard_val);
                }

                // Recompute topology fingerprint including guard states.
                let guard_state_hash =
                    guard_state_fingerprint(&graph.guarded_groups, &values, GuardLookup::Lenient);
                new_snapshot.topology_fingerprint =
                    graph.topology_fingerprint().combine(guard_state_hash);
                set
            } else {
                HashMap::new()
            }
        };

        // ── Resolution phase ───────────────────────────────────────────
        // If a solver is present, check whether any constraints governing
        // auto params are in the dirty cone. If so, re-run the solver
        // to update auto param values and propagate to dependents.
        let mut resolved_params = HashMap::new();
        let mut diagnostics = Vec::new();

        if let Some(ref solver) = self.solver {
            // Group auto params by entity (template) name
            let mut entity_groups: HashMap<String, (Vec<AutoParam>, HashSet<ValueCellId>)> =
                HashMap::new();

            for (_, node) in new_snapshot.graph.value_cells.iter() {
                if node.kind.is_auto() {
                    let entry = entity_groups
                        .entry(node.id.entity.clone())
                        .or_insert_with(|| (Vec::new(), HashSet::new()));
                    entry.0.push(AutoParam {
                        id: node.id.clone(),
                        param_type: node.cell_type.clone(),
                        bounds: None,
                        free: node.kind.is_auto_free(),
                    });
                    entry.1.insert(node.id.clone());
                }
            }

            // Union of all resolved auto param IDs across groups for second wave
            let mut all_resolved_ids: HashSet<ValueCellId> = HashSet::new();

            // Snapshot current values BEFORE the loop so each group's solver
            // receives the same baseline — preventing cross-group contamination
            // where one group's resolved values leak into another group's input.
            let snapshot_values = values.clone();

            // Solve each entity group independently
            for (scope_name, (auto_param_list, auto_ids)) in &entity_groups {
                // Find constraints referencing this group's auto params
                let filtered_constraints: Vec<_> = new_snapshot
                    .graph
                    .constraints
                    .iter()
                    .filter(|(_, cnode)| {
                        let trace = extract_dependency_trace(&cnode.expr);
                        trace.reads.iter().any(|r| auto_ids.contains(r))
                    })
                    .map(|(_, cnode)| (cnode.id.clone(), cnode.expr.clone()))
                    .collect();

                // Check if any of those constraints are in the dirty cone
                let constraints_dirty = filtered_constraints
                    .iter()
                    .any(|(cid, _)| dirty_cone.contains(&NodeId::Constraint(cid.clone())));

                if !constraints_dirty {
                    continue;
                }

                // Look up the template-native objective by entity name.
                let objective = self.objectives.get(scope_name).cloned();

                // Build ResolutionProblem and solve
                let problem = ResolutionProblem {
                    auto_params: auto_param_list.clone(),
                    constraints: filtered_constraints,
                    current_values: snapshot_values.clone(),
                    objective,
                    // Arc::clone is O(1) — a refcount bump into the merged table
                    // already held by Engine.functions (tasks #1997, #2286).
                    functions: Arc::clone(&functions),
                };

                match solver.solve(&problem) {
                    SolveResult::Solved {
                        values: solver_values,
                        unique,
                    } => {
                        for (id, val) in &solver_values {
                            values.insert(id.clone(), val.clone());
                            resolved_params.insert(id.clone(), val.clone());
                            all_resolved_ids.insert(id.clone());

                            // Update snapshot values
                            new_snapshot
                                .values
                                .insert(id.clone(), (val.clone(), DeterminacyState::Determined));

                            // Update param_overrides so subsequent edits
                            // use the resolved value
                            self.param_overrides.insert(id.clone(), val.clone());

                            // Update cache
                            let node_id = NodeId::Value(id.clone());
                            let trace = DependencyTrace::default();
                            let cached_result =
                                CachedResult::Value(val.clone(), DeterminacyState::Determined);
                            self.cache.record_evaluation(
                                node_id,
                                cached_result,
                                VersionId(version_id),
                                trace,
                            );
                        }
                        if !unique {
                            for ap in auto_param_list {
                                if ap.free {
                                    diagnostics.push(Diagnostic::warning(format!(
                                        "Parameter `{}` resolved via auto(free) \
                                         -- result is not uniquely determined.",
                                        ap.id.member
                                    )));
                                }
                            }
                        }
                    }
                    SolveResult::Infeasible {
                        diagnostics: solver_diags,
                    } => {
                        diagnostics.extend(solver_diags);
                    }
                    SolveResult::NoProgress { reason } => {
                        diagnostics.push(Diagnostic::warning(format!(
                            "Constraint solver made no progress: {}",
                            reason
                        )));
                    }
                }
            }

            // ── Second propagation wave (once, with union of all resolved IDs) ──
            // Re-resolved auto params may have changed value. Let bindings
            // depending on them may NOT be in the original dirty cone.
            // Guard: skip if eval_state is None (defensive; the early guard at
            // edit_param entry ensures this is unreachable, but an if-let is
            // consistent with the guard re-elaboration phase below which uses
            // .and_then for the same field).
            if !all_resolved_ids.is_empty()
                && let Some(es) = self.eval_state.as_ref()
            {
                let wave2_dirty =
                    crate::dirty::compute_dirty_cone(&all_resolved_ids, &es.reverse_index);
                let wave2_eval =
                    crate::dirty::compute_eval_set(&wave2_dirty, &self.demand, &es.trace_map);

                for node_id in &wave2_eval {
                    if let NodeId::Value(vcid) = node_id
                        && let Some(node) = new_snapshot.graph.value_cells.get(vcid)
                        && let Some(ref expr) = node.default_expr
                    {
                        let val = reify_expr::eval_expr(
                            expr,
                            &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                        );
                        values.insert(vcid.clone(), val.clone());
                        new_snapshot
                            .values
                            .insert(vcid.clone(), (val.clone(), DeterminacyState::Determined));

                        // Update cache for re-evaluated node
                        let trace = extract_dependency_trace(expr);
                        let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
                        self.cache.record_evaluation(
                            node_id.clone(),
                            cached_result,
                            VersionId(version_id),
                            trace,
                        );
                    }
                }

                // Post-wave2 cleanup (tasks 2140, 2144): wave2 can re-evaluate
                // inactive-branch members of ANY guarded group — including groups
                // Phase 1 skipped via the per-group unchanged-guard short-circuit
                // (task 2144) and groups entirely outside the dirty-guard trigger.
                // Re-deactivate all guarded groups (idempotent for groups wave2
                // did not touch).  See `reapply_guard_deactivations_post_wave2`.
                reapply_guard_deactivations_post_wave2(
                    &new_snapshot.graph,
                    &mut values,
                    &mut new_snapshot.values,
                );
            }
        }

        // ── Guard re-elaboration phase ──────────────────────────────────
        // If any structure-controlling (guard) cells changed boolean value,
        // re-evaluate affected guarded group members: activate the correct
        // branch (members or else_members) and deactivate the other.
        // Finally, recompute topology fingerprint to reflect guard state.
        //
        // `guard_changed` is also true when Phase 1 processed a group with a
        // guard value that wave2 subsequently changed (flip-then-revert); in
        // that case `group_needs_phase3` detects the inconsistency regardless
        // of whether the final guard value matches the pre-edit snapshot
        // (task 2146).
        {
            let guard_changed = new_snapshot.graph.guarded_groups.iter().any(|group| {
                let old_guard_val = old_guard_for(self.eval_state.as_ref(), &group.guard_cell);
                group_needs_phase3(group, &values, old_guard_val, &phase1_reelaborated)
            });

            if guard_changed {
                // Field-level borrow splitting: pre-bind `graph` so the loop can
                // iterate `&graph.guarded_groups` (shared) while `&mut new_snapshot.values`
                // (a disjoint field) remains exclusively borrowed inside the body.
                // This matches the Phase 1 pattern at lines 647-708; no .clone() needed.
                let graph = &new_snapshot.graph;
                // Re-evaluate each guarded group based on current guard values
                for group in &graph.guarded_groups {
                    // Cross-phase dedup — see `group_needs_phase3` (tasks 2140 / 2146).
                    // The unified predicate also handles the wave2 flip case where
                    // Phase 1 recorded a different guard value than the current one.
                    let old_guard_val = old_guard_for(self.eval_state.as_ref(), &group.guard_cell);
                    if !group_needs_phase3(group, &values, old_guard_val, &phase1_reelaborated) {
                        continue;
                    }
                    // Absent-guard skip rationale and warn-before-assert invariant: see `phase3_get_guard_val` docs.
                    let Some(guard_val) = phase3_get_guard_val(&values, &group.guard_cell) else {
                        continue;
                    };
                    self.last_guard_phase_group_evals += 1;
                    reelaborate_guarded_group(
                        graph,
                        group,
                        &guard_val,
                        &mut values,
                        &mut new_snapshot.values,
                        &functions,
                        &self.meta_map,
                    );
                }

                // Recompute topology fingerprint to include guard states.
                let guard_state_hash = guard_state_fingerprint(
                    &new_snapshot.graph.guarded_groups,
                    &values,
                    GuardLookup::Strict,
                );
                new_snapshot.topology_fingerprint = new_snapshot
                    .graph
                    .topology_fingerprint()
                    .combine(guard_state_hash);
            }
        }

        // ── Collection count re-elaboration phase ─────────────────────
        // If any structure_controlling cell is a collection count cell and
        // its value changed, add/remove instances to match the new count.
        {
            let collection_subs = new_snapshot.graph.collection_subs.clone();
            for col_sub in &collection_subs {
                let new_count_val = values
                    .get(&col_sub.count_cell)
                    .cloned()
                    .unwrap_or(Value::Undef);
                let old_count_val = self
                    .eval_state
                    .as_ref()
                    .and_then(|s| s.snapshot.values.get(&col_sub.count_cell))
                    .map(|(v, _)| v.clone())
                    .unwrap_or(Value::Undef);

                if new_count_val == old_count_val {
                    continue;
                }

                // Helper closure: resolve a collection count value to an integer.
                // Returns (count, optional warning diagnostic).
                // Value::Undef is treated as 0 without warning — it represents an undetermined
                // count for which no instances were created. Any other non-integer type emits a
                // warning (potential upstream type bug) and also returns 0.
                let resolve_count = |val: &Value, label: &str| -> (i64, Option<Diagnostic>) {
                    match val {
                        Value::Int(n) => (*n, None),
                        Value::Undef => (0, None),
                        other => (
                            0,
                            Some(Diagnostic::warning(format!(
                                "Collection count cell {} has non-integer {} value {:?}; treating as 0",
                                col_sub.count_cell, label, other
                            ))),
                        ),
                    }
                };

                // Remove old instances from graph and snapshot
                let (old_count, old_warn) = resolve_count(&old_count_val, "old");
                if let Some(w) = old_warn {
                    diagnostics.push(w);
                }
                for i in 0..old_count {
                    let scoped_entity =
                        format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, i);
                    for (member, _, _, _) in &col_sub.child_value_cells {
                        let scoped_id = ValueCellId::new(&scoped_entity, member);
                        new_snapshot.graph.value_cells.remove(&scoped_id);
                        new_snapshot.values.remove(&scoped_id);
                        values.remove(&scoped_id);
                        // Task 2184 (mirrors task 2086 Fix 1 in edit_source): invalidate cache
                        // so a subsequent edit_param that re-grows this collection sub at the
                        // same scoped index evaluates freshly instead of returning a stale
                        // CachedResult from a prior incarnation.
                        self.cache.invalidate(&NodeId::Value(scoped_id));
                    }
                }

                // Create new instances based on new count
                let (new_count, new_warn) = resolve_count(&new_count_val, "new");
                if let Some(w) = new_warn {
                    diagnostics.push(w);
                }
                for i in 0..new_count {
                    let scoped_entity =
                        format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, i);
                    for (member, kind, cell_type, default_expr) in &col_sub.child_value_cells {
                        let scoped_id = ValueCellId::new(&scoped_entity, member);
                        let id_hash = ContentHash::of_str(&format!("{}", scoped_id));
                        let expr_hash = default_expr
                            .as_ref()
                            .map(|e| e.content_hash)
                            .unwrap_or(ContentHash(0));
                        let node = crate::graph::ValueCellNode {
                            id: scoped_id.clone(),
                            kind: *kind,
                            cell_type: cell_type.clone(),
                            default_expr: default_expr.clone(),
                            content_hash: id_hash.combine(expr_hash),
                        };
                        new_snapshot
                            .graph
                            .value_cells
                            .insert(scoped_id.clone(), node);

                        // Evaluate the cell
                        let val = if let Some(expr) = default_expr {
                            reify_expr::eval_expr(
                                expr,
                                &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                            )
                        } else {
                            Value::Undef
                        };
                        values.insert(scoped_id.clone(), val.clone());
                        new_snapshot
                            .values
                            .insert(scoped_id, (val, DeterminacyState::Determined));
                    }
                }

                // Update per-member synthetic lists: __list_{name}__{member}
                for (member, _, _, _) in &col_sub.child_value_cells {
                    let member_items: Vec<Value> = (0..new_count)
                        .map(|idx| {
                            let scoped_id = ValueCellId::new(
                                format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, idx),
                                member,
                            );
                            values.get(&scoped_id).cloned().unwrap_or(Value::Undef)
                        })
                        .collect();
                    let member_list_id = ValueCellId::new(
                        &col_sub.parent_entity,
                        format!("__list_{}__{}", col_sub.sub_name, member),
                    );
                    let member_list_val = Value::List(member_items);
                    values.insert(member_list_id.clone(), member_list_val.clone());
                    new_snapshot.values.insert(
                        member_list_id,
                        (member_list_val, DeterminacyState::Determined),
                    );
                }

                // Recompute topology fingerprint to reflect count change
                let count_state_hash = ContentHash::of_str(&format!(
                    "collection:{}={}",
                    col_sub.count_cell, new_count
                ));
                new_snapshot.topology_fingerprint = new_snapshot
                    .graph
                    .topology_fingerprint()
                    .combine(count_state_hash);
            }
        }

        // Store state (actual_eval_set excludes early-cutoff-skipped nodes)
        self.last_eval_set = actual_eval_set;
        self.eval_state.as_mut().unwrap().snapshot = new_snapshot;

        Ok(EvalResult {
            values,
            diagnostics,
            resolved_params,
        })
    }

    /// Incrementally re-evaluate after a structural source edit.
    ///
    /// Mirrors `edit_param`'s `NotInitialized` precondition: requires a prior
    /// `eval()` to establish the baseline snapshot, reverse index, trace map,
    /// and demand registry. Returns `Err(EngineError::NotInitialized)` when
    /// called on a fresh Engine before any eval.
    ///
    /// Algorithm (step-6 — diff-driven incremental eval):
    /// 1. Build a fresh `Snapshot`, `ReverseDependencyIndex`, trace map, and
    ///    `DemandRegistry` from the new module.
    /// 2. Diff the old and new `EvaluationGraph`s at value-cell granularity
    ///    via `diff_value_cells` → `(changed, added, removed)`.
    /// 3. Compute `dirty_cone` via `compute_dirty_cone` over
    ///    `changed ∪ added`, augment with the changed/added cells themselves
    ///    (so their own `default_expr` re-evaluates) and with dependents of
    ///    removed cells via the OLD reverse_index (defensively, gated on
    ///    presence in the new graph).
    /// 4. `eval_set = compute_eval_set(dirty_cone, new_demand, new_trace_map)`.
    /// 5. Seed the working `values` map and `new_snapshot.values`: for every
    ///    cell present in both graphs with unchanged `content_hash`, copy the
    ///    prior `(Value, DeterminacyState)`; for changed/added cells keep the
    ///    `Snapshot::from_compiled_module` default (Undef) — the eval loop
    ///    below fills these in.
    /// 6. Invalidate cache entries for removed and changed value cells.
    /// 7. Refresh `self.functions` / `self.compiled_purposes` / `self.meta_map`
    ///    / `self.objectives` from the new module (module-level state a pure
    ///    cell diff cannot detect).
    /// 8. Per-cell eval loop (shape mirrors `edit_param`): iterate the
    ///    topologically sorted `eval_set`, evaluate each Value node's
    ///    `default_expr`, record a cache entry, and propagate the
    ///    Changed/Unchanged outcome via `has_changed_parent` / `skipped` so
    ///    unchanged sub-cones short-circuit.
    /// 9. Install the new snapshot (with `Edit { changed, parent }`
    ///    provenance), `reverse_index`, `trace_map`, and `demand` into
    ///    `self`; stash `actual_eval_set` in `self.last_eval_set`.
    ///
    /// Constraint / realization diffing and the solver / guard / collection
    /// re-elaboration phases are deferred to later steps (see `.task/plan.json`
    /// steps 10 and 14).
    pub fn edit_source(&mut self, module: &CompiledModule) -> Result<EvalResult, EngineError> {
        // Precondition: prior eval() must have populated eval_state. This is
        // the same precondition as edit_param and is validated first so that
        // all later steps can rely on a present baseline.
        if self.eval_state.is_none() {
            return Err(EngineError::NotInitialized);
        }
        // Disjoint-field borrow: Rust's NLL tracks this borrow as touching only
        // the `eval_state` field (not all of `self`), so later mutable borrows
        // of sibling fields — `self.param_overrides.retain(...)` and
        // `self.cache.invalidate(...)` — coexist without a lifetime conflict.
        // `eval_state` is used read-only throughout: parent_id, old graph,
        // reverse_index, and trace_map.
        let eval_state = self.eval_state.as_ref().unwrap();

        // (1) Capture the parent snapshot id before we mutate any state.
        let parent_id = eval_state.snapshot.id;

        // (2) Build the new snapshot from the incoming CompiledModule.
        //     Snapshot::from_compiled_module seeds every value cell to
        //     (Undef, Undetermined) or (Undef, Auto); the seeding loop
        //     below overwrites those with the preserved prior values for
        //     cells whose content_hash matches the old graph.
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;
        let mut new_snapshot = crate::snapshot::Snapshot::from_compiled_module(module);
        // Invariant mirror of engine_eval.rs:248-249 — covers the edit-time recompile path.
        #[cfg(debug_assertions)]
        crate::engine_eval::assert_value_cell_types_representable(&new_snapshot.graph);
        new_snapshot.id = SnapshotId(snapshot_id);
        new_snapshot.version = VersionId(version_id);

        // (3) Rebuild dependency structures against the NEW graph plus the
        //     module's composed fields. Full rebuild is O(nodes · avg_trace_size),
        //     matching cold eval(); see the design-decision rationale in
        //     plan.json for why we don't patch in place. Composed-field deps
        //     are surfaced through the augmented `Lambda { captures, .. }`
        //     injected by the compiler's `phase_augment_composed_captures`
        //     post-pass — see `deps.rs::build_from_graph_and_fields`.
        let new_reverse_index = crate::deps::ReverseDependencyIndex::build_from_graph_and_fields(
            &new_snapshot.graph,
            &module.fields,
        );
        let new_trace_map =
            crate::deps::build_trace_map_and_fields(&new_snapshot.graph, &module.fields);

        // Shared demand-seeding helper with Engine::eval — see
        // `build_demand_for_graph` for the per-kind initialization.
        let new_demand = crate::engine_eval::build_demand_for_graph(&new_snapshot.graph);

        // (4) Diff the old and new graphs at value-cell granularity.
        let (changed, added, removed) =
            diff_value_cells(&eval_state.snapshot.graph, &new_snapshot.graph);
        // (4a) Snapshot the diff for test-instrumentation — premise lock for T3 et al.
        // Disjoint-field borrow: `eval_state` is borrowed read-only above; this
        // assignment touches only the sibling field `last_diff_value_cells` — NLL
        // allows it (see the borrow safety note at the top of this function).
        // Gated to avoid the three HashSet clones in production builds.
        #[cfg(any(test, feature = "test-instrumentation"))]
        {
            self.last_diff_value_cells = Some((changed.clone(), added.clone(), removed.clone()));
        }
        let changed_set: HashSet<ValueCellId> =
            changed.iter().chain(added.iter()).cloned().collect();

        // (4b) Diff constraints and realizations (step-10). These nodes are
        //      positional (`entity, index`) and have their own content_hash on
        //      `ConstraintNodeData` / `RealizationNodeData`. A re-ordered
        //      declaration surfaces as `changed` at the shifted index, not
        //      add+remove. We don't evaluate constraint/realization expressions
        //      at edit_source time — they are deferred to check() / build() —
        //      but we DO want them to appear in `last_eval_set()` when changed
        //      or added, so callers can observe the diff, and we want their
        //      stale cache entries invalidated when removed.
        let (changed_constraints, added_constraints, removed_constraints) =
            diff_constraints(&eval_state.snapshot.graph, &new_snapshot.graph);
        let (changed_realizations, added_realizations, removed_realizations) =
            diff_realizations(&eval_state.snapshot.graph, &new_snapshot.graph);

        // (4c) Checkout warm state from the pool for every node entering the
        //      topology in this edit, keyed by `NodeId`. Per arch §4.3 lines
        //      539-540 and §6.4 lines 654-660: a re-appearing node gets its
        //      previously-donated warm state if the pool still holds it; a
        //      `None` checkout means the entry was LRU-evicted and evaluation
        //      falls through the cold path with no seeded warm state.
        //
        //      We collect into a local `pending_warm_seeds` map (transient,
        //      scoped to this edit_source call) and drain it into the cache
        //      AFTER all post-eval phases complete (see step (14b) below).
        //      The drain MUST come after phases 1-4 because
        //      `cache.record_evaluation` clears `warm_state` on every call —
        //      seeding earlier would be wiped by any guard/solver
        //      re-elaboration that re-evaluates the seeded node.
        //
        //      Symmetric across all three NodeId variants currently produced
        //      by `diff_*` helpers — Value, Constraint, Realization — via
        //      the shared `checkout_added_warm_seeds` helper. Resolution is
        //      not yet in any `diff_*` helper (see TODO(resolution-diff)
        //      above); ComputeNode is not yet a NodeId variant. The pool API
        //      itself is variant-agnostic, so any future variant slots in
        //      as a single additional call to the helper.
        let mut pending_warm_seeds = PendingWarmSeedsGuard::new(&mut self.warm_pool);
        checkout_added_warm_seeds(&mut pending_warm_seeds, &added, NodeId::Value);
        checkout_added_warm_seeds(
            &mut pending_warm_seeds,
            &added_constraints,
            NodeId::Constraint,
        );
        checkout_added_warm_seeds(
            &mut pending_warm_seeds,
            &added_realizations,
            NodeId::Realization,
        );

        // (5) Compute the dirty cone over changed ∪ added using the NEW
        //     reverse index (which reflects post-edit dependencies). The
        //     compute_dirty_cone helper excludes the roots themselves, so
        //     we also splice in NodeId::Value for each changed/added cell
        //     — their own default_expr must be re-evaluated.
        let mut dirty_cone = crate::dirty::compute_dirty_cone(&changed_set, &new_reverse_index);
        for id in &changed_set {
            dirty_cone.insert(NodeId::Value(id.clone()));
        }

        // (6) Defensively include dependents of REMOVED cells via the OLD
        //     reverse index, gated on presence in the new graph. A removed
        //     cell typically also forces its dependents to be classified as
        //     `changed` (their expressions lost a ValueRef), but the OLD
        //     reverse index is the authoritative source for "what used to
        //     read this cell"; skipping it would miss dependents whose
        //     expressions happen to remain shape-compatible (e.g., a
        //     fallback branch).
        //
        //     Resolution nodes are currently treated as not-still-present:
        //     they are live in the graph (`deps.rs`, `cache.rs`), but the
        //     eval() / edit_source() demand-seeding path does not
        //     `add_demand` them, and edit_source has no `diff_resolutions`
        //     helper yet. The moment Resolution demand is added, this arm
        //     becomes a latent staleness hazard — a Resolution dependent of
        //     a removed cell would silently retain a stale cached value.
        //
        //     TODO(resolution-diff): add a `diff_resolutions` helper and
        //     replace this `false` with a
        //     `new_snapshot.graph.resolutions.contains_key(rid)` presence
        //     check, symmetric with the other arms, once Resolution nodes
        //     participate in the demand set.
        {
            let old_reverse_index = &eval_state.reverse_index;
            for id in &removed {
                for dep in old_reverse_index.dependents_of(id) {
                    let still_present = match dep {
                        NodeId::Value(vcid) => new_snapshot.graph.value_cells.contains_key(vcid),
                        NodeId::Constraint(cid) => new_snapshot.graph.constraints.contains_key(cid),
                        NodeId::Realization(rid) => {
                            new_snapshot.graph.realizations.contains_key(rid)
                        }
                        NodeId::Resolution(_) => false, // TODO(resolution-diff)
                    };
                    if still_present {
                        dirty_cone.insert(dep.clone());
                    }
                }
            }
        }

        // (6b) Insert Constraint / Realization nodes for changed + added
        //      entries into dirty_cone so they appear in last_eval_set. Every
        //      constraint and realization is demanded by eval() / edit_source()
        //      (see the `new_demand` rebuild above), so any entry we splice in
        //      here survives the demand ∩ dirty intersection in compute_eval_set.
        //
        //      Constraint/realization nodes are tracked but NOT evaluated
        //      eagerly here — the expressions are deferred to check() / build()
        //      via `check_constraints_with_values`, which reads the installed
        //      snapshot and the up-to-date graph. This preserves edit_param's
        //      contract (its eval loop also skips Constraint/Realization nodes).
        for cid in &changed_constraints {
            dirty_cone.insert(NodeId::Constraint(cid.clone()));
        }
        for cid in &added_constraints {
            dirty_cone.insert(NodeId::Constraint(cid.clone()));
        }
        for rid in &changed_realizations {
            dirty_cone.insert(NodeId::Realization(rid.clone()));
        }
        for rid in &added_realizations {
            dirty_cone.insert(NodeId::Realization(rid.clone()));
        }

        // (7) Compute eval_set (topo-sorted) from dirty ∩ demand.
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &new_demand, &new_trace_map);

        // (8) Seed values by preserving unchanged-content_hash entries from
        //     the old snapshot, with `param_overrides` winning for Param cells
        //     (step-12). Changed cells retain their
        //     Snapshot::from_compiled_module default (Undef) so the eval loop
        //     fills them in; added cells are seeded from overrides (if any,
        //     for Param kind) else left Undef for the eval loop. Removed cells
        //     are simply absent from the new graph, and their override entries
        //     are purged from `self.param_overrides` below.
        let mut values = ValueMap::new();
        // Shortcut references into the prior snapshot for the seeding loop below.
        let old_graph_snapshot_values = &eval_state.snapshot.values;
        let old_graph_cells = &eval_state.snapshot.graph.value_cells;
        for (id, new_node) in new_snapshot.graph.value_cells.iter() {
            // `param_overrides` wins for Param cells whose content_hash is
            // unchanged across the edit. This mirrors eval_cached's precedence
            // rule ("override always wins for Param cells") and ensures an
            // override established before a structural edit survives the edit.
            // For Param cells whose content_hash CHANGED (e.g. the source
            // default was edited), we intentionally skip the override — the
            // diff has classified the cell as dirty and the eval loop will
            // re-derive it from the new default_expr. If the user wants the
            // override to persist across a content-hash-shifting edit, they
            // can re-install it via set_param_and_invalidate after edit_source.
            let unchanged_hash = old_graph_cells
                .get(id)
                .map(|old_node| old_node.content_hash == new_node.content_hash)
                .unwrap_or(false);

            if matches!(new_node.kind, reify_compiler::ValueCellKind::Param)
                && unchanged_hash
                && let Some(override_val) = self.param_overrides.get(id)
            {
                new_snapshot.values.insert(
                    id.clone(),
                    (override_val.clone(), DeterminacyState::Determined),
                );
                values.insert(id.clone(), override_val.clone());
                continue;
            }

            if unchanged_hash && let Some((val, det)) = old_graph_snapshot_values.get(id) {
                new_snapshot.values.insert(id.clone(), (val.clone(), *det));
                values.insert(id.clone(), val.clone());
                continue;
            }
            // Changed/added/no prior entry: read the Undef seed placed by
            // Snapshot::from_compiled_module so the working values map has
            // an entry for every present cell (downstream expressions can
            // fail-stop on missing reads).
            if let Some((val, _)) = new_snapshot.values.get(id) {
                values.insert(id.clone(), val.clone());
            }
        }

        // (8b) Purge param_overrides entries for cells that no longer exist
        //      in the new graph (step-12). A dormant override on a removed
        //      cell has nothing to apply to and, if left in place, would
        //      zombie-resurrect if a future edit re-adds a cell with the same
        //      ValueCellId. We also drop overrides for cells that still exist
        //      but are no longer Param (kind changed from Param to Let or
        //      Auto) — the override is only meaningful for Param cells.
        self.param_overrides.retain(|id, _| {
            new_snapshot
                .graph
                .value_cells
                .get(id)
                .map(|node| matches!(node.kind, reify_compiler::ValueCellKind::Param))
                .unwrap_or(false)
        });

        // (9) Invalidate cache entries for changed and removed cells, plus
        //     changed/removed constraints and realizations (step-10). Added
        //     entries have no prior cache entry, so we skip them — the per-cell
        //     eval loop (for value cells) and the downstream check()/build()
        //     path (for constraints/realizations) will populate fresh entries.
        //     Dependents of value-cell changes are refreshed (or transitioned
        //     through Pending) by the per-cell eval loop below.
        //
        // For REMOVED nodes, donate any per-node warm state to the engine's
        // `WarmStatePool` BEFORE invalidating. Per arch §4.3 lines 539-540
        // and §6.4 lines 654-660: a node leaving the topology hands its warm
        // state to the pool keyed by `NodeId`; LRU eviction inside the pool
        // is the only memory bound (no engine-level eviction logic). Symmetric
        // across all three NodeId variants currently produced by `diff_*`
        // helpers — Value, Constraint, Realization — via the shared
        // `donate_warm_state_and_invalidate` helper. Resolution is not yet in any
        // `diff_*` helper (see TODO(resolution-diff) above), so it does not
        // donate today; ComputeNode is not yet a NodeId variant.
        for id in &changed {
            self.cache.invalidate(&NodeId::Value(id.clone()));
        }
        donate_warm_state_and_invalidate(
            pending_warm_seeds.pool_mut(),
            &mut self.cache,
            &removed,
            NodeId::Value,
        );
        for cid in &changed_constraints {
            self.cache.invalidate(&NodeId::Constraint(cid.clone()));
        }
        donate_warm_state_and_invalidate(
            pending_warm_seeds.pool_mut(),
            &mut self.cache,
            &removed_constraints,
            NodeId::Constraint,
        );
        for rid in &changed_realizations {
            self.cache.invalidate(&NodeId::Realization(rid.clone()));
        }
        donate_warm_state_and_invalidate(
            pending_warm_seeds.pool_mut(),
            &mut self.cache,
            &removed_realizations,
            NodeId::Realization,
        );

        // (10) Attach provenance: Edit with the value-cell-level changed set
        //      (constraints / realizations remain implicit in the new graph;
        //      see plan.json design decision).
        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: changed_set.clone(),
            parent: parent_id,
        };

        // (11) Refresh function / purpose / meta / objective tables from the
        //      new module. A source edit can add/remove/change any of these;
        //      none are captured by the per-cell content_hash diff, so
        //      relying on cell-level diffing alone would silently serve
        //      stale tables (see eval() for the same refresh rationale).
        self.functions = merge_functions(module, &self.prelude_functions);
        self.compiled_purposes = module.compiled_purposes.clone();
        // Snapshot the field declarations so `Engine::edit_param` can
        // re-elaborate composed fields incrementally when their tracked
        // dependencies change (task 2343 step-8). Mirrors the same
        // assignment in `Engine::eval`.
        self.compiled_fields = Arc::new(module.fields.clone());
        self.meta_map = build_meta_map(module);
        self.objectives.clear();
        for template in &module.templates {
            if let Some(obj) = &template.objective {
                self.objectives.insert(template.name.clone(), obj.clone());
            }
        }

        // Arc::clone is O(1) — a refcount bump. The merged table was built and
        // sealed by `merge_functions` (see lib.rs) at the assignment above
        // (same pattern as Engine::eval). The local binding satisfies the borrow
        // checker the same way as edit_param() above.
        let functions = Arc::clone(&self.functions);

        // (12) Per-cell eval loop (shape mirrors edit_param's). Transitions
        //      cache entries in the eval set through Pending, iterates in
        //      topological order, evaluates each Value node's default_expr,
        //      and propagates Changed/Unchanged outcomes via
        //      has_changed_parent / skipped for early cutoff.
        self.cache.reset_pending_transition_count();
        for node_id in &eval_set {
            self.cache.mark_pending(node_id);
        }

        // Seed has_changed_parent from the dependents of every cell in the
        // changed_set (via the NEW reverse index) — these start the edit in
        // the "must not skip" state even before the root itself is evaluated.
        let mut has_changed_parent: HashSet<NodeId> = HashSet::new();
        for id in &changed_set {
            for dep in new_reverse_index.dependents_of(id) {
                has_changed_parent.insert(dep.clone());
            }
        }

        let mut skipped: HashSet<NodeId> = HashSet::new();
        let mut actual_eval_set: Vec<NodeId> = Vec::with_capacity(eval_set.len());

        for node_id in &eval_set {
            if skipped.contains(node_id) {
                continue;
            }
            actual_eval_set.push(node_id.clone());

            if let NodeId::Value(vcid) = node_id
                && let Some(node) = new_snapshot.graph.value_cells.get(vcid)
                && let Some(ref expr) = node.default_expr
            {
                let start = Instant::now();
                self.journal.record(EvalEvent {
                    timestamp: start,
                    node_id: node_id.clone(),
                    kind: EventKind::Started,
                    version: VersionId(version_id),
                    payload: None,
                });

                let val = reify_expr::eval_expr(
                    expr,
                    &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                );
                values.insert(vcid.clone(), val.clone());
                new_snapshot
                    .values
                    .insert(vcid.clone(), (val.clone(), DeterminacyState::Determined));

                let trace = extract_dependency_trace(expr);
                let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
                let outcome = self.cache.record_evaluation(
                    node_id.clone(),
                    cached_result,
                    VersionId(version_id),
                    trace,
                );

                self.journal.record(EvalEvent {
                    timestamp: Instant::now(),
                    node_id: node_id.clone(),
                    kind: EventKind::Completed { outcome },
                    version: VersionId(version_id),
                    payload: Some(EventPayload::Duration(start.elapsed())),
                });

                // Early-cutoff propagation — identical policy to edit_param:
                // - Changed: dependents inherit has_changed_parent and are
                //   unmarked from `skipped` (a Mixed-fan-in dependent may
                //   have been optimistically added by an Unchanged sibling).
                // - Unchanged: dependents enter `skipped` only if no Changed
                //   parent has been seen for them yet.
                let dependents = new_reverse_index.dependents_of(vcid);
                if outcome == EvalOutcome::Changed {
                    for dep in dependents {
                        has_changed_parent.insert(dep.clone());
                        skipped.remove(dep);
                    }
                } else {
                    for dep in dependents {
                        if !has_changed_parent.contains(dep) {
                            skipped.insert(dep.clone());
                        }
                    }
                }
            }
            // Constraint / Realization nodes: tracked in eval_set but not
            // evaluated here (deferred to check() / build()), same as in
            // edit_param.
        }

        // (13) Restore Final freshness for nodes the early-cutoff path
        //      skipped (they were pre-marked Pending but never re-evaluated).
        for node_id in &skipped {
            self.cache.restore_final(node_id);
        }

        // ── Post-eval phases — parity with edit_param's tail (step-14) ──
        //
        // The following four phases mirror the logic at the tail of
        // `edit_param` (guard re-elaboration, solver resolution + second
        // wave, post-resolution guard re-elaboration, collection-count
        // re-elaboration). Without them, a source edit that touches a
        // guard expression, a constraint governing an auto param, or a
        // collection count cell would leave downstream cells stale or
        // Undef. The cross-check test
        // `edit_source_matches_cold_eval_on_mixed_bracket_edit` and the
        // dedicated `edit_source_guard_expr_change_flips_active_branch`
        // test pin these phases.
        //
        // Differences from edit_param: phase 2's second-wave dirty cone
        // / eval-set use the NEW graph's `new_reverse_index` and
        // `new_trace_map` (rather than `self.eval_state.as_ref()`'s
        // pre-edit structures) because a source edit can change edges,
        // so dependents in the new graph may differ from the old. Phases
        // 3 and 4 still read `self.eval_state.as_ref()` for pre-edit
        // guard/count values; self.eval_state has NOT yet been replaced
        // (that happens in step 15 below).

        // Reset the per-edit guard-phase group evaluation counter. This counter
        // is incremented for each group that is NOT skipped in Phase 1 or Phase 3;
        // it is exposed via last_guard_phase_group_evals() for test assertions.
        self.last_guard_phase_group_evals = 0;
        // Reset the role-flip probe counter (task 2094). Counts detect_role_flip
        // invocations on the hot path; exposed via last_role_flip_probes() for
        // the deferred-probe perf-lock test.
        self.last_role_flip_probes = 0;

        // Cross-phase dedup map (task 2142, 2146): maps guard_cell → guard_val for
        // every group that Phase 1 actually re-elaborated in this edit_source call.
        // Phase 3 consults this map: it skips groups already covered by Phase 1 when
        // the recorded guard value matches the current value, but falls through to
        // full re-elaboration when wave2 has flipped the guard after Phase 1 recorded
        // it (task 2146 fix). Reelaboration is idempotent for a given guard value —
        // provided wave2 has not subsequently overwritten inactive members (the
        // post-wave2 cleanup in the solver block re-deactivates them, mirroring
        // task 2140). Declared at function scope because edit_source's Phase 1 is a
        // large multi-step block (role-flip probe + composite has_dirty_guards);
        // wrapping it as a block-expression would churn more lines than necessary.
        let mut phase1_reelaborated: HashMap<ValueCellId, Value> = HashMap::new();

        // ── Phase 1: Guard re-elaboration (dirty-cone trigger) ───────────
        // If any structure_controlling cell is in the dirty cone or
        // changed_set — e.g., because its expression or an input
        // changed — re-evaluate each guarded group's guard cell and
        // activate/deactivate branch members accordingly. This runs
        // BEFORE the resolution phase so guards gated on auto params
        // have the best-available (possibly Undef) inputs.
        //
        // We ALSO trigger Phase 1 when any `added` value cell intersects a
        // guarded group's members or else_members. This covers reviewer
        // comment #3: when an edit inserts a new `let` into an existing
        // `where … else` group without touching the guard expression or
        // any structure_controlling cell, the Step-12 per-cell eval loop
        // evaluates the new member's default_expr into a Determined value
        // — but if the new member lands on the *inactive* branch, cold eval
        // would deactivate it to Undef via `deactivate_if_not_auto`. Forcing
        // Phase 1 to run re-elaborates every guarded group, which routes
        // the added member through the correct activation path. This also
        // covers symmetric cases (added members on the active branch) —
        // Phase 1 just re-evaluates them, matching cold eval's behavior.
        //
        // We ALSO trigger Phase 1 when an existing cell's *role* within a
        // guarded group changes — i.e. it moves from the `members` branch
        // to the `else_members` branch (or vice versa) while its id and
        // expression text are unchanged. `diff_value_cells` compares per-cell
        // `content_hash` (id_hash.combine(expr_hash)), which has no notion of
        // containing group or branch, so a role-flipped cell is classified
        // neither `changed` nor `added`. Without this trigger, Phase 1 never
        // fires and the old-branch value survives on the wrong branch.
        // We detect this by building a per-cell role map (ValueCellId →
        // (guard_cell_id, branch_tag)) for both the old and new graphs and
        // firing when the maps differ. Phase 1's existing activation/deactivation
        // loop then routes every member through the correct path. Lock:
        // `edit_source_role_flipped_guard_member_matches_cold_eval` (task 2084).
        {
            let graph = &new_snapshot.graph;
            let has_added_guard_member = graph.guarded_groups.iter().any(|group| {
                group.members.iter().any(|m| added.contains(m))
                    || group.else_members.iter().any(|m| added.contains(m))
            });
            // Cheap check 1: structure_controlling dirtiness.
            let sc_dirty = graph.structure_controlling.iter().any(|sc_id| {
                dirty_cone.contains(&NodeId::Value(sc_id.clone())) || changed_set.contains(sc_id)
            });
            // Lazy memo for role-flip (task 2094): `detect_role_flip` builds two
            // O(N) HashMaps over guarded_groups. Defer behind the cheap checks —
            // when sc_dirty or has_added_guard_member already fires, we skip the
            // HashMap build entirely. The per-group skip below also needs this
            // value; memoize so detect_role_flip is called at most once per edit.
            // Correctness pinned by task-2084 locks. Counter tracked via
            // `self.last_role_flip_probes` for the perf-lock test (task 2094).
            //
            // `pending_warm_seeds` holds `&mut self.warm_pool` for the duration
            // of edit_source. The free function `probe_role_flip` accepts
            // disjoint borrows (`self.eval_state` and `self.last_role_flip_probes`
            // are distinct from `self.warm_pool`), so no whole-self conflict.
            // `old_groups` is a shared slice borrow from `self.eval_state` whose
            // lifetime spans both `probe_role_flip` call sites; it does not
            // conflict with `&mut self.warm_pool` (disjoint fields).
            let old_groups: &[GuardedGroupInfo] = self
                .eval_state
                .as_ref()
                .unwrap()
                .snapshot
                .graph
                .guarded_groups
                .as_slice();
            let mut role_flip_memo: Option<bool> = None;
            let has_dirty_guards = sc_dirty || has_added_guard_member || {
                let result = probe_role_flip(
                    old_groups,
                    &graph.guarded_groups,
                    &mut self.last_role_flip_probes,
                );
                role_flip_memo = Some(result);
                result
            };

            if has_dirty_guards {
                for group in &graph.guarded_groups {
                    // Re-evaluate the guard cell's expression
                    let guard_val = if let Some(node) = graph.value_cells.get(&group.guard_cell) {
                        if let Some(ref expr) = node.default_expr {
                            reify_expr::eval_expr(
                                expr,
                                &eval_ctx_with_meta(&values, &functions, &self.meta_map)
                                    .with_determinacy(&new_snapshot.values),
                            )
                        } else {
                            Value::Undef
                        }
                    } else {
                        Value::Undef
                    };
                    // Per-group skip: if this group's guard value is unchanged vs.
                    // the pre-edit snapshot, AND no members of this group were
                    // added in this edit, AND no role-flip was detected. Role-flip
                    // suppresses the skip because we can't identify which groups
                    // were affected without a full role-map walk. The role-flip
                    // check is evaluated lazily via role_flip_memo (task 2094):
                    // consult detect_role_flip only when both cheaper conditions
                    // pass. Gives zero detect_role_flip calls when sc_dirty fires
                    // the outer trigger AND every group's guard VALUE changed.
                    let has_added_in_group = group.members.iter().any(|m| added.contains(m))
                        || group.else_members.iter().any(|m| added.contains(m));
                    // Always write the guard cell value before the skip check.
                    // Phase 1 re-evaluates guards with a determinacy context that
                    // the main eval loop lacks; DeterminacyPredicate guards (e.g.
                    // `determined(x)`) evaluate to Undef in the main loop and must
                    // be corrected here — even when we skip member re-elaboration.
                    let guard_det = if matches!(&guard_val, Value::Bool(_)) {
                        DeterminacyState::Determined
                    } else {
                        DeterminacyState::Undetermined
                    };
                    values.insert(group.guard_cell.clone(), guard_val.clone());
                    new_snapshot
                        .values
                        .insert(group.guard_cell.clone(), (guard_val.clone(), guard_det));
                    if guard_value_unchanged(
                        self.eval_state.as_ref().map(|s| &s.snapshot.values),
                        &group.guard_cell,
                        &guard_val,
                    ) && !has_added_in_group
                    {
                        // Lazy role-flip check (task 2094): populate role_flip_memo
                        // on first query, reuse on subsequent groups.
                        let flipped = match role_flip_memo {
                            Some(v) => v,
                            None => {
                                let result = probe_role_flip(
                                    old_groups,
                                    &graph.guarded_groups,
                                    &mut self.last_role_flip_probes,
                                );
                                role_flip_memo = Some(result);
                                result
                            }
                        };
                        if !flipped {
                            continue;
                        }
                    }
                    // Skipped here ⇒ no entry in `phase1_reelaborated`,
                    // so Phase 3 reaches case (c): the standard `old_guard_val != current`
                    // check still catches any later wave2 flip that reverts the guard.
                    self.last_guard_phase_group_evals += 1;
                    // Record guard_cell → guard_val so Phase 3 can detect a
                    // wave2 flip: if the current guard value differs from the
                    // recorded value, Phase 3 falls through to full re-elaboration
                    // (task 2146 fix). The insert sits after the skip-continue so
                    // only actually-processed groups land in the map — guard-flip,
                    // added-member, and role-flip triggers all satisfy "Phase 1
                    // re-elaborated this group", which is precisely what Phase 3
                    // needs to know.
                    phase1_reelaborated.insert(group.guard_cell.clone(), guard_val.clone());

                    let is_true = matches!(&guard_val, Value::Bool(true));
                    let is_false = matches!(&guard_val, Value::Bool(false));

                    for mid in &group.members {
                        if is_true {
                            if let Some(node) = graph.value_cells.get(mid)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                                );
                                values.insert(mid.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(mid.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Auto cells skipped — see `deactivate_if_not_auto` doc.
                            deactivate_if_not_auto(
                                graph,
                                mid,
                                &mut values,
                                &mut new_snapshot.values,
                            );
                        }
                    }
                    for mid in &group.else_members {
                        if is_false {
                            if let Some(node) = graph.value_cells.get(mid)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                                );
                                values.insert(mid.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(mid.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Auto cells skipped — see `deactivate_if_not_auto` doc.
                            deactivate_if_not_auto(
                                graph,
                                mid,
                                &mut values,
                                &mut new_snapshot.values,
                            );
                        }
                    }
                }

                // Recompute topology fingerprint including guard states.
                let guard_state_hash =
                    guard_state_fingerprint(&graph.guarded_groups, &values, GuardLookup::Lenient);
                new_snapshot.topology_fingerprint =
                    graph.topology_fingerprint().combine(guard_state_hash);
            }
        }

        // ── Phase 2: Solver resolution + second-wave propagation ─────────
        // Reuses the same structure as edit_param's resolution phase, but
        // with two key substitutions: (a) the second-wave dirty cone and
        // eval set use `new_reverse_index`, `new_trace_map`, and
        // `new_demand` (rather than the pre-edit `self.eval_state` /
        // `self.demand`) because edit_source can reshape dependency edges;
        // (b) we draw `scope_name` from `self.objectives` just as before.
        let mut resolved_params: HashMap<ValueCellId, Value> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        if let Some(ref solver) = self.solver {
            // Group auto params by entity (template) name
            let mut entity_groups: HashMap<String, (Vec<AutoParam>, HashSet<ValueCellId>)> =
                HashMap::new();

            for (_, node) in new_snapshot.graph.value_cells.iter() {
                if node.kind.is_auto() {
                    let entry = entity_groups
                        .entry(node.id.entity.clone())
                        .or_insert_with(|| (Vec::new(), HashSet::new()));
                    entry.0.push(AutoParam {
                        id: node.id.clone(),
                        param_type: node.cell_type.clone(),
                        bounds: None,
                        free: node.kind.is_auto_free(),
                    });
                    entry.1.insert(node.id.clone());
                }
            }

            // Union of all resolved auto param IDs across groups for second wave
            let mut all_resolved_ids: HashSet<ValueCellId> = HashSet::new();

            // Snapshot current values BEFORE the loop so each group's solver
            // receives the same baseline — preventing cross-group contamination
            // where one group's resolved values leak into another group's input.
            let snapshot_values = values.clone();

            // Solve each entity group independently
            for (scope_name, (auto_param_list, auto_ids)) in &entity_groups {
                // Find constraints referencing this group's auto params
                let filtered_constraints: Vec<_> = new_snapshot
                    .graph
                    .constraints
                    .iter()
                    .filter(|(_, cnode)| {
                        let trace = extract_dependency_trace(&cnode.expr);
                        trace.reads.iter().any(|r| auto_ids.contains(r))
                    })
                    .map(|(_, cnode)| (cnode.id.clone(), cnode.expr.clone()))
                    .collect();

                // Check if any of those constraints are in the dirty cone
                let constraints_dirty = filtered_constraints
                    .iter()
                    .any(|(cid, _)| dirty_cone.contains(&NodeId::Constraint(cid.clone())));

                if !constraints_dirty {
                    continue;
                }

                // Look up the template-native objective by entity name.
                let objective = self.objectives.get(scope_name).cloned();

                // Build ResolutionProblem and solve
                let problem = ResolutionProblem {
                    auto_params: auto_param_list.clone(),
                    constraints: filtered_constraints,
                    current_values: snapshot_values.clone(),
                    objective,
                    // Arc::clone is O(1) — a refcount bump into the merged table
                    // already held by Engine.functions (tasks #1997, #2286).
                    functions: Arc::clone(&functions),
                };

                match solver.solve(&problem) {
                    SolveResult::Solved {
                        values: solver_values,
                        unique,
                    } => {
                        for (id, val) in &solver_values {
                            values.insert(id.clone(), val.clone());
                            resolved_params.insert(id.clone(), val.clone());
                            all_resolved_ids.insert(id.clone());

                            // Update snapshot values
                            new_snapshot
                                .values
                                .insert(id.clone(), (val.clone(), DeterminacyState::Determined));

                            // Update param_overrides so subsequent edits
                            // use the resolved value
                            self.param_overrides.insert(id.clone(), val.clone());

                            // Update cache
                            let node_id = NodeId::Value(id.clone());
                            let trace = DependencyTrace::default();
                            let cached_result =
                                CachedResult::Value(val.clone(), DeterminacyState::Determined);
                            self.cache.record_evaluation(
                                node_id,
                                cached_result,
                                VersionId(version_id),
                                trace,
                            );
                        }
                        if !unique {
                            for ap in auto_param_list {
                                if ap.free {
                                    diagnostics.push(Diagnostic::warning(format!(
                                        "Parameter `{}` resolved via auto(free) \
                                         -- result is not uniquely determined.",
                                        ap.id.member
                                    )));
                                }
                            }
                        }
                    }
                    SolveResult::Infeasible {
                        diagnostics: solver_diags,
                    } => {
                        diagnostics.extend(solver_diags);
                    }
                    SolveResult::NoProgress { reason } => {
                        diagnostics.push(Diagnostic::warning(format!(
                            "Constraint solver made no progress: {}",
                            reason
                        )));
                    }
                }
            }

            // ── Second propagation wave ─────────────────────────────────
            // Re-resolved auto params may have changed value. Let bindings
            // depending on them may not be in the original dirty cone.
            // For edit_source we MUST use the NEW reverse_index / trace_map
            // / demand (rather than self.eval_state's stale pre-edit
            // structures) because dependency edges may have shifted.
            if !all_resolved_ids.is_empty() {
                let wave2_dirty =
                    crate::dirty::compute_dirty_cone(&all_resolved_ids, &new_reverse_index);
                let wave2_eval =
                    crate::dirty::compute_eval_set(&wave2_dirty, &new_demand, &new_trace_map);

                for node_id in &wave2_eval {
                    if let NodeId::Value(vcid) = node_id
                        && let Some(node) = new_snapshot.graph.value_cells.get(vcid)
                        && let Some(ref expr) = node.default_expr
                    {
                        let val = reify_expr::eval_expr(
                            expr,
                            &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                        );
                        values.insert(vcid.clone(), val.clone());
                        new_snapshot
                            .values
                            .insert(vcid.clone(), (val.clone(), DeterminacyState::Determined));

                        // Update cache for re-evaluated node
                        let trace = extract_dependency_trace(expr);
                        let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
                        self.cache.record_evaluation(
                            node_id.clone(),
                            cached_result,
                            VersionId(version_id),
                            trace,
                        );
                    }
                }

                // Post-wave2 cleanup (tasks 2142, 2144): wave2 can re-evaluate
                // inactive-branch members of ANY guarded group — including groups
                // Phase 1 skipped via the per-group unchanged-guard short-circuit
                // (task 2144) and groups entirely outside the dirty-guard trigger.
                // Re-deactivate all guarded groups (idempotent for groups wave2
                // did not touch).  See `reapply_guard_deactivations_post_wave2`.
                // edit_source's wave2 uses local new_reverse_index /
                // new_trace_map / new_demand (not self.eval_state), so
                // the call lives directly inside `if !all_resolved_ids…`.
                reapply_guard_deactivations_post_wave2(
                    &new_snapshot.graph,
                    &mut values,
                    &mut new_snapshot.values,
                );
            }
        }

        // ── Phase 3: Guard re-elaboration (value-changed trigger) ────────
        // Catches guards whose computed boolean value differs from the
        // pre-edit snapshot — e.g., resolver resolved an auto param that
        // feeds the guard, or the dirty-cone path missed an edge (defensive).
        // Uses GuardLookup::Strict because eval() has populated every guard
        // cell by this point; a missing cell would be a logic error.
        //
        // `guard_changed` is also true when Phase 1 processed a group with a
        // guard value that wave2 subsequently changed (flip-then-revert); in
        // that case `group_needs_phase3` detects the inconsistency regardless
        // of whether the final guard value matches the pre-edit snapshot
        // (task 2146).
        {
            let guard_changed = new_snapshot.graph.guarded_groups.iter().any(|group| {
                let old_guard_val = old_guard_for(self.eval_state.as_ref(), &group.guard_cell);
                group_needs_phase3(group, &values, old_guard_val, &phase1_reelaborated)
            });

            if guard_changed {
                // Field-level borrow splitting: pre-bind `graph` so the loop can
                // iterate `&graph.guarded_groups` (shared) while `&mut new_snapshot.values`
                // (a disjoint field) remains exclusively borrowed inside the body.
                // This matches the Phase 1 pattern and the edit_param Phase 3 fix; no .clone() needed.
                let graph = &new_snapshot.graph;
                for group in &graph.guarded_groups {
                    // Cross-phase dedup — see `group_needs_phase3` (tasks 2142 / 2146).
                    // The unified predicate handles three cases: (a) Phase 1 with
                    // matching guard value → skip; (b) Phase 1 with different guard
                    // value (wave2 flip) → re-elaborate unconditionally; (c) Phase 1
                    // did not fire → fall back to old-vs-new skip. Phase 3 has no
                    // added-member or role-flip exception (those are Phase 1 concerns
                    // only).
                    let old_guard_val = old_guard_for(self.eval_state.as_ref(), &group.guard_cell);
                    if !group_needs_phase3(group, &values, old_guard_val, &phase1_reelaborated) {
                        continue;
                    }
                    // Absent-guard skip rationale and warn-before-assert invariant: see `phase3_get_guard_val` docs.
                    let Some(guard_val) = phase3_get_guard_val(&values, &group.guard_cell) else {
                        continue;
                    };
                    self.last_guard_phase_group_evals += 1;
                    let guard_is_true = matches!(&guard_val, Value::Bool(true));
                    let guard_is_false = matches!(&guard_val, Value::Bool(false));

                    for member_id in &group.members {
                        if guard_is_true {
                            if let Some(node) = graph.value_cells.get(member_id)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                                );
                                values.insert(member_id.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(member_id.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            deactivate_if_not_auto(
                                graph,
                                member_id,
                                &mut values,
                                &mut new_snapshot.values,
                            );
                        }
                    }

                    for member_id in &group.else_members {
                        if guard_is_false {
                            if let Some(node) = graph.value_cells.get(member_id)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                                );
                                values.insert(member_id.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(member_id.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            deactivate_if_not_auto(
                                graph,
                                member_id,
                                &mut values,
                                &mut new_snapshot.values,
                            );
                        }
                    }
                }

                let guard_state_hash =
                    guard_state_fingerprint(&graph.guarded_groups, &values, GuardLookup::Strict);
                new_snapshot.topology_fingerprint =
                    graph.topology_fingerprint().combine(guard_state_hash);
            }
        }

        // ── Phase 4: Collection count re-elaboration ─────────────────────
        // If any structure_controlling count cell's value changed vs. the
        // pre-edit snapshot, add/remove instances to match the new count.
        {
            let collection_subs = new_snapshot.graph.collection_subs.clone();
            for col_sub in &collection_subs {
                let new_count_val = values
                    .get(&col_sub.count_cell)
                    .cloned()
                    .unwrap_or(Value::Undef);
                let old_count_val = self
                    .eval_state
                    .as_ref()
                    .and_then(|s| s.snapshot.values.get(&col_sub.count_cell))
                    .map(|(v, _)| v.clone())
                    .unwrap_or(Value::Undef);

                if new_count_val == old_count_val {
                    continue;
                }

                // Helper closure: resolve a collection count value to an integer.
                let resolve_count = |val: &Value, label: &str| -> (i64, Option<Diagnostic>) {
                    match val {
                        Value::Int(n) => (*n, None),
                        Value::Undef => (0, None),
                        other => (
                            0,
                            Some(Diagnostic::warning(format!(
                                "Collection count cell {} has non-integer {} value {:?}; treating as 0",
                                col_sub.count_cell, label, other
                            ))),
                        ),
                    }
                };

                // Remove old instances from graph and snapshot
                let (old_count, old_warn) = resolve_count(&old_count_val, "old");
                if let Some(w) = old_warn {
                    diagnostics.push(w);
                }
                for i in 0..old_count {
                    let scoped_entity =
                        format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, i);
                    for (member, _, _, _) in &col_sub.child_value_cells {
                        let scoped_id = ValueCellId::new(&scoped_entity, member);
                        new_snapshot.graph.value_cells.remove(&scoped_id);
                        new_snapshot.values.remove(&scoped_id);
                        values.remove(&scoped_id);
                        // Task 2086 Fix 1: invalidate cache so a subsequent edit
                        // that re-adds a scoped cell at the same (parent, sub,
                        // index, member) key evaluates freshly instead of returning
                        // a stale CachedResult from a prior incarnation.
                        // Scope: this loop iterates `0..old_count` using the NEW
                        // `col_sub.child_value_cells` (the surviving shape), so it
                        // only covers members present in the new shape.  Scoped cells
                        // for members absent from the new shape are invalidated by
                        // Step (9)'s `diff_value_cells.removed` path (those cells are
                        // absent from `new_snapshot.graph.value_cells` and therefore
                        // classified as `removed` by `diff_value_cells`).
                        // Why Phase 4 still needs this despite Step (9): Step (9)
                        // catches index-count reductions (old index absent from new
                        // graph) and entirely-removed subs via `diff_value_cells`.
                        // This Fix 1 path catches same-index re-incarnation: when
                        // count shrinks from n to m, the surviving indices 0..m have
                        // an identical content_hash in both old and new snapshots
                        // (same scoped_id string + same default_expr from unchanged
                        // child template), so `diff_value_cells` classifies them as
                        // UNCHANGED and Step (9) does NOT invalidate them.  Phase 4's
                        // remove loop tears them down and its create loop re-inserts
                        // them without calling cache.record_evaluation; without this
                        // explicit invalidation the stale cache entry at V_A would
                        // survive and a later edit_source that re-expands the sub
                        // would short-circuit via `basis_version` and return wrong
                        // cached values (pinned by the grow→shrink→regrow regression
                        // test).
                        self.cache.invalidate(&NodeId::Value(scoped_id));
                    }
                }

                // Create new instances based on new count
                let (new_count, new_warn) = resolve_count(&new_count_val, "new");
                if let Some(w) = new_warn {
                    diagnostics.push(w);
                }
                for i in 0..new_count {
                    let scoped_entity =
                        format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, i);
                    for (member, kind, cell_type, default_expr) in &col_sub.child_value_cells {
                        let scoped_id = ValueCellId::new(&scoped_entity, member);
                        let id_hash = ContentHash::of_str(&format!("{}", scoped_id));
                        let expr_hash = default_expr
                            .as_ref()
                            .map(|e| e.content_hash)
                            .unwrap_or(ContentHash(0));
                        let node = crate::graph::ValueCellNode {
                            id: scoped_id.clone(),
                            kind: *kind,
                            cell_type: cell_type.clone(),
                            default_expr: default_expr.clone(),
                            content_hash: id_hash.combine(expr_hash),
                        };
                        new_snapshot
                            .graph
                            .value_cells
                            .insert(scoped_id.clone(), node);

                        let val = if let Some(expr) = default_expr {
                            reify_expr::eval_expr(
                                expr,
                                &eval_ctx_with_meta(&values, &functions, &self.meta_map),
                            )
                        } else {
                            Value::Undef
                        };
                        values.insert(scoped_id.clone(), val.clone());
                        new_snapshot
                            .values
                            .insert(scoped_id, (val, DeterminacyState::Determined));
                    }
                }

                // Update per-member synthetic lists: __list_{name}__{member}
                for (member, _, _, _) in &col_sub.child_value_cells {
                    let member_items: Vec<Value> = (0..new_count)
                        .map(|idx| {
                            let scoped_id = ValueCellId::new(
                                format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, idx),
                                member,
                            );
                            values.get(&scoped_id).cloned().unwrap_or(Value::Undef)
                        })
                        .collect();
                    let member_list_id = ValueCellId::new(
                        &col_sub.parent_entity,
                        format!("__list_{}__{}", col_sub.sub_name, member),
                    );
                    let member_list_val = Value::List(member_items);
                    values.insert(member_list_id.clone(), member_list_val.clone());
                    new_snapshot.values.insert(
                        member_list_id,
                        (member_list_val, DeterminacyState::Determined),
                    );
                }

                let count_state_hash = ContentHash::of_str(&format!(
                    "collection:{}={}",
                    col_sub.count_cell, new_count
                ));
                new_snapshot.topology_fingerprint = new_snapshot
                    .graph
                    .topology_fingerprint()
                    .combine(count_state_hash);
            }
        }

        // (14b) Drain `pending_warm_seeds` into the cache, completing the
        //       checkout-and-seed half of the WarmStatePool round-trip
        //       (arch §4.3 lines 539-540, §6.4 lines 654-660).
        //
        //       Why HERE: `cache.record_evaluation` clears `warm_state` on
        //       every call (see cache.rs:263). The drain MUST come after
        //       steps (12)/(13)/(14) of this function — (12) per-cell value
        //       eval, (13) guard re-elaboration / solver re-runs, (14)
        //       collection-sub fingerprint re-eval — because each of those
        //       paths can call `record_evaluation` on a seeded node and
        //       wipe the slot. After (14) all such re-evaluations are done
        //       and the cache slots are settled.
        //
        //       Round-trip preservation when no cache entry exists yet:
        //       `donate_warm_state` returns `false` (and consumes the
        //       state) for a node that is not in the cache. For Value
        //       cells `edit_source`'s eval loop (12) creates the entry,
        //       so the seed lands. For Constraint and Realization variants
        //       today, no cache entry exists at edit_source time —
        //       `engine_build.rs` and `check_constraints` populate those
        //       on demand from `build()` / `check()`. To avoid silently
        //       dropping state that was already taken from the pool by
        //       (4c), we probe `cache.get` first; if the entry exists the
        //       state is seeded, otherwise it is re-donated so it remains
        //       recoverable on a subsequent topology event. When those
        //       variants gain edit-time cache entries the donate-back
        //       path simply stops triggering.
        //
        //       The cache-miss re-donation now calls `donate_preserving_lru`
        //       (via `PendingWarmSeedsGuard::drain_into_cache_or_repool`),
        //       preserving the entry's original `last_accessed` stamp from
        //       step (4c).  This prevents a round-tripping entry from
        //       appearing "recently accessed" relative to entries that were
        //       never checked out, which would unfairly shield it from LRU
        //       eviction (reviewer suggestion S1, task 2516).
        //
        //       Early-return preservation: `pending_warm_seeds` is a
        //       `PendingWarmSeedsGuard` whose `Drop` impl re-donates any
        //       un-drained entries back to the pool. On the success path
        //       `drain_into_cache_or_repool` empties the map so the
        //       natural `Drop` at function-end is a no-op. On any `?`,
        //       `return Err(...)`, or panic between (4c) and here, `Drop`
        //       fires with a non-empty map and re-donates all surviving
        //       entries, making them recoverable on the next `edit_source`.
        pending_warm_seeds.drain_into_cache_or_repool(&mut self.cache);

        // (15) Install the new snapshot, dep structures, and demand; record
        //      actual_eval_set (excludes early-cutoff-skipped nodes).
        self.eval_state = Some(crate::EvaluationState {
            snapshot: new_snapshot,
            reverse_index: new_reverse_index,
            trace_map: new_trace_map,
        });
        self.demand = new_demand;
        self.last_eval_set = actual_eval_set;

        Ok(EvalResult {
            values,
            diagnostics,
            resolved_params,
        })
    }

    /// Evaluates ALL constraints (not just dirty ones) to produce a complete
    /// CheckResult, mirroring check()'s pattern but incrementally.
    ///
    /// Requires a prior call to eval() or check() to establish the baseline.
    pub fn edit_check(
        &mut self,
        cell: ValueCellId,
        new_value: reify_types::Value,
    ) -> Result<CheckResult, EngineError> {
        let eval_result = self.edit_param(cell, new_value)?;
        let (constraint_results, constraint_diagnostics) =
            self.check_constraints_with_values(&eval_result.values)?;

        let mut diagnostics = eval_result.diagnostics;
        diagnostics.extend(constraint_diagnostics);

        Ok(CheckResult {
            values: eval_result.values,
            constraint_results,
            diagnostics,
            resolved_params: eval_result.resolved_params,
        })
    }
}

#[cfg(test)]
mod tests {
    use reify_compiler::ValueCellKind;
    use reify_types::{
        CompiledExpr, ContentHash, DeterminacyState, PersistentMap, Type, Value, ValueCellId,
        ValueMap,
    };

    use std::collections::HashMap;

    use crate::graph::{EvaluationGraph, GuardedGroupInfo, ValueCellNode};

    use super::{
        deactivate_if_not_auto, group_needs_phase3, guard_value_unchanged, phase3_get_guard_val,
        reelaborate_guarded_group,
    };

    /// Construct a [`ValueCellNode`] for use in unit tests.
    ///
    /// The `content_hash` is derived deterministically from `id.to_string()`
    /// (`"entity.member"` format), so every unique `ValueCellId` produces a
    /// distinct hash without requiring callers to supply one explicitly.
    fn make_cell(
        id: &ValueCellId,
        kind: ValueCellKind,
        cell_type: Type,
        default_expr: Option<CompiledExpr>,
    ) -> ValueCellNode {
        ValueCellNode {
            id: id.clone(),
            kind,
            cell_type,
            default_expr,
            content_hash: ContentHash::of_str(&id.to_string()),
        }
    }

    /// Run [`reelaborate_guarded_group`] with `guard_val = Bool(guard)` and
    /// empty functions / meta on the supplied graph and group, returning the
    /// resulting `(values, snapshot_values)` maps.
    ///
    /// Collapses the 7-line call-site boilerplate into a single line, leaving
    /// each test as a thin setup + assertion wrapper.
    fn run_with_guard(
        graph: EvaluationGraph,
        group: GuardedGroupInfo,
        guard: bool,
    ) -> (
        ValueMap,
        PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    ) {
        let mut values = ValueMap::default();
        let mut snapshot_values = PersistentMap::default();
        reelaborate_guarded_group(
            &graph,
            &group,
            &Value::Bool(guard),
            &mut values,
            &mut snapshot_values,
            &[],
            &HashMap::new(),
        );
        (values, snapshot_values)
    }

    #[test]
    fn deactivate_if_not_auto_skips_auto_cell() {
        let id = ValueCellId::new("E", "auto_param");
        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Auto { free: false },
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("auto_param"),
            },
        );

        let mut values: ValueMap = ValueMap::default();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        deactivate_if_not_auto(&graph, &id, &mut values, &mut snapshot_values);

        // Auto cell: helper must NOT insert anything.
        assert!(
            values.get(&id).is_none(),
            "Auto cell must not be deactivated in values"
        );
        assert!(
            snapshot_values.get(&id).is_none(),
            "Auto cell must not be deactivated in snapshot_values"
        );
    }

    #[test]
    fn deactivate_if_not_auto_writes_undef_for_param() {
        let id = ValueCellId::new("E", "param");
        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("param"),
            },
        );

        let mut values: ValueMap = ValueMap::default();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        deactivate_if_not_auto(&graph, &id, &mut values, &mut snapshot_values);

        assert_eq!(values.get(&id), Some(&Value::Undef));
        assert_eq!(
            snapshot_values.get(&id),
            Some(&(Value::Undef, DeterminacyState::Undetermined))
        );
    }

    #[test]
    fn deactivate_if_not_auto_writes_undef_for_missing_cell() {
        let id = ValueCellId::new("X", "missing");
        let graph = EvaluationGraph::default(); // empty — cell not present

        let mut values: ValueMap = ValueMap::default();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        deactivate_if_not_auto(&graph, &id, &mut values, &mut snapshot_values);

        // Missing cell → treated as non-Auto → must be deactivated.
        assert_eq!(values.get(&id), Some(&Value::Undef));
        assert_eq!(
            snapshot_values.get(&id),
            Some(&(Value::Undef, DeterminacyState::Undetermined))
        );
    }

    /// Happy-path characterization: two valid groups with non-overlapping
    /// members produce the expected four-entry role map.
    #[test]
    fn build_role_map_returns_expected_map_for_valid_groups() {
        use std::collections::HashMap;

        use crate::graph::GuardedGroupInfo;

        use super::build_role_map;

        let g1 = ValueCellId::new("E1", "guard");
        let g2 = ValueCellId::new("E2", "guard");
        let a = ValueCellId::new("E1", "a");
        let b = ValueCellId::new("E1", "b");
        let c = ValueCellId::new("E2", "c");
        let d = ValueCellId::new("E2", "d");

        let group1 = GuardedGroupInfo {
            guard_cell: g1.clone(),
            members: vec![a.clone()],
            else_members: vec![b.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };
        let group2 = GuardedGroupInfo {
            guard_cell: g2.clone(),
            members: vec![c.clone()],
            else_members: vec![d.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let map: HashMap<ValueCellId, (ValueCellId, u8)> = build_role_map(&[group1, group2]);

        assert_eq!(map.len(), 4);
        assert_eq!(map.get(&a), Some(&(g1.clone(), 0u8)));
        assert_eq!(map.get(&b), Some(&(g1.clone(), 1u8)));
        assert_eq!(map.get(&c), Some(&(g2.clone(), 0u8)));
        assert_eq!(map.get(&d), Some(&(g2.clone(), 1u8)));
    }

    /// Duplicate ValueCellId across two groups must panic in debug builds.
    ///
    /// Gated by `#[cfg(debug_assertions)]` because `debug_assert!` is a no-op
    /// in release mode — without the gate `cargo test --release` would run the
    /// body, the silent overwrite would not panic, and `#[should_panic]` would
    /// fail. Pattern mirrors `crates/reify-expr/tests/gradient_tests.rs:4043`.
    #[cfg(debug_assertions)]
    #[should_panic(expected = "appeared in multiple guarded-group roles")]
    #[test]
    fn build_role_map_panics_on_duplicate_member() {
        use crate::graph::GuardedGroupInfo;

        use super::build_role_map;

        let g1 = ValueCellId::new("E1", "guard");
        let g2 = ValueCellId::new("E2", "guard");
        let shared = ValueCellId::new("E1", "shared");

        let group1 = GuardedGroupInfo {
            guard_cell: g1.clone(),
            members: vec![shared.clone()],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let group2 = GuardedGroupInfo {
            guard_cell: g2.clone(),
            members: vec![shared.clone()],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };

        // Must panic: `shared` appears in two groups.
        build_role_map(&[group1, group2]);
    }

    /// A ValueCellId in both `members` and `else_members` of the *same* group
    /// must NOT panic: intra-group duplicates are permitted and resolved by
    /// last-write semantics (else_members entry wins).
    ///
    /// This exercises the second `insert` call-site in `build_role_map` and
    /// pins the observable behavior for callers: the cell ends up mapped to
    /// `(guard_cell, 1u8)` (the else-branch tag) when it appears in both
    /// branches.  Real compiled modules can produce this pattern (e.g. an
    /// "effective" output cell that is active in both guard branches).
    #[test]
    fn build_role_map_intra_group_duplicate_last_write_wins() {
        use std::collections::HashMap;

        use crate::graph::GuardedGroupInfo;

        use super::build_role_map;

        let g1 = ValueCellId::new("E1", "guard");
        let shared = ValueCellId::new("E1", "shared");

        // `shared` appears in both `members` (branch 0) and `else_members`
        // (branch 1) of the same group.  Expected: no panic; else_members wins.
        let group = GuardedGroupInfo {
            guard_cell: g1.clone(),
            members: vec![shared.clone()],
            else_members: vec![shared.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let map: HashMap<ValueCellId, (ValueCellId, u8)> = build_role_map(&[group]);

        // One entry; else_members (branch 1) overwrites members (branch 0).
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&shared), Some(&(g1.clone(), 1u8)));
    }

    /// `detect_role_flip` must return `false` when old and new have the same
    /// intra-group duplicate shape: a cell appearing in both `members` and
    /// `else_members` of the same group on both sides.
    ///
    /// This pins the bug fix: the old inline probe would spuriously return `true`
    /// for this shape because `build_role_map` last-write-wins maps the cell
    /// to tag=1, but the new-graph walk sees tag=0 first (members iterated first),
    /// causing a per-element mismatch before the count check.  Symmetric
    /// `build_role_map` on both sides resolves identically → maps equal →
    /// no flip.
    #[test]
    fn detect_role_flip_identical_intra_group_duplicate_returns_false() {
        use crate::graph::GuardedGroupInfo;

        use super::detect_role_flip;

        let g1 = ValueCellId::new("E1", "guard");
        let shared = ValueCellId::new("E1", "shared");

        // Both old and new have the identical intra-group duplicate shape.
        let make_group = || GuardedGroupInfo {
            guard_cell: g1.clone(),
            members: vec![shared.clone()],
            else_members: vec![shared.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let old_groups = [make_group()];
        let new_groups = [make_group()];

        // Identical shapes → no role flip.
        assert!(
            !detect_role_flip(&old_groups, &new_groups),
            "detect_role_flip must return false for identical intra-group duplicate shapes"
        );
    }

    /// `detect_role_flip` must return `true` when branches are swapped between
    /// old and new: old has `members=[x], else_members=[y]` and new has
    /// `members=[y], else_members=[x]`.
    ///
    /// This is the positive-direction lock: it ensures the symmetric-map
    /// comparison doesn't over-relax.  If someone future-refactors the helper
    /// to return `false` unconditionally, or compares only key sets (ignoring
    /// branch tags), this test catches it.
    #[test]
    fn detect_role_flip_returns_true_for_cross_branch_swap() {
        use crate::graph::GuardedGroupInfo;

        use super::detect_role_flip;

        let g = ValueCellId::new("E", "guard");
        let x = ValueCellId::new("E", "x");
        let y = ValueCellId::new("E", "y");

        let old_groups = [GuardedGroupInfo {
            guard_cell: g.clone(),
            members: vec![x.clone()],
            else_members: vec![y.clone()],
            constraints: vec![],
            else_constraints: vec![],
        }];
        // New graph swaps the branches.
        let new_groups = [GuardedGroupInfo {
            guard_cell: g.clone(),
            members: vec![y.clone()],
            else_members: vec![x.clone()],
            constraints: vec![],
            else_constraints: vec![],
        }];

        assert!(
            detect_role_flip(&old_groups, &new_groups),
            "detect_role_flip must return true when member branches are swapped"
        );
    }

    /// `detect_role_flip` must return `false` for two empty slices — the empty
    /// fast-path in the helper avoids allocating two empty `HashMap`s and must
    /// return the correct answer.
    #[test]
    fn detect_role_flip_returns_false_for_empty_groups() {
        use super::detect_role_flip;

        assert!(
            !detect_role_flip(&[], &[]),
            "detect_role_flip must return false for two empty slices"
        );
    }

    /// When `guard_val = Bool(true)`, `reelaborate_guarded_group` must:
    ///   (a) evaluate the active-branch `members` cell's `default_expr` and
    ///       write the result into both `values` and `snapshot_values` with
    ///       `DeterminacyState::Determined`;
    ///   (b) deactivate inactive non-Auto `else_members` cells
    ///       (`Value::Undef` / `Undetermined`);
    ///   (c) leave inactive Auto `else_members` cells absent from both maps
    ///       (Auto cell lifecycle is owned by the solver, not guard logic).
    #[test]
    fn reelaborate_guarded_group_activates_members_when_guard_true() {
        let guard_id = ValueCellId::new("E", "guard");
        let member_id = ValueCellId::new("E", "member");
        let else_member_id = ValueCellId::new("E", "else_member");
        let auto_else_id = ValueCellId::new("E", "auto_else");

        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(
            guard_id.clone(),
            make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None),
        );
        graph.value_cells.insert(
            member_id.clone(),
            make_cell(
                &member_id,
                ValueCellKind::Param,
                Type::Int,
                Some(CompiledExpr::literal(Value::Int(42), Type::Int)),
            ),
        );
        graph.value_cells.insert(
            else_member_id.clone(),
            make_cell(&else_member_id, ValueCellKind::Param, Type::Int, None),
        );
        graph.value_cells.insert(
            auto_else_id.clone(),
            make_cell(
                &auto_else_id,
                ValueCellKind::Auto { free: false },
                Type::Real,
                None,
            ),
        );

        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![member_id.clone()],
            else_members: vec![else_member_id.clone(), auto_else_id.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // Active member: evaluated default_expr → Int(42), Determined.
        assert_eq!(values.get(&member_id), Some(&Value::Int(42)));
        assert_eq!(
            snapshot_values.get(&member_id),
            Some(&(Value::Int(42), DeterminacyState::Determined))
        );

        // Inactive non-Auto else_member: deactivated to Undef / Undetermined.
        assert_eq!(values.get(&else_member_id), Some(&Value::Undef));
        assert_eq!(
            snapshot_values.get(&else_member_id),
            Some(&(Value::Undef, DeterminacyState::Undetermined))
        );

        // Inactive Auto else_member: absent from both maps.
        assert!(
            values.get(&auto_else_id).is_none(),
            "Auto cell must not appear in values"
        );
        assert!(
            snapshot_values.get(&auto_else_id).is_none(),
            "Auto cell must not appear in snapshot_values"
        );
    }

    /// Pins the documented "Cells without a `default_expr` … are left unchanged"
    /// contract for the **active branch** of `reelaborate_guarded_group`.
    ///
    /// The member cell IS present in `graph.value_cells` but its `default_expr`
    /// is `None`, so the inner `if let Some(ref expr) = node.default_expr` guard
    /// fails and the function must silently skip the cell — leaving both `values`
    /// and `snapshot_values` empty for it.
    ///
    /// A regression that replaced the guarded `if let Some(node) = … && let
    /// Some(ref expr) = node.default_expr` with an unconditional insert (or that
    /// silently inserted `Value::Undef` on the missing-expr branch) would be
    /// caught here.
    #[test]
    fn reelaborate_guarded_group_active_member_without_default_expr_is_noop() {
        let guard_id = ValueCellId::new("E", "guard");
        let member_id = ValueCellId::new("E", "member");

        let mut graph = EvaluationGraph::default();
        // Guard cell is present (guard itself doesn't need a default_expr).
        graph.value_cells.insert(
            guard_id.clone(),
            make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None),
        );
        // Member cell IS present in the graph, but has no default_expr.
        graph.value_cells.insert(
            member_id.clone(),
            make_cell(&member_id, ValueCellKind::Param, Type::Int, None),
        );

        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![member_id.clone()],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // Active member with no default_expr: must be left entirely untouched.
        assert!(
            values.get(&member_id).is_none(),
            "Active member with default_expr=None must not appear in values"
        );
        assert!(
            snapshot_values.get(&member_id).is_none(),
            "Active member with default_expr=None must not appear in snapshot_values"
        );
    }

    /// Pins the "absent from the graph" half of the documented "Cells without a
    /// `default_expr` (or absent from the graph) are left unchanged" contract for
    /// the **active branch** of `reelaborate_guarded_group`.
    ///
    /// The member ID is included in `group.members` but is NOT inserted into
    /// `graph.value_cells`, so the outer `if let Some(node) = graph.value_cells.get(mid)`
    /// guard fails and the function must silently skip the cell — leaving both
    /// `values` and `snapshot_values` empty for it.
    ///
    /// A regression that dropped this guard (e.g. via `&graph.value_cells[mid]`,
    /// `.unwrap()`, or any unconditional insert keyed on the raw member id) would
    /// be caught here.
    #[test]
    fn reelaborate_guarded_group_active_member_absent_from_graph_is_noop() {
        let guard_id = ValueCellId::new("E", "guard");
        // member_id is referenced in the group but intentionally NOT inserted
        // into graph.value_cells — it is wholly absent from the graph.
        let member_id = ValueCellId::new("E", "member");

        let mut graph = EvaluationGraph::default();
        // Only the guard cell is in the graph.
        graph.value_cells.insert(
            guard_id.clone(),
            make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None),
        );

        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![member_id.clone()],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // Active member absent from graph: must be left entirely untouched.
        assert!(
            values.get(&member_id).is_none(),
            "Active member absent from graph must not appear in values"
        );
        assert!(
            snapshot_values.get(&member_id).is_none(),
            "Active member absent from graph must not appear in snapshot_values"
        );
    }

    /// Pins the behavior of `reelaborate_guarded_group` on the **inactive branch**
    /// for `else_members` when an `else_member` is present in the graph but its
    /// `default_expr` is `None`.
    ///
    /// With `guard_val = Bool(true)`, `else_members` are on the **inactive branch**
    /// and are passed to `deactivate_if_not_auto`. That helper does NOT inspect
    /// `default_expr` — it only checks whether the cell is `Auto`. A non-Auto cell
    /// (here `ValueCellKind::Param`) must be written as
    /// `Value::Undef / DeterminacyState::Undetermined` regardless of whether it
    /// carries a `default_expr`.
    ///
    /// A regression that skipped deactivation for cells without a `default_expr`
    /// (e.g. by wrapping the `deactivate_if_not_auto` call in a `default_expr.is_some()`
    /// guard) would be caught here.
    #[test]
    fn reelaborate_guarded_group_inactive_else_member_without_default_expr_is_deactivated() {
        let guard_id = ValueCellId::new("E", "guard");
        let else_member_id = ValueCellId::new("E", "else_member");

        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(
            guard_id.clone(),
            make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None),
        );
        // else_member IS in the graph but has no default_expr.
        graph.value_cells.insert(
            else_member_id.clone(),
            make_cell(&else_member_id, ValueCellKind::Param, Type::Int, None),
        );

        // guard=true → members active, else_members INACTIVE → deactivate_if_not_auto.
        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![],
            else_members: vec![else_member_id.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // deactivate_if_not_auto does not check default_expr; Param → Undef/Undetermined.
        assert_eq!(
            values.get(&else_member_id),
            Some(&Value::Undef),
            "Inactive Param else_member with default_expr=None must be deactivated to Undef"
        );
        assert_eq!(
            snapshot_values.get(&else_member_id),
            Some(&(Value::Undef, DeterminacyState::Undetermined)),
            "Inactive Param else_member with default_expr=None must be Undetermined in snapshot_values"
        );
    }

    /// Pins the behavior of `reelaborate_guarded_group` on the **inactive branch**
    /// for `else_members` when an `else_member` is wholly absent from
    /// `graph.value_cells`.
    ///
    /// With `guard_val = Bool(true)`, `else_members` are on the **inactive branch**
    /// and are passed to `deactivate_if_not_auto`. That helper treats a missing cell
    /// as non-Auto (preserving the prior `is_some_and` semantics documented in its
    /// docstring) and writes `Value::Undef / DeterminacyState::Undetermined`.
    ///
    /// A regression that skipped absent cells on the inactive branch (e.g. by
    /// wrapping the `deactivate_if_not_auto` call in a `graph.value_cells.get(mid)
    /// .is_some()` guard) would be caught here.
    #[test]
    fn reelaborate_guarded_group_inactive_else_member_absent_from_graph_is_deactivated() {
        let guard_id = ValueCellId::new("E", "guard");
        // else_member_id is included in the group but NOT inserted into graph.
        let else_member_id = ValueCellId::new("E", "else_member");

        let mut graph = EvaluationGraph::default();
        // Only the guard cell is in the graph.
        graph.value_cells.insert(
            guard_id.clone(),
            make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None),
        );

        // guard=true → members active, else_members INACTIVE → deactivate_if_not_auto.
        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![],
            else_members: vec![else_member_id.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // Missing cell → non-Auto treatment → Undef/Undetermined.
        assert_eq!(
            values.get(&else_member_id),
            Some(&Value::Undef),
            "Absent else_member must be deactivated to Undef on the inactive branch"
        );
        assert_eq!(
            snapshot_values.get(&else_member_id),
            Some(&(Value::Undef, DeterminacyState::Undetermined)),
            "Absent else_member must be Undetermined in snapshot_values on the inactive branch"
        );
    }

    /// When `guard_val = Bool(false)`, `reelaborate_guarded_group` must
    /// activate `else_members` and deactivate `members`.
    ///
    /// Also covers the non-Bool (`Value::Undef`) guard edge case: neither
    /// branch becomes active, so ALL members and else_members follow the
    /// deactivation path (non-Auto → Undef, Auto → absent).
    #[test]
    fn reelaborate_guarded_group_activates_else_members_when_guard_false() {
        // ── Shared graph ──────────────────────────────────────────────────
        let guard_id = ValueCellId::new("E", "guard");
        let member_id = ValueCellId::new("E", "member");
        let auto_member_id = ValueCellId::new("E", "auto_member");
        let else_member_id = ValueCellId::new("E", "else_member");

        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(
            guard_id.clone(),
            make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None),
        );
        graph.value_cells.insert(
            member_id.clone(),
            make_cell(&member_id, ValueCellKind::Param, Type::Int, None),
        );
        graph.value_cells.insert(
            auto_member_id.clone(),
            make_cell(
                &auto_member_id,
                ValueCellKind::Auto { free: false },
                Type::Real,
                None,
            ),
        );
        graph.value_cells.insert(
            else_member_id.clone(),
            make_cell(
                &else_member_id,
                ValueCellKind::Param,
                Type::Int,
                Some(CompiledExpr::literal(Value::Int(7), Type::Int)),
            ),
        );

        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![member_id.clone(), auto_member_id.clone()],
            else_members: vec![else_member_id.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        // ── guard = false: else_members active, members deactivated ───────
        {
            let mut values: ValueMap = ValueMap::default();
            let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
                PersistentMap::default();

            reelaborate_guarded_group(
                &graph,
                &group,
                &Value::Bool(false),
                &mut values,
                &mut snapshot_values,
                &[],
                &HashMap::new(),
            );

            // Active else_member: evaluated default_expr → Int(7), Determined.
            assert_eq!(values.get(&else_member_id), Some(&Value::Int(7)));
            assert_eq!(
                snapshot_values.get(&else_member_id),
                Some(&(Value::Int(7), DeterminacyState::Determined))
            );

            // Inactive non-Auto member: deactivated.
            assert_eq!(values.get(&member_id), Some(&Value::Undef));
            assert_eq!(
                snapshot_values.get(&member_id),
                Some(&(Value::Undef, DeterminacyState::Undetermined))
            );

            // Inactive Auto member: absent.
            assert!(
                values.get(&auto_member_id).is_none(),
                "Auto member must not appear"
            );
            assert!(
                snapshot_values.get(&auto_member_id).is_none(),
                "Auto member must not appear"
            );
        }

        // ── guard = Undef (non-Bool): both branches inactive ─────────────
        {
            let mut values: ValueMap = ValueMap::default();
            let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
                PersistentMap::default();

            reelaborate_guarded_group(
                &graph,
                &group,
                &Value::Undef,
                &mut values,
                &mut snapshot_values,
                &[],
                &HashMap::new(),
            );

            // Both branches deactivated: non-Auto → Undef, Auto → absent.
            assert_eq!(values.get(&member_id), Some(&Value::Undef));
            assert_eq!(
                snapshot_values.get(&member_id),
                Some(&(Value::Undef, DeterminacyState::Undetermined))
            );
            assert!(
                values.get(&auto_member_id).is_none(),
                "Auto member must not appear"
            );
            assert_eq!(values.get(&else_member_id), Some(&Value::Undef));
            assert_eq!(
                snapshot_values.get(&else_member_id),
                Some(&(Value::Undef, DeterminacyState::Undetermined))
            );
        }
    }

    // ── guard_value_unchanged ─────────────────────────────────────────────

    /// (a) Snapshot contains guard_cell with Bool(true); new_val is Bool(true)
    /// → guard is unchanged → returns true.
    #[test]
    fn guard_value_unchanged_returns_true_when_value_matches() {
        let guard_cell = ValueCellId::new("E", "guard");
        let mut snapshot: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();
        snapshot.insert(
            guard_cell.clone(),
            (Value::Bool(true), DeterminacyState::Determined),
        );

        assert!(guard_value_unchanged(
            Some(&snapshot),
            &guard_cell,
            &Value::Bool(true)
        ));
    }

    /// (b) Snapshot contains guard_cell with Bool(true); new_val is Bool(false)
    /// → guard changed → returns false.
    #[test]
    fn guard_value_unchanged_returns_false_when_value_differs() {
        let guard_cell = ValueCellId::new("E", "guard");
        let mut snapshot: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();
        snapshot.insert(
            guard_cell.clone(),
            (Value::Bool(true), DeterminacyState::Determined),
        );

        assert!(!guard_value_unchanged(
            Some(&snapshot),
            &guard_cell,
            &Value::Bool(false)
        ));
    }

    /// (c) Snapshot does NOT contain guard_cell → old value is absent → returns false.
    #[test]
    fn guard_value_unchanged_returns_false_when_cell_absent_from_snapshot() {
        let guard_cell = ValueCellId::new("E", "guard");
        let snapshot: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        assert!(!guard_value_unchanged(
            Some(&snapshot),
            &guard_cell,
            &Value::Bool(true)
        ));
    }

    /// (d) snapshot_values is None (no prior eval state) → returns false.
    #[test]
    fn guard_value_unchanged_returns_false_when_snapshot_is_none() {
        let guard_cell = ValueCellId::new("E", "guard");

        assert!(!guard_value_unchanged(
            None,
            &guard_cell,
            &Value::Bool(true)
        ));
    }

    // ── group_needs_phase3 ────────────────────────────────────────────────

    /// (a) Phase 1 recorded the **same** guard value as the current one
    /// → Phase 1's work is still valid → skip → returns false.
    /// The `old_guard_val` is irrelevant when Phase 1 touched the group.
    #[test]
    fn group_needs_phase3_returns_false_when_phase1_recorded_same_value() {
        let guard_cell = ValueCellId::new("E", "guard");
        let group = GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let mut values = ValueMap::default();
        values.insert(guard_cell.clone(), Value::Bool(true)); // current = true
        let mut phase1: HashMap<ValueCellId, Value> = HashMap::new();
        phase1.insert(guard_cell.clone(), Value::Bool(true)); // Phase 1 = true (same as current)

        // old = false, current = true (same as phase1 → case a)
        let needs = group_needs_phase3(
            &group,
            &values,
            Some(&Value::Bool(false)), // old
            &phase1,
        );
        assert!(
            !needs,
            "case (a): Phase 1 still valid → skip → needs_phase3=false"
        );
    }

    /// (b) Phase 1 recorded a **different** guard value from the current one
    /// (wave2 flipped the guard after Phase 1 ran) → must re-elaborate regardless
    /// of what the old snapshot says → returns true.
    ///
    /// This is the regression-locking case for task 2146: the guard currently
    /// matches the snapshot (both false), but Phase 1 had set it to true, so
    /// Phase 3 must not skip this group.
    #[test]
    fn group_needs_phase3_returns_true_when_phase1_recorded_different_value() {
        let guard_cell = ValueCellId::new("E", "guard");
        let group = GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let mut values = ValueMap::default();
        values.insert(guard_cell.clone(), Value::Bool(false)); // current (wave2 flipped back)
        let mut phase1: HashMap<ValueCellId, Value> = HashMap::new();
        phase1.insert(guard_cell.clone(), Value::Bool(true)); // Phase 1 evaluated to true

        // Wave2 flipped back to false; old snapshot was also false.
        // Even though current == old, Phase 1's record differs → case (b) → must re-elaborate.
        let needs = group_needs_phase3(
            &group,
            &values,
            Some(&Value::Bool(false)), // old snapshot
            &phase1,
        );
        assert!(
            needs,
            "case (b): wave2 flip → must re-elaborate → needs_phase3=true"
        );
    }

    /// (c-skip) Phase 1 did NOT touch this group AND guard is unchanged
    /// (current == old) → apply standard old-vs-new skip → returns false.
    #[test]
    fn group_needs_phase3_returns_false_when_phase1_empty_and_guard_unchanged() {
        let guard_cell = ValueCellId::new("E", "guard");
        let group = GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let mut values = ValueMap::default();
        values.insert(guard_cell.clone(), Value::Bool(true)); // current
        let phase1: HashMap<ValueCellId, Value> = HashMap::new();

        let needs = group_needs_phase3(
            &group,
            &values,
            Some(&Value::Bool(true)), // old == current → skip
            &phase1,
        );
        assert!(
            !needs,
            "case (c): guard unchanged → skip → needs_phase3=false"
        );
    }

    /// (c-re-elaborate) Phase 1 did NOT touch this group AND guard changed
    /// → must re-elaborate → returns true.
    #[test]
    fn group_needs_phase3_returns_true_when_phase1_empty_and_guard_changed() {
        let guard_cell = ValueCellId::new("E", "guard");
        let group = GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let mut values = ValueMap::default();
        values.insert(guard_cell.clone(), Value::Bool(false)); // current
        let phase1: HashMap<ValueCellId, Value> = HashMap::new();

        let needs = group_needs_phase3(
            &group,
            &values,
            Some(&Value::Bool(true)), // old ≠ current → must re-elaborate
            &phase1,
        );
        assert!(
            needs,
            "case (c): guard changed → must re-elaborate → needs_phase3=true"
        );
    }

    /// (c-no-prior) Phase 1 did NOT touch this group AND there is no prior
    /// guard value (None) → None ≠ Some(current) → returns true.
    #[test]
    fn group_needs_phase3_returns_true_when_phase1_empty_and_old_absent() {
        let guard_cell = ValueCellId::new("E", "guard");
        let group = GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let mut values = ValueMap::default();
        values.insert(guard_cell.clone(), Value::Bool(true)); // current
        let phase1: HashMap<ValueCellId, Value> = HashMap::new();

        let needs = group_needs_phase3(
            &group, &values,
            None, // no old value → None ≠ Some(Bool(true)) → must re-elaborate
            &phase1,
        );
        assert!(
            needs,
            "case (c): no prior value → must re-elaborate → needs_phase3=true"
        );
    }

    /// (b-undef) Phase 1 recorded Bool(true) but current is Undef
    /// → values differ → case (b) → must re-elaborate → returns true.
    #[test]
    fn group_needs_phase3_returns_true_when_phase1_recorded_bool_but_current_is_undef() {
        let guard_cell = ValueCellId::new("E", "guard");
        let group = GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let mut values = ValueMap::default();
        values.insert(guard_cell.clone(), Value::Undef); // current is Undef (distinct from Bool(true))
        let mut phase1: HashMap<ValueCellId, Value> = HashMap::new();
        phase1.insert(guard_cell.clone(), Value::Bool(true)); // Phase 1 set Bool(true)

        let needs = group_needs_phase3(
            &group,
            &values,
            Some(&Value::Bool(true)), // old value (irrelevant in case b)
            &phase1,
        );
        assert!(
            needs,
            "case (b): Phase1=Bool(true), current=Undef → must re-elaborate → needs_phase3=true"
        );
    }

    /// When guard_cell is ABSENT from values AND there is no prior value
    /// → no structural change → group_needs_phase3 returns false.
    #[test]
    fn group_needs_phase3_returns_false_when_guard_cell_absent_and_no_prior() {
        let guard_cell = ValueCellId::new("E", "guard");
        let group = GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let values = ValueMap::default(); // guard_cell absent
        let phase1: HashMap<ValueCellId, Value> = HashMap::new();

        let needs = group_needs_phase3(&group, &values, None, &phase1);
        assert!(!needs, "absent guard + no prior → group_needs_phase3=false");
    }

    /// When guard_cell is ABSENT from values but WAS present before
    /// → structural change (guard cell disappeared) → group_needs_phase3 returns true.
    #[test]
    fn group_needs_phase3_returns_true_when_guard_cell_disappears() {
        let guard_cell = ValueCellId::new("E", "guard");
        let group = GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let values = ValueMap::default(); // guard_cell absent
        let phase1: HashMap<ValueCellId, Value> = HashMap::new();

        // guard_cell was present before (old_guard_val = Some)
        let needs = group_needs_phase3(&group, &values, Some(&Value::Bool(true)), &phase1);
        assert!(
            needs,
            "guard cell disappeared → structural change → group_needs_phase3=true"
        );
    }

    /// Happy path: `phase3_get_guard_val` returns `Some(value)` when the guard
    /// cell is present in `values`.  The returned value is a clone of the stored
    /// entry — callers may mutate `values` independently afterwards.
    #[test]
    fn phase3_get_guard_val_returns_some_when_guard_present() {
        let guard_cell = ValueCellId::new("E", "guard");
        let mut values = ValueMap::default();
        values.insert(guard_cell.clone(), Value::Bool(true));

        let result = phase3_get_guard_val(&values, &guard_cell);
        assert_eq!(
            result,
            Some(Value::Bool(true)),
            "phase3_get_guard_val must return Some(Bool(true)) when the guard cell is present"
        );
    }

    /// Dual-mode absent-guard contract: when the guard cell is absent from
    /// `values`, `phase3_get_guard_val` must emit exactly one WARN event
    /// scoped to `reify_eval::engine_edit` and (in release builds) return `None`.
    ///
    /// The test wraps the call in `catch_unwind(AssertUnwindSafe(...))` so it
    /// runs in both debug and release builds:
    /// - In debug builds, `debug_assert!(false)` panics; `catch_unwind` returns
    ///   `Err(_)`.  The `#[cfg(debug_assertions)]` assertion below confirms this.
    /// - In release builds, the `debug_assert!` is a no-op; the helper returns
    ///   `None`.  The `#[cfg(not(debug_assertions))]` assertion below confirms this.
    ///
    /// The WARN fires *before* the `debug_assert!` (Warn-before-assert ordering
    /// is load-bearing), so the counter increments unconditionally regardless of
    /// build mode.  The WARN-counter assertion is therefore unconditional.
    #[test]
    fn phase3_get_guard_val_warns_and_returns_none_when_guard_absent() {
        use reify_test_support::CountingSubscriberBuilder;
        use std::panic::AssertUnwindSafe;
        use std::sync::atomic::Ordering;

        let guard_cell = ValueCellId::new("E", "guard");
        let values = ValueMap::default(); // guard_cell absent

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_eval::engine_edit")
            .build();
        let warn_arc = counters[&tracing::Level::WARN].clone();

        // Track whether the helper returned None using an AtomicBool (UnwindSafe
        // and Send, so it crosses the catch_unwind boundary without issue).
        // `Value` is not `Copy`, so `Cell<Option<Value>>` would not work here.
        #[cfg(not(debug_assertions))]
        let result_was_none = std::sync::atomic::AtomicBool::new(false);

        let unwind_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ret = tracing::subscriber::with_default(subscriber, || {
                phase3_get_guard_val(&values, &guard_cell)
            });
            #[cfg(not(debug_assertions))]
            result_was_none.store(_ret.is_none(), Ordering::Release);
        }));

        assert_eq!(
            warn_arc.load(Ordering::Acquire),
            1,
            "absent-guard arm: expected exactly one WARN from \
             reify_eval::engine_edit when the guard cell is absent from values"
        );

        #[cfg(debug_assertions)]
        assert!(
            unwind_result.is_err(),
            "absent-guard arm (debug build): catch_unwind must return Err(_) because \
             debug_assert!(false) panics when the guard cell is absent"
        );

        #[cfg(not(debug_assertions))]
        assert!(
            result_was_none.load(Ordering::Acquire),
            "absent-guard arm (release build): helper must return None \
             when the guard cell is absent"
        );
    }

    // -----------------------------------------------------------------------
    // PendingWarmSeedsGuard unit tests
    // (steps 1, 3, 5 — guard Drop contract, drain no-op, payload integrity)
    // -----------------------------------------------------------------------

    /// Drop with non-empty map re-donates remaining entries to the pool.
    ///
    /// Pins the primary guard contract: any entry that was checked out into the
    /// guard's staging map and NOT consumed by `drain_into_cache_or_repool` on
    /// the success path is re-donated to `pool` by `Drop`, preserving recoverability
    /// across early-return / panic / `?` between steps (4c) and (14b).
    #[test]
    fn pending_warm_seeds_guard_redonates_remaining_entries_on_drop() {
        use crate::cache::NodeId;
        use crate::warm_pool::WarmStatePool;
        use reify_types::OpaqueState;
        use super::PendingWarmSeedsGuard;

        let mut pool = WarmStatePool::new(1024);
        assert_eq!(pool.used_bytes(), 0);

        let node_id = NodeId::Value(ValueCellId::new("T", "x"));

        {
            let mut pending = PendingWarmSeedsGuard::new(&mut pool);
            pending.insert(node_id.clone(), OpaqueState::new(0xDEADBEEFu32, 16), std::time::Instant::now());
            // Guard goes out of scope here, triggering Drop — no drain called
        }

        // After Drop, the entry must have been re-donated to pool
        assert!(
            pool.used_bytes() >= 16,
            "Drop must re-donate entries to pool; used_bytes was {}",
            pool.used_bytes()
        );
        let recovered = pool
            .checkout(&node_id)
            .expect("entry must be recoverable from pool after guard Drop");
        assert_eq!(
            recovered.downcast::<u32>(),
            Some(0xDEADBEEF),
            "payload must be preserved through the drop re-donation"
        );
    }

    /// `drain_into_cache_or_repool` empties `self.map` so the subsequent
    /// natural `Drop` is a no-op (regression against double-donation).
    ///
    /// When neither entry has a matching cache entry, the drain re-donates
    /// both to the pool.  After the guard goes out of scope `Drop` fires with
    /// an empty map and does nothing, so `pool.used_bytes()` equals the sum
    /// of the two original sizes (not double).
    #[test]
    fn pending_warm_seeds_guard_drain_into_cache_or_repool_makes_drop_inert() {
        use crate::cache::{CacheStore, NodeId};
        use crate::warm_pool::WarmStatePool;
        use reify_types::{ConstraintNodeId, OpaqueState};
        use super::PendingWarmSeedsGuard;

        let mut pool = WarmStatePool::new(4096);
        let mut cache = CacheStore::new();

        let val_nid = NodeId::Value(ValueCellId::new("T", "v"));
        let con_nid = NodeId::Constraint(ConstraintNodeId::new("T", 0));

        {
            let mut pending = PendingWarmSeedsGuard::new(&mut pool);
            pending.insert(val_nid.clone(), OpaqueState::new(0xAAu8, 100), std::time::Instant::now());
            pending.insert(con_nid.clone(), OpaqueState::new(0xBBu8, 50), std::time::Instant::now());
            // Neither node has a cache entry → drain MUST re-donate both to pool.
            pending.drain_into_cache_or_repool(&mut cache);
            // Guard drops here with an empty map → Drop is a no-op.
        }

        assert_eq!(
            pool.used_bytes(),
            150,
            "drain must donate each entry exactly once (no double-donation)"
        );
        assert_eq!(pool.len(), 2, "pool must have exactly 2 entries");
        assert_eq!(
            pool.checkout(&val_nid)
                .expect("val entry must be in pool")
                .downcast::<u8>(),
            Some(0xAA),
            "Value payload preserved"
        );
        assert_eq!(
            pool.checkout(&con_nid)
                .expect("constraint entry must be in pool")
                .downcast::<u8>(),
            Some(0xBB),
            "Constraint payload preserved"
        );
    }

    /// Drop re-donation preserves the original payload bytes for all three
    /// NodeId variants (Value, Constraint, Realization), and the total
    /// `used_bytes` accounting matches the sum of the individual sizes.
    ///
    /// Pins variant-symmetry of the Drop re-donation path.
    #[test]
    fn pending_warm_seeds_guard_drop_preserves_payload_bytes_for_each_entry() {
        use crate::cache::NodeId;
        use crate::warm_pool::WarmStatePool;
        use reify_types::{ConstraintNodeId, OpaqueState, RealizationNodeId};
        use super::PendingWarmSeedsGuard;

        let mut pool = WarmStatePool::new(4096);

        let val_nid = NodeId::Value(ValueCellId::new("T", "v"));
        let con_nid = NodeId::Constraint(ConstraintNodeId::new("T", 0));
        let rea_nid = NodeId::Realization(RealizationNodeId::new("T", 0));

        {
            let mut pending = PendingWarmSeedsGuard::new(&mut pool);
            pending.insert(val_nid.clone(), OpaqueState::new(0xDEADu32, 8), std::time::Instant::now());
            pending.insert(con_nid.clone(), OpaqueState::new(0xBEEFu32, 16), std::time::Instant::now());
            pending.insert(rea_nid.clone(), OpaqueState::new(0xFEEDu32, 24), std::time::Instant::now());
            // No drain — Drop fires with all three entries
        }

        // Total: 8 + 16 + 24 = 48 bytes
        assert_eq!(
            pool.used_bytes(),
            48,
            "used_bytes must equal sum of all three entry sizes (8+16+24)"
        );

        assert_eq!(
            pool.checkout(&val_nid)
                .expect("Value entry must be in pool after Drop")
                .downcast::<u32>(),
            Some(0xDEAD),
            "Value payload preserved"
        );
        assert_eq!(
            pool.checkout(&con_nid)
                .expect("Constraint entry must be in pool after Drop")
                .downcast::<u32>(),
            Some(0xBEEF),
            "Constraint payload preserved"
        );
        assert_eq!(
            pool.checkout(&rea_nid)
                .expect("Realization entry must be in pool after Drop")
                .downcast::<u32>(),
            Some(0xFEED),
            "Realization payload preserved"
        );
    }

    /// `drain_into_cache_or_repool` cache-HIT arm: when the cache already holds
    /// an entry for a node, the staged warm-state is donated to the cache via
    /// `cache.donate_warm_state`, NOT re-donated to the pool.
    ///
    /// Pins `drain_into_cache_or_repool`'s cache-HIT arm:
    /// ```text
    /// if cache.get(&nid).is_some() { cache.donate_warm_state(&nid, state); }
    /// ```
    ///
    /// After drain the pool is empty (the cache-hit branch fired, not the
    /// pool re-donation branch), the warm-state is retrievable from the cache,
    /// and the subsequent guard Drop is inert (map was emptied by drain).
    #[test]
    fn pending_warm_seeds_guard_drain_cache_hit_makes_drop_inert() {
        use crate::cache::{CachedResult, CacheStore, NodeCache, NodeId};
        use crate::deps::DependencyTrace;
        use crate::warm_pool::WarmStatePool;
        use reify_types::{DeterminacyState, Freshness, OpaqueState, Value, VersionId};
        use super::PendingWarmSeedsGuard;

        const PAYLOAD: u32 = 0xCAFEBABE;
        const SIZE: usize = 8;

        let mut pool = WarmStatePool::new(1024);
        let mut cache = CacheStore::new();

        let nid = NodeId::Value(ValueCellId::new("T", "hit_node"));

        // Pre-populate the cache so `cache.get(&nid).is_some()` is true.
        cache.put(
            nid.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(0),
            ),
        );
        assert!(cache.get(&nid).is_some(), "cache entry must exist before drain");

        {
            let mut pending = PendingWarmSeedsGuard::new(&mut pool);
            pending.insert(
                nid.clone(),
                OpaqueState::new(PAYLOAD, SIZE),
                std::time::Instant::now(),
            );
            // Cache HIT → state goes to cache, NOT to pool.
            pending.drain_into_cache_or_repool(&mut cache);
            // Guard drops here with empty map → Drop is a no-op.
        }

        // (a) Pool must be empty: the cache-hit branch did NOT re-donate to pool.
        assert_eq!(
            pool.used_bytes(),
            0,
            "cache-hit: pool must remain empty after drain (entry went to cache)"
        );

        // (b) Warm-state must now be retrievable from the cache.
        let warm = cache
            .get_warm_state(&nid)
            .expect("cache-hit: warm-state must be in cache after drain");
        assert_eq!(
            warm.downcast::<u32>(),
            Some(PAYLOAD),
            "cache-hit: warm-state payload must be preserved"
        );

        // (c) After the guard has dropped (end of block above), the pool is
        //     still empty — Drop was inert because the map was empty after drain.
        assert_eq!(
            pool.used_bytes(),
            0,
            "cache-hit: pool must still be empty after guard Drop (Drop was inert)"
        );
    }

    /// `drain_into_cache_or_repool` cache-MISS arm: when the cache holds NO
    /// entry for a node, the staged warm-state is re-donated to the pool via
    /// `pool.donate_preserving_lru(nid, state, stamp)` — preserving the
    /// originally-staged `last_accessed` Instant rather than refreshing it
    /// to `Instant::now()`.
    ///
    /// Pins `drain_into_cache_or_repool`'s cache-MISS arm:
    /// ```text
    /// else { self.pool.donate_preserving_lru(nid, state, stamp); }
    /// ```
    ///
    /// After drain the pool holds exactly one entry — the re-donated state with
    /// the original payload bytes AND the original LRU stamp — the cache has no
    /// warm-state for the node, and the subsequent guard Drop is inert (map was
    /// emptied by drain, so no double re-donation occurs).
    ///
    /// Sibling of `pending_warm_seeds_guard_drain_cache_hit_makes_drop_inert`
    /// (the cache-HIT counterpart); both together pin the binary `if cache.get(&nid).is_some()`
    /// dispatch in `drain_into_cache_or_repool`.
    #[test]
    fn pending_warm_seeds_guard_drain_cache_miss_repools_with_lru_stamp() {
        use crate::cache::{CacheStore, NodeId};
        use crate::warm_pool::WarmStatePool;
        use reify_types::OpaqueState;
        use super::PendingWarmSeedsGuard;

        const PAYLOAD: u32 = 0xDEAD_BEEF;
        const SIZE: usize = 8;

        let mut pool = WarmStatePool::new(1024);
        let mut cache = CacheStore::new();

        let nid = NodeId::Value(ValueCellId::new("T", "miss_node"));

        // Cache is empty → `cache.get(&nid).is_some()` is false → MISS branch fires.
        assert!(
            cache.get(&nid).is_none(),
            "cache must be empty before drain so the MISS branch is taken"
        );

        // Capture the exact stamp passed to `insert` so we can assert exact equality
        // on the re-donated entry's `last_accessed` later.
        let stamp = std::time::Instant::now();

        {
            let mut pending = PendingWarmSeedsGuard::new(&mut pool);
            pending.insert(nid.clone(), OpaqueState::new(PAYLOAD, SIZE), stamp);
            // Cache MISS → state goes back to pool via `donate_preserving_lru(nid, state, stamp)`.
            pending.drain_into_cache_or_repool(&mut cache);
            // Guard drops here with empty map → Drop is a no-op (no double re-donation).
        }

        // (a) Pool must hold exactly one entry — Drop did NOT re-donate (drain emptied the map).
        assert_eq!(
            pool.len(),
            1,
            "cache-miss: pool must have exactly one entry after drain + Drop \
             (Drop must be inert because drain emptied the staging map; \
              a non-1 count indicates double re-donation by Drop)"
        );

        // (b) Byte-accounting must reflect the re-donated entry (before checkout drains it).
        assert_eq!(
            pool.used_bytes(),
            SIZE,
            "cache-miss: re-donated entry must be reflected in pool used_bytes accounting"
        );

        // (c) Cache must NOT carry a warm-state for this node — entry went to pool, not cache.
        assert!(
            cache.get_warm_state(&nid).is_none(),
            "cache-miss: cache must have no warm-state for this node \
             (cache had no entry, so the MISS branch fired and routed to pool)"
        );

        // (d) Pool entry must carry the original payload AND the original LRU stamp.
        let (state, recovered_stamp) = pool
            .checkout_with_lru_stamp(&nid)
            .expect("cache-miss: pool must hold the re-donated entry");
        assert_eq!(
            state.downcast::<u32>(),
            Some(PAYLOAD),
            "cache-miss: warm-state payload must be preserved through drain → re-pool"
        );
        assert_eq!(
            recovered_stamp,
            stamp,
            "cache-miss: re-donated entry's LRU stamp must EQUAL the originally-staged \
             stamp (donate_preserving_lru must NOT refresh to Instant::now())"
        );
    }

    /// Guard re-donates its staged entries when dropped during a panic unwind.
    ///
    /// This is the end-to-end test for the guard's primary safety contract:
    /// simulate `edit_source` step (4c) (checkout into the guard) followed by a
    /// panic between (4c) and (14b), then assert that the pool can recover the
    /// entry on the next call — i.e. the guard's `Drop` fired during unwind and
    /// re-donated the checked-out state.
    ///
    /// `AssertUnwindSafe<F>` is unconditionally `UnwindSafe` for any `F`, so
    /// capturing `&mut pool` directly is sufficient — no raw-pointer indirection
    /// required.
    #[test]
    fn pending_warm_seeds_guard_redonates_on_panic() {
        use crate::cache::NodeId;
        use crate::warm_pool::WarmStatePool;
        use reify_types::OpaqueState;
        use std::panic::{self, AssertUnwindSafe};
        use super::PendingWarmSeedsGuard;

        let mut pool = WarmStatePool::new(1024);
        let node_id = NodeId::Value(ValueCellId::new("T", "panic_node"));

        // Prime the pool with a known payload so we can verify recovery.
        pool.donate(node_id.clone(), OpaqueState::new(0xCAFEu32, 4));

        // Simulate step (4c): checkout → guard live → panic → Drop re-donates.
        // AssertUnwindSafe wraps the closure unconditionally, so &mut pool can
        // be captured directly without a raw-pointer indirection.
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let mut guard = PendingWarmSeedsGuard::new(&mut pool);
            let (state, stamp) = guard
                .pool_mut()
                .checkout_with_lru_stamp(&node_id)
                .expect("state should be in pool before panic");
            guard.insert(node_id.clone(), state, stamp);
            // Simulate a panic inside edit_source between steps (4c) and (14b).
            panic!("simulated mid-edit panic");
        }));
        assert!(result.is_err(), "catch_unwind must catch the panic");

        // After the unwind, the guard's Drop must have re-donated the entry.
        let recovered = pool
            .checkout(&node_id)
            .expect("pool must contain the entry re-donated by guard Drop");
        assert_eq!(
            recovered.downcast::<u32>(),
            Some(0xCAFE),
            "payload must survive the panic → Drop → pool round-trip"
        );
    }

    // -----------------------------------------------------------------------
    // Task 2523 S4/S5 — Drop safety-net telemetry
    // -----------------------------------------------------------------------

    /// Dropping a guard with a non-empty staging map (the panic/early-return
    /// safety-net path) emits exactly one `WARN` from `reify_eval::engine_edit`.
    ///
    /// Two arms are tested:
    ///
    /// 1. **Safety-net fires** — guard is dropped without calling
    ///    `drain_into_cache_or_repool`.  Drop re-donates the staged entry and
    ///    must emit a single `WARN`.
    ///
    /// 2. **Inert drop** — `drain_into_cache_or_repool` was called first (success
    ///    path). Drop fires with an empty map and must emit **zero** `WARN`s.
    #[test]
    fn pending_warm_seeds_guard_drop_emits_warn_when_safety_net_fires() {
        use crate::cache::{CacheStore, NodeId};
        use crate::warm_pool::WarmStatePool;
        use reify_test_support::CountingSubscriberBuilder;
        use reify_types::OpaqueState;
        use std::sync::atomic::Ordering;
        use super::PendingWarmSeedsGuard;

        // ---- Arm 1: safety-net fires (non-empty Drop) ----
        {
            let (subscriber, counters) = CountingSubscriberBuilder::new()
                .count_level(tracing::Level::WARN)
                .target_prefix("reify_eval::engine_edit")
                .build();
            let warn_arc = counters[&tracing::Level::WARN].clone();

            tracing::subscriber::with_default(subscriber, || {
                let mut pool = WarmStatePool::new(1024);
                let nid = NodeId::Value(ValueCellId::new("T", "safety_net"));
                let mut pending = PendingWarmSeedsGuard::new(&mut pool);
                pending.insert(nid, OpaqueState::new(0xABu8, 8), std::time::Instant::now());
                // Drop fires here with a non-empty map → safety net → must WARN.
            });

            assert_eq!(
                warn_arc.load(Ordering::Acquire),
                1,
                "safety-net arm: expected exactly one WARN from \
                 reify_eval::engine_edit when Drop fires with a non-empty map"
            );
        }

        // ---- Arm 2: inert drop (drain called first) ----
        {
            let (subscriber, counters) = CountingSubscriberBuilder::new()
                .count_level(tracing::Level::WARN)
                .target_prefix("reify_eval::engine_edit")
                .build();
            let warn_arc = counters[&tracing::Level::WARN].clone();

            tracing::subscriber::with_default(subscriber, || {
                let mut pool = WarmStatePool::new(1024);
                let mut cache = CacheStore::new();
                let nid = NodeId::Value(ValueCellId::new("T", "inert_drop"));
                let mut pending = PendingWarmSeedsGuard::new(&mut pool);
                pending.insert(nid, OpaqueState::new(0xCDu8, 8), std::time::Instant::now());
                // Success path: drain empties the map.
                pending.drain_into_cache_or_repool(&mut cache);
                // Drop fires here with an empty map → no WARN.
            });

            assert_eq!(
                warn_arc.load(Ordering::Acquire),
                0,
                "inert-drop arm: expected zero WARNs when Drop fires with an \
                 empty map (drain already consumed the entry)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 2516 S6 — Drop WARN structured-field schema
    // -----------------------------------------------------------------------

    /// Dropping a guard with a non-empty staging map emits a WARN event whose
    /// structured-field schema includes `count` (entry count, regression-locked).
    ///
    /// This test pins the *schema* — which fields are present and what value
    /// `count` carries — but does **not** assert the message-body wording.
    /// It mirrors `auto_trim_warn_omits_invariant_current_len_field` in
    /// `warm_pool.rs` by combining a positive `contains_key("count")` check,
    /// a value-pin `assert_eq!(…, Some("1"))`, and a discriminating negative
    /// `!contains_key("cap")` (the auto-trim WARN's signature field must not
    /// appear in the drop path).
    ///
    /// Structured fields with actionable, varying values are the unit of
    /// log-aggregator queries; body wording is verified by code review.
    #[test]
    fn pending_warm_seeds_guard_drop_warn_emits_count_field() {
        use crate::warm_pool::WarmStatePool;
        use crate::cache::NodeId;
        use reify_test_support::warn_capturing_subscriber;
        use reify_types::OpaqueState;
        use super::PendingWarmSeedsGuard;

        let (subscriber, capture) = warn_capturing_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            let mut pool = WarmStatePool::new(1024);
            let nid = NodeId::Value(ValueCellId::new("T", "count_field_test"));
            let mut pending = PendingWarmSeedsGuard::new(&mut pool);
            pending.insert(nid, OpaqueState::new(0xAAu8, 8), std::time::Instant::now());
            // Drop fires with a non-empty map → safety-net WARN must fire.
        });

        // Exactly one WARN should fire.
        capture.assert_count(1);

        let all_fields = capture.fields_by_event();
        let event_fields = &all_fields[0];

        // Regression-lock: `count` (established actionable field) must be present.
        assert!(
            event_fields.contains_key("count"),
            "safety-net WARN must include the `count` structured field; \
             got fields: {event_fields:?}"
        );

        // Value-pin: `count` must equal "1" for a single staged entry.
        // The test-support visitor captures integer fields via record_debug,
        // which stores format!("{value:?}"); for usize, Debug == Display == "1".
        assert_eq!(
            event_fields.get("count").map(String::as_str),
            Some("1"),
            "safety-net WARN's `count` field must equal \"1\" for a single staged \
             entry; got fields: {event_fields:?}"
        );

        // Discriminating negative: the auto-trim WARN's `cap` field must NOT
        // appear in the drop path (mirrors the negative assertion in
        // `auto_trim_warn_omits_invariant_current_len_field` in warm_pool.rs).
        assert!(
            !event_fields.contains_key("cap"),
            "`cap` must NOT appear in the safety-net WARN (it is the auto-trim \
             WARN's signature field; this drop path emits only `count`); \
             got fields: {event_fields:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Task 2516 S1 — guard LRU-stamp preservation (step-3 / step-4)
    // -----------------------------------------------------------------------

    /// `drain_into_cache_or_repool` must re-donate cache-miss entries with
    /// their *original* `last_accessed` stamp (via `donate_preserving_lru`),
    /// not a fresh `Instant::now()`.
    ///
    /// Setup (budget = 300):
    /// 1. Donate X (100 bytes) at time T_X.
    /// 2. Sleep 2 ms so T_X is strictly older than any later timestamp.
    /// 3. `checkout_with_lru_stamp(&X)` → `(x_state, x_stamp)`.
    /// 4. Build a guard; `pending.insert(X, x_state, x_stamp)`.
    /// 5. Donate Y (100 bytes) directly to pool; Y's stamp is newer than T_X.
    ///    used_pool = 100 (only Y, since X was checked out).
    /// 6. `drain_into_cache_or_repool(&mut empty_cache)`: X has no cache entry,
    ///    so it must be re-pooled via `donate_preserving_lru` preserving T_X.
    ///    used_pool = 200 (X + Y).
    /// 7. Donate Z (200 bytes): 200+200=400 > 300 → evict exactly one entry.
    ///    X's stamp (T_X) < Y's stamp → X is the LRU victim.
    ///
    /// Assert X is evicted and Y survives.  If `drain_into_cache_or_repool`
    /// used `pool.donate(…)` instead (refreshing the stamp), X would appear
    /// newer than Y and Y would be evicted — the test would flip.
    #[test]
    fn pending_warm_seeds_guard_drain_preserves_lru_stamp_for_repooled_entries() {
        use crate::cache::{CacheStore, NodeId};
        use crate::warm_pool::WarmStatePool;
        use reify_types::OpaqueState;
        use std::time::Duration;
        use super::PendingWarmSeedsGuard;

        // Budget just fits X+Y (200) but not X+Y+Z (400).
        let mut pool = WarmStatePool::new(300);
        let node_x = NodeId::Value(ValueCellId::new("T", "x"));
        let node_y = NodeId::Value(ValueCellId::new("T", "y"));
        let node_z = NodeId::Value(ValueCellId::new("T", "z"));

        // Step 1-2: donate X, then sleep so T_X is strictly older.
        // 15 ms is safely above worst-case Instant granularity (~15 ms on Windows /
        // frequency-scaling CI runners) so the LRU ordering is unconditionally observable.
        pool.donate(node_x.clone(), OpaqueState::new(0xAAu8, 100));
        std::thread::sleep(Duration::from_millis(15));

        // Step 3: capture X's state and its original stamp.
        let (x_state, x_stamp) = pool
            .checkout_with_lru_stamp(&node_x)
            .expect("X must be in pool");
        // pool is now empty (X checked out).

        // Step 4: build guard and stage X with its original stamp.
        let mut cache = CacheStore::new();
        {
            let mut pending = PendingWarmSeedsGuard::new(&mut pool);
            pending.insert(node_x.clone(), x_state, x_stamp);

            // Step 5: donate Y directly (Y's stamp is fresh / newer than x_stamp).
            pending.pool_mut().donate(node_y.clone(), OpaqueState::new(0xBBu8, 100));
            // pool holds Y (100 bytes); X is still in the guard's staging map.

            // Step 6: drain — X has no cache entry → cache-miss → donate_preserving_lru.
            pending.drain_into_cache_or_repool(&mut cache);
            // pool holds X (stamp = T_X) + Y (stamp > T_X), used = 200.
        }

        // Step 7: donate Z (200 bytes) — forces one eviction (200+200=400 > 300).
        pool.donate(node_z.clone(), OpaqueState::new(0xCCu8, 200));

        // X (oldest stamp) must be the LRU victim.
        assert!(
            pool.checkout(&node_x).is_none(),
            "X must be evicted: its preserved stamp T_X is older than Y's; \
             if drain used pool.donate() instead of donate_preserving_lru(), \
             X would look fresh and Y would be evicted instead"
        );
        assert!(
            pool.checkout(&node_y).is_some(),
            "Y must survive: its stamp is newer than X's preserved stamp"
        );
        assert!(
            pool.checkout(&node_z).is_some(),
            "Z (just donated) must remain in the pool"
        );
    }

    /// The panic-path `Drop` must also re-donate using `donate_preserving_lru`
    /// (preserving the original stamp), not the plain `donate` that refreshes
    /// the LRU clock.
    ///
    /// Same setup as `…drain_preserves_lru_stamp_for_repooled_entries` but the
    /// guard is dropped during a panic unwind instead of calling `drain_…`.
    #[test]
    fn pending_warm_seeds_guard_drop_preserves_lru_stamp_on_panic() {
        use crate::cache::NodeId;
        use crate::warm_pool::WarmStatePool;
        use reify_types::OpaqueState;
        use std::panic::{self, AssertUnwindSafe};
        use std::time::Duration;
        use super::PendingWarmSeedsGuard;

        let mut pool = WarmStatePool::new(300);
        let node_x = NodeId::Value(ValueCellId::new("T", "x"));
        let node_y = NodeId::Value(ValueCellId::new("T", "y"));
        let node_z = NodeId::Value(ValueCellId::new("T", "z"));

        // Donate X, sleep, then checkout to capture X's original (old) stamp.
        // 15 ms ensures strictly-ordered Instants on coarse-grained platforms.
        pool.donate(node_x.clone(), OpaqueState::new(0xAAu8, 100));
        std::thread::sleep(Duration::from_millis(15));
        let (x_state, x_stamp) = pool
            .checkout_with_lru_stamp(&node_x)
            .expect("X must be in pool");

        // Run the panic scenario: guard stages X then panics before drain.
        {
            // SAFETY: `pool` outlives the closure; no other reference to `pool`
            // exists while the closure runs.
            let pool_ptr: *mut WarmStatePool = &mut pool;
            let nid_x = node_x.clone();
            let nid_y = node_y.clone();
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                let pool_ref = unsafe { &mut *pool_ptr };
                let mut guard = PendingWarmSeedsGuard::new(pool_ref);
                guard.insert(nid_x.clone(), x_state, x_stamp);
                // Donate Y directly so it has a newer stamp than x_stamp.
                guard.pool_mut().donate(nid_y.clone(), OpaqueState::new(0xBBu8, 100));
                panic!("simulated mid-edit panic before drain");
            }));
            assert!(result.is_err(), "catch_unwind must catch the panic");
        }
        // Drop fired: X is back in pool with its preserved stamp.
        // pool holds X (stamp = T_X) + Y (stamp > T_X), used = 200.

        // Donate Z (200 bytes) → forces eviction (200+200=400 > 300).
        // X (oldest stamp) must be the LRU victim.
        pool.donate(node_z.clone(), OpaqueState::new(0xCCu8, 200));

        assert!(
            pool.checkout(&node_x).is_none(),
            "X must be evicted after Drop: its preserved stamp is older than Y's; \
             if Drop used pool.donate() instead of donate_preserving_lru(), \
             X would look fresh and Y would be the LRU victim"
        );
        assert!(
            pool.checkout(&node_y).is_some(),
            "Y must survive: stamp is newer than X's preserved stamp"
        );
        assert!(
            pool.checkout(&node_z).is_some(),
            "Z must remain (just donated)"
        );
    }
}
