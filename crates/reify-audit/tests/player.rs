//! Integration tests for the P-LAYER import-layer-violation detector.
//!
//! User-observable signal (per task description and
//! `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §4-d/§8):
//!   `cargo test -p reify-audit player::tests`
//!
//! Cargo's test filter matches a path-substring against test paths
//! *within* the integration-test binary. To make the substring `player::tests`
//! resolve, the file body is wrapped in `mod player { mod tests { ... } }` so
//! each test's path becomes `player::tests::<name>` — matching the
//! puntested.rs convention.
//!
//! All tests use in-memory rusqlite + MockJCodemunchOps + MockGitOps so they
//! remain hermetic. PLAYER is repo-wide (ignores task_metadata/target_task_id/
//! window), so task_metadata is empty and target_task_id/window/now/
//! producer_branch are all None.

mod player {

use reify_audit::{
    AuditContext, EvidenceRef, LayerViolation, MockGitOps, MockJCodemunchOps, Pattern,
    Severity, player,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

/// Build a LayerViolation with the given fields.
fn lv(from: &str, to: &str, rule: &str) -> LayerViolation {
    LayerViolation {
        from_file: from.to_string(),
        to_file: to.to_string(),
        rule: rule.to_string(),
    }
}

mod tests {
    use super::*;

    /// Core contract: a single LayerViolation yields exactly one finding with
    /// the correct shape.
    ///
    /// Seeds one LayerViolation and asserts:
    /// - exactly 1 finding
    /// - pattern == PLayerViolation
    /// - severity == Low
    /// - task_id is empty (repo-wide, not task-scoped)
    /// - summary contains from_file, to_file, and rule strings
    /// - evidence == vec![EvidenceRef::File { path: from_file }]
    #[test]
    fn layer_violation_yields_finding() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();

        // Rule string matches the production wire format emitted by
        // `layer_violations_from_wire`: `format!("rule[{rule_index}]: {from_symbol} → {to_symbol}")`.
        jc.set_layer_violations(vec![lv(
            "crates/reify-ast/src/parser.rs",
            "crates/reify-eval/src/engine.rs",
            "rule[0]: reify_ast::parser → reify_eval::engine",
        )]);

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

        let findings = player::check(&ctx);

        assert_eq!(
            findings.len(),
            1,
            "expected exactly 1 finding; got {:?}",
            findings
        );

        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PLayerViolation, "pattern must be PLayerViolation");
        assert_eq!(f.severity, Severity::Low, "severity must be Low");
        assert!(
            f.task_id.is_empty(),
            "PLAYER findings must have empty task_id (repo-wide); got {:?}",
            f.task_id
        );

        assert!(
            f.summary.contains("crates/reify-ast/src/parser.rs"),
            "summary must contain from_file; got: {}",
            f.summary
        );
        assert!(
            f.summary.contains("crates/reify-eval/src/engine.rs"),
            "summary must contain to_file; got: {}",
            f.summary
        );
        assert!(
            f.summary.contains("rule[0]: reify_ast::parser → reify_eval::engine"),
            "summary must contain rule; got: {}",
            f.summary
        );

        assert_eq!(
            f.evidence,
            vec![EvidenceRef::File {
                path: "crates/reify-ast/src/parser.rs".to_string()
            }],
            "evidence must be a single EvidenceRef::File pointing at from_file"
        );
    }

    /// Empty mock yields zero findings — the "clean ruleset" signal.
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

        let findings = player::check(&ctx);
        assert!(
            findings.is_empty(),
            "empty mock must yield no findings; got {:?}",
            findings
        );
    }

    /// Multiple violations map one-to-one: each LayerViolation becomes exactly
    /// one Finding, and each from_file is cited in the corresponding evidence.
    /// Faithful pass-through: the detector does not merge, filter, or reorder.
    #[test]
    fn multiple_violations_map_one_to_one() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();

        let violations = vec![
            lv(
                "crates/reify-core/src/span.rs",
                "crates/reify-ast/src/tree.rs",
                "core-must-not-depend-on-ast",
            ),
            lv(
                "crates/reify-ast/src/walk.rs",
                "crates/reify-compiler/src/lower.rs",
                "ast-must-not-depend-on-compiler",
            ),
            lv(
                "crates/reify-ir/src/node.rs",
                "crates/reify-eval/src/exec.rs",
                "ir-must-not-depend-on-eval",
            ),
        ];

        jc.set_layer_violations(violations.clone());

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

        let findings = player::check(&ctx);

        assert_eq!(
            findings.len(),
            violations.len(),
            "finding count must equal violation count (1:1 map); got {:?}",
            findings
        );

        // Each violation's from_file must appear in the corresponding finding.
        for (i, v) in violations.iter().enumerate() {
            assert!(
                findings[i].summary.contains(&v.from_file),
                "finding[{}] summary must contain from_file '{}'; got: {}",
                i,
                v.from_file,
                findings[i].summary
            );
            assert!(
                findings[i].evidence.iter().any(|e| {
                    matches!(e, EvidenceRef::File { path } if path == &v.from_file)
                }),
                "finding[{}] evidence must cite from_file '{}'; got {:?}",
                i,
                v.from_file,
                findings[i].evidence
            );
        }
    }

} // mod tests

} // mod player
