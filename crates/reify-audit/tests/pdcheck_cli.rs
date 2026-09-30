//! End-to-end CLI coverage for `--pattern PDCHECK`.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test pdcheck_cli`
//!
//! `tests/pdcheck.rs` drives the lane's functions directly, and the binary's
//! inline tests cover the `run_pdcheck` predicate — but neither reaches the
//! dispatch arm that makes the detector reachable from a command line. Swap
//! that arm's body for another lane's, or delete it, and both suites stay
//! green. This binary closes that gap the way `tests/cli.rs` does it for
//! PTODO: a real git repo, a real on-disk `tasks.db`, the real binary, and
//! assertions on the process exit code plus the emitted findings JSON.
//!
//! The two file-fixture helpers below are deliberate local copies of
//! `tests/cli.rs`'s: they are three lines each, and sharing them would mean
//! editing that binary's helper block, which is not this change's to touch.

mod common;

use std::path::Path;
use std::process::Command;

/// The live #5778 shape: #5791 relocated this file out of `reify-eval`,
/// leaving two `expect: present` rows naming a path git no longer tracks.
const DEAD: &str = "crates/reify-eval/src/arg_acceptance.rs";
const LIVE: &str = "crates/reify-ir/src/arg_acceptance.rs";

/// `git init` a repo at `dir` holding [`DEAD`], then `git mv` it to [`LIVE`]
/// in a second commit. Afterwards `git ls-files` lists only [`LIVE`], while
/// `git log -1 -- <DEAD>` still resolves — the exact three-way state the lane
/// must read as "dead, and repointable".
fn repo_with_relocated_file(dir: &Path) {
    let run = |args: &[&str]| {
        let status = common::git_env::git_cmd(dir)
            .args(args)
            .status()
            .expect("git command failed to spawn");
        assert!(status.success(), "git {:?} exited {:?}", args, status.code());
    };
    let write = |rel: &str| {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("path has a parent"))
            .expect("create fixture dirs");
        std::fs::write(&path, "pub fn angle_spec() {}\n").expect("write fixture");
    };

    run(&["init", "--initial-branch=main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    write(DEAD);
    run(&["add", "."]);
    run(&["commit", "-m", "seed arg_acceptance under reify-eval"]);

    std::fs::create_dir_all(dir.join(LIVE).parent().expect("path has a parent"))
        .expect("create destination dir");
    run(&["mv", DEAD, LIVE]);
    run(&["commit", "-m", "relocate arg_acceptance to reify-ir"]);
}

fn write_empty_tasks_json(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("tasks.json");
    std::fs::write(&path, "[]").expect("write tasks.json");
    path
}

fn write_empty_runs_db(dir: &Path) -> std::path::PathBuf {
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

/// Build the invocation, with every substrate the binary needs pinned at a
/// tempdir so no ambient file or env var can decide the outcome.
fn pdcheck_command(repo: &Path, aux: &Path) -> Command {
    let tasks_file = write_empty_tasks_json(aux);
    let runs_db = write_empty_runs_db(aux);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_reify-audit"));
    cmd.args([
        "--pattern",
        "PDCHECK",
        "--no-jcodemunch",
        "--project-root",
        repo.to_str().expect("utf-8 repo path"),
        "--tasks-file",
        tasks_file.to_str().expect("utf-8 tasks.json path"),
        "--runs-db",
        runs_db.to_str().expect("utf-8 runs.db path"),
    ]);
    // The DB location must come from --project-root, not from whatever the
    // ambient shell exported (see `ptodo::tasks_db_path`).
    cmd.env_remove("REIFY_PTODO_TASKS_DB");
    cmd
}

/// The whole wiring, end to end: a seeded unsatisfiable row reaches stderr as
/// one High `PDeliveredCheckPath` finding and sets the process exit code,
/// which is the High-severity count.
#[test]
fn pattern_pdcheck_reports_a_seeded_unsatisfiable_row_and_exits_one() {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_relocated_file(repo.path());

    common::schema::seed_tasks_db_at_with_metadata(
        &repo.path().join(".taskmaster/tasks/tasks.db"),
        &[(
            "master",
            5778,
            "deferred",
            &format!(
                r#"{{"delivered_checks":[{{"name":"angle-spec-absent-today","kind":"grep","expect":"present","pattern":"pub fn angle_spec","paths":["{DEAD}"]}}]}}"#
            ),
        )],
    );

    let out = pdcheck_command(repo.path(), aux.path())
        .output()
        .expect("invoke reify-audit --pattern PDCHECK");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(1),
        "the exit code is the High-severity count, and this run has exactly one; \
         got {:?}\nstderr:\n{stderr}",
        out.status.code()
    );

    let findings = findings_from_stderr(&stderr);
    assert_eq!(
        findings.len(),
        1,
        "exactly one finding for the one seeded row; got:\n{:#}",
        serde_json::Value::Array(findings.clone())
    );
    let finding = &findings[0];
    assert_eq!(
        finding["pattern"].as_str(),
        Some("PDeliveredCheckPath"),
        "the dispatch arm must route to the PDCHECK lane, not another detector: {finding:#}"
    );
    assert_eq!(finding["severity"].as_str(), Some("High"));
    assert_eq!(finding["task_id"].as_str(), Some("5778"));
    assert!(
        finding["summary"]
            .as_str()
            .is_some_and(|s| s.starts_with("delivered-check-unsatisfiable-path:")),
        "summary must carry the finding kind as its prefix: {finding:#}"
    );
    assert!(
        finding["summary"].as_str().is_some_and(|s| s.contains(LIVE)),
        "the repair hint must name the rename target the fixer repoints to: {finding:#}"
    );
}

/// The opt-in half of the same wiring: a healthy backlog leaves the run
/// exit-neutral, so the detector cannot turn a clean sweep red on its own.
#[test]
fn pattern_pdcheck_on_a_healthy_backlog_exits_zero() {
    let repo = tempfile::tempdir().expect("create repo tempdir");
    let aux = tempfile::tempdir().expect("create aux tempdir");
    repo_with_relocated_file(repo.path());

    common::schema::seed_tasks_db_at_with_metadata(
        &repo.path().join(".taskmaster/tasks/tasks.db"),
        &[(
            "master",
            5778,
            "deferred",
            &format!(
                r#"{{"delivered_checks":[{{"name":"angle-spec-present-today","kind":"grep","expect":"present","pattern":"pub fn angle_spec","paths":["{LIVE}"]}}]}}"#
            ),
        )],
    );

    let out = pdcheck_command(repo.path(), aux.path())
        .output()
        .expect("invoke reify-audit --pattern PDCHECK");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(0),
        "a row whose path still resolves is not a finding; stderr:\n{stderr}"
    );
}
