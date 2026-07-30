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
//! User-observable signal:
//!   `cargo test -p reify-audit --test pdiag_baseline`

use reify_audit::pdiag::{live_counts, parse_baseline};
use reify_audit::{AuditContext, MockGitOps, MockJCodemunchOps};
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap};

// -----------------------------------------------------------------------
// Fixture — a tempdir working tree plus a MockGitOps that tracks exactly
// what was written. Deliberately the same shape as `pdiag.rs`'s in-module
// `Fixture`: the enumeration seam is `ls_files()`, but content is read from
// the real working tree, so the IO fail-safe branches stay reachable rather
// than mocked away.
// -----------------------------------------------------------------------

struct Fixture {
    root: tempfile::TempDir,
    tracked: Vec<String>,
}

impl Fixture {
    fn new() -> Self {
        Self { root: tempfile::tempdir().expect("tempdir"), tracked: Vec::new() }
    }

    /// Write `content` at `path` and track it.
    fn write(&mut self, path: &str, content: &str) -> &mut Self {
        let full = self.root.path().join(path);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        std::fs::write(&full, content).expect("write");
        self.tracked.push(path.to_string());
        self
    }

    /// Track a path WITHOUT creating it — the `ls_files`/working-tree skew a
    /// mid-rebase or just-deleted file produces.
    fn track_only(&mut self, path: &str) -> &mut Self {
        self.tracked.push(path.to_string());
        self
    }

    /// Write an untracked data file (used to plant a baseline manifest, which
    /// `live_counts` must be blind to).
    fn write_untracked(&mut self, path: &str, content: &str) -> &mut Self {
        let full = self.root.path().join(path);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        std::fs::write(&full, content).expect("write");
        self
    }

    fn counts(&self) -> BTreeMap<String, u32> {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let jc = MockJCodemunchOps::new();
        let mut git = MockGitOps::new();
        git.set_ls_files(self.tracked.clone());
        let ctx = AuditContext {
            project_root: self.root.path().to_path_buf(),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };
        live_counts(&ctx)
    }
}

/// `n` code-less constructor sites, one per line — the dominant real shape
/// (`crates/reify-eval/src/geometry_ops.rs:313`).
fn codeless_src(n: usize) -> String {
    (0..n).map(|i| format!("    out.push(Diagnostic::error(format!(\"boom {i}\")));\n")).collect()
}

/// `n` sites that each carry a code on the same line.
fn coded_src(n: usize) -> String {
    (0..n)
        .map(|i| {
            format!("    out.push(Diagnostic::error(format!(\"boom {i}\")).with_code(code));\n")
        })
        .collect()
}

/// Expected map, spelled as `(path, count)` pairs.
fn expect(pairs: &[(&str, u32)]) -> BTreeMap<String, u32> {
    pairs.iter().map(|(p, n)| ((*p).to_string(), *n)).collect()
}

// -----------------------------------------------------------------------
// (a) live_counts — the absolute per-file census the generator renders
// -----------------------------------------------------------------------

#[test]
fn live_counts_reports_absolute_code_less_counts_per_tracked_path() {
    let mut fx = Fixture::new();
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
    let mut fx = Fixture::new();
    fx.write("crates/reify-eval/src/coded.rs", &coded_src(4));
    fx.write("crates/reify-eval/src/empty.rs", "pub fn nothing() {}\n");
    fx.write("crates/reify-eval/src/dirty.rs", &codeless_src(2));

    assert_eq!(fx.counts(), expect(&[("crates/reify-eval/src/dirty.rs", 2)]));
}

#[test]
fn live_counts_honours_the_is_swept_path_scope() {
    let mut fx = Fixture::new();
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
    let mut fx = Fixture::new();
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
    let mut fx = Fixture::new();
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
    let mut fx = Fixture::new();
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
    assert_eq!(Fixture::new().counts(), BTreeMap::new());
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
fn a_live_census_renders_rows_that_parse_back_unchanged() {
    // The generator's whole contract in one assertion: whatever `live_counts`
    // reports must be expressible in — and recoverable from — the manifest
    // grammar. If these two ever drift, a regenerated baseline stops parsing.
    let mut fx = Fixture::new();
    fx.write("crates/reify-eval/src/geometry_ops.rs", &codeless_src(3));
    fx.write("crates/reify-compiler/src/expr.rs", &codeless_src(1));
    fx.write("gui/src-tauri/src/lib.rs", &codeless_src(2));
    let census = fx.counts();

    let rendered: String =
        census.iter().map(|(path, count)| format!("{path} {count}\n")).collect();

    assert_eq!(parse_baseline(&rendered).expect("rendered census must parse"), census);
}
