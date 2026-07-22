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
//!
//! # Corpus sweep + break-glass env knob
//!
//! The broad sweep compiles + evaluates every `.ri` file under `examples/`
//! (sharded into `CORPUS_SHARD_COUNT` `#[test]` fns — see that const's doc for
//! why) and asserts each produces ZERO non-exempt snapshot↔cache divergences,
//! modulo the explicit, printed `KNOWN_RESIDUAL_DIVERGENCE_SKIPS` list. The
//! gate ASSERTS by default (no env needed), and the verify pipeline runs that
//! default — this is a standard `cargo test`, so no `verify.sh` wiring is
//! required.
//!
//! `REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS=1` is the break-glass escape hatch: it
//! DOWNGRADES the corpus assertion to a warn (prints offenders, does not fail),
//! mirroring `REIFY_MAIN_GATE_BYPASS` (CLAUDE.md) so a future change that
//! introduces a divergence can never wedge the merge queue. Because task γ has
//! landed, ι ships in the terminal ASSERT state; a `REIFY_SNAPSHOT_CACHE_AUDIT_ENFORCE`
//! alias is noted for symmetry with the main-gate ENFORCE/BYPASS pattern but is
//! a no-op against this default-assert shipped state.

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

// ── Broad debug-gate corpus sweep over examples/ ─────────────────────────────

/// Recursively collect every `.ri` file under `dir` (including subdirectories).
/// Unreadable entries/directories are silently skipped — this only ever walks
/// our own repo directories, which are expected to be readable.
fn collect_ri_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ri_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ri") {
            out.push(path);
        }
    }
}

/// Files with a residual snapshot↔cache divergence that is a documented
/// eval-surface limitation, NOT a checker gap fixable within
/// `cache_divergence.rs`'s `(snapshot_values, cache, journal)` signature — each
/// root-caused during the ι broad-sweep investigation (step-8). Matched by path
/// SUFFIX against each corpus file's display path. Every skip is PRINTED (never
/// silent) so bounded coverage never reads as full coverage; if a future engine
/// change resolves one of these, `run_corpus_shard` FAILS the sweep with a "no
/// longer diverges" error so the now-stale entry gets deleted rather than
/// lingering as dead weight that silently masks the recovered coverage.
///
/// Starts EMPTY: task γ (engine_eval.rs migration) routed every post-pass
/// overwrite through `commit_cell_result` with `CacheLeg::Skip`, so no
/// Record-committed example cell is expected to diverge from its cache without
/// a `cache-skip=` marker. A residual that traces to a genuine post-γ
/// snapshot↔cache write bug (not a documented eval-surface limitation) is a
/// design_concern to escalate, NOT a KNOWN_RESIDUAL entry to paper over.
const KNOWN_RESIDUAL_DIVERGENCE_SKIPS: &[(&str, &str)] = &[
    (
        "examples/fdm_bracket.ri",
        "FdmBracket.r_print: an `@optimized` `solve_elastic_static(...)` FEA \
         compute-dispatch result cell (task #4726). Compute dispatch runs OUTSIDE \
         the plain expr-eval commit path — it is NOT one of the three post-passes \
         (self-datum / structural-query / annotation-args) task γ routed through \
         `commit_cell_result` — so the ComputeNode writes its result into the cache \
         while the retained `eval_state()` snapshot value for the cell diverges. This \
         is the compute-dispatch analog of the geometry-handle standing surface gap: \
         `invariants.rs` clause 5a already EXEMPTS exactly this `@optimized` cell \
         class from the sibling stale-Undef invariant for the same 'evaluated outside \
         the plain expr-eval path' reason. A documented eval-surface limitation, not a \
         post-γ snapshot↔cache write bug.",
    ),
    (
        "examples/fea_shell_too_thick_annotated.ri",
        "FeaShellTooThickAnnotated.result: same class as fdm_bracket.ri above — an \
         `@optimized` `solve_elastic_static(...)` FEA compute-dispatch result cell \
         whose ComputeNode-written cache entry diverges from the retained snapshot \
         value because compute dispatch runs outside the `commit_cell_result` \
         post-pass path task γ migrated (invariants.rs clause 5a exempts the same \
         cell class from the sibling invariant). A documented eval-surface \
         limitation, not a post-γ snapshot↔cache write bug.",
    ),
];

/// Number of shards the broad corpus sweep is split across — one shard per
/// `snapshot_cache_sweep_shard_NN` `#[test]` fn (below).
///
/// The user-observable debug-gate signal (task ι, PRD §2.6 / §7 B4) is that
/// every `.ri` file under `examples/` produces ZERO non-exempt snapshot↔cache
/// divergences — modulo the explicit, printed `KNOWN_RESIDUAL_DIVERGENCE_SKIPS`
/// above. Running all ~251 example files in ONE sequential test would take long
/// enough, as the last test left running with nothing to interleave its output
/// with, to trip the verify pipeline's heartbeat-idle backstop despite every
/// file passing (the exact failure mode the precedent, task 4952, debugged).
/// Sharding into `CORPUS_SHARD_COUNT` independent `#[test]` fns lets the test
/// runner schedule them as separate, concurrently-run tests — each reporting
/// its own PASS line — so the worst-case silent gap is bounded by roughly one
/// shard's share of the corpus (~11 files) instead of the whole corpus.
const CORPUS_SHARD_COUNT: usize = 24;

/// Break-glass: `REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS=1` downgrades the corpus
/// assertion to a warn (prints offenders, does not fail), mirroring
/// `REIFY_MAIN_GATE_BYPASS`. See the file header for the ENFORCE/BYPASS
/// symmetry rationale.
fn audit_bypassed() -> bool {
    std::env::var("REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS").is_ok_and(|v| v == "1")
}

/// Collects the full, deterministically-sorted `examples/` corpus file list.
/// Every shard calls this and keeps only the files whose index (in this SAME
/// sorted order) is `≡ shard_index (mod CORPUS_SHARD_COUNT)`, so the partition
/// is stable across shards/runs without sharing state between the independent
/// shard processes. Cheap (a directory walk, no compilation).
fn corpus_files() -> Vec<std::path::PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let examples_dir = std::path::Path::new(manifest_dir).join("../../examples");
    let mut files = Vec::new();
    collect_ri_files(&examples_dir, &mut files);
    files.sort();
    files
}

/// Runs the broad corpus sweep over the slice of `examples/` assigned to
/// `shard_index` (of `CORPUS_SHARD_COUNT` total). Per file: SKIP any file whose
/// compile emits an Error-severity diagnostic (printed), exempt
/// `KNOWN_RESIDUAL_DIVERGENCE_SKIPS` entries that STILL diverge (printed with
/// their divergence count), and require zero divergences everywhere else. A
/// KNOWN_RESIDUAL entry that no longer diverges FAILS the sweep as stale (so the
/// dead exemption gets deleted). When `REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS=1`, all
/// offenders (real divergences AND stale residuals) are printed and the
/// assertion is downgraded to a warn.
fn run_corpus_shard(shard_index: usize) {
    let files = corpus_files();
    let shard_files: Vec<&std::path::PathBuf> = files
        .iter()
        .enumerate()
        .filter(|(i, _)| i % CORPUS_SHARD_COUNT == shard_index)
        .map(|(_, f)| f)
        .collect();
    let shard_file_count = shard_files.len();

    let mut skipped: Vec<String> = Vec::new();
    let mut known_residual_skips: Vec<(String, &'static str, usize)> = Vec::new();
    // KNOWN_RESIDUAL entries that were matched but produced ZERO divergences:
    // their exemption is now stale and must be deleted (fails the sweep below).
    let mut stale_residuals: Vec<String> = Vec::new();
    let mut offenders: Vec<(String, Vec<reify_eval::SnapshotCacheDivergence>)> = Vec::new();

    for path in shard_files {
        let display = path.display().to_string();
        let source =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {display}: {e}"));

        let compiled = reify_test_support::compile_source_with_stdlib(&source);
        let errors = reify_test_support::collect_errors(&compiled.diagnostics);
        if !errors.is_empty() {
            skipped.push(display);
            continue;
        }

        let mut engine = reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            Some(Box::new(reify_test_support::MockGeometryKernel::new())),
        );
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        engine.eval(&compiled);
        let divergences = engine.check_snapshot_cache_divergence();

        if let Some((_, reason)) = KNOWN_RESIDUAL_DIVERGENCE_SKIPS
            .iter()
            .find(|(suffix, _)| display.ends_with(suffix))
        {
            // A KNOWN_RESIDUAL entry that no longer diverges is stale: route it
            // to `stale_residuals` so the sweep FAILS loud (below) and the entry
            // gets deleted, instead of the resolved residual silently lingering
            // as a dead exemption that masks the now-recovered coverage.
            if divergences.is_empty() {
                stale_residuals.push(display);
            } else {
                known_residual_skips.push((display, reason, divergences.len()));
            }
            continue;
        }

        if !divergences.is_empty() {
            offenders.push((display, divergences));
        }
    }

    eprintln!(
        "snapshot_cache_divergence_sweep shard {shard_index}/{CORPUS_SHARD_COUNT}: {} files evaluated, {} skipped (compile errors), {} skipped (known residual), {} stale residual(s)",
        shard_file_count - skipped.len() - known_residual_skips.len() - stale_residuals.len(),
        skipped.len(),
        known_residual_skips.len(),
        stale_residuals.len(),
    );
    for s in &skipped {
        eprintln!("  SKIP (compile error): {s}");
    }
    for (f, reason, divergence_count) in &known_residual_skips {
        eprintln!(
            "  SKIP (known residual, {divergence_count} divergence(s)): {f}\n    reason: {reason}"
        );
    }

    if offenders.is_empty() && stale_residuals.is_empty() {
        return;
    }

    let mut failures: Vec<String> = Vec::new();

    if !offenders.is_empty() {
        let report = offenders
            .iter()
            .map(|(f, ds)| {
                let detail = ds
                    .iter()
                    .map(|d| format!("    {:?}: {}", d.cell, d.detail))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("  {f}:\n{detail}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        failures.push(format!(
            "expected zero non-exempt snapshot↔cache divergences across examples/; \
             offending file(s):\n{report}"
        ));
    }

    if !stale_residuals.is_empty() {
        let report = stale_residuals
            .iter()
            .map(|f| format!("  {f}"))
            .collect::<Vec<_>>()
            .join("\n");
        failures.push(format!(
            "stale KNOWN_RESIDUAL_DIVERGENCE_SKIPS entr(ies) that no longer diverge — \
             delete them from KNOWN_RESIDUAL_DIVERGENCE_SKIPS so a resolved residual \
             stops masking the now-recovered coverage:\n{report}"
        ));
    }

    let report = failures.join("\n\n");

    if audit_bypassed() {
        eprintln!(
            "[REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS] shard {shard_index}: DOWNGRADED to warn:\n{report}"
        );
        return;
    }

    panic!(
        "{report}\n\n(set REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS=1 to downgrade this to a \
         warning as a break-glass escape hatch)"
    );
}

/// One `#[test]` fn per corpus shard — see `CORPUS_SHARD_COUNT`'s doc comment
/// for why the broad sweep is sharded, and `run_corpus_shard` for the per-shard
/// logic. `$idx` must range exactly over `0..CORPUS_SHARD_COUNT` (checked by
/// `corpus_shard_count_matches_generated_tests` below).
macro_rules! corpus_shard_tests {
    ($($name:ident = $idx:literal),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                run_corpus_shard($idx);
            }
        )+

        /// Every shard index passed to THIS macro invocation, in source order —
        /// derived from the same repetition that generates the `#[test]` fns
        /// above, so deleting a `snapshot_cache_sweep_shard_NN` line shrinks
        /// this array too. This is what lets
        /// `corpus_shard_count_matches_generated_tests` detect a deleted shard
        /// line: comparing two independently-hardcoded literals cannot.
        const GENERATED_SHARD_INDICES: &[usize] = &[$($idx),+];
    };
}

corpus_shard_tests! {
    snapshot_cache_sweep_shard_00 = 0,
    snapshot_cache_sweep_shard_01 = 1,
    snapshot_cache_sweep_shard_02 = 2,
    snapshot_cache_sweep_shard_03 = 3,
    snapshot_cache_sweep_shard_04 = 4,
    snapshot_cache_sweep_shard_05 = 5,
    snapshot_cache_sweep_shard_06 = 6,
    snapshot_cache_sweep_shard_07 = 7,
    snapshot_cache_sweep_shard_08 = 8,
    snapshot_cache_sweep_shard_09 = 9,
    snapshot_cache_sweep_shard_10 = 10,
    snapshot_cache_sweep_shard_11 = 11,
    snapshot_cache_sweep_shard_12 = 12,
    snapshot_cache_sweep_shard_13 = 13,
    snapshot_cache_sweep_shard_14 = 14,
    snapshot_cache_sweep_shard_15 = 15,
    snapshot_cache_sweep_shard_16 = 16,
    snapshot_cache_sweep_shard_17 = 17,
    snapshot_cache_sweep_shard_18 = 18,
    snapshot_cache_sweep_shard_19 = 19,
    snapshot_cache_sweep_shard_20 = 20,
    snapshot_cache_sweep_shard_21 = 21,
    snapshot_cache_sweep_shard_22 = 22,
    snapshot_cache_sweep_shard_23 = 23,
}

/// Drift guard: `corpus_shard_tests!` above must enumerate EXACTLY
/// `0..CORPUS_SHARD_COUNT` — one `#[test]` fn per shard index, no gaps and no
/// out-of-range entries — or some corpus files would silently never be swept (a
/// gap) or `run_corpus_shard` would be invoked with an index that can never
/// match any file (dead weight). Asserted against `GENERATED_SHARD_INDICES` —
/// the array the macro emits FROM THE SAME repetition that generates the shard
/// `#[test]` fns — rather than a separately hand-maintained literal count:
/// deleting a `snapshot_cache_sweep_shard_NN` line shrinks
/// `GENERATED_SHARD_INDICES` too, so this guard actually fails when that drift
/// occurs (a literal-vs-literal comparison would not).
#[test]
fn corpus_shard_count_matches_generated_tests() {
    assert_eq!(
        GENERATED_SHARD_INDICES.len(),
        CORPUS_SHARD_COUNT,
        "corpus_shard_tests! generated {} shard test(s) but CORPUS_SHARD_COUNT is \
         {CORPUS_SHARD_COUNT} — every index in 0..CORPUS_SHARD_COUNT must have exactly \
         one snapshot_cache_sweep_shard_NN test, or some corpus files silently never \
         get swept",
        GENERATED_SHARD_INDICES.len()
    );

    // Stronger than a count match: pin the exact index SET too, so a
    // duplicate/out-of-range index masking a missing one (same count, wrong
    // coverage) can't slip through.
    let mut sorted_indices = GENERATED_SHARD_INDICES.to_vec();
    sorted_indices.sort_unstable();
    let expected: Vec<usize> = (0..CORPUS_SHARD_COUNT).collect();
    assert_eq!(
        sorted_indices, expected,
        "corpus_shard_tests! must enumerate EXACTLY 0..CORPUS_SHARD_COUNT — no gaps, \
         duplicates, or out-of-range indices — got {GENERATED_SHARD_INDICES:?}"
    );
}
