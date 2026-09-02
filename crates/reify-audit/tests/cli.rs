//! Integration tests for the `reify-audit` CLI binary.
//!
//! Tests invoke the compiled binary via `env!("CARGO_BIN_EXE_reify-audit")`
//! and assert on stdout, stderr, and exit codes.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test cli`

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;

mod common;

/// The per-call fail-soft breadcrumb literals, shared with
/// `tests/jcodemunch_live.rs`.
///
/// `#[path]` rather than a `common::` re-export: the two binaries consume
/// these with opposite polarity — the tests here assert the literal is
/// PRESENT in the real binary's stderr, `assert_live_leg` over there asserts
/// it is ABSENT — and an absence check is only meaningful against a string
/// the binary can actually emit. See the module's own header.
#[path = "common/breadcrumbs.rs"]
mod breadcrumbs;

/// The `tasks.json` record fixtures, shared with `tests/jcodemunch_live.rs`.
///
/// `#[path]` for the same reason as `breadcrumbs` above: two binaries need a
/// P1-ELIGIBLE record, and a fixture that gets P1's eligibility rules wrong
/// does not fail — it goes vacuous. Keeping the rules in one module is what
/// stops the two copies from drifting. See the module's own header.
#[path = "common/task_json.rs"]
mod task_json;

use task_json::{done_task_fixture, task_fixture};

// -----------------------------------------------------------------------
// Fixture helpers
// -----------------------------------------------------------------------

/// [`task_fixture`] with a caller-controlled `files` list.
///
/// [`task_fixture`] hardcodes `files: ["crates/reify-audit/src/lib.rs"]`, which
/// is tracked on main and therefore always corroborates. The pre-done landing
/// tests need the declared deliverable set to be the variable under test.
fn task_fixture_with_files(
    task_id: &str,
    status: &str,
    kind: Option<&str>,
    commit: Option<&str>,
    files: &[&str],
) -> serde_json::Value {
    let mut v = task_fixture(task_id, status, kind, commit);
    v["files"] = serde_json::json!(files);
    v
}

/// Absolute path to this workspace's git repository root.
///
/// `CARGO_MANIFEST_DIR` is `<root>/crates/reify-audit`, so two levels up is the
/// worktree root. Tests that exercise `path_tracked_on` must point
/// `--project-root` at a REAL git repo — a bare `tempfile::tempdir()` is not one,
/// so every `git ls-tree` there fails and `path_tracked_on` fail-safes to
/// `false` for every path, which would make an "absent from main" assertion
/// vacuously true.
fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("canonicalize repo root from CARGO_MANIFEST_DIR/../..")
}

/// Write tasks.json with the given task fixtures to `dir/tasks.json`.
fn write_tasks_json(dir: &Path, tasks: &[serde_json::Value]) -> std::path::PathBuf {
    let path = dir.join("tasks.json");
    let content = serde_json::to_string_pretty(tasks).expect("serialize tasks.json");
    std::fs::write(&path, content).expect("write tasks.json");
    path
}

/// Create a minimal SQLite `runs.db` in `dir` with just the `events` table
/// (verbatim schema from `crates/reify-audit/tests/p5.rs:32`). Returns the path.
fn write_empty_runs_db(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("runs.db");
    let conn = rusqlite::Connection::open(&path).expect("open runs.db");
    conn.execute_batch("CREATE TABLE events (task_id TEXT, event_type TEXT);")
        .expect("create events table");
    path
}

/// Insert a task_completed event into runs.db.
fn insert_completed_event(db_path: &Path, task_id: &str) {
    let conn = rusqlite::Connection::open(db_path).expect("open runs.db");
    conn.execute(
        "INSERT INTO events (task_id, event_type) VALUES (?, 'task_completed')",
        rusqlite::params![task_id],
    )
    .expect("insert task_completed event");
}

/// Extract the JSON findings array from binary stderr.
///
/// The binary writes git diagnostic messages (from `RealGitOps::run_or_warn`)
/// to stderr BEFORE writing the JSON array. Those messages start with
/// "reify-audit: " and appear on lines before the `[` that opens the JSON.
///
/// We search for the LAST `\n[` in the output so that any earlier diagnostic
/// line that happens to contain `[` (e.g. a path with brackets, a git error
/// message like `[detached HEAD]`) doesn't corrupt the parse boundary. The
/// JSON array is always the final block; `rfind("\n[")` reliably locates it.
///
/// This keeps tests robust to git failures in temp dirs (which aren't real
/// git repositories).
fn parse_findings_from_stderr(stderr: &str) -> Vec<serde_json::Value> {
    let json_start = find_findings_array_start(stderr)
        .unwrap_or_else(|| panic!("no JSON array found in stderr; full stderr:\n{stderr}"));
    serde_json::from_str(&stderr[json_start..]).unwrap_or_else(|e| {
        panic!(
            "stderr does not contain valid JSON after '[': {e}\nstderr:\n{stderr}"
        )
    })
}

/// Byte offset where the findings array begins, or `None` if there is none.
///
/// Factored out of [`parse_findings_from_stderr`] so the positive direction
/// ("an array was emitted") and the negative direction
/// ([`stderr_has_parseable_findings_array`], "no array was emitted") share ONE
/// definition of the parse boundary and cannot drift apart.
fn find_findings_array_start(stderr: &str) -> Option<usize> {
    stderr
        .rfind("\n[")
        .map(|pos| pos + 1) // skip the '\n', keep the '['
        .or_else(|| {
            // Fallback: JSON starts at position 0 (no preceding diagnostic lines).
            if stderr.starts_with('[') { Some(0) } else { None }
        })
}

/// Whether stderr carries a parseable findings array — non-panicking, for
/// asserting the ABSENCE of one.
///
/// This models the `/audit` skill's exit-125 disambiguator, which separates an
/// infrastructure error from "125 High-severity findings" on exactly this
/// property: successful runs always emit a JSON array on stderr, while error
/// paths emit human-readable text and never produce parseable JSON. Tests that
/// assert a refusal emits no array must therefore ask the same question the
/// skill asks, rather than assuming a panic means absence.
#[allow(dead_code)]
fn stderr_has_parseable_findings_array(stderr: &str) -> bool {
    match find_findings_array_start(stderr) {
        Some(start) => {
            serde_json::from_str::<Vec<serde_json::Value>>(&stderr[start..]).is_ok()
        }
        None => false,
    }
}

/// Recursively copy the directory tree at `src` into `dst` (creating `dst`).
/// Used to lift the committed `tests/fixtures/ptodo/` tree into a throwaway
/// git repo so its root-relative paths escape the live `crates/reify-audit/`
/// allowlist (the detector keys the allowlist off the project-root-relative
/// path, and here the project root IS the fixture root).
fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create dst dir");
    for entry in std::fs::read_dir(src).expect("read_dir src") {
        let entry = entry.expect("dir entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("file_type").is_dir() {
            copy_dir_recursive(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy file");
        }
    }
}

/// `git init` + add + commit every file under `dir` (identity + gpgsign
/// disabled). After this, `git -C <dir> ls-files` returns every fixture path
/// so `RealGitOps::ls_files` enumerates them for the PTODO structural sweep.
///
/// Builds every invocation through `common::git_env::git_cmd`, as does
/// `tests/real_git_ops.rs` — both now share the single
/// `reify_audit::git_env` constructor. That is load-bearing, not tidiness:
/// under a git hook, `GIT_INDEX_FILE` (a *temporary* index, especially for
/// `git commit --only`) and `GIT_DIR` are exported into the whole process
/// tree and override `-C <tempdir>`, so a bare `git -C <tempdir> add .`
/// writes the PARENT repository's index instead of this one.
fn git_init_commit_all(dir: &Path) {
    let run = |args: &[&str]| {
        let status = common::git_env::git_cmd(dir)
            .args(args)
            .status()
            .expect("git command failed to spawn");
        assert!(status.success(), "git {:?} exited {:?}", args, status.code());
    };
    run(&["init", "--initial-branch=main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["add", "."]);
    run(&["commit", "-m", "ptodo fixtures"]);
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

mod cli {
    use super::*;

    /// Smoke test: `--help` exits 0 and mentions the four key flags.
    #[test]
    fn binary_help_succeeds() {
        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .arg("--help")
            .output()
            .expect("failed to invoke reify-audit --help");

        assert_eq!(
            out.status.code(),
            Some(0),
            "--help must exit 0; got {:?}\nstdout: {}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );

        let stdout = String::from_utf8_lossy(&out.stdout);
        for flag in &["--task", "--pre-done", "--since", "--pattern", "--jcodemunch-url", "--no-jcodemunch"] {
            assert!(
                stdout.contains(flag),
                "--help stdout must contain '{}'\nFull stdout:\n{}",
                flag,
                stdout
            );
        }
    }

    /// `--task <id> --pre-done` on a done/merged task with an empty `events`
    /// table should produce a P5PhantomDone High finding and exit non-zero.
    #[test]
    fn pre_done_phantom_done_emits_high_finding() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks = vec![task_fixture("3242", "done", Some("merged"), Some("deadbeef"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task",
                "3242",
                "--pre-done",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --task 3242 --pre-done");

        // Exit code must be non-zero (at least one High finding)
        let code = out.status.code().unwrap_or(1);
        assert!(
            code >= 1,
            "expected non-zero exit for phantom-done; got {}\nstdout: {}\nstderr: {}",
            code,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );

        // Stderr must contain the JSON array of findings
        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);

        // Must contain a P5PhantomDone High finding for task 3242
        let p5_high = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["severity"].as_str() == Some("High")
                && f["task_id"].as_str() == Some("3242")
        });
        assert!(
            p5_high.is_some(),
            "expected P5PhantomDone/High/3242 in findings; got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // Stdout must contain the task id in the summary
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("3242"),
            "stdout summary must mention task 3242\nstdout: {}",
            stdout
        );
    }

    /// The pre-done gate must REFUSE a done-flip whose declared deliverable is
    /// absent from main, in the state the hook ACTUALLY sees: pre-transition
    /// status and no persisted `done_provenance`.
    ///
    /// Why that is the real state: fused-memory's `task_interceptor.py` fires
    /// the hook at step "2d" BEFORE the write, so the live `get_task` returns
    /// "in-progress"/"review" (never "done"), and `done_provenance` is only
    /// accumulated in the interceptor's in-memory `audit_fields` — it is not
    /// persisted until after the hook returns. The upstream hook template
    /// (`middleware/pre_done_hook.py`) substitutes only `{id}`, with no env
    /// injection and no stdin, so the subprocess receives no task state beyond
    /// the id. A gate that requires either signal is structurally unable to
    /// fire on the transition it exists to guard.
    ///
    /// The control tasks pin the other two directions: a task with no declared
    /// deliverable (research / ops / escalation work) must never be refused,
    /// and a task whose deliverable IS on main must pass cleanly.
    ///
    /// Control (c) is what makes (a) non-vacuous. `path_tracked_on` fail-safes
    /// to `false` on ANY git error, so (a)'s refusal would still be asserted if
    /// `git ls-tree main` were broken for every path in this environment
    /// (missing `main` ref, sanitised `GIT_DIR`, shallow checkout) — and (b)
    /// returns before any git call at all. (c) fails loudly the moment real git
    /// stops resolving `main`, which is the property `repo_root()`'s doc
    /// comment claims but nothing previously pinned.
    #[test]
    fn pre_done_gate_refuses_unlanded_task_without_provenance() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();
        let root = repo_root();

        let tasks = vec![
            // (a) unlanded deliverable — must be refused.
            task_fixture_with_files(
                "63451",
                "in-progress",
                None,
                None,
                &["crates/reify-audit-ghost/src/lib.rs"],
            ),
            // (b) control: no declared deliverable — must flip freely.
            task_fixture_with_files("63452", "in-progress", None, None, &[]),
            // (c) positive control: a deliverable that IS tracked on main.
            task_fixture_with_files(
                "63453",
                "in-progress",
                None,
                None,
                &["crates/reify-audit/src/lib.rs"],
            ),
        ];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let run = |task_id: &str| {
            Command::new(bin)
                .args([
                    "--task",
                    task_id,
                    "--pre-done",
                    "--tasks-file",
                    tasks_file.to_str().unwrap(),
                    "--runs-db",
                    runs_db.to_str().unwrap(),
                    "--project-root",
                    root.to_str().unwrap(),
                ])
                .output()
                .expect("invoke reify-audit --pre-done")
        };

        // (a) The refusal.
        let out = run("63451");
        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        let p5_high = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["severity"].as_str() == Some("High")
                && f["task_id"].as_str() == Some("63451")
        });
        assert!(
            p5_high.is_some(),
            "pre-done gate must emit P5PhantomDone/High/63451 for a deliverable \
             absent from main; got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
        let code = out.status.code().unwrap_or(1);
        assert!(
            code >= 1,
            "pre-done gate must exit non-zero (refusing the done-flip); got {}\n\
             stdout: {}\nstderr: {}",
            code,
            String::from_utf8_lossy(&out.stdout),
            stderr
        );

        // (b) The control: no deliverable declared → nothing to corroborate.
        let out = run("63452");
        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "a task with an empty metadata.files must never be refused; got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
        assert_eq!(
            out.status.code(),
            Some(0),
            "empty-files control must exit 0\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            stderr
        );

        // (c) The positive control: this file is tracked on main, so the
        // healthy-flip leg must close the gate WITHOUT reaching any rescue.
        // If real git ever stops resolving `main` here, this is the assertion
        // that fails — which is precisely what keeps (a) honest.
        let out = run("63453");
        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "a deliverable tracked on main must pass the pre-done gate cleanly — a \
             finding here means `git ls-tree {} -- crates/reify-audit/src/lib.rs` did \
             not resolve, which would also make case (a)'s refusal vacuous; got:\n{:#}",
            "main",
            serde_json::Value::Array(findings.clone())
        );
        assert_eq!(
            out.status.code(),
            Some(0),
            "tracked-on-main control must exit 0\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            stderr
        );
    }

    /// `REIFY_AUDIT_PREDONE_WARN_ONLY` is scoped to the pre-done landing
    /// refusal ONLY — it must never mute a sweep High.
    ///
    /// The break-glass exists so an operator can soak a fail-closed gate
    /// without a red-tier fused-memory restart. If it leaked into the sweep it
    /// would become a general P5 mute, silently disarming the phantom-done
    /// detector for every task on the box that inherits the env var.
    #[test]
    fn pre_done_warn_only_env_does_not_mute_sweep_high() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        // A done/merged task with an empty `events` table: the classic sweep
        // High, reached through the provenance path, not the pre-done leg.
        let tasks = vec![task_fixture("3242", "done", Some("merged"), Some("deadbeef"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task",
                "3242",
                "--pattern",
                "P5",
                "--no-jcodemunch",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .env("REIFY_AUDIT_PREDONE_WARN_ONLY", "1")
            .output()
            .expect("invoke reify-audit sweep with the break-glass set");

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        let high = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["severity"].as_str() == Some("High")
                && f["task_id"].as_str() == Some("3242")
        });
        assert!(
            high.is_some(),
            "the pre-done break-glass must not downgrade a SWEEP High — that would \
             make it a general P5 mute; got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
        assert!(
            out.status.code().unwrap_or(0) >= 1,
            "a sweep High must still exit non-zero with the break-glass set; got {:?}\n\
             stdout: {}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            stderr
        );
    }

    /// `REIFY_AUDIT_PREDONE_WARN_ONLY=1` makes the pre-done refusal advisory:
    /// the finding is still emitted, at Low, and the exit code drops to 0.
    ///
    /// Why the hatch exists: this gate is fail-closed production infrastructure
    /// that had never emitted a finding before this task. Without an escape
    /// hatch, an operator hit by a misfire must edit
    /// `~/.config/systemd/user/fused-memory.service` and restart fused-memory —
    /// a red-tier restart under a dirty-start guard. Mirrors the house
    /// `REIFY_MAIN_GATE_BYPASS` / `REIFY_STASH_GUARD_BYPASS` convention.
    ///
    /// This must be a subprocess test, not a library one: `std::env::set_var`
    /// is process-global and would race sibling tests under `cargo test`'s
    /// thread-per-test model (the same hazard `tests/common/git_env.rs`
    /// documents at length for `GIT_DIR`).
    #[test]
    fn pre_done_warn_only_env_downgrades_refusal_to_low() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();
        let root = repo_root();

        let tasks = vec![task_fixture_with_files(
            "63451",
            "in-progress",
            None,
            None,
            &["crates/reify-audit-ghost/src/lib.rs"],
        )];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let run = |warn_only: bool| {
            let mut cmd = Command::new(bin);
            cmd.args([
                "--task",
                "63451",
                "--pre-done",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                root.to_str().unwrap(),
            ]);
            if warn_only {
                cmd.env("REIFY_AUDIT_PREDONE_WARN_ONLY", "1");
            } else {
                cmd.env_remove("REIFY_AUDIT_PREDONE_WARN_ONLY");
            }
            cmd.output().expect("invoke reify-audit --pre-done")
        };

        // Break-glass active: advisory.
        let out = run(true);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        let low = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["task_id"].as_str() == Some("63451")
        });
        let low = low.unwrap_or_else(|| {
            panic!(
                "warn-only must PRESERVE the finding, only drop its blocking effect; \
                 got:\n{:#}",
                serde_json::Value::Array(findings.clone())
            )
        });
        assert_eq!(
            low["severity"].as_str(),
            Some("Low"),
            "warn-only must downgrade the refusal to Low; got:\n{:#}",
            low
        );
        assert_eq!(
            out.status.code(),
            Some(0),
            "warn-only must exit 0 (the exit code counts Highs)\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            stderr
        );

        // Default is ARMED.
        let out = run(false);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        let high = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["severity"].as_str() == Some("High")
                && f["task_id"].as_str() == Some("63451")
        });
        assert!(
            high.is_some(),
            "without the env var the gate must stay ARMED (High); got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
        assert!(
            out.status.code().unwrap_or(1) >= 1,
            "armed default must exit non-zero; got {:?}",
            out.status.code()
        );
    }

    /// The pre-done gate must ACCEPT a task whose declared deliverable was
    /// RENAMED away by its own landing commit.
    ///
    /// This is the end-to-end half of
    /// `changed_paths_in_commit_reports_both_sides_of_a_rename`: that test pins
    /// the seam's return value, this one pins the GATE's verdict — the thing
    /// that was actually broken. A seam-only guard would still let a future
    /// change reintroduce the false refusal somewhere between the seam and
    /// `check_pre_done_landing`.
    ///
    /// The shape mirrors the reported reproduction exactly: `a/old.rs` on main,
    /// a branch that `git mv`s it to `a/new.rs`, merged `--no-ff`, and a task
    /// declaring BOTH paths with no `done_provenance`. `a/old.rs` is not tracked
    /// on main (it was renamed away), so the deletion/rename rescue is the only
    /// leg that can corroborate it, and with rename detection on the landing
    /// commit's delta reports `a/new.rs` alone — producing
    /// `[High] P5PhantomDone task=9911: … neither tracked on main nor covered by
    /// a task-referencing commit's own delta` with evidence
    /// `MetadataFiles { entries: ["a/old.rs"] }` and exit 1.
    ///
    /// Two fixture details are load-bearing. The merge subject must reference
    /// the id with non-digit neighbours (`task/9911 ` — a `/` and a space) or
    /// the digit-boundary filter in `task_referencing_commits` drops the hit and
    /// the test would go red for the wrong reason. And `--project-root` must be
    /// the temp repo — a REAL git repo, per `repo_root()`'s doc comment — or
    /// every `git ls-tree` fails, `path_tracked_on` fail-safes to `false` for
    /// `a/new.rs` too, and the scenario stops being a rename scenario.
    #[test]
    fn pre_done_gate_accepts_rename_task_via_landing_commit() {
        let repo_tmp = tempfile::tempdir().expect("create repo tempdir");
        let repo = repo_tmp.path();
        let run = |args: &[&str]| {
            let status = common::git_env::git_cmd(repo)
                .args(args)
                .status()
                .expect("git command failed to spawn");
            assert!(status.success(), "git {:?} exited {:?}", args, status.code());
        };

        run(&["init", "--initial-branch=main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["config", "commit.gpgsign", "false"]);

        std::fs::create_dir_all(repo.join("a")).expect("create a/");
        std::fs::write(repo.join("a/old.rs"), "fn task() {}\n").expect("write a/old.rs");
        run(&["add", "."]);
        run(&["commit", "-m", "base: add a/old.rs"]);

        run(&["checkout", "-b", "feat"]);
        run(&["mv", "a/old.rs", "a/new.rs"]);
        run(&["commit", "-m", "feat: move old.rs to new.rs"]);
        run(&["checkout", "main"]);
        run(&["merge", "--no-ff", "-m", "Merge task/9911 into main", "feat"]);

        // tasks.json and runs.db live OUTSIDE the repo so the fixture repo's
        // tree stays exactly the two commits above.
        let data_tmp = tempfile::tempdir().expect("create data tempdir");
        let dir = data_tmp.path();
        let tasks = vec![task_fixture_with_files(
            "9911",
            "in-progress",
            None,
            None,
            &["a/old.rs", "a/new.rs"],
        )];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task",
                "9911",
                "--pre-done",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                repo.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --task 9911 --pre-done");

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        let p5 = findings
            .iter()
            .find(|f| f["pattern"].as_str() == Some("P5PhantomDone"));
        assert!(
            p5.is_none(),
            "a deliverable renamed away by the task's own landing commit must not be \
             refused — the rename really did touch both paths; got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
        assert_eq!(
            out.status.code(),
            Some(0),
            "the rename flip must exit 0\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            stderr
        );
    }

    /// `--task <id>` (no `--pre-done`) runs all three detectors; P5 finds the
    /// phantom-done; a pending-status task yields zero findings.
    ///
    /// Note: P1 is quiet under `NoopJCodemunchOps` and P2 has no trigger
    /// fixture here — only P5 fires. The test verifies all three detectors
    /// run without error (not that all three produce findings).
    #[test]
    fn task_spot_check_finds_phantom_done_when_running_all_detectors() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks = vec![
            task_fixture("3242", "done", Some("merged"), Some("deadbeef")),
            task_fixture("7777", "pending", None, None),
        ];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");

        // --- Spot-check on done/merged task (expect at least P5 High) ---
        let out = Command::new(bin)
            .args([
                "--task",
                "3242",
                "--no-jcodemunch",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --task 3242");

        let code = out.status.code().unwrap_or(1);
        assert!(code >= 1, "expected non-zero exit for 3242 spot-check");

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);

        let p5_high = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["severity"].as_str() == Some("High")
                && f["task_id"].as_str() == Some("3242")
        });
        assert!(
            p5_high.is_some(),
            "spot-check on 3242 must include P5PhantomDone High; findings:\n{:#}",
            serde_json::Value::Array(findings)
        );

        // --- Spot-check on pending task (expect zero findings) ---
        let out2 = Command::new(bin)
            .args([
                "--task",
                "7777",
                "--no-jcodemunch",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --task 7777");

        assert_eq!(
            out2.status.code(),
            Some(0),
            "pending task 7777 must yield exit 0\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out2.stdout),
            String::from_utf8_lossy(&out2.stderr)
        );

        let stderr2 = String::from_utf8_lossy(&out2.stderr);
        let findings2 = parse_findings_from_stderr(&stderr2);
        assert!(
            findings2.is_empty(),
            "pending task 7777 must yield zero findings; got:\n{:#}",
            serde_json::Value::Array(findings2)
        );
    }

    /// `--since <date> --pattern P5` emits only the phantom-done finding;
    /// a corroborated task produces no P5 finding.
    #[test]
    fn since_window_with_pattern_filter() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        // Task 9999 has files=[] so P5's git-diff check trivially passes
        // (files_missing_from(&[], &[]) is empty). The task_completed event
        // satisfies the runs.db corroboration leg; together these ensure 9999
        // produces no P5 finding even though we don't have a real git repo.
        let task_9999 = serde_json::json!({
            "task_id": "9999",
            "status": "done",
            "files": [],
            "done_provenance": {"kind": "merged", "commit": "cafebabe", "note": null},
            "title": "Task 9999",
            "prd": null,
            "consumer_ref": null,
            "audit_foundation": null,
            "done_at": null
        });
        let tasks = vec![
            task_fixture("3242", "done", Some("merged"), Some("deadbeef")),
            task_9999,
        ];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        // Corroborate 9999: runs.db check passes, git check trivially passes (no files).
        insert_completed_event(&runs_db, "9999");

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--since",
                "2026-05-01",
                "--pattern",
                "P5",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --since --pattern P5");

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);

        // 3242 must appear with P5 High
        let p5_3242 = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["task_id"].as_str() == Some("3242")
        });
        assert!(
            p5_3242.is_some(),
            "expected P5PhantomDone for 3242; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // 9999 must NOT appear
        let p5_9999 = findings
            .iter()
            .find(|f| f["task_id"].as_str() == Some("9999"));
        assert!(
            p5_9999.is_none(),
            "corroborated task 9999 must not appear; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // No P1 or P2 entries (--pattern P5 restricts)
        let non_p5 = findings.iter().find(|f| {
            !matches!(
                f["pattern"].as_str(),
                Some("P5PhantomDone") | Some("P5MetadataFilesGitignored")
            )
        });
        assert!(
            non_p5.is_none(),
            "--pattern P5 must not include P1/P2 findings; got:\n{:#}",
            serde_json::Value::Array(findings)
        );
    }

    /// `--pre-done --pattern P1` must error with exit 125 (infrastructure error),
    /// not silently run P5 or P1.
    #[test]
    fn pre_done_and_pattern_is_an_error() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();
        let tasks = vec![task_fixture("1", "done", Some("merged"), Some("abc"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task", "1",
                "--pre-done",
                "--pattern", "P1",
                "--tasks-file", tasks_file.to_str().unwrap(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pre-done --pattern P1");

        assert_eq!(
            out.status.code(),
            Some(125),
            "--pre-done --pattern must exit 125; got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `--pre-done --since <date>` must error with exit 125.
    #[test]
    fn pre_done_and_since_is_an_error() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();
        let tasks = vec![task_fixture("1", "done", Some("merged"), Some("abc"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task", "1",
                "--pre-done",
                "--since", "2026-05-01",
                "--tasks-file", tasks_file.to_str().unwrap(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pre-done --since");

        assert_eq!(
            out.status.code(),
            Some(125),
            "--pre-done --since must exit 125; got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `--task <id> --pre-done` on a done/merged task whose `files` includes at
    /// least one path, run against a non-git tempdir, must emit a
    /// `"reify-audit: git check-ignore exited"` breadcrumb to stderr.
    ///
    /// When `git check-ignore` is run against a non-git directory it exits 128
    /// ("fatal: not a git repository"). The third arm added to
    /// `RealGitOps::is_gitignored` should emit the breadcrumb for any exit
    /// code other than 0 or 1.  On current code there is no such breadcrumb,
    /// so this test is RED until the impl step lands.
    #[test]
    fn git_check_ignore_non_standard_exit_logs_breadcrumb() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        // task_fixture includes files: ["crates/reify-audit/src/lib.rs"]
        // which is enough to trigger is_gitignored for that path.
        let tasks = vec![task_fixture("4200", "done", Some("merged"), Some("deadbeef"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task",
                "4200",
                "--pre-done",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --task 4200 --pre-done");

        let stderr = String::from_utf8_lossy(&out.stderr);
        // Pin both the format string (locks in the breadcrumb text) and the
        // specific exit code (128 = git's "fatal: not a git repository"), so
        // that a future change accidentally remapping 128 to a recognised arm
        // would still fail this test.
        assert!(
            stderr.contains("reify-audit: git check-ignore exited Some(128)"),
            "stderr must contain 'reify-audit: git check-ignore exited Some(128)' breadcrumb \
             when git exits 128 (non-git dir); full stderr:\n{}",
            stderr
        );
    }

    /// Invoking the binary without `--tasks-file` falls back to the live
    /// fused-memory MCP loader. When the configured endpoint is unreachable,
    /// that fallback must exit 125 so the pre-done hook's refuse-on-non-zero
    /// contract still holds — the binary must never silently no-op when its
    /// task source is missing.
    ///
    /// This is the regression-lock for the original phantom-done bug: the
    /// removed `.taskmaster/tasks/tasks.json` default used to make the binary
    /// silently exit 125 with a confusing "no such file" message; under the
    /// HTTP-loader design the equivalent failure (MCP unreachable) must
    /// surface as a clear connection error and still exit 125.
    #[test]
    fn missing_tasks_file_with_unreachable_mcp_exits_125() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();
        let runs_db = write_empty_runs_db(dir);

        // An endpoint that refuses connections by construction — see
        // `common::net` for why port 0 has no TOCTOU window.
        let unreachable_url = common::net::unreachable_mcp_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task",
                "1",
                "--pre-done",
                "--fused-memory-url",
                &unreachable_url,
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
                // NOTE: intentionally omitting --tasks-file
            ])
            .output()
            .expect("invoke reify-audit without --tasks-file");

        assert_eq!(
            out.status.code(),
            Some(125),
            "missing --tasks-file + unreachable MCP must exit 125; got {:?}\nstdout: {}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("fused-memory"),
            "stderr must mention 'fused-memory' to identify the MCP failure; \
             full stderr:\n{}",
            stderr
        );
    }

    /// Pins the per-instance `AtomicBool` dedup at
    /// `src/lib.rs:275, 375, 387, 396`. With N=3 metadata.files entries and
    /// `git check-ignore` exiting 128 on each call (non-git tempdir), the
    /// pre-dedup code emitted three breadcrumbs; the current short-circuit
    /// emits exactly one.
    ///
    /// Distinct from `git_check_ignore_non_standard_exit_logs_breadcrumb`,
    /// which uses N=1 — the AtomicBool dedup is never exercised there
    /// because is_gitignored is only invoked once.
    ///
    /// N=3 (not N=2) catches three regression modes at once: pre-dedup
    /// (3 breadcrumbs), partial-skip (2), and any future bug that fires
    /// the breadcrumb twice.
    ///
    /// The single-instance contract that makes the per-task budget
    /// meaningful in production is documented on `RealGitOps` in
    /// `src/lib.rs` (Part D of task 3720).
    ///
    /// No `task_completed` event is inserted: `check_one` would emit a
    /// P5 High in its absence, but `check_task` still invokes
    /// `check_gitignored` afterwards (`p5_phantom_done.rs:102-114`), so
    /// the breadcrumb fires regardless.
    #[test]
    fn git_check_ignore_breadcrumb_dedups_across_files() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let mut t = task_fixture("4201", "done", Some("merged"), Some("deadbeef"));
        t["files"] = serde_json::json!([
            "crates/x/a.rs",
            "crates/x/b.rs",
            "crates/x/c.rs",
        ]);
        let tasks = vec![t];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task",
                "4201",
                "--pre-done",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --task 4201 --pre-done");

        let stderr = String::from_utf8_lossy(&out.stderr);
        let breadcrumb_count = stderr
            .matches("reify-audit: git check-ignore exited Some(128)")
            .count();
        assert_eq!(
            breadcrumb_count, 1,
            "with N=3 files in a non-git dir, the AtomicBool dedup must emit \
             exactly 1 breadcrumb (not 3); got {breadcrumb_count}\n\
             full stderr:\n{stderr}"
        );
    }

    /// Duplicate flags follow last-wins semantics.
    ///
    /// The pre-done hook wrapper (`scripts/reify-audit-predone-wrapper.sh`)
    /// passes `--tasks-file <snapshot> --runs-db <db> --project-root <root>`
    /// *before* forwarding `$@`. Callers can override any of those defaults by
    /// appending their own flags. This test locks that the last `--tasks-file`
    /// occurrence wins, so the wrapper's assumption never silently breaks.
    ///
    /// See the `parse_args` doc-comment in `src/bin/reify-audit.rs` for the
    /// authoritative description of the last-wins contract.
    #[test]
    fn duplicate_flags_last_wins() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        // A valid tasks file (the last --tasks-file should point here).
        let task = task_fixture("dup-test-1", "done", None, None);
        let tasks_path = write_tasks_json(dir, &[task]);
        let runs_db = write_empty_runs_db(dir);

        // A non-existent tasks file (the first --tasks-file; should lose).
        let nonexistent = dir.join("does-not-exist.json");

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task",
                "dup-test-1",
                "--pre-done",
                // First --tasks-file (non-existent) — wrapper-supplied position.
                "--tasks-file",
                nonexistent.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
                // Second --tasks-file (valid) — caller-supplied override wins.
                "--tasks-file",
                tasks_path.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit with duplicate --tasks-file");

        // If the first (non-existent) --tasks-file won, the binary would
        // exit 125 ("error reading tasks-file: ..."). Any other exit code
        // (0 or 1-254) means the last flag correctly won.
        assert_ne!(
            out.status.code(),
            Some(125),
            "last --tasks-file must win (exit 125 means the wrong, non-existent \
             file was used); stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `--pattern P1` over the same fixture yields an empty array (Noop
    /// JCodemunchOps means P1 never fires), proving P5 is NOT invoked.
    #[test]
    fn pattern_filter_isolates_each_detector() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks = vec![task_fixture("3242", "done", Some("merged"), Some("deadbeef"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern",
                "P1",
                "--no-jcodemunch",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern P1 --no-jcodemunch");

        assert_eq!(
            out.status.code(),
            Some(0),
            "--pattern P1 --no-jcodemunch must exit 0"
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "--pattern P1 --no-jcodemunch must yield zero findings; got:\n{:#}",
            serde_json::Value::Array(findings)
        );
    }

    /// P1 with an unreachable jcodemunch endpoint fails soft to Noop:
    /// exits 0, produces zero findings, and emits a fallback breadcrumb.
    ///
    /// The old contract (exit 125) is inverted: jcodemunch is an optional
    /// substrate, so an unreachable endpoint degrades P1 to zero findings
    /// while still running P2/P5. Exit 125 is reserved for genuine arg/IO
    /// misconfiguration (e.g. unreadable tasks-file, bad runs-db).
    #[test]
    fn p1_unreachable_jcodemunch_fails_soft_to_noop() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks = vec![task_fixture("1", "pending", None, None)];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let unreachable_url = common::net::unreachable_mcp_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern", "P1",
                "--jcodemunch-url", &unreachable_url,
                "--tasks-file", tasks_file.to_str().unwrap(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern P1 unreachable jcodemunch");

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(0),
            "--pattern P1 with unreachable jcodemunch must fail-soft to exit 0; got {:?}\nstdout: {}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            stderr
        );

        // Zero findings (P1 degrades to Noop).
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "P1 with unreachable jcodemunch must produce zero findings; got:\n{:#}",
            serde_json::Value::Array(findings)
        );

        // Fallback breadcrumb must appear on stderr, pinned to the endpoint
        // under test. Loose substrings will not do: the binary independently
        // emits "tasks.db unreachable at ... advisory lanes degraded", so a
        // bare `contains("unreachable")`/`contains("degrad")` is satisfied
        // whether or not the jcodemunch arm ever ran.
        let breadcrumb = format!("jcodemunch unreachable at '{unreachable_url}'");
        assert!(
            stderr.contains(&breadcrumb),
            "stderr must contain the fail-soft breadcrumb `{breadcrumb}`; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("P1 degraded to zero findings"),
            "stderr breadcrumb must describe the fail-soft degradation; stderr:\n{stderr}"
        );
    }

    /// Default sweep (no --pattern/--task/--since) survives an unreachable
    /// jcodemunch endpoint: P5 still runs and detects phantom-done tasks, exit
    /// code is non-zero (findings found), and the fallback breadcrumb appears.
    #[test]
    fn default_sweep_survives_unreachable_jcodemunch() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        // Phantom-done fixture: done/merged with no runs.db corroboration.
        let tasks = vec![task_fixture("3242", "done", Some("merged"), Some("deadbeef"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let unreachable_url = common::net::unreachable_mcp_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--jcodemunch-url", &unreachable_url,
                "--tasks-file", tasks_file.to_str().unwrap(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit default sweep unreachable jcodemunch");

        let code = out.status.code().unwrap_or(99);
        assert_ne!(
            code, 125,
            "default sweep must NOT exit 125 when jcodemunch is unreachable; got {code}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            code >= 1,
            "default sweep must exit non-zero (P5 finding expected); got {code}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);

        // P5 must have fired and found the phantom-done task.
        let findings = parse_findings_from_stderr(&stderr);
        let p5_high = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["severity"].as_str() == Some("High")
                && f["task_id"].as_str() == Some("3242")
        });
        assert!(
            p5_high.is_some(),
            "default sweep must include P5PhantomDone/High/3242 even when jcodemunch is down; findings:\n{:#}",
            serde_json::Value::Array(findings)
        );

        // Fallback breadcrumb must appear on stderr.
        assert!(
            stderr.contains("jcodemunch"),
            "stderr must contain fallback breadcrumb mentioning 'jcodemunch'; stderr:\n{stderr}"
        );
    }

    /// Regression lock (#5830): the URL minted for "jcodemunch is
    /// unreachable" tests must be unreachable BY CONSTRUCTION, not merely
    /// unowned at the instant it is minted.
    ///
    /// The adversary here is the real one: `spawn_mock_mcp_on` is the same
    /// in-suite responder whose ephemeral-port recycling closed the failure
    /// chain in the observed flake. It answers `initialize` with a
    /// well-formed JSON-RPC result AND an assigned `Mcp-Session-Id`, so
    /// `RealJCodemunchOps::new` returns `Ok`, the fail-soft `Err(e)`
    /// breadcrumb arm never runs, and
    /// `default_sweep_survives_unreachable_jcodemunch`'s breadcrumb
    /// assertion blows up. Same fixture and argv as that test, and the same
    /// assertion set, so this lock is strictly stronger than the test it
    /// shadows; the only difference is the deliberate hijack.
    ///
    /// The session header is load-bearing for THIS lock's adversary, not
    /// incidental: without it the client now rejects the handshake, the
    /// hijacker stops being a convincing jcodemunch, and the lock quietly
    /// degrades into a duplicate of the test it is meant to shadow.
    #[test]
    fn unreachable_jcodemunch_url_cannot_be_hijacked_by_a_racing_mcp_responder() {
        let url = common::net::unreachable_mcp_url();
        let (_addr, hijack) = common::net::try_hijack_url(&url);
        // Stand a REAL MCP responder at the address the URL names, if the
        // hijack landed. `|_args| None` suffices: the breadcrumb hinges on
        // `initialize` succeeding, which the mock always answers happily.
        let mock = hijack.map(|listener| spawn_mock_mcp_on(listener, |_args| None));
        let hijacked = mock.is_some();

        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();
        let tasks = vec![task_fixture("3242", "done", Some("merged"), Some("deadbeef"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--jcodemunch-url", &url,
                "--tasks-file", tasks_file.to_str().unwrap(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit default sweep against a hijacked jcodemunch url");

        // Tear the mock down BEFORE asserting so a failing assertion cannot
        // leak the accept thread into the rest of the run.
        if let Some(mock) = mock {
            mock.stop();
        }

        let code = out.status.code().unwrap_or(99);
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_ne!(
            code, 125,
            "default sweep must NOT exit 125 under a racing MCP responder on \
             {url} (hijack landed: {hijacked}); got {code}\nstderr:\n{stderr}"
        );
        assert!(
            code >= 1,
            "default sweep must exit non-zero (P5 finding expected) under a \
             racing MCP responder on {url} (hijack landed: {hijacked}); got \
             {code}\nstderr:\n{stderr}"
        );

        // P5 must still fire and find the phantom-done task.
        let findings = parse_findings_from_stderr(&stderr);
        let p5_high = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["severity"].as_str() == Some("High")
                && f["task_id"].as_str() == Some("3242")
        });
        assert!(
            p5_high.is_some(),
            "P5PhantomDone/High/3242 must survive a racing MCP responder on \
             {url} (hijack landed: {hijacked}); findings:\n{:#}",
            serde_json::Value::Array(findings)
        );

        // Assert the WHOLE breadcrumb, pinned to the endpoint under test.
        // Two loose substrings would not do: the binary also emits an
        // unrelated "tasks.db unreachable at ..." PTODO diagnostic for this
        // tempdir project root, so a bare `contains("unreachable")` is
        // satisfied whether or not the jcodemunch arm ever ran.
        let breadcrumb = format!("jcodemunch unreachable at '{url}'");
        assert!(
            stderr.contains(&breadcrumb),
            "the fail-soft breadcrumb `{breadcrumb}` must survive a racing MCP \
             responder on {url} (hijack landed: {hijacked}); stderr:\n{stderr}"
        );
    }

    /// Regression lock (#5830), socket layer: a client must still be refused
    /// at the exact address the unreachable-jcodemunch URL names, even while
    /// a racing binder holds that address.
    ///
    /// Phrased against the outcome ("still refused") rather than against
    /// "the bind failed", so it holds for any fix that removes the
    /// time-of-check/time-of-use window rather than over-fitting to one.
    #[test]
    fn unreachable_jcodemunch_url_refuses_connections_even_under_a_racing_binder() {
        let url = common::net::unreachable_mcp_url();
        // `_hijack` is deliberately bound (not `_`) so any listener that DID
        // land stays alive across the connect below — that is the adversary.
        let (addr, _hijack) = common::net::try_hijack_url(&url);
        assert!(
            TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_err(),
            "a client must be refused at {addr} even while a racing binder \
             holds the address named by {url}"
        );
    }

    /// `--pattern P1 --no-jcodemunch` keeps P1 inert (Noop) and exits 0.
    ///
    /// Verifies the offline escape hatch: even after step-6 activates real
    /// jcodemunch, the explicit flag opts back into NoopJCodemunchOps.
    #[test]
    fn no_jcodemunch_flag_keeps_p1_inert() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks = vec![task_fixture("1", "pending", None, None)];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let unreachable_url = common::net::unreachable_mcp_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern", "P1",
                "--no-jcodemunch",
                "--jcodemunch-url", &unreachable_url,
                "--tasks-file", tasks_file.to_str().unwrap(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern P1 --no-jcodemunch");

        assert_eq!(
            out.status.code(),
            Some(0),
            "--pattern P1 --no-jcodemunch must exit 0 (Noop, no connection); got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "--pattern P1 --no-jcodemunch must yield zero findings; got:\n{:#}",
            serde_json::Value::Array(findings)
        );
        // --no-jcodemunch bypasses the jcodemunch seam entirely (Noop), so the
        // fail-soft breadcrumb must NOT appear — the user opted in to silence.
        assert!(
            !stderr.contains("jcodemunch unreachable"),
            "--no-jcodemunch must not emit the fail-soft breadcrumb; stderr:\n{stderr}"
        );
    }

    /// `--task <id> --pre-done` with an unreachable jcodemunch URL must NOT
    /// exit 125 — the pre-done path runs P5 only and never contacts jcodemunch.
    #[test]
    fn pre_done_stays_jcodemunch_free_with_unreachable_jcodemunch() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks = vec![task_fixture("42", "done", Some("merged"), Some("abc"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let unreachable_url = common::net::unreachable_mcp_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task", "42",
                "--pre-done",
                "--jcodemunch-url", &unreachable_url,
                "--tasks-file", tasks_file.to_str().unwrap(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pre-done with closed jcodemunch url");

        assert_ne!(
            out.status.code(),
            Some(125),
            "--pre-done must not contact jcodemunch (unreachable jcodemunch-url must not cause exit 125); \
             got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `--pattern PDEAD --no-jcodemunch` exits 0 with an empty findings array.
    ///
    /// Confirms PDEAD is an accepted pattern (parser does not exit 125) and that
    /// with NoopJCodemunchOps the tool exits cleanly with zero findings.
    ///
    /// Note: this cannot verify that the `if run_pdead { ... }` dispatch arm is
    /// present in main() — NoopJCodemunchOps returns `vec![]` regardless, so a
    /// dropped arm would still pass. Actual wiring is covered by the bin unit
    /// tests (`parse_args_accepts_pdead_pattern`, `needs_jcodemunch_pattern_routing`).
    #[test]
    fn pdead_no_jcodemunch_exits_0_with_empty_findings() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks_file = write_tasks_json(dir, &[]);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern",
                "PDEAD",
                "--no-jcodemunch",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern PDEAD --no-jcodemunch");

        assert_eq!(
            out.status.code(),
            Some(0),
            "--pattern PDEAD --no-jcodemunch must exit 0; got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "--pattern PDEAD --no-jcodemunch must yield zero findings; got:\n{:#}",
            serde_json::Value::Array(findings)
        );
    }

    /// `--pattern PUNTESTED --no-jcodemunch` exits 0 with an empty findings array.
    ///
    /// Confirms PUNTESTED is an accepted pattern (parser does not exit 125) and that
    /// with NoopJCodemunchOps the tool exits cleanly with zero findings.
    #[test]
    fn puntested_no_jcodemunch_exits_0_with_empty_findings() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks_file = write_tasks_json(dir, &[]);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern",
                "PUNTESTED",
                "--no-jcodemunch",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern PUNTESTED --no-jcodemunch");

        assert_eq!(
            out.status.code(),
            Some(0),
            "--pattern PUNTESTED --no-jcodemunch must exit 0; got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "--pattern PUNTESTED --no-jcodemunch must yield zero findings; got:\n{:#}",
            serde_json::Value::Array(findings)
        );
    }

    /// `--pattern PLAYER --no-jcodemunch` exits 0 with an empty findings array.
    ///
    /// Confirms PLAYER is an accepted pattern (parser does not exit 125) and that
    /// with `NoopJCodemunchOps` the tool exits cleanly with zero findings.
    ///
    /// Note: this cannot verify that the `if run_player { ... }` dispatch arm is
    /// present in main() — `NoopJCodemunchOps` returns `vec![]` regardless, so a
    /// dropped arm would still pass. `player_dispatch_forwards_canned_layer_violation`
    /// (S2) covers end-to-end dispatch through the live jcodemunch seam.
    #[test]
    fn player_no_jcodemunch_exits_0_with_empty_findings() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks_file = write_tasks_json(dir, &[]);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern",
                "PLAYER",
                "--no-jcodemunch",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern PLAYER --no-jcodemunch");

        assert_eq!(
            out.status.code(),
            Some(0),
            "--pattern PLAYER --no-jcodemunch must exit 0; got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "--pattern PLAYER --no-jcodemunch must yield zero findings; got:\n{:#}",
            serde_json::Value::Array(findings)
        );
    }

    /// `--pattern PLAYER` with a live mock jcodemunch that returns one layer
    /// violation produces exactly one `PLayerViolation/Low` finding.
    ///
    /// This is the first end-to-end test proving the `if run_player { player::check }`
    /// dispatch arm (the `run_player` predicate in the binary) forwards through the
    /// real jcodemunch seam. The noop smoke test above cannot cover this gap (see
    /// `player_no_jcodemunch_exits_0_with_empty_findings` above): with `--no-jcodemunch`,
    /// a dropped dispatch arm also yields zero findings and exit 0. Here the decisive
    /// assertion is that exactly one `PLayerViolation/Low` finding surfaces with the
    /// from/to files threaded through `player::check`'s summary and evidence.
    ///
    /// Canned violation flow: mock → `RealJCodemunchOps::get_layer_violations` →
    /// `layer_violations_from_wire` → `player::check` → `Finding{pattern:PLayerViolation, ...}`.
    #[test]
    fn player_dispatch_forwards_canned_layer_violation() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        // PLAYER is jcodemunch-backed, so this run now passes through the §4.3
        // freshness gate. Give it a real one-commit repo and a fresh index so
        // the precondition holds and the dispatch path under test is actually
        // reached — a bare non-git tempdir leaves `live_head` unverifiable,
        // which the gate refuses. Making the project root real is a strict
        // improvement to this test rather than an accommodation.
        let live_head = common::index_fixture::init_git_repo_with_one_commit(dir);
        let index_dir = tmp.path().join("code-index");
        common::index_fixture::write_index_db(
            &index_dir,
            &common::index_fixture::expected_repo_id(dir),
            Some(&live_head),
            2,
        );

        let tasks_file = write_tasks_json(dir, &[]);
        let runs_db = write_empty_runs_db(dir);

        let mock = spawn_mock_mcp(|_args| {
            Some(serde_json::json!({
                "violations": [{
                    "from": "crates/reify-cli",
                    "to": "crates/reify-kernel",
                    "from_symbol": "reify_cli::main",
                    "to_symbol": "reify_kernel::solver::Solver::solve",
                    "allowed": false,
                    "rule_index": 0
                }]
            }))
        });

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern",
                "PLAYER",
                "--jcodemunch-url",
                mock.url(),
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
                "--jcodemunch-index-dir",
                index_dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern PLAYER with mock jcodemunch");

        mock.stop();

        // PLayerViolation is Severity::Low → high_severity_exit_code == 0.
        assert_eq!(
            out.status.code(),
            Some(0),
            "--pattern PLAYER with one Low finding must exit 0; got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one PLayerViolation finding; got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
        let f = &findings[0];
        assert_eq!(
            f["pattern"].as_str(),
            Some("PLayerViolation"),
            "finding pattern must be PLayerViolation; got:\n{f:#}"
        );
        assert_eq!(
            f["severity"].as_str(),
            Some("Low"),
            "finding severity must be Low; got:\n{f:#}"
        );
        let summary = f["summary"].as_str().unwrap_or("");
        assert!(
            summary.starts_with("crates/reify-cli imports crates/reify-kernel"),
            "finding summary must begin 'crates/reify-cli imports crates/reify-kernel' \
             (directional from→to); got: {summary:?}"
        );
        assert_eq!(
            f["evidence"][0]["File"]["path"].as_str(),
            Some("crates/reify-cli"),
            "finding evidence[0] must point at from_file; got:\n{:#}",
            f["evidence"]
        );
    }

    /// `--pattern P5` with an unreachable jcodemunch URL must NOT exit 125 —
    /// P5 never contacts jcodemunch.
    #[test]
    fn sweep_pattern_p5_skips_jcodemunch() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks = vec![task_fixture("77", "pending", None, None)];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let unreachable_url = common::net::unreachable_mcp_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern", "P5",
                "--jcodemunch-url", &unreachable_url,
                "--tasks-file", tasks_file.to_str().unwrap(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern P5 with closed jcodemunch url");

        assert_ne!(
            out.status.code(),
            Some(125),
            "--pattern P5 must not contact jcodemunch (exit 125 would mean it did); \
             got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // -------------------------------------------------------------------
    // comma-separated --pattern integration tests (step-1 RED, step-2 GREEN)
    // -------------------------------------------------------------------

    /// `--pattern P1,P2,P5` must be accepted (not exit 125) and must run the
    /// union of P1+P2+P5 detectors. With the phantom-done fixture, P5 fires and
    /// the exit code is non-zero with a P5PhantomDone/High finding for task 3242.
    #[test]
    fn pattern_comma_list_runs_union_of_detectors() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks = vec![task_fixture("3242", "done", Some("merged"), Some("deadbeef"))];
        let tasks_file = write_tasks_json(dir, &tasks);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task",
                "3242",
                "--pattern",
                "P1,P2,P5",
                "--no-jcodemunch",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern P1,P2,P5");

        // First assert: must NOT exit 125 (the bug: current binary exits 125 for comma patterns).
        assert_ne!(
            out.status.code(),
            Some(125),
            "--pattern P1,P2,P5 must not exit 125 (comma list must be accepted); \
             stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        // Second assert: exit code must be >= 1 (at least one High finding).
        let code = out.status.code().unwrap_or(0);
        assert!(
            code >= 1,
            "--pattern P1,P2,P5 with phantom-done fixture must exit non-zero; got {code}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        // Third assert: parse findings and verify P5PhantomDone/High/3242 is present.
        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        let p5_high = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["severity"].as_str() == Some("High")
                && f["task_id"].as_str() == Some("3242")
        });
        assert!(
            p5_high.is_some(),
            "--pattern P1,P2,P5 must dispatch P5 and find P5PhantomDone/High/3242; findings:\n{:#}",
            serde_json::Value::Array(findings)
        );
    }

    /// `--pattern P1,BOGUS` must exit 125 with a clear error naming `BOGUS`
    /// and listing the known tokens.
    #[test]
    fn pattern_comma_list_unknown_token_exits_125() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        let tasks_file = write_tasks_json(dir, &[]);
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern",
                "P1,BOGUS",
                "--no-jcodemunch",
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern P1,BOGUS");

        assert_eq!(
            out.status.code(),
            Some(125),
            "--pattern P1,BOGUS must exit 125 (unknown token); got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("'BOGUS'"),
            "stderr must name the offending token 'BOGUS' (with surrounding quotes); stderr: {stderr}"
        );
        // Per-token containment (not the exact connecting prose) so the test
        // survives future token additions / reordering.
        for tok in ["P1", "P2", "P5", "PDEAD", "PUNTESTED", "PLAYER", "PTODO"] {
            assert!(
                stderr.contains(tok),
                "stderr must list known token {tok}; stderr: {stderr}"
            );
        }
    }

    // -------------------------------------------------------------------
    // PTODO structural-lane end-to-end (step-15 RED / step-16 GREEN)
    // -------------------------------------------------------------------

    /// `--pattern PTODO` over the committed fixture tree (copied into a fresh
    /// git repo, with the fixture root AS the project root) emits exactly the
    /// three structural findings — untracked (scenario01), malformed-cite
    /// (scenario04), phantom-tracking (scenario05) — each `PTodo`/`Medium`
    /// with a §8.3 kind-prefixed summary and a `File` evidence ref at the
    /// offending path. The scenario10 pair (inline `ptodo:allow` escape +
    /// the nested `crates/reify-audit/` allowlisted file) must be suppressed,
    /// and the run exits 0 (all Medium → no High).
    ///
    /// RED until step-16 wires the `if run_ptodo { ptodo::check }` dispatch
    /// arm in `main()` — until then `--pattern PTODO` runs no detector and
    /// yields zero findings.
    #[test]
    fn ptodo_fixture_tree_emits_three_kinds_and_suppresses_allowlist_and_escape() {
        // Repo dir holds ONLY the committed fixtures (so ls-files is exactly
        // the fixture set); the tasks-file/runs-db live in a separate aux dir
        // so they are never tracked and never enumerated by the sweep.
        let repo = tempfile::tempdir().expect("create repo tempdir");
        let aux = tempfile::tempdir().expect("create aux tempdir");

        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ptodo");
        copy_dir_recursive(&fixtures, repo.path());
        git_init_commit_all(repo.path());

        let tasks_file = write_tasks_json(aux.path(), &[]);
        let runs_db = write_empty_runs_db(aux.path());

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern",
                "PTODO",
                "--no-jcodemunch",
                "--project-root",
                repo.path().to_str().unwrap(),
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern PTODO on fixture tree");

        // untracked (scenario01) and blocker-prose untracked (scenario07) are now High
        // (task η, #4559) → exit code = 2 (High-severity count).
        assert_eq!(
            out.status.code(),
            Some(2),
            "PTODO fixture sweep must exit 2 (2 High untracked findings); got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);

        assert_eq!(
            findings.len(),
            4,
            "PTODO fixture sweep must emit exactly 4 findings \
             (untracked/malformed-cite/phantom-tracking + blocker-prose ignore); got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // Every fixture finding is Pattern::PTodo.
        for f in &findings {
            assert_eq!(
                f["pattern"].as_str(),
                Some("PTodo"),
                "every fixture finding must be PTodo; got:\n{f:#}"
            );
        }
        // Per-kind severity (task η, #4559): untracked→High; malformed-cite/phantom-tracking→Medium.
        let severity_of = |path: &str, kind_prefix: &str| -> Option<&str> {
            findings.iter().find_map(|f| {
                if f["task_id"].as_str() == Some(path)
                    && f["summary"].as_str().is_some_and(|s| s.starts_with(kind_prefix))
                {
                    f["severity"].as_str()
                } else {
                    None
                }
            })
        };
        assert_eq!(
            severity_of("scenario01_untracked.rs", "untracked:"),
            Some("High"),
            "untracked must be High"
        );
        assert_eq!(
            severity_of("scenario07_ignore_blocker_prose.rs", "untracked:"),
            Some("High"),
            "blocker-prose untracked must be High"
        );
        assert_eq!(
            severity_of("scenario04_malformed_cite.rs", "malformed-cite:"),
            Some("Medium"),
            "malformed-cite must stay Medium"
        );
        assert_eq!(
            severity_of("scenario05_phantom_tracking.rs", "phantom-tracking:"),
            Some("Medium"),
            "phantom-tracking must stay Medium"
        );

        // Each expected scenario: task_id = root-relative path, summary begins
        // with the §8.3 kind token, evidence[0] is a File ref at the same path.
        let has = |path: &str, kind_prefix: &str| -> bool {
            findings.iter().any(|f| {
                f["task_id"].as_str() == Some(path)
                    && f["summary"].as_str().is_some_and(|s| s.starts_with(kind_prefix))
                    && f["evidence"][0]["File"]["path"].as_str() == Some(path)
            })
        };
        assert!(
            has("scenario01_untracked.rs", "untracked:"),
            "scenario01 must yield an 'untracked' PTodo finding; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
        assert!(
            has("scenario04_malformed_cite.rs", "malformed-cite:"),
            "scenario04 must yield a 'malformed-cite' PTodo finding; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
        assert!(
            has("scenario05_phantom_tracking.rs", "phantom-tracking:"),
            "scenario05 must yield a 'phantom-tracking' PTodo finding; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // Scenario 7: blocker-prose ignore → untracked finding.
        assert!(
            has("scenario07_ignore_blocker_prose.rs", "untracked:"),
            "scenario07 must yield an 'untracked' PTodo finding; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // Scenario 10: neither the inline-escape file nor the allowlisted nested
        // file may surface a finding.  Also used for scenario08 (operational
        // ignore) and scenario07's absence of a scenario08 finding.
        let none_mentions = |needle: &str| -> bool {
            !findings
                .iter()
                .any(|f| f["task_id"].as_str().is_some_and(|t| t.contains(needle)))
        };

        // Scenario 8: operational ignore → no finding.
        assert!(
            none_mentions("scenario08_ignore_operational.rs"),
            "scenario08 (operational reason) must yield no finding; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        assert!(
            none_mentions("scenario10_inline_escape.rs"),
            "inline-escape file (ptodo:allow) must yield no finding; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
        assert!(
            none_mentions("scenario10_allowlisted.rs"),
            "allowlisted nested file (crates/reify-audit/ prefix) must yield no finding; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
    }

    /// The SHIPPED binary must honour its own `--project-root` over an
    /// ambient `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`.
    ///
    /// This is the production half of the hook-environment defect. Git
    /// exports those three vars into a hook's whole process tree, and for
    /// `git commit --only` `GIT_INDEX_FILE` names a *temporary* index. An
    /// unsanitized `git -C <root> ls-files` then reads the PARENT repo's
    /// index instead of the fixture repo's, so the PTODO sweep enumerates a
    /// different file set — silently, with no error. Observed as a divergent
    /// finding set: exit `Some(1)` where `Some(2)` was expected.
    ///
    /// Deliberately a near-clone of
    /// `ptodo_fixture_tree_emits_three_kinds_and_suppresses_allowlist_and_escape`:
    /// same fixture tree, same helpers, same expectations. The ONLY delta is
    /// the three poison vars, which makes "a hook environment changes
    /// nothing" the literal claim under test.
    ///
    /// The poison is applied to the CHILD ONLY via `.env(..)`. This test
    /// never touches its own process environment — `std::env::set_var` is
    /// process-global and would race sibling tests under `cargo test`'s
    /// thread-per-test model.
    #[test]
    fn ptodo_fixture_sweep_survives_ambient_hook_git_env() {
        let repo = tempfile::tempdir().expect("create repo tempdir");
        let aux = tempfile::tempdir().expect("create aux tempdir");

        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ptodo");
        copy_dir_recursive(&fixtures, repo.path());
        git_init_commit_all(repo.path());

        let tasks_file = write_tasks_json(aux.path(), &[]);
        let runs_db = write_empty_runs_db(aux.path());

        // A decoy repo standing in for the parent repo a hook would point at,
        // built by the shared helper so the poisoned variable list has exactly
        // one home on the test side (`common::git_env::hook_git_env`) rather
        // than one copy here and one in the replay harness.
        let decoy = common::git_env::decoy_repo();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let mut cmd = Command::new(bin);
        cmd.args([
            "--pattern",
            "PTODO",
            "--no-jcodemunch",
            "--project-root",
            repo.path().to_str().unwrap(),
            "--tasks-file",
            tasks_file.to_str().unwrap(),
            "--runs-db",
            runs_db.to_str().unwrap(),
        ]);
        common::git_env::poison_with_hook_git_env(&mut cmd, &decoy);

        let out = cmd
            .output()
            .expect("invoke reify-audit --pattern PTODO under poisoned git env");

        let stderr = String::from_utf8_lossy(&out.stderr);

        // Same expectations as the clean-env test — that is the point.
        assert_eq!(
            out.status.code(),
            Some(2),
            "PTODO fixture sweep must exit 2 under an ambient hook git env exactly as it \
             does without one (2 High untracked findings); got {:?}\nstderr: {}",
            out.status.code(),
            stderr
        );

        let findings = parse_findings_from_stderr(&stderr);
        assert_eq!(
            findings.len(),
            4,
            "PTODO fixture sweep must emit exactly 4 findings under an ambient hook git \
             env (an unsanitized ls-files reads the decoy index and enumerates a \
             different file set); got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        let severity_of = |path: &str, kind_prefix: &str| -> Option<&str> {
            findings.iter().find_map(|f| {
                if f["task_id"].as_str() == Some(path)
                    && f["summary"].as_str().is_some_and(|s| s.starts_with(kind_prefix))
                {
                    f["severity"].as_str()
                } else {
                    None
                }
            })
        };
        assert_eq!(
            severity_of("scenario01_untracked.rs", "untracked:"),
            Some("High"),
            "untracked must be High under an ambient hook git env"
        );
        assert_eq!(
            severity_of("scenario07_ignore_blocker_prose.rs", "untracked:"),
            Some("High"),
            "blocker-prose untracked must be High under an ambient hook git env"
        );
        assert_eq!(
            severity_of("scenario04_malformed_cite.rs", "malformed-cite:"),
            Some("Medium"),
            "malformed-cite must stay Medium under an ambient hook git env"
        );
        assert_eq!(
            severity_of("scenario05_phantom_tracking.rs", "phantom-tracking:"),
            Some("Medium"),
            "phantom-tracking must stay Medium under an ambient hook git env"
        );

        // Keep every TempDir guard alive until the assertions are done.
        drop(repo);
        drop(aux);
        drop(decoy);
    }

    /// The fixture-repo helpers must survive a real *ambient* hook git
    /// environment, not just a per-child one.
    ///
    /// `ptodo_fixture_sweep_survives_ambient_hook_git_env` above poisons only
    /// the spawned binary, so it proves the PRODUCTION path. It cannot prove
    /// the helper path: `git_init_commit_all` runs in the *test* process,
    /// whose environment that test deliberately leaves clean. Under a real
    /// hook both are poisoned — which is how `git ["add", "."] exited
    /// Some(128)` was observed, the fixture repo's `add` colliding with the
    /// parent repository's `index.lock`.
    ///
    /// So re-run the `cli::ptodo_*` git-fixture tests in a child process that
    /// has the poison ambient. The name of this test is deliberately outside
    /// the `ptodo_` filter prefix so the replay cannot select itself; the
    /// helper's `REIFY_AUDIT_HOOK_ENV_REPLAY` guard is the second line of
    /// defence.
    ///
    /// The floor of 5 is the selection measured today (`--list` with this
    /// filter names `ptodo_degrades_fail_soft_when_tasks_db_absent`,
    /// `ptodo_env_override_redirects_tasks_db`,
    /// `ptodo_fixture_sweep_survives_ambient_hook_git_env`,
    /// `ptodo_fixture_tree_emits_three_kinds_and_suppresses_allowlist_and_escape`
    /// and `ptodo_orphaned_cite_resolved_against_default_tasks_db`). It exists
    /// because libtest exits 0 on a zero-match filter: without the floor, a
    /// rename here would silently downgrade this harness to a vacuous pass.
    /// Adding a `ptodo_*` test raises the selection freely; losing one fails.
    #[test]
    fn hook_env_replay_of_ptodo_git_fixture_tests() {
        common::git_env::replay_self_under_hook_git_env(&["cli::ptodo_"], 5);
    }

    /// §6.7 PTODO liveness degradation (end-to-end): `--pattern PTODO` over a
    /// repo with a cited marker and an untracked marker but NO
    /// `.taskmaster/tasks/tasks.db` must (1) emit the EXACT §6.7 breadcrumb on
    /// stderr, (2) still surface the untracked structural finding in the JSON,
    /// and (3) exit 0 — never 125 (125 is reserved for arg/IO misconfig, not an
    /// absent optional substrate). `.taskmaster/` is untracked, so this absent-DB
    /// path is the common case during worktree verify.
    ///
    /// RED until step-12 emits the breadcrumb; the structural finding and exit 0
    /// already hold under step-10's silent skip, so the breadcrumb assertions
    /// are the only failing ones.
    #[test]
    fn ptodo_degrades_fail_soft_when_tasks_db_absent() {
        // Repo holds ONLY the two markers (so ls-files is exactly that set);
        // the tasks-file/runs-db live in a separate aux dir, never tracked. No
        // .taskmaster/tasks/tasks.db is created anywhere → the liveness lane
        // degrades fail-soft.
        let repo = tempfile::tempdir().expect("create repo tempdir");
        let aux = tempfile::tempdir().expect("create aux tempdir");

        std::fs::write(repo.path().join("cited.rs"), "// TODO(#4444): orphan-or-not\n")
            .expect("write cited.rs");
        std::fs::write(repo.path().join("untracked.rs"), "// TODO: wire this\n")
            .expect("write untracked.rs");
        git_init_commit_all(repo.path());

        let tasks_file = write_tasks_json(aux.path(), &[]);
        let runs_db = write_empty_runs_db(aux.path());

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern",
                "PTODO",
                "--no-jcodemunch",
                "--project-root",
                repo.path().to_str().unwrap(),
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern PTODO with no tasks.db");

        let stderr = String::from_utf8_lossy(&out.stderr);

        // (3) Fail-soft: untracked finding is High → exit 1 (never 125, which is IO misconfig).
        // Task η (#4559) flipped untracked to High; degradation still exits non-0 for the right
        // reason (High-count exit) not 125 (misconfig sentinel).
        assert_eq!(
            out.status.code(),
            Some(1),
            "untracked is High → exit 1 (not 0, not 125); got {:?}\nstderr:\n{stderr}",
            out.status.code()
        );

        // (1) The EXACT §6.7 breadcrumb. The path between the anchors is the
        // resolved <repo>/.taskmaster/tasks/tasks.db, asserted via its stable
        // tail rather than pinned literally (tempdir prefix varies).
        assert!(
            stderr.contains("reify-audit: tasks.db unreachable at '"),
            "missing breadcrumb prefix; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("' — PTODO liveness (β), inverse (ζ), and G-allow advisory lanes degraded; structural checks still run"),
            "missing breadcrumb suffix; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains(".taskmaster/tasks/tasks.db"),
            "breadcrumb must name the resolved tasks.db path; stderr:\n{stderr}"
        );

        // (2) The structural lane is unaffected: the untracked finding still
        // parses out of the same stderr stream (the breadcrumb is a leading
        // diagnostic line that parse_findings_from_stderr skips).
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.iter().any(|f| {
                f["task_id"].as_str() == Some("untracked.rs")
                    && f["summary"].as_str().is_some_and(|s| s.starts_with("untracked:"))
            }),
            "untracked structural finding must survive degradation; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // The cited marker yields no finding (β skipped, α suppresses cited lines).
        assert!(
            !findings.iter().any(|f| f["task_id"].as_str() == Some("cited.rs")),
            "cited file must yield no finding when the DB is absent; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );
    }

    /// §8.3 orphaned end-to-end: `--pattern PTODO` over a repo with a cited
    /// marker (#4444) and an untracked marker, resolved against an UNTRACKED
    /// `<repo>/.taskmaster/tasks/tasks.db` (seeded AFTER the git commit, so it
    /// mirrors the untracked-in-worktree reality) whose task 4444 = `done`. The
    /// JSON must carry an `orphaned` finding for the cited file (summary names
    /// `#4444` + `done`) alongside the untracked structural finding, exit 0, and
    /// emit no degradation breadcrumb (the DB is present and readable).
    #[test]
    fn ptodo_orphaned_cite_resolved_against_default_tasks_db() {
        let repo = tempfile::tempdir().expect("create repo tempdir");
        let aux = tempfile::tempdir().expect("create aux tempdir");

        std::fs::write(
            repo.path().join("cited.rs"),
            "// TODO(#4444): wire the orphaned-cite path\n",
        )
        .expect("write cited.rs");
        std::fs::write(repo.path().join("untracked.rs"), "// TODO: wire this\n")
            .expect("write untracked.rs");
        git_init_commit_all(repo.path());

        // Seed the DB at the DEFAULT path AFTER the commit → untracked, as in a
        // real worktree. Task 4444 is terminal (done) → the cite is orphaned.
        crate::common::schema::seed_tasks_db_at(
            &repo.path().join(".taskmaster/tasks/tasks.db"),
            &[("master", 4444, "done")],
        );

        let tasks_file = write_tasks_json(aux.path(), &[]);
        let runs_db = write_empty_runs_db(aux.path());

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern",
                "PTODO",
                "--no-jcodemunch",
                "--project-root",
                repo.path().to_str().unwrap(),
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern PTODO with seeded default tasks.db");

        let stderr = String::from_utf8_lossy(&out.stderr);
        // task η (#4559): untracked=High + orphaned=High → 2 High findings → exit 2.
        assert_eq!(
            out.status.code(),
            Some(2),
            "untracked (High) + orphaned (High) → exit 2; got {:?}\nstderr:\n{stderr}",
            out.status.code()
        );

        let findings = parse_findings_from_stderr(&stderr);

        // The orphaned cite: a PTodo/High finding at cited.rs whose summary
        // names the id and the terminal status (task η: orphaned → High).
        let orphaned = findings
            .iter()
            .find(|f| f["task_id"].as_str() == Some("cited.rs"))
            .unwrap_or_else(|| {
                panic!(
                    "expected orphaned finding for cited.rs; findings:\n{:#}",
                    serde_json::Value::Array(findings.clone())
                )
            });
        assert_eq!(orphaned["pattern"].as_str(), Some("PTodo"));
        assert_eq!(orphaned["severity"].as_str(), Some("High")); // task η: orphaned → High
        let summary = orphaned["summary"].as_str().unwrap_or("");
        assert!(summary.starts_with("orphaned:"), "summary: {summary}");
        assert!(summary.contains("#4444"), "summary must name the id: {summary}");
        assert!(summary.contains("done"), "summary must name the status: {summary}");

        // The structural untracked finding coexists.
        assert!(
            findings.iter().any(|f| {
                f["task_id"].as_str() == Some("untracked.rs")
                    && f["summary"].as_str().is_some_and(|s| s.starts_with("untracked:"))
            }),
            "untracked structural finding must coexist; findings:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // DB present and readable → no degradation breadcrumb.
        assert!(
            !stderr.contains("lanes degraded"),
            "no degradation breadcrumb when the DB is present; stderr:\n{stderr}"
        );
    }

    /// §6.7 env override: with NO default-path DB but `REIFY_PTODO_TASKS_DB`
    /// (set via `Command::env` — never in-process `set_var`, which is unsafe
    /// under edition 2024) pointing at an aux-dir DB whose task 4444 = `done`,
    /// the orphaned finding still appears — proving the override is honored over
    /// the (absent) default path, with no degradation breadcrumb.
    #[test]
    fn ptodo_env_override_redirects_tasks_db() {
        let repo = tempfile::tempdir().expect("create repo tempdir");
        let aux = tempfile::tempdir().expect("create aux tempdir");

        std::fs::write(
            repo.path().join("cited.rs"),
            "// TODO(#4444): wire the orphaned-cite path\n",
        )
        .expect("write cited.rs");
        git_init_commit_all(repo.path());

        // The default path <repo>/.taskmaster/tasks/tasks.db is intentionally
        // ABSENT; the override DB lives in the aux dir instead.
        let override_db = aux.path().join("override-tasks.db");
        crate::common::schema::seed_tasks_db_at(&override_db, &[("master", 4444, "done")]);

        let tasks_file = write_tasks_json(aux.path(), &[]);
        let runs_db = write_empty_runs_db(aux.path());

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .env("REIFY_PTODO_TASKS_DB", &override_db)
            .args([
                "--pattern",
                "PTODO",
                "--no-jcodemunch",
                "--project-root",
                repo.path().to_str().unwrap(),
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit with REIFY_PTODO_TASKS_DB override");

        let stderr = String::from_utf8_lossy(&out.stderr);
        // task η (#4559): orphaned → High → 1 High finding → exit 1.
        assert_eq!(
            out.status.code(),
            Some(1),
            "orphaned (High) → exit 1; got {:?}\nstderr:\n{stderr}",
            out.status.code()
        );

        let findings = parse_findings_from_stderr(&stderr);
        let orphaned = findings
            .iter()
            .find(|f| f["task_id"].as_str() == Some("cited.rs"))
            .unwrap_or_else(|| {
                panic!(
                    "env override must resolve the cite → orphaned finding; findings:\n{:#}",
                    serde_json::Value::Array(findings.clone())
                )
            });
        let summary = orphaned["summary"].as_str().unwrap_or("");
        assert!(summary.starts_with("orphaned:"), "summary: {summary}");
        assert!(summary.contains("#4444"), "summary: {summary}");
        assert!(summary.contains("done"), "summary: {summary}");

        // The override DB is present → the default path's absence does NOT
        // degrade the lane.
        assert!(
            !stderr.contains("lanes degraded"),
            "override DB is present → no degradation; stderr:\n{stderr}"
        );
    }
}

// -----------------------------------------------------------------------
// HTTP-loader test harness (--fused-memory-url path)
// -----------------------------------------------------------------------
//
// These tests exercise the production loader path (no --tasks-file). A tiny
// blocking HTTP server stands in for fused-memory; it speaks just enough of
// MCP streamable-HTTP to answer `initialize`, `notifications/initialized`,
// and a single `tools/call get_task` per session.
//
// "Just enough" now includes assigning a session id: the `initialize`
// response carries an `Mcp-Session-Id` header (see [`MOCK_SESSION_ID`]),
// because `JcodemunchClient` treats an initialize response with no
// server-assigned session as a hard `Protocol` failure. The mock answers
// the header but does not police it — see
// [`write_response_with_session`] for why enforcement is off the table.

/// Read a complete HTTP/1.1 request from `stream` and return its body as a
/// JSON Value. Assumes Content-Length is present (which `ureq` always sets
/// for `send_json`). Returns `None` on EOF / parse failure.
fn read_request_body(stream: &mut TcpStream) -> Option<serde_json::Value> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse().ok()?;
        }
    }
    if content_length == 0 {
        return Some(serde_json::Value::Null);
    }
    let mut buf = vec![0u8; content_length];
    reader.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

/// The session id this mock assigns on `initialize`.
///
/// `JcodemunchClient` requires the server to assign one — an `initialize`
/// response with no `Mcp-Session-Id` header is a hard `Protocol` failure —
/// so without this the mock would no longer stand in for a live seam at
/// all. Deliberately not 32 lowercase hex, so it can never be confused
/// with a client-minted id.
const MOCK_SESSION_ID: &str = "mock-mcp-session";

fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    write_response_with_session(stream, status, None, body)
}

/// As [`write_response`], but additionally emits an `Mcp-Session-Id`
/// response header when `session` is `Some`.
///
/// Only the `initialize` arm needs it: assigning the session is the
/// server's job, and every later request carries the id back on the
/// request side. The mock deliberately does NOT *enforce* the contract
/// (no 404 on an inbound id, no 400 on a missing one) — the same responder
/// backs the `--fused-memory-url` loader tests, and `fused_memory_client`
/// still mints its own id and sends it on `initialize`, so enforcement
/// would turn all of those red. The jcodemunch-side contract is locked
/// gate-resident by the hermetic unit tests in `jcodemunch_client.rs`,
/// which assert the request headers directly.
fn write_response_with_session(
    stream: &mut TcpStream,
    status: u16,
    session: Option<&str>,
    body: &[u8],
) {
    let status_text = match status {
        200 => "OK",
        202 => "Accepted",
        _ => "OK",
    };
    let mut header = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(session) = session {
        header.push_str(&format!("Mcp-Session-Id: {session}\r\n"));
    }
    header.push_str("\r\n");
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

/// Handle returned by [`spawn_mock_mcp`]. Carries the bound `SocketAddr`
/// directly so the stop helper doesn't need to re-parse the URL (a brittle
/// approach that hangs the test runner forever if the URL shape ever
/// changes).
struct MockServer {
    url: String,
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

/// Spawn a one-shot mock MCP server on an OS-assigned ephemeral port.
/// `task_responder` is given the `tools/call` arguments and returns the
/// JSON-RPC `result` value to send back (or `None` to return an error
/// envelope). Returns a [`MockServer`] handle; the caller calls
/// [`MockServer::stop`] (or lets it drop) to tear down.
///
/// Thin wrapper over [`spawn_mock_mcp_on`], which takes an already-bound
/// listener so a caller can place the responder at a *chosen* address.
fn spawn_mock_mcp<F>(task_responder: F) -> MockServer
where
    F: Fn(&serde_json::Value) -> Option<serde_json::Value> + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    spawn_mock_mcp_on(listener, task_responder)
}

/// Spawn a one-shot mock MCP server on an ALREADY-BOUND `listener`, deriving
/// the advertised `addr`/`url` from `listener.local_addr()`. Lets a caller
/// stand a real MCP responder at a specific address (e.g. to play the
/// adversary in a port-recycling regression lock) rather than at whatever
/// ephemeral port the OS hands out.
///
/// The accept loop uses a short `set_nonblocking` poll so it wakes
/// periodically to check the stop flag even without a wakeup connection —
/// that way a stop request can't hang the test runner.
fn spawn_mock_mcp_on<F>(listener: TcpListener, task_responder: F) -> MockServer
where
    F: Fn(&serde_json::Value) -> Option<serde_json::Value> + Send + Sync + 'static,
{
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("http://127.0.0.1:{}/mcp/", addr.port());
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);
    let responder = Arc::new(task_responder);

    // Non-blocking accept with a short poll so the accept loop wakes
    // regularly enough to notice the stop flag even if the wakeup
    // connection in `stop()` never lands.
    listener
        .set_nonblocking(true)
        .expect("set_nonblocking on mock listener");

    let handle = thread::spawn(move || {
        loop {
            if stop_clone.load(Ordering::Relaxed) {
                return;
            }
            let mut stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => {
                    // Backoff for non-WouldBlock errors (e.g. EMFILE on
                    // a constrained CI box) so the accept loop doesn't
                    // peg a CPU until the test's overall timeout.
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            // Restore blocking semantics on the accepted stream so the
            // BufReader inside read_request_body() doesn't busy-loop.
            let _ = stream.set_nonblocking(false);
            let body = match read_request_body(&mut stream) {
                Some(b) => b,
                None => continue,
            };
            let method = body
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let req_id = body.get("id").cloned();

            match method.as_str() {
                "initialize" => {
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {
                            "protocolVersion": "2024-11-05",
                            "capabilities": {},
                            "serverInfo": {"name": "mock-mcp", "version": "0.1"}
                        }
                    });
                    write_response_with_session(
                        &mut stream,
                        200,
                        Some(MOCK_SESSION_ID),
                        resp.to_string().as_bytes(),
                    );
                }
                "notifications/initialized" => {
                    write_response(&mut stream, 202, b"");
                }
                "tools/call" => {
                    let args = body
                        .get("params")
                        .and_then(|p| p.get("arguments"))
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let resp_value = match responder(&args) {
                        Some(structured) => serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": {"structuredContent": structured, "content": []}
                        }),
                        None => serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "error": {"code": -32000, "message": "task not found"}
                        }),
                    };
                    write_response(&mut stream, 200, resp_value.to_string().as_bytes());
                }
                _ => {
                    write_response(&mut stream, 200, b"{}");
                }
            }
        }
    });

    MockServer {
        url,
        addr,
        stop,
        handle: Some(handle),
    }
}

impl MockServer {
    fn url(&self) -> &str {
        &self.url
    }

    /// Signal the accept loop to exit and join the thread. Uses the bound
    /// `SocketAddr` directly (no URL parsing) — a stop request will always
    /// reach the loop, plus the loop's own non-blocking poll guarantees it
    /// wakes even if the wakeup connection is dropped.
    fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Best-effort wakeup; the non-blocking accept poll is the safety
        // net so this can fail without hanging the test.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        // Idempotent shutdown if the test never called `.stop()` (e.g. on
        // panic). Mirrors `stop()` minus the join — we let the thread
        // tear down on its own to avoid blocking the drop path.
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(50));
    }
}

mod http_loader {
    use super::*;

    /// Pre-done via HTTP loader: a corroborated done/merged task with no
    /// files should yield zero findings and exit 0. Proves the binary
    /// successfully (a) connects to MCP, (b) calls get_task, (c) decodes
    /// the wire shape, (d) runs the P5 check against the decoded metadata.
    #[test]
    fn pre_done_via_http_loader_corroborated_exits_zero() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let runs_db = write_empty_runs_db(dir);
        // Seed the runs.db corroboration leg.
        insert_completed_event(&runs_db, "9999");

        let mock = spawn_mock_mcp(|args| {
            assert_eq!(args.get("id").and_then(|v| v.as_str()), Some("9999"));
            // Files=[] → P5's git-diff check trivially passes; the runs.db
            // task_completed event corroborates the done-flip.
            Some(serde_json::json!({
                "id": "9999",
                "title": "Mock task 9999",
                "status": "done",
                "updatedAt": "2026-05-16T07:39:04Z",
                "metadata": {
                    "files": [],
                    "done_provenance": {"kind": "merged", "commit": "cafebabe", "note": null}
                }
            }))
        });

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task", "9999",
                "--pre-done",
                "--fused-memory-url", mock.url(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit");

        mock.stop();

        assert_eq!(
            out.status.code(),
            Some(0),
            "corroborated task must exit 0; stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(findings.is_empty(), "expected zero findings; got {:#}", serde_json::Value::Array(findings));
    }

    /// Pre-done via HTTP loader: a done/merged task with files but no
    /// runs.db corroboration event should emit a P5PhantomDone High finding.
    /// Proves the loader populates `files`/`done_provenance` correctly.
    #[test]
    fn pre_done_via_http_loader_phantom_finding() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let runs_db = write_empty_runs_db(dir);

        let mock = spawn_mock_mcp(|_args| {
            Some(serde_json::json!({
                "id": 4242,
                "title": "Mock task 4242",
                "status": "done",
                "updatedAt": "2026-05-16T07:39:04Z",
                "metadata": {
                    "files": ["crates/reify-audit/src/lib.rs"],
                    "done_provenance": {"kind": "merged", "commit": "deadbeef", "note": null}
                }
            }))
        });

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task", "4242",
                "--pre-done",
                "--fused-memory-url", mock.url(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit");

        mock.stop();

        let code = out.status.code().unwrap_or(-1);
        assert!(code >= 1, "expected non-zero exit for phantom-done; got {code}");

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        let p5_high = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["severity"].as_str() == Some("High")
                && f["task_id"].as_str() == Some("4242")
        });
        assert!(
            p5_high.is_some(),
            "expected P5PhantomDone/High/4242 in findings; got {:#}",
            serde_json::Value::Array(findings)
        );
    }

    /// Pre-done via HTTP loader: server returns a JSON-RPC error envelope
    /// → binary must exit 125 (ERROR_EXIT), preserving the
    /// refuse-on-non-zero contract of the pre-done hook.
    #[test]
    fn pre_done_via_http_loader_missing_task_exits_125() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let runs_db = write_empty_runs_db(dir);

        let mock = spawn_mock_mcp(|_args| None);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task", "0000",
                "--pre-done",
                "--fused-memory-url", mock.url(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit");

        mock.stop();

        assert_eq!(
            out.status.code(),
            Some(125),
            "missing task must exit 125; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Sweep path via HTTP loader: `--since` (no `--pre-done`) routes
    /// through `get_tasks` + `collect_tasks_recursive` and must flatten
    /// subtasks before handing them to the detectors. This guards the
    /// parent/subtask plumbing that the pre-done tests don't exercise
    /// (they only call `get_task` for a single id). A regression in
    /// subtask flattening — e.g. wrong wrap key or missed recursion —
    /// would let phantom-done subtasks slip through the sweep silently.
    #[test]
    fn sweep_via_http_loader_flattens_subtasks_and_detects_phantom() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let runs_db = write_empty_runs_db(dir);
        // Corroborate the parent so only the subtask should fire P5.
        insert_completed_event(&runs_db, "5000");

        let mock = spawn_mock_mcp(|args| {
            // Sweep path sends `get_tasks` with `with_subtasks: true`. The
            // pre-done hot path sends `get_task` with `id`. Distinguish
            // here so the mock can return the expected shape.
            if args.get("id").is_some() {
                return Some(serde_json::Value::Null);
            }
            Some(serde_json::json!({
                "tasks": [
                    {
                        "id": 5000,
                        "title": "Parent task",
                        "status": "done",
                        "updatedAt": "2026-05-16T07:39:04Z",
                        "metadata": {
                            "files": [],
                            "done_provenance": {"kind": "merged", "commit": "cafe", "note": null}
                        },
                        "subtasks": [
                            {
                                "id": "5000.1",
                                "title": "Phantom subtask",
                                "status": "done",
                                "updatedAt": "2026-05-16T07:39:04Z",
                                "metadata": {
                                    "files": ["crates/reify-audit/src/lib.rs"],
                                    "done_provenance": {
                                        "kind": "merged",
                                        "commit": "deadbeef",
                                        "note": null
                                    }
                                }
                            }
                        ]
                    }
                ]
            }))
        });

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--since", "2026-05-01",
                "--pattern", "P5",
                "--fused-memory-url", mock.url(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --since (sweep)");

        mock.stop();

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);

        // The subtask must surface — proves the flattener walks `subtasks[]`.
        let subtask_finding = findings.iter().find(|f| {
            f["pattern"].as_str() == Some("P5PhantomDone")
                && f["task_id"].as_str() == Some("5000.1")
        });
        assert!(
            subtask_finding.is_some(),
            "expected P5PhantomDone for subtask 5000.1 (sweep+flatten); got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // The corroborated parent must NOT surface — proves the parent
        // also flowed through the detector (a flatten that dropped the
        // parent would still pass this test, but flatten dropping the
        // subtask would not — that's the actual regression risk).
        let parent_finding = findings
            .iter()
            .find(|f| f["task_id"].as_str() == Some("5000"));
        assert!(
            parent_finding.is_none(),
            "corroborated parent 5000 must not appear; got:\n{:#}",
            serde_json::Value::Array(findings)
        );
    }

    /// Pre-done via HTTP loader: MCP endpoint refuses the connection → exit
    /// 125. Proves the loader's connection-failure path is wired to
    /// ERROR_EXIT rather than silently exiting 0.
    #[test]
    fn pre_done_via_http_loader_connection_refused_exits_125() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let runs_db = write_empty_runs_db(dir);

        // An endpoint that refuses connections by construction — see
        // `common::net` for why port 0 has no TOCTOU window.
        let url = common::net::unreachable_mcp_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task", "1234",
                "--pre-done",
                "--fused-memory-url", &url,
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit");

        assert_eq!(
            out.status.code(),
            Some(125),
            "connection refused must exit 125; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Regression: sweep path via HTTP loader must not overflow ureq's
    /// 10 MiB `into_string` cap when the task corpus exceeds 10 MiB.
    ///
    /// The mock returns a `get_tasks` payload whose serialized body is
    /// ~11 MiB (one `pending` task with an oversized `title`). On the
    /// unfixed code the binary exits 125 with "MCP HTTP error: read body:
    /// response too big for into_string". After the fix it loads the
    /// corpus and exits 0 (`--pattern P1` under NoopJCodemunchOps yields
    /// zero findings since there are no `done` tasks).
    #[test]
    fn sweep_via_http_loader_oversized_corpus_does_not_overflow() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let runs_db = write_empty_runs_db(dir);

        // Build an ~11 MiB title: ureq's cap is exactly 10 * 1024 * 1024
        // bytes; 11 MiB clears it with margin once the JSON envelope is
        // serialized.
        let oversized_title = "x".repeat(11 * 1024 * 1024);

        let mock = spawn_mock_mcp(move |args| {
            // Sweep path sends `get_tasks` (no `id`). Pre-done path sends
            // `get_task` with `id`. Return Null for any `get_task` calls
            // (there should be none in a sweep, but guard anyway).
            if args.get("id").is_some() {
                return Some(serde_json::Value::Null);
            }
            Some(serde_json::json!({
                "tasks": [
                    {
                        "id": 1,
                        "status": "pending",
                        "title": oversized_title,
                        "metadata": {}
                    }
                ]
            }))
        });

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--since", "2026-01-01",
                "--pattern", "P1",
                "--no-jcodemunch",
                "--fused-memory-url", mock.url(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit (oversized corpus sweep)");

        mock.stop();

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_ne!(
            out.status.code(),
            Some(125),
            "oversized corpus must NOT overflow (exit 125 = ureq cap hit); stderr:\n{stderr}"
        );
        assert_eq!(
            out.status.code(),
            Some(0),
            "no done tasks → P1 should yield zero findings (exit 0); stderr:\n{stderr}"
        );
    }

    /// Sweep path via HTTP loader: server returns a well-formed JSON-RPC
    /// envelope but the `tools/call get_tasks` result lacks the `tasks`
    /// array. The loader must refuse this — otherwise the sweep would
    /// silently return zero tasks and exit 0, looking healthy while
    /// actually masking a server-side bug. Guards the `missing or
    /// non-array \`tasks\` field` branch in `FusedMemoryClient::get_tasks`.
    #[test]
    fn sweep_via_http_loader_malformed_tasks_payload_exits_125() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let runs_db = write_empty_runs_db(dir);

        // Responder returns an empty object — well-formed envelope,
        // missing `tasks` field. The mock wraps this in
        // `result.structuredContent`, so the wire shape after the MCP
        // adapter is `{}` (no `tasks` array).
        let mock = spawn_mock_mcp(|_args| Some(serde_json::json!({})));

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--since", "1970-01-01",
                "--fused-memory-url", mock.url(),
                "--runs-db", runs_db.to_str().unwrap(),
                "--project-root", dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit");

        mock.stop();

        assert_eq!(
            out.status.code(),
            Some(125),
            "malformed get_tasks payload must exit 125; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("get_tasks") && stderr.contains("tasks"),
            "stderr should breadcrumb the malformed-tasks reason; got: {stderr}"
        );
    }
}

// -----------------------------------------------------------------------
// §4.3 freshness gate — end-to-end boundary scenarios
// -----------------------------------------------------------------------

/// The freshness gate refuses to query a stale or empty jcodemunch corpus.
///
/// Every test here is HERMETIC and gate-resident: no serve, no network, no
/// `uvx`, no `#[ignore]`. The degenerate corpus shapes are manufactured on
/// disk by `common::index_fixture`, and `spawn_mock_mcp` stands in for the
/// serve so `RealJCodemunchOps::new` succeeds — the precondition for the gate
/// firing at all. That matters beyond convenience: a live-only test for this
/// gate would be a PASS-shaped skip on every machine without a serve, which is
/// exactly the "looks green, proves nothing" failure the gate exists to
/// eliminate. Making this task's own evidence vacuous would be self-defeating.
mod freshness_gate {
    use super::*;
    use crate::common::index_fixture::{
        expected_repo_id, index_db_path, init_git_repo_with_one_commit, write_index_db,
    };

    const BOGUS_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    /// A jcodemunch-shaped mock that answers the handshake and returns an
    /// empty result for any `tools/call`.
    ///
    /// The gate must fire BEFORE any detector query, so on the refusal paths
    /// this responder should never be consulted for anything but `initialize`.
    fn spawn_jcodemunch_mock() -> MockServer {
        spawn_mock_mcp(|_args| Some(serde_json::json!({})))
    }

    /// The common scaffolding: a real one-commit git repo (so `live_head` is a
    /// genuine sha), an empty tasks fixture, an empty runs.db, and a separate
    /// tempdir standing in for `~/.code-index`.
    struct Scenario {
        _tmp: tempfile::TempDir,
        repo: std::path::PathBuf,
        index_dir: std::path::PathBuf,
        tasks_file: std::path::PathBuf,
        runs_db: std::path::PathBuf,
        live_head: String,
        repo_id: String,
    }

    fn scenario() -> Scenario {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo dir");
        let index_dir = tmp.path().join("code-index");
        std::fs::create_dir_all(&index_dir).expect("create index dir");

        let live_head = init_git_repo_with_one_commit(&repo);
        let repo_id = expected_repo_id(&repo);
        let tasks_file = write_tasks_json(tmp.path(), &[]);
        let runs_db = write_empty_runs_db(tmp.path());

        Scenario { _tmp: tmp, repo, index_dir, tasks_file, runs_db, live_head, repo_id }
    }

    impl Scenario {
        /// Invoke the binary with the given pattern and extra flags.
        fn run(&self, pattern: &str, extra: &[&str]) -> std::process::Output {
            self.run_raw(&["--pattern", pattern], extra)
        }

        /// Invoke the binary with NO `--pattern` — the default all-detector
        /// sweep, whose run set is mixed (P1 alongside P2/P5/PTODO/…).
        fn run_default_sweep(&self, extra: &[&str]) -> std::process::Output {
            self.run_raw(&[], extra)
        }

        fn run_raw(&self, pattern: &[&str], extra: &[&str]) -> std::process::Output {
            let bin = env!("CARGO_BIN_EXE_reify-audit");
            let mut cmd = Command::new(bin);
            cmd.args(pattern);
            cmd.args([
                "--tasks-file", self.tasks_file.to_str().unwrap(),
                "--runs-db", self.runs_db.to_str().unwrap(),
                "--project-root", self.repo.to_str().unwrap(),
                "--jcodemunch-index-dir", self.index_dir.to_str().unwrap(),
            ]);
            cmd.args(extra);
            cmd.output().expect("invoke reify-audit")
        }

        /// Invoke WITHOUT `--jcodemunch-index-dir`, so the binary must resolve
        /// the index directory from the environment. `env` entries are applied
        /// verbatim; `None` removes the variable.
        ///
        /// Every env-precedence assertion must go through this helper rather
        /// than `run_raw`: with the flag present, the production default is
        /// never exercised at all, so a wrong default stays green. That gap is
        /// exactly what let the `CODE_INDEX_PATH` divergence through review.
        fn run_env(
            &self,
            pattern: &str,
            env: &[(&str, Option<&str>)],
            extra: &[&str],
        ) -> std::process::Output {
            let bin = env!("CARGO_BIN_EXE_reify-audit");
            let mut cmd = Command::new(bin);
            cmd.args(["--pattern", pattern]);
            cmd.args([
                "--tasks-file", self.tasks_file.to_str().unwrap(),
                "--runs-db", self.runs_db.to_str().unwrap(),
                "--project-root", self.repo.to_str().unwrap(),
            ]);
            // Clear both index-dir variables first so an inherited value from
            // the developer's shell cannot decide the outcome.
            cmd.env_remove("JCODEMUNCH_INDEX_DIR");
            cmd.env_remove("CODE_INDEX_PATH");
            for (k, v) in env {
                match v {
                    Some(val) => cmd.env(k, val),
                    None => cmd.env_remove(k),
                };
            }
            cmd.args(extra);
            cmd.output().expect("invoke reify-audit")
        }
    }

    /// With no `--jcodemunch-index-dir`, the gate must probe `CODE_INDEX_PATH`.
    ///
    /// `CODE_INDEX_PATH` is jcodemunch's own index-directory variable and the
    /// supported redirection across this substrate:
    /// `scripts/jcodemunch-index-reify.sh` resolves the DB as
    /// `${CODE_INDEX_PATH:-$HOME/.code-index}/local-<name>.db`, and
    /// `tests/infra/test_jcodemunch_index_reify.sh` drives its whole suite
    /// through a temp one. If the gate ignored it, then on any host or CI job
    /// that sets it the indexer would write a healthy corpus to
    /// `$CODE_INDEX_PATH` while the gate probed `$HOME/.code-index`, found
    /// nothing, and hard-refused `E_JC_INDEX_EMPTY` against a fully-indexed
    /// tree — the phantom-reindex failure the identity half already forbids.
    ///
    /// The assertion is deliberately `E_JC_INDEX_STALE`, not merely "non-zero":
    /// STALE is reachable ONLY by opening the DB written at `index_dir` and
    /// reading its `git_head`. `E_JC_INDEX_EMPTY` is what a wrong directory
    /// produces, so the two markers cleanly separate "probed the right place"
    /// from "probed nothing".
    #[test]
    fn index_dir_defaults_to_code_index_path_env() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(BOGUS_SHA), 12);
        let mock = spawn_jcodemunch_mock();

        let out = s.run_env(
            "P1",
            &[("CODE_INDEX_PATH", Some(s.index_dir.to_str().unwrap()))],
            &["--jcodemunch-url", mock.url()],
        );
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(125),
            "the gate must refuse using the CODE_INDEX_PATH store; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("E_JC_INDEX_STALE"),
            "STALE proves the DB under CODE_INDEX_PATH was actually opened and \
             its git_head read; EMPTY would mean the gate probed elsewhere. \
             stderr:\n{stderr}"
        );
        assert!(
            stderr.contains(BOGUS_SHA),
            "refusal must name the index head read from CODE_INDEX_PATH; \
             stderr:\n{stderr}"
        );
    }

    /// `JCODEMUNCH_INDEX_DIR` is the audit-local override and must outrank
    /// `CODE_INDEX_PATH`. Both are set to REAL but DIFFERENT stores: the
    /// override holds a stale DB, `CODE_INDEX_PATH` holds a fresh one. Only a
    /// gate that honours the documented precedence refuses; one that silently
    /// preferred `CODE_INDEX_PATH` would proceed to the detector instead.
    #[test]
    fn jcodemunch_index_dir_env_outranks_code_index_path() {
        let s = scenario();
        let other = s._tmp.path().join("other-index");
        std::fs::create_dir_all(&other).expect("create second index dir");
        // Override store: stale. CODE_INDEX_PATH store: fresh and populated.
        write_index_db(&other, &s.repo_id, Some(BOGUS_SHA), 12);
        write_index_db(&s.index_dir, &s.repo_id, Some(&s.live_head), 7);
        let mock = spawn_jcodemunch_mock();

        let out = s.run_env(
            "P1",
            &[
                ("JCODEMUNCH_INDEX_DIR", Some(other.to_str().unwrap())),
                ("CODE_INDEX_PATH", Some(s.index_dir.to_str().unwrap())),
            ],
            &["--jcodemunch-url", mock.url()],
        );
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert!(
            stderr.contains("E_JC_INDEX_STALE") && stderr.contains(BOGUS_SHA),
            "JCODEMUNCH_INDEX_DIR must win: the refusal has to name the stale \
             head from the override store, not the fresh CODE_INDEX_PATH one. \
             stderr:\n{stderr}"
        );
    }

    /// The `--jcodemunch-index-dir` flag outranks both variables. Guards the
    /// top of the documented precedence chain, so a future refactor cannot
    /// quietly let an inherited `CODE_INDEX_PATH` capture an explicit flag.
    #[test]
    fn jcodemunch_index_dir_flag_outranks_both_env_vars() {
        let s = scenario();
        let decoy = s._tmp.path().join("decoy-index");
        std::fs::create_dir_all(&decoy).expect("create decoy index dir");
        // Both env stores are fresh; only the flagged store is stale.
        write_index_db(&decoy, &s.repo_id, Some(&s.live_head), 7);
        write_index_db(&s.index_dir, &s.repo_id, Some(BOGUS_SHA), 12);
        let mock = spawn_jcodemunch_mock();

        let out = s.run_env(
            "P1",
            &[
                ("JCODEMUNCH_INDEX_DIR", Some(decoy.to_str().unwrap())),
                ("CODE_INDEX_PATH", Some(decoy.to_str().unwrap())),
            ],
            &[
                "--jcodemunch-url", mock.url(),
                "--jcodemunch-index-dir", s.index_dir.to_str().unwrap(),
            ],
        );
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert!(
            stderr.contains("E_JC_INDEX_STALE") && stderr.contains(BOGUS_SHA),
            "the explicit flag must win over both env vars; stderr:\n{stderr}"
        );
    }

    /// B4 — a corpus indexed at a DIFFERENT commit must be refused.
    ///
    /// This is the harm §4.3 exists to prevent: queries answer about another
    /// commit's code, so P1 reports symbols as orphaned that the current tree
    /// references — a fabricated High-severity finding, indistinguishable at
    /// the CLI surface from a real one.
    #[test]
    fn b4_stale_index_refuses_with_e_jc_index_stale() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(BOGUS_SHA), 12);
        let mock = spawn_jcodemunch_mock();

        let out = s.run("P1", &["--jcodemunch-url", mock.url()]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(125),
            "a stale index must refuse the run; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("E_JC_INDEX_STALE"),
            "refusal must carry the machine-readable marker; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains(BOGUS_SHA),
            "refusal must name the index head; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains(&s.live_head),
            "refusal must name the live head; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("symbol_count=12"),
            "refusal must name the symbol count; stderr:\n{stderr}"
        );
        // The one field that says WHICH index to rebuild. Without this
        // assertion, dropping `with_repo_id` leaves the whole suite green
        // while the operator loses the only actionable identifier — the
        // §4.2-derived id is not something they can reconstruct by hand.
        assert!(
            stderr.contains(&s.repo_id),
            "refusal must name the probed repo id {}; stderr:\n{stderr}",
            s.repo_id
        );

        // Load-bearing, not cosmetic. The `/audit` skill disambiguates
        // exit-125-as-infra-error from exit-125-as-125-High-findings by
        // parsing stderr for a JSON array. Because the refusal returns before
        // any findings are serialized, it emits no array and the EXISTING
        // disambiguator classifies it correctly with no skill-side change.
        assert!(
            !stderr_has_parseable_findings_array(&stderr),
            "a refusal must emit NO findings array; stderr:\n{stderr}"
        );
    }

    /// B5 — an index at the right commit but carrying zero symbols must be
    /// refused. The mirror-image failure: a head comparison alone says "all
    /// good" while every query returns nothing, so every producer looks
    /// orphaned. This is the shape `delete-index` leaves behind.
    #[test]
    fn b5_empty_husk_index_refuses_with_e_jc_index_empty() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(&s.live_head), 0);
        let mock = spawn_jcodemunch_mock();

        let out = s.run("P1", &["--jcodemunch-url", mock.url()]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(125),
            "an empty-husk index must refuse the run; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("E_JC_INDEX_EMPTY"),
            "refusal must carry the machine-readable marker; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("symbol_count=0"),
            "refusal must name the symbol count; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains(&s.repo_id),
            "refusal must name the probed repo id {}; stderr:\n{stderr}",
            s.repo_id
        );
        assert!(
            !stderr_has_parseable_findings_array(&stderr),
            "a refusal must emit NO findings array; stderr:\n{stderr}"
        );
    }

    /// B5b — no index at all is the limiting case of an empty one, and gets
    /// the same refusal. Also the end-to-end proof of the read-only open:
    /// `Connection::open` CREATES a missing file, so a run against an absent
    /// index must not litter a phantom zero-symbol DB that jcodemunch would
    /// then register as an empty repo — the gate must not manufacture the very
    /// condition it detects.
    #[test]
    fn b5b_absent_index_refuses_without_creating_the_db() {
        let s = scenario();
        let expected_db = index_db_path(&s.index_dir, &s.repo_id);
        assert!(!expected_db.exists(), "precondition: no index db");
        let mock = spawn_jcodemunch_mock();

        let out = s.run("P1", &["--jcodemunch-url", mock.url()]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(125),
            "an absent index must refuse the run; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("E_JC_INDEX_EMPTY"),
            "refusal must carry the machine-readable marker; stderr:\n{stderr}"
        );
        assert!(
            !stderr_has_parseable_findings_array(&stderr),
            "a refusal must emit NO findings array; stderr:\n{stderr}"
        );
        assert!(
            !expected_db.exists(),
            "the gate must NOT create the index db it fails to find, at {}",
            expected_db.display()
        );
    }

    /// B6 — a fresh, populated index lets the run PROCEED.
    ///
    /// Per the manifest's `no-finding-count-assertion` guardrail this asserts
    /// only that a well-formed findings array was emitted, never that it is
    /// non-empty: the corpus here is a synthetic two-symbol index, so a
    /// specific finding count would pin an artifact of the fixture rather than
    /// any behaviour of the gate.
    #[test]
    fn b6_fresh_index_admits_the_run() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(&s.live_head), 2);
        let mock = spawn_jcodemunch_mock();

        let out = s.run("P1", &["--jcodemunch-url", mock.url()]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(0),
            "a fresh populated index must admit the run; stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("E_JC_INDEX_STALE") && !stderr.contains("E_JC_INDEX_EMPTY"),
            "an admitted run must emit NEITHER marker; stderr:\n{stderr}"
        );
        // The same predicate the /audit skill's exit-125 disambiguator
        // applies, and the exact inverse of what B4/B5/B5b assert. A bare
        // `contains('[')` would be satisfied by any incidental diagnostic
        // line — `git check-ignore exited Some(..)`, a PTODO
        // `tasks.db unreachable at ...` breadcrumb — so it would still pass
        // if the findings array were never serialized at all.
        assert!(
            stderr_has_parseable_findings_array(&stderr),
            "an admitted run must emit a well-formed findings array; stderr:\n{stderr}"
        );
    }

    /// B7 — detectors that never touch jcodemunch must be unaffected by a
    /// stale index. `needs_jcodemunch` is false for a P2/P5-only run, so no
    /// client is constructed and the gate is unreachable. A gate that fired
    /// here would turn every P2/P5 sweep red on any machine with a stale
    /// corpus it never consults.
    #[test]
    fn b7_non_jcodemunch_patterns_ignore_a_stale_index() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(BOGUS_SHA), 12);

        let out = s.run("P2,P5", &[]);
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(out.status.code(), Some(0), "stderr:\n{stderr}");
        assert!(
            !stderr.contains("E_JC_INDEX_STALE") && !stderr.contains("E_JC_INDEX_EMPTY"),
            "the gate must be unreachable for non-jcodemunch patterns; stderr:\n{stderr}"
        );
        assert!(
            stderr_has_parseable_findings_array(&stderr),
            "the run must still emit its findings array; stderr:\n{stderr}"
        );
    }

    /// SERVE-DOWN PRECEDENCE — with no serve there is no stale corpus to be
    /// misled by, so there is nothing to refuse.
    ///
    /// This is the regression lock on where the gate fires. The existing
    /// unreachable-serve fail-soft (P1 degrades to zero findings, exit 0, one
    /// breadcrumb) is a documented healthy path: jcodemunch is legitimately
    /// absent in a task worktree. Gating before the connection attempt would
    /// convert that into a hard exit 125 on every such machine — turning a
    /// fail-soft into an outage, and breaking the pre-existing contract that
    /// an optional substrate never fails a run.
    #[test]
    fn serve_down_fail_soft_takes_precedence_over_a_stale_index() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(BOGUS_SHA), 12);

        let unreachable = common::net::unreachable_mcp_url();
        let out = s.run("P1", &["--jcodemunch-url", &unreachable]);
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(0),
            "an unreachable serve must still fail-soft, not refuse; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("jcodemunch unreachable"),
            "the existing fail-soft breadcrumb must survive; stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("E_JC_INDEX_STALE") && !stderr.contains("E_JC_INDEX_EMPTY"),
            "the gate must not fire when no serve was reached; stderr:\n{stderr}"
        );
    }

    /// Overwrite `tasks_file` with ONE P1-eligible done task pinned to
    /// `commit`, replacing whatever `scenario()` put there.
    ///
    /// The eligibility rules (`status: "done"`, a `done_provenance.commit`, a
    /// non-null `done_at`) live in `task_json::done_task_fixture`, shared with
    /// `tests/jcodemunch_live.rs`'s `write_synthetic_done_task`, and the
    /// serialize-and-write in `write_tasks_json`; this helper only supplies
    /// what is scenario-specific.
    ///
    /// `since_sha` is derived by P1 as `{commit}^1`, which does NOT resolve in
    /// `scenario()`'s one-commit repo. That is fine and deliberate: both SHAs
    /// are passed verbatim into `RealJCodemunchOps::get_changed_symbols`'s
    /// `tools/call` arguments with no local git resolution, and the mock errors
    /// the call regardless.
    fn write_p1_done_task(tasks_file: &std::path::Path, commit: &str) {
        let mut task = done_task_fixture("synthetic-per-call-p1", commit, 1_700_000_000);
        // `seed.rs` is the one file `init_git_repo_with_one_commit` actually
        // commits into the scenario repo, so this record names a path that
        // EXISTS where the binary is pointed. `task_fixture`'s default is a
        // reify path with no meaning inside a throwaway temp repo; P1 never
        // reads `files` on this route (it derives its range from
        // `done_provenance.commit` and its symbols from `get_changed_symbols`),
        // so an inert-but-plausible path would read as a live premise.
        task["files"] = serde_json::json!(["seed.rs"]);

        let dir = tasks_file
            .parent()
            .expect("the scenario's tasks.json must live in a directory");
        let written = write_tasks_json(dir, &[task]);
        assert_eq!(
            written, tasks_file,
            "write_tasks_json must land on the scenario's OWN tasks.json — the \
             one `Scenario::run` passes as --tasks-file"
        );
    }

    /// The assertions a PER-CALL fail-soft run PASSES — i.e. everything that
    /// makes it indistinguishable from a genuine zero-finding success.
    ///
    /// Exit 0 (the seam fail-softs, it does not refuse); no `E_JC_INDEX_`
    /// marker (the §4.3 gate ADMITTED the run, so a detector really did
    /// execute and the breadcrumb the caller asserts on is reachable); no
    /// `jcodemunch unreachable at` (the handshake SUCCEEDED, so this is the
    /// PER-CALL layer and not the CONSTRUCTION one).
    ///
    /// Shared by all three `per_call_fail_soft_*` tests rather than written
    /// three times over — the shape `jcodemunch_live.rs::assert_live_leg`
    /// already uses for its two legs, and for the same reason: one contract
    /// asserted in three places drifts into three subtly different claims
    /// about it, and each copy then has to be found and fixed separately.
    ///
    /// Each caller adds only its own breadcrumb and findings assertions.
    fn assert_admitted_after_a_successful_handshake(out: &std::process::Output, stderr: &str) {
        assert_eq!(
            out.status.code(),
            Some(0),
            "a per-call failure must still fail-soft, not refuse; stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("E_JC_INDEX_"),
            "the gate must have ADMITTED this run — a refusal would mean no \
             detector ever ran and the per-call breadcrumb is unreachable; \
             stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("jcodemunch unreachable at"),
            "the handshake must have SUCCEEDED, or this would be the \
             CONSTRUCTION fail-soft layer rather than the PER-CALL one; \
             stderr:\n{stderr}"
        );
    }

    /// PER-CALL FAIL-SOFT — the vacuous pass that survives every other check.
    ///
    /// CONSUMER: `jcodemunch_live.rs::assert_live_leg`, via
    /// `breadcrumbs::PDEAD_CALL`. This test is the anti-rot lock for that
    /// literal. The seam tests over there prove `assert_live_leg` FIRES on a
    /// given string; they cannot prove that string is what the binary actually
    /// emits. This test observes the breadcrumb come out of the REAL binary on
    /// the ordinary merge gate, so a reword of the `eprintln!` in
    /// `RealJCodemunchOps::get_dead_code`'s `Err` arm turns THIS test red
    /// instead of silently reverting the capstone to vacuous.
    ///
    /// Both consumers read the SAME constant, so a reword is one edit in
    /// `tests/common/breadcrumbs.rs` and both move together. When each binary
    /// spelled its own copy, only prose bound them: fixing this test alone
    /// restored the green build while leaving the capstone asserting the
    /// absence of a string that could no longer appear — PRD §2.4's failure
    /// mode displaced one file over.
    ///
    /// The sibling below (`serve_down_fail_soft_takes_precedence_over_a_stale_index`,
    /// above) pins the CONSTRUCTION layer; this pins the PER-CALL layer. They
    /// are independent: a per-call failure happens AFTER a successful
    /// handshake, which is exactly what the first four assertions here
    /// document — this run is indistinguishable from success without the
    /// fifth.
    ///
    /// HERMETIC and gate-resident, like the rest of this module: no serve, no
    /// network, no `uvx`, no `#[ignore]`.
    #[test]
    fn per_call_fail_soft_is_a_vacuous_pass_the_capstone_must_catch() {
        let s = scenario();
        // FRESH index, so the §4.3 gate ADMITS. Required: under a
        // jcodemunch-only run set a stale/empty index hard-exits 125 BEFORE
        // any `tools/call`, and no breadcrumb could ever be reached.
        write_index_db(&s.index_dir, &s.repo_id, Some(&s.live_head), 7);

        // `None` => HTTP 200 carrying a top-level JSON-RPC `error` envelope,
        // which `JcodemunchClient::call_tool` maps to `LoadError::Protocol` —
        // landing on precisely the `Err` arm that prints the breadcrumb.
        // `initialize` and `notifications/initialized` are still answered
        // normally, so `RealJCodemunchOps::new` SUCCEEDS and the construction
        // fail-soft stays silent.
        let mock = spawn_mock_mcp(|_args| None);
        let out = s.run("PDEAD", &["--jcodemunch-url", mock.url()]);
        // Tear the mock down BEFORE asserting so a failing assertion cannot
        // leak the accept thread into the rest of the run (the convention
        // every other mock-using test in this file follows; `Drop` alone sets
        // the flag without joining the thread).
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        // --- the four assertions that ALL PASS on this vacuous run ---
        assert_admitted_after_a_successful_handshake(&out, &stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "the errored op returns Vec::new(), so PDEAD's array must be \
             empty here; got {findings:?}\nstderr:\n{stderr}"
        );

        // --- the fifth, and the only one that can tell the difference ---
        assert!(
            stderr.contains(breadcrumbs::PDEAD_GET_DEAD_CODE),
            "the real binary must emit the per-call fail-soft breadcrumb this \
             run's emptiness is EXPLAINED BY. Every assertion above passed, so \
             without this one the run is indistinguishable from a genuine \
             zero-finding success. If the `eprintln!` at \
             the Err arm of RealJCodemunchOps::get_dead_code was reworded, \
             update breadcrumbs::PDEAD_GET_DEAD_CODE — the live capstone reads the \
             same constant and moves with it.\nstderr:\n{stderr}"
        );
    }

    /// PER-CALL FAIL-SOFT, P1 leg — the anti-rot lock for the load-bearing
    /// half of `breadcrumbs::P1_CALL`.
    ///
    /// CONSUMER: `jcodemunch_live.rs::assert_live_leg`, via
    /// `breadcrumbs::P1_CALL`, which reads the same
    /// `breadcrumbs::P1_GET_CHANGED_SYMBOLS` this test asserts on — so a
    /// reword of the `eprintln!` in `RealJCodemunchOps::get_changed_symbols`'s
    /// `Err` arm is one edit and both consumers move.
    ///
    /// `find_references` is NOT asserted HERE, and could not be: this
    /// responder errors every tool, so `get_changed_symbols` returns no
    /// symbol, so `p1_producer_orphan::check`'s `for symbol in ...` loop —
    /// which is where the `find_references` call lives — never runs its body. That is a property of THIS responder, not of the harness —
    /// the sibling below (`per_call_fail_soft_on_p1s_second_call`) dispatches
    /// on the arguments to reach it.
    #[test]
    fn per_call_fail_soft_on_the_p1_pair() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(&s.live_head), 7);
        write_p1_done_task(&s.tasks_file, &s.live_head);

        let mock = spawn_mock_mcp(|_args| None);
        let out = s.run("P1", &["--jcodemunch-url", mock.url()]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_admitted_after_a_successful_handshake(&out, &stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "get_changed_symbols returned Vec::new(), so P1 has nothing to \
             report; got {findings:?}\nstderr:\n{stderr}"
        );

        assert!(
            stderr.contains(breadcrumbs::P1_GET_CHANGED_SYMBOLS),
            "the real binary must emit the per-call fail-soft breadcrumb. If \
             the `eprintln!` in the Err arm of \
             RealJCodemunchOps::get_changed_symbols was reworded, \
             update breadcrumbs::P1_GET_CHANGED_SYMBOLS — the live capstone \
             reads the same constant.\nstderr:\n{stderr}"
        );
    }

    /// PER-CALL FAIL-SOFT, P1's SECOND call — the anti-rot lock for
    /// `breadcrumbs::P1_FIND_REFERENCES`.
    ///
    /// The sibling above errors EVERY tool, so `get_changed_symbols` returns
    /// nothing and `find_references` is never reached. That is a limitation of
    /// that responder, not of the mock: `spawn_mock_mcp`'s closure receives
    /// the `tools/call` arguments, and the two calls are trivially
    /// distinguishable — `get_changed_symbols` sends
    /// `{repo, since_sha, until_sha}` while `find_references` sends
    /// `{repo, identifier}` (each op's own `call_tool` arguments). So
    /// this test DISPATCHES: it answers the first call with a real symbol row
    /// and errors only the second, which is what walks the real binary into
    /// `RealJCodemunchOps::find_references`'s `Err` arm.
    ///
    /// Without it, `P1_FIND_REFERENCES` would be pinned only by
    /// `jcodemunch_live.rs`'s seam test — i.e. against SYNTHETIC stderr, which
    /// proves `assert_live_leg` fires on the string but not that the binary
    /// ever emits it. A reword of that `Err` arm would then leave every test
    /// green while the live capstone asserted the absence of a string that can
    /// no longer appear.
    ///
    /// The symbol row is `seed`/`seed.rs`/line 1, which is not decoration:
    /// `init_git_repo_with_one_commit` commits exactly `pub fn seed() {}` into
    /// the scenario repo, so suppression enrichment
    /// (`RealJCodemunchOps::get_changed_symbols`) reads a file that EXISTS and
    /// finds no `#[allow(dead_code)]` / `#[cfg(test)]` above the declaration.
    /// A symbol that tripped either guard would be skipped before
    /// `find_references` was ever called, and this test would pass vacuously.
    ///
    /// HERMETIC and gate-resident: no serve, no network, no `uvx`, no
    /// `#[ignore]`.
    #[test]
    fn per_call_fail_soft_on_p1s_second_call() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(&s.live_head), 7);
        write_p1_done_task(&s.tasks_file, &s.live_head);

        // `identifier` is present ONLY in find_references' arguments — error
        // that one, answer get_changed_symbols with one well-formed
        // `added_symbols` row (the shape `changed_symbols_from_wire` reads).
        let mock = spawn_mock_mcp(|args| {
            if args.get("identifier").is_some() {
                None
            } else {
                Some(serde_json::json!({
                    "added_symbols": [{"name": "seed", "file": "seed.rs", "line": 1}]
                }))
            }
        });
        let out = s.run("P1", &["--jcodemunch-url", mock.url()]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_admitted_after_a_successful_handshake(&out, &stderr);
        // The FIRST call must have SUCCEEDED, or this test would be re-proving
        // the sibling above rather than reaching the second call at all.
        assert!(
            !stderr.contains(breadcrumbs::P1_GET_CHANGED_SYMBOLS),
            "get_changed_symbols was answered, so its breadcrumb must be \
             silent — if it fired, the dispatch predicate is wrong and \
             find_references was never reached; stderr:\n{stderr}"
        );

        // The symbol flowed all the way through: find_references errored, so
        // it returned no reference, so P1 found no non-test caller and
        // reported the orphan. Asserting the FINDING (not emptiness) is what
        // proves the row survived decode + enrichment + every per-symbol
        // suppression guard.
        let findings = parse_findings_from_stderr(&stderr);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly the one orphan finding for `seed`; \
             got {findings:?}\nstderr:\n{stderr}"
        );
        assert_eq!(
            findings[0]["pattern"], "P1ProducerOrphan",
            "the finding must come from the P1 detector; \
             got {:?}",
            findings[0]
        );
        assert!(
            findings[0]["summary"].as_str().unwrap_or_default().contains("`seed`"),
            "the finding must name the symbol the mock returned, or the row \
             did not actually reach the detector; got {:?}",
            findings[0]
        );

        assert!(
            stderr.contains(breadcrumbs::P1_FIND_REFERENCES),
            "the real binary must emit find_references' per-call fail-soft \
             breadcrumb. If the `eprintln!` at \
             the Err arm of RealJCodemunchOps::find_references was reworded, \
             update breadcrumbs::P1_FIND_REFERENCES — the live capstone reads the \
             same constant.\nstderr:\n{stderr}"
        );
    }

    /// `--no-jcodemunch` PRECEDENCE — the explicit escape hatch bypasses the
    /// seam entirely, so it must bypass the gate too. An escape hatch that
    /// still hard-failed on index state would not be an escape hatch.
    #[test]
    fn no_jcodemunch_bypasses_the_gate_entirely() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(BOGUS_SHA), 12);

        let out = s.run("P1", &["--no-jcodemunch"]);
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(out.status.code(), Some(0), "stderr:\n{stderr}");
        assert!(
            !stderr.contains("E_JC_INDEX_STALE") && !stderr.contains("E_JC_INDEX_EMPTY"),
            "--no-jcodemunch must bypass the gate; stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("jcodemunch unreachable"),
            "--no-jcodemunch must not even attempt a connection; stderr:\n{stderr}"
        );
    }

    /// BLAST RADIUS — a mixed run set must NOT be killed by an unusable
    /// corpus.
    ///
    /// The default sweep runs P2, P5, PTODO and PDSSENTINEL alongside P1, and
    /// none of those four consult jcodemunch at all. Refusing the process here
    /// would delete four working detectors' output over one stale index — and
    /// §4.2 makes that the EXPECTED case, not an anomaly: identity is now
    /// per-checkout, so every task worktree derives an id nothing has indexed.
    /// A default `reify-audit --since <date>` from any warm lane with the
    /// serve up would exit 125 with zero findings.
    ///
    /// So the corpus is still never queried (the Noop seam answers every
    /// jcodemunch call with nothing), but the run completes and emits its
    /// findings array, exactly as the unreachable-serve fail-soft does.
    #[test]
    fn default_sweep_degrades_rather_than_refusing_on_a_stale_index() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(BOGUS_SHA), 12);
        let mock = spawn_jcodemunch_mock();

        let out = s.run_default_sweep(&["--jcodemunch-url", mock.url()]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_ne!(
            out.status.code(),
            Some(125),
            "a mixed run set must not be refused wholesale; stderr:\n{stderr}"
        );
        assert!(
            stderr_has_parseable_findings_array(&stderr),
            "the non-jcodemunch detectors must still emit their findings; stderr:\n{stderr}"
        );
        // Degraded, not silent: the marker keeps the condition machine-
        // detectable, and the breadcrumb names what was lost.
        assert!(
            stderr.contains("E_JC_INDEX_STALE"),
            "the degraded sweep must still carry the marker; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("degraded to zero findings"),
            "the breadcrumb must name what was lost; stderr:\n{stderr}"
        );
    }

    /// The same stale corpus, selected by an ALL-jcodemunch pattern, is still
    /// a hard refusal — nothing in that run set could have survived it. Pins
    /// the boundary from the other side, so a future widening of the
    /// degrade path cannot silently swallow §4.3.
    #[test]
    fn all_jcodemunch_pattern_still_refuses_hard() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(BOGUS_SHA), 12);
        let mock = spawn_jcodemunch_mock();

        let out = s.run("P1,PDEAD", &["--jcodemunch-url", mock.url()]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(125),
            "an all-jcodemunch run set must refuse; stderr:\n{stderr}"
        );
        assert!(stderr.contains("E_JC_INDEX_STALE"), "stderr:\n{stderr}");
        assert!(
            !stderr_has_parseable_findings_array(&stderr),
            "a refusal must emit NO findings array; stderr:\n{stderr}"
        );
    }

    /// A valid SQLite file carrying the WRONG SCHEMA is neither stale nor
    /// empty: the file exists, and what is behind it is unknown. It gets its
    /// own code and its own remedy, because "re-index this checkout" is the
    /// wrong instruction for a corpus that may be perfectly intact.
    ///
    /// The reader-level version of this is pinned in the library's unit tests;
    /// what is pinned HERE is that the diagnostic survives all the way into
    /// the rendered CLI message, which no reader-level test can show.
    #[test]
    fn schema_drifted_index_refuses_with_its_own_code_and_diagnostic() {
        let s = scenario();
        let db = index_db_path(&s.index_dir, &s.repo_id);
        let conn = rusqlite::Connection::open(&db).expect("open drifted db");
        conn.execute_batch("CREATE TABLE something_else (x INTEGER);")
            .expect("create unrelated table");
        drop(conn);
        let mock = spawn_jcodemunch_mock();

        let out = s.run("P1", &["--jcodemunch-url", mock.url()]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(out.status.code(), Some(125), "stderr:\n{stderr}");
        assert!(
            stderr.contains("E_JC_INDEX_UNREADABLE"),
            "schema drift must carry its own marker, not be collapsed into \
             EMPTY; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("index unreadable:"),
            "the reader's diagnostic must reach the operator; stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("re-index this checkout before querying"),
            "the empty/stale remedy points at the wrong artifact here; stderr:\n{stderr}"
        );
        assert!(
            !stderr_has_parseable_findings_array(&stderr),
            "a refusal must emit NO findings array; stderr:\n{stderr}"
        );
    }

    /// A freshness claim that cannot be VERIFIED gets a third message,
    /// carrying no marker token at all.
    ///
    /// `--project-root` outside any git repo makes `git rev-parse HEAD` fail,
    /// so there is no live head to compare against. Neither staleness nor
    /// emptiness nor unreadability of the INDEX has been established, and
    /// labelling this as any of them would send the operator to re-index when
    /// the real fault is the git invocation.
    #[test]
    fn unverifiable_head_refuses_without_naming_an_index_fault() {
        let s = scenario();
        write_index_db(&s.index_dir, &s.repo_id, Some(&s.live_head), 2);
        let non_git = s.repo.parent().expect("tempdir parent").join("not-a-repo");
        std::fs::create_dir_all(&non_git).expect("create non-git root");
        let mock = spawn_jcodemunch_mock();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern", "P1",
                "--tasks-file", s.tasks_file.to_str().unwrap(),
                "--runs-db", s.runs_db.to_str().unwrap(),
                "--project-root", non_git.to_str().unwrap(),
                "--jcodemunch-index-dir", s.index_dir.to_str().unwrap(),
                "--jcodemunch-url", mock.url(),
            ])
            .output()
            .expect("invoke reify-audit");
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(125),
            "an unverifiable head must refuse; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("cannot verify jcodemunch index freshness"),
            "the third message must be distinct from the index refusals; stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("E_JC_INDEX_"),
            "no index fault has been established, so no index marker may be \
             emitted; stderr:\n{stderr}"
        );
        assert!(
            !stderr_has_parseable_findings_array(&stderr),
            "a refusal must emit NO findings array; stderr:\n{stderr}"
        );
    }
}

// -----------------------------------------------------------------------
// §4.2 — the derived identity must reach the wire
// -----------------------------------------------------------------------

/// Close the loop between §4.2 and the detector query.
///
/// It is not enough that `resolve_repo_id` returns the right string in a unit
/// test: the id the detector actually SENDS as `"repo"` must be that same
/// derived value, and the id the gate PROBES must be that same value too. A
/// plausible wiring slip is for an override to reach the ops constructor while
/// the gate silently keeps checking the derived path — which would gate one
/// index and query another, re-opening exactly the vacuity the gate closes.
mod wire_identity {
    use super::*;
    use crate::common::index_fixture::{
        expected_repo_id, index_db_path, init_git_repo_with_one_commit, write_index_db,
    };

    /// Spawn a jcodemunch mock that RECORDS every `tools/call` arguments
    /// object it is handed, returning the shared log alongside the server.
    ///
    /// `spawn_mock_mcp`'s responder is bounded `Fn(&Value) -> Option<Value> +
    /// Send + Sync + 'static`, so it can close over shared state. There is no
    /// existing recorder helper in this file to reuse — the other mock call
    /// sites merely INSPECT their args — so the capture is written here.
    fn spawn_recording_mock() -> (MockServer, Arc<std::sync::Mutex<Vec<serde_json::Value>>>) {
        let calls: Arc<std::sync::Mutex<Vec<serde_json::Value>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&calls);
        let mock = spawn_mock_mcp(move |args| {
            sink.lock().expect("record call args").push(args.clone());
            // A canned layer violation keeps PLAYER's dispatch path alive, so
            // the wire call this test observes is a real detector query.
            Some(serde_json::json!({
                "violations": [{
                    "from": "crates/reify-cli",
                    "to": "crates/reify-kernel",
                    "from_symbol": "reify_cli::main",
                    "to_symbol": "reify_kernel::solver::Solver::solve",
                    "allowed": false,
                    "rule_index": 0
                }]
            }))
        });
        (mock, calls)
    }

    /// Every recorded call that carries a `repo` field, as strings.
    fn recorded_repos(calls: &Arc<std::sync::Mutex<Vec<serde_json::Value>>>) -> Vec<String> {
        calls
            .lock()
            .expect("read recorded calls")
            .iter()
            .filter_map(|args| args.get("repo").and_then(|r| r.as_str()).map(str::to_string))
            .collect()
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        repo: std::path::PathBuf,
        index_dir: std::path::PathBuf,
        tasks_file: std::path::PathBuf,
        runs_db: std::path::PathBuf,
        live_head: String,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo dir");
        let index_dir = tmp.path().join("code-index");
        std::fs::create_dir_all(&index_dir).expect("create index dir");
        let live_head = init_git_repo_with_one_commit(&repo);
        let tasks_file = write_tasks_json(tmp.path(), &[]);
        let runs_db = write_empty_runs_db(tmp.path());
        Fixture { _tmp: tmp, repo, index_dir, tasks_file, runs_db, live_head }
    }

    impl Fixture {
        fn run(&self, url: &str, extra: &[&str]) -> std::process::Output {
            let bin = env!("CARGO_BIN_EXE_reify-audit");
            let mut cmd = Command::new(bin);
            cmd.args([
                "--pattern", "PLAYER",
                "--jcodemunch-url", url,
                "--tasks-file", self.tasks_file.to_str().unwrap(),
                "--runs-db", self.runs_db.to_str().unwrap(),
                "--project-root", self.repo.to_str().unwrap(),
                "--jcodemunch-index-dir", self.index_dir.to_str().unwrap(),
            ]);
            cmd.args(extra);
            cmd.output().expect("invoke reify-audit")
        }
    }

    /// With no `--jcodemunch-repo`, the id on the wire must be the §4.2 derived
    /// identity for the project root — and specifically NOT the legacy
    /// git-identity default this task removed.
    #[test]
    fn derived_repo_id_reaches_the_wire() {
        let f = fixture();
        let derived = expected_repo_id(&f.repo);
        write_index_db(&f.index_dir, &derived, Some(&f.live_head), 2);

        let (mock, calls) = spawn_recording_mock();
        let out = f.run(mock.url(), &[]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(0),
            "fresh index must admit the run; stderr:\n{stderr}"
        );

        let repos = recorded_repos(&calls);
        assert!(
            !repos.is_empty(),
            "no jcodemunch call carrying a `repo` was observed — the detector \
             never queried, so this test would be vacuous; stderr:\n{stderr}"
        );
        for repo in &repos {
            assert_eq!(
                repo, &derived,
                "the wire must carry the §4.2 derived identity; recorded {repos:?}"
            );
            assert_ne!(
                repo, "leodearden/reify",
                "the removed legacy git-identity default must never reach the wire"
            );
        }
    }

    /// `--jcodemunch-repo` must reach BOTH consumers: the wire AND the gate.
    ///
    /// The index is written ONLY at the override's flattened filename and
    /// deliberately NOT at the derived path, so admission is itself the proof
    /// that the gate probed the override — had it kept checking the derived
    /// path it would have found no index and refused with E_JC_INDEX_EMPTY.
    #[test]
    fn override_repo_id_reaches_both_the_wire_and_the_gate() {
        let f = fixture();
        let override_id = "my/custom-repo";
        let override_db = index_db_path(&f.index_dir, override_id);
        write_index_db(&f.index_dir, override_id, Some(&f.live_head), 2);
        assert_eq!(
            override_db.file_name().unwrap().to_str().unwrap(),
            "my-custom-repo.db",
            "the override's slash must be flattened for the on-disk filename"
        );
        assert!(
            !index_db_path(&f.index_dir, &expected_repo_id(&f.repo)).exists(),
            "no index at the DERIVED path: admission must be attributable to \
             the gate probing the override"
        );

        let (mock, calls) = spawn_recording_mock();
        let out = f.run(mock.url(), &["--jcodemunch-repo", override_id]);
        mock.stop();
        let stderr = String::from_utf8_lossy(&out.stderr);

        assert_eq!(
            out.status.code(),
            Some(0),
            "the gate must probe the OVERRIDE's index, not the derived path; \
             stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("E_JC_INDEX_EMPTY") && !stderr.contains("E_JC_INDEX_STALE"),
            "the override's index is fresh, so neither marker may appear; \
             stderr:\n{stderr}"
        );

        let repos = recorded_repos(&calls);
        assert!(!repos.is_empty(), "the detector never queried; stderr:\n{stderr}");
        for repo in &repos {
            assert_eq!(
                repo, override_id,
                "the override must reach the wire verbatim; recorded {repos:?}"
            );
        }
    }
}
