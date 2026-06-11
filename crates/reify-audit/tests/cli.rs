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

// -----------------------------------------------------------------------
// Fixture helpers
// -----------------------------------------------------------------------

/// Minimal tasks.json fixture object with all 9 required TaskMetadata fields.
/// Returns a serde_json::Value so callers can override fields as needed.
fn task_fixture(
    task_id: &str,
    status: &str,
    kind: Option<&str>,
    commit: Option<&str>,
) -> serde_json::Value {
    let done_provenance = match kind {
        Some(k) => serde_json::json!({
            "kind": k,
            "commit": commit,
            "note": null
        }),
        None => serde_json::Value::Null,
    };
    serde_json::json!({
        "task_id": task_id,
        "status": status,
        "files": ["crates/reify-audit/src/lib.rs"],
        "done_provenance": done_provenance,
        "title": format!("Task {}", task_id),
        "prd": null,
        "consumer_ref": null,
        "audit_foundation": null,
        "done_at": null
    })
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
    let json_start = stderr
        .rfind("\n[")
        .map(|pos| pos + 1) // skip the '\n', keep the '['
        .or_else(|| {
            // Fallback: JSON starts at position 0 (no preceding diagnostic lines).
            if stderr.starts_with('[') { Some(0) } else { None }
        })
        .unwrap_or_else(|| panic!("no JSON array found in stderr; full stderr:\n{stderr}"));
    serde_json::from_str(&stderr[json_start..]).unwrap_or_else(|e| {
        panic!(
            "stderr does not contain valid JSON after '[': {e}\nstderr:\n{stderr}"
        )
    })
}

/// Bind an OS-assigned port, record it, then drop the listener so the port
/// is closed before the binary connects. Returns a URL pointing at the freed
/// port suitable for "connection refused" tests.
///
/// TOCTOU: another process could reclaim the freed port before the binary
/// connects. In practice ephemeral ports are not immediately reused and this
/// idiom is widely accepted for this purpose.
fn closed_port_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    format!("http://127.0.0.1:{port}/mcp")
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
/// disabled, mirroring `tests/real_git_ops.rs`). After this, `git -C <dir>
/// ls-files` returns every fixture path so `RealGitOps::ls_files` enumerates
/// them for the PTODO structural sweep.
fn git_init_commit_all(dir: &Path) {
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
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

        // Find a closed port to guarantee connection refused.
        let throwaway = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = throwaway.local_addr().expect("addr").port();
        drop(throwaway);
        let unreachable_url = format!("http://127.0.0.1:{port}/mcp");

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

        let closed_url = closed_port_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern", "P1",
                "--jcodemunch-url", &closed_url,
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

        // Fallback breadcrumb must appear on stderr.
        assert!(
            stderr.contains("jcodemunch"),
            "stderr must contain fallback breadcrumb mentioning 'jcodemunch'; stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("degrad") || stderr.contains("unreachable") || stderr.contains("Noop"),
            "stderr breadcrumb must describe fail-soft degradation; stderr:\n{stderr}"
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

        let closed_url = closed_port_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--jcodemunch-url", &closed_url,
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

        let closed_url = closed_port_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern", "P1",
                "--no-jcodemunch",
                "--jcodemunch-url", &closed_url,
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

        let closed_url = closed_port_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task", "42",
                "--pre-done",
                "--jcodemunch-url", &closed_url,
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

        let closed_url = closed_port_url();

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--pattern", "P5",
                "--jcodemunch-url", &closed_url,
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

        // All findings are Medium → exit 0.
        assert_eq!(
            out.status.code(),
            Some(0),
            "PTODO fixture sweep must exit 0 (all Medium, no High); got {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);

        assert_eq!(
            findings.len(),
            3,
            "PTODO fixture sweep must emit exactly 3 findings \
             (untracked/malformed-cite/phantom-tracking); got:\n{:#}",
            serde_json::Value::Array(findings.clone())
        );

        // Every fixture finding is Pattern::PTodo / Severity::Medium.
        for f in &findings {
            assert_eq!(
                f["pattern"].as_str(),
                Some("PTodo"),
                "every fixture finding must be PTodo; got:\n{f:#}"
            );
            assert_eq!(
                f["severity"].as_str(),
                Some("Medium"),
                "every fixture finding must be Medium; got:\n{f:#}"
            );
        }

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

        // Scenario 10: neither the inline-escape file nor the allowlisted nested
        // file may surface a finding.
        let none_mentions = |needle: &str| -> bool {
            !findings
                .iter()
                .any(|f| f["task_id"].as_str().is_some_and(|t| t.contains(needle)))
        };
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

        // (3) Fail-soft: all findings Medium → exit 0, never 125.
        assert_eq!(
            out.status.code(),
            Some(0),
            "DB-absent degradation must exit 0, never 125; got {:?}\nstderr:\n{stderr}",
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
            stderr.contains("' — PTODO liveness degraded; structural checks still run"),
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
        assert_eq!(
            out.status.code(),
            Some(0),
            "orphaned finding is Medium → exit 0; got {:?}\nstderr:\n{stderr}",
            out.status.code()
        );

        let findings = parse_findings_from_stderr(&stderr);

        // The orphaned cite: a PTodo/Medium finding at cited.rs whose summary
        // names the id and the terminal status.
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
        assert_eq!(orphaned["severity"].as_str(), Some("Medium"));
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
            !stderr.contains("PTODO liveness degraded"),
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
        assert_eq!(
            out.status.code(),
            Some(0),
            "exit 0; stderr:\n{stderr}"
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
            !stderr.contains("PTODO liveness degraded"),
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

fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let status_text = match status {
        200 => "OK",
        202 => "Accepted",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
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

/// Spawn a one-shot mock MCP server. `task_responder` is given the
/// `tools/call` arguments and returns the JSON-RPC `result` value to send
/// back (or `None` to return an error envelope). Returns a [`MockServer`]
/// handle; the caller calls [`MockServer::stop`] (or lets it drop) to tear
/// down. The accept loop also uses a short `set_nonblocking` poll so it
/// wakes periodically to check the stop flag even without a wakeup
/// connection — that way a stop request can't hang the test runner.
fn spawn_mock_mcp<F>(task_responder: F) -> MockServer
where
    F: Fn(&serde_json::Value) -> Option<serde_json::Value> + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
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
                    write_response(&mut stream, 200, resp.to_string().as_bytes());
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

        // Find a port that's almost certainly closed: bind, get its port,
        // then drop the listener so subsequent connects refuse.
        let throwaway = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = throwaway.local_addr().expect("addr").port();
        drop(throwaway);
        let url = format!("http://127.0.0.1:{port}/mcp/");

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
