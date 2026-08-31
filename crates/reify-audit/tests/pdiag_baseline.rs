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
//! [`reify_audit::pdiag::live_counts`], and the generator is a thin renderer
//! over it. That is the PRD §6.6 "derivation lives in ONE place" invariant,
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
//! (B) **`live_counts_are_within_the_committed_baseline`** — on-demand,
//!   `#[ignore]`. Runs `pdiag::check` over the real working tree with
//!   `RealGitOps` and asserts ZERO `Severity::High` findings, i.e. no file
//!   exceeds its row and no code-less file is missing one. Medium (slack)
//!   findings are expected and deliberately tolerated: a file someone
//!   opportunistically improved must never turn a diff RED.
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

use reify_audit::Severity;
use reify_audit::pdiag::test_support::{Fixture, coded_src, codeless_src};
use reify_audit::pdiag::{BASELINE_HEADER, is_swept_path, parse_baseline, render_baseline};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    if std::process::Command::new("git").arg("--version").output().is_err() {
        eprintln!("pdiag_baseline: skipping whole-repo ratchet — git not available");
        return;
    }
    let root = repo_root();
    if !root.join(".git").exists() {
        eprintln!("pdiag_baseline: skipping whole-repo ratchet — {root:?} is not a git checkout");
        return;
    }

    // `conn`, `jc` and `task_metadata` are inert placeholders: PDIAG is a
    // purely structural lane (ls_files + working-tree reads), touching neither
    // the task DB nor jcodemunch.
    use reify_audit::{AuditContext, MockJCodemunchOps};
    use rusqlite::Connection;
    use std::collections::HashMap;

    let git = reify_audit::RealGitOps::new(root.clone());
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: root.clone(),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    let high: Vec<String> = reify_audit::pdiag::check(&ctx)
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
