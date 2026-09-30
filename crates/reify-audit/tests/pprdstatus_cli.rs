//! End-to-end CLI coverage for `--pattern PPRDSTATUS`.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test pprdstatus_cli`
//!
//! `tests/pprdstatus.rs` drives `pprdstatus::check` directly, which cannot
//! see whether the `--pattern PPRDSTATUS` token reaches that detector. This
//! binary runs the real binary against a real git repo and a `--tasks-file`
//! corpus, and asserts on the exit code (the High-severity count) and the
//! findings JSON on stderr.
//!
//! Fixture provenance: `fixtures/pprdstatus/kernel-seam-contracts.pre-edd9703fae.md`
//! and `.post-edd9703fae.md` are verbatim excerpts of
//! `docs/prds/kernel-seam-contracts.md` around its SHIPPED re-stamp, commit
//! `edd9703fae`. The third case commits the LIVE copy of that doc, read from
//! this tree, which is the task's literal signal: silent on the doc as it
//! stands on main.
//!
//! `findings_from_stderr` and `write_empty_runs_db` are deliberate local
//! copies of `tests/pdcheck_cli.rs`'s, for the reason that file gives.

mod common;

use common::fixtures::legacy_meta;
use reify_audit::TaskMetadata;
use std::path::{Path, PathBuf};
use std::process::Output;

const PRE_FIX: &str = include_str!("fixtures/pprdstatus/kernel-seam-contracts.pre-edd9703fae.md");
const POST_FIX: &str = include_str!("fixtures/pprdstatus/kernel-seam-contracts.post-edd9703fae.md");
const LIVE_DOC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/prds/kernel-seam-contracts.md"
);

const KERNEL_SEAM_CONTRACTS: &str = "docs/prds/kernel-seam-contracts.md";

/// `git init` a repo at `dir` holding `content` at [`KERNEL_SEAM_CONTRACTS`],
/// committed so `git ls-files` tracks it.
fn repo_with_prd(dir: &Path, content: &str) {
    let run = |args: &[&str]| {
        let status = common::git_env::git_cmd(dir)
            .args(args)
            .status()
            .expect("git command failed to spawn");
        assert!(
            status.success(),
            "git {:?} exited {:?}",
            args,
            status.code()
        );
    };
    let path = dir.join(KERNEL_SEAM_CONTRACTS);
    std::fs::create_dir_all(path.parent().expect("path has a parent")).expect("create docs/prds");
    std::fs::write(&path, content).expect("write PRD fixture");

    run(&["init", "--initial-branch=main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["add", "."]);
    run(&["commit", "-m", "seed kernel-seam-contracts"]);
}

/// The kernel-seam-contracts decomposition, every leaf `done`: α #5102 …
/// ξ #5116 plus the adopted #4876.
fn write_done_leaves_tasks_json(dir: &Path) -> PathBuf {
    let leaves: Vec<TaskMetadata> = (5102..=5116)
        .chain([4876])
        .map(|id| TaskMetadata {
            status: "done".to_string(),
            prd: Some(KERNEL_SEAM_CONTRACTS.to_string()),
            ..legacy_meta(&id.to_string())
        })
        .collect();
    let path = dir.join("tasks.json");
    std::fs::write(
        &path,
        serde_json::to_string(&leaves).expect("serialize tasks"),
    )
    .expect("write tasks.json");
    path
}

fn write_empty_runs_db(dir: &Path) -> PathBuf {
    let path = dir.join("runs.db");
    let conn = rusqlite::Connection::open(&path).expect("open runs.db");
    conn.execute_batch("CREATE TABLE events (task_id TEXT, event_type TEXT);")
        .expect("create events table");
    path
}

/// The findings array the binary appends to stderr, parsed.
fn findings_from_stderr(stderr: &str) -> Vec<serde_json::Value> {
    let start = stderr
        .rfind("\n[")
        .map(|pos| pos + 1)
        .or_else(|| stderr.starts_with('[').then_some(0))
        .unwrap_or_else(|| panic!("no findings array in stderr:\n{stderr}"));
    serde_json::from_str(&stderr[start..])
        .unwrap_or_else(|e| panic!("stderr findings array does not parse: {e}\n{stderr}"))
}

/// Run `reify-audit --pattern PPRDSTATUS` over a repo holding `prd_text`,
/// with every substrate pinned at a tempdir.
fn run_pprdstatus(prd_text: &str) -> Output {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_prd(repo.path(), prd_text);
    let tasks_file = write_done_leaves_tasks_json(aux.path());
    let runs_db = write_empty_runs_db(aux.path());
    std::process::Command::new(env!("CARGO_BIN_EXE_reify-audit"))
        .args([
            "--pattern",
            "PPRDSTATUS",
            "--no-jcodemunch",
            "--project-root",
            repo.path().to_str().expect("utf-8 repo path"),
            "--tasks-file",
            tasks_file.to_str().expect("utf-8 tasks.json path"),
            "--runs-db",
            runs_db.to_str().expect("utf-8 runs.db path"),
        ])
        .output()
        .expect("invoke reify-audit --pattern PPRDSTATUS")
}

/// The pre-fix doc is stale on both lanes: one finding each, both High, so the
/// exit code (the High count) is 2.
#[test]
fn pattern_pprdstatus_reports_both_lanes_on_the_pre_fix_doc() {
    let out = run_pprdstatus(PRE_FIX);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(2), "stderr:\n{stderr}");
    let findings = findings_from_stderr(&stderr);
    assert_eq!(
        findings.len(),
        2,
        "{:#}",
        serde_json::Value::Array(findings.clone())
    );
    for finding in &findings {
        assert_eq!(
            finding["pattern"].as_str(),
            Some("PPrdStatus"),
            "{finding:#}"
        );
        assert_eq!(finding["severity"].as_str(), Some("High"), "{finding:#}");
    }
    for prefix in ["stale-status-header:", "cite-status-contradiction:"] {
        let matching = findings
            .iter()
            .filter(|finding| {
                finding["summary"]
                    .as_str()
                    .is_some_and(|s| s.starts_with(prefix))
            })
            .count();
        assert_eq!(
            matching, 1,
            "one {prefix} finding expected; stderr:\n{stderr}"
        );
    }
}

#[test]
fn pattern_pprdstatus_is_silent_on_the_post_fix_doc() {
    let out = run_pprdstatus(POST_FIX);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(0), "stderr:\n{stderr}");
    assert_eq!(
        findings_from_stderr(&stderr),
        Vec::<serde_json::Value>::new()
    );
}

/// The task's literal signal: silent on `docs/prds/kernel-seam-contracts.md`
/// as it stands in this tree.
#[test]
fn pattern_pprdstatus_is_silent_on_the_live_kernel_seam_contracts_doc() {
    let live = std::fs::read_to_string(LIVE_DOC).expect("read the live kernel-seam-contracts doc");
    let out = run_pprdstatus(&live);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(0), "stderr:\n{stderr}");
    assert_eq!(
        findings_from_stderr(&stderr),
        Vec::<serde_json::Value>::new()
    );
}
