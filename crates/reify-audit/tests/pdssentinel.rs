//! Integration tests for the PDSSENTINEL ds-sentinel reintroduction guard
//! (`pdssentinel::check`).
//!
//! ## Step-5: hermetic fixture-tree integration test
//!
//! Drives `check()` against a `tempfile` tempdir as `project_root` with
//! fixture `.rs` files at the scoped paths, a `MockGitOps::set_ls_files`, an
//! in-memory rusqlite, and a `MockJCodemunchOps`. The structural lane reads
//! working-tree content via `std::fs::read_to_string(project_root.join(path))`
//! so the files must exist on disk; only the file *list* is mocked.
//!
//! ## Step-9: zero-on-main guard
//!
//! Runs `pdssentinel::check()` over the REAL repo root (via `RealGitOps`) and
//! asserts the detector returns ZERO findings against the actual
//! `crates/reify-compiler/src/` scoped files. A future un-marked
//! `dimensionless_scalar()`-after-`UnresolvedType`-push reintroduction will
//! flip this RED — that is the anti-regression contract.

mod common;

use reify_audit::{
    AuditContext, EvidenceRef, MockGitOps, MockJCodemunchOps, Pattern, RealGitOps, Severity,
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
    std::fs::write(&full, content).expect("write_file");
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-5: hermetic fixture-tree integration test
// ─────────────────────────────────────────────────────────────────────────────

/// `check()` emits a finding for an in-scope, non-allow-marked offender and
/// suppresses: (a) an allow-marked KEEP site in the same in-scope file,
/// (b) a clean in-scope file with no offender pattern, and (c) an out-of-scope
/// file containing the offender pattern.
///
/// Findings are in deterministic (path, line) order.
#[test]
fn check_emits_in_scope_offender_suppresses_allow_and_out_of_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // In-scope file (entity.rs): one real offender + one allow-marked KEEP.
    let entity_path = "crates/reify-compiler/src/entity.rs";
    write_file(
        root,
        entity_path,
        "\
fn resolve_param_type(name: &str, diagnostics: &mut Vec<Diagnostic>) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(\"unresolved type: {}\", name))
            .with_code(DiagnosticCode::UnresolvedType)
    );
    // offender: no allow marker
    Type::dimensionless_scalar()
}

fn resolve_field_codomain(te: TypeExpr, diagnostics: &mut Vec<Diagnostic>) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(\"function type not allowed: {}\", te))
            .with_code(DiagnosticCode::UnresolvedType)
    );
    Type::dimensionless_scalar() // ds-sentinel:allow intentional KEEP: arrow field codomain
}
",
    );

    // In-scope file (functions.rs): clean — no offender.
    let functions_path = "crates/reify-compiler/src/functions.rs";
    write_file(
        root,
        functions_path,
        "\
fn compile_fn(return_type: Type) -> Type {
    return_type
}
",
    );

    // Out-of-scope file (ice.rs): contains an offender pattern but must NOT be flagged.
    let ice_path = "crates/reify-compiler/src/ice.rs";
    write_file(
        root,
        ice_path,
        "\
fn ice_handler(diagnostics: &mut Vec<Diagnostic>) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(\"ice: {}\", name))
            .with_code(DiagnosticCode::UnresolvedType)
    );
    Type::dimensionless_scalar()
}
",
    );

    // Out-of-scope path: a tests/ file — must NOT be flagged.
    let tests_path = "crates/reify-compiler/tests/some_test.rs";
    write_file(
        root,
        tests_path,
        "\
fn test_helper(diagnostics: &mut Vec<Diagnostic>) -> Type {
    diagnostics.push(
        Diagnostic::error(\"unresolved\".to_string())
            .with_code(DiagnosticCode::UnresolvedType)
    );
    Type::dimensionless_scalar()
}
",
    );

    let mut git = MockGitOps::new();
    git.set_ls_files(vec![
        entity_path.to_string(),
        functions_path.to_string(),
        ice_path.to_string(),
        tests_path.to_string(),
    ]);

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

    let findings = reify_audit::pdssentinel::check(&ctx);

    // Exactly one finding: the in-scope, non-allow-marked offender in entity.rs.
    assert_eq!(
        findings.len(),
        1,
        "expected exactly 1 finding (in-scope offender in entity.rs); \
         got {}: {:?}",
        findings.len(),
        findings
    );

    let f = &findings[0];

    // Pattern and severity.
    assert_eq!(
        f.pattern,
        Pattern::PDsSentinel,
        "finding must carry PDsSentinel pattern; got: {:?}",
        f.pattern
    );
    assert_eq!(
        f.severity,
        Severity::Medium,
        "finding must be Medium severity; got: {:?}",
        f.severity
    );

    // task_id = file path.
    assert_eq!(
        f.task_id, entity_path,
        "finding task_id must be the file path; got: {:?}",
        f.task_id
    );

    // Summary contains "ds-sentinel: line N:".
    assert!(
        f.summary.starts_with("ds-sentinel: line "),
        "finding summary must start with 'ds-sentinel: line '; got: {:?}",
        f.summary
    );

    // Evidence references the file.
    assert!(
        f.evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::File { path } if path == entity_path)),
        "finding evidence must reference the file path {:?}; got: {:?}",
        entity_path,
        f.evidence
    );

    // The allow-marked KEEP site must NOT appear in findings.
    let allow_hit = findings.iter().any(|f| f.summary.contains("allow"));
    assert!(
        !allow_hit,
        "allow-marked KEEP site must be suppressed; findings: {:?}",
        findings
    );
}

/// Scope guard: `check()` returns zero findings when all scoped files are
/// clean (no offender pattern in any of them).
#[test]
fn check_returns_empty_for_clean_scoped_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // One in-scope file that is clean.
    write_file(
        root,
        "crates/reify-compiler/src/traits.rs",
        "fn resolve_type(ty: Type) -> Type { ty }\n",
    );

    let mut git = MockGitOps::new();
    git.set_ls_files(vec!["crates/reify-compiler/src/traits.rs".to_string()]);

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

    let findings = reify_audit::pdssentinel::check(&ctx);
    assert!(
        findings.is_empty(),
        "expected zero findings for clean scoped files; got: {:?}",
        findings
    );
}

/// Determinism guard: findings from multiple in-scope files with offenders are
/// returned in (path, line) order.
#[test]
fn check_findings_are_in_deterministic_path_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Two in-scope files each with one offender. entity.rs sorts before traits.rs.
    let entity_offender = "\
diagnostics.push(
    Diagnostic::error(format!(\"unresolved: {}\", name))
        .with_code(DiagnosticCode::UnresolvedType)
);
Type::dimensionless_scalar()
";
    write_file(root, "crates/reify-compiler/src/entity.rs", entity_offender);
    write_file(root, "crates/reify-compiler/src/traits.rs", entity_offender);

    // Register in reverse-alphabetical order to confirm sorting is by path not insertion.
    let mut git = MockGitOps::new();
    git.set_ls_files(vec![
        "crates/reify-compiler/src/traits.rs".to_string(),
        "crates/reify-compiler/src/entity.rs".to_string(),
    ]);

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

    let findings = reify_audit::pdssentinel::check(&ctx);

    assert_eq!(
        findings.len(),
        2,
        "expected 2 findings (one per file); got: {:?}",
        findings
    );
    // First finding must be from entity.rs (alphabetically earlier).
    assert_eq!(
        findings[0].task_id,
        "crates/reify-compiler/src/entity.rs",
        "first finding must be from entity.rs (sorted first); findings: {:?}",
        findings
    );
    assert_eq!(
        findings[1].task_id,
        "crates/reify-compiler/src/traits.rs",
        "second finding must be from traits.rs (sorted second); findings: {:?}",
        findings
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-9: zero-on-main guard (real repo)
// ─────────────────────────────────────────────────────────────────────────────

/// ZERO-ON-MAIN guard: the detector must return zero findings against the
/// actual `crates/reify-compiler/src/` scoped files on main.
///
/// This is the user-observable backward-boundary signal: a future un-marked
/// `dimensionless_scalar()`-after-`UnresolvedType`-push reintroduction in any
/// of the scoped files will flip this test RED.
///
/// Legitimate KEEP sites (e.g. the `functions.rs` arrow/function field
/// domain/codomain arms — PRD §3 KEEP / esc-4646-3) must carry a
/// `// ds-sentinel:allow <rationale>` marker to be suppressed.
///
/// References: `docs/prds/dimensionless-scalar-sentinel-stampout.md` §8/§10;
/// tests/real_git_ops.rs (pattern for a real-repo audit test).
#[test]
fn zero_on_main_scoped_files_have_no_unmarked_offenders() {
    // Resolve the repo root from CARGO_MANIFEST_DIR (crates/reify-audit) → ../../
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by cargo test");
    let repo_root = std::path::Path::new(&manifest_dir)
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root");

    let git = RealGitOps::new(&repo_root);
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: repo_root.clone(),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    let findings = reify_audit::pdssentinel::check(&ctx);

    // Guard against vacuous pass: if ls_files() returned empty (e.g. running
    // from an exported/vendored tree without a git index), the scoped-file
    // loop in check() never executes and the assertion below would pass
    // trivially without actually scanning any files.  Fail loudly instead.
    let all_files = ctx.git.ls_files();
    assert!(
        !all_files.is_empty(),
        "ls_files() returned an empty file list — the test is likely running \
         outside a git work-tree (e.g. a vendored/exported tarball). \
         The zero-on-main contract cannot be verified; fail rather than pass \
         vacuously."
    );
    // The four exact-scope files must be present in the index.  If any are
    // missing the guard above would have caught an empty list, but these
    // four must be individually present to ensure the detector is scanning
    // the right files.
    let required_scope_files = [
        "crates/reify-compiler/src/entity.rs",
        "crates/reify-compiler/src/functions.rs",
        "crates/reify-compiler/src/traits.rs",
        "crates/reify-compiler/src/expr.rs",
    ];
    for required in &required_scope_files {
        assert!(
            all_files.iter().any(|f| f == required),
            "ls_files() is missing required scope file '{}'. \
             The git index may be incomplete or the file was deleted. \
             The zero-on-main contract cannot be verified for this file.",
            required
        );
    }

    assert!(
        findings.is_empty(),
        "expected zero PDSSENTINEL findings on main — all legitimate \
         dimensionless_scalar() KEEP sites must carry a \
         `// ds-sentinel:allow <rationale>` marker.\n\
         Flagged sites (add allow markers or convert to Type::Error):\n{:#?}",
        findings
    );
}
