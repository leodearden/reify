//! End-to-end CLI coverage for `--pattern POLDCORE`.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test poldcore_cli`
//!
//! `tests/poldcore.rs` drives `poldcore::check` directly, which cannot see
//! whether the `--pattern POLDCORE` token reaches that detector, or whether
//! the live fused-memory loader carries the facts it reads. This binary runs
//! the real binary against a real git repo holding a hold list, and asserts
//! on the exit code (the High-severity count) and the findings JSON on
//! stderr. The corpus comes from a `--tasks-file`, or, in the `live_loader_*`
//! case, from `tests/common/mcp_mock.rs` in fused-memory's wire shape.

mod common;

#[path = "common/cli_fixture.rs"]
mod cli_fixture;

use cli_fixture::write_empty_runs_db;
use common::findings::{parse_findings_from_stderr, stderr_has_parseable_findings_array};
use common::fixtures::legacy_meta;
use reify_audit::TaskMetadata;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HOLD_LIST: &str = "docs/notes/old-evaluation-core-hold.toml";
const HOLD_LIST_TEXT: &str = r#"holder = 10510
files = [
    "crates/reify-eval/src/engine_eval.rs",
    "crates/reify-eval/src/engine_build/",
    "crates/reify-eval/src/lib.rs",
]
registration_only = ["crates/reify-eval/src/lib.rs"]
"#;

const HOLDER: &str = "10510";
const MISSED: &str = "900";
const RELEASED: &str = "901";
const HELD: &str = "902";
const MISSED_TITLE: &str = "Reshape the engine's build stage";
const FILES: [&str; 3] = [
    "crates/reify-eval/src/engine_eval.rs",
    "crates/reify-eval/src/engine_build/stage.rs",
    "crates/reify-gui/src/x.rs",
];

/// `git init` a repo at `dir` holding the hold list, committed so the tree
/// is a real checkout.
fn repo_with_hold_list(dir: &Path) {
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
    let path = dir.join(HOLD_LIST);
    std::fs::create_dir_all(path.parent().expect("path has a parent")).expect("create docs/notes");
    std::fs::write(&path, HOLD_LIST_TEXT).expect("write hold list");

    run(&["init", "--initial-branch=main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["add", "."]);
    run(&["commit", "-m", "seed the old-core hold list"]);
}

fn pending(id: &str, title: &str) -> TaskMetadata {
    TaskMetadata {
        status: "pending".to_string(),
        title: title.to_string(),
        files: FILES.iter().map(|f| f.to_string()).collect(),
        ..legacy_meta(id)
    }
}

/// The healthy holder, plus three tasks on the same held files: one missed,
/// one released, one held by its edge on the holder.
fn corpus() -> Vec<TaskMetadata> {
    vec![
        TaskMetadata {
            files: vec![],
            has_deferred_watch: true,
            ..pending(HOLDER, "[HOLDER] Old evaluation core")
        },
        pending(MISSED, MISSED_TITLE),
        TaskMetadata {
            has_old_core_release: true,
            ..pending(RELEASED, "Released kept-code work")
        },
        TaskMetadata {
            dependencies: vec![HOLDER.to_string()],
            ..pending(HELD, "Held engine work")
        },
    ]
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

/// `reify-audit --pattern POLDCORE` with every substrate but the task source
/// pinned: the repo, no jcodemunch, and the runs.db. The caller appends the
/// task source.
fn audit_command(repo: &Path, runs_db: &Path) -> Command {
    let mut command = Command::new(reify_test_support::test_package::bin_exe("reify-audit"));
    command.args([
        "--pattern",
        "POLDCORE",
        "--no-jcodemunch",
        "--project-root",
        repo.to_str().expect("utf-8 repo path"),
        "--runs-db",
        runs_db.to_str().expect("utf-8 runs.db path"),
    ]);
    command
}

/// Run `reify-audit --pattern POLDCORE` against the `tasks` corpus, loaded
/// from a `--tasks-file`.
fn run_poldcore(tasks: &[TaskMetadata]) -> Output {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_hold_list(repo.path());
    let tasks_file = write_tasks_json(aux.path(), tasks);
    let runs_db = write_empty_runs_db(aux.path());
    audit_command(repo.path(), &runs_db)
        .args([
            "--tasks-file",
            tasks_file.to_str().expect("utf-8 tasks.json path"),
        ])
        .output()
        .expect("invoke reify-audit --pattern POLDCORE")
}

/// Run `reify-audit --pattern POLDCORE`, loading the corpus through the live
/// fused-memory loader: a mock server answers `get_tasks` with `wire_tasks`.
fn run_poldcore_live(wire_tasks: Value) -> Output {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_hold_list(repo.path());
    let runs_db = write_empty_runs_db(aux.path());
    let mock = common::mcp_mock::spawn_mock_mcp(move |args| {
        if args.get("id").is_some() {
            return Some(Value::Null);
        }
        Some(json!({"tasks": wire_tasks}))
    });
    let out = audit_command(repo.path(), &runs_db)
        .args(["--fused-memory-url", mock.url()])
        .output()
        .expect("invoke reify-audit --pattern POLDCORE (live loader)");
    mock.stop();
    out
}

/// Asserts the run reported exactly MISSED, as one High hold-missed finding.
fn assert_only_missed(out: &Output) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr:\n{stderr}");
    let findings = parse_findings_from_stderr(&stderr);
    assert_eq!(findings.len(), 1, "{:#}", Value::Array(findings.clone()));
    let finding = &findings[0];
    assert_eq!(
        finding["pattern"].as_str(),
        Some("POldCoreHold"),
        "{finding:#}"
    );
    assert_eq!(finding["task_id"].as_str(), Some(MISSED), "{finding:#}");
    let summary = finding["summary"]
        .as_str()
        .expect("a finding's summary is a string");
    assert!(summary.starts_with("hold-missed:"), "{summary}");
    assert!(summary.contains(MISSED_TITLE), "{summary}");
}

#[test]
fn pattern_poldcore_reports_only_the_missed_task() {
    assert_only_missed(&run_poldcore(&corpus()));
}

/// An empty corpus leaves POLDCORE nothing to check, so a run of it alone
/// refuses with 125 and prints no findings array. An empty array here would
/// pass for a clean result.
#[test]
fn pattern_poldcore_refuses_an_empty_task_corpus() {
    let out = run_poldcore(&[]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(125), "stderr:\n{stderr}");
    assert!(
        stderr.contains("the task corpus is empty"),
        "the 125 must be the empty-corpus refusal, not an argument error; \
         stderr:\n{stderr}"
    );
    assert!(
        !stderr_has_parseable_findings_array(&stderr),
        "a refusal must print no findings array; stderr:\n{stderr}"
    );
}

/// The same corpus in fused-memory's wire shape: the production loader must
/// carry the holder's deferred watch, HELD's top-level edge and RELEASED's
/// release, or the run reports more than MISSED.
#[test]
fn live_loader_carries_the_hold_facts_end_to_end() {
    let wire = |id: &str, title: &str, deps: Value, metadata: Value| {
        let mut metadata = metadata;
        metadata["files"] = json!(FILES);
        json!({
            "id": id.parse::<u64>().expect("numeric id"),
            "title": title,
            "status": "pending",
            "dependencies": deps,
            "metadata": metadata,
        })
    };
    let holder = json!({
        "id": 10510,
        "title": "[HOLDER] Old evaluation core",
        "status": "pending",
        "dependencies": [],
        "metadata": {
            "files": [],
            "deferred_watch": "Never dispatches while the new evaluation core is being built."
        }
    });
    let out = run_poldcore_live(json!([
        holder,
        wire(MISSED, MISSED_TITLE, json!([]), json!({})),
        wire(
            RELEASED,
            "Released kept-code work",
            json!([]),
            json!({"old_core_release": {
                "reason": "kept-code work the new engine needs",
                "approved_by": "Leo",
                "approved_at": "2026-11-25"
            }}),
        ),
        wire(HELD, "Held engine work", json!([10510]), json!({})),
    ]));

    assert_only_missed(&out);
}
