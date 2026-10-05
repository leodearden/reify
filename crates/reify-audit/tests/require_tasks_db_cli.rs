//! End-to-end CLI coverage for `--require-tasks-db`.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test require_tasks_db_cli`
//!
//! Without the flag, a task DB the PTODO lanes cannot use degrades them
//! fail-soft (PRD §6.7): a breadcrumb on stderr, and a run that can look
//! clean. With it, the same condition is a refusal — exit 255, naming the
//! resolved path, and no findings array (PRD §6.7 amendment, ruling §19(b)).
//! The consumer is dark-factory 5796's cadenced sweep, which must tell "the
//! DB said nothing is orphaned" from "nothing was checked".

mod common;

#[path = "common/cli_fixture.rs"]
mod cli_fixture;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cli_fixture::{write_empty_runs_db, write_empty_tasks_json};
use serde_json::Value;

/// The refusal code. 1–254 is the High-finding count and 125 is the
/// arg/IO error, so this is the only code that collides with neither.
const TASKS_DB_ABSENT_EXIT: i32 = 255;

/// The arg/IO error code: what a flag combination that can guard nothing gets.
const ERROR_EXIT: i32 = 125;

/// The §6.7 fail-soft breadcrumb fragment: present exactly when the
/// DB-backed lanes degraded.
const DEGRADE_MARK: &str = "lanes degraded";

/// The marker's cited task, seeded `done` wherever a DB exists, so a run that
/// really consulted the DB reports it as one High `orphaned:` finding.
const CITED_ID: i64 = 4444;

/// A one-commit repo whose only file carries a marker citing [`CITED_ID`].
fn repo_with_cited_marker(dir: &Path) {
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
    run(&["init", "--initial-branch=main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    std::fs::write(
        dir.join("cited.rs"),
        format!("// TODO(#{CITED_ID}): wire the orphaned-cite path\n"),
    )
    .expect("write cited.rs");
    run(&["add", "."]);
    run(&["commit", "-m", "seed a cited marker"]);
}

/// The repo's default task-DB location (`ptodo::tasks_db_path` without the
/// env override).
fn default_tasks_db(repo: &Path) -> PathBuf {
    repo.join(".taskmaster/tasks/tasks.db")
}

/// Seed a task DB at `path` in which [`CITED_ID`] is `done`.
fn seed_cited_id_done(path: &Path) {
    common::schema::seed_tasks_db_at(path, &[("master", CITED_ID, "done")]);
}

/// `--pattern PTODO` over `repo`, every other substrate pinned under `aux`.
/// The ambient `REIFY_PTODO_TASKS_DB` is always removed; a test that needs
/// the override sets it on the returned command.
fn ptodo_command(repo: &Path, aux: &Path, require: bool) -> Command {
    pattern_command("PTODO", repo, aux, require)
}

/// [`ptodo_command`] with any `--pattern` value.
fn pattern_command(pattern: &str, repo: &Path, aux: &Path, require: bool) -> Command {
    let tasks_file = write_empty_tasks_json(aux);
    let runs_db = write_empty_runs_db(aux);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_reify-audit"));
    cmd.args([
        "--pattern",
        pattern,
        "--no-jcodemunch",
        "--project-root",
        repo.to_str().expect("utf-8 repo path"),
        "--tasks-file",
        tasks_file.to_str().expect("utf-8 tasks.json path"),
        "--runs-db",
        runs_db.to_str().expect("utf-8 runs.db path"),
    ]);
    if require {
        cmd.arg("--require-tasks-db");
    }
    cmd.env_remove("REIFY_PTODO_TASKS_DB");
    cmd
}

fn run(mut cmd: Command) -> (Option<i32>, String) {
    let out: Output = cmd.output().expect("invoke reify-audit");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The findings array the binary appends to stderr, if one parses.
fn findings_array(stderr: &str) -> Option<Vec<Value>> {
    let start = stderr
        .rfind("\n[")
        .map(|pos| pos + 1)
        .or_else(|| stderr.starts_with('[').then_some(0))?;
    serde_json::from_str(&stderr[start..]).ok()
}

fn has_findings_array(stderr: &str) -> bool {
    findings_array(stderr).is_some()
}

fn findings(stderr: &str) -> Vec<Value> {
    findings_array(stderr)
        .unwrap_or_else(|| panic!("no parseable findings array in stderr:\n{stderr}"))
}

/// Whether `findings` holds the High `orphaned:` finding for `cited.rs`
/// naming [`CITED_ID`] — the proof that the DB-backed lanes ran.
fn has_cited_orphan(findings: &[Value]) -> bool {
    findings.iter().any(|f| {
        f["task_id"].as_str() == Some("cited.rs")
            && f["severity"].as_str() == Some("High")
            && f["summary"]
                .as_str()
                .is_some_and(|s| s.starts_with("orphaned:") && s.contains(&format!("#{CITED_ID}")))
    })
}

/// The refusal shape every `--require-tasks-db` refusal shares, wherever it
/// fires: the dedicated code, the resolved path, and no findings array.
fn assert_refused_naming(code: Option<i32>, stderr: &str, path: &Path) {
    assert_eq!(
        code,
        Some(TASKS_DB_ABSENT_EXIT),
        "a task DB the lanes cannot use under --require-tasks-db must refuse with the \
         dedicated code; stderr:\n{stderr}"
    );
    let path = path.display().to_string();
    assert!(
        stderr.contains(&path),
        "the refusal must name the resolved path {path}; stderr:\n{stderr}"
    );
    assert!(
        !has_findings_array(stderr),
        "a refused run must emit no findings array; stderr:\n{stderr}"
    );
}

/// A DB that cannot be opened is refused before any detector runs, so no lane
/// even degrades.
fn assert_refused_before_any_detector(code: Option<i32>, stderr: &str, path: &Path) {
    assert_refused_naming(code, stderr, path);
    assert!(
        !stderr.contains(DEGRADE_MARK),
        "an unopenable DB is refused before the lanes, not degraded; stderr:\n{stderr}"
    );
}

#[test]
fn absent_db_fails_loud_where_the_flagless_run_fails_soft() {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_cited_marker(repo.path());

    let (code, stderr) = run(ptodo_command(repo.path(), aux.path(), false));
    assert_eq!(
        code,
        Some(0),
        "without the flag an absent DB degrades to a clean-looking run; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains(DEGRADE_MARK),
        "the flagless run must leave the §6.7 breadcrumb; stderr:\n{stderr}"
    );

    let (code, stderr) = run(ptodo_command(repo.path(), aux.path(), true));
    assert_refused_before_any_detector(code, &stderr, &default_tasks_db(repo.path()));
}

#[test]
fn required_path_is_resolved_through_the_env_override() {
    let aux = tempfile::tempdir().expect("create aux tempdir");

    // The default path is absent, so only an honoured override can turn the
    // refusal into a DB-backed run.
    let repo_without_db = tempfile::tempdir().expect("create repo tempdir");
    repo_with_cited_marker(repo_without_db.path());
    let override_db = aux.path().join("override-tasks.db");
    seed_cited_id_done(&override_db);
    let mut cmd = ptodo_command(repo_without_db.path(), aux.path(), true);
    cmd.env("REIFY_PTODO_TASKS_DB", &override_db);
    let (code, stderr) = run(cmd);
    assert_eq!(
        code,
        Some(1),
        "a present override DB must run the lanes; stderr:\n{stderr}"
    );
    assert!(
        has_cited_orphan(&findings(&stderr)),
        "the override DB's done task must surface as orphaned; stderr:\n{stderr}"
    );

    // The default path is healthy, so only an honoured override can turn the
    // DB-backed run into a refusal.
    let repo_with_db = tempfile::tempdir().expect("create repo tempdir");
    repo_with_cited_marker(repo_with_db.path());
    seed_cited_id_done(&default_tasks_db(repo_with_db.path()));
    let missing_db = aux.path().join("missing-tasks.db");
    let mut cmd = ptodo_command(repo_with_db.path(), aux.path(), true);
    cmd.env("REIFY_PTODO_TASKS_DB", &missing_db);
    let (code, stderr) = run(cmd);
    assert_refused_before_any_detector(code, &stderr, &missing_db);
}

/// An existing file that is not a tasks DB opens fine and only fails inside
/// the lanes, so the requirement must be enforced on whether the lanes really
/// used the DB, not on whether the path opened.
#[test]
fn a_file_that_opens_but_is_not_a_tasks_db_is_refused() {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_cited_marker(repo.path());
    let not_a_tasks_db = aux.path().join("empty-tasks.db");
    std::fs::write(&not_a_tasks_db, "").expect("write an empty file");

    let mut cmd = ptodo_command(repo.path(), aux.path(), false);
    cmd.env("REIFY_PTODO_TASKS_DB", &not_a_tasks_db);
    let (code, stderr) = run(cmd);
    assert_eq!(
        code,
        Some(0),
        "without the flag a non-tasks DB degrades to a clean-looking run; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains(DEGRADE_MARK),
        "the precondition: the lanes degrade on this file; stderr:\n{stderr}"
    );

    let mut cmd = ptodo_command(repo.path(), aux.path(), true);
    cmd.env("REIFY_PTODO_TASKS_DB", &not_a_tasks_db);
    let (code, stderr) = run(cmd);
    assert_refused_naming(code, &stderr, &not_a_tasks_db);
}

/// Only PTODO reads the task DB. A run set without it has no lane to guard,
/// so the flag is an argument conflict (125), not a DB refusal (255) and not
/// a silent no-op that reads as "the DB requirement held".
#[test]
fn a_run_set_without_ptodo_rejects_the_flag_as_an_arg_conflict() {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_cited_marker(repo.path());
    seed_cited_id_done(&default_tasks_db(repo.path()));

    let (code, stderr) = run(pattern_command("P2", repo.path(), aux.path(), true));
    assert_eq!(
        code,
        Some(ERROR_EXIT),
        "--pattern P2 selects no task-DB reader; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("--require-tasks-db") && stderr.contains("PTODO"),
        "the conflict must name the flag and the detector it guards; stderr:\n{stderr}"
    );
    assert!(
        !has_findings_array(&stderr),
        "an arg conflict runs no detector; stderr:\n{stderr}"
    );

    let (code, stderr) = run(pattern_command("P2,PTODO", repo.path(), aux.path(), true));
    assert_eq!(
        code,
        Some(1),
        "a mixed run set that includes PTODO keeps the flag; stderr:\n{stderr}"
    );
    assert!(
        has_cited_orphan(&findings(&stderr)),
        "the DB-backed lanes must have run; stderr:\n{stderr}"
    );
}

#[test]
fn present_db_is_behaviour_identical_to_the_flagless_run() {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_cited_marker(repo.path());
    seed_cited_id_done(&default_tasks_db(repo.path()));

    let (flagless_code, flagless_stderr) = run(ptodo_command(repo.path(), aux.path(), false));
    let (flagged_code, flagged_stderr) = run(ptodo_command(repo.path(), aux.path(), true));

    assert_eq!(flagless_code, Some(1), "stderr:\n{flagless_stderr}");
    assert_eq!(flagged_code, Some(1), "stderr:\n{flagged_stderr}");

    let flagged = findings(&flagged_stderr);
    assert_eq!(
        flagged,
        findings(&flagless_stderr),
        "a present DB makes the flag a no-op on the findings"
    );
    assert!(
        has_cited_orphan(&flagged),
        "the DB-backed lanes must have run; stderr:\n{flagged_stderr}"
    );
    assert!(
        !flagged_stderr.contains(DEGRADE_MARK),
        "a present DB must not degrade; stderr:\n{flagged_stderr}"
    );
}
