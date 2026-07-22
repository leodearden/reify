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

#[cfg(test)]
mod tests {
    use super::*;

    use reify_core::{ValueCellId, VersionId};
    use reify_ir::{DeterminacyState, PersistentMap, Value};

    use crate::cache::{CacheStore, CachedResult, NodeId};
    use crate::deps::DependencyTrace;
    use crate::journal::EventJournal;

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
}
