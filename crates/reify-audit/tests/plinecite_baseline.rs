//! `plinecite-baseline-gen` — the regenerator of
//! `crates/reify-audit/plinecite-baseline.txt` — driven as a real binary over
//! hermetic, staged git repos.
//!
//! The binary owns only its flags and the choice between `Ledger::kept()` and
//! the full live debt; the row derivation is `plinecite::baseline_ledger` and
//! the bytes are `plinecite_baseline::render_baseline`. What is pinned here is
//! what the binary alone can break: stdout purity, the exit-code contract, and
//! that growth needs `--admit-new`.

use reify_audit::plinecite_baseline::{BASELINE_PATH, CiteRow, render_baseline};
use reify_audit::{AuditContext, Finding, MockJCodemunchOps, RealGitOps};
use rusqlite::Connection;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::{Command, Output};

/// `git` on PATH. `false` means "skipped", with the reason already on stderr.
fn git_available(check: &str) -> bool {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("plinecite_baseline: skipping {check} — git not available");
        return false;
    }
    true
}

/// Run `git` in `root`, panicking on failure. Built through
/// `reify_audit::git_env` so an ambient `GIT_INDEX_FILE` (exported by
/// `hooks/pre-commit`) cannot redirect staging into the parent repository.
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

fn write_file(root: &Path, rel: &str, content: &str) {
    let full = root.join(rel);
    std::fs::create_dir_all(full.parent().expect("fixture path has a parent"))
        .expect("create_dir_all");
    std::fs::write(&full, content).expect("write fixture file");
}

/// A hermetic git repo with `files` written and STAGED — `RealGitOps` reads
/// the index, so no commit (and no committer identity) is needed.
fn staged_fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git_in(dir.path(), &["init", "-q"]);
    for (rel, content) in files {
        write_file(dir.path(), rel, content);
    }
    git_in(dir.path(), &["add", "-A"]);
    dir
}

/// `plinecite::check` over `root` through `RealGitOps` — PLINECITE reads only
/// `ls_files` and the working tree, so the sqlite and jcodemunch seams are
/// inert.
fn check_real(root: &Path) -> Vec<Finding> {
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
    reify_audit::plinecite::check(&ctx)
}

/// The real generator aimed at `root`. Sanitized directly: the program is a
/// reify binary that runs git internally, not git itself.
fn run_generator(root: &Path, extra: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_plinecite-baseline-gen"));
    cmd.arg("--project-root").arg(root).args(extra);
    reify_audit::git_env::sanitize(&mut cmd);
    cmd.output().expect("plinecite-baseline-gen spawns")
}

const STDLIB: &str = "crates/reify-compiler/stdlib/a.ri";

fn row(cite: &str) -> CiteRow {
    CiteRow {
        path: STDLIB.to_string(),
        cite: cite.to_string(),
    }
}

/// A tree whose live debt and committed ledger overlap in every way:
///
/// - `kept.rs:1` — cited AND ledgered → kept;
/// - `new.rs:2` — cited, not ledgered → new debt;
/// - `gone.rs:3` — ledgered, cited nowhere → stale.
fn ledger_fixture() -> tempfile::TempDir {
    let committed = render_baseline(&[row("kept.rs:1"), row("gone.rs:3")].into());
    staged_fixture(&[
        (
            STDLIB,
            "// see kept.rs:1\nstructure S {}\n// see new.rs:2\n",
        ),
        (BASELINE_PATH, &committed),
    ])
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("the ledger is UTF-8")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Help renders no ledger, so it must not report success: under the
/// documented redirect a zero-byte ledger with exit 0 reads as "no debt".
#[test]
fn the_generator_refuses_to_report_success_for_help() {
    if !git_available("the_generator_refuses_to_report_success_for_help") {
        return;
    }
    let dir = ledger_fixture();

    for flag in ["--help", "-h"] {
        let out = run_generator(dir.path(), &[flag]);

        assert_eq!(out.status.code(), Some(2), "{flag}: {}", stderr_of(&out));
        assert!(out.stdout.is_empty(), "{flag}: got:\n{}", stdout_of(&out));
        assert!(
            stderr_of(&out).contains("Usage:"),
            "{flag} must still print its usage, on stderr"
        );
    }
}

/// Only `--project-root` names the root: a stray positional is refused like
/// an unknown flag, never silently taken as a different tree.
#[test]
fn the_generator_rejects_an_unknown_argument() {
    if !git_available("the_generator_rejects_an_unknown_argument") {
        return;
    }
    let dir = ledger_fixture();

    for arg in ["--rebless", "elsewhere"] {
        let out = run_generator(dir.path(), &[arg]);

        assert_eq!(out.status.code(), Some(2), "{arg}: {}", stderr_of(&out));
        assert!(out.stdout.is_empty(), "{arg}: got:\n{}", stdout_of(&out));
    }
}

/// An empty scope is what any git failure degrades to, so it must exit 3
/// with NOTHING on stdout: the documented `… > plinecite-baseline.txt` recipe
/// has already truncated the ledger, and even a header-only render IS the
/// wipe.
#[test]
fn the_generator_refuses_a_tree_with_nothing_in_scope() {
    if !git_available("the_generator_refuses_a_tree_with_nothing_in_scope") {
        return;
    }
    let committed = render_baseline(&[row("kept.rs:1")].into());
    let staged = staged_fixture(&[
        ("docs/notes/n.md", "// see kept.rs:1\n"),
        (BASELINE_PATH, &committed),
    ]);
    let not_a_repo = tempfile::tempdir().expect("tempdir");

    for root in [staged.path(), not_a_repo.path()] {
        let out = run_generator(root, &[]);

        assert_eq!(out.status.code(), Some(3), "{root:?}: {}", stderr_of(&out));
        assert!(out.stdout.is_empty(), "{root:?}: got:\n{}", stdout_of(&out));
        assert!(
            stderr_of(&out).contains(BASELINE_PATH),
            "the refusal must name the ledger it declined to overwrite; got:\n{}",
            stderr_of(&out)
        );
    }
}

/// The default regeneration writes `Ledger::kept()`: the stale row goes, and
/// new debt is NOT admitted, only counted on stderr.
#[test]
fn the_default_regeneration_only_shrinks() {
    if !git_available("the_default_regeneration_only_shrinks") {
        return;
    }
    let dir = ledger_fixture();

    let out = run_generator(dir.path(), &[]);

    assert_eq!(out.status.code(), Some(0), "stderr:\n{}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        render_baseline(&[row("kept.rs:1")].into()),
        "stdout must be exactly the kept rows"
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("dropped 1 stale row(s)")
            && stderr.contains("1 new-debt row(s) NOT admitted")
            && stderr.contains("--admit-new"),
        "stderr must count the dropped and the un-admitted rows and name the flag \
         that admits debt; got:\n{stderr}"
    );
}

/// `--admit-new` ledgers ALL live debt, and the file it writes is exactly one
/// the ratchet accepts — generation and enforcement are one derivation.
#[test]
fn admit_new_ledgers_all_live_debt_and_the_ratchet_accepts_it() {
    if !git_available("admit_new_ledgers_all_live_debt_and_the_ratchet_accepts_it") {
        return;
    }
    let dir = ledger_fixture();
    let root = dir.path();

    let out = run_generator(root, &["--admit-new"]);

    assert_eq!(out.status.code(), Some(0), "stderr:\n{}", stderr_of(&out));
    let expected: BTreeSet<CiteRow> = [row("kept.rs:1"), row("new.rs:2")].into();
    let ledger = stdout_of(&out);
    assert_eq!(ledger, render_baseline(&expected));

    write_file(root, BASELINE_PATH, &ledger);
    git_in(root, &["add", "-A"]);
    let findings = check_real(root);
    assert!(
        findings.is_empty(),
        "a freshly admitted ledger must satisfy its own ratchet: no line-cite and \
         no stale-baseline-entry; got {findings:#?}"
    );
}
