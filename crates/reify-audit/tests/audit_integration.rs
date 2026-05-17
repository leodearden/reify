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
//! ## Relationship to per-detector test files
//!
//! `tests/p5.rs`, `tests/p1.rs`, and `tests/p2.rs` are the authoritative
//! detectors for their respective detectors and already cover each scenario in
//! depth. The three single-detector tests here (`p5_phantom_done_task_3242_*`,
//! `p1_producer_orphan_c04_*`, `p2_consumer_stub_c39_*`) are minimal smoke
//! anchors that confirm this binary compiles and links against the public lib
//! surface correctly — they are intentionally thin.
//!
//! The genuinely unique contribution of this binary is the
//! `seven_prepd_legacy_tasks_produce_no_false_positives` test, which exercises
//! a single shared `AuditContext` against all three detectors simultaneously and
//! asserts no cross-detector false positives on the pre-`/prd` legacy task shape.
//! That cross-detector scenario cannot be expressed as a single test in any of
//! the per-detector files.
//!
//! All fixtures are in-memory; no git repo, no runs.db file required.

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
/// `events.event_type`). Mirrors p5.rs:32; integration-test binaries are
/// standalone compilation units so a shared helper module is the right fix,
/// but that requires touching p5.rs (outside this task's locked scope).
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

    /// False-positive cross-check — seven pre-`/prd` legacy tasks (sourced
    /// from the phase-3-files-synthesis.md cluster index) must produce zero
    /// findings across all three detectors in a single shared AuditContext.
    ///
    /// Why none fires:
    ///  - P5 skips tasks with `done_provenance=None` (early-returns on the
    ///    `done_provenance.as_ref()?` guard in p5_phantom_done.rs).
    ///  - P1 skips tasks with `done_at=None` (the `let Some(done_at) = …`
    ///    destructure at p1_producer_orphan.rs:82-84).
    ///  - P2 iterates `meta.files` which is `vec![]` for all seven tasks
    ///    → zero `diff_added_lines` calls → no markers → no findings.
    ///
    /// Task IDs: 215 (C-24), 250 (C-07), 2347 (C-07 doc), 2358 (C-36),
    /// 2658 (C-04 substitute — avoids reusing 2657 from the P2 positive
    /// fixture in step-3), 2699 (C-07 dispatch), 2954 (C-07 docs-only).
    #[test]
    fn seven_prepd_legacy_tasks_produce_no_false_positives() {
        let conn = seed_db();
        // Seed the schema but no rows — P5 will check the DB; an empty
        // events table is valid (it just means no task_completed events).

        // All seven tasks share the canonical legacy shape: status=done,
        // files=vec![], no done_provenance, no done_at, benign title.
        let legacy_ids = ["215", "250", "2347", "2358", "2658", "2699", "2954"];
        let mut task_metadata = HashMap::new();
        for id in legacy_ids {
            task_metadata.insert(id.to_string(), legacy_meta(id));
        }

        // Default-empty mocks: no diff paths, no added lines, no changed
        // symbols, no refs. Any unexpected call returns an empty vec.
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: Some(NOW),
            producer_branch: None,
        };

        let p5_findings = p5_phantom_done::check(&ctx);
        assert!(
            p5_findings.is_empty(),
            "P5: expected no findings on seven legacy tasks; got {:?}",
            p5_findings
        );

        let p1_findings = p1_producer_orphan::check(&ctx);
        assert!(
            p1_findings.is_empty(),
            "P1: expected no findings on seven legacy tasks; got {:?}",
            p1_findings
        );

        let p2_findings = p2_consumer_stub::check(&ctx);
        assert!(
            p2_findings.is_empty(),
            "P2: expected no findings on seven legacy tasks; got {:?}",
            p2_findings
        );
    }

    /// Seeded incident #3 — P2 consumer-stub, synthetic cluster C-39 shape:
    /// Manifold `KernelAttributeHook::propagate_attributes` done task whose
    /// added diff introduces BOTH `tracing::warn!(reason="task_9_pending", …)`
    /// AND `unimplemented!()` on the same file. Both stub markers must appear
    /// in the single consolidated finding (per-file aggregation).
    #[test]
    fn p2_consumer_stub_c39_manifold_hook_shape_flags_both_markers() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        // P2 issues zero SQL; in-memory connection satisfies AuditContext.conn.
        let hook_path = "crates/reify-kernel-manifold/src/attr_hook.rs";

        let mut git = MockGitOps::new();
        // The task branch's diff of attr_hook.rs adds two stub lines that
        // mirror the actual C-39 cluster body (task_9_pending + unimplemented!).
        git.set_diff_added_lines(
            "main",
            "task/2657",
            hook_path,
            vec![
                (
                    15,
                    "        tracing::warn!(reason=\"task_9_pending\", \"manifold hook deferred\")"
                        .to_string(),
                ),
                (16, "        unimplemented!()".to_string()),
            ],
        );

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "2657".to_string(),
            TaskMetadata {
                task_id: "2657".to_string(),
                status: "done".to_string(),
                files: vec![hook_path.to_string()],
                done_provenance: None,
                // Benign title: no "stub"/"placeholder" → Medium not Low.
                title: "Wire Manifold attribute hook".to_string(),
                prd: None,
                consumer_ref: None,
                audit_foundation: None,
                done_at: None,
            },
        );

        let jc = MockJCodemunchOps::new(); // P2 ignores jcodemunch
        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata,
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = p2_consumer_stub::check(&ctx);
        // P2 aggregates per (task, file): two stub lines on one file → one finding.
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one P2 finding (dual-marker on one file); got {:?}",
            findings
        );
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::P2ConsumerStub, "wrong pattern: {:?}", f);
        assert_eq!(
            f.severity,
            Severity::Medium,
            "benign title → Medium; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "2657");
        // Evidence must reference the hook path.
        assert!(
            f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == hook_path
            )),
            "expected EvidenceRef::File for attr_hook.rs; got {:?}",
            f.evidence
        );
        // The §14 hand-off requirement: summary must contain BOTH canonical labels.
        assert!(
            f.summary.contains("tracing::warn!(task_pending)"),
            "summary must contain tracing::warn! label; got: {:?}",
            f.summary
        );
        assert!(
            f.summary.contains("unimplemented!"),
            "summary must contain unimplemented! label; got: {:?}",
            f.summary
        );
    }

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
            producer_branch: None,
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
        // Severity::Medium + Pattern::P1ProducerOrphan already prove the post-grace
        // branch executed; a summary-substring check would pin prose rather than
        // behaviour and is already covered by tests/p1.rs.
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
            producer_branch: None,
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
