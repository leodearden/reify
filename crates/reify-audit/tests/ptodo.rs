//! Integration tests for the PTODO structural-lane detector (`ptodo::check`).
//!
//! User-observable signal:
//!   `cargo test -p reify-audit ptodo::tests`
//!
//! These tests drive `check()` against a real on-disk working tree (a
//! `tempfile` tempdir as `project_root`) plus a `MockGitOps` whose
//! `set_ls_files` supplies the tracked-path enumeration. The structural lane
//! reads working-tree content via `std::fs::read_to_string(project_root.join(
//! path))`, so the files must exist on disk; only the file *list* is mocked.
//! In-memory rusqlite + MockJCodemunchOps satisfy `AuditContext` (the
//! structural lane issues no SQL and no jcodemunch queries).

mod common;

mod ptodo {

use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, Pattern, Severity,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

/// Write `content` to relative `path` inside `root`, creating parent dirs.
fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write file");
}

mod tests {
    use super::*;

    /// The structural lane emits exactly the three content-marker kinds
    /// (untracked / malformed-cite / phantom-tracking), one per offending
    /// swept file, and suppresses each of: an allowlisted-prefix path, an
    /// inline-escaped line, a non-swept extension, and a canonically-cited
    /// marker.
    #[test]
    fn check_emits_three_kinds_and_suppresses_the_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Three offending swept files → one finding each.
        write_file(root, "untracked.rs", "// TODO: wire this\n");
        write_file(root, "malformed.rs", "// TODO(task δ): migrate\n");
        write_file(root, "phantom.rs", "// tracked as a follow-up task\n");
        // Suppressed paths: allowlisted prefix, inline escape, non-swept ext,
        // and a canonically-cited marker (tracked → deferred to β).
        write_file(root, "crates/reify-audit/x.rs", "// TODO: allowlisted self\n");
        write_file(root, "escaped.rs", "// TODO: leave me  // ptodo:allow\n");
        write_file(root, "notes.md", "// TODO: in a non-swept doc\n");
        write_file(root, "cited.rs", "// TODO(#4553): cited\n");

        let all_paths: Vec<String> = [
            "untracked.rs",
            "malformed.rs",
            "phantom.rs",
            "crates/reify-audit/x.rs",
            "escaped.rs",
            "notes.md",
            "cited.rs",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let mut git = MockGitOps::new();
        git.set_ls_files(all_paths);

        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: root.to_path_buf(),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = reify_audit::ptodo::check(&ctx);

        // Exactly three findings, all PTodo + Medium.
        assert_eq!(
            findings.len(),
            3,
            "expected exactly 3 PTODO findings; got {findings:?}"
        );
        for f in &findings {
            assert_eq!(f.pattern, Pattern::PTodo, "wrong pattern: {f:?}");
            assert_eq!(f.severity, Severity::Medium, "wrong severity: {f:?}");
        }

        // Locate the finding whose evidence references `path`.
        let finding_for = |path: &str| -> &Finding {
            findings
                .iter()
                .find(|f| {
                    f.evidence
                        .iter()
                        .any(|e| matches!(e, EvidenceRef::File { path: p } if p == path))
                })
                .unwrap_or_else(|| panic!("no finding referencing {path}; findings={findings:?}"))
        };

        // §8.3 kind carried as a stable summary prefix `"<kind>: …"`.
        assert!(
            finding_for("untracked.rs").summary.starts_with("untracked:"),
            "untracked.rs summary must start with the kind token: {:?}",
            finding_for("untracked.rs").summary
        );
        assert!(
            finding_for("malformed.rs")
                .summary
                .starts_with("malformed-cite:"),
            "malformed.rs summary must start with the kind token: {:?}",
            finding_for("malformed.rs").summary
        );
        assert!(
            finding_for("phantom.rs")
                .summary
                .starts_with("phantom-tracking:"),
            "phantom.rs summary must start with the kind token: {:?}",
            finding_for("phantom.rs").summary
        );

        // The four suppressed files must yield NO finding.
        for suppressed in [
            "crates/reify-audit/x.rs",
            "escaped.rs",
            "notes.md",
            "cited.rs",
        ] {
            let any = findings.iter().any(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path: p } if p == suppressed))
            });
            assert!(
                !any,
                "{suppressed} must not produce a finding; findings={findings:?}"
            );
        }
    }

    /// End-to-end `check` with the task DB PRESENT at the default path: a
    /// canonically-cited file (`#4444`, seeded `done`) and an untracked file
    /// coexist — the liveness lane emits an `orphaned` finding for the cited
    /// file (carrying `#4444` + `done`) while the structural lane emits the
    /// `untracked` finding for the other. (RED until step-10 wires the lane.)
    #[test]
    fn check_runs_liveness_lane_alongside_structural() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "cited.rs", "// TODO(#4444): x\n");
        write_file(root, "untracked.rs", "// TODO: wire\n");

        // Seed the DB at the DEFAULT resolved path (no env override needed).
        crate::common::schema::seed_tasks_db_at(
            &root.join(".taskmaster/tasks/tasks.db"),
            &[("master", 4444, "done")],
        );

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["cited.rs".to_string(), "untracked.rs".to_string()]);

        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: root.to_path_buf(),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = reify_audit::ptodo::check(&ctx);

        let find_for = |path: &str| -> Option<&Finding> {
            findings.iter().find(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path: p } if p == path))
            })
        };

        // The structural lane still flags the untracked marker.
        let untracked = find_for("untracked.rs")
            .unwrap_or_else(|| panic!("expected untracked finding; findings={findings:?}"));
        assert!(
            untracked.summary.starts_with("untracked:"),
            "summary: {}",
            untracked.summary
        );

        // The liveness lane flags the orphaned cite (terminal `done` status).
        let orphaned = find_for("cited.rs")
            .unwrap_or_else(|| panic!("expected orphaned finding; findings={findings:?}"));
        assert_eq!(orphaned.pattern, Pattern::PTodo);
        assert_eq!(orphaned.severity, Severity::Medium);
        assert!(
            orphaned.summary.starts_with("orphaned:"),
            "summary: {}",
            orphaned.summary
        );
        assert!(orphaned.summary.contains("#4444"), "summary: {}", orphaned.summary);
        assert!(orphaned.summary.contains("done"), "summary: {}", orphaned.summary);
    }

    /// §6.7 degradation (in-process): the SAME tree as the liveness test but
    /// with NO task DB at the default path. `check` must skip the liveness lane
    /// silently — only the structural `untracked` finding is returned, with no
    /// `orphaned`/`unknown-id` finding for the cited file, and no panic.
    #[test]
    fn check_degrades_when_tasks_db_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        write_file(root, "cited.rs", "// TODO(#4444): x\n");
        write_file(root, "untracked.rs", "// TODO: wire\n");
        // NB: no `.taskmaster/tasks/tasks.db` is seeded → the default path is
        // absent, so the read-only open fails and the liveness lane degrades.

        let mut git = MockGitOps::new();
        git.set_ls_files(vec!["cited.rs".to_string(), "untracked.rs".to_string()]);

        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: root.to_path_buf(),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = reify_audit::ptodo::check(&ctx);

        // The structural lane still flags the untracked marker.
        assert!(
            findings.iter().any(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path } if path == "untracked.rs"))
                    && f.summary.starts_with("untracked:")
            }),
            "untracked structural finding must survive DB-absent degradation; findings={findings:?}"
        );

        // The liveness lane is skipped entirely: no orphaned/unknown-id finding,
        // and in particular none referencing the cited file.
        for f in &findings {
            assert!(
                !f.summary.starts_with("orphaned:") && !f.summary.starts_with("unknown-id:"),
                "no liveness finding may be emitted when the DB is absent; got {:?}",
                f.summary
            );
        }
        assert!(
            !findings.iter().any(|f| {
                f.evidence
                    .iter()
                    .any(|e| matches!(e, EvidenceRef::File { path } if path == "cited.rs"))
            }),
            "cited file must yield no finding when the DB is absent; findings={findings:?}"
        );
    }
}

}

// -----------------------------------------------------------------------
// §8.2/§8.3 liveness lane — resolve_liveness against a seeded tasks table
// -----------------------------------------------------------------------
//
// These tests drive `ptodo::resolve_liveness` directly against an in-memory
// `tasks` table (common::schema::seed_tasks_db), with NO filesystem and NO
// real task DB. They pin the §8.2 multi-cite rule and the §8.3 orphaned /
// unknown-id taxonomy (both Medium).

mod liveness {
    use crate::common::schema::{insert_task, seed_tasks_db};
    use reify_audit::{EvidenceRef, Pattern, Severity};

    /// Build a single cited-marker tuple `(path, line, ids, text)` — the shape
    /// `check` collects from `scan_file`'s `Cited` entries.
    fn marker(path: &str, line: usize, ids: &[u32], text: &str) -> (String, usize, Vec<u32>, String) {
        (path.to_string(), line, ids.to_vec(), text.to_string())
    }

    #[test]
    fn orphaned_done_carries_id_and_status() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 4444, "done");

        let cited = vec![marker("a.rs", 2, &[4444], "// TODO(#4444): x")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert_eq!(findings.len(), 1, "one orphaned finding; got {findings:?}");
        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PTodo);
        assert_eq!(f.severity, Severity::Medium);
        assert!(f.summary.starts_with("orphaned:"), "summary: {}", f.summary);
        assert!(f.summary.contains("#4444"), "summary must carry id: {}", f.summary);
        assert!(f.summary.contains("done"), "summary must carry status: {}", f.summary);
        assert_eq!(f.task_id, "a.rs");
        assert!(
            matches!(&f.evidence[..], [EvidenceRef::File { path }] if path == "a.rs"),
            "evidence must be a single File ref at the path: {:?}",
            f.evidence
        );
    }

    #[test]
    fn orphaned_cancelled_is_terminal() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 10, "cancelled");

        let cited = vec![marker("b.rs", 1, &[10], "// TODO(#10): y")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert_eq!(findings.len(), 1, "got {findings:?}");
        assert!(findings[0].summary.starts_with("orphaned:"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("cancelled"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("#10"), "{}", findings[0].summary);
    }

    #[test]
    fn live_statuses_emit_no_finding() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 1, "pending");
        insert_task(&conn, "master", 2, "in-progress");

        let cited = vec![
            marker("p.rs", 1, &[1], "// TODO(#1): a"),
            marker("p.rs", 2, &[2], "// TODO(#2): b"),
        ];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert!(findings.is_empty(), "live cites must not orphan; got {findings:?}");
    }

    #[test]
    fn absent_id_is_unknown_id() {
        let conn = seed_tasks_db();
        // id 999 is never seeded.
        let cited = vec![marker("c.rs", 5, &[999], "// TODO(#999): z")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert_eq!(findings.len(), 1, "got {findings:?}");
        assert!(findings[0].summary.starts_with("unknown-id:"), "{}", findings[0].summary);
        assert!(findings[0].summary.contains("#999"), "{}", findings[0].summary);
    }

    #[test]
    fn one_live_cite_suffices_for_multi_cite_marker() {
        let conn = seed_tasks_db();
        insert_task(&conn, "master", 4444, "done");
        insert_task(&conn, "master", 5555, "pending");

        // A single marker citing one terminal (done) AND one live (pending) id
        // → tracked → NO finding (§8.2 "one live cite suffices").
        let cited = vec![marker("m.rs", 3, &[4444, 5555], "// TODO(#4444, #5555): x")];
        let findings = reify_audit::ptodo::resolve_liveness(&conn, &cited).expect("resolve");

        assert!(findings.is_empty(), "one live cite suffices; got {findings:?}");
    }
}
