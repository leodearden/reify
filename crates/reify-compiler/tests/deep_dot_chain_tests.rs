//! Integration tests for the deep-dot-chain lint (spec §5.7).
//!
//! The lint walks left-to-right `MemberAccess` chains in the parsed AST and
//! emits a Warning diagnostic with [`DiagnosticCode::DeepDotChain`] when the
//! chain length exceeds [`crate::compile_builder::dot_chain_lint::DEEP_DOT_CHAIN_THRESHOLD`]
//! (= 4). `a.b.c.d` (length 4) is at-threshold and does not warn;
//! `a.b.c.d.e` (length 5) warns.
//!
//! These integration tests use the public `compile_source` / `warnings_only`
//! helpers from `reify-test-support`, mirroring the style of
//! `import_warning_tests.rs` and `diagnostic_coverage_checkpoint.rs`.

use reify_test_support::{compile_source, warnings_only};
use reify_types::DiagnosticCode;

/// A chain at exactly the threshold (length 4 — `a.b.c.d`) must not warn.
///
/// This is a regression lock: the gate is `> THRESHOLD`, not `>= THRESHOLD`,
/// so at-threshold chains are explicitly OK per spec §5.7's example.
#[test]
fn chain_at_threshold_does_not_warn() {
    // Source uses a chain of length 4 (root `a` + 3 dot hops `.b.c.d`).
    // The structure body just establishes a scope for the chain to live in;
    // the lint only cares about AST shape, not name resolution.
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    assert!(
        deep_dot_chain_warnings.is_empty(),
        "expected no DeepDotChain warnings for at-threshold chain `a.b.c.d`, \
         got: {:?}",
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// A chain just above threshold (length 5 — `a.b.c.d.e`) emits exactly one
/// Warning whose `code == Some(DiagnosticCode::DeepDotChain)`.
#[test]
fn chain_above_threshold_emits_one_warning_with_deep_dot_chain_code() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    assert_eq!(
        deep_dot_chain_warnings.len(),
        1,
        "expected exactly 1 DeepDotChain warning for above-threshold chain `a.b.c.d.e`, \
         got: {:?}",
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
