//! Integration tests for the P-UNTESTED static test-reachability detector.
//!
//! User-observable signal (per task description and
//! `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §4-d):
//!   `cargo test -p reify-audit puntested::tests`
//!
//! Cargo's test filter matches a path-substring against test paths
//! *within* the integration-test binary. To make the substring `puntested::tests`
//! resolve, the file body is wrapped in `mod puntested { mod tests { ... } }` so
//! each test's path becomes `puntested::tests::<name>` — matching the pdead.rs
//! convention.
//!
//! All tests use in-memory rusqlite + MockJCodemunchOps + MockGitOps so they
//! remain hermetic. PUNTESTED is repo-wide (ignores task_metadata/target_task_id/
//! window), so task_metadata is empty and target_task_id/window/now/
//! producer_branch are all None.

mod puntested {

use reify_audit::{
    AuditContext, EvidenceRef, MockGitOps, MockJCodemunchOps, Pattern,
    Severity, UntestedSymbol, puntested,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;

fn untested(
    symbol_id: &str,
    name: &str,
    file: &str,
    reached: bool,
    confidence: f64,
) -> UntestedSymbol {
    UntestedSymbol {
        symbol_id: symbol_id.to_string(),
        name: name.to_string(),
        file: file.to_string(),
        reached,
        confidence,
    }
}

mod tests {
    use super::*;

    /// Core contract: an unreached symbol above the confidence floor yields
    /// one finding with the correct shape.
    ///
    /// Seeds one UntestedSymbol (reached:false, confidence:0.75) and asserts:
    /// - exactly 1 finding
    /// - pattern == PUntested
    /// - severity == Low
    /// - task_id is empty (repo-wide, not task-scoped)
    /// - summary contains name, file, "0.75", and "not reached"
    /// - evidence == vec![EvidenceRef::File { path: sym.file }]
    #[test]
    fn untested_symbol_yields_finding() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();

        jc.set_untested_symbols(vec![untested(
            "reify-core::solver::legacy_relax",
            "legacy_relax",
            "crates/reify-core/src/solver.rs",
            false,
            0.75,
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

        let findings = puntested::check(&ctx);

        assert_eq!(
            findings.len(),
            1,
            "expected exactly 1 finding; got {:?}",
            findings
        );

        let f = &findings[0];
        assert_eq!(f.pattern, Pattern::PUntested, "pattern must be PUntested");
        assert_eq!(f.severity, Severity::Low, "severity must be Low");
        assert!(
            f.task_id.is_empty(),
            "PUNTESTED findings must have empty task_id; got {:?}",
            f.task_id
        );

        assert!(
            f.summary.contains("legacy_relax"),
            "summary must contain symbol name; got: {}",
            f.summary
        );
        assert!(
            f.summary.contains("crates/reify-core/src/solver.rs"),
            "summary must contain file path; got: {}",
            f.summary
        );
        assert!(
            f.summary.contains("0.75"),
            "summary must contain confidence '0.75'; got: {}",
            f.summary
        );
        assert!(
            f.summary.contains("not reached"),
            "summary must contain 'not reached'; got: {}",
            f.summary
        );

        assert_eq!(
            f.evidence,
            vec![EvidenceRef::File { path: "crates/reify-core/src/solver.rs".to_string() }],
            "evidence must be a single EvidenceRef::File pointing at the symbol's file"
        );
    }

    /// A reached symbol (reached:true) must be suppressed even if above the
    /// confidence floor.
    #[test]
    fn reached_symbol_is_suppressed() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();

        jc.set_untested_symbols(vec![untested(
            "id",
            "reached_fn",
            "crates/reify-x/src/a.rs",
            true,  // reached → must be suppressed
            0.9,
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

        let findings = puntested::check(&ctx);
        assert!(
            findings.is_empty(),
            "reached symbol must yield no findings; got {:?}",
            findings
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

        let findings = puntested::check(&ctx);
        assert!(
            findings.is_empty(),
            "empty mock must yield no findings; got {:?}",
            findings
        );
    }

    /// Confidence-floor bracket: a symbol at 0.49 is excluded by the seam's
    /// `>= 0.5` filter; a symbol at 0.51 survives. Pins that check() passes
    /// the 0.5 floor to get_untested_symbols.
    ///
    /// Both symbols are reached:false so the detector would emit a finding for
    /// any symbol the seam returns; the bracket isolates the seam's threshold.
    #[test]
    fn confidence_floor_excludes_below_0_5() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let git = MockGitOps::new();
        let mut jc = MockJCodemunchOps::new();

        jc.set_untested_symbols(vec![
            untested("b", "below_fn", "crates/reify-x/src/b.rs", false, 0.49),
            untested("a", "above_fn", "crates/reify-x/src/c.rs", false, 0.51),
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

        let findings = puntested::check(&ctx);

        assert_eq!(
            findings.len(),
            1,
            "expected exactly 1 finding (0.49 dropped by seam at 0.5 floor); got {:?}",
            findings
        );
        assert!(
            findings[0].summary.contains("above_fn"),
            "surviving finding must name the above-floor symbol; got: {}",
            findings[0].summary
        );
    }

    /// Scope-excludes: stdlib prefix and test paths are suppressed.
    ///
    /// Seeds four above-threshold (confidence 0.9) reached:false UntestedSymbols:
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

        jc.set_untested_symbols(vec![
            untested("s1", "stdlib_sym",   "crates/reify-stdlib/src/prelude.rs", false, 0.9),
            untested("s2", "test_dir_sym", "crates/reify-x/tests/it.rs",         false, 0.9),
            untested("s3", "test_src_sym", "crates/reify-x/src/foo_test.rs",     false, 0.9),
            untested("s4", "live_sym",     "crates/reify-x/src/live.rs",         false, 0.9),
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

        let findings = puntested::check(&ctx);

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

} // mod puntested
