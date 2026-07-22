//! Debug-gate integration suite for the INV-EVAL-4 snapshot↔cache content-hash
//! divergence audit (task ι, PRD `docs/prds/v0_6/eval-cell-commit-substrate.md`
//! §2.6 / §3 P3 / §7 B4).
//!
//! Runs `reify_eval::check_snapshot_cache_divergence` — and the
//! `Engine::check_snapshot_cache_divergence` convenience wrapper — proving the
//! invariant holds post-eval: for every cell in `snapshot.values`, any PRESENT
//! cache entry agrees by content-hash with the snapshot value, unless the cell
//! is `CacheLeg::Skip`-exempt (a `cache-skip=` marker on its latest `Started`
//! journal event). A MISSING cache entry is never a divergence (forward audit).
//!
//! MANDATED anti-silent-accept: `seeded_divergence_is_reported` fabricates a
//! minimal divergent state (NOT a real compile+eval — post-γ there is no
//! reproducible real divergence through `eval()`, since every post-pass
//! overwrite is `CacheLeg::Skip`) and asserts the checker actually fires. A
//! checker that always returned `vec![]` would otherwise make the corpus sweep
//! (step-7) vacuously green — the exact rationale the precedent (task 4952,
//! `no_stale_undef_invariant_gate.rs`) records.

use std::time::Instant;

use reify_core::{ValueCellId, VersionId};
use reify_eval::cache::{CacheStore, CachedResult, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::journal::{EvalEvent, EventJournal, EventKind, EventPayload};
use reify_ir::{DeterminacyState, PersistentMap, Value};

/// Builds the shared mismatched state: the snapshot holds `Int(2)` for the
/// returned cell, while the cache holds `Int(1)` for that same cell — so the
/// stored `result_hash` diverges from the snapshot side's recomputed hash.
/// Constructible entirely via the public `CacheStore::{new,record_evaluation}`
/// API, so the fire test needs no real end-to-end divergence.
fn seeded_divergent_state() -> (
    ValueCellId,
    PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    CacheStore,
) {
    let cell = ValueCellId::new("SnapCacheGate", "diverged");

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

    (cell, snapshot_values, cache)
}

/// MANDATED anti-silent-accept fire test: a present-but-mismatched, non-exempt
/// cache entry MUST be reported. A checker never observed to fire would make
/// the corpus sweep vacuously green (precedent task 4952).
#[test]
fn seeded_divergence_is_reported() {
    let (cell, snapshot_values, cache) = seeded_divergent_state();
    let journal = EventJournal::new();

    let divergences =
        reify_eval::check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

    assert!(
        !divergences.is_empty(),
        "expected the checker to report the seeded snapshot↔cache divergence, got \
         zero — a checker that never fires would make the corpus sweep vacuously green"
    );
    assert!(
        divergences.iter().any(|d| d.cell == cell),
        "expected a divergence naming cell {cell:?}, got {:?}",
        divergences.iter().map(|d| &d.cell).collect::<Vec<_>>()
    );
}

/// Companion exemption test: the SAME divergent state plus a `cache-skip=`
/// marker on the cell's latest `Started` event → the checker must report
/// nothing (the divergence is intentional, flagged by a `CacheLeg::Skip`).
#[test]
fn seeded_skip_committed_divergence_is_exempted() {
    let (cell, snapshot_values, cache) = seeded_divergent_state();

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

    let divergences =
        reify_eval::check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

    assert!(
        divergences.is_empty(),
        "a Skip-committed (cache-skip= marker) cell must be exempt despite the hash \
         divergence, got {divergences:?}"
    );
}

/// RED until step-6: the `Engine::check_snapshot_cache_divergence` wrapper does
/// not exist yet. A fresh engine that has never eval'd has no retained
/// `eval_state()`, so the wrapper must return an empty `Vec` — there is no
/// retained snapshot/cache to check.
#[test]
fn fresh_engine_reports_no_divergence() {
    let engine =
        reify_eval::Engine::new(Box::new(reify_constraints::SimpleConstraintChecker), None);
    let divergences = engine.check_snapshot_cache_divergence();
    assert!(
        divergences.is_empty(),
        "a fresh engine (no eval run) has no retained state to check, got {divergences:?}"
    );
}
