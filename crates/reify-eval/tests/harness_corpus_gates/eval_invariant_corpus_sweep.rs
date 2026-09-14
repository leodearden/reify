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

// ── Corpus enumeration ─────────────────────────────────────────

/// One corpus member: the absolute path used to READ the file, and the
/// repo-relative shard key, carried together.
///
/// The key travels with the path so no caller ever re-derives it — it is the
/// shard key, the residual-exemption match target and the scope discriminator,
/// and three independent derivations of one string is three chances to disagree.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CorpusFile {
    path: std::path::PathBuf,
    rel: String,
}

/// The full corpus: reify-eval's own `tests/fixtures/`, every `examples/` file,
/// and the ONE explicit #4946 R3f-bridge premise fixture.
///
/// CRITICAL — the prd-gate fixture is referenced as that explicit `<name>.ri`
/// LEAF and never as its directory. `verify.sh`'s `tests/prd-gate/fixtures/*.ri`
/// no-heavy carve-out rests on the premise that no `*.rs` globs that directory,
/// and `test_verify_scope.sh`'s PG-DRIFT-DIR guard reds on a directory
/// reference. Walking it would silently widen the carve-out's blast radius.
///
/// `files.sort()` is retained even though the sorted position no longer
/// determines the shard: listings, skip reports and diagnostics stay
/// deterministically ordered, which is what makes two runs' output diffable.
///
/// Cheap (a directory walk, no compilation) — recomputing it once per shard is
/// not worth caching, and keeps the shard processes free of shared state.
fn corpus_files() -> Vec<CorpusFile> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = workspace_root();

    let mut files = Vec::new();
    eval_gate_support::collect_ri_files(&manifest_dir.join("tests/fixtures"), &mut files);
    eval_gate_support::collect_ri_files(&manifest_dir.join("../../examples"), &mut files);
    files.push(manifest_dir.join("../../tests/prd-gate/fixtures/geometry_let_selector_consumer.ri"));

    let mut corpus: Vec<CorpusFile> = files
        .into_iter()
        .map(|path| {
            let rel = repo_relative(&path, &root);
            CorpusFile { path, rel }
        })
        .collect();
    corpus.sort();
    corpus
}

/// The corpus slice this shard owns. Every shard runs this independently, so the
/// partition must be derivable from the key alone — which is exactly what
/// [`shard_of`] gives, with no state shared between the shard processes.
fn shard_files(shard_index: usize) -> Vec<CorpusFile> {
    corpus_files()
        .into_iter()
        .filter(|f| shard_of(&f.rel) == shard_index)
        .collect()
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
    let corpus = corpus_files();
    assert!(
        corpus.len() > 250,
        "non-vacuity: expected the live corpus to hold hundreds of .ri files, got {}",
        corpus.len()
    );
    for CorpusFile { rel: key, .. } in &corpus {
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

// ── The shared evaluation core ────────────────────────────────────

/// The invariants this sweep asserts. A CLOSED enum, not a string tag: a typo
/// in a meaningful string would silently create a third, never-checked
/// "invariant", which is precisely the ad-hoc-string failure this codebase
/// gates against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvariantId {
    /// INV-EVAL-5, the no-stale-Undef invariant (task α, PRD
    /// `docs/prds/v0_6/eval-uniform-dependency-handling.md` §6.1).
    StaleUndef,
    /// INV-EVAL-4, the snapshot↔cache content-hash divergence audit (task ι, PRD
    /// `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.6 / §3 P3 / §7 B4).
    SnapshotCacheDivergence,
}

/// One invariant finding, in the shape BOTH checkers already report.
///
/// `reify_eval::StaleUndefViolation` and `reify_eval::SnapshotCacheDivergence`
/// each expose exactly `{ cell, detail }`, so this is a faithful common shape and
/// NOT a merge of the two checkers — it is the report type, not the logic.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Finding {
    cell: reify_core::ValueCellId,
    detail: String,
}

/// What one invariant made of one corpus file.
///
/// `OutOfScope` is a distinct variant rather than an empty finding list on
/// purpose: "this invariant does not cover this file" and "this invariant
/// covered this file and found nothing" are different facts, and collapsing them
/// would let a silently narrowed scope read as a clean pass.
#[derive(Clone, Debug, PartialEq, Eq)]
enum InvariantOutcome {
    OutOfScope,
    Checked(Vec<Finding>),
}

/// What one corpus file's single evaluation produced, per invariant.
///
/// Every invariant in [`GATES`] appears here for EVERY file, in scope or not, so
/// the shape is uniform across the corpus and a missing entry is unambiguously a
/// defect rather than a scope decision.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileOutcome {
    rel: String,
    per_invariant: Vec<(InvariantId, InvariantOutcome)>,
}

impl FileOutcome {
    /// This file's findings for `id`. `None` means this file was NOT checked for
    /// `id` — either it is out of that invariant's scope, or the sweep does not
    /// carry the invariant at all, which
    /// `one_corpus_evaluation_feeds_both_invariant_checkers` asserts never happens.
    /// `Some(&[])` is the materially different "checked, and clean".
    fn findings(&self, id: InvariantId) -> Option<&[Finding]> {
        match self.per_invariant.iter().find(|(i, _)| *i == id) {
            Some((_, InvariantOutcome::Checked(f))) => Some(f.as_slice()),
            _ => None,
        }
    }
}

/// Which corpus files an invariant covers.
///
/// Explicit, structured data rather than an implicit consequence of which file
/// the sweep used to live in — before unification each sweep's scope was simply
/// whatever directory walk its own `corpus_files()` happened to do, which is
/// exactly the kind of fact that gets lost in a move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CorpusScope {
    /// The whole corpus: reify-eval's `tests/fixtures/`, `examples/`, and the
    /// explicit prd-gate leaf. INV-EVAL-5's pre-unification scope.
    Union,
    /// `examples/` only.
    ///
    /// A FAITHFUL reproduction of INV-EVAL-4's pre-unification scope, NOT an
    /// oversight to be helpfully "fixed": that sweep walked `examples/` and
    /// nothing else. Widening it to the 34 reify-eval fixture files would be a
    /// coverage CHANGE that could surface fresh residuals, which is outside a
    /// zero-loss restructuring — named as a follow-up, deliberately not taken
    /// here. Pinned by `each_invariant_keeps_its_own_pre_unification_corpus_scope`.
    ExamplesOnly,
}

impl CorpusScope {
    /// Whether this scope covers the corpus file keyed `rel`.
    ///
    /// Matches on the repo-relative key's leading component — which is precisely
    /// why the key is normalised to a worktree-independent `/`-separated string
    /// (see [`repo_relative`]): an absolute path would carry a per-checkout
    /// prefix and `ExamplesOnly` would match nothing in any lane.
    fn covers(self, rel: &str) -> bool {
        match self {
            CorpusScope::Union => true,
            CorpusScope::ExamplesOnly => rel.starts_with("examples/"),
        }
    }
}

/// One invariant's declaration: what it is, what it is called in a report, which
/// corpus files it covers, which of those are exempt, and what happens when it
/// finds something.
///
/// The two invariants' residual lists and failure policies differ, and those
/// differences are DATA here rather than two hand-copied code branches: one
/// implementation reads these fields, so the policies cannot drift apart the way
/// the two sweeps' engine constructors did (task 5578).
struct InvariantGate {
    id: InvariantId,
    /// How this invariant names itself in sweep output, so a red says which
    /// invariant broke without the reader decoding an enum variant.
    label: &'static str,
    scope: CorpusScope,
    /// `(repo-relative path SUFFIX, root-cause reason)` for files with a
    /// residual finding that is NOT a checker gap. Every entry is PRINTED with
    /// its reason and finding count when it is exempted — never silent — so
    /// bounded coverage can never read as full coverage.
    residuals: &'static [(&'static str, &'static str)],
    /// Whether a declared residual that produces ZERO findings fails the sweep.
    ///
    /// INV-EVAL-4 shipped with this ON: a resolved residual is reported loud so
    /// the dead exemption gets deleted rather than lingering as weight that
    /// masks the now-recovered coverage. INV-EVAL-5 shipped with it OFF. That
    /// asymmetry is PRESERVED, not harmonised — turning it on for INV-EVAL-5
    /// would be a semantic change outside a zero-loss restructuring, and is
    /// named as a follow-up.
    stale_residual_is_fatal: bool,
    /// Break-glass env var that DOWNGRADES this invariant's failure to a warn.
    ///
    /// INV-EVAL-4 shipped with `REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS`, mirroring
    /// `REIFY_MAIN_GATE_BYPASS`, so a future change that introduces a divergence
    /// can never wedge the merge queue. INV-EVAL-5 shipped with none, and is not
    /// granted one here — same preservation argument as above.
    bypass_env: Option<&'static str>,
}

impl InvariantGate {
    /// The declared root-cause reason exempting `rel` from this invariant, if
    /// any. Matched by path SUFFIX against the repo-relative key, as both old
    /// sweeps matched against their display path.
    fn residual_reason(&self, rel: &str) -> Option<&'static str> {
        self.residuals
            .iter()
            .find(|(suffix, _)| rel.ends_with(suffix))
            .map(|(_, reason)| *reason)
    }

    /// Whether this invariant's failure is downgraded to a warn right now.
    ///
    /// Read at report time, not at declaration time, because the process
    /// environment is not knowable at `const` construction.
    fn bypassed(&self) -> bool {
        self.bypass_env
            .is_some_and(|key| std::env::var(key).is_ok_and(|v| v == "1"))
    }
}

/// Files with a residual stale-Undef violation that is NOT a checker gap fixable
/// within `invariants.rs`'s `(graph, values, trace_map, functions)` signature —
/// each traced to its root cause during the α broad-sweep investigation (task
/// 4952 step-10). Carried here VERBATIM from
/// `no_stale_undef_invariant_gate.rs`'s `KNOWN_RESIDUAL_SKIPS` by task #7431:
/// the reasons are root-cause RECORDS, not prose, and are neither paraphrased
/// nor merged with INV-EVAL-4's list. If a future engine change resolves one of
/// these, its entry should be deleted rather than left as dead weight.
const STALE_UNDEF_RESIDUALS: &[(&str, &str)] = &[
    (
        "examples/integration_corner_cases.ri",
        "RecTree.child.{span,depth}: a `sub child = RecTree(...) where depth > 0` \
         self-recursive sub. The compiler statically emits one placeholder level of \
         child value cells regardless of the runtime `where` guard's truth value, but \
         that guard's active/inactive state is a compiler-side concept never threaded \
         into the runtime EvaluationGraph (unlike value-cell-level `guard()` branches, \
         which DO get a GuardedGroupInfo entry). Fixing this needs a new \
         EvaluationGraph field populated from the compiler's sub-instantiation guard \
         info — a change to shared graph-construction code, out of this task's scope.",
    ),
    (
        "crates/reify-eval/tests/fixtures/match_block_decls_bolt.ri",
        "Bolt.head.across_flats: a decl-level `match head_type { ... => sub head: ... }` \
         block. The compiler tracks per-arm active/inactive state in \
         `TopologyTemplate::match_arm_groups` (`GuardedDeclGroup`), but \
         `EvaluationGraph::from_templates` does not carry that field into the runtime \
         graph at all (confirmed: no analogous field exists on EvaluationGraph). Same \
         class of gap as the RecTree entry above, for match blocks instead of `where` \
         guards — needs shared graph-construction plumbing, out of this task's scope.",
    ),
    (
        "examples/multi_load_bracket.ri",
        "MultiLoadBracket.critical_case: `worst_case(results, |r| r)` — a lambda-over-Map \
         combinator. Reproducibly hits a pre-existing reify-expr dispatch gap \
         (\"[reify-expr] sample: Field lambda is not a Lambda: Undef\", printed 3x during \
         this sweep — once per load case) unrelated to geometry, kinematics, or \
         dynamics. A worst_case/lambda-dispatch product limitation, not a staleness \
         false-positive this checker should paper over.",
    ),
    (
        "examples/surface_finish_functional.ri",
        "Demo.total: reads through `let bom = AssemblyBOM()` — a whole-structure VALUE \
         constructor call (not a `sub` declaration) for a structure that itself declares \
         nested subs (`sub p1 = Plate()`, `sub p2 = Bracket()`). Their finishing_cost \
         fields do not resolve when the parent is constructed as an inline value \
         expression rather than a `sub`. A pre-existing struct-constructor-with-nested- \
         subs eval limitation, independent of geometry/staleness.",
    ),
];

/// Files with a residual snapshot↔cache divergence that is a documented
/// eval-surface limitation, NOT a checker gap fixable within
/// `cache_divergence.rs`'s `(snapshot_values, cache, journal)` signature — each
/// root-caused during the ι broad-sweep investigation (step-8). Carried here
/// VERBATIM from `snapshot_cache_divergence_gate.rs`'s
/// `KNOWN_RESIDUAL_DIVERGENCE_SKIPS` by task #7431.
///
/// A residual that traces to a genuine post-γ snapshot↔cache write bug (not a
/// documented eval-surface limitation) is a design_concern to escalate, NOT a
/// residual entry to paper over.
const SNAPSHOT_CACHE_DIVERGENCE_RESIDUALS: &[(&str, &str)] = &[
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

/// Every invariant this sweep asserts, with its per-invariant scope.
///
/// Exactly two entries — pinned by
/// `one_corpus_evaluation_feeds_both_invariant_checkers`, so dropping one reds
/// rather than silently halving the coverage.
const GATES: &[InvariantGate] = &[
    InvariantGate {
        id: InvariantId::StaleUndef,
        label: "INV-EVAL-5 (no stale Undef)",
        scope: CorpusScope::Union,
        residuals: STALE_UNDEF_RESIDUALS,
        stale_residual_is_fatal: false,
        bypass_env: None,
    },
    InvariantGate {
        id: InvariantId::SnapshotCacheDivergence,
        label: "INV-EVAL-4 (snapshot↔cache divergence)",
        scope: CorpusScope::ExamplesOnly,
        residuals: SNAPSHOT_CACHE_DIVERGENCE_RESIDUALS,
        stale_residual_is_fatal: true,
        bypass_env: Some("REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS"),
    },
];

/// INV-EVAL-5's adapter: ONE `Engine` wrapper call, mapped to [`Finding`].
///
/// Deliberately a separate named function from its sibling below. Task 5060 is
/// explicit that the snapshot↔cache audit is a DISTINCT invariant and the two
/// checkers must NOT be merged; keeping one adapter per invariant holds that at
/// the code level, not just in `reify-eval`'s `src/`. What this file shares is
/// the expensive corpus EVALUATION — never the checking.
fn stale_undef_findings(engine: &reify_eval::Engine) -> Vec<Finding> {
    engine
        .check_no_stale_undef()
        .into_iter()
        .map(|v| Finding { cell: v.cell, detail: v.detail })
        .collect()
}

/// INV-EVAL-4's adapter — see [`stale_undef_findings`] for why these are two
/// functions and not one parameterised over a checker.
fn snapshot_cache_divergence_findings(engine: &reify_eval::Engine) -> Vec<Finding> {
    engine
        .check_snapshot_cache_divergence()
        .into_iter()
        .map(|d| Finding { cell: d.cell, detail: d.detail })
        .collect()
}

/// Run every invariant against ONE already-evaluated engine.
///
/// The engine arrives ALREADY eval'd and by shared reference, which is what
/// makes "exactly one eval per corpus file" structural rather than a convention
/// a future edit could quietly break: this function cannot evaluate anything,
/// so a second eval would have to be written somewhere a reader can see it.
///
/// Sound because `Engine::check_no_stale_undef` (`invariants.rs`) and
/// `Engine::check_snapshot_cache_divergence` (`cache_divergence.rs`) are both
/// `&self` reads of the same retained `eval_state()` snapshot the preceding
/// `eval()` installed, and neither mutates the engine — so one evaluation feeds
/// both, in either order, with no order-dependent result.
fn check_file(engine: &reify_eval::Engine, rel: &str) -> FileOutcome {
    let per_invariant = GATES
        .iter()
        .map(|gate| {
            let outcome = if gate.scope.covers(rel) {
                InvariantOutcome::Checked(match gate.id {
                    InvariantId::StaleUndef => stale_undef_findings(engine),
                    InvariantId::SnapshotCacheDivergence => {
                        snapshot_cache_divergence_findings(engine)
                    }
                })
            } else {
                InvariantOutcome::OutOfScope
            };
            (gate.id, outcome)
        })
        .collect();

    FileOutcome { rel: rel.to_string(), per_invariant }
}

/// The whole CPU saving in one assertion: BOTH invariants are asserted per file
/// off a SINGLE compile+eval.
///
/// Before unification each sweep compiled and evaluated its own copy of the
/// overlapping corpus — 299 evals for INV-EVAL-5 plus 264 for INV-EVAL-4 across
/// 48 test processes. The merge is sound because `Engine::check_no_stale_undef`
/// (`invariants.rs`) and `Engine::check_snapshot_cache_divergence`
/// (`cache_divergence.rs`) are both `&self` reads of the same retained
/// `eval_state()` snapshot, and neither mutates the engine.
///
/// `examples/fdm_bracket.ri` is the fixture deliberately: it is a live
/// KNOWN_RESIDUAL divergence entry, so at least one invariant returns a NON-empty
/// finding list here and (b)'s comparison cannot pass vacuously.
#[test]
fn one_corpus_evaluation_feeds_both_invariant_checkers() {
    let root = workspace_root();
    let rel = "examples/fdm_bracket.ri";
    let path = root.join(rel);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let compiled = reify_test_support::compile_source_with_stdlib(&source);
    let errors = reify_test_support::collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "{rel} must compile cleanly: {errors:#?}");

    // ONE engine, ONE eval — everything below reads that single snapshot.
    let mut engine = eval_gate_support::gate_engine(true);
    engine.eval(&compiled);

    let outcome = check_file(&engine, rel);

    // (a) Exactly two invariants, exactly these two. A future edit that drops one
    //     from the sweep reds HERE, instead of silently halving the coverage.
    let ids: Vec<InvariantId> = outcome.per_invariant.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids,
        vec![InvariantId::StaleUndef, InvariantId::SnapshotCacheDivergence],
        "every corpus file must be checked for BOTH invariants, no more and no fewer"
    );
    assert_eq!(outcome.rel, rel, "the outcome must carry its own corpus key");

    // (b) The adapters neither filter nor reorder: each invariant's findings are
    //     exactly what the Engine wrapper returns on this same engine.
    let direct_stale: Vec<reify_core::ValueCellId> =
        engine.check_no_stale_undef().into_iter().map(|v| v.cell).collect();
    let direct_divergence: Vec<reify_core::ValueCellId> = engine
        .check_snapshot_cache_divergence()
        .into_iter()
        .map(|d| d.cell)
        .collect();

    let cells = |id: InvariantId| -> Vec<reify_core::ValueCellId> {
        outcome
            .findings(id)
            .expect("both invariants present per (a)")
            .iter()
            .map(|f| f.cell.clone())
            .collect()
    };
    assert_eq!(cells(InvariantId::StaleUndef), direct_stale);
    assert_eq!(cells(InvariantId::SnapshotCacheDivergence), direct_divergence);

    assert!(
        !direct_stale.is_empty() || !direct_divergence.is_empty(),
        "non-vacuity: {rel} was chosen because it is a live KNOWN_RESIDUAL entry, so \
         at least one invariant must report findings here — otherwise (b) compares \
         two empty lists and proves nothing about filtering or reordering"
    );

    // (c) No order dependence, in either direction. Both wrappers are `&self`
    //     reads of the same retained snapshot, so neither running the routine
    //     twice nor swapping the two checkers' order may change the result. This
    //     is the "no new order-dependent red" half of the user-observable signal.
    assert_eq!(
        check_file(&engine, rel),
        outcome,
        "check_file must be a pure read of the post-eval engine"
    );
    let reversed_divergence: Vec<reify_core::ValueCellId> = engine
        .check_snapshot_cache_divergence()
        .into_iter()
        .map(|d| d.cell)
        .collect();
    let reversed_stale: Vec<reify_core::ValueCellId> =
        engine.check_no_stale_undef().into_iter().map(|v| v.cell).collect();
    assert_eq!(
        (reversed_stale, reversed_divergence),
        (direct_stale, direct_divergence),
        "running the two checkers in the reverse order must give identical findings"
    );
}

/// What makes this restructuring ZERO-LOSS rather than a coverage change: the
/// two old sweeps did NOT cover the same corpus, and unification must not
/// quietly widen either one.
///
/// INV-EVAL-5 swept the 299-file UNION (reify-eval's `tests/fixtures/` +
/// `examples/` + the prd-gate leaf). INV-EVAL-4 swept `examples/` ONLY. Sharing
/// the evaluation is free; sharing the SCOPE would not be — widening INV-EVAL-4
/// to the 34 fixture files is a coverage CHANGE that could surface fresh
/// residuals, which is outside a zero-loss restructuring. It is deliberately NOT
/// done here and is named as a follow-up instead, so a future reader cannot
/// mistake the narrower scope for an oversight.
#[test]
fn each_invariant_keeps_its_own_pre_unification_corpus_scope() {
    let scope = |id: InvariantId| {
        GATES
            .iter()
            .find(|g| g.id == id)
            .unwrap_or_else(|| panic!("no gate declared for {id:?}"))
            .scope
    };
    let stale = scope(InvariantId::StaleUndef);
    let divergence = scope(InvariantId::SnapshotCacheDivergence);

    // (a) A reify-eval fixture: INV-EVAL-5 only.
    let fixture = "crates/reify-eval/tests/fixtures/undef_trace.ri";
    assert!(stale.covers(fixture), "{fixture} was in the INV-EVAL-5 corpus");
    assert!(
        !divergence.covers(fixture),
        "{fixture} was NOT in the INV-EVAL-4 corpus (examples/ only) — covering it \
         now would be a coverage change, not a restructuring"
    );

    // (b) The explicit #4946 prd-gate leaf: likewise INV-EVAL-5 only.
    let leaf = "tests/prd-gate/fixtures/geometry_let_selector_consumer.ri";
    assert!(stale.covers(leaf), "{leaf} was in the INV-EVAL-5 corpus");
    assert!(!divergence.covers(leaf), "{leaf} was NOT in the INV-EVAL-4 corpus");

    // (c) An examples/ member: BOTH, including a nested one.
    for example in ["examples/fdm_bracket.ri", "examples/auto/bearing_constraint_select.ri"] {
        assert!(stale.covers(example), "{example} was in the INV-EVAL-5 corpus");
        assert!(divergence.covers(example), "{example} was in the INV-EVAL-4 corpus");
    }

    // (d) Counted over the LIVE corpus, so neither invariant can silently gain or
    //     lose files as the corpus grows.
    let corpus = corpus_files();
    let examples: Vec<&CorpusFile> = corpus
        .iter()
        .filter(|f| f.rel.starts_with("examples/"))
        .collect();
    assert!(
        !examples.is_empty() && examples.len() < corpus.len(),
        "non-vacuity: the corpus must hold both examples/ and non-examples/ members \
         ({} of {}), or (d) cannot distinguish the two scopes",
        examples.len(),
        corpus.len()
    );

    let covered = |s: CorpusScope| corpus.iter().filter(|f| s.covers(&f.rel)).count();
    assert_eq!(
        covered(stale),
        corpus.len(),
        "INV-EVAL-5 swept the whole union before unification and must still do so"
    );
    assert_eq!(
        covered(divergence),
        examples.len(),
        "INV-EVAL-4 swept exactly the examples/ subset before unification and must \
         still do so — no wider, no narrower"
    );
}

/// The second and third asymmetries the two old gates carry, pinned so
/// unification cannot silently harmonise them.
///
/// Sharing the corpus EVALUATION is free. Sharing the residual list or the
/// failure policy would not be: a single global exemption list would stop
/// checking `examples/fdm_bracket.ri` for INV-EVAL-5, which it is NOT exempt
/// from, and harmonising the policies would either strengthen INV-EVAL-5 to fail
/// on a stale residual or weaken INV-EVAL-4's break-glass — each a semantic
/// change outside a zero-loss restructuring, and each named as a follow-up
/// instead of taken here.
#[test]
fn residual_exemptions_and_failure_policy_stay_per_invariant() {
    let gate = |id: InvariantId| {
        GATES
            .iter()
            .find(|g| g.id == id)
            .unwrap_or_else(|| panic!("no gate declared for {id:?}"))
    };
    let stale = gate(InvariantId::StaleUndef);
    let divergence = gate(InvariantId::SnapshotCacheDivergence);

    // (a) The two residual lists are SEPARATE. Each of these files is exempt
    //     from ONE invariant and must still be CHECKED for the other — a single
    //     global exemption list reds here.
    let fdm = "examples/fdm_bracket.ri";
    assert!(
        divergence.residual_reason(fdm).is_some(),
        "{fdm} is a declared INV-EVAL-4 residual"
    );
    assert!(
        stale.residual_reason(fdm).is_none() && stale.scope.covers(fdm),
        "{fdm} is NOT exempt from INV-EVAL-5 and must still be checked for it"
    );

    let multi_load = "examples/multi_load_bracket.ri";
    assert!(
        stale.residual_reason(multi_load).is_some(),
        "{multi_load} is a declared INV-EVAL-5 residual"
    );
    assert!(
        divergence.residual_reason(multi_load).is_none() && divergence.scope.covers(multi_load),
        "{multi_load} is NOT exempt from INV-EVAL-4 and must still be checked for it"
    );

    // (b) Residuals match by path SUFFIX against the repo-relative key, and every
    //     declared entry matches EXACTLY ONE live corpus file. An entry matching
    //     zero files is dead weight that masks nothing and hides that the
    //     coverage it documented has silently moved or vanished; an entry
    //     matching several would exempt files nobody root-caused.
    let corpus = corpus_files();
    assert_eq!(stale.residuals.len(), 4, "INV-EVAL-5 declared 4 residuals");
    assert_eq!(divergence.residuals.len(), 2, "INV-EVAL-4 declared 2 residuals");
    for g in GATES {
        for (suffix, reason) in g.residuals {
            let matched: Vec<&str> = corpus
                .iter()
                .filter(|f| f.rel.ends_with(suffix))
                .map(|f| f.rel.as_str())
                .collect();
            assert_eq!(
                matched.len(),
                1,
                "{}: residual {suffix:?} must match exactly one live corpus file, \
                 matched {matched:?} — a renamed .ri must red here rather than \
                 silently voiding the exemption",
                g.label
            );
            assert!(
                !reason.trim().is_empty(),
                "{}: residual {suffix:?} must carry its root-cause reason",
                g.label
            );
            assert!(
                g.scope.covers(matched[0]),
                "{}: residual {suffix:?} exempts {} from an invariant that does not \
                 even cover it — dead weight",
                g.label,
                matched[0]
            );
        }
    }

    // (c) The failure POLICY is per-invariant, read from the declared fields
    //     rather than from the process environment (no env mutation: these tests
    //     run concurrently in-process with the shard tests).
    assert_eq!(
        divergence.bypass_env,
        Some("REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS"),
        "INV-EVAL-4 shipped with a break-glass warn-downgrade knob"
    );
    assert!(
        divergence.stale_residual_is_fatal,
        "INV-EVAL-4 fails the sweep on a residual that no longer diverges, so the \
         dead exemption gets deleted instead of masking recovered coverage"
    );
    assert_eq!(
        stale.bypass_env, None,
        "INV-EVAL-5 shipped with NO bypass knob — granting it one here would be a \
         semantic change, not a restructuring"
    );
    assert!(
        !stale.stale_residual_is_fatal,
        "INV-EVAL-5 merely PRINTS its residuals; making them fatal here would be a \
         semantic change, not a restructuring"
    );
}
