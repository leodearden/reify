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
use reify_types::{Freshness, ValueCellId};

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
/// Returns the set of [`NodeId`]s whose freshness was actually updated; nodes
/// pruned by early cutoff (or with no cache entry) are not included.
///
/// # Touch-list
///
/// **Touches:** `freshness`, and (transitively, via `mark_pending_with_cause`)
/// `pending_cause`.
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
pub fn propagate_freshness_only(
    cache: &mut CacheStore,
    reverse_index: &ReverseDependencyIndex,
    changed: &HashSet<ValueCellId>,
    generation: u64,
) -> HashSet<NodeId> {
    let mut updated: HashSet<NodeId> = HashSet::new();
    let mut frontier: VecDeque<ValueCellId> = changed.iter().cloned().collect();
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
            let current = cache.freshness(&dependent);

            // Failed-skip guard (cache.rs:545-566 mark_failed contract;
            // arch §9.2 lines 880-890): a Failed node is a terminal
            // chain-root state. Re-deriving it via the §7.2/§9.2 helper
            // would silently flip it to Final/Intermediate/Pending based
            // on its inputs, destroying the chain-root invariant
            // (`pending_cause` reader contract requires Failed to read
            // None). However, downstream propagation MUST continue —
            // dependents of Failed b correctly become Pending with b as
            // the chain root, so we DO push the cell onto the frontier
            // before continuing. The `continue` MUST come AFTER
            // `frontier.push_back` so that the inner derivation step at
            // c (which sees Failed b as input) still fires.
            if matches!(current, Freshness::Failed { .. }) {
                if let NodeId::Value(vcid) = &dependent {
                    frontier.push_back(vcid.clone());
                }
                continue;
            }

            // Cause-bearing variant (arch §9.2 lines 880-890): returns the
            // upstream `NodeId` chain root for a Pending output. We forward
            // `cause` through `mark_pending_with_cause` so the §9.2
            // diagnostic chain is preserved across freshness-only walks
            // (matching `evaluate_let_bindings`'s pre-eval Pending gate).
            // still_refining=false: this walk runs after the value-mode
            // refinement pass has settled, so derivation must consult actual
            // input freshnesses, not the §7.2 refinement gate that
            // short-circuits to Intermediate{generation}.
            let (new, cause) =
                cache.derive_output_freshness_for_node_with_cause(&dependent, false, generation);

            // Freshness early cutoff (arch §3.5 lines 432-436): if the
            // newly-derived freshness equals the current freshness the walk
            // halts at this node and does NOT propagate to its dependents.
            //
            // The strict `==` comparison is correct for non-Pending
            // outputs because `Freshness` derives `PartialEq`, so a single
            // `generation` parameter is threaded through the whole walk:
            // an Intermediate{1} input that produces an Intermediate{1}
            // output cuts off, but an Intermediate{1} → Intermediate{2}
            // change would not (yielding a legitimate generation-bumping
            // transition that *should* propagate). DO NOT WEAKEN this
            // comparison for Final/Intermediate without re-deriving the
            // §7.2 truth table.
            //
            // Pending requires a separate cutoff (see below) because the
            // pure helper returns `Pending { last_substantive:
            // ResultRef::none() }` while the canonical writers replace
            // `last_substantive` with `ResultRef::of_hash(entry.result_hash)`.
            // A naive `new == current` check therefore never fires for
            // already-Pending dependents, which would re-bump
            // `pending_transition_count` on every walk and break the
            // fixed-point property under repeated invocation.
            if new == current {
                continue;
            }

            // Pending idempotency cutoff: when both the derived freshness
            // and the current freshness are Pending and the diagnostic
            // chain root (`pending_cause`) matches what the writer would
            // record, the canonical Pending writers would only re-record
            // the same final state while bumping `pending_transition_count`
            // and re-cloning `pending_cause`. Short-circuit before invoking
            // the writer so the walk remains a true fixed-point operator
            // for Pending transitions, matching its behavior for the
            // Final/Intermediate cases above.
            //
            // Why compare `pending_cause` instead of `last_substantive`?
            // `last_substantive` cannot be compared against the pure
            // helper's output (`ResultRef::none()`) because the writer
            // replaces it with `ResultRef::of_hash(entry.result_hash)`.
            // `pending_cause` IS comparable: a Pending entry written by
            // `mark_pending_with_cause(c)` carries `Some(c)`, and one
            // written by `mark_pending` carries `None` — exactly the same
            // values the cause-bearing helper returns. So if the stored
            // cause matches the derived cause AND both freshnesses are
            // Pending, we know the writer's effect would be a no-op
            // modulo the counter bump.
            if matches!(new, Freshness::Pending { .. })
                && matches!(current, Freshness::Pending { .. })
                && cache.pending_cause(&dependent) == cause
            {
                continue;
            }

            // Route Pending writes through the canonical Pending writers so
            // they (a) capture `last_substantive: ResultRef::of_hash(...)`
            // from the entry's existing `result_hash`, and (b) record the
            // §9.2 chain root via `pending_cause`. Other variants
            // (Final / Intermediate) go through `set_freshness` directly.
            // Failed is never written by the walk — the cause-bearing
            // helper never returns Failed (only Final / Intermediate /
            // Pending), so the `_ =>` arm cannot in practice receive
            // Failed.
            let wrote = match &new {
                Freshness::Pending { .. } => match cause.clone() {
                    Some(c) => cache.mark_pending_with_cause(&dependent, c),
                    None => cache.mark_pending(&dependent),
                },
                _ => cache.set_freshness(&dependent, new.clone()),
            };

            if wrote {
                updated.insert(dependent.clone());
                if let NodeId::Value(vcid) = &dependent {
                    frontier.push_back(vcid.clone());
                }
            }
            // Absent-entry guard is actually the early-cutoff branch above:
            // `freshness()` returns Final by default for absent nodes
            // (cache.rs:617-621), the §7.2/§9.2 helper returns (Final, None)
            // when iterating zero inputs, so `new == current == Final` fires
            // the cutoff before any writer is invoked. This `if wrote` arm is
            // belt-and-suspenders defense in case a future code path bypasses
            // that cutoff for an absent dependent.
        }
    }

    updated
}

#[cfg(test)]
mod tests {
    use crate::cache::{CacheStore, CachedResult, NodeCache, NodeId};
    use crate::deps::{DependencyTrace, ReverseDependencyIndex};
    use reify_types::{
        ContentHash, DeterminacyState, ErrorRef, Freshness, ResultRef, Value, ValueCellId, VersionId,
    };
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

        let updated_first =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);
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
        let updated_second =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);

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
            Freshness::Failed { error: b_error.clone() },
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

        let updated =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);

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

        let updated =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);

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

        let updated_first =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);
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
        let updated_second =
            super::propagate_freshness_only(&mut cache, &reverse_index, &changed, 1);

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
}
