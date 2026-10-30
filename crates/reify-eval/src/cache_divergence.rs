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
            // Skip-exemption: a `CacheLeg::Skip` commit (the structural-query /
            // annotation-args post-pass overwrites task γ migrated)
            // deliberately leaves the cache entry un-updated relative
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

impl crate::Engine {
    /// Debug-gate corpus-harness wrapper: threads this Engine's own retained
    /// post-eval state into the free-function [`check_snapshot_cache_divergence`]
    /// — reading `eval_state().snapshot.values` against the Engine's own private
    /// `cache` and `journal` fields. Returns an empty `Vec` when no eval has run
    /// yet (`eval_state()` is `None`): there is no retained state to check.
    ///
    /// Mirrors `invariants`'s `Engine::check_no_stale_undef`: it always inspects
    /// exactly the snapshot the calling entry point just produced, since every
    /// state-producing entry point (`eval`, `eval_cached`, `edit_param`,
    /// `edit_source`) installs its post-run snapshot into `self.eval_state`
    /// immediately before returning.
    pub fn check_snapshot_cache_divergence(&self) -> Vec<SnapshotCacheDivergence> {
        let Some(state) = self.eval_state() else {
            return Vec::new();
        };
        check_snapshot_cache_divergence(&state.snapshot.values, &self.cache, &self.journal)
    }
}

/// `true` iff `node`'s LATEST `EventKind::Started` journal event carries a
/// `cache-skip=` marker in its `EventPayload::Custom` payload — the exact
/// `<trace-slug>|cache-skip=<reason>` shape `commit_cell_result` writes onto
/// the `Started` event for every `CacheLeg::Skip` commit (`cell_commit.rs`).
///
/// LATEST-`Started` (not ANY-`Started`) is the faithful predicate: the
/// post-pass overwrites task γ migrated (structural-query, annotation-args)
/// are Record-then-Skip, so the cell's most-recent `Started`
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
        .find(|event| matches!(event.kind, EventKind::Started { .. }))
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
    use crate::journal::{
        CacheLegKind, CommitTag, EvalEvent, EventJournal, EventKind, TraceSource,
    };

    /// A divergent state for `cell`: the snapshot holds `Int(2)` while the
    /// cache entry was recorded with `Int(1)`, so the stored `result_hash`
    /// differs from the hash the snapshot side recomputes.
    fn divergent_state(
        cell: &ValueCellId,
    ) -> (
        PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        CacheStore,
    ) {
        let mut snapshot_values = PersistentMap::new();
        snapshot_values.insert(cell.clone(), (Value::Int(2), DeterminacyState::Determined));
        let mut cache = CacheStore::new();
        cache.record_evaluation(
            NodeId::Value(cell.clone()),
            CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
            VersionId(1),
            DependencyTrace::default(),
        );
        (snapshot_values, cache)
    }

    /// A journal `Started` event for `cell` carrying `commit` and no payload.
    fn started(cell: &ValueCellId, commit: Option<CommitTag>) -> EvalEvent {
        EvalEvent {
            timestamp: Instant::now(),
            node_id: NodeId::Value(cell.clone()),
            kind: EventKind::Started { commit },
            version: VersionId(1),
            payload: None,
        }
    }

    fn post_pass(cache: CacheLegKind) -> Option<CommitTag> {
        Some(CommitTag {
            trace: TraceSource::PostPassOverwrite,
            cache,
        })
    }

    fn skipped() -> CacheLegKind {
        CacheLegKind::Skipped {
            reason: "seeded".to_string(),
        }
    }

    /// Anti-silent-accept self-test: a present-but-mismatched cache entry with
    /// no Skip-tagged commit in the journal is a genuine divergence the
    /// checker MUST report.
    #[test]
    fn divergence_fires_on_present_but_mismatched_cache_entry() {
        let cell = ValueCellId::new("SnapCacheDemo", "diverged");
        let (snapshot_values, cache) = divergent_state(&cell);
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

    /// A divergent cell whose latest `Started` event carries a
    /// `CacheLegKind::Skipped` commit tag — what `commit_cell_result` records
    /// for a `CacheLeg::Skip` commit — is EXEMPT. The tag is the only carrier:
    /// the event has no payload.
    #[test]
    fn skip_committed_cell_is_exempt() {
        let cell = ValueCellId::new("SnapCacheSkip", "diverged_but_skipped");
        let (snapshot_values, cache) = divergent_state(&cell);
        let mut journal = EventJournal::new();
        journal.record(started(&cell, post_pass(skipped())));

        let divergences = check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

        assert!(
            divergences.is_empty(),
            "a cell whose latest Started event carries a Skipped commit tag must \
             be exempt even though its snapshot↔cache hashes diverge, got {divergences:?}"
        );
    }

    /// The LATEST commit decides: an earlier Skipped commit does not exempt a
    /// cell whose later commit recorded the cache leg.
    #[test]
    fn recorded_commit_after_skip_is_reported() {
        let cell = ValueCellId::new("SnapCacheSkip", "skipped_then_recorded");
        let (snapshot_values, cache) = divergent_state(&cell);
        let mut journal = EventJournal::new();
        journal.record(started(&cell, post_pass(skipped())));
        journal.record(started(&cell, post_pass(CacheLegKind::Recorded)));

        let divergences = check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

        assert_eq!(
            divergences.iter().map(|d| &d.cell).collect::<Vec<_>>(),
            vec![&cell],
            "a Recorded commit after a Skipped one must leave the divergence reported"
        );
    }

    /// A `Started` event with no commit tag is a visit, not a Skip commit, so
    /// it exempts nothing.
    #[test]
    fn untagged_started_is_not_exempt() {
        let cell = ValueCellId::new("SnapCacheSkip", "untagged");
        let (snapshot_values, cache) = divergent_state(&cell);
        let mut journal = EventJournal::new();
        journal.record(started(&cell, None));

        let divergences = check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

        assert_eq!(
            divergences.iter().map(|d| &d.cell).collect::<Vec<_>>(),
            vec![&cell],
            "an untagged Started event must not exempt a divergent cell"
        );
    }

    /// A cell whose cache entry was recorded with the SAME `(val, det)` as the
    /// snapshot holds — so the stored `result_hash` equals the snapshot side's
    /// recomputed hash — is NOT a divergence, independent of the journal.
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
