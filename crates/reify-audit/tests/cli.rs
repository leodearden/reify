//! Integration tests for the `reify-audit` CLI binary.
//!
//! Tests invoke the compiled binary via `env!("CARGO_BIN_EXE_reify-audit")`
//! and assert on stdout, stderr, and exit codes.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test cli`

use std::path::Path;
use std::process::Command;

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
        for flag in &["--task", "--pre-done", "--since", "--pattern"] {
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

    /// Invoking the binary without `--tasks-file` must exit 125 with a clear
    /// message naming the missing flag. The old silent default
    /// `.taskmaster/tasks/tasks.json` must not silently replace it.
    #[test]
    fn missing_tasks_file_exits_125_with_clear_error() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();
        let runs_db = write_empty_runs_db(dir);

        let bin = env!("CARGO_BIN_EXE_reify-audit");
        let out = Command::new(bin)
            .args([
                "--task",
                "1",
                "--pre-done",
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
            "missing --tasks-file must exit 125; got {:?}\nstdout: {}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("--tasks-file"),
            "stderr must mention '--tasks-file' to identify the missing flag; \
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
                "--tasks-file",
                tasks_file.to_str().unwrap(),
                "--runs-db",
                runs_db.to_str().unwrap(),
                "--project-root",
                dir.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit --pattern P1");

        assert_eq!(
            out.status.code(),
            Some(0),
            "--pattern P1 with NoopJCodemunchOps must exit 0"
        );

        let stderr = String::from_utf8_lossy(&out.stderr);
        let findings = parse_findings_from_stderr(&stderr);
        assert!(
            findings.is_empty(),
            "--pattern P1 with Noop must yield zero findings; got:\n{:#}",
            serde_json::Value::Array(findings)
        );
    }
}

