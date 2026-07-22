//! Task ι (PRD `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.6 / §3 P3 /
//! §7 B4): the INV-EVAL-4 snapshot↔cache content-hash divergence audit.
//!
//! `check_snapshot_cache_divergence` is a pure function over retained
//! post-eval state (`snapshot.values` + the `CacheStore` + the
//! `EventJournal`) that reports every value cell whose snapshot value
//! disagrees, by content-hash, with its present cache entry — the INV-EVAL-4
//! bug class §2.6 exists to catch. A thin `Engine::check_snapshot_cache_divergence`
//! wrapper (added in a later step) threads the Engine's own retained
//! `eval_state()`/`cache`/`journal` into this free function for the debug-gate
//! corpus harness.
//!
//! This is a FORWARD (snapshot→cache) audit: a MISSING cache entry is NOT a
//! divergence — only a present-but-mismatched, non-Skip-exempt entry is (see
//! [`check_snapshot_cache_divergence`]'s doc). It is COMPLEMENTARY to (NOT
//! merged with) `invariants::check_no_stale_undef` (INV-EVAL-5, task 4952):
//! a distinct invariant from a distinct PRD, kept in its own module for clean
//! provenance.
//!
//! Lives in-crate (not in an integration test) because the `Engine` wrapper
//! reads the private `self.cache` / `self.journal` fields directly — exactly
//! as `invariants::check_no_stale_undef` reads the private `self.functions`.

use reify_core::ValueCellId;
use reify_ir::{DeterminacyState, PersistentMap, Value};

use crate::cache::{CacheStore, CachedResult, NodeId};
use crate::journal::{EventJournal, EventKind, EventPayload};

/// A single snapshot↔cache content-hash divergence: `cell`'s snapshot value
/// disagrees, by content-hash, with its PRESENT (and non-Skip-exempt) cache
/// entry. Mirrors `invariants::StaleUndefViolation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotCacheDivergence {
    pub cell: ValueCellId,
    pub detail: String,
}

/// INV-EVAL-4 (§2.6): report every value cell whose snapshot value diverges,
/// by content-hash, from its present cache entry.
///
/// FORWARD (snapshot→cache) audit. For each `(cell, (val, det))` in
/// `snapshot_values`:
/// - `cache.get(NodeId::Value(cell))` is `None` ⇒ NOT a divergence — a MISSING
///   cache entry is legitimate, never a disagreement. Cells that never
///   evaluated, build-only `Type::Geometry` cells (which sit at `(Undef, _)`
///   in `eval_state()` by construction — see `invariants.rs` clause 7),
///   guard-inactive members, and Skip-cold cells all legitimately have no
///   cache entry. A `CacheLeg::Record`-committed non-Undef cell ALWAYS has a
///   cache entry (the `commit_cell_result` primitive writes it atomically), so
///   absence never hides a real Record/snapshot mismatch. This mirrors
///   `check_no_stale_undef`'s "err toward exempt, enumerate residuals" policy.
/// - `Some(entry)` whose stored `entry.result_hash` differs from the snapshot
///   side's recomputed `CachedResult::Value(val.clone(), *det).content_hash()`
///   ⇒ a divergence, UNLESS the cell is Skip-exempt — its latest `Started`
///   journal event carries a `cache-skip=` marker (see
///   [`cell_latest_commit_was_skip`]). `result_hash` is the authoritative content
///   identity the CacheStore keyed the entry on; the snapshot side is
///   recomputed because `DeterminacyState` is `Copy` (so `*det` is cheap) and
///   the `CachedResult::Value` domain-separation tag makes a non-`Value` cache
///   entry (should never occur for a `NodeId::Value` key) also mismatch — which
///   is correct.
pub fn check_snapshot_cache_divergence(
    snapshot_values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    cache: &CacheStore,
    journal: &EventJournal,
) -> Vec<SnapshotCacheDivergence> {
    let mut divergences = Vec::new();
    for (cell, (val, det)) in snapshot_values.iter() {
        let node = NodeId::Value(cell.clone());
        // Cache-absence is NOT a divergence (see the fn doc).
        let Some(entry) = cache.get(&node) else {
            continue;
        };
        let expected = CachedResult::Value(val.clone(), *det).content_hash();
        if entry.result_hash != expected {
            // Skip-exemption: a `CacheLeg::Skip` commit (the self-datum /
            // structural-query / annotation-args post-pass overwrites task γ
            // migrated) deliberately leaves the cache entry un-updated relative
            // to the snapshot value it just wrote, flagging the divergence as
            // intentional via a `cache-skip=` marker on the cell's latest
            // `Started` event. Such a cell is exempt by construction.
            if cell_latest_commit_was_skip(journal, &node) {
                continue;
            }
            divergences.push(SnapshotCacheDivergence {
                cell: cell.clone(),
                detail: format!(
                    "value cell {cell:?}: snapshot value content-hash {expected:?} \
                     ≠ cache entry result_hash {:?}",
                    entry.result_hash
                ),
            });
        }
    }
    divergences
}

/// `true` iff `node`'s LATEST `EventKind::Started` journal event carries a
/// `cache-skip=` marker in its `EventPayload::Custom` payload — the exact
/// `<trace-slug>|cache-skip=<reason>` shape `commit_cell_result` writes onto
/// the `Started` event for every `CacheLeg::Skip` commit (`cell_commit.rs`).
///
/// LATEST-`Started` (not ANY-`Started`) is the faithful predicate: the three
/// post-pass overwrites task γ migrated (self-datum, structural-query,
/// annotation-args) are Record-then-Skip, so the cell's most-recent `Started`
/// event carries the marker and the current snapshot value reflects that Skip.
/// An earlier Record `Started` (no marker) must not mask a later Skip, nor a
/// later plain Record mask an earlier Skip.
///
/// The journal is consulted ONLY for this exemption — it is NOT a complete
/// authority on cache writes (some `engine_eval.rs` cache writes emit no
/// journal event at all — `engine_eval.rs:5380`), so absence-of-marker never
/// implies anything about the cache itself; it just means "not Skip-exempt".
fn cell_latest_commit_was_skip(journal: &EventJournal, node: &NodeId) -> bool {
    journal
        .events_for_node(node)
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::Started))
        .is_some_and(|event| {
            matches!(&event.payload, Some(EventPayload::Custom(s)) if s.contains("cache-skip="))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Instant;

    use reify_core::{ValueCellId, VersionId};
    use reify_ir::{DeterminacyState, PersistentMap, Value};

    use crate::cache::{CacheStore, CachedResult, NodeId};
    use crate::deps::DependencyTrace;
    use crate::journal::{EvalEvent, EventJournal, EventKind, EventPayload};

    /// Anti-silent-accept RED self-test (step-1): seeds a present-but-mismatched
    /// cache entry and asserts the checker fires. The snapshot holds
    /// `Value::Int(2)` for `cell`, but the cache entry for the SAME cell was
    /// recorded with `Value::Int(1)` — so the stored `result_hash`
    /// (= hash(Int(1))) ≠ the `CachedResult::Value(Int(2), Determined)`
    /// content-hash the snapshot side recomputes. With no `cache-skip=` journal
    /// marker present, this is a genuine divergence the checker MUST report.
    ///
    /// RED before step-2: the module's `check_snapshot_cache_divergence` free
    /// function and `SnapshotCacheDivergence` struct do not exist yet, so this
    /// does not compile.
    #[test]
    fn divergence_fires_on_present_but_mismatched_cache_entry() {
        let cell = ValueCellId::new("SnapCacheDemo", "diverged");

        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        snapshot_values.insert(cell.clone(), (Value::Int(2), DeterminacyState::Determined));

        // Seed the cache with a DIFFERENT value (Int(1)) for the same cell, so
        // the stored `result_hash` diverges from the snapshot side's hash.
        let mut cache = CacheStore::new();
        cache.record_evaluation(
            NodeId::Value(cell.clone()),
            CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
            VersionId(1),
            DependencyTrace::default(),
        );

        // Empty journal: no `cache-skip=` marker, so the cell is NOT exempt.
        let journal = EventJournal::new();

        let divergences = check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

        assert!(
            !divergences.is_empty(),
            "expected the checker to report the seeded snapshot↔cache divergence, \
             got zero — a checker that never fires would make the corpus sweep \
             vacuously green"
        );
        assert!(
            divergences.iter().any(|d| d.cell == cell),
            "expected a divergence naming cell {cell:?}, got {:?}",
            divergences.iter().map(|d| &d.cell).collect::<Vec<_>>()
        );
    }

    /// RED before step-4: a cell whose snapshot↔cache hashes DIVERGE but whose
    /// latest `Started` journal event carries a `cache-skip=` marker (the exact
    /// `<trace-slug>|cache-skip=<reason>` shape `commit_cell_result` writes for
    /// every `CacheLeg::Skip` commit) must be EXEMPT. Step-2's checker has no
    /// exemption, so it still reports this cell — this test fails until step-4
    /// adds the Skip guard.
    #[test]
    fn skip_committed_cell_is_exempt() {
        let cell = ValueCellId::new("SnapCacheSkip", "diverged_but_skipped");

        // Identical mismatched state to the step-1 fire test: snapshot Int(2),
        // cache Int(1) — the hashes diverge.
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        snapshot_values.insert(cell.clone(), (Value::Int(2), DeterminacyState::Determined));

        let mut cache = CacheStore::new();
        cache.record_evaluation(
            NodeId::Value(cell.clone()),
            CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
            VersionId(1),
            DependencyTrace::default(),
        );

        // The distinguishing input: a `cache-skip=` marker on the cell's latest
        // Started event — as commit_cell_result writes for a CacheLeg::Skip.
        let mut journal = EventJournal::new();
        journal.record(EvalEvent {
            timestamp: Instant::now(),
            node_id: NodeId::Value(cell.clone()),
            kind: EventKind::Started,
            version: VersionId(1),
            payload: Some(EventPayload::Custom(
                "post-pass-overwrite|cache-skip=seeded".to_string(),
            )),
        });

        let divergences = check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

        assert!(
            divergences.is_empty(),
            "a cell whose latest Started event carries a `cache-skip=` marker must \
             be exempt even though its snapshot↔cache hashes diverge, got {divergences:?}"
        );
    }

    /// A cell whose cache entry was recorded with the SAME `(val, det)` as the
    /// snapshot holds — so the stored `result_hash` equals the snapshot side's
    /// recomputed hash — is NOT a divergence. GREEN under step-2 already (the
    /// core checker only fires on a hash MISMATCH); pins that matching state is
    /// never reported, independent of the journal.
    #[test]
    fn matching_hash_is_not_a_divergence() {
        let cell = ValueCellId::new("SnapCacheMatch", "agrees");

        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        snapshot_values.insert(cell.clone(), (Value::Int(7), DeterminacyState::Determined));

        let mut cache = CacheStore::new();
        cache.record_evaluation(
            NodeId::Value(cell.clone()),
            CachedResult::Value(Value::Int(7), DeterminacyState::Determined),
            VersionId(1),
            DependencyTrace::default(),
        );

        let journal = EventJournal::new();

        let divergences = check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

        assert!(
            divergences.is_empty(),
            "a cell whose snapshot value hashes equal to its cache entry's \
             result_hash is NOT a divergence, got {divergences:?}"
        );
    }
}
