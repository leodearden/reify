//! The unified corpus-wide invariant sweep: ONE compile+eval per `.ri` corpus
//! file, asserting BOTH post-eval invariants off that single evaluation.
//!
//! Task #7431 merged two independently-sharded 24-way sweeps that were each
//! doing their own full compile+eval of an overlapping corpus:
//!
//! - `no_stale_undef_invariant_gate::broad_corpus_sweep_shard_NN` — INV-EVAL-5,
//!   the no-stale-Undef invariant (task α, PRD
//!   `docs/prds/v0_6/eval-uniform-dependency-handling.md` §6.1), over the UNION
//!   of reify-eval's `tests/fixtures/`, `examples/`, and the one explicit
//!   `tests/prd-gate/fixtures/geometry_let_selector_consumer.ri` leaf.
//! - `harness_cache::snapshot_cache_divergence_gate::snapshot_cache_sweep_shard_NN`
//!   — INV-EVAL-4, the snapshot↔cache content-hash divergence audit (task ι, PRD
//!   `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.6 / §3 P3 / §7 B4), over
//!   `examples/` only.
//!
//! The merge is sound because `Engine::check_no_stale_undef` (`invariants.rs`)
//! and `Engine::check_snapshot_cache_divergence` (`cache_divergence.rs`) both
//! take `&self` and both read the SAME retained `eval_state()` snapshot the
//! preceding `eval()` installed; neither mutates the engine. One `eval()` can
//! therefore feed both checkers, in either order, with no order-dependent
//! result. What is shared is the corpus EVALUATION — never the checking.
//!
//! # The two CHECKERS stay distinct (task 5060)
//!
//! Task 5060 is explicit that the snapshot↔cache divergence audit is a DISTINCT
//! invariant and the checkers must NOT be merged. They are not: each invariant
//! keeps its own adapter fn, its own corpus SCOPE, its own residual-exemption
//! list and its own failure POLICY, all carried as data on a per-invariant
//! descriptor. This unit shares the expensive part (compile + eval) and nothing
//! else.
//!
//! # Anti-silent-accept guards
//!
//! A corpus sweep asserting "zero findings" is vacuous unless the checkers are
//! independently observed to FIRE. Those seeded-violation self-tests are
//! deliberately RETAINED in the two original units rather than moved here, and
//! they run in the same nextest pass:
//!
//! - `no_stale_undef_invariant_gate::seeded_stale_undef_violation_is_reported`
//! - `no_stale_undef_invariant_gate::seeded_stale_undef_composition_violation_is_reported`
//! - `no_stale_undef_invariant_gate::seeded_composition_over_unresolved_cross_cell_operand_is_exempted_by_dependency_clause`
//! - `no_stale_undef_invariant_gate::seeded_solid_boolean_union_undef_is_exempted_by_geometry_clause`
//! - `no_stale_undef_invariant_gate::seeded_kind_mismatch_composition_undef_is_unexempted_without_caller_discipline`
//! - `harness_cache::snapshot_cache_divergence_gate::seeded_divergence_is_reported`
//! - `harness_cache::snapshot_cache_divergence_gate::seeded_skip_committed_divergence_is_exempted`

use crate::eval_gate_support;

// ── Shard keying ──────────────────────────────────────────────────

/// Number of shards the corpus sweep is split across — one shard per
/// `corpus_sweep_shard_NN` `#[test]` fn below.
///
/// Kept at 24, the count BOTH pre-unification sweeps used, and for their
/// reason verbatim. The user-observable debug-gate signal is that every corpus
/// `.ri` file produces zero findings for the invariants in its scope. Running
/// that as ONE sequential test takes long enough — as the last test left
/// running, with nothing to interleave its output with — to trip the verify
/// pipeline's heartbeat-idle backstop despite every file passing (task 4952
/// debugged exactly that). Sharding into independent `#[test]` fns lets
/// cargo-nextest schedule them as separate, concurrently-run processes, each
/// reporting its own PASS/SLOW line, so the worst-case silent gap is bounded by
/// one shard's share of the corpus rather than the whole corpus.
///
/// Unification does not disturb that rationale: this sweep does ONE compile+eval
/// per file where the two old sweeps each did their own, so a shard's wall time
/// is if anything lower than either predecessor's at the same shard count.
/// Re-tuning the count is a separate, measurement-driven change.
const CORPUS_SHARD_COUNT: usize = 24;

/// Which shard owns `rel_path` — keyed on a hash of the repo-relative path, NOT
/// on the file's index in a sorted corpus listing.
///
/// Index keying (what both pre-unification sweeps used) is perfectly balanced
/// but insert-unstable: one added, deleted or renamed `.ri` file shifts every
/// later index and so reassigns roughly 23/24 of the corpus. Hash keying
/// reassigns ONLY the file that changed, which is what makes "this reproduction
/// is in shard 7" survive an unrelated corpus edit — the concrete win, pinned by
/// `shard_of_is_independent_of_corpus_membership`. The balance it costs is
/// bounded and measured by
/// `hash_sharding_partitions_the_corpus_within_measured_bounds`.
///
/// [`reify_core::ContentHash`] rather than `std::collections::hash_map::DefaultHasher`
/// because xxh3-128 is a FIXED algorithm: a path's shard is then reproducible
/// across toolchain versions and across this repo's linked worktrees, which is
/// the entire point. `DefaultHasher`'s output is explicitly not guaranteed
/// stable across Rust releases, so it would silently repartition the corpus on a
/// toolchain bump. `reify-core` is already a reify-eval dev-dependency, so this
/// reuse adds no dependency.
///
/// The key MUST be repo-relative — see [`repo_relative`].
fn shard_of(rel_path: &str) -> usize {
    (reify_core::ContentHash::of_str(rel_path).0 % CORPUS_SHARD_COUNT as u128) as usize
}

/// Every live corpus `.ri` file, as repo-relative shard keys.
///
/// S1-local scaffolding: the real `corpus_files()` lands later in this file and
/// carries its key alongside the absolute path. Until then this walk is what
/// gives the range property below real data (299 paths today) instead of three
/// hand-picked literals.
fn live_corpus_keys() -> Vec<String> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../..").canonicalize().expect("workspace root");

    let mut files = Vec::new();
    eval_gate_support::collect_ri_files(&manifest_dir.join("tests/fixtures"), &mut files);
    eval_gate_support::collect_ri_files(&root.join("examples"), &mut files);
    files.push(root.join("tests/prd-gate/fixtures/geometry_let_selector_consumer.ri"));

    files
        .iter()
        .map(|p| {
            let canonical = p.canonicalize().unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            canonical
                .strip_prefix(&root)
                .unwrap_or(&canonical)
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// The headline behaviour change task #7431 makes: a corpus file's shard is a
/// function of the FILE ALONE, not of its position in a sorted corpus listing.
///
/// Both pre-unification sweeps keyed their shards `i % CORPUS_SHARD_COUNT` on
/// the file's index in a sorted discovery walk. That is perfectly balanced but
/// insert-UNSTABLE: adding, deleting or renaming ONE `.ri` file shifts every
/// later file by one index and therefore reassigns roughly 23/24 of the corpus
/// to a different shard. A reproduction recorded as "shard 7 reds" stops being
/// findable the moment anyone touches the corpus. Hash keying trades a little
/// balance (quantified in `hash_sharding_partitions_the_corpus_within_measured_bounds`)
/// for the property that a corpus edit reassigns only the edited file.
#[test]
fn shard_of_is_independent_of_corpus_membership() {
    // (a) A pure function of the key: same input, same shard, every time, and
    //     for two independently-constructed equal `&str`s (so the result cannot
    //     be keyed on a pointer, a length, or interning).
    let target = "examples/fdm_bracket.ri";
    let rebuilt: String = ["examples/", "fdm_bracket", ".ri"].concat();
    assert_eq!(rebuilt, target, "the rebuilt key must be equal by value");
    assert_eq!(
        shard_of(target),
        shard_of(target),
        "shard_of must be deterministic across repeated calls"
    );
    assert_eq!(
        shard_of(target),
        shard_of(&rebuilt),
        "shard_of must depend on the key's VALUE, not on which allocation it came from"
    );

    // (b) No list, anywhere, at any size, can move the target's shard.
    let expected = shard_of(target);
    let mut corpora: Vec<Vec<String>> = Vec::new();
    for earlier_siblings in [0usize, 1, 40] {
        // Synthetic siblings under `crates/` all sort BEFORE `examples/...`.
        let mut corpus: Vec<String> = (0..earlier_siblings)
            .map(|n| format!("crates/reify-eval/tests/fixtures/synthetic_{n:03}.ri"))
            .collect();
        corpus.push(target.to_string());
        corpus.sort();
        assert_eq!(
            shard_of(target),
            expected,
            "inserting {earlier_siblings} earlier-sorting sibling(s) must not move \
             {target}'s shard — the whole point of hash keying"
        );
        corpora.push(corpus);
    }

    // Non-vacuity control for (b): under the OLD `i % CORPUS_SHARD_COUNT` index
    // keying those same three corpora DO disagree about the target's shard, so
    // the assertion above is discriminating rather than trivially true.
    let index_keyed: Vec<usize> = corpora
        .iter()
        .map(|c| c.iter().position(|p| p == target).expect("target present") % CORPUS_SHARD_COUNT)
        .collect();
    assert!(
        index_keyed.iter().any(|s| *s != index_keyed[0]),
        "control failed: the three corpora must disagree under index keying \
         (got {index_keyed:?}), or (b) proves nothing"
    );

    // (c) Range property, over the live corpus rather than hand-picked literals.
    let keys = live_corpus_keys();
    assert!(
        keys.len() > 250,
        "non-vacuity: expected the live corpus to hold hundreds of .ri files, got {}",
        keys.len()
    );
    for key in &keys {
        let shard = shard_of(key);
        assert!(
            shard < CORPUS_SHARD_COUNT,
            "shard_of({key}) returned {shard}, outside 0..{CORPUS_SHARD_COUNT} — no \
             corpus_shard_tests! entry would ever run that file"
        );
    }

    // A literal from the OTHER corpus root, so (c) is not examples/-only.
    assert!(shard_of("crates/reify-eval/tests/fixtures/undef_trace.ri") < CORPUS_SHARD_COUNT);
}
