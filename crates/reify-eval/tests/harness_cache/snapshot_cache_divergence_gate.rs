//! Debug-gate integration suite for the INV-EVAL-4 snapshot↔cache content-hash
//! divergence audit (task ι, PRD `docs/prds/v0_6/eval-cell-commit-substrate.md`
//! §2.6 / §3 P3 / §7 B4).
//!
//! Runs `reify_eval::check_snapshot_cache_divergence` — and the
//! `Engine::check_snapshot_cache_divergence` convenience wrapper — proving the
//! invariant holds post-eval: for every cell in `snapshot.values`, any PRESENT
//! cache entry agrees by content-hash with the snapshot value, unless the cell
//! is `CacheLeg::Skip`-exempt (its latest `Started` journal event carries a
//! `CacheLegKind::Skipped` commit tag). A MISSING cache entry is never a
//! divergence (forward audit).
//!
//! MANDATED anti-silent-accept: `seeded_divergence_is_reported` fabricates a
//! minimal divergent state (NOT a real compile+eval — there is no reproducible
//! real non-exempt divergence through `eval()`, since every post-pass
//! overwrite is `CacheLeg::Skip`) and asserts the checker actually fires. A
//! checker that always returned `vec![]` would otherwise make the unified corpus
//! sweep vacuously green — the exact rationale the precedent (task 4952,
//! `no_stale_undef_invariant_gate.rs`) records.
//!
//! # Where the corpus sweep went (task #7431)
//!
//! The broad sweep over `examples/` no longer lives here. It was unified with
//! the sibling INV-EVAL-5 no-stale-Undef sweep into ONE hash-keyed sweep that
//! does a single compile+eval per corpus file and asserts BOTH invariants off
//! it: `harness_corpus_gates::eval_invariant_corpus_sweep::corpus_sweep_shard_NN`.
//! The residual list moved with it as `SNAPSHOT_CACHE_DIVERGENCE_RESIDUALS`, as
//! did the `REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS` break-glass knob — which still
//! DOWNGRADES this invariant's failures to a warn (prints offenders, does not
//! fail), mirroring `REIFY_MAIN_GATE_BYPASS` (CLAUDE.md) so a future change that
//! introduces a divergence can never wedge the merge queue, and still applies to
//! THIS invariant's findings only. The gate still ASSERTS by default and is
//! still a standard `cargo test`, so no `verify.sh` wiring is required.
//!
//! Only the corpus EVALUATION is shared there; the two CHECKERS stay distinct
//! per task 5060. What stays HERE is everything about this checker rather than
//! about the corpus — the seeded self-tests below, which remain the sweep's
//! vacuity guard and execute in the same nextest pass, alongside the rest of the
//! cache subsystem's tests.

use std::time::Instant;

use reify_core::{ValueCellId, VersionId};
use reify_eval::cache::{CacheStore, CachedResult, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::journal::{
    CacheLegKind, CommitTag, EvalEvent, EventJournal, EventKind, TraceSource,
};
use reify_ir::{DeterminacyState, PersistentMap, Value};
use reify_test_support::{make_engine, parse_and_compile};

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

/// A journal `Started` event for `cell` carrying a post-pass commit tag with
/// the given cache leg, and no payload.
fn post_pass_started(cell: &ValueCellId, cache: CacheLegKind) -> EvalEvent {
    EvalEvent {
        timestamp: Instant::now(),
        node_id: NodeId::Value(cell.clone()),
        kind: EventKind::Started {
            commit: Some(CommitTag {
                trace: TraceSource::PostPassOverwrite,
                cache,
            }),
        },
        version: VersionId(1),
        payload: None,
    }
}

fn seeded_skip() -> CacheLegKind {
    CacheLegKind::Skipped { reason: "seeded" }
}

/// Companion exemption test: the SAME divergent state plus a
/// `CacheLegKind::Skipped` commit tag on the cell's latest `Started` event →
/// the checker must report nothing (the divergence is intentional, flagged by
/// a `CacheLeg::Skip` commit).
#[test]
fn seeded_skip_committed_divergence_is_exempted() {
    let (cell, snapshot_values, cache) = seeded_divergent_state();

    let mut journal = EventJournal::new();
    journal.record(post_pass_started(&cell, seeded_skip()));

    let divergences =
        reify_eval::check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

    assert!(
        divergences.is_empty(),
        "a Skip-committed (Skipped commit tag) cell must be exempt despite the hash \
         divergence, got {divergences:?}"
    );
}

/// The LATEST commit decides: a Recorded commit after a Skipped one leaves the
/// divergence reported.
#[test]
fn seeded_recorded_after_skip_divergence_is_reported() {
    let (cell, snapshot_values, cache) = seeded_divergent_state();

    let mut journal = EventJournal::new();
    journal.record(post_pass_started(&cell, seeded_skip()));
    journal.record(post_pass_started(&cell, CacheLegKind::Recorded));

    let divergences =
        reify_eval::check_snapshot_cache_divergence(&snapshot_values, &cache, &journal);

    assert_eq!(
        divergences.iter().map(|d| &d.cell).collect::<Vec<_>>(),
        vec![&cell],
        "a Recorded commit after a Skipped one must not exempt the divergent cell"
    );
}

/// A real compile+eval whose post-pass Skip commit genuinely diverges from its
/// cache entry, and is exempt only through the typed tag. `ms = self.members`
/// is overwritten by the structural-query post-pass with `CacheLeg::Skip`, so
/// its cache entry keeps the main pass's pre-expansion value.
#[test]
fn real_post_pass_skip_divergence_is_exempted_by_its_tag() {
    let module = parse_and_compile(
        r#"
    structure S {
        let ms = self.members
        let ms_det = determined(ms)
    }
"#,
    );
    let mut engine = make_engine();
    engine.eval(&module);

    let ms = ValueCellId::new("S", "ms");
    let node = NodeId::Value(ms.clone());

    // (1) `ms` genuinely diverges, so the exemption is load-bearing.
    let (value, det) = engine
        .snapshot()
        .and_then(|snapshot| snapshot.values.get(&ms))
        .expect("ms is in the snapshot")
        .clone();
    let entry = engine
        .cache_store()
        .get(&node)
        .expect("the main pass caches ms before the post-pass overwrites it");
    assert_ne!(
        entry.result_hash,
        CachedResult::Value(value, det).content_hash(),
        "ms's cache entry must diverge from its snapshot value, or this test \
         proves nothing about the exemption"
    );

    // (2) The latest commit for `ms` is the post-pass's Skip.
    let latest_commit = engine.journal().latest_commit_for_node(&node);
    assert!(
        matches!(
            latest_commit,
            Some(CommitTag {
                cache: CacheLegKind::Skipped { .. },
                ..
            })
        ),
        "ms's latest Started event must carry a Skipped commit tag, got {latest_commit:?}"
    );

    // (3) The checker exempts it.
    let divergences = engine.check_snapshot_cache_divergence();
    assert!(
        divergences.is_empty(),
        "the Skip-committed post-pass cell must be exempt, got {divergences:?}"
    );
}

/// RED until step-6: the `Engine::check_snapshot_cache_divergence` wrapper does
/// not exist yet. A fresh engine that has never eval'd has no retained
/// `eval_state()`, so the wrapper must return an empty `Vec` — there is no
/// retained snapshot/cache to check.
#[test]
fn fresh_engine_reports_no_divergence() {
    let engine =
        reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(reify_constraints::SimpleConstraintChecker), None);
    let divergences = engine.check_snapshot_cache_divergence();
    assert!(
        divergences.is_empty(),
        "a fresh engine (no eval run) has no retained state to check, got {divergences:?}"
    );
}
