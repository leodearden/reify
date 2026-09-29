//! `pdoccover-baseline-gen` — the regenerator of
//! `crates/reify-audit/pdoccover-baseline.txt` — driven as a real binary over
//! hermetic, staged git repos.
//!
//! The binary owns only its flags and the choice between `Ledger::kept()` and
//! the full live debt; the row derivation is `pdoccover::baseline_ledger` and
//! the bytes are `pdoccover_baseline::render_baseline`. What is pinned here is
//! what the binary alone can break: stdout purity, the exit-code contract, and
//! that growth needs `--admit-new`.

use reify_audit::pdoccover_baseline::{BASELINE_PATH, BaselineRow, render_baseline};
use reify_audit::{AuditContext, MockJCodemunchOps, RealGitOps};
use rusqlite::Connection;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::{Command, Output};

/// `git` on PATH. `false` means "skipped", with the reason already on stderr.
fn git_available(check: &str) -> bool {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("pdoccover_baseline: skipping {check} — git not available");
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

/// The real generator aimed at `root`. Sanitized directly: the program is a
/// reify binary that runs git internally, not git itself.
fn run_generator(root: &Path, extra: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdoccover-baseline-gen"));
    cmd.arg("--project-root").arg(root).args(extra);
    reify_audit::git_env::sanitize(&mut cmd);
    cmd.output().expect("pdoccover-baseline-gen spawns")
}

const UNITS: &str = "crates/reify-compiler/src/units.rs";
const CHUNK: &str = "crates/reify-mcp/src/tools/chunks/stdlib.md";

/// A tree whose live debt and committed ledger overlap in every way:
///
/// - `kept_op` — undocumented AND ledgered → kept;
/// - `new_op` — undocumented, not ledgered → new debt;
/// - `ghost_op` — claimed by the chunk, declared nowhere → new fabrication debt;
/// - `stale_doc_op` — ledgered, but now documented → stale;
/// - `vanished_op` — ledgered, declared by no registry → stale.
fn ledger_fixture() -> tempfile::TempDir {
    staged_fixture(&[
        (
            UNITS,
            "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \"kept_op\",\n    \
             \"new_op\",\n    \"stale_doc_op\",\n];\n",
        ),
        (
            CHUNK,
            "# Stdlib\n\n- `stale_doc_op(x)` — now documented.\n\
             - `ghost_op(x)` — ahead of the implementation.\n",
        ),
        (BASELINE_PATH, "kept_op\nstale_doc_op\nvanished_op\n"),
    ])
}

fn undocumented(name: &str) -> BaselineRow {
    BaselineRow::Undocumented(name.to_string())
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("the ledger is UTF-8")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// An empty index is what any git failure degrades to, so it must exit 3
/// with NOTHING on stdout: the documented `… > pdoccover-baseline.txt` recipe
/// has already truncated the ledger, and even a header-only render IS the wipe.
#[test]
fn the_generator_refuses_a_degenerate_census() {
    if !git_available("the_generator_refuses_a_degenerate_census") {
        return;
    }
    let dir = staged_fixture(&[]);

    let out = run_generator(dir.path(), &[]);

    assert_eq!(out.status.code(), Some(3), "stderr:\n{}", stderr_of(&out));
    assert!(out.stdout.is_empty(), "got:\n{}", stdout_of(&out));
    assert!(
        stderr_of(&out).contains(BASELINE_PATH),
        "the refusal must name the ledger it declined to overwrite; got:\n{}",
        stderr_of(&out)
    );
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

#[test]
fn the_generator_rejects_an_unknown_flag() {
    if !git_available("the_generator_rejects_an_unknown_flag") {
        return;
    }
    let dir = ledger_fixture();

    let out = run_generator(dir.path(), &["--rebless"]);

    assert_eq!(out.status.code(), Some(2), "stderr:\n{}", stderr_of(&out));
    assert!(out.stdout.is_empty(), "got:\n{}", stdout_of(&out));
}

/// The default regeneration writes `Ledger::kept()`: stale rows go, and new
/// debt — omission or fabrication — is NOT admitted, only counted on stderr.
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
        render_baseline(&[undocumented("kept_op")].into()),
        "stdout must be exactly the kept rows"
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("2 new-debt row(s) NOT admitted") && stderr.contains("--admit-new"),
        "stderr must count the un-admitted debt and name the flag that admits it; \
         got:\n{stderr}"
    );
}

/// `--admit-new` ledgers ALL live debt, and the file it writes is exactly one
/// the ratchet accepts — generation and enforcement are one derivation (PRD
/// §6.6).
#[test]
fn admit_new_ledgers_all_live_debt_and_the_ratchet_accepts_it() {
    if !git_available("admit_new_ledgers_all_live_debt_and_the_ratchet_accepts_it") {
        return;
    }
    let dir = ledger_fixture();
    let root = dir.path();

    let out = run_generator(root, &["--admit-new"]);

    assert_eq!(out.status.code(), Some(0), "stderr:\n{}", stderr_of(&out));
    let expected: BTreeSet<BaselineRow> = [
        undocumented("kept_op"),
        undocumented("new_op"),
        BaselineRow::Fabricated {
            chunk: CHUNK.to_string(),
            name: "ghost_op".to_string(),
        },
    ]
    .into();
    let ledger = stdout_of(&out);
    assert_eq!(ledger, render_baseline(&expected));

    write_file(root, BASELINE_PATH, &ledger);
    git_in(root, &["add", "-A"]);
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
    let findings = reify_audit::pdoccover::check(&ctx);
    assert!(
        findings.is_empty(),
        "a freshly admitted ledger must satisfy its own ratchet; got {findings:?}"
    );
}
