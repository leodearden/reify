//! End-to-end CLI coverage for `--pattern PPRDSTATUS`.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test pprdstatus_cli`
//!
//! `tests/pprdstatus.rs` drives `pprdstatus::check` directly, which cannot
//! see whether the `--pattern PPRDSTATUS` token reaches that detector. This
//! binary runs the real binary against a real git repo and a `--tasks-file`
//! corpus, and asserts on the exit code (the High-severity count) and the
//! findings JSON on stderr. The `live_loader_*` cases instead serve the corpus
//! through the live fused-memory loader, from `tests/common/mcp_mock.rs`.
//!
//! Fixture provenance: `fixtures/pprdstatus/kernel-seam-contracts.pre-edd9703fae.md`
//! and `.post-edd9703fae.md` are verbatim excerpts of
//! `docs/prds/kernel-seam-contracts.md` around its SHIPPED re-stamp, commit
//! `edd9703fae`. The third case commits the LIVE copy of that doc, read from
//! this tree, which is the task's literal signal: silent on the doc as it
//! stands on main. That case is coupled to the tree on purpose; its doc
//! comment says what the coupling costs.
//!
//! `findings_from_stderr` and `write_empty_runs_db` are deliberate local
//! copies of `tests/pdcheck_cli.rs`'s, for the reason that file gives.

mod common;

use common::fixtures::legacy_meta;
use reify_audit::TaskMetadata;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
fn done_leaves() -> Vec<TaskMetadata> {
    (5102..=5116)
        .chain([4876])
        .map(|id| TaskMetadata {
            status: "done".to_string(),
            prd: Some(KERNEL_SEAM_CONTRACTS.to_string()),
            ..legacy_meta(&id.to_string())
        })
        .collect()
}

fn write_tasks_json(dir: &Path, tasks: &[TaskMetadata]) -> PathBuf {
    let path = dir.join("tasks.json");
    std::fs::write(
        &path,
        serde_json::to_string(tasks).expect("serialize tasks"),
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
/// against the [`done_leaves`] corpus.
fn run_pprdstatus(prd_text: &str) -> Output {
    run_pprdstatus_over(prd_text, &done_leaves())
}

/// Run `reify-audit --pattern PPRDSTATUS` over a repo holding `prd_text`,
/// against the `tasks` corpus.
fn run_pprdstatus_over(prd_text: &str, tasks: &[TaskMetadata]) -> Output {
    run_reify_audit("PPRDSTATUS", prd_text, tasks)
}

/// `reify-audit --pattern <pattern>` with every substrate but the task source
/// pinned: the repo, no jcodemunch, and the runs.db. The caller appends the
/// task source.
fn audit_command(pattern: &str, repo: &Path, runs_db: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_reify-audit"));
    command.args([
        "--pattern",
        pattern,
        "--no-jcodemunch",
        "--project-root",
        repo.to_str().expect("utf-8 repo path"),
        "--runs-db",
        runs_db.to_str().expect("utf-8 runs.db path"),
    ]);
    command
}

/// Run `reify-audit --pattern <pattern>` over a repo holding `prd_text`,
/// against the `tasks` corpus, with every substrate pinned at a tempdir.
fn run_reify_audit(pattern: &str, prd_text: &str, tasks: &[TaskMetadata]) -> Output {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_prd(repo.path(), prd_text);
    let tasks_file = write_tasks_json(aux.path(), tasks);
    let runs_db = write_empty_runs_db(aux.path());
    audit_command(pattern, repo.path(), &runs_db)
        .args([
            "--tasks-file",
            tasks_file.to_str().expect("utf-8 tasks.json path"),
        ])
        .output()
        .unwrap_or_else(|e| panic!("invoke reify-audit --pattern {pattern}: {e}"))
}

/// Run `reify-audit --pattern PPRDSTATUS` over a repo holding `prd_text`,
/// loading the corpus through the live fused-memory loader: a mock server
/// answers `get_tasks` with `wire_tasks`.
fn run_pprdstatus_live(prd_text: &str, wire_tasks: Value) -> Output {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_prd(repo.path(), prd_text);
    let runs_db = write_empty_runs_db(aux.path());
    let mock = common::mcp_mock::spawn_mock_mcp(move |args| {
        if args.get("id").is_some() {
            return Some(Value::Null);
        }
        Some(json!({"tasks": wire_tasks}))
    });
    let out = audit_command("PPRDSTATUS", repo.path(), &runs_db)
        .args(["--fused-memory-url", mock.url()])
        .output()
        .unwrap_or_else(|e| panic!("invoke reify-audit --pattern PPRDSTATUS (live loader): {e}"));
    mock.stop();
    out
}

/// A `get_tasks` row: decomposition leaf `id`, tagged with the PRD under `key`.
fn wire_leaf(id: u32, status: &str, key: &str, prd: &str) -> Value {
    json!({"id": id, "title": "t", "status": status, "metadata": {key: prd}})
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
///
/// Deliberately coupled to that live doc. A docs-only commit landed directly
/// on `main` runs only `hooks/pre-commit`, whose check plan is empty for a
/// docs-only stage, so an edit to the doc's header can turn this test red
/// without running it, and the red then surfaces in the next unrelated
/// merge. That cost is accepted: the doc is a frozen SHIPPED record, and an
/// edit that makes PPRDSTATUS fire on it is exactly what this test exists
/// to report.
/// The detector's own behaviour is pinned independently of the tree by
/// `pattern_pprdstatus_is_silent_on_the_post_fix_doc`.
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

/// An empty corpus leaves PPRDSTATUS nothing to check, so a run of it alone
/// refuses with 125 and prints no findings array, even over the pre-fix doc
/// that fires both lanes once the corpus is loaded. An empty array here would
/// pass for a clean result.
#[test]
fn pattern_pprdstatus_refuses_an_empty_task_corpus() {
    let out = run_pprdstatus_over(PRE_FIX, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(125), "stderr:\n{stderr}");
    assert!(
        !stderr.lines().any(|line| line.starts_with('[')),
        "a refusal must print no findings array; stderr:\n{stderr}"
    );
}

/// A mixed run over an empty corpus refuses nothing: the other detectors
/// still emit the findings array, so PPRDSTATUS's breadcrumb is the only
/// thing that marks its zero findings "not checked" rather than clean.
#[test]
fn mixed_run_over_an_empty_task_corpus_marks_pprdstatus_skipped() {
    let out = run_reify_audit("P5,PPRDSTATUS", PRE_FIX, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(0), "stderr:\n{stderr}");
    assert_eq!(
        findings_from_stderr(&stderr),
        Vec::<serde_json::Value>::new()
    );
    assert!(
        stderr
            .lines()
            .any(|line| line.contains("PPRDSTATUS skipped")),
        "a mixed run must mark PPRDSTATUS unchecked; stderr:\n{stderr}"
    );
}

/// The current `/prd` decompose tags its leaves `metadata.prd_path`, the
/// legacy one `metadata.prd`. A live leaf under either key keeps the PRD's
/// header from reading stale.
#[test]
fn live_loader_counts_a_live_prd_path_leaf() {
    let out = run_pprdstatus_live(
        PRE_FIX,
        json!([
            wire_leaf(5102, "done", "prd", KERNEL_SEAM_CONTRACTS),
            wire_leaf(5103, "pending", "prd_path", KERNEL_SEAM_CONTRACTS),
        ]),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(0), "stderr:\n{stderr}");
    assert_eq!(findings_from_stderr(&stderr), Vec::<Value>::new());
}

/// A landed `prd_path`-tagged leaf beside a cancelled `prd`-tagged bookmark
/// recommends SHIPPED, not WITHDRAWN.
#[test]
fn live_loader_stamps_shipped_when_the_landed_leaf_is_prd_path_tagged() {
    let out = run_pprdstatus_live(
        PRE_FIX,
        json!([
            wire_leaf(5102, "cancelled", "prd", KERNEL_SEAM_CONTRACTS),
            wire_leaf(5103, "done", "prd_path", KERNEL_SEAM_CONTRACTS),
        ]),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(1), "stderr:\n{stderr}");
    let findings = findings_from_stderr(&stderr);
    assert_eq!(findings.len(), 1, "{:#}", Value::Array(findings.clone()));
    let summary = findings[0]["summary"]
        .as_str()
        .expect("a finding's summary is a string");
    assert!(summary.starts_with("stale-status-header:"), "{summary}");
    assert!(
        summary.contains("(1 done, 1 cancelled: #5102, #5103)"),
        "{summary}"
    );
    assert!(summary.contains("stamp SHIPPED"), "{summary}");
}
