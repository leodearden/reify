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
//! prunes the walk's frontier.
//!
//! Public API populated by subsequent task #2335 steps.

use std::collections::{HashSet, VecDeque};

use crate::cache::{CacheStore, NodeId};
use crate::deps::ReverseDependencyIndex;
use reify_types::ValueCellId;

/// Propagate freshness forward through the dependents of `changed` cells
/// without recomputing any value, per arch §3.5 lines 432-436.
///
/// BFS forward walk from each `ValueCellId` in `changed`: for every dependent
/// found via [`ReverseDependencyIndex::dependents_of`], re-derive the
/// dependent's freshness from its cached `dependency_trace.reads` using
/// [`CacheStore::derive_output_freshness_for_node`] and write it back via
/// [`CacheStore::set_freshness`] when it differs from the dependent's current
/// freshness. When the new freshness equals the current one, the *freshness
/// early cutoff* fires at that node and propagation stops along that branch.
///
/// Returns the set of [`NodeId`]s whose freshness was actually updated; nodes
/// pruned by early cutoff (or with no cache entry) are not included.
///
/// # Touch-list
///
/// **Touches:** `freshness`, and (transitively, via `mark_pending_with_cause`
/// once routed in step-10) `pending_cause`.
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
///
/// # Scope of writes
///
/// Subsequent task #2335 steps refine the write site to use the cause-bearing
/// helper `derive_output_freshness_for_node_with_cause` and route Pending
/// outputs through `mark_pending_with_cause` / `mark_pending`. The current
/// implementation handles the Intermediate ↔ Final transitions only — Pending
/// chain forwarding and Failed-skip semantics are added in later steps.
pub fn propagate_freshness_only(
    cache: &mut CacheStore,
    reverse_index: &ReverseDependencyIndex,
    changed: &HashSet<ValueCellId>,
    generation: u64,
) -> HashSet<NodeId> {
    let mut updated: HashSet<NodeId> = HashSet::new();
    let mut frontier: VecDeque<ValueCellId> = changed.iter().cloned().collect();
    let mut visited: HashSet<ValueCellId> = HashSet::new();

    while let Some(cell) = frontier.pop_front() {
        // Visited-cells guard: a cell can be enqueued multiple times by
        // different upstream branches; skip its dependents on the second
        // visit so we never re-process the same cell. This also ensures
        // step-3's three-cell chain a→b→c terminates correctly under
        // diamond shapes (where a downstream cell may be reached from
        // multiple upstreams).
        if !visited.insert(cell.clone()) {
            continue;
        }

        // Snapshot the dependent NodeIds before mutating the cache so the
        // borrow on `reverse_index` is released before we call
        // `cache.derive_*` / `cache.set_freshness`.
        let dependents: Vec<NodeId> = reverse_index.dependents_of(&cell).iter().cloned().collect();

        for dependent in dependents {
            let current = cache.freshness(&dependent);
            let new = cache.derive_output_freshness_for_node(&dependent, false, generation);

            // Freshness early cutoff (arch §3.5 lines 432-436): if the
            // newly-derived freshness equals the current freshness the walk
            // halts at this node and does NOT propagate to its dependents.
            //
            // The strict `==` comparison is correct because `Freshness`
            // derives `PartialEq`, so a single `generation` parameter is
            // threaded through the whole walk: an Intermediate{1} input that
            // produces an Intermediate{1} output cuts off, but an
            // Intermediate{1} → Intermediate{2} change would not (yielding a
            // legitimate generation-bumping transition that *should*
            // propagate). DO NOT WEAKEN this comparison without re-deriving
            // the §7.2/§9.2 truth table — `Freshness::Pending`'s
            // `last_substantive` field is part of the equality and matters
            // for diagnostic-chain correctness.
            if new == current {
                continue;
            }

            if cache.set_freshness(&dependent, new.clone()) {
                updated.insert(dependent.clone());
                if let NodeId::Value(vcid) = &dependent {
                    frontier.push_back(vcid.clone());
                }
            }
            // If `set_freshness` returns false the entry is absent — nothing
            // to write and nothing to propagate from a non-cached node.
        }
    }

    updated
}

#[cfg(test)]
mod tests {
    use crate::cache::{CacheStore, CachedResult, NodeCache, NodeId};
    use crate::deps::{DependencyTrace, ReverseDependencyIndex};
    use reify_types::{
        DeterminacyState, Freshness, Value, ValueCellId, VersionId,
    };
    use std::collections::HashSet;

    /// Helper: insert a Value-cell cache entry with the given freshness and reads.
    fn put_value_entry(
        cache: &mut CacheStore,
        cell: &ValueCellId,
        freshness: Freshness,
        reads: Vec<ValueCellId>,
    ) {
        cache.put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Real(0.0), DeterminacyState::Determined),
                freshness,
                DependencyTrace { reads },
                VersionId(1),
            ),
        );
    }

    /// Helper: insert a Value-cell entry with the given concrete `Value`,
    /// freshness, reads, and basis_version. Used by step-7's snapshot test.
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
        put_value_entry(&mut cache, &a, Freshness::Intermediate { generation: 1 }, vec![]);
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

        let updated =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);

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
        put_value_entry(&mut cache, &a, Freshness::Intermediate { generation: 1 }, vec![]);
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

        let updated =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);

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
        put_value_entry(&mut cache, &a, Freshness::Intermediate { generation: 1 }, vec![]);
        put_value_entry(&mut cache, &c, Freshness::Intermediate { generation: 1 }, vec![]);
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

        let updated =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);

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

        let updated =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);
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
}
