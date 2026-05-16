//! Integration smoke test for the F-infra slice 1 detector core.
//!
//! User-observable signal (per task description and
//! `docs/architecture-audit/f-infra-design.md` §14 hand-off):
//!   `cargo test -p reify-audit --test audit_integration`
//!
//! Cargo's test filter matches a path-substring against test paths
//! *within* the integration-test binary (the binary's own filename is not
//! part of those paths). To make the substring `audit_integration::tests`
//! resolve, the file body is wrapped in
//! `mod audit_integration { mod tests { ... } }` so each test's path
//! becomes `audit_integration::tests::<name>` — matching the p5.rs/p1.rs/p2.rs
//! convention.
//!
//! Re-exercises the three detectors through the public lib surface with no
//! detector-internal seams. All fixtures are in-memory; no git repo, no
//! runs.db file required.

mod audit_integration {

use reify_audit::{
    AuditContext, ChangedSymbol, DoneProvenance, EvidenceRef, MockGitOps, MockJCodemunchOps,
    Pattern, Severity, TaskMetadata, p1_producer_orphan, p2_consumer_stub, p5_phantom_done,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

/// Minimal schema — pin reflects only the columns the production
/// `has_task_completed_event` query reads (`events.task_id` and
/// `events.event_type`). Verbatim from p5.rs:32 — intentional duplication;
/// if the schema changes, both p5.rs and this file must be updated, giving
/// two pinning sites that catch missed updates.
const RUNS_DB_SCHEMA: &str = r#"
CREATE TABLE events (task_id TEXT, event_type TEXT);
"#;

/// Open an in-memory SQLite connection and seed the events-table schema.
fn seed_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(RUNS_DB_SCHEMA).expect("create schema");
    conn
}

/// Seed a single `task_completed` event row for `task_id`.
fn insert_task_completed_event(conn: &Connection, task_id: &str) {
    conn.execute(
        "INSERT INTO events (task_id, event_type) VALUES (?, ?)",
        rusqlite::params![task_id, "task_completed"],
    )
    .unwrap();
}

/// Builder for the pre-`/prd` legacy shape used by the false-positive
/// cross-check (step-4): status=done, files=vec![], no done_provenance,
/// no prd/consumer_ref, no done_at, benign title. This shape clears all
/// three detectors without triggering any false positive.
fn legacy_meta(task_id: &str) -> TaskMetadata {
    TaskMetadata {
        task_id: task_id.to_string(),
        status: "done".to_string(),
        files: vec![],
        done_provenance: None,
        title: "Wire foo into bar".to_string(),
        prd: None,
        consumer_ref: None,
        audit_foundation: None,
        done_at: None,
    }
}

/// Fixed synthetic "now" (epoch-seconds) so grace-window boundaries are
/// deterministic across runs. Tests derive `done_at` relative to this.
/// Mirrors `const NOW` in p1.rs:29.
const NOW: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

mod tests {
    use super::*;

    /// Seeded incident #2 — P1 producer-orphan, synthetic cluster C-04 shape:
    /// public symbol `resolve_unique_by_attribute` introduced by a done task
    /// 15 days ago (past the 14-day grace window); zero workspace callers;
    /// no audit_foundation or pending consumer task → exactly one Medium finding.
    #[test]
    fn p1_producer_orphan_c04_shape_flagged_medium_post_grace() {
        // 15 days past done-flip: strictly beyond the 14-day grace window.
        let done_at = NOW - 15 * DAY;

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        // P1 issues zero SQL; in-memory connection satisfies AuditContext.conn.
        let git = MockGitOps::new(); // P1 doesn't touch git

        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            "main",
            done_at,
            vec![ChangedSymbol {
                name: "resolve_unique_by_attribute".to_string(),
                file: "crates/reify-eval/src/selector_resolution.rs".to_string(),
                line: 42,
                has_allow_dead_code: false,
                has_cfg_test: false,
                g_allow_marker: None,
            }],
        );
        // Zero callers → true orphan.
        jc.set_find_references("resolve_unique_by_attribute", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "7301".to_string(),
            TaskMetadata {
                task_id: "7301".to_string(),
                status: "done".to_string(),
                files: vec![],
                done_provenance: None,
                title: "Wire persistent naming resolution".to_string(),
                prd: Some("docs/persistent-naming-v2.md".to_string()),
                consumer_ref: None,
                audit_foundation: None,
                done_at: Some(done_at),
            },
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: Some(NOW),
        };

        let findings = p1_producer_orphan::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one P1 finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P1ProducerOrphan, "wrong pattern: {:?}", f);
        assert_eq!(
            f.severity,
            Severity::Medium,
            "15 days > 14-day grace → Medium; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "7301");
        // Evidence must include EvidenceRef::File for the symbol's declaring file.
        assert!(
            f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/reify-eval/src/selector_resolution.rs"
            )),
            "expected EvidenceRef::File for selector_resolution.rs; got {:?}",
            f.evidence
        );
        // Summary must mention the grace window in the post-grace wording.
        assert!(
            f.summary.to_lowercase().contains("grace window"),
            "summary must mention grace window; got {:?}",
            f.summary
        );
    }

    /// Seeded incident #1 — P5 phantom-done, May-09 task-3242 shape:
    /// kind=merged, claimed commit's diff does NOT cover `metadata.files`;
    /// `git log main --grep` returns empty (no sibling-FF rescue).
    ///
    /// Re-exercises the same incident shape as
    /// `p5::tests::task_3242_shape_returns_high_severity_phantom_done` (in
    /// `tests/p5.rs`) but via the integration-test binary's public-lib
    /// invocation path rather than the unit-test seam.
    #[test]
    fn p5_phantom_done_task_3242_shape_flagged_high() {
        let conn = seed_db();
        // task_completed event is present (the orchestrator wrote one) —
        // the phantom-done signal is in the *git* corroboration, not the DB.
        insert_task_completed_event(&conn, "3242");

        let mut git = MockGitOps::new();
        // No sibling-FF rescue: log_grep returns empty.
        git.set_log_grep("main", "3242", vec![]);
        // Claimed commit's diff covers only an unrelated path; the
        // `metadata.files` entry (pruning.rs) is absent → phantom-done.
        git.set_diff_changed_paths(
            "main",
            "7958491da22f",
            vec!["docs/unrelated.md".to_string()],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "3242".to_string(),
            TaskMetadata {
                task_id: "3242".to_string(),
                status: "done".to_string(),
                // The actual May-09 3242 incident path from the task write-up.
                files: vec!["crates/reify-shell-extract/src/pruning.rs".to_string()],
                done_provenance: Some(DoneProvenance {
                    kind: Some("merged".to_string()),
                    commit: Some("7958491da22f".to_string()),
                    note: None,
                }),
                title: "Wire shell-extract pruning".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new(); // P5 ignores jcodemunch
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
        };

        let findings = p5_phantom_done::check(&ctx);
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one phantom-done finding; got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P5PhantomDone, "wrong pattern: {:?}", f);
        assert_eq!(f.severity, Severity::High, "expected High severity; got {:?}", f.severity);
        assert_eq!(f.task_id, "3242");

        // Evidence must include a MetadataFiles ref naming the missing path.
        let meta_files_evidence = f.evidence.iter().find_map(|e| match e {
            EvidenceRef::MetadataFiles { entries } => Some(entries),
            _ => None,
        });
        let entries = meta_files_evidence.expect("MetadataFiles evidence must be present");
        assert!(
            entries.iter().any(|p| p == "crates/reify-shell-extract/src/pruning.rs"),
            "expected pruning.rs in MetadataFiles evidence; got {:?}",
            entries
        );
    }
}

} // mod audit_integration
