//! Integration tests for the P1 producer-orphan detector.
//!
//! User-observable signal (per task description and
//! `docs/architecture-audit/f-infra-design.md` §5 P1):
//!   `cargo test -p reify-audit p1::tests`
//!
//! Cargo's test filter matches a path-substring against test paths
//! *within* the integration-test binary. To make the substring `p1::tests`
//! resolve, the file body is wrapped in `mod p1 { mod tests { ... } }` so
//! each test's path becomes `p1::tests::<name>` — matching the p5.rs/p2.rs
//! convention.
//!
//! All tests use in-memory rusqlite + MockJCodemunchOps + MockGitOps so they
//! remain hermetic (P1 issues zero SQL; the in-memory connection satisfies
//! AuditContext.conn without requiring a schema).

mod p1 {

use reify_audit::{
    AuditContext, ChangedSymbol, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, Pattern,
    Severity, SymbolReference, TaskMetadata, p1_producer_orphan,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

/// A fixed synthetic "now" (epoch-seconds) so grace-window boundaries are
/// deterministic. Tests derive `done_at` relative to this.
const NOW: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

/// Build a `done` producer task with a benign title (does NOT signal a
/// stub) and the given done-flip timestamp + originating PRD.
fn done_meta(task_id: &str, done_at: i64, prd: Option<&str>) -> TaskMetadata {
    TaskMetadata {
        task_id: task_id.to_string(),
        status: "done".to_string(),
        files: vec![],
        done_provenance: None,
        title: "Wire foo into bar".to_string(),
        prd: prd.map(|s| s.to_string()),
        consumer_ref: None,
        audit_foundation: None,
        done_at: Some(done_at),
    }
}

/// Build a `ChangedSymbol` with no suppression metadata (the orphan-candidate
/// default); individual tests flip `has_*` / `g_allow_marker` as needed.
fn changed_symbol(name: &str, file: &str) -> ChangedSymbol {
    ChangedSymbol {
        name: name.to_string(),
        file: file.to_string(),
        line: 42,
        has_allow_dead_code: false,
        has_cfg_test: false,
        g_allow_marker: None,
    }
}

mod tests {
    use super::*;

    /// Pin the public-API surface P1 adds by destructuring or exhaustively
    /// matching each new type. Adding a `Pattern` variant without an arm,
    /// renaming a `ChangedSymbol`/`SymbolReference` field, or changing the
    /// `AuditContext`/`TaskMetadata` shape will fail this test at compile
    /// time — exactly what downstream crates (T-4 CLI) need from a stable API.
    #[test]
    fn api_surface_pin() {
        // Pattern: all THREE variants must be reachable; the exhaustive
        // `match` forces a test update on any future enum extension.
        for p in [
            Pattern::P5PhantomDone,
            Pattern::P2ConsumerStub,
            Pattern::P1ProducerOrphan,
        ] {
            match p {
                Pattern::P5PhantomDone => {}
                Pattern::P2ConsumerStub => {}
                Pattern::P1ProducerOrphan => {}
            }
        }

        // ChangedSymbol / SymbolReference: destructure every field by name.
        let ChangedSymbol {
            name: _,
            file: _,
            line: _,
            has_allow_dead_code: _,
            has_cfg_test: _,
            g_allow_marker: _,
        } = ChangedSymbol {
            name: "new_widget".to_string(),
            file: "crates/reify-x/src/widget.rs".to_string(),
            line: 42,
            has_allow_dead_code: false,
            has_cfg_test: false,
            g_allow_marker: None,
        };
        let SymbolReference { file: _, line: _ } = SymbolReference {
            file: "crates/reify-y/src/uses_widget.rs".to_string(),
            line: 7,
        };

        // AuditContext: populate the new `jcodemunch` + `now` fields.
        // TaskMetadata: populate the existing `title` + the 4 new P1 fields.
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "1".to_string(),
            TaskMetadata {
                task_id: "1".to_string(),
                status: "done".to_string(),
                files: vec![],
                done_provenance: None,
                title: "Wire foo into bar".to_string(),
                prd: Some("docs/x.md".to_string()),
                consumer_ref: None,
                audit_foundation: None,
                done_at: Some(0),
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
            now: None,
        };

        // Default-empty mock → no changed symbols → no findings.
        let findings: Vec<Finding> = p1_producer_orphan::check(&ctx);
        assert!(
            findings.is_empty(),
            "default-empty mock must yield no findings; got {:?}",
            findings
        );

        // Severity remains reachable (pin alongside Pattern).
        for s in [Severity::Low, Severity::Medium, Severity::High] {
            match s {
                Severity::Low | Severity::Medium | Severity::High => {}
            }
        }
    }

    /// Required #1 — a `done` task introduced a public symbol with zero
    /// references, no pending consumer task, and a done-flip 15 days old
    /// (past the 14-day grace window) → exactly one Medium P1 finding
    /// citing the symbol's file via `EvidenceRef::File`.
    #[test]
    fn producer_orphan_flagged_medium_after_grace_window() {
        let done_at = NOW - 15 * DAY;

        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();
        jc.set_changed_symbols(
            "main",
            done_at,
            vec![changed_symbol("new_widget", "crates/reify-x/src/widget.rs")],
        );
        jc.set_find_references("new_widget", vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            "7001".to_string(),
            done_meta("7001", done_at, Some("docs/x.md")),
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
        assert_eq!(f.pattern, Pattern::P1ProducerOrphan);
        assert_eq!(
            f.severity,
            Severity::Medium,
            "15 days > 14-day grace → Medium; got {:?}",
            f.severity
        );
        assert_eq!(f.task_id, "7001");
        assert!(
            f.evidence.iter().any(|e| matches!(
                e,
                EvidenceRef::File { path } if path == "crates/reify-x/src/widget.rs"
            )),
            "expected EvidenceRef::File for widget.rs; got {:?}",
            f.evidence
        );
    }
}

} // mod p1
