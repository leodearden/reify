//! Generator-seam tests for the PDIAG codes-mandatory ratchet (task #5405).
//!
//! `pdiag::check` answers "is the diff RED?" — a *delta* question. The baseline
//! generator asks the orthogonal *census* question: how many code-less
//! `Diagnostic::error(...)` / `Diagnostic::warning(...)` sites does each swept
//! file carry right now? `check`'s `Vec<Finding>` cannot answer it — a file
//! sitting exactly at its baseline row emits nothing at all, and the four
//! `RatchetVerdict` variants carry deltas, not a complete census.
//!
//! So `pdiag` exposes exactly one extra seam for the generator,
//! [`reify_audit::pdiag::live_counts`] — taken by the generator through
//! [`reify_audit::pdiag::census_summary`], which is that same census plus the
//! swept-file total the generator refuses on — and the generator is a thin
//! renderer over it. That is the PRD §6.6 "derivation lives in ONE place" invariant,
//! mirroring how `ptodo-baseline-gen` calls `ptodo::fingerprint` rather than
//! re-deriving fingerprints: generation and enforcement read the SAME scan, so
//! a regenerated baseline can never disagree with the ratchet that checks it.
//!
//! The companion seam is [`reify_audit::pdiag::parse_baseline`], made public so
//! the manifest grammar can be driven directly from tests rather than only
//! through whatever rows the committed file happens to contain.
//!
//! ## Test tiers
//!
//! (a/b) **seam tests** — hermetic tempdir fixtures pinning `live_counts`'s
//!   census contract, and the `render_baseline` → `parse_baseline` round trip
//!   over the REAL renderer the generator binary itself calls.
//!
//! (c) **binary tests** — the real `pdiag-baseline-gen` spawned over a staged
//!   git fixture, covering the residue the seam cannot reach: argument
//!   rejection, stdout purity, and the `swept == 0` refusal. That refusal is an
//!   ORDERING property (it must precede the render, because the shell truncates
//!   the redirect target before the process starts), and ordering is invisible
//!   to every library-level test here.
//!
//! (A) **`baseline_exists_and_parses`** — always-on, hermetic. Resolves
//!   `crates/reify-audit/pdiag-baseline.txt` via `CARGO_MANIFEST_DIR` (so it
//!   works in any worktree), asserts the file EXISTS, and runs the real
//!   `parse_baseline` over it. Existence is asserted rather than skipped-on:
//!   `pdiag::check` treats an unreadable manifest as an EMPTY one, which is the
//!   fail-LOUD direction for the ratchet but would let a deleted or renamed
//!   manifest slip past *this* test vacuously.
//!
//!   **`the_committed_baseline_carries_the_generated_preamble`** joins it in
//!   this tier: also always-on, and also main-INDEPENDENT — it compares the
//!   committed bytes against the crate's own `pdiag::BASELINE_HEADER` and never
//!   reads the working tree's diagnostics. It guards the one drift class
//!   `parse_baseline` structurally cannot see, since `#` lines are comments to
//!   the grammar: a hand-edit that strips the preamble carrying the regen
//!   command, the policy pointer and the "regenerating is NOT a remediation"
//!   warning.
//!
//! (A′) **`rejects_*`** — always-on, hermetic. Drives crafted content straight
//!   through the same `parse_baseline`, so every grammar rule keeps real
//!   coverage independent of what the committed file holds. PDIAG's manifest
//!   ships POPULATED (unlike PTODO's zero-residual empty one), so (A) is
//!   non-vacuous today — but migration is opportunistic per PRD §6.7, and these
//!   rules must not go inert as rows burn down toward zero.
//!
//! (B) **on-demand, `#[ignore]`d**, both against the real working tree with
//!   `RealGitOps`, both sharing one invocation and one graceful-skip:
//!
//!   * **`live_counts_are_within_the_committed_baseline`** runs `pdiag::check`
//!     and asserts ZERO `Severity::High` findings — no file exceeds its row and
//!     no code-less file is missing one. Medium (slack) findings are expected
//!     and deliberately tolerated: a file someone opportunistically improved
//!     must never turn a diff RED.
//!   * **`regenerating_the_committed_baseline_is_a_no_op`** asserts
//!     `render_baseline(live_counts(..))` is BYTE-identical to the committed
//!     file. Strictly wider: High covers only `Exceeded`/`NewFile`, so downward
//!     drift (`Stale`) and `OrphanRow` are exit-neutral by design and could
//!     otherwise sit in the manifest indefinitely with no signal anywhere.
//!     Byte-identity catches both directions plus row order and preamble drift.
//!
//!   Both stay out of `tests/infra/test_reify_audit_pdiag.sh`: byte-identity is
//!   RED on downward drift too, so making it merge-blocking would reverse the
//!   `Stale`/`OrphanRow`-are-exit-neutral decision and let an unrelated task's
//!   landed `DiagnosticCode` turn every open branch RED.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test pdiag_baseline`               (a/b + A + A′)
//!   `cargo test -p reify-audit --test pdiag_baseline -- --ignored`  (+ B)
//!
//! On (B) failure the fix is never a hand-edit — regenerate:
//!   ```text
//!   cargo run -p reify-audit --bin pdiag-baseline-gen -- --project-root . \
//!     > crates/reify-audit/pdiag-baseline.txt
//!   ```
//!   …and only after confirming the new sites genuinely warrant no code. The
//!   remediation triad (attach a `DiagnosticCode` / take the reviewed
//!   `pdiag:allow` opt-out / shrink the row) is in
//!   `docs/notes/diagnostic-severity-policy.md` §3.

use reify_audit::pdiag::test_support::{Fixture, coded_src, codeless_src};
use reify_audit::pdiag::{
    BASELINE_HEADER, is_swept_path, live_counts, parse_baseline, render_baseline,
};
use reify_audit::{AuditContext, MockJCodemunchOps, RealGitOps, Severity};
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

/// `crates/reify-audit/pdiag-baseline.txt`, resolved from the manifest dir so
/// the test is worktree-independent.
fn baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("pdiag-baseline.txt")
}

/// Repo root: `crates/reify-audit` → two `.parent()` hops.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/reify-audit has a parent (crates/)")
        .parent()
        .expect("crates/ has a parent (repo root)")
        .to_path_buf()
}

/// The canonical regeneration command, quoted in every failure message so
/// whoever just went RED never reaches for a hand-edit.
const REGEN: &str = "cargo run -p reify-audit --bin pdiag-baseline-gen -- \
                     --project-root . > crates/reify-audit/pdiag-baseline.txt";

/// `git` on PATH. `false` means "skipped", with the reason already on stderr.
///
/// A bare `--version` probe opens no repository, so it is the one invocation
/// `reify_audit::git_env`'s rule exempts from the `git -C <root>` constructor.
fn git_available(check: &str) -> bool {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("pdiag_baseline: skipping {check} — git not available");
        return false;
    }
    true
}

/// The graceful-skip preamble the two on-demand (B) checks share: `git` on
/// PATH, and a resolved root that is actually a checkout. `None` means
/// "skipped", with the reason already on stderr.
///
/// Shared rather than copied because the two checks must skip under EXACTLY the
/// same conditions — a divergence would leave one of them silently running
/// against a tree the other declined to look at.
fn live_checkout(check: &str) -> Option<PathBuf> {
    if !git_available(check) {
        return None;
    }
    let root = repo_root();
    // A linked worktree's `.git` is a FILE, not a directory — `exists()` is the
    // predicate that covers both, which matters because this crate is developed
    // in warm-lane worktrees far more often than in the main checkout.
    if !root.join(".git").exists() {
        eprintln!("pdiag_baseline: skipping {check} — {root:?} is not a git checkout");
        return None;
    }
    Some(root)
}

/// The real-tree [`AuditContext`], spelled ONCE — the `RealGitOps` counterpart
/// to `Fixture::with_ctx`'s synthetic one.
///
/// A closure rather than a returned value: the context borrows the git seam,
/// the sqlite handle and the jcodemunch stub, and all three have to outlive it.
///
/// `conn`, `jc` and `task_metadata` are inert placeholders — PDIAG is a purely
/// structural lane (`ls_files` plus working-tree reads) and touches neither the
/// task DB nor jcodemunch.
fn with_live_ctx<R>(root: &Path, f: impl FnOnce(&AuditContext) -> R) -> R {
    let git = RealGitOps::new(root.to_path_buf());
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: root.to_path_buf(),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };
    f(&ctx)
}

// The fixture, `codeless_src` and `coded_src` are `pdiag::test_support`'s —
// the SAME harness `pdiag.rs`'s unit tests drive, so the nine-field
// `AuditContext` literal and the census helpers exist in exactly one place.
// The fixture borrows its root, so each test owns the `TempDir`.

/// Expected map, spelled as `(path, count)` pairs.
fn expect(pairs: &[(&str, u32)]) -> BTreeMap<String, u32> {
    pairs.iter().map(|(p, n)| ((*p).to_string(), *n)).collect()
}

// -----------------------------------------------------------------------
// (a) live_counts — the absolute per-file census the generator renders
// -----------------------------------------------------------------------

#[test]
fn live_counts_reports_absolute_code_less_counts_per_tracked_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.write("crates/reify-eval/src/geometry_ops.rs", &codeless_src(3));
    fx.write("crates/reify-compiler/src/expr.rs", &codeless_src(1));
    fx.write("gui/src-tauri/src/lib.rs", &codeless_src(2));

    assert_eq!(
        fx.counts(),
        expect(&[
            ("crates/reify-compiler/src/expr.rs", 1),
            ("crates/reify-eval/src/geometry_ops.rs", 3),
            ("gui/src-tauri/src/lib.rs", 2),
        ]),
        "live_counts must report the ABSOLUTE per-file count, not a delta"
    );
}

#[test]
fn files_with_no_code_less_sites_are_omitted_rather_than_stored_as_zero() {
    // The manifest grammar rejects a `0` row outright (a clean file has NO
    // row), so a zero-valued entry here would render an unparseable baseline.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.write("crates/reify-eval/src/coded.rs", &coded_src(4));
    fx.write("crates/reify-eval/src/empty.rs", "pub fn nothing() {}\n");
    fx.write("crates/reify-eval/src/dirty.rs", &codeless_src(2));

    assert_eq!(fx.counts(), expect(&[("crates/reify-eval/src/dirty.rs", 2)]));
}

#[test]
fn live_counts_honours_the_is_swept_path_scope() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    // In scope.
    fx.write("crates/reify-eval/src/in_scope.rs", &codeless_src(1));
    // Out: a `tests` path segment, and the `tests.rs` / `*_tests.rs` stems.
    fx.write("crates/reify-eval/tests/harness.rs", &codeless_src(5));
    fx.write("crates/reify-eval/src/engine_build/tests.rs", &codeless_src(5));
    fx.write("crates/reify-eval/src/engine_build/expr_tests.rs", &codeless_src(5));
    // Out: the detector's own crate (self-match) and the test-support crate.
    fx.write("crates/reify-audit/src/pdssentinel.rs", &codeless_src(5));
    fx.write("crates/reify-test-support/src/lib.rs", &codeless_src(5));
    // Out: not a `crates/<name>/src/` or `gui/src-tauri/src/` path at all.
    fx.write("crates/reify-eval/build.rs", &codeless_src(5));
    fx.write("scripts/helper.rs", &codeless_src(5));
    // Out: not Rust.
    fx.write("crates/reify-eval/src/notes.md", &codeless_src(5));

    assert_eq!(fx.counts(), expect(&[("crates/reify-eval/src/in_scope.rs", 1)]));
}

#[test]
fn live_counts_honours_the_pdiag_allow_escape() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    let escaped = "    out.push(Diagnostic::error(\"boom\")); // pdiag:allow — reviewed\n";
    fx.write(
        "crates/reify-eval/src/mixed.rs",
        &format!("{escaped}{}", codeless_src(2)),
    );
    fx.write("crates/reify-eval/src/all_escaped.rs", &format!("{escaped}{escaped}"));

    assert_eq!(
        fx.counts(),
        expect(&[("crates/reify-eval/src/mixed.rs", 2)]),
        "an escaped site must not be counted, and a fully escaped file must be omitted"
    );
}

#[test]
fn tracked_paths_absent_from_the_working_tree_are_skipped() {
    // `ls_files()` and the tree can legitimately disagree mid-rebase; inventing
    // a count there would be a false RED, so the read failure is a skip.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.track_only("crates/reify-eval/src/vanished.rs");
    fx.write("crates/reify-eval/src/present.rs", &codeless_src(1));

    assert_eq!(fx.counts(), expect(&[("crates/reify-eval/src/present.rs", 1)]));
}

#[test]
fn live_counts_is_blind_to_the_committed_baseline() {
    // The load-bearing difference from `check`. The generator must be able to
    // regenerate the manifest from scratch, so its census may not be filtered
    // by whatever the manifest currently allows — a file sitting exactly at its
    // baseline row emits no finding, yet must still render a row.
    let path = "crates/reify-eval/src/geometry_ops.rs";
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.write(path, &codeless_src(7));
    fx.write_untracked("crates/reify-audit/pdiag-baseline.txt", &format!("{path} 7\n"));

    assert_eq!(
        fx.counts(),
        expect(&[(path, 7)]),
        "live_counts is a census of the tree, not a comparison against the manifest"
    );
}

#[test]
fn an_empty_tree_yields_an_empty_census() {
    let tmp = tempfile::tempdir().expect("tempdir");
    assert_eq!(Fixture::new(tmp.path()).counts(), BTreeMap::new());
}

#[test]
fn census_summary_reports_swept_files_alongside_the_counts() {
    // The generator's degenerate-census refusal keys on `swept`, so the two
    // facts must be separable: a tree whose every diagnostic already carries a
    // code is CLEAN (rows: none, swept: non-zero) and its zero-row manifest is
    // the end state the ratchet is aimed at — writing it must stay possible.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.write("crates/reify-eval/src/coded.rs", &coded_src(4));
    fx.write("crates/reify-eval/src/clean.rs", "pub fn nothing() {}\n");

    let (swept, counts) = fx.summary();
    assert_eq!(swept, 2, "both in-scope files were swept even though neither has a row");
    assert_eq!(counts, BTreeMap::new());
    assert_eq!(counts, fx.counts(), "census_summary's map IS live_counts");
}

#[test]
fn census_summary_reports_zero_swept_when_the_enumeration_comes_back_empty() {
    // `RealGitOps::ls_files` degrades to `vec![]` on ANY git failure, and the
    // generator's recipe redirects stdout over the committed manifest — so this
    // is the exact state in which emitting a header-only file would WIPE the
    // ratchet's own baseline. `swept == 0` is the signal it refuses on.
    let tmp = tempfile::tempdir().expect("tempdir");
    assert_eq!(Fixture::new(tmp.path()).summary().0, 0);
}

#[test]
fn tracked_files_all_out_of_scope_report_zero_swept() {
    // "Swept" is post-scope, not raw `ls_files` output: a tree that tracks only
    // out-of-scope paths censused nothing, and the generator must treat it the
    // same as an enumeration failure rather than render an empty manifest.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.write("crates/reify-eval/tests/harness.rs", &codeless_src(3));
    fx.write("scripts/helper.rs", &codeless_src(3));
    fx.write("crates/reify-eval/src/notes.md", &codeless_src(3));

    assert_eq!(fx.summary().0, 0);
}

// -----------------------------------------------------------------------
// (b) parse_baseline — the manifest grammar, driven directly
// -----------------------------------------------------------------------

#[test]
fn parse_baseline_round_trips_a_well_formed_manifest() {
    let content = "\
# PDIAG baseline — regenerate, never hand-edit.

crates/reify-compiler/src/expr.rs 1
crates/reify-eval/src/geometry_ops.rs 30
gui/src-tauri/src/lib.rs 2
";
    assert_eq!(
        parse_baseline(content).expect("well-formed manifest must parse"),
        expect(&[
            ("crates/reify-compiler/src/expr.rs", 1),
            ("crates/reify-eval/src/geometry_ops.rs", 30),
            ("gui/src-tauri/src/lib.rs", 2),
        ]),
        "`#` comments and blank lines are ignored; every other row is `<path> <count>`"
    );
}

#[test]
fn parse_baseline_accepts_an_empty_manifest() {
    // The end state the ratchet is aimed at: zero residual code-less sites.
    assert_eq!(parse_baseline("").expect("empty parses"), BTreeMap::new());
    assert_eq!(parse_baseline("# header only\n").expect("comment-only parses"), BTreeMap::new());
}

#[test]
fn render_baseline_emits_the_preamble_then_ascending_rows() {
    // The generator's whole contract in one assertion: whatever `live_counts`
    // reports must be expressible in — and recoverable from — the manifest
    // grammar. If these two ever drift, a regenerated baseline stops parsing.
    //
    // Driven through the REAL renderer. This test used to re-implement
    // `format!("{path} {count}\n")` itself, which made it a SECOND derivation of
    // the manifest format — one free to agree with itself forever while the
    // binary's copy drifted, and one that could say nothing at all about the
    // preamble because the preamble lived only inside the binary. Now the
    // binary, this round trip and the idempotency check all render through
    // `pdiag::render_baseline`.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut fx = Fixture::new(tmp.path());
    fx.write("crates/reify-eval/src/geometry_ops.rs", &codeless_src(3));
    fx.write("crates/reify-compiler/src/expr.rs", &codeless_src(1));
    fx.write("gui/src-tauri/src/lib.rs", &codeless_src(2));
    let census = fx.counts();

    let rendered = render_baseline(&census);

    // The preamble travels WITH the file — regen command, policy pointer, and
    // the "regenerating is NOT a remediation" warning — so a manifest read in
    // isolation still carries the deterrent.
    let body = rendered.strip_prefix(BASELINE_HEADER).unwrap_or_else(|| {
        panic!("rendered manifest must open with BASELINE_HEADER, got:\n{rendered}")
    });

    // …then exactly one row per census entry, ascending by path, and nothing
    // else. Compared against the BTreeMap's own iteration order, which IS
    // ascending — and re-asserted below so the ordering rule is pinned by an
    // explicit claim rather than by a property of the expectation's container.
    let rows: Vec<&str> = body.lines().collect();
    let expected: Vec<String> =
        census.iter().map(|(path, count)| format!("{path} {count}")).collect();
    assert_eq!(rows, expected, "rendered rows must be one `<path> <count>` per census entry");
    assert!(
        rows.windows(2).all(|w| w[0] < w[1]),
        "rendered rows must ascend by path, got {rows:?}"
    );

    // And the round trip: whatever the renderer emits, the parser recovers —
    // preamble and all, since `parse_baseline` skips `#` lines.
    assert_eq!(parse_baseline(&rendered).expect("rendered manifest must parse"), census);
}

// -----------------------------------------------------------------------
// (c) pdiag-baseline-gen — the BINARY's own contract
// -----------------------------------------------------------------------
//
// (a)/(b) drive the library seam, which is deliberately everything the binary
// does NOT own. What is left over is small and entirely the binary's: argument
// parsing, stdout purity, and the `swept == 0` refusal. That refusal is a
// pure ORDERING property — it has to run before anything reaches stdout,
// because the documented recipe redirects stdout OVER the committed manifest
// and the shell truncates that file before this process starts. Move the check
// below the render, or downgrade its exit code, and every seam test above
// stays green while the wipe re-opens.

/// Run `git` in `root`, panicking on failure.
///
/// Built through `reify_audit::git_env` rather than a bare `Command::new`:
/// an ambient `GIT_INDEX_FILE` — which `hooks/pre-commit` exports down through
/// the whole test run — otherwise overrides `-C <tempdir>` and stages into the
/// PARENT repository's index.
fn git_in(root: &Path, args: &[&str]) {
    let out = reify_audit::git_env::command(root)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A hermetic git repo at a fresh tempdir with `files` written and STAGED.
///
/// `RealGitOps::ls_files` — the enumeration the binary runs, unlike the
/// `MockGitOps` the [`Fixture`] seam tests use — reads the INDEX, so staging is
/// enough and no commit (and therefore no `user.name`/`user.email`) is needed.
fn staged_fixture(files: &[(&str, String)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git_in(root, &["init", "-q"]);
    for (rel, content) in files {
        let full = root.join(rel);
        std::fs::create_dir_all(full.parent().expect("fixture path has a parent"))
            .expect("create_dir_all");
        std::fs::write(&full, content).expect("write fixture file");
    }
    git_in(root, &["add", "-A"]);
    dir
}

/// The real generator binary aimed at `root`, plus any `extra` arguments.
///
/// Sanitized DIRECTLY rather than through the `git -C <root>` constructor: the
/// program here is a reify binary that runs git internally, not git itself,
/// which is the other-shape case `git_env::sanitize` sanctions.
fn run_generator(root: &Path, extra: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdiag-baseline-gen"));
    cmd.arg("--project-root").arg(root).args(extra);
    reify_audit::git_env::sanitize(&mut cmd);
    cmd.output().expect("pdiag-baseline-gen spawns")
}

/// A degenerate census exits 3 with NOTHING on stdout — the guard that stops
/// the documented `… > pdiag-baseline.txt` recipe from truncating the ratchet's
/// own manifest to its header.
#[test]
fn the_generator_refuses_a_degenerate_census() {
    if !git_available("the_generator_refuses_a_degenerate_census") {
        return;
    }
    // An initialised repo with an EMPTY index: `ls_files` succeeds and reports
    // nothing, which is byte-for-byte what a git FAILURE degrades to
    // (`RealGitOps::ls_files` swallows spawn errors, non-zero exits and
    // non-UTF-8 output alike). The refusal cannot tell them apart and must not
    // need to.
    let dir = staged_fixture(&[]);

    let out = run_generator(dir.path(), &[]);

    assert_eq!(
        out.status.code(),
        Some(3),
        "a census that reached zero swept files must exit 3, got {:?}; stderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "the refusal must emit NOTHING on stdout — the shell has already truncated the \
         redirect target, so even a header-only render IS the wipe. Got:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("crates/reify-audit/pdiag-baseline.txt"),
        "the refusal must name the manifest it declined to overwrite, got:\n{stderr}"
    );
}

/// A real census exits 0 and renders EXACTLY the manifest the library would —
/// preamble included, parseable, one row per code-less swept file.
#[test]
fn the_generator_renders_the_census_it_scanned() {
    if !git_available("the_generator_renders_the_census_it_scanned") {
        return;
    }
    // Three staged files spanning all three outcomes: one swept file with
    // code-less sites (a row), one swept file whose sites are all coded (no
    // row — a clean file's ABSENCE is the only spelling of "clean"), and one
    // out-of-scope path (invisible to the sweep entirely).
    let dir = staged_fixture(&[
        ("crates/reify-eval/src/geometry_ops.rs", codeless_src(2)),
        ("crates/reify-eval/src/clean.rs", coded_src(3)),
        ("crates/reify-eval/tests/harness.rs", codeless_src(9)),
    ]);

    let out = run_generator(dir.path(), &[]);

    assert_eq!(
        out.status.code(),
        Some(0),
        "a census that reached swept files must exit 0 regardless of how big the backlog \
         is — judging the tree is `reify-audit --pattern PDIAG`'s job. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("manifest is UTF-8");
    assert_eq!(
        stdout,
        format!("{BASELINE_HEADER}crates/reify-eval/src/geometry_ops.rs 2\n"),
        "stdout must be the library's render and nothing else — no diagnostics, no \
         trailing chatter"
    );
    assert_eq!(
        parse_baseline(&stdout).expect("a freshly generated manifest must parse"),
        expect(&[("crates/reify-eval/src/geometry_ops.rs", 2)]),
        "generation and enforcement read the same census, so the generator's own output \
         is by construction a manifest the ratchet accepts"
    );
}

/// An unknown flag exits 2 with nothing on stdout.
#[test]
fn the_generator_rejects_an_unknown_flag() {
    if !git_available("the_generator_rejects_an_unknown_flag") {
        return;
    }
    let dir = staged_fixture(&[("crates/reify-eval/src/geometry_ops.rs", codeless_src(1))]);

    let out = run_generator(dir.path(), &["--rebless"]);

    assert_eq!(
        out.status.code(),
        Some(2),
        "an unrecognised argument must exit 2, not silently fall back to a default \
         census; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "a rejected invocation must write no manifest, got:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// `--help` exits NON-ZERO with nothing on stdout.
///
/// The exit code is the load-bearing half. Under the documented recipe the
/// shell truncates `pdiag-baseline.txt` before this process starts, so a help
/// path that exits 0 with empty stdout leaves a ZERO-BYTE manifest and reports
/// success — worse than the `swept == 0` case exit 3 exists for, because
/// `parse_baseline("")` SUCCEEDS and the ratchet would then read an empty
/// baseline and flag every code-less file as a `NewFile` High. Asserting
/// stdout-is-empty alone would pass on exactly that broken binary.
#[test]
fn the_generator_refuses_to_report_success_for_help() {
    if !git_available("the_generator_refuses_to_report_success_for_help") {
        return;
    }
    let dir = staged_fixture(&[("crates/reify-eval/src/geometry_ops.rs", codeless_src(1))]);

    for flag in ["--help", "-h"] {
        let out = run_generator(dir.path(), &[flag]);

        assert_eq!(
            out.status.code(),
            Some(2),
            "{flag} renders no manifest, so it must exit 2 like every other non-census \
             path — exit 0 here silently blesses a truncated pdiag-baseline.txt; \
             stderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stdout.is_empty(),
            "{flag} must keep usage text on stderr — anything on stdout lands IN the \
             manifest under the documented redirect. Got:\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("Usage:"),
            "{flag} must still print its usage, on stderr"
        );
    }
}

// -----------------------------------------------------------------------
// (A) The committed manifest exists and parses
// -----------------------------------------------------------------------

/// The committed `pdiag-baseline.txt` EXISTS and the real `parse_baseline`
/// accepts it.
///
/// Existence is a hard assertion, not a graceful skip. `pdiag::check` reads an
/// unreadable manifest as an EMPTY one — deliberately fail-loud for the
/// ratchet, since every code-less file then surfaces as a `NewFile` High — but
/// that convention would make *this* test pass vacuously against a manifest
/// someone deleted or renamed. So the two guards point opposite ways on
/// purpose, and between them there is no way to lose the manifest quietly.
#[test]
fn baseline_exists_and_parses() {
    let path = baseline_path();
    assert!(
        path.exists(),
        "pdiag-baseline.txt not found at {path:?} — the PDIAG ratchet has no manifest to \
         ratchet against. Regenerate it with:\n  {REGEN}"
    );

    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));

    if let Err(err) = parse_baseline(&content) {
        panic!("pdiag-baseline.txt does not parse: {err}\nRegenerate it with:\n  {REGEN}");
    }
}

/// The committed manifest opens with exactly [`BASELINE_HEADER`].
///
/// Always-on and deliberately NOT `#[ignore]`d, because it is
/// main-INDEPENDENT: it compares the committed file against the crate's own
/// constant and never reads the working tree's diagnostics, so it carries zero
/// exposure to the main-moves-under-the-branch drift that keeps the whole-repo
/// ratchet on-demand. Being merge-blocking costs nothing here.
///
/// It closes the one drift class nothing else guards: a hand-edit or a
/// truncation that strips the "GENERATED — do not hand-edit" / "Regenerating is
/// NOT a remediation" preamble. `parse_baseline` cannot catch it — `#` lines are
/// comments to the grammar, so a manifest with the preamble deleted parses
/// perfectly — and the ratchet cannot either, since it only ever reads rows.
/// Yet that preamble is precisely the text that deters the re-bless-the-
/// regression move the whole detector exists to prevent.
#[test]
fn the_committed_baseline_carries_the_generated_preamble() {
    let path = baseline_path();
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));

    assert!(
        content.starts_with(BASELINE_HEADER),
        "pdiag-baseline.txt has lost the generated preamble — the regen command, the policy \
         pointer and the \"regenerating is NOT a remediation\" warning all travel with it, and \
         `parse_baseline` cannot notice because `#` lines are comments to the grammar. \
         Restore it by regenerating:\n  {REGEN}\n\nExpected the file to open with:\n{}\n\nIt \
         opens with:\n{}",
        BASELINE_HEADER,
        content.chars().take(BASELINE_HEADER.chars().count()).collect::<String>(),
    );
}

// -----------------------------------------------------------------------
// (A′) Grammar rules, driven over synthetic content
//
// PDIAG's committed manifest ships POPULATED, so (A) is non-vacuous today.
// But migration of the existing backlog is opportunistic (PRD §6.7), so the
// row count is meant to fall toward zero — at which point (A) would exercise
// nothing but `path.exists()`. These drive the SAME validator directly, so
// each rule keeps permanent coverage.
//
// Every rejection is asserted to name `pdiag-baseline.txt:<lineno>`: the
// error string is read by whoever just broke the build, and a rule that
// cannot say *which line* is a rule they cannot act on.
// -----------------------------------------------------------------------

/// Assert `content` is rejected and the message names line `lineno`.
fn rejected_at(content: &str, lineno: usize) -> String {
    let err = parse_baseline(content)
        .expect_err("content must be rejected")
        .to_string();
    assert!(
        err.contains(&format!("pdiag-baseline.txt:{lineno}")),
        "error must name pdiag-baseline.txt:{lineno}, got {err:?}"
    );
    err
}

#[test]
fn rejects_a_row_without_exactly_two_fields() {
    rejected_at("crates/reify-eval/src/a.rs\n", 1);
    rejected_at("crates/reify-eval/src/a.rs 3 extra\n", 1);
}

#[test]
fn rejects_a_non_numeric_count() {
    rejected_at("crates/reify-eval/src/a.rs three\n", 1);
    // Negative counts are not a thing: the census is a `u32`.
    rejected_at("crates/reify-eval/src/a.rs -1\n", 1);
}

#[test]
fn rejects_a_literal_zero_count() {
    // A clean file has NO row. A `0` would be a second spelling of the same
    // state and would make the orphan-row advisory unrepresentable.
    let err = rejected_at("crates/reify-eval/src/a.rs 0\n", 1);
    assert!(err.contains("delete the line"), "the fix must be stated, got {err:?}");
}

#[test]
fn rejects_a_path_outside_the_sweep() {
    // Kept honestly coupled to the predicate the live scan uses: a row no
    // scan can ever clear would otherwise sit in the manifest forever as a
    // permanent advisory, which is how a scope narrowing goes unnoticed.
    const OUT_OF_SCOPE: &str = "crates/reify-audit/src/pdiag.rs";
    assert!(!is_swept_path(OUT_OF_SCOPE), "fixture must actually be out of scope");
    rejected_at(&format!("{OUT_OF_SCOPE} 2\n"), 1);

    assert!(!is_swept_path("docs/notes/severity.md"));
    rejected_at("docs/notes/severity.md 2\n", 1);
}

#[test]
fn rejects_rows_out_of_ascending_order() {
    let unsorted = "crates/reify-eval/src/b.rs 1\ncrates/reify-eval/src/a.rs 1\n";
    rejected_at(unsorted, 2);
}

#[test]
fn rejects_a_duplicate_path() {
    // Silent last-wins would let a bad merge double a file's allowance.
    // The duplicate rule is checked BEFORE the ordering rule precisely so this
    // message is reachable: strict ascension rejects every repeat on its own
    // (adjacent as `path <= previous`, non-adjacent because it cannot be
    // ascending either), so asserting only `is_err()` here passed while
    // exercising the ordering rule and left the duplicate branch dead code.
    for (content, lineno) in [
        ("crates/reify-eval/src/a.rs 1\ncrates/reify-eval/src/a.rs 2\n", 2),
        (
            "crates/reify-eval/src/a.rs 1\ncrates/reify-eval/src/b.rs 1\ncrates/reify-eval/src/a.rs 2\n",
            3,
        ),
    ] {
        let err = rejected_at(content, lineno);
        assert!(
            err.contains("duplicate row for"),
            "the duplicate rule must be the one that fires, got {err:?}"
        );
    }
}

#[test]
fn line_numbers_count_comment_and_blank_lines() {
    // The reported number must index the FILE, not the surviving rows —
    // otherwise it points at the wrong line in a manifest with a header block.
    rejected_at("# header\n\ncrates/reify-eval/src/a.rs 0\n", 3);
}

// -----------------------------------------------------------------------
// (B) The committed manifest actually covers the live tree
// -----------------------------------------------------------------------

/// On-demand: run `pdiag::check` over the real working tree and assert it emits
/// ZERO `Severity::High` findings.
///
/// High is exactly the hard-gate set — `Exceeded` (a file went UP) and
/// `NewFile` (a code-less file with no row at all) — and the CLI's exit code is
/// the count of High findings, so "zero High" is precisely "`reify-audit
/// --pattern PDIAG` exits 0 on this tree".
///
/// Medium findings (`Stale`, `OrphanRow`) are NOT asserted against. They mean
/// someone fixed something the manifest still budgets for, and turning that
/// RED would make every opportunistic migration merge-blocking — the exact
/// posture PRD §6.7 rules out.
///
/// Graceful-skip when `git` is unavailable or the resolved root is not a
/// checkout; the always-on tiers above still cover the grammar.
#[ignore = "on-demand whole-repo ratchet; run via --ignored. Needs a real git \
    checkout — graceful-skip otherwise."]
#[test]
fn live_counts_are_within_the_committed_baseline() {
    let Some(root) = live_checkout("whole-repo ratchet") else {
        return;
    };

    let high: Vec<String> = with_live_ctx(&root, reify_audit::pdiag::check)
        .into_iter()
        .filter(|f| f.severity == Severity::High)
        .map(|f| f.summary)
        .collect();

    assert!(
        high.is_empty(),
        "{} PDIAG hard-gate finding(s) against the committed baseline:\n{}\n\n\
         Attach a DiagnosticCode, or take the reviewed `pdiag:allow` opt-out \
         (docs/notes/diagnostic-severity-policy.md §3). Only if the sites are \
         genuinely warranted, regenerate:\n  {REGEN}",
        high.len(),
        high.join("\n"),
    );
}

/// On-demand: regenerating the manifest against this tree must be a NO-OP —
/// `render_baseline(live_counts(..))` byte-identical to the committed file.
///
/// Coverage this adds over the ratchet above, which is deliberately narrower:
/// `check`'s High verdicts are only `Exceeded` and `NewFile`, so DOWNWARD drift
/// (`Stale` — a row still budgeting for sites someone has since coded) and
/// `OrphanRow` (a deleted or renamed file) are Medium and exit-neutral by
/// design, and can therefore sit in the manifest indefinitely with no signal
/// anywhere in the suite. Byte-identity catches both directions, plus row ORDER
/// and preamble/format drift, in one command.
///
/// Deliberately `#[ignore]`d and deliberately NOT a scenario in
/// `tests/infra/test_reify_audit_pdiag.sh`. Byte-identity is RED on downward
/// drift too, so making it merge-blocking would reverse the standing decision
/// that `Stale`/`OrphanRow` stay exit-neutral — an unrelated task landing a
/// `DiagnosticCode` on main would turn every open branch RED through no fault
/// of its author. The blocking direction is already covered mechanically by the
/// infra gate; this check is the pre-land hygiene pass, sharing one invocation
/// with its sibling:
///   `cargo test -p reify-audit --test pdiag_baseline -- --ignored`
///
/// On mismatch it prints the ROW-LEVEL delta, not a byte diff. The reader is
/// whoever is about to land, not a parser, and a raw diff of an 89-line file
/// buries the one row that moved.
#[ignore = "on-demand regeneration-idempotency check; run via --ignored. Needs \
    a real git checkout — graceful-skip otherwise."]
#[test]
fn regenerating_the_committed_baseline_is_a_no_op() {
    let Some(root) = live_checkout("regeneration-idempotency check") else {
        return;
    };

    let path = baseline_path();
    let committed =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));

    let live = with_live_ctx(&root, live_counts);
    if render_baseline(&live) == committed {
        return;
    }

    // Reached only on failure, so the cost of a second parse is irrelevant and
    // the payoff is a message someone can act on without running anything else.
    let rows = match parse_baseline(&committed) {
        Ok(rows) => rows,
        Err(err) => panic!(
            "pdiag-baseline.txt does not parse ({err}), so no row-level delta can be \
             computed. Regenerate it:\n  {REGEN}"
        ),
    };

    let mut delta: Vec<String> = Vec::new();
    for (p, live_count) in &live {
        match rows.get(p) {
            None => delta.push(format!("  + {p} {live_count}   (new row)")),
            Some(was) if was != live_count => {
                let note = if live_count > was { "WENT UP" } else { "went down" };
                delta.push(format!("  ~ {p} {was} -> {live_count}   ({note})"));
            }
            Some(_) => {}
        }
    }
    for (p, was) in &rows {
        if !live.contains_key(p) {
            delta.push(format!(
                "  - {p} {was}   (row no longer earned — file deleted, renamed, or fully coded)"
            ));
        }
    }

    let body = if delta.is_empty() {
        // The maps agree, so the drift is in the bytes AROUND the rows: a
        // stripped or edited preamble, a reordering, stray whitespace. Naming
        // that explicitly beats printing an empty delta and looking broken.
        "The rows themselves agree — the drift is in the manifest's FORMAT: the preamble, \
         the row order, or whitespace. Regenerating restores the canonical bytes."
            .to_string()
    } else {
        delta.join("\n")
    };

    panic!(
        "regenerating pdiag-baseline.txt would change it — the committed manifest no longer \
         describes this tree:\n{body}\n\n\
         A row that WENT UP or a NEW row is a real regression: attach a DiagnosticCode, or take \
         the reviewed `pdiag:allow` opt-out (docs/notes/diagnostic-severity-policy.md §3). Rows \
         that went down or vanished are someone else's fix already landed, and absorbing them is \
         the whole point of a ratchet. Either way the manifest is only ever written by its \
         generator:\n  {REGEN}"
    );
}
