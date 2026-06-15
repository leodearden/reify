//! Compiler accept-and-ignore diagnostic tests for PURPOSE-NESTED
//! `default <TypeName> = <expr>` declarations.
//!
//! Top-level ambient defaults now carry real task-B semantics (file-scope
//! collection: same-scope duplicate detection, declaration-site type checks,
//! and — later — injection into top-level structures). Those signals live in
//! `ambient_default_injection_tests.rs`; the task-A `W_DEFAULT_NOT_WIRED`
//! placeholder warning has been removed for the top-level form.
//!
//! PURPOSE-nested defaults remain a parsed-but-not-yet-applied placeholder until
//! purpose-scope collection lands (a later step): each `default` directly inside
//! a `purpose` body still emits exactly one `Severity::Warning` whose message
//! contains `W_DEFAULT_NOT_WIRED` and zero `Severity::Error` diagnostics.

use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

// ─── Purpose-nested form ─────────────────────────────────────────────────────

/// A `default Material = steel` nested directly inside a `purpose` body compiles
/// without an Error and emits exactly one W_DEFAULT_NOT_WIRED Warning.
#[test]
fn purpose_nested_default_emits_not_yet_wired_warning() {
    let source = r#"
purpose Exploration() {
    default Material = steel
}
"#;
    let module = compile_source_with_stdlib(source);

    let warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("W_DEFAULT_NOT_WIRED"))
        .collect();

    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one W_DEFAULT_NOT_WIRED warning for purpose-nested default; got: {:?}",
        module.diagnostics
    );

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for purpose-nested default declaration; got: {:?}",
        errors
    );
}

// ─── Warning count equals declaration count ────────────────────────────────────

/// Two purpose-nested `default` declarations each produce their own W_DEFAULT_NOT_WIRED
/// warning — the `for d in &p.defaults` loop in the Purpose arm fires once per entry.
#[test]
fn two_purpose_nested_defaults_emit_two_warnings() {
    let source = r#"
purpose Exploration() {
    default Material = steel
    default Fluid = water
}
"#;
    let module = compile_source_with_stdlib(source);

    let warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("W_DEFAULT_NOT_WIRED"))
        .collect();

    assert_eq!(
        warnings.len(),
        2,
        "expected exactly 2 W_DEFAULT_NOT_WIRED warnings for 2 purpose-nested defaults; got: {:?}",
        module.diagnostics
    );

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics; got: {:?}",
        errors
    );
}
