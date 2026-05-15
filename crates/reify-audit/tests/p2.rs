//! Integration tests for the P2 consumer-stub detector.
//!
//! User-observable signal (per task description and
//! `docs/architecture-audit/f-infra-design.md` §5 P2):
//!   `cargo test -p reify-audit p2::tests`
//!
//! The body is wrapped in `mod p2 { mod tests { ... } }` so each test's
//! path becomes `p2::tests::<name>` — matching the p5.rs convention.
//!
//! All tests use in-memory rusqlite + MockGitOps so they remain hermetic
//! (P2 issues zero SQL; the in-memory connection satisfies AuditContext.conn
//! without requiring a schema).

mod p2 {

use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, Pattern, Severity, TaskMetadata,
    p2_consumer_stub,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

mod tests {
    use super::*;

    /// Build a minimal TaskMetadata with a benign title that does NOT
    /// contain "stub" or "placeholder" (so P2 emits Medium, not Low).
    fn benign_meta(task_id: &str, files: Vec<String>) -> TaskMetadata {
        TaskMetadata {
            task_id: task_id.to_string(),
            status: "in_progress".to_string(),
            files,
            done_provenance: None,
            title: "Wire foo into bar".to_string(),
        }
    }

    /// Verify that all nine canonical stub-pattern families are detected when
    /// they appear on the added-lines side of a diff (i.e. as `+` lines).
    ///
    /// Families covered (one synthetic path per family):
    ///   a) `crates/x/a.rs` — `// TODO(impl pending)`  (TODO.*pending)
    ///   b) `crates/x/b.rs` — `// TODO(post-merge)`    (TODO post-\w+)
    ///   c) `crates/x/c.rs` — `// TODO(wire later)`    (TODO.*later)
    ///   d) `crates/x/d.rs` — `// TODO(task_9999)`     (TODO task_\d+)
    ///   e) `crates/x/e.rs` — `unimplemented!()`
    ///   f) `crates/x/f.rs` — `panic!("not yet wired")`
    ///   g) `crates/x/g.rs` — `tracing::warn!(reason="task_foo_pending", "x")`
    ///   h) `crates/x/h.rs` — `Value::Undef => { /* pending */ }`  (Undef+pending)
    ///   i) `crates/x/i.rs` — `// fixme`
    ///
    /// Each path has exactly one stub line → nine findings expected.
    #[test]
    fn detects_canonical_stub_patterns_on_added_lines() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9001";
        let paths: Vec<&str> = vec![
            "crates/x/a.rs",
            "crates/x/b.rs",
            "crates/x/c.rs",
            "crates/x/d.rs",
            "crates/x/e.rs",
            "crates/x/f.rs",
            "crates/x/g.rs",
            "crates/x/h.rs",
            "crates/x/i.rs",
        ];
        let stub_lines: Vec<&str> = vec![
            "    // TODO(impl pending)",
            "    // TODO(post-merge)",
            "    // TODO(wire later)",
            "    // TODO(task_9999)",
            "    unimplemented!()",
            "    panic!(\"not yet wired\")",
            "    tracing::warn!(reason=\"task_foo_pending\", \"x\")",
            "    Value::Undef => { /* pending */ }",
            "    // fixme",
        ];

        let mut git = MockGitOps::new();
        let task_branch = format!("task/{}", task_id);
        for (path, stub_line) in paths.iter().zip(stub_lines.iter()) {
            git.set_diff_added_lines(
                "main",
                &task_branch,
                path,
                vec![(10, stub_line.to_string())],
            );
        }

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            task_id.to_string(),
            benign_meta(task_id, paths.iter().map(|p| p.to_string()).collect()),
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            task_metadata,
            target_task_id: None,
            window: None,
        };

        let findings = p2_consumer_stub::check(&ctx);
        assert_eq!(
            findings.len(),
            9,
            "expected 9 findings (one per stub-pattern path); got {:?}",
            findings
        );
        for f in &findings {
            assert_eq!(f.pattern, Pattern::P2ConsumerStub, "wrong pattern: {:?}", f);
            assert_eq!(f.severity, Severity::Medium, "benign title should → Medium: {:?}", f);
        }
        // Each finding's evidence must reference the correct file path.
        for path in &paths {
            let found = findings.iter().any(|f| {
                f.evidence.iter().any(|e| match e {
                    EvidenceRef::File { path: p } => p == path,
                    _ => false,
                })
            });
            assert!(found, "no finding with EvidenceRef::File for {path}");
        }
    }

    /// Verify that stub patterns that were already present on `main` (i.e. NOT
    /// added by this branch) are NOT flagged.  This pins the "added-lines seam
    /// only" invariant: the detector must consult `diff_added_lines`, never the
    /// full file contents.
    #[test]
    fn moved_code_preexisting_not_flagged() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let task_id = "9002";
        let path = "crates/x/moved.rs";

        let mut git = MockGitOps::new();
        // Empty added-lines: the stub marker existed pre-branch and was not touched.
        git.set_diff_added_lines("main", &format!("task/{}", task_id), path, vec![]);

        let mut task_metadata = HashMap::new();
        task_metadata.insert(
            task_id.to_string(),
            benign_meta(task_id, vec![path.to_string()]),
        );

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            task_metadata,
            target_task_id: None,
            window: None,
        };

        let findings = p2_consumer_stub::check(&ctx);
        assert!(
            findings.is_empty(),
            "pre-existing stubs on main must NOT be flagged; got {:?}",
            findings
        );
    }
}

} // mod p2
