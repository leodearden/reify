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

            if new == current {
                // Freshness early cutoff: nothing changed at this node, so
                // we do not propagate further along this branch.
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
        DeterminacyState, Freshness, Type, Value, ValueCellId, VersionId,
    };
    use std::collections::HashSet;

    /// Helper: insert a Value-cell cache entry with the given freshness and reads.
    fn put_value_entry(
        cache: &mut CacheStore,
        cell: &ValueCellId,
        freshness: Freshness,
        reads: Vec<ValueCellId>,
    ) {
        let _ = Type::Real;
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
}
