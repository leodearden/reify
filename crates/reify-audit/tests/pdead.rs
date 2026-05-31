//! Integration tests for the P-DEAD dead-code detector.
//!
//! User-observable signal (per task description and
//! `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3/§4-d):
//!   `cargo test -p reify-audit pdead::tests`
//!
//! Cargo's test filter matches a path-substring against test paths
//! *within* the integration-test binary. To make the substring `pdead::tests`
//! resolve, the file body is wrapped in `mod pdead { mod tests { ... } }` so
//! each test's path becomes `pdead::tests::<name>` — matching the p1.rs/p2.rs
//! convention.
//!
//! All tests use in-memory rusqlite + MockJCodemunchOps + MockGitOps so they
//! remain hermetic. PDEAD is repo-wide (ignores task_metadata/target_task_id/
//! window), so task_metadata is empty and target_task_id/window/now/
//! producer_branch are all None.

mod pdead {

use reify_audit::{
    AuditContext, DeadSymbol, EvidenceRef, MockGitOps, MockJCodemunchOps, Pattern,
    Severity, pdead_dead_code,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

fn dead_sym(
    id: &str,
    name: &str,
    kind: &str,
    file: &str,
    line: usize,
    confidence: f64,
    signals: Vec<&str>,
) -> DeadSymbol {
    DeadSymbol {
        id: id.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        file: file.to_string(),
        line,
        confidence,
        signals: signals.iter().map(|s| s.to_string()).collect(),
    }
}

mod tests {
    use super::*;

    /// Core contract: confidence filtering delegates to the seam.
    ///
    /// Seeds three DeadSymbols at confidence 0.3, 0.5, 0.9. The seam's
    /// `>= 0.5` filter drops the 0.3 one; check() must yield exactly TWO
    /// findings with pattern==PDeadCode, severity==Low, each citing its
    /// symbol's file via EvidenceRef::File. Also pins that at least one
    /// summary contains name, line number, "confidence", and a signal string.
    #[test]
    fn confidence_filter_and_finding_shape() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();

        jc.set_dead_code(vec![
            dead_sym("sym-a", "dropped_fn",  "function", "crates/reify-x/src/a.rs", 10, 0.3, vec!["no_callers"]),
            dead_sym("sym-b", "boundary_fn", "function", "crates/reify-x/src/b.rs", 20, 0.5, vec!["no_callers", "private_module"]),
            dead_sym("sym-c", "certain_fn",  "function", "crates/reify-x/src/c.rs", 30, 0.9, vec!["no_callers"]),
        ]);

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = pdead_dead_code::check(&ctx);

        // Pin the threshold contract: check() must request exactly 0.5 from the seam.
        // This ensures the detector's DEFAULT_MIN_CONFIDENCE constant is the one
        // actually passed to get_dead_code(), independent of the mock's filter behavior.
        assert_eq!(
            jc.last_dead_code_min_confidence(),
            Some(0.5),
            "check() must pass DEFAULT_MIN_CONFIDENCE (0.5) to get_dead_code"
        );

        assert_eq!(
            findings.len(),
            2,
            "expected exactly 2 findings (0.3 dropped by seam); got {:?}",
            findings
        );

        for f in &findings {
            assert_eq!(f.pattern, Pattern::PDeadCode, "pattern must be PDeadCode");
            assert_eq!(f.severity, Severity::Low, "severity must be Low");
            // PDEAD is repo-wide: task_id must be empty (contract for downstream consumers).
            assert!(f.task_id.is_empty(), "PDEAD findings must have empty task_id; got {:?}", f.task_id);
        }

        // Each finding cites its symbol's file via EvidenceRef::File.
        let files_in_evidence: Vec<&str> = findings
            .iter()
            .flat_map(|f| {
                f.evidence.iter().filter_map(|e| {
                    if let EvidenceRef::File { path } = e { Some(path.as_str()) } else { None }
                })
            })
            .collect();

        assert!(
            files_in_evidence.contains(&"crates/reify-x/src/b.rs"),
            "finding for sym-b must cite its file; evidence files: {:?}",
            files_in_evidence
        );
        assert!(
            files_in_evidence.contains(&"crates/reify-x/src/c.rs"),
            "finding for sym-c must cite its file; evidence files: {:?}",
            files_in_evidence
        );

        // At least one summary surfaces name/line/confidence/signals.
        let summary_b = findings
            .iter()
            .find(|f| {
                f.evidence.iter().any(|e| {
                    matches!(e, EvidenceRef::File { path } if path == "crates/reify-x/src/b.rs")
                })
            })
            .map(|f| f.summary.clone())
            .expect("finding for b.rs must exist");

        assert!(
            summary_b.contains("boundary_fn"),
            "summary must contain symbol name; got: {summary_b}"
        );
        assert!(
            summary_b.contains("20"),
            "summary must contain line number; got: {summary_b}"
        );
        assert!(
            summary_b.contains("confidence"),
            "summary must contain 'confidence'; got: {summary_b}"
        );
        assert!(
            summary_b.contains("no_callers"),
            "summary must contain at least one signal; got: {summary_b}"
        );
    }

    /// Default-empty mock yields zero findings.
    #[test]
    fn empty_mock_yields_no_findings() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = pdead_dead_code::check(&ctx);
        assert!(
            findings.is_empty(),
            "empty mock must yield no findings; got {:?}",
            findings
        );
    }

    /// Scope-excludes: stdlib prefix and test paths are suppressed.
    ///
    /// Seeds four above-threshold (confidence 0.9) DeadSymbols:
    /// - stdlib: crates/reify-stdlib/src/prelude.rs → excluded
    /// - test dir: crates/reify-x/tests/it.rs → excluded
    /// - _test.rs suffix: crates/reify-x/src/foo_test.rs → excluded
    /// - normal src: crates/reify-x/src/live.rs → survives
    ///
    /// check() must yield exactly ONE finding citing live.rs.
    #[test]
    fn scope_excludes_stdlib_and_test_paths() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();

        jc.set_dead_code(vec![
            dead_sym("s1", "stdlib_sym",   "function", "crates/reify-stdlib/src/prelude.rs", 1, 0.9, vec!["no_callers"]),
            dead_sym("s2", "test_dir_sym", "function", "crates/reify-x/tests/it.rs",         2, 0.9, vec!["no_callers"]),
            dead_sym("s3", "test_src_sym", "function", "crates/reify-x/src/foo_test.rs",     3, 0.9, vec!["no_callers"]),
            dead_sym("s4", "live_sym",     "function", "crates/reify-x/src/live.rs",         4, 0.9, vec!["no_callers"]),
        ]);

        let ctx = AuditContext {
            project_root: PathBuf::from("/tmp/fake-project"),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let findings = pdead_dead_code::check(&ctx);

        assert_eq!(
            findings.len(),
            1,
            "expected exactly 1 finding (stdlib+test paths excluded); got {:?}",
            findings
        );

        assert!(
            findings[0].evidence.iter().any(|e| {
                matches!(e, EvidenceRef::File { path } if path == "crates/reify-x/src/live.rs")
            }),
            "surviving finding must cite live.rs; got {:?}",
            findings[0].evidence
        );
    }

} // mod tests

} // mod pdead
