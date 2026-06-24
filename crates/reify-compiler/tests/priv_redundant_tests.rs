//! E_PRIV_REDUNDANT lint tests (task #3978 δ — module-and-visibility-hardening Slice C).
//!
//! `priv` is valid only on `param`, `sub`, and `port` members (it hides them
//! from external access). Applying `priv` to a `let` or `constraint` member is
//! always redundant — those are already inaccessible from outside the structure
//! body — and is rejected as `Severity::Error` with
//! [`DiagnosticCode::PrivRedundant`].
//!
//! ## Positive tests (step-1 RED → step-2 GREEN)
//!
//! - `structure S { priv let x = 5 }` → exactly one `Error`
//!   containing `"E_PRIV_REDUNDANT"`.
//! - `structure S { param t : Real = 1  priv constraint t > 0 }` → exactly
//!   one `Error` containing `"E_PRIV_REDUNDANT"`.
//!
//! ## Negative tests (must NOT emit E_PRIV_REDUNDANT)
//!
//! - Plain `let x = 5` without `priv`.
//! - Plain `constraint` without `priv`.
//! - `priv param p : Real = 0` — valid use of `priv`.
//! - `priv sub a = Inner()` inside an outer structure — valid use of `priv`.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{compile_source, errors_only};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Collect only `PrivRedundant` errors from a diagnostic slice.
fn priv_redundant_errors<'a>(
    errs: &'a [&'a reify_core::Diagnostic],
) -> Vec<&'a reify_core::Diagnostic> {
    errs.iter()
        .copied()
        .filter(|d| d.code == Some(DiagnosticCode::PrivRedundant))
        .collect()
}

// ── positive tests ────────────────────────────────────────────────────────────

/// `priv let x = 5` inside a structure must emit exactly one
/// `Severity::Error` with `DiagnosticCode::PrivRedundant` whose message
/// contains `"E_PRIV_REDUNDANT"`.
///
/// RED until step-2 creates `priv_redundant_lint.rs` and wires it.
#[test]
fn priv_let_emits_e_priv_redundant() {
    let source = r#"
structure S {
    priv let x = 5
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let redundant = priv_redundant_errors(&errors);

    assert_eq!(
        redundant.len(),
        1,
        "expected exactly 1 PrivRedundant error for `priv let`, got {}: {:?}",
        redundant.len(),
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let d = redundant[0];
    assert_eq!(
        d.severity,
        Severity::Error,
        "PrivRedundant diagnostic must be Severity::Error, got {:?}",
        d.severity
    );
    assert!(
        d.message.contains("E_PRIV_REDUNDANT"),
        "PrivRedundant message must contain \"E_PRIV_REDUNDANT\", got: {:?}",
        d.message
    );
    assert!(
        !d.labels.is_empty(),
        "PrivRedundant diagnostic must carry at least one label, got none"
    );
}

/// `priv constraint t > 0` inside a structure must emit exactly one
/// `Severity::Error` with `DiagnosticCode::PrivRedundant` whose message
/// contains `"E_PRIV_REDUNDANT"`.
///
/// RED until step-2 creates `priv_redundant_lint.rs` and wires it.
#[test]
fn priv_constraint_emits_e_priv_redundant() {
    let source = r#"
structure S {
    param t : Real = 1
    priv constraint t > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let redundant = priv_redundant_errors(&errors);

    assert_eq!(
        redundant.len(),
        1,
        "expected exactly 1 PrivRedundant error for `priv constraint`, got {}: {:?}",
        redundant.len(),
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let d = redundant[0];
    assert_eq!(
        d.severity,
        Severity::Error,
        "PrivRedundant diagnostic must be Severity::Error, got {:?}",
        d.severity
    );
    assert!(
        d.message.contains("E_PRIV_REDUNDANT"),
        "PrivRedundant message must contain \"E_PRIV_REDUNDANT\", got: {:?}",
        d.message
    );
    assert!(
        !d.labels.is_empty(),
        "PrivRedundant diagnostic must carry at least one label, got none"
    );
}

// ── negative tests ────────────────────────────────────────────────────────────

/// Plain `let x = 5` (without `priv`) must NOT emit `E_PRIV_REDUNDANT`.
#[test]
fn plain_let_does_not_emit_e_priv_redundant() {
    let source = r#"
structure S {
    let x = 5
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let redundant = priv_redundant_errors(&errors);

    assert_eq!(
        redundant.len(),
        0,
        "plain `let x = 5` must NOT emit PrivRedundant, got: {:?}",
        redundant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Plain `constraint t > 0` (without `priv`) must NOT emit `E_PRIV_REDUNDANT`.
#[test]
fn plain_constraint_does_not_emit_e_priv_redundant() {
    let source = r#"
structure S {
    param t : Real = 1
    constraint t > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let redundant = priv_redundant_errors(&errors);

    assert_eq!(
        redundant.len(),
        0,
        "plain `constraint t > 0` must NOT emit PrivRedundant, got: {:?}",
        redundant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// `priv param p : Real = 0` is a valid use of `priv` and must NOT emit
/// `E_PRIV_REDUNDANT`.  Other errors may exist but not this code.
#[test]
fn priv_param_does_not_emit_e_priv_redundant() {
    let source = r#"
structure S {
    priv param p : Real = 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let redundant = priv_redundant_errors(&errors);

    assert_eq!(
        redundant.len(),
        0,
        "`priv param` must NOT emit PrivRedundant, got: {:?}",
        redundant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// `priv sub a = Inner()` is a valid use of `priv` and must NOT emit
/// `E_PRIV_REDUNDANT`.
#[test]
fn priv_sub_does_not_emit_e_priv_redundant() {
    let source = r#"
structure Inner {}
structure Outer {
    priv sub a = Inner()
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let redundant = priv_redundant_errors(&errors);

    assert_eq!(
        redundant.len(),
        0,
        "`priv sub` must NOT emit PrivRedundant, got: {:?}",
        redundant.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
