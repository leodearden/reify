//! Integration tests for the P5 phantom-done detector.
//!
//! User-observable signal (per task description and
//! `docs/architecture-audit/f-infra-design.md` §10):
//!   `cargo test -p reify-audit p5::tests`
//!
//! Cargo's test filter is a path-substring match. The integration-test path
//! is `p5::tests::<name>` which contains the literal substring `p5::tests`,
//! so this file is wrapped in `mod tests { ... }` to make the filter resolve.
//!
//! All tests use in-memory rusqlite + MockGitOps so they remain hermetic
//! (no real git repo, no real runs.db file).

use reify_audit::{
    AuditContext, DoneProvenance, EvidenceRef, Finding, GitCommit, MockGitOps, Pattern, Severity,
    TaskMetadata, p5_phantom_done,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

/// Schema pin for the live `data/orchestrator/runs.db`. Verified via
/// `SELECT sql FROM sqlite_master ...` against the production file.
/// Production code never CREATE TABLEs (read-only). If dark-factory migrates
/// the schema, these tests fail meaningfully and we update in lockstep with
/// the production query.
const RUNS_DB_SCHEMA: &str = r#"
CREATE TABLE runs (
    run_id         TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL,
    prd_path       TEXT,
    started_at     TEXT NOT NULL,
    completed_at   TEXT,
    total_tasks    INTEGER DEFAULT 0,
    completed      INTEGER DEFAULT 0,
    blocked        INTEGER DEFAULT 0,
    escalated      INTEGER DEFAULT 0,
    total_cost_usd REAL DEFAULT 0.0,
    paused_for_cap INTEGER DEFAULT 0,
    cap_pause_secs REAL DEFAULT 0.0
);
CREATE TABLE task_results (
    run_id              TEXT NOT NULL REFERENCES runs(run_id),
    task_id             TEXT NOT NULL,
    project_id          TEXT NOT NULL,
    title               TEXT,
    outcome             TEXT NOT NULL,
    cost_usd            REAL DEFAULT 0.0,
    duration_ms         INTEGER DEFAULT 0,
    agent_invocations   INTEGER DEFAULT 0,
    execute_iterations  INTEGER DEFAULT 0,
    verify_attempts     INTEGER DEFAULT 0,
    review_cycles       INTEGER DEFAULT 0,
    steward_cost_usd    REAL DEFAULT 0.0,
    steward_invocations INTEGER DEFAULT 0,
    completed_at        TEXT,
    PRIMARY KEY (run_id, task_id)
);
CREATE TABLE events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT    NOT NULL,
    run_id      TEXT    NOT NULL,
    task_id     TEXT,
    event_type  TEXT    NOT NULL,
    phase       TEXT,
    role        TEXT,
    data        TEXT    DEFAULT '{}',
    cost_usd    REAL,
    duration_ms INTEGER
);
"#;

fn seed_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(RUNS_DB_SCHEMA).expect("create schema");
    conn
}

fn insert_run(conn: &Connection, run_id: &str) {
    conn.execute(
        "INSERT INTO runs (run_id, project_id, started_at) VALUES (?, 'reify', '2026-05-14T00:00:00Z')",
        rusqlite::params![run_id],
    )
    .unwrap();
}

fn insert_task_result(conn: &Connection, run_id: &str, task_id: &str, outcome: &str) {
    conn.execute(
        "INSERT INTO task_results (run_id, task_id, project_id, title, outcome) \
         VALUES (?, ?, 'reify', 'test task', ?)",
        rusqlite::params![run_id, task_id, outcome],
    )
    .unwrap();
}

fn insert_task_completed_event(conn: &Connection, run_id: &str, task_id: &str) {
    conn.execute(
        "INSERT INTO events (timestamp, run_id, task_id, event_type, data) \
         VALUES ('2026-05-14T00:01:00Z', ?, ?, 'task_completed', '{\"outcome\":\"done\"}')",
        rusqlite::params![run_id, task_id],
    )
    .unwrap();
}

mod tests {
    use super::*;

    #[test]
    fn happy_path_returns_no_findings() {
        // Seed runs.db with a clean done task: a `task_completed` event exists
        // and metadata.files is corroborated by the claimed merge commit.
        let conn = seed_db();
        insert_run(&conn, "run-happy");
        insert_task_result(&conn, "run-happy", "1000", "done");
        insert_task_completed_event(&conn, "run-happy", "1000");

        // MockGitOps reports the claimed commit's diff covers metadata.files
        // and nothing is gitignored.
        let mut git = MockGitOps::new();
        git.set_diff_changed_paths(
            "main",
            "abc123",
            vec!["crates/x/foo.rs".to_string()],
        );
        git.set_log_grep("main", "1000", vec![]);
        // (gitignored: default false for any path not explicitly set)

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "1000".to_string(),
            TaskMetadata {
                task_id: "1000".to_string(),
                status: "done".to_string(),
                files: vec!["crates/x/foo.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("abc123".to_string()),
                    note: None,
                }),
            },
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            task_metadata,
            target_task_id: None,
            window: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert!(
            findings.is_empty(),
            "expected no findings on clean happy-path; got {:?}",
            findings
        );

        // Sanity-touch the public types so this test pins the surface
        // (dead-code linter will complain if e.g. EvidenceRef variants drift).
        let _ = Pattern::P5PhantomDone;
        let _ = Severity::High;
        let _: Option<Finding> = None;
        let _: Option<EvidenceRef> = None;
        let _: Option<GitCommit> = None;
    }

    /// Models the May-09 task 3242 incident
    /// (`~/.claude/projects/-home-leo-src-reify/memory/project_task3242_unblock.md`):
    /// kind=merged, claimed commit not reachable from main, single-file
    /// `crates/reify-shell-extract/src/pruning.rs` edit. The corroborating
    /// diff does NOT cover that path → high-severity phantom-done.
    #[test]
    fn task_3242_shape_returns_high_severity_phantom_done() {
        let conn = seed_db();
        insert_run(&conn, "run-3242");
        insert_task_result(&conn, "run-3242", "3242", "done");
        // Task completed event is present (the orchestrator did write one) —
        // the smoking gun is that the *git* corroboration fails, not the DB.
        insert_task_completed_event(&conn, "run-3242", "3242");

        let mut git = MockGitOps::new();
        // log_grep returns empty → no sibling-FF rescue.
        git.set_log_grep("main", "3242", vec![]);
        // diff of the claimed commit against main does NOT include
        // pruning.rs → metadata.files mismatch → phantom-done.
        git.set_diff_changed_paths(
            "main",
            "7958491da22f",
            // Only some unrelated path showed up in the claimed commit.
            vec!["docs/unrelated.md".to_string()],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "3242".to_string(),
            TaskMetadata {
                task_id: "3242".to_string(),
                status: "done".to_string(),
                files: vec!["crates/reify-shell-extract/src/pruning.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("7958491da22f".to_string()),
                    note: None,
                }),
            },
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            task_metadata,
            target_task_id: None,
            window: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one phantom-done finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.task_id, "3242");

        // Evidence must include a MetadataFiles ref naming the missing path.
        let metadata_files_evidence = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::MetadataFiles { entries } => Some(entries),
            _ => None,
        });
        let entries = metadata_files_evidence.expect("MetadataFiles evidence present");
        assert!(
            entries
                .iter()
                .any(|p| p == "crates/reify-shell-extract/src/pruning.rs"),
            "expected pruning.rs in MetadataFiles evidence; got {:?}",
            entries
        );
    }

    /// Models the Cargo.lock-only false-positive
    /// (`~/.claude/projects/-home-leo-src-reify/memory/project_post_merge_equivalence_false_positive_cargo_lock.md`):
    /// every "real" metadata.files path is covered by the claimed commit's
    /// diff; only Cargo.lock fell out because main absorbed an unrelated
    /// dependency bump in the meantime. Should downgrade to Severity::Low
    /// rather than scream Severity::High.
    #[test]
    fn cargo_lock_only_divergence_downgrades_to_low() {
        let conn = seed_db();
        insert_run(&conn, "run-cargo-lock");
        insert_task_result(&conn, "run-cargo-lock", "2000", "done");
        insert_task_completed_event(&conn, "run-cargo-lock", "2000");

        let mut git = MockGitOps::new();
        // Claimed commit's diff covers ONLY foo.rs — Cargo.lock is missing.
        git.set_diff_changed_paths(
            "main",
            "def456",
            vec!["crates/reify-x/src/foo.rs".to_string()],
        );
        // Sibling-FF rescue is irrelevant for the Cargo.lock case.
        git.set_log_grep("main", "2000", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "2000".to_string(),
            TaskMetadata {
                task_id: "2000".to_string(),
                status: "done".to_string(),
                files: vec![
                    "crates/reify-x/src/foo.rs".to_string(),
                    "Cargo.lock".to_string(),
                ],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("def456".to_string()),
                    note: None,
                }),
            },
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            task_metadata,
            target_task_id: None,
            window: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding (downgraded); got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone);
        assert_eq!(
            f.severity,
            Severity::Low,
            "Cargo.lock-only divergence must downgrade to Low; got {:?}",
            f.severity
        );
        assert!(
            f.summary.to_lowercase().contains("cargo.lock"),
            "summary should mention Cargo.lock; got {:?}",
            f.summary
        );
        let metadata_files_evidence = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::MetadataFiles { entries } => Some(entries),
            _ => None,
        });
        let entries = metadata_files_evidence.expect("MetadataFiles evidence present");
        assert!(
            entries.iter().any(|p| p == "Cargo.lock"),
            "expected Cargo.lock in MetadataFiles evidence; got {:?}",
            entries
        );
    }
}
