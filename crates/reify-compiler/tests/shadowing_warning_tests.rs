//! Shadowing warning tests (spec §8.5, PRD docs/prds/shadowing-warning.md).
//!
//! The lint walks AST scopes once and emits a Warning diagnostic with
//! [`DiagnosticCode::Shadowing`] when a child-scope declaration uses the same
//! name as a name visible from an enclosing parent scope.

use reify_test_support::{compile_source, warnings_only};
use reify_types::{DiagnosticCode, Severity};

/// Basic lambda-shadows-entity-param case: a lambda parameter `x` declared
/// inside a structure that already declares `param x` MUST emit exactly one
/// `Shadowing` warning. The warning carries two labels — the lambda's `x`
/// site (child) and the entity's `param x` site (original) — with non-empty,
/// distinct spans.
#[test]
fn lambda_param_shadows_entity_param_emits_w_shadow() {
    let source = r#"
structure S {
    param x : Real = 1
    let f = |x| x * 2
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let shadow_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Shadowing))
        .collect();

    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly 1 Shadowing warning, got {}: {:?}",
        shadow_warnings.len(),
        shadow_warnings
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = shadow_warnings[0];
    assert_eq!(warning.severity, Severity::Warning);

    assert_eq!(
        warning.labels.len(),
        2,
        "Shadowing warning must carry two labels (child + original), got: {:?}",
        warning.labels
    );

    let l0 = &warning.labels[0];
    let l1 = &warning.labels[1];
    assert!(
        !l0.span.is_empty(),
        "child-site label span must be non-empty, got: {:?}",
        l0.span
    );
    assert!(
        !l1.span.is_empty(),
        "original-decl label span must be non-empty, got: {:?}",
        l1.span
    );
    assert_ne!(
        l0.span, l1.span,
        "child-site and original-decl spans must be distinct, both = {:?}",
        l0.span
    );
}
