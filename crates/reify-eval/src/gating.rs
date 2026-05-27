//! Cache-aware gating helpers for the `OnlyRunOnFinalInputs` scheduling policy.
//!
//! Provides stateless helpers that classify whether a candidate node's
//! dependency inputs are all-Final, per arch §7.3 lines 762–767 and §3.5
//! line 436 ("freshness propagation can unlock gated work").
//!
//! ## Layering
//!
//! This module lives in `reify-eval` and is intentionally policy-agnostic: it
//! accepts a pre-built candidate gated set and does NOT import
//! `NodePolicyOverrides` from `reify-runtime`.  The dependency arrow is
//! `reify-runtime → reify-eval → reify-types`, so importing from `reify-runtime`
//! would introduce a cycle.  The runtime layer computes the candidate gated set
//! via `NodePolicyOverrides::resolve()` and feeds it to these helpers.

use std::collections::HashSet;

use crate::cache::{CacheStore, NodeCache, NodeId};
use reify_core::ValueCellId;

/// Inner predicate: returns `true` iff any input listed in `entry`'s
/// `dependency_trace.reads` is non-Final in `cache`.
///
/// This is the shared kernel called by both [`has_non_final_inputs`] and
/// [`unblocked_gated_nodes`] so each performs only a single `cache.get` per
/// node.
fn entry_has_non_final_inputs(cache: &CacheStore, entry: &NodeCache) -> bool {
    entry
        .dependency_trace
        .reads
        .iter()
        .any(|read: &ValueCellId| !cache.freshness(&NodeId::Value(read.clone())).is_final())
}

/// Returns `true` iff any of `node`'s recorded `dependency_trace.reads`
/// input cells is non-Final in `cache`.
///
/// "Non-Final" means any of `Freshness::Intermediate`, `Pending`, or `Failed`.
/// The name reflects the actual predicate: `!is_final()` on every input.
///
/// A non-Final input means the cached value is not yet authoritative, so a
/// node with `OnlyRunOnFinalInputs` policy must not be scheduled yet.
///
/// ## Absence semantics
///
/// - If `node` has **no cache entry**: returns `false` (vacuously runnable).
///   This matches [`CacheStore::freshness`]'s default-to-Final-on-absent
///   contract (`cache.rs:611–620`) and avoids spurious skips for cold-start
///   scenarios where the node has never been evaluated.
/// - If `node` has a cache entry but **empty `dependency_trace.reads`**
///   (param-like node with no upstream inputs): returns `false` — there are
///   no inputs to gate on.
///
/// ## Freshness check
///
/// Each input is looked up via `cache.freshness(&NodeId::Value(read))`.
/// Any non-`is_final()` result (Intermediate, Pending, or Failed) causes this
/// function to return `true`.  Failed inputs are treated as non-Final for
/// robustness: in practice a gated node downstream of a Failed cell will see
/// Pending (via the §9.2 chain), but treating Failed as blocking is
/// consistent with the "Final is the only safe-to-run state" principle and
/// keeps this helper aligned with the scheduler's single `bool` predicate
/// (see `reify-runtime/src/concurrent.rs`).
///
/// The canonical end-to-end witness is
/// `crates/reify-eval/tests/only_run_on_final_inputs_gating.rs`.
///
/// See arch §7.3 lines 762–767 and §3.5 line 436.
pub fn has_non_final_inputs(cache: &CacheStore, node: &NodeId) -> bool {
    match cache.get(node) {
        Some(entry) => entry_has_non_final_inputs(cache, entry),
        None => false,
    }
}

/// Returns the subset of `gated` nodes whose dependency inputs are all-Final —
/// i.e., nodes that the freshness walk has newly unblocked.
///
/// A gated node is included in the result iff **both** conditions hold:
/// 1. It has a cache entry (it has been evaluated at least once).
/// 2. [`has_non_final_inputs`]`(cache, node)` returns `false` (all inputs are Final).
///
/// The "must have a cache entry" requirement distinguishes "newly unblocked
/// because the freshness walk transitioned its inputs" from the cold-start
/// case where the node simply has never been evaluated.  See arch §3.5 line
/// 436.
///
/// Each candidate performs exactly one `cache.get` lookup (via the shared
/// [`entry_has_non_final_inputs`] kernel), avoiding the redundant double-lookup
/// that would occur from calling `cache.get(node).is_some()` followed by a
/// separate `has_non_final_inputs(cache, node)`.
///
/// # Type parameters
///
/// `I` is any iterator over `&NodeId` references — callers can pass a slice,
/// a `Vec`, a `HashSet`, etc.
///
/// The canonical end-to-end witness is
/// `crates/reify-eval/tests/only_run_on_final_inputs_gating.rs`.
///
/// See arch §3.5 line 436 ("freshness propagation can unlock gated work").
pub fn unblocked_gated_nodes<'a, I>(cache: &CacheStore, gated: I) -> HashSet<NodeId>
where
    I: IntoIterator<Item = &'a NodeId>,
{
    gated
        .into_iter()
        .filter_map(|node| {
            cache
                .get(node)
                .filter(|entry| !entry_has_non_final_inputs(cache, entry))
                .map(|_| node.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{CachedResult, NodeCache};
    use crate::deps::DependencyTrace;
    use reify_core::{ValueCellId, VersionId};
    use reify_ir::{DeterminacyState, ErrorRef, Freshness, ResultRef, Value};

    // ── helpers ───────────────────────────────────────────────────────────────

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

    // ── has_non_final_inputs unit tests (step-3) ───────────────────────────

    /// (a) All inputs Final → false.
    #[test]
    fn has_non_final_inputs_all_final_returns_false() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        let mut cache = CacheStore::new();
        put_value_entry(&mut cache, &a, Freshness::Final, vec![]);
        put_value_entry(&mut cache, &b, Freshness::Final, vec![a.clone()]);

        assert!(
            !has_non_final_inputs(&cache, &NodeId::Value(b.clone())),
            "all-Final inputs must return false"
        );
    }

    /// (b) One Intermediate input → true.
    #[test]
    fn has_non_final_inputs_one_intermediate_returns_true() {
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
        put_value_entry(&mut cache, &b, Freshness::Final, vec![a.clone()]);

        assert!(
            has_non_final_inputs(&cache, &NodeId::Value(b.clone())),
            "one Intermediate input must return true"
        );
    }

    /// (c) One Pending input → true.
    #[test]
    fn has_non_final_inputs_one_pending_returns_true() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        let mut cache = CacheStore::new();
        put_value_entry(
            &mut cache,
            &a,
            Freshness::Pending {
                last_substantive: ResultRef::none(),
            },
            vec![],
        );
        put_value_entry(&mut cache, &b, Freshness::Final, vec![a.clone()]);

        assert!(
            has_non_final_inputs(&cache, &NodeId::Value(b.clone())),
            "one Pending input must return true"
        );
    }

    /// (d) One Failed input → true.
    #[test]
    fn has_non_final_inputs_one_failed_returns_true() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        let mut cache = CacheStore::new();
        put_value_entry(
            &mut cache,
            &a,
            Freshness::Failed {
                error: ErrorRef::new("boom"),
            },
            vec![],
        );
        put_value_entry(&mut cache, &b, Freshness::Final, vec![a.clone()]);

        assert!(
            has_non_final_inputs(&cache, &NodeId::Value(b.clone())),
            "one Failed input must return true (Failed is non-Final)"
        );
    }

    /// (e) Node has no cache entry → false (vacuously runnable).
    #[test]
    fn has_non_final_inputs_node_absent_returns_false() {
        let e = "T";
        let b = ValueCellId::new(e, "b");
        let cache = CacheStore::new();

        assert!(
            !has_non_final_inputs(&cache, &NodeId::Value(b.clone())),
            "absent node must return false (vacuously runnable)"
        );
    }

    /// (f) Node has cache entry but empty dependency_trace.reads → false
    ///     (param-like node with no upstream inputs).
    #[test]
    fn has_non_final_inputs_empty_reads_returns_false() {
        let e = "T";
        let a = ValueCellId::new(e, "a");

        let mut cache = CacheStore::new();
        put_value_entry(&mut cache, &a, Freshness::Final, vec![]);

        assert!(
            !has_non_final_inputs(&cache, &NodeId::Value(a.clone())),
            "empty reads (param-like) must return false"
        );
    }

    // ── unblocked_gated_nodes unit tests (step-5) ─────────────────────────────

    /// (a) All gated nodes have all-Final inputs → returned set equals input set.
    #[test]
    fn unblocked_gated_nodes_all_final_returns_all() {
        let e = "T";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let c = ValueCellId::new(e, "c");

        let mut cache = CacheStore::new();
        put_value_entry(&mut cache, &a, Freshness::Final, vec![]);
        put_value_entry(&mut cache, &b, Freshness::Final, vec![a.clone()]);
        put_value_entry(&mut cache, &c, Freshness::Final, vec![a.clone()]);

        let gated = vec![NodeId::Value(b.clone()), NodeId::Value(c.clone())];
        let result = unblocked_gated_nodes(&cache, &gated);

        assert_eq!(
            result,
            gated.iter().cloned().collect::<HashSet<_>>(),
            "all-Final gated nodes must all be returned"
        );
    }

    /// (b) All gated nodes have one Intermediate input → returned set is empty.
    #[test]
    fn unblocked_gated_nodes_intermediate_input_returns_empty() {
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
        put_value_entry(&mut cache, &b, Freshness::Final, vec![a.clone()]);

        let gated = vec![NodeId::Value(b.clone())];
        let result = unblocked_gated_nodes(&cache, &gated);

        assert!(
            result.is_empty(),
            "Intermediate input must block all gated nodes; got: {:?}",
            result
        );
    }

    /// (c) Mixed: 3 gated nodes where 2 have all-Final inputs and 1 has
    ///     Pending input → returned set is exactly the 2 all-Final nodes.
    #[test]
    fn unblocked_gated_nodes_mixed_returns_only_all_final() {
        let e = "T";
        let a_final = ValueCellId::new(e, "a_final");
        let a_pending = ValueCellId::new(e, "a_pending");
        let b1 = ValueCellId::new(e, "b1");
        let b2 = ValueCellId::new(e, "b2");
        let b3 = ValueCellId::new(e, "b3");

        let mut cache = CacheStore::new();
        put_value_entry(&mut cache, &a_final, Freshness::Final, vec![]);
        put_value_entry(
            &mut cache,
            &a_pending,
            Freshness::Pending {
                last_substantive: ResultRef::none(),
            },
            vec![],
        );
        put_value_entry(&mut cache, &b1, Freshness::Final, vec![a_final.clone()]);
        put_value_entry(&mut cache, &b2, Freshness::Final, vec![a_final.clone()]);
        put_value_entry(&mut cache, &b3, Freshness::Final, vec![a_pending.clone()]);

        let gated = vec![
            NodeId::Value(b1.clone()),
            NodeId::Value(b2.clone()),
            NodeId::Value(b3.clone()),
        ];
        let result = unblocked_gated_nodes(&cache, &gated);

        let expected: HashSet<_> = [NodeId::Value(b1.clone()), NodeId::Value(b2.clone())]
            .into_iter()
            .collect();
        assert_eq!(
            result, expected,
            "only nodes with all-Final inputs must be returned"
        );
    }

    /// (d) Empty `gated` iterator → empty set.
    #[test]
    fn unblocked_gated_nodes_empty_iterator_returns_empty() {
        let cache = CacheStore::new();
        let result = unblocked_gated_nodes(&cache, &Vec::<NodeId>::new());
        assert!(
            result.is_empty(),
            "empty gated set must produce empty result"
        );
    }

    /// (e) Gated node not in cache → excluded from result (not "newly unblocked").
    ///
    /// The contract surface: a node absent from the cache has never been
    /// evaluated; it is "vacuously runnable" per `has_non_final_inputs` but
    /// NOT "newly unblocked by the freshness walk", so it must NOT appear in
    /// `unblocked_gated_nodes`.
    #[test]
    fn unblocked_gated_nodes_absent_node_excluded() {
        let e = "T";
        let b = ValueCellId::new(e, "b");
        let cache = CacheStore::new();

        let gated = vec![NodeId::Value(b.clone())];
        let result = unblocked_gated_nodes(&cache, &gated);

        assert!(
            result.is_empty(),
            "absent (never-evaluated) node must be excluded from unblocked set; got: {:?}",
            result
        );
    }

    // ── gating_composes_with_freshness_walk_in_isolation (step-7) ────────────

    /// Pins the cross-module composition of `has_non_final_inputs` /
    /// `unblocked_gated_nodes` with `propagate_freshness_only` without
    /// needing the full Engine fixture.
    ///
    /// Three-cell synthetic chain: `a` (Intermediate{1}), `c` (Intermediate{1}),
    /// `b` (Intermediate{1}, reads=[a, c]).
    ///
    /// (i) Before any walk: `has_non_final_inputs(b)` = true,
    ///     `unblocked_gated_nodes([b])` = empty.
    /// (ii) Flip `a` → Final; walk from {a}; b is still Intermediate (c blocks).
    /// (iii) Flip `c` → Final; walk from {c}; b becomes Final,
    ///       `has_non_final_inputs(b)` = false,
    ///       `unblocked_gated_nodes([b])` = {b}.
    #[test]
    fn gating_composes_with_freshness_walk_in_isolation() {
        use crate::deps::ReverseDependencyIndex;
        use crate::freshness_walk;
        use crate::graph::EvaluationGraph;

        let graph = EvaluationGraph::default();

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

        let mut index = ReverseDependencyIndex::new();
        index.add(a.clone(), NodeId::Value(b.clone()));
        index.add(c.clone(), NodeId::Value(b.clone()));

        let b_node = NodeId::Value(b.clone());
        let gated = vec![b_node.clone()];

        // (i) Before any walk.
        assert!(
            has_non_final_inputs(&cache, &b_node),
            "(i) b has Intermediate inputs before any walk"
        );
        assert!(
            unblocked_gated_nodes(&cache, &gated).is_empty(),
            "(i) b must not be unblocked before walk"
        );

        // (ii) Flip `a` → Final; walk from {a}.
        assert!(cache.set_freshness(&NodeId::Value(a.clone()), Freshness::Final));
        let mut changed = HashSet::new();
        changed.insert(a.clone());
        freshness_walk::propagate_freshness_only(&mut cache, &index, &graph, &changed, 1);

        // b is still Intermediate because c is still Intermediate.
        assert_eq!(
            cache.freshness(&b_node),
            Freshness::Intermediate { generation: 1 },
            "(ii) b must still be Intermediate (c still blocks)"
        );
        assert!(
            has_non_final_inputs(&cache, &b_node),
            "(ii) b still has intermediate input (c)"
        );
        assert!(
            unblocked_gated_nodes(&cache, &gated).is_empty(),
            "(ii) b must not be unblocked yet"
        );

        // (iii) Flip `c` → Final; walk from {c}.
        assert!(cache.set_freshness(&NodeId::Value(c.clone()), Freshness::Final));
        let mut changed2 = HashSet::new();
        changed2.insert(c.clone());
        let updated =
            freshness_walk::propagate_freshness_only(&mut cache, &index, &graph, &changed2, 1);

        assert!(
            updated.contains(&b_node),
            "(iii) walk must report b as updated; got: {:?}",
            updated
        );
        assert_eq!(
            cache.freshness(&b_node),
            Freshness::Final,
            "(iii) b must be Final after both inputs are Final"
        );
        assert!(
            !has_non_final_inputs(&cache, &b_node),
            "(iii) b must have no intermediate inputs"
        );
        let unblocked = unblocked_gated_nodes(&cache, &gated);
        assert_eq!(
            unblocked,
            std::iter::once(b_node.clone()).collect::<HashSet<_>>(),
            "(iii) unblocked_gated_nodes must return {{b}}"
        );
    }
}
