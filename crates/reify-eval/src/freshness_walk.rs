//! Freshness-only propagation walk for value-unchanged transitions.
//!
//! Implements arch §3.5 (`docs/reify-implementation-architecture.md`, lines
//! 432-436): when a node's value is unchanged but its freshness transitions
//! (e.g. an upstream node flips Intermediate → Final), propagate freshness
//! downstream through the graph WITHOUT re-running any value evaluator. The
//! walk is a peer of the value-mode dirty cone walk in
//! [`crate::dirty::compute_dirty_cone`]; the two walks share the BFS skeleton
//! but differ in their write semantics.
//!
//! Freshness derivation reuses the §7.2/§9.2 truth-table helper
//! [`crate::cache::CacheStore::derive_output_freshness_for_node_with_cause`]
//! (arch §7.2 lines 730-749, arch §9.2 lines 880-890). Each visited dependent's
//! new freshness is computed from the freshnesses of its cached
//! `dependency_trace.reads`, and only written when the new freshness differs
//! from the current one — that comparison is the *freshness early cutoff* that
//! prunes the walk's frontier. Pending outputs are routed through the
//! canonical `mark_pending_with_cause` / `mark_pending` writers so the §9.2
//! diagnostic chain is preserved across freshness-only walks. Failed nodes
//! are treated as terminal chain roots: the walk skips them as write
//! targets but continues propagating from them to their downstream
//! dependents.
//!
//! ### Chain-root variants forwarded (PRD §3 / task δ 3423)
//!
//! The diagnostic chain root carried in `pending_cause` and forwarded by
//! this walk is variant-agnostic — the §9.2 helper neither inspects nor
//! special-cases the `NodeId` variant. Both chain-root forms are valid per
//! `docs/prds/v0_3/compute-node-contract.md` §3 ("Chain-root contract
//! extension"):
//!
//! - `NodeId::Value(_)` — a Failed leaf whose error gated a downstream cell
//!   (the original arch §9.2 case).
//! - `NodeId::Compute(_)` — an **in-flight ComputeNode** that is itself the
//!   chain root, set by
//!   [`crate::cache::CacheStore::begin_compute_dispatch`] via
//!   `mark_pending_with_cause` while a trampoline dispatch is running
//!   (task 3420/α admitted this variant; task 3423/δ wires the dispatch
//!   lifecycle that produces it). A downstream cell reading an in-flight
//!   output VC therefore receives `pending_cause =
//!   Some(NodeId::Compute(c_id))` through this walk with no walk-side change.
//!   Pinned by `cache::tests`'
//!   `cache_store_pending_cause_admits_compute_chain_root` and
//!   `derive_output_freshness_with_cause_forwards_compute_chain_root`, and
//!   end-to-end through this walk by `tests`'
//!   `propagate_freshness_only_forwards_compute_chain_root_through_pending_output_to_downstream`.
//!
//! ## Implementation notes
//!
//! ### `visited` allocated per-call, not persisted
//!
//! `visited` is allocated fresh on every call — never stashed on
//! [`CacheStore`] or any persistent collection. This makes the walk
//! idempotent under repeated invocation (step-13 / step-14): once
//! propagation has settled, the early-cutoff gate (`new == current`) prunes
//! every dependent on the second call, returning an empty `updated` set.
//! Persisting `visited` across calls would incorrectly skip cells that
//! legitimately need to be re-walked when a new edit triggers a fresh
//! propagation round.
//!
//! ### Dependents snapshotted before cache mutation
//!
//! Dependents are collected into a `Vec<NodeId>` before the per-dependent
//! loop so the immutable borrow on `reverse_index` is released before
//! `&mut cache` methods are called. Unlike
//! [`crate::dirty::compute_dirty_cone`] (`dirty.rs:29-38`) — which can
//! iterate the borrowed set directly because it does NOT mutate the cache
//! during iteration — this walk calls
//! `derive_output_freshness_for_node_with_cause`, `set_freshness`, and
//! `mark_pending_with_cause` per dependent, so the iteration borrow must
//! drop first. A `SmallVec<[NodeId; 4]>` could shave heap allocations for
//! the common low-fanout case, but that is a micro-optimization not
//! observed to matter in profiling today.

use std::collections::{HashSet, VecDeque};

use crate::cache::{CacheStore, NodeId};
use crate::deps::ReverseDependencyIndex;
use reify_core::ValueCellId;
use reify_ir::Freshness;

/// Propagate freshness forward through the dependents of `changed` cells
/// without recomputing any value, per arch §3.5 lines 432-436.
///
/// BFS forward walk from each `ValueCellId` in `changed`: for every dependent
/// found via [`ReverseDependencyIndex::dependents_of`], re-derive the
/// dependent's freshness from its cached `dependency_trace.reads` using
/// [`CacheStore::derive_output_freshness_for_node_with_cause`] and write it
/// back via the canonical writer matching the derived variant
/// ([`CacheStore::set_freshness`] for Final/Intermediate,
/// [`CacheStore::mark_pending_with_cause`] / [`CacheStore::mark_pending`]
/// for Pending). When the new freshness equals the current one — or when both
/// are Pending and the diagnostic chain root (`pending_cause`) already
/// matches — the *freshness early cutoff* fires at that node and propagation
/// stops along that branch.
///
/// Failed nodes are skipped as write targets (they are terminal chain roots
/// per `mark_failed`'s contract at `cache.rs:545-566`), but propagation
/// continues FROM them to their downstream dependents so chain-root
/// information is forwarded correctly.
///
/// `changed` accepts any iterable of borrowed [`ValueCellId`] references —
/// a `&HashSet<ValueCellId>`, a `&[ValueCellId]`, `std::iter::once(&id)`,
/// etc.  The items are consumed once to seed the BFS frontier; the data
/// only flows `iter → VecDeque`, so no intermediate collection is required.
///
/// Returns the set of [`NodeId`]s whose freshness was actually updated; nodes
/// pruned by early cutoff (or with no cache entry) are not included.
///
/// # Touch-list
///
/// **Touches:** `freshness`, and (transitively, via `mark_pending_with_cause`)
/// `pending_cause`. P3.3: when a visited dependent is `NodeId::Compute(_)`,
/// the walk also derives & writes freshness for each of the compute node's
/// declared `output_value_cells` (edge #12) so the freshness side-table
/// for those VCs reflects the upstream re-derivation. Like every other
/// write the walk performs, these go through `set_freshness` /
/// `mark_pending_with_cause` / `mark_pending` — never `put` or
/// `record_evaluation`.
///
/// **Does NOT touch:** `result`, `result_hash`, `dependency_trace`,
/// `basis_version`, `warm_state`. The walk also never calls `record_evaluation`
/// or `put` — only `set_freshness` / `mark_pending_with_cause` / `mark_pending`
/// (the canonical freshness writers in `cache.rs`).
///
/// This pins arch §3.5 line 432: "the input hash for downstream nodes is
/// unchanged, so no value recomputation occurs." The
/// `walk_does_not_recompute_values_or_bump_basis_version` unit test (step-7)
/// asserts this byte-by-byte.
pub fn propagate_freshness_only<'a>(
    cache: &mut CacheStore,
    reverse_index: &ReverseDependencyIndex,
    graph: &crate::graph::EvaluationGraph,
    changed: impl IntoIterator<Item = &'a ValueCellId>,
    generation: u64,
) -> HashSet<NodeId> {
    let mut updated: HashSet<NodeId> = HashSet::new();
    let mut frontier: VecDeque<ValueCellId> = changed.into_iter().cloned().collect();
    // Allocated per-call for idempotency — see module doc §"Implementation notes".
    let mut visited: HashSet<ValueCellId> = HashSet::new();

    while let Some(cell) = frontier.pop_front() {
        // Visited guard: skip a cell already processed this call
        // (prevents double-processing in diamond-shaped dependency graphs).
        if !visited.insert(cell.clone()) {
            continue;
        }

        // Snapshot dependents before mutating cache — see module doc §"Implementation notes".
        let dependents: Vec<NodeId> = reverse_index.dependents_of(&cell).iter().cloned().collect();

        for dependent in dependents {
            let cutoffs_passed = process_dependent_freshness(
                cache,
                &dependent,
                &mut frontier,
                &mut updated,
                generation,
                /* push_value_on_all_branches */ false,
            );

            // P3.3 step-16: edge #12 — Compute → output_value_cells fan-out.
            // Runs only when `cutoffs_passed` is true: the ComputeNode's
            // freshness actually transitioned (passed Failed-skip,
            // freshness-early, and Pending-idempotency cutoffs). If C's
            // derived freshness equals its current freshness, the fan-out
            // is skipped — the outputs are coupled to C's freshness via
            // this walk, so unchanged C implies unchanged outputs. The
            // per-output processing reuses `process_dependent_freshness`
            // (with `push_value_on_all_branches=true` so the more
            // conservative "always push" semantics mirror `dirty.rs:49-56`).
            if cutoffs_passed
                && let NodeId::Compute(cn_id) = &dependent
                && let Some(cn_data) = graph.compute_nodes.get(cn_id)
            {
                // Clone the list so the immutable borrow on `graph` drops
                // before any `&mut cache` write inside the inner loop.
                let outputs = cn_data.output_value_cells.clone();
                for out_vc in outputs {
                    let out_node = NodeId::Value(out_vc);
                    process_dependent_freshness(
                        cache,
                        &out_node,
                        &mut frontier,
                        &mut updated,
                        generation,
                        /* push_value_on_all_branches */ true,
                    );
                }
            }
        }
    }

    updated
}

/// Run the freshness-derivation step for a single dependent: read current
/// freshness, derive new freshness, apply the three cutoffs (Failed-skip,
/// freshness-early, Pending-idempotency), then route the write through
/// the canonical writers and record `updated` / `frontier` per the
/// established propagation rules.
///
/// Returns `true` when the canonical writer was invoked — i.e. all three
/// cutoffs were bypassed. The "absent-entry" belt-and-suspenders case
/// (writer returns false; see cache.rs:617-621) counts as `true` because
/// `new != current` did hold. Callers use this to gate further fan-out
/// (only the main per-dependent loop does; the output-VC inner loop
/// discards the return value).
///
/// `push_value_on_all_branches`:
/// - `false` (main per-dependent loop): push the dependent (if it's a
///   Value) onto the frontier only on Failed-skip or successful canonical
///   write. The cutoff branches do not push, matching the original
///   `continue`-without-push behaviour at the top-level loop.
/// - `true` (output-VC fan-out): push the dependent (always a Value) onto
///   the frontier in *every* branch — Failed, both cutoffs, and the
///   not-wrote case. Mirrors `dirty.rs:49-56` so downstream consumers of
///   the output VC always get a chance to re-derive even when this VC's
///   own freshness didn't change.
///
/// # Cross-references
///
/// - Failed-skip contract: arch §9.2 lines 880-890 + cache.rs:545-566
///   (`mark_failed`'s chain-root invariant). Re-deriving Failed via the
///   §7.2/§9.2 helper would silently flip it to Final/Intermediate/
///   Pending based on its inputs; we never write to a Failed node.
/// - Freshness early cutoff: arch §3.5 lines 432-436. The strict `==` is
///   correct because `Freshness` derives `PartialEq` and a single
///   `generation` parameter threads through the whole walk.
/// - Pending idempotency cutoff: locks in the fixed-point property under
///   repeated invocation. Compare `pending_cause` rather than
///   `last_substantive` because the canonical writers replace
///   `last_substantive` with `ResultRef::of_hash(entry.result_hash)` —
///   which the §9.2 helper never returns.
/// - Chain-root variant-agnosticism (PRD §3 / task δ 3423): the forwarded
///   `pending_cause` may be `NodeId::Value(_)` (a Failed leaf) **or**
///   `NodeId::Compute(_)` (an in-flight ComputeNode set by
///   `begin_compute_dispatch`/`mark_pending_with_cause`). This function
///   neither reads nor branches on the variant — it threads whatever the
///   §9.2 helper returns straight through — so the in-flight ComputeNode
///   chain root forwards onto downstream cells with no code-path change.
///   Pinned by `cache::tests`'
///   `cache_store_pending_cause_admits_compute_chain_root` /
///   `derive_output_freshness_with_cause_forwards_compute_chain_root`
///   (task 3420/α) and end-to-end by `tests`'
///   `propagate_freshness_only_forwards_compute_chain_root_through_pending_output_to_downstream`
///   (task 3423/δ).
fn process_dependent_freshness(
    cache: &mut CacheStore,
    dependent: &NodeId,
    frontier: &mut VecDeque<ValueCellId>,
    updated: &mut HashSet<NodeId>,
    generation: u64,
    push_value_on_all_branches: bool,
) -> bool {
    let current = cache.freshness(dependent);

    // Failed-skip: terminal chain-root state. Forward downstream via the
    // frontier so dependents become Pending-with-cause-rooted-at-Failed
    // (matches the canonical Pending writer's invariant). The push MUST
    // come before the return so the inner derivation at any downstream
    // node, which sees Failed as input, still fires.
    if matches!(current, Freshness::Failed { .. }) {
        if let NodeId::Value(vcid) = dependent {
            frontier.push_back(vcid.clone());
        }
        return false;
    }

    // still_refining=false: this walk runs after value-mode refinement
    // has settled, so derivation consults actual input freshnesses, not
    // the §7.2 refinement gate that short-circuits to Intermediate{generation}.
    let (new, cause) =
        cache.derive_output_freshness_for_node_with_cause(dependent, false, generation);

    // Freshness early cutoff: identical comparison for Final/Intermediate
    // (PartialEq + thread-shared generation). Pending requires the
    // separate cutoff below because the helper returns
    // `Pending { last_substantive: ResultRef::none() }` while the
    // writer replaces it with `ResultRef::of_hash(...)`, so naive `==`
    // never fires for already-Pending dependents and would re-bump
    // `pending_transition_count` on every walk.
    if new == current {
        if push_value_on_all_branches && let NodeId::Value(vcid) = dependent {
            frontier.push_back(vcid.clone());
        }
        return false;
    }

    // Pending idempotency cutoff: both freshnesses Pending AND
    // `pending_cause` matches what the writer would record ⇒ writer's
    // effect would be a no-op modulo the counter bump. Short-circuit to
    // keep the walk a true fixed-point operator for Pending transitions.
    if matches!(new, Freshness::Pending { .. })
        && matches!(current, Freshness::Pending { .. })
        && cache.pending_cause(dependent) == cause
    {
        if push_value_on_all_branches && let NodeId::Value(vcid) = dependent {
            frontier.push_back(vcid.clone());
        }
        return false;
    }

    // Route Pending writes through the canonical Pending writers so they
    // (a) capture `last_substantive: ResultRef::of_hash(...)` from the
    // entry's `result_hash`, and (b) record the §9.2 chain root via
    // `pending_cause`. Other variants (Final / Intermediate) go through
    // `set_freshness` directly. Failed is never returned by the §9.2
    // helper, so the `_ =>` arm cannot in practice receive it.
    let wrote = match &new {
        Freshness::Pending { .. } => match cause.clone() {
            Some(c) => cache.mark_pending_with_cause(dependent, c),
            None => cache.mark_pending(dependent),
        },
        _ => cache.set_freshness(dependent, new.clone()),
    };

    if wrote {
        updated.insert(dependent.clone());
        if let NodeId::Value(vcid) = dependent {
            frontier.push_back(vcid.clone());
        }
    } else if push_value_on_all_branches && let NodeId::Value(vcid) = dependent {
        // Belt-and-suspenders: the §7.2/§9.2 helper returns (Final, None)
        // for an absent dependent and `freshness()` returns Final by
        // default (cache.rs:617-621), so `new == current == Final` fires
        // the early cutoff before any writer is invoked — this branch is
        // unreachable in practice. Still push when the caller requested
        // "always push" to preserve the conservative output-VC semantics
        // against any future code path that bypasses that cutoff.
        frontier.push_back(vcid.clone());
    }

    true
}

#[cfg(test)]
mod tests {
    use crate::cache::{CacheStore, CachedResult, NodeCache, NodeId};
    use crate::deps::{DependencyTrace, ReverseDependencyIndex};
    use crate::graph::EvaluationGraph;
    use reify_core::{ContentHash, RealizationNodeId, ValueCellId, VersionId};
    use reify_ir::{DeterminacyState, ErrorRef, Freshness, GeometryHandleId, ResultRef, Value};
    use std::collections::HashSet;

    /// Helper: insert a Value-cell entry with the given concrete `Value`,
    /// freshness, reads, and basis_version. Used by step-7's snapshot test
    /// and the underlying primitive for `put_value_entry`.
    fn put_value_entry_with_payload(
        cache: &mut CacheStore,
        cell: &ValueCellId,
        value: Value,
        freshness: Freshness,
        reads: Vec<ValueCellId>,
        basis_version: VersionId,
    ) {
        cache.put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(value, DeterminacyState::Determined),
                freshness,
                DependencyTrace { reads },
                basis_version,
            ),
        );
    }

    /// Helper: insert a Value-cell cache entry with the given freshness and
    /// reads, defaulting the value/basis_version. Thin wrapper over
    /// [`put_value_entry_with_payload`] for tests that don't care about
    /// the concrete value or basis_version.
    fn put_value_entry(
        cache: &mut CacheStore,
        cell: &ValueCellId,
        freshness: Freshness,
        reads: Vec<ValueCellId>,
    ) {
        put_value_entry_with_payload(
            cache,
            cell,
            Value::Real(0.0),
            freshness,
            reads,
            VersionId(1),
        );
    }

    /// Step-1: a single Intermediate→Final transition on `a` propagates to `b`.
    ///
    /// Hand-built two-cell chain `a → b`:
    /// - cache: a, b both `Intermediate { generation: 1 }`
    /// - reverse_index: `a → {Value(b)}`
    /// - b's `dependency_trace.reads = [a]`
    ///
    /// After flipping `a` to Final and walking from `{a}`, `b` must be Final
    /// and the returned `updated` set must contain `Value(b)`.
    #[test]
    fn propagates_intermediate_to_final_through_two_cell_chain() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        let mut cache = CacheStore::new();
        put_value_entry(
            &mut cache,
            &a,
            Freshness::Intermediate { generation: 1 },
            vec![],
        );
        put_value_entry(
            &mut cache,
            &b,
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
        );

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));

        // Flip `a` to Final via the canonical writer; assert success so the
        // test fails loudly if the cache invariant changes.
        assert!(cache.set_freshness(&NodeId::Value(a.clone()), Freshness::Final));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        assert_eq!(
            cache.freshness(&NodeId::Value(b.clone())),
            Freshness::Final,
            "b should be Final after walking from a=Final"
        );
        assert!(
            updated.contains(&NodeId::Value(b.clone())),
            "updated set should contain Value(b), got: {:?}",
            updated
        );
    }

    /// Step-3: BFS frontier carries propagation past the immediate dependents.
    ///
    /// 3-cell chain `a → b → c` with all three Intermediate{generation: 1};
    /// `b.reads = [a]`, `c.reads = [b]`. Reverse-index has
    /// `a → {Value(b)}` and `b → {Value(c)}`. After flipping `a` to Final
    /// and walking from `{a}`, both `b` and `c` must end up Final and
    /// appear in the returned `updated` set.
    #[test]
    fn propagates_through_three_cell_chain_via_frontier() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let c = ValueCellId::new(e, "c");

        let mut cache = CacheStore::new();
        put_value_entry(
            &mut cache,
            &a,
            Freshness::Intermediate { generation: 1 },
            vec![],
        );
        put_value_entry(
            &mut cache,
            &b,
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
        );
        put_value_entry(
            &mut cache,
            &c,
            Freshness::Intermediate { generation: 1 },
            vec![b.clone()],
        );

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));
        reverse_index.add(b.clone(), NodeId::Value(c.clone()));

        assert!(cache.set_freshness(&NodeId::Value(a.clone()), Freshness::Final));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        assert_eq!(
            cache.freshness(&NodeId::Value(b.clone())),
            Freshness::Final,
            "b should be Final after walking from a=Final"
        );
        assert_eq!(
            cache.freshness(&NodeId::Value(c.clone())),
            Freshness::Final,
            "c should be Final once b's flip propagates through the BFS frontier"
        );
        assert!(
            updated.contains(&NodeId::Value(b.clone())),
            "updated should contain Value(b), got: {:?}",
            updated
        );
        assert!(
            updated.contains(&NodeId::Value(c.clone())),
            "updated should contain Value(c), got: {:?}",
            updated
        );
    }

    /// Step-5: Freshness early cutoff fires when a node's derived freshness
    /// equals its current freshness, halting propagation along that branch.
    ///
    /// 4-cell graph: `a → b`, `c → b`, `b → d`. All four start as
    /// `Intermediate { generation: 1 }`; `b.reads = [a, c]` and
    /// `d.reads = [b]`. After flipping `a` to Final (with `c` left
    /// Intermediate), the walk visits `b`'s derivation:
    /// - `still_refining = false`
    /// - inputs: a=Final, c=Intermediate{1}
    /// - §7.2 derivation: any non-Final input → Intermediate{generation: 1}
    /// - current: Intermediate{1} == derived → freshness early cutoff fires
    ///
    /// Pins arch §3.5 line 434: "If not (e.g. another input is still
    /// Intermediate), freshness early cutoff fires and propagation stops."
    /// `b` must remain Intermediate{1}, `d` must NOT be in the updated set,
    /// and `d`'s freshness must remain Intermediate{1}.
    #[test]
    fn freshness_early_cutoff_stops_walk_when_node_freshness_unchanged() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let c = ValueCellId::new(e, "c");
        let d = ValueCellId::new(e, "d");

        let mut cache = CacheStore::new();
        put_value_entry(
            &mut cache,
            &a,
            Freshness::Intermediate { generation: 1 },
            vec![],
        );
        put_value_entry(
            &mut cache,
            &c,
            Freshness::Intermediate { generation: 1 },
            vec![],
        );
        put_value_entry(
            &mut cache,
            &b,
            Freshness::Intermediate { generation: 1 },
            vec![a.clone(), c.clone()],
        );
        put_value_entry(
            &mut cache,
            &d,
            Freshness::Intermediate { generation: 1 },
            vec![b.clone()],
        );

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));
        reverse_index.add(c.clone(), NodeId::Value(b.clone()));
        reverse_index.add(b.clone(), NodeId::Value(d.clone()));

        // Flip ONLY a to Final; leave c as Intermediate.
        assert!(cache.set_freshness(&NodeId::Value(a.clone()), Freshness::Final));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        assert_eq!(
            cache.freshness(&NodeId::Value(b.clone())),
            Freshness::Intermediate { generation: 1 },
            "b should stay Intermediate{{1}} (c is still Intermediate, so b derives back to Intermediate)"
        );
        assert!(
            !updated.contains(&NodeId::Value(d.clone())),
            "d must NOT be in updated set — early cutoff at b stops the walk, got: {:?}",
            updated
        );
        assert_eq!(
            cache.freshness(&NodeId::Value(d.clone())),
            Freshness::Intermediate { generation: 1 },
            "d's freshness must be untouched (early cutoff at b prevents propagation to d)"
        );
        assert!(
            !updated.contains(&NodeId::Value(b.clone())),
            "b must NOT be in updated set — its freshness derived back to the same value"
        );
    }

    /// Step-7: the walk must not recompute values, mutate `result_hash`,
    /// or bump `basis_version`. Only `freshness` is touched.
    ///
    /// 2-cell chain `a → b` with concrete `Value::Real(5.0)` / `Value::Real(10.0)`
    /// payloads and `basis_version = VersionId(7)`. Snapshot each cached entry
    /// before the walk; after running the walk that flips `a` to Final and
    /// updates `b` to Final, assert byte-identical equality on the snapshotted
    /// `result_hash`, the inner `Value`, and `basis_version`.
    ///
    /// Pins arch §3.5 line 432: "the input hash for downstream nodes is
    /// unchanged, so no value recomputation occurs."
    #[test]
    fn walk_does_not_recompute_values_or_bump_basis_version() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let basis = VersionId(7);

        let mut cache = CacheStore::new();
        put_value_entry_with_payload(
            &mut cache,
            &a,
            Value::Real(5.0),
            Freshness::Intermediate { generation: 1 },
            vec![],
            basis,
        );
        put_value_entry_with_payload(
            &mut cache,
            &b,
            Value::Real(10.0),
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
            basis,
        );

        // Snapshot b's entry BEFORE the walk.
        let b_node = NodeId::Value(b.clone());
        let b_before = cache.get(&b_node).expect("b must be cached").clone();
        let b_before_hash = b_before.result_hash;
        let b_before_basis = b_before.basis_version;
        let b_before_value = match &b_before.result {
            CachedResult::Value(v, _) => v.clone(),
            other => panic!("expected CachedResult::Value, got {:?}", other),
        };

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));

        assert!(cache.set_freshness(&NodeId::Value(a.clone()), Freshness::Final));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );
        assert!(
            updated.contains(&b_node),
            "sanity: b must be updated by the walk, got: {:?}",
            updated
        );
        assert_eq!(
            cache.freshness(&b_node),
            Freshness::Final,
            "sanity: b must be Final after the walk"
        );

        // Snapshot b's entry AFTER the walk — every non-freshness field must be
        // byte-identical to the pre-walk snapshot.
        let b_after = cache.get(&b_node).expect("b must still be cached").clone();
        assert_eq!(
            b_after.result_hash, b_before_hash,
            "result_hash must be byte-identical (no value recomputation occurred)"
        );
        assert_eq!(
            b_after.basis_version, b_before_basis,
            "basis_version must be byte-identical (the walk does NOT bump versions)"
        );
        let b_after_value = match &b_after.result {
            CachedResult::Value(v, _) => v.clone(),
            other => panic!("expected CachedResult::Value, got {:?}", other),
        };
        assert_eq!(
            b_after_value, b_before_value,
            "cached Value must be byte-identical (no value recomputation occurred)"
        );
    }

    /// Step-13: The walk is a fixed-point operator under repeated invocation.
    ///
    /// 2-cell chain `a → b`. After flipping a to Final, the first walk
    /// flips b Intermediate{1} → Final and returns `{Value(b)}`. Running
    /// the same walk again with the same arguments must:
    /// - Return an empty `HashSet<NodeId>` (no node's freshness changed).
    /// - Leave the cache byte-identical to its post-first-walk state.
    ///
    /// Pins that the early-cutoff gate (step-6) plus the visited-cells guard
    /// (step-4) make repeated invocations no-ops once propagation has
    /// settled. The fixed-point property is what allows callers to safely
    /// re-run the walk after partial state changes (e.g. an edit_param
    /// follow-up after another walk has already flipped some downstream
    /// nodes) without double-counting updates.
    #[test]
    fn walk_is_idempotent_under_repeated_invocation() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        let mut cache = CacheStore::new();
        put_value_entry_with_payload(
            &mut cache,
            &a,
            Value::Real(5.0),
            Freshness::Intermediate { generation: 1 },
            vec![],
            VersionId(1),
        );
        put_value_entry_with_payload(
            &mut cache,
            &b,
            Value::Real(10.0),
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
            VersionId(1),
        );

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));

        let a_node = NodeId::Value(a.clone());
        let b_node = NodeId::Value(b.clone());

        // Flip a to Final and run the walk; b transitions Intermediate → Final.
        assert!(cache.set_freshness(&a_node, Freshness::Final));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated_first = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );
        assert!(
            updated_first.contains(&b_node),
            "sanity: first walk must update b, got: {:?}",
            updated_first
        );
        assert_eq!(
            cache.freshness(&b_node),
            Freshness::Final,
            "sanity: b must be Final after first walk"
        );

        // Snapshot every cached entry's relevant fields after the first walk.
        let a_after_first = cache.get(&a_node).expect("a cached").clone();
        let b_after_first = cache.get(&b_node).expect("b cached").clone();

        // Run the walk again with the same arguments.
        let updated_second = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        assert!(
            updated_second.is_empty(),
            "second walk must return an empty set (early cutoff fires at every dependent), got: {:?}",
            updated_second
        );

        // Cache must be byte-identical between the two snapshots.
        let a_after_second = cache.get(&a_node).expect("a cached").clone();
        let b_after_second = cache.get(&b_node).expect("b cached").clone();
        assert_eq!(
            a_after_second.freshness, a_after_first.freshness,
            "a's freshness must be byte-identical between successive walks"
        );
        assert_eq!(
            b_after_second.freshness, b_after_first.freshness,
            "b's freshness must be byte-identical between successive walks"
        );
        assert_eq!(
            a_after_second.result_hash, a_after_first.result_hash,
            "a's result_hash must be byte-identical (the walk never touches result_hash)"
        );
        assert_eq!(
            b_after_second.result_hash, b_after_first.result_hash,
            "b's result_hash must be byte-identical (the walk never touches result_hash)"
        );
        assert_eq!(
            a_after_second.basis_version, a_after_first.basis_version,
            "a's basis_version must be byte-identical (the walk never bumps basis_version)"
        );
        assert_eq!(
            b_after_second.basis_version, b_after_first.basis_version,
            "b's basis_version must be byte-identical (the walk never bumps basis_version)"
        );
    }

    /// Step-11: A Failed node is terminal — the walk must NOT re-derive it
    /// (which would silently flip Failed → Final/Intermediate/Pending based
    /// on its inputs and destroy the chain-root invariant) but MUST still
    /// propagate FROM it to its downstream dependents.
    ///
    /// 3-cell chain `a → b → c`, all initially `Intermediate { generation: 1 }`.
    /// Mark `b` as Failed via `mark_failed`. Snapshot b's freshness and
    /// `pending_cause` (== None per `mark_failed`'s contract). After flipping
    /// `a` to Final and walking from `{a}`:
    ///
    /// Asserts:
    /// - `b.freshness` is STILL `Failed { error: ErrorRef::new("b is broken") }`
    ///   (walk did NOT recompute it).
    /// - `b.pending_cause` is STILL `None` (walk did NOT touch the side-table).
    /// - `c.freshness == Pending { last_substantive: ResultRef::of_hash(c_prev_hash) }`
    ///   AND `c.pending_cause == Some(NodeId::Value(b))` — the walk DID
    ///   propagate FROM Failed b to c, treating Failed b as the chain root.
    ///
    /// Pins arch §9.2 lines 880-890 (Failed is terminal, downstream Pending
    /// propagation continues with the Failed leaf as chain root) and
    /// `mark_failed`'s contract at cache.rs:545-566.
    #[test]
    fn failed_node_is_terminal_and_skipped_during_walk() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let c = ValueCellId::new(e, "c");

        let mut cache = CacheStore::new();
        put_value_entry_with_payload(
            &mut cache,
            &a,
            Value::Real(5.0),
            Freshness::Intermediate { generation: 1 },
            vec![],
            VersionId(1),
        );
        put_value_entry_with_payload(
            &mut cache,
            &b,
            Value::Real(10.0),
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
            VersionId(1),
        );
        put_value_entry_with_payload(
            &mut cache,
            &c,
            Value::Real(20.0),
            Freshness::Intermediate { generation: 1 },
            vec![b.clone()],
            VersionId(1),
        );

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));
        reverse_index.add(b.clone(), NodeId::Value(c.clone()));

        let a_node = NodeId::Value(a.clone());
        let b_node = NodeId::Value(b.clone());
        let c_node = NodeId::Value(c.clone());

        // Snapshot c's prior result_hash for the Pending.last_substantive check.
        let c_prev_hash: ContentHash = cache.get(&c_node).expect("c cached").result_hash;

        // Mark b as Failed via the canonical writer; this clears
        // `pending_cause` per cache.rs:561.
        let b_error = ErrorRef::new("b is broken");
        assert!(cache.mark_failed(&b_node, b_error.clone()));
        assert_eq!(
            cache.freshness(&b_node),
            Freshness::Failed {
                error: b_error.clone()
            },
            "sanity: b must be Failed before the walk"
        );
        assert_eq!(
            cache.pending_cause(&b_node),
            None,
            "sanity: mark_failed clears pending_cause (cache.rs:561)"
        );

        // Flip a to Final; this is the edge that triggers the walk.
        assert!(cache.set_freshness(&a_node, Freshness::Final));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        // (i) b's freshness must STILL be Failed — the walk must skip it as
        //     a write target (re-derivation would destroy the chain root).
        assert_eq!(
            cache.freshness(&b_node),
            Freshness::Failed { error: b_error },
            "b must STILL be Failed — walk must not re-derive a Failed node"
        );
        // (ii) b's pending_cause must remain None — Failed nodes are chain
        //      roots, never forwarders (arch §9.2 + mark_failed contract).
        assert_eq!(
            cache.pending_cause(&b_node),
            None,
            "b's pending_cause must remain None — Failed is a chain root, not a forwarder"
        );
        // (iii) c must be Pending with last_substantive set to its prior
        //       result_hash, and its pending_cause must point at b (the
        //       Failed leaf). The walk DID still propagate FROM b to c.
        assert_eq!(
            cache.freshness(&c_node),
            Freshness::Pending {
                last_substantive: ResultRef::of_hash(c_prev_hash),
            },
            "c must be Pending with last_substantive = ResultRef::of_hash(c_prev_hash) \
             (arch §9.2: mark_pending_with_cause captures the cached hash)"
        );
        assert_eq!(
            cache.pending_cause(&c_node),
            Some(b_node.clone()),
            "c's pending_cause must be Some(Value(b)) — Failed b is the chain root"
        );
        assert!(
            updated.contains(&c_node),
            "updated must contain Value(c) — the walk propagated from Failed b to c, got: {:?}",
            updated
        );
    }

    /// Step-9: When an upstream node is Failed, the walk must record the
    /// downstream node as Pending and forward the Failed NodeId via the
    /// `pending_cause` side-table (arch §9.2 lines 880-890).
    ///
    /// 2-cell chain `a → b`: a present and Final initially, b present and
    /// Intermediate. Mark a as Failed via `mark_failed`. Walk from `{a}`.
    ///
    /// Asserts:
    /// - `b.freshness == Pending { last_substantive: ResultRef::of_hash(b_prev_hash) }`
    ///   (the value-bearing form set by `mark_pending_with_cause`).
    /// - `b.pending_cause == Some(NodeId::Value(a))`.
    /// - `updated` set contains `Value(b)`.
    #[test]
    fn failed_upstream_propagates_pending_with_cause() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        let mut cache = CacheStore::new();
        // Seed `a` with Final and `b` with Intermediate; both have concrete
        // payloads so `result_hash` carries a non-trivial value.
        put_value_entry_with_payload(
            &mut cache,
            &a,
            Value::Real(5.0),
            Freshness::Final,
            vec![],
            VersionId(1),
        );
        put_value_entry_with_payload(
            &mut cache,
            &b,
            Value::Real(10.0),
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
            VersionId(1),
        );

        // Snapshot b's prior result_hash for the Pending.last_substantive check.
        let b_node = NodeId::Value(b.clone());
        let b_prev_hash: ContentHash = cache.get(&b_node).expect("b cached").result_hash;

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));

        // Mark a as Failed via the canonical writer.
        let a_node = NodeId::Value(a.clone());
        assert!(cache.mark_failed(&a_node, ErrorRef::new("synthetic")));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        assert!(
            updated.contains(&b_node),
            "updated must contain Value(b), got: {:?}",
            updated
        );
        assert_eq!(
            cache.freshness(&b_node),
            Freshness::Pending {
                last_substantive: ResultRef::of_hash(b_prev_hash),
            },
            "b must be Pending with last_substantive set to its prior result_hash \
             (arch §9.2: mark_pending_with_cause captures the current cached hash)"
        );
        assert_eq!(
            cache.pending_cause(&b_node),
            Some(a_node),
            "b's pending_cause must be Some(Value(a)) — the chain root for arch §9.2"
        );
    }

    /// Amendment: Pending idempotency cutoff — when both the derived and
    /// current freshness are Pending and the diagnostic-chain cause already
    /// matches, the walk must short-circuit (no write, no counter bump,
    /// not in the returned `updated` set).
    ///
    /// 2-cell chain `a → b`: `a` Failed, `b` Intermediate, then run the walk
    /// once so `b` becomes Pending with `pending_cause = Some(Value(a))`.
    /// Snapshot the cache's `pending_transition_count`. Re-run the walk with
    /// identical arguments. The second invocation must:
    /// - Return an empty `HashSet<NodeId>`.
    /// - Leave the cache's `pending_transition_count` byte-identical to the
    ///   snapshot (no counter bump from a redundant `mark_pending_with_cause`).
    /// - Leave `b`'s freshness and `pending_cause` byte-identical.
    ///
    /// This pins the fix for the `reviewer_comprehensive` finding that the
    /// naive `new == current` cutoff did NOT fire for already-Pending
    /// dependents (the pure helper returns `Pending { last_substantive:
    /// ResultRef::none() }` while the writer stores `Pending {
    /// last_substantive: ResultRef::of_hash(...) }`), so the walk would
    /// re-write the same final state and bump `pending_transition_count`
    /// on every invocation.
    #[test]
    fn walk_is_idempotent_for_pending_with_cause_transitions() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        let mut cache = CacheStore::new();
        put_value_entry_with_payload(
            &mut cache,
            &a,
            Value::Real(5.0),
            Freshness::Final,
            vec![],
            VersionId(1),
        );
        put_value_entry_with_payload(
            &mut cache,
            &b,
            Value::Real(10.0),
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
            VersionId(1),
        );

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));

        let a_node = NodeId::Value(a.clone());
        let b_node = NodeId::Value(b.clone());

        // Mark a as Failed and run the walk once: b transitions to Pending
        // with cause = Some(Value(a)), driven by mark_pending_with_cause.
        assert!(cache.mark_failed(&a_node, ErrorRef::new("synthetic")));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated_first = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );
        assert!(
            updated_first.contains(&b_node),
            "sanity: first walk must Pending-flip b, got: {:?}",
            updated_first
        );
        assert_eq!(
            cache.pending_cause(&b_node),
            Some(a_node.clone()),
            "sanity: first walk must record cause = Some(Value(a))"
        );

        // Snapshot counter and entry state after the first walk.
        let counter_after_first = cache.pending_transition_count();
        let b_freshness_after_first = cache.freshness(&b_node);
        let b_cause_after_first = cache.pending_cause(&b_node);

        // Re-run with identical arguments — the Pending idempotency cutoff
        // must short-circuit at b.
        let updated_second = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        assert!(
            updated_second.is_empty(),
            "second walk must return empty (Pending idempotency cutoff fires), got: {:?}",
            updated_second
        );
        assert_eq!(
            cache.pending_transition_count(),
            counter_after_first,
            "pending_transition_count must NOT bump on the second walk \
             (no redundant mark_pending_with_cause invocation)"
        );
        assert_eq!(
            cache.freshness(&b_node),
            b_freshness_after_first,
            "b's freshness must be byte-identical between successive walks"
        );
        assert_eq!(
            cache.pending_cause(&b_node),
            b_cause_after_first,
            "b's pending_cause must be byte-identical between successive walks"
        );
    }

    /// U1 — Realization-Failed-skip invariant: when a `NodeId::Realization(_)`
    /// dependent has `Failed` freshness, the walk must skip it as a write target
    /// AND must NOT push it onto the BFS frontier.
    ///
    /// Realization sinks match the `dirty.rs:33` convention: only Value variants
    /// propagate via `frontier.push_back`. The `if let NodeId::Value(vcid)` guard
    /// at `freshness_walk.rs:131` does not match for Realization variants, so a
    /// Failed Realization both (a) is skipped as a write target (the `continue`
    /// at line 134 fires before derivation) and (b) is not pushed onto the
    /// frontier (the `push_back` at line 132 is guarded by the `NodeId::Value`
    /// arm). This means the walk dead-ends at R without attempting to propagate
    /// from it.
    ///
    /// Contrast with the Value-Failed case
    /// (`failed_node_is_terminal_and_skipped_during_walk`, lines 693-795): a
    /// Failed Value node DOES push onto the frontier so that downstream c
    /// becomes Pending. The Realization case is the opposite — no push, no
    /// downstream propagation.
    ///
    /// Topology:
    /// - `a: ValueCellId` seeded `Intermediate { generation: 1 }` (no reads).
    /// - `R: RealizationNodeId` inserted via `cache.put(Realization(R), …)`
    ///   with `Freshness::Intermediate { generation: 1 }` and
    ///   `dependency_trace.reads = [a]`, then transitioned to `Failed` via
    ///   `mark_failed`. `GeometryHandle(0)` is the standard synthetic result
    ///   (cache.rs:102-107).
    /// - `reverse_index: a → {Realization(R)}`.
    ///
    /// Walk: flip `a` to `Final` →
    /// `propagate_freshness_only(&mut cache, &reverse_index, &{a}, 1)`.
    ///
    /// Pins `freshness_walk.rs:130-135` (Failed-skip + Realization-no-push).
    /// Pins `mark_failed`'s contract at `cache.rs:545-566` (clears pending_cause).
    #[test]
    fn failed_realization_dependent_is_skipped_and_not_pushed() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let rid = RealizationNodeId::new(e, 0);
        let r_node = NodeId::Realization(rid.clone());

        let mut cache = CacheStore::new();
        // Seed `a` with Intermediate so the walk has something to propagate from.
        put_value_entry(
            &mut cache,
            &a,
            Freshness::Intermediate { generation: 1 },
            vec![],
        );

        // Insert the Realization entry directly via cache.put. No test-instrumentation
        // feature needed — this is inside the in-crate mod tests block.
        // GeometryHandle(0) is the standard synthetic handle for unit tests (cache.rs:102-107).
        cache.put(
            r_node.clone(),
            NodeCache::new(
                CachedResult::GeometryHandle(GeometryHandleId(0)),
                Freshness::Intermediate { generation: 1 },
                DependencyTrace {
                    reads: vec![a.clone()],
                },
                VersionId(1),
            ),
        );

        // Transition R to Failed via the canonical writer.
        let r_error = ErrorRef::new("synthetic");
        assert!(
            cache.mark_failed(&r_node, r_error.clone()),
            "mark_failed must succeed — R is in the cache"
        );

        // Sanity: R is Failed and has no pending_cause (mark_failed clears it,
        // per cache.rs:561).
        assert_eq!(
            cache.freshness(&r_node),
            Freshness::Failed {
                error: r_error.clone()
            },
            "sanity: R must be Failed before the walk"
        );
        assert_eq!(
            cache.pending_cause(&r_node),
            None,
            "sanity: mark_failed clears pending_cause (cache.rs:561)"
        );

        // Wire the reverse dependency: a → Realization(R).
        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), r_node.clone());

        // Flip a to Final — the edge that triggers the walk.
        assert!(cache.set_freshness(&NodeId::Value(a.clone()), Freshness::Final));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        // (i) R's freshness must STILL be Failed — the walk must skip it as a
        //     write target (re-derivation would silently flip Failed → Final,
        //     destroying the chain-root invariant; freshness_walk.rs:130-135).
        assert_eq!(
            cache.freshness(&r_node),
            Freshness::Failed { error: r_error },
            "R must STILL be Failed — walk must not re-derive a Failed Realization"
        );

        // (ii) R's pending_cause must remain None — Failed nodes are chain roots,
        //      never forwarders (arch §9.2 + mark_failed contract).
        assert_eq!(
            cache.pending_cause(&r_node),
            None,
            "R's pending_cause must remain None — Failed is a chain root, not a forwarder"
        );

        // (iii) R must NOT appear in the updated set — Realizations are not write
        //       targets in the freshness-only walk (only Value variants are pushed
        //       onto the frontier at freshness_walk.rs:131-133).
        assert!(
            !updated.contains(&r_node),
            "R must NOT be in the updated set — it is not a write target, got: {:?}",
            updated
        );

        // (iv) Walk terminates without panic (implicit: assertions above are reached).
    }

    /// U2 — True-diamond convergence: BFS from `a` in the diamond topology
    /// `a→b`, `a→c`, `b→d`, `c→d` must propagate Final to all of b, c, d,
    /// and the `updated` set must contain exactly those three nodes (d appears
    /// at most once — no double-write despite two convergence paths).
    ///
    /// The early-cutoff at `freshness_walk.rs:171` is the load-bearing guard
    /// in this all-Intermediate scenario: when `d` is processed as a dependent
    /// of `c` (the second convergence path), its freshness is already Final
    /// (written on the `b→d` path), so `new == current == Final` fires the
    /// cutoff and `d` is NOT pushed again. The `visited` guard at lines 103/108
    /// is belt-and-suspenders (d is never pushed a second time in this topology
    /// because the cutoff prevents the second `frontier.push_back`). The
    /// `updated.len() == 3` assertion pins "d appears at most once" regardless
    /// of which guard is load-bearing.
    ///
    /// Topology (all four cells seeded `Intermediate { generation: 1 }`):
    /// - `a.reads = []`, `b.reads = [a]`, `c.reads = [a]`, `d.reads = [b, c]`
    /// - `reverse_index`: `a → {Value(b), Value(c)}`, `b → {Value(d)}`,
    ///   `c → {Value(d)}`
    ///
    /// Walk: flip `a` to `Final` →
    /// `propagate_freshness_only(&mut cache, &reverse_index, &{a}, 1)`.
    #[test]
    fn true_diamond_convergence_visits_d_at_most_once() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let c = ValueCellId::new(e, "c");
        let d = ValueCellId::new(e, "d");

        let mut cache = CacheStore::new();
        put_value_entry(
            &mut cache,
            &a,
            Freshness::Intermediate { generation: 1 },
            vec![],
        );
        put_value_entry(
            &mut cache,
            &b,
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
        );
        put_value_entry(
            &mut cache,
            &c,
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
        );
        put_value_entry(
            &mut cache,
            &d,
            Freshness::Intermediate { generation: 1 },
            vec![b.clone(), c.clone()],
        );

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));
        reverse_index.add(a.clone(), NodeId::Value(c.clone()));
        reverse_index.add(b.clone(), NodeId::Value(d.clone()));
        reverse_index.add(c.clone(), NodeId::Value(d.clone()));

        // Flip a to Final — both b and c will derive Final, then d derives Final.
        assert!(cache.set_freshness(&NodeId::Value(a.clone()), Freshness::Final));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        // (i) b, c, d must all be Final after the walk.
        assert_eq!(
            cache.freshness(&NodeId::Value(b.clone())),
            Freshness::Final,
            "b must be Final (a=Final → b derives Final)"
        );
        assert_eq!(
            cache.freshness(&NodeId::Value(c.clone())),
            Freshness::Final,
            "c must be Final (a=Final → c derives Final)"
        );
        assert_eq!(
            cache.freshness(&NodeId::Value(d.clone())),
            Freshness::Final,
            "d must be Final (b=Final and c=Final → d derives Final)"
        );

        // (ii) Updated must contain exactly b, c, d — d appears at most once
        //      despite two convergence paths (b→d and c→d).
        assert_eq!(
            updated.len(),
            3,
            "updated must have exactly 3 entries (b, c, d — not 4 from double-counting d), \
             got: {:?}",
            updated
        );
        assert!(
            updated.contains(&NodeId::Value(b.clone())),
            "updated must contain Value(b), got: {:?}",
            updated
        );
        assert!(
            updated.contains(&NodeId::Value(c.clone())),
            "updated must contain Value(c), got: {:?}",
            updated
        );
        assert!(
            updated.contains(&NodeId::Value(d.clone())),
            "updated must contain Value(d), got: {:?}",
            updated
        );

        // (iii) Walk terminates (implicit: assertions above are reached).
    }

    /// P3.3 step-15: edge #6 → edge #12 freshness propagation through a
    /// ComputeNode and onto its declared `output_value_cells`.
    ///
    /// Topology:
    /// - `a: ValueCellId` seeded `Intermediate { generation: 1 }`, reads=[].
    /// - `C: ComputeNodeId` cache-seeded with `dependency_trace.reads=[a]`
    ///   and `Freshness::Intermediate { generation: 1 }`. Its
    ///   `ComputeNodeData` is inserted into `graph.compute_nodes` with
    ///   `value_inputs=[a]` and `output_value_cells=[b]`.
    /// - `b: ValueCellId` seeded `Intermediate { generation: 1 }`, reads=[]
    ///   (so its standalone freshness derivation yields Final from zero
    ///   inputs per §7.2). b is in `graph.value_cells` so the impl
    ///   can ask the helper to re-derive b after C flips.
    /// - Reverse index built from `graph` registers `a → Compute(C)` via
    ///   the step-4 loop.
    ///
    /// Walk: flip `a` to `Final`, then call
    /// `propagate_freshness_only(&mut cache, &reverse_index, &graph, &{a}, 1)`.
    ///
    /// Assertions:
    /// (i) `Compute(C)` in `updated`, its freshness is `Final` — pins
    ///     edge #6 propagation into a `NodeId::Compute(_)` dependent (the
    ///     existing per-dependent derivation already handles this once the
    ///     graph parameter threads through).
    /// (ii) `Value(b)` in `updated`, its freshness is `Final` — pins
    ///     edge #12 propagation onto C's `output_value_cells`: when C's
    ///     freshness changes, each output VC must be re-derived and
    ///     written through the canonical writers exactly like a regular
    ///     dependent would be.
    ///
    /// Fails today because (a) `propagate_freshness_only` does not yet
    /// take a `graph` parameter and (b) the per-dependent block does not
    /// fan out onto C's `output_value_cells`.
    #[test]
    fn propagate_freshness_only_propagates_through_compute_node_to_output_value_cells() {
        use crate::graph::{ComputeNodeData, EvaluationGraph, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, Type};

        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        // Build the EvaluationGraph: a, b as Param ValueCells, C as a
        // ComputeNode with value_inputs=[a] and output_value_cells=[b].
        let mut graph = EvaluationGraph::default();
        for name in &["a", "b"] {
            let id = ValueCellId::new(e, *name);
            graph.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Real,
                    default_expr: None,
                    content_hash: ContentHash::of_str(name),
                },
            );
        }
        let c_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![a.clone()],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });

        // Seed cache entries.
        let mut cache = CacheStore::new();
        put_value_entry(
            &mut cache,
            &a,
            Freshness::Intermediate { generation: 1 },
            vec![],
        );
        put_value_entry(
            &mut cache,
            &b,
            Freshness::Intermediate { generation: 1 },
            vec![],
        );
        // Seed Compute(C)'s cache entry with reads=[a]. The CachedResult
        // payload mirrors the cold-start synthesis pattern in
        // insert_synthetic_realization_entry (cache.rs:987-1009): a
        // placeholder Value entry whose specific bits the walk never reads
        // — only the freshness side-table and dependency_trace matter.
        let c_node = NodeId::Compute(c_id.clone());
        cache.put(
            c_node.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Real(0.0), DeterminacyState::Determined),
                Freshness::Intermediate { generation: 1 },
                DependencyTrace {
                    reads: vec![a.clone()],
                },
                VersionId(1),
            ),
        );

        // Build the reverse index from the graph — this registers
        // `a → Compute(C)` via the step-4 loop over graph.compute_nodes.
        let reverse_index = ReverseDependencyIndex::build_from_graph(&graph);

        // Flip a to Final — the edge that triggers the walk.
        assert!(cache.set_freshness(&NodeId::Value(a.clone()), Freshness::Final));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let updated =
            super::propagate_freshness_only(&mut cache, &reverse_index, &graph, &changed, 1);

        // (i) Compute(C) reached via edge #6, freshness derived to Final
        //     from reads=[a] (a is Final).
        assert!(
            updated.contains(&c_node),
            "updated must contain Compute(C) via edge #6 (a → C), got: {:?}",
            updated
        );
        assert_eq!(
            cache.freshness(&c_node),
            Freshness::Final,
            "Compute(C)'s freshness must be Final after a flips Final"
        );

        // (ii) Value(b) reached via edge #12 (C's output_value_cells),
        //      freshness derived to Final from b's empty reads.
        let b_node = NodeId::Value(b.clone());
        assert!(
            updated.contains(&b_node),
            "updated must contain Value(b) via edge #12 (C → b), got: {:?}",
            updated
        );
        assert_eq!(
            cache.freshness(&b_node),
            Freshness::Final,
            "Value(b)'s freshness must be Final after edge #12 propagation from C"
        );
    }

    /// task δ / 3423 (PRD §3 chain-root contract extension, §8 task δ):
    /// regression-pin that the in-flight ComputeNode begin-dispatch state
    /// (output VC `Pending` with `pending_cause = Some(NodeId::Compute(C))`)
    /// forwards the Compute chain root through `propagate_freshness_only`
    /// onto a downstream cell, variant-agnostically via the §9.2 helper.
    ///
    /// Setup mirrors `failed_upstream_propagates_pending_with_cause`, except
    /// the chain root is `NodeId::Compute(c_id)` — the
    /// `begin_compute_dispatch` in-flight state set by
    /// `mark_pending_with_cause` — instead of a Failed `NodeId::Value`. Pins
    /// task 3420's chain-root extension end-to-end through the walk for the δ
    /// use case. Expected to pass on the existing impl (cause forwarding is
    /// variant-agnostic), so this is a regression-pin, not a behaviour
    /// change. Companion to `cache::tests`'
    /// `cache_store_pending_cause_admits_compute_chain_root` /
    /// `derive_output_freshness_with_cause_forwards_compute_chain_root`.
    #[test]
    fn propagate_freshness_only_forwards_compute_chain_root_through_pending_output_to_downstream() {
        use reify_core::ComputeNodeId;

        let e = "T";
        let b = ValueCellId::new(e, "b");
        let d = ValueCellId::new(e, "d");
        let c_id = ComputeNodeId::new(e, 0);

        let mut cache = CacheStore::new();
        // (a) Output VC `b` with a concrete payload, then transitioned to the
        //     in-flight begin_compute_dispatch state: Pending with
        //     last_substantive captured from b's prior result_hash and
        //     pending_cause = Some(NodeId::Compute(c_id)).
        put_value_entry_with_payload(
            &mut cache,
            &b,
            Value::Int(42),
            Freshness::Final,
            vec![],
            VersionId(1),
        );
        assert!(
            cache.mark_pending_with_cause(
                &NodeId::Value(b.clone()),
                NodeId::Compute(c_id.clone()),
            ),
            "mark_pending_with_cause must succeed on the seeded output VC \
             (this is the in-flight begin_compute_dispatch state)"
        );

        // (b) Downstream cell `d`: Intermediate, reads=[b], concrete payload
        //     so its result_hash is non-trivial for the last_substantive check.
        put_value_entry_with_payload(
            &mut cache,
            &d,
            Value::Int(7),
            Freshness::Intermediate { generation: 1 },
            vec![b.clone()],
            VersionId(1),
        );
        let d_node = NodeId::Value(d.clone());
        let d_prev_hash: ContentHash = cache.get(&d_node).expect("d cached").result_hash;

        // (c) reverse_index: b → Value(d).
        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(b.clone(), NodeId::Value(d.clone()));

        let mut changed = HashSet::new();
        changed.insert(b.clone());

        let updated = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            &changed,
            1,
        );

        // (i) d was updated by the walk.
        assert!(
            updated.contains(&d_node),
            "updated must contain Value(d), got: {:?}",
            updated
        );
        // (ii) d is now Pending with last_substantive = its prior result_hash
        //      (the canonical Pending writer captures the current cached hash).
        assert_eq!(
            cache.freshness(&d_node),
            Freshness::Pending {
                last_substantive: ResultRef::of_hash(d_prev_hash),
            },
            "d must be Pending with last_substantive set to its prior result_hash"
        );
        // (iii) The Compute chain root forwarded through the walk
        //       variant-agnostically (task 3420 extension, δ use case).
        assert_eq!(
            cache.pending_cause(&d_node),
            Some(NodeId::Compute(c_id.clone())),
            "d's pending_cause must forward the in-flight ComputeNode chain \
             root Some(NodeId::Compute(c_id))"
        );
    }

    /// Step-1 (task 3649): `propagate_freshness_only` accepts any `IntoIterator`
    /// with `Item = &ValueCellId`, not only `&HashSet<ValueCellId>`.
    ///
    /// The unique coverage here is compile-time: `[a.clone()].iter()` (a slice
    /// iterator) must type-check against the widened signature. Behavioral
    /// correctness (Final propagation, updated-set membership) is already covered
    /// by `propagates_intermediate_to_final_through_two_cell_chain`; no
    /// assertions are repeated here.
    ///
    /// RED: does NOT compile against `changed: &HashSet<ValueCellId>` — a
    /// slice iterator is not a `&HashSet`. GREEN once the signature is widened
    /// to `impl IntoIterator<Item = &ValueCellId>`.
    #[test]
    fn propagate_freshness_only_accepts_borrowed_iterator() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        let mut cache = CacheStore::new();
        put_value_entry(
            &mut cache,
            &a,
            Freshness::Intermediate { generation: 1 },
            vec![],
        );
        put_value_entry(
            &mut cache,
            &b,
            Freshness::Intermediate { generation: 1 },
            vec![a.clone()],
        );

        let mut reverse_index = ReverseDependencyIndex::new();
        reverse_index.add(a.clone(), NodeId::Value(b.clone()));

        assert!(cache.set_freshness(&NodeId::Value(a.clone()), Freshness::Final));

        // Compile-time check: a slice iterator type-checks against the widened
        // `impl IntoIterator<Item = &ValueCellId>` signature.
        let _ = super::propagate_freshness_only(
            &mut cache,
            &reverse_index,
            &EvaluationGraph::default(),
            [a.clone()].iter(),
            1,
        );
    }
}
