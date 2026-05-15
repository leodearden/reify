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
/// We scan for the first `[` to locate the JSON portion.
///
/// This keeps tests robust to git failures in temp dirs (which aren't real
/// git repositories).
fn parse_findings_from_stderr(stderr: &str) -> Vec<serde_json::Value> {
    let json_start = stderr
        .find('[')
        .unwrap_or_else(|| panic!("no '[' found in stderr; full stderr:\n{stderr}"));
    serde_json::from_str(&stderr[json_start..]).unwrap_or_else(|e| {
        panic!(
            "stderr does not contain valid JSON after '[': {e}\nstderr: {stderr}"
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
    #[test]
    fn task_spot_check_runs_all_three_detectors() {
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
                Some("P5PhantomDone") | Some("MetadataFilesGitignored")
            )
        });
        assert!(
            non_p5.is_none(),
            "--pattern P5 must not include P1/P2 findings; got:\n{:#}",
            serde_json::Value::Array(findings)
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

