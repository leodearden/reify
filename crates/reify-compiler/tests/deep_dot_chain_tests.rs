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

/// The DeepDotChain warning's message contains the rendered chain text
/// (`a.b.c.d.e`) so that humans reading the diagnostic see the offending
/// chain inline without needing the source span.
#[test]
fn chain_warning_message_contains_full_chain_text() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warning = warnings
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .expect("expected one DeepDotChain warning");

    assert!(
        deep_dot_chain_warning.message.contains("a.b.c.d.e"),
        "expected DeepDotChain warning message to contain `a.b.c.d.e`, got: {:?}",
        deep_dot_chain_warning.message
    );
}

/// The DeepDotChain warning has at least one DiagnosticLabel whose span
/// equals the outermost MemberAccess.span — i.e. starts at byte offset of
/// root identifier `a` and ends after final member `e`.
#[test]
fn chain_warning_has_label_covering_full_chain_span() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warning = warnings
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .expect("expected one DeepDotChain warning");

    assert!(
        !deep_dot_chain_warning.labels.is_empty(),
        "expected at least one label on the DeepDotChain warning, got: {:?}",
        deep_dot_chain_warning
    );

    // Compute the expected chain span by locating "a.b.c.d.e" in the source.
    let needle = "a.b.c.d.e";
    let start = source
        .find(needle)
        .expect("test source must contain `a.b.c.d.e` literally") as u32;
    let end = start + needle.len() as u32;

    let has_full_span_label = deep_dot_chain_warning
        .labels
        .iter()
        .any(|l| l.span.start == start && l.span.end == end);

    assert!(
        has_full_span_label,
        "expected a label whose span covers the full chain (bytes {start}..{end}), \
         got labels: {:?}",
        deep_dot_chain_warning.labels
    );
}
