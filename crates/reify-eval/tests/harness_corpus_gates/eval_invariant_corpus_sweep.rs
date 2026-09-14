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

// ── The repo-relative shard key ────────────────────────────────────

/// The repo root, resolved from this crate's manifest dir.
///
/// Canonicalised, so it is directly comparable with a canonicalised corpus path
/// no matter how many `..` hops or symlinks either side carries.
fn workspace_root() -> std::path::PathBuf {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../..");
    root.canonicalize().unwrap_or(root)
}

/// `path`, expressed relative to `root`, as the `/`-separated string used as the
/// shard key everywhere in this file.
///
/// This normalisation is what makes a file's shard stable across this repo's
/// linked worktrees: the absolute prefix, which differs per checkout, is
/// stripped, so only the part that is genuinely the same in every checkout is
/// hashed. Hashing a `Path`/`OsStr` directly would reintroduce that prefix and
/// silently reassign the whole corpus per lane.
///
/// Canonicalising both sides, rather than hand-rolling a `..` collapser, is what
/// resolves the hops the corpus-root joins introduce
/// (`crates/reify-eval/../../examples/x.ri` → `examples/x.ri`) and any symlink on
/// either side. A side that does not exist on this filesystem — a synthetic path
/// in a test — falls back to itself, which is lexically correct for a path that
/// carries no `..` to begin with.
///
/// Returns an owned `String`: the shard key is a VALUE, not a borrow into a path
/// buffer whose lifetime a caller would then have to manage.
fn repo_relative(path: &std::path::Path, root: &std::path::Path) -> String {
    fn resolved(p: &std::path::Path) -> std::path::PathBuf {
        p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
    }

    let path = resolved(path);
    let root = resolved(root);
    let rel = path.strip_prefix(&root).unwrap_or_else(|_| {
        panic!(
            "corpus path {} does not sit under the repo root {} — the shard key \
             would carry a per-checkout prefix",
            path.display(),
            root.display()
        )
    });

    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

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
/// hand-picked literals. It routes through [`repo_relative`], so there is still
/// exactly ONE relativizer in this file.
fn live_corpus_keys() -> Vec<String> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = workspace_root();

    let mut files = Vec::new();
    eval_gate_support::collect_ri_files(&manifest_dir.join("tests/fixtures"), &mut files);
    eval_gate_support::collect_ri_files(&root.join("examples"), &mut files);
    files.push(root.join("tests/prd-gate/fixtures/geometry_let_selector_consumer.ri"));

    files.iter().map(|p| repo_relative(p, &root)).collect()
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

/// The trap that makes hash keying CORRECT in this repo: the shard key must be
/// a REPO-RELATIVE path, never the absolute one.
///
/// Every corpus root here is derived from `env!("CARGO_MANIFEST_DIR")`, whose
/// prefix differs per checkout — `/home/leo/src/reify` in the main checkout
/// versus `/home/leo/src/warm-lanes/worktrees/_lane-N` in each of the 235 linked
/// worktrees. Hashing the absolute path would therefore give the SAME `.ri` file
/// a DIFFERENT shard in every lane, destroying exactly the reproducibility
/// `shard_of` exists to buy: "shard 7 reds" would not transfer from the lane
/// that found it to the checkout someone reproduces it in.
#[test]
fn shard_key_is_worktree_independent() {
    // (a) Two checkouts of the same repo, same file. Deliberately synthetic and
    //     non-existent on this filesystem, which is what makes the assertion
    //     deterministic everywhere: neither side canonicalizes, so this pins the
    //     prefix-stripping alone. Part (b) below covers the canonicalizing path.
    let main_root = std::path::Path::new("/nonexistent-checkout/reify");
    let lane_root = std::path::Path::new("/nonexistent-checkout/warm-lanes/worktrees/_lane-7");
    let main_path = main_root.join("examples/fdm_bracket.ri");
    let lane_path = lane_root.join("examples/fdm_bracket.ri");

    assert_ne!(
        main_path, lane_path,
        "control: the two absolute paths must genuinely differ, or (a) proves nothing"
    );

    let from_main = repo_relative(&main_path, main_root);
    let from_lane = repo_relative(&lane_path, lane_root);
    assert_eq!(
        from_main, from_lane,
        "the same corpus file must key identically from any checkout"
    );
    assert_eq!(from_main, "examples/fdm_bracket.ri");
    assert_eq!(
        shard_of(&from_main),
        shard_of(&from_lane),
        "...and therefore land in the same shard in every worktree"
    );

    // Non-vacuity control: hashing the ABSOLUTE paths would have disagreed.
    assert_ne!(
        shard_of(&main_path.to_string_lossy()),
        shard_of(&lane_path.to_string_lossy()),
        "control: absolute-path keying must disagree across checkouts, or (a) is \
         not pinning anything"
    );

    // (b) Real live corpus paths — these carry the `../..` hops the corpus-root
    //     joins introduce, so this is the canonicalizing half.
    let root = workspace_root();
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let live: Vec<std::path::PathBuf> = vec![
        manifest_dir.join("tests/fixtures/undef_trace.ri"),
        manifest_dir.join("../../examples/fdm_bracket.ri"),
        manifest_dir.join("../../tests/prd-gate/fixtures/geometry_let_selector_consumer.ri"),
    ];
    let expected_roots = [
        "examples/",
        "crates/reify-eval/tests/fixtures/",
        "tests/prd-gate/fixtures/",
    ];
    for path in &live {
        assert!(path.exists(), "premise: {} must exist", path.display());
        let rel = repo_relative(path, &root);
        assert!(
            !rel.starts_with('/'),
            "{rel} must be relative — a leading / means the prefix was not stripped"
        );
        assert!(
            !rel.split('/').any(|c| c == ".."),
            "{rel} must carry no `..` component — the corpus-root joins introduce \
             `crates/reify-eval/../../examples/x.ri`, which must normalise to \
             `examples/x.ri` or two spellings of one file would key to two shards"
        );
        assert!(
            expected_roots.iter().any(|p| rel.starts_with(p)),
            "{rel} must sit under one of the three corpus roots {expected_roots:?}"
        );
    }

    // (c) `/` separators verbatim, so the key is exactly what a reader would
    //     type and what `KNOWN_RESIDUAL_*` suffixes are written against.
    assert_eq!(
        repo_relative(&manifest_dir.join("../../examples/fdm_bracket.ri"), &root),
        "examples/fdm_bracket.ri"
    );
    assert_eq!(
        repo_relative(&manifest_dir.join("tests/fixtures/undef_trace.ri"), &root),
        "crates/reify-eval/tests/fixtures/undef_trace.ri"
    );
}

/// Hash keying is a sound REPLACEMENT for index keying, not merely a different
/// one: it must still partition the corpus exhaustively, leave no shard idle,
/// and stay balanced enough that no shard becomes the binary's straggler.
///
/// # Why these bounds, and not a guess
///
/// (a) is the deterministic core and can never flake: it is what proves no file
/// silently stops being swept — the failure both old
/// `corpus_shard_count_matches_generated_tests` guards existed to prevent.
///
/// (b) and (c) are statistical, so their thresholds are MEASURED rather than
/// picked. Over the real 299 relative corpus paths, three independent
/// well-distributed 128-bit hashes (blake2b-128, sha256[:16], md5) gave a max
/// shard of 19 / 21 / 20 and a min of 7 / 6 / 8 against a mean of 12.46. A
/// 20 000-trial Monte-Carlo of multinomial(299, 24) gave max-bucket p50 20,
/// p90 22, p99 25, p99.9 28, absolute max 32; and P(some shard empty) is
/// 24·(23/24)^299 ≈ 9.3e-5. xxh3-128 is a well-distributed hash, so its residues
/// mod 24 are statistically indistinguishable from those samples.
///
/// The bound in (c) — 3× the ceiling mean, = 39 at 299 files — therefore clears
/// every measured point sample by ≥1.85× and the simulated absolute max by
/// 1.22×, while still reddening on the pathologies that actually matter: a
/// constant-keyed hash (all 299 in one shard) or a truncated modulus (shards
/// left empty). It is stated as a FORMULA over the live corpus, so growing the
/// corpus cannot make it fragile.
#[test]
fn hash_sharding_partitions_the_corpus_within_measured_bounds() {
    let corpus = corpus_files();
    let total = corpus.len();
    assert!(
        total > 250,
        "non-vacuity: expected hundreds of corpus files, got {total}"
    );

    let shards: Vec<Vec<CorpusFile>> = (0..CORPUS_SHARD_COUNT).map(shard_files).collect();
    let sizes: Vec<usize> = shards.iter().map(Vec::len).collect();
    let histogram = || {
        sizes
            .iter()
            .enumerate()
            .map(|(i, n)| format!("  shard {i:02}: {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // (a) EXHAUSTIVE and DISJOINT: the shards are a partition of the corpus.
    let mut swept: Vec<String> = shards
        .iter()
        .flat_map(|s| s.iter().map(|f| f.rel.clone()))
        .collect();
    let mut expected: Vec<String> = corpus.iter().map(|f| f.rel.clone()).collect();
    swept.sort();
    expected.sort();
    assert_eq!(
        swept.len(),
        total,
        "the shards must hold each corpus file EXACTLY once — {} slot(s) across \
         {CORPUS_SHARD_COUNT} shards for {total} file(s) means a file is swept \
         twice or not at all:\n{}",
        swept.len(),
        histogram()
    );
    assert_eq!(
        swept,
        expected,
        "the union of every shard must be exactly corpus_files() — a file missing \
         here is a file that is never swept by any invariant:\n{}",
        histogram()
    );

    // (b) NO DEAD SHARD: an empty shard means a `#[test]` fn that can never
    //     observe anything, and (under hash keying) files scattered invisibly
    //     rather than an obvious arithmetic hole.
    let min = *sizes.iter().min().expect("CORPUS_SHARD_COUNT > 0");
    assert!(
        min >= 1,
        "every shard must own at least one file; shard sizes:\n{}\n\
         (P(some shard empty) at {total} files over {CORPUS_SHARD_COUNT} shards is \
         ~9.3e-5, so this is a truncated modulus or a degenerate key, not bad luck)",
        histogram()
    );

    // (c) BALANCE: bounded at 3x the ceiling mean over the LIVE corpus.
    let ceiling_mean = total.div_ceil(CORPUS_SHARD_COUNT);
    let bound = 3 * ceiling_mean;
    let max = *sizes.iter().max().expect("CORPUS_SHARD_COUNT > 0");
    assert!(
        max <= bound,
        "the largest shard holds {max} file(s), over the bound of {bound} \
         (3 x ceil({total}/{CORPUS_SHARD_COUNT})). Measured point samples over this \
         corpus with three independent 128-bit hashes were 19/21/20, so a breach \
         here is a degenerate key (a constant hash puts every file in one shard), \
         not ordinary variance; shard sizes:\n{}",
        histogram()
    );

    eprintln!(
        "corpus partition: {total} file(s) over {CORPUS_SHARD_COUNT} shards \
         (min {min}, max {max}, bound {bound})\n{}",
        histogram()
    );
}
