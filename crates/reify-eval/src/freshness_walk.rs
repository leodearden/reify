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
}
