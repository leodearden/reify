//! Compiler accept-and-ignore diagnostic tests for `default <TypeName> = <expr>`.
//!
//! PRD §8 task-A signal: `reify check` on a file with an ambient-default declaration
//! emits exactly one `Severity::Warning` whose message contains `W_DEFAULT_NOT_WIRED`
//! and zero `Severity::Error` diagnostics.  The declaration is accepted (parsed) and
//! not resolved — it is a grammar-producer placeholder awaiting task-B semantics.
//!
//! These tests are RED while `entities_phase.rs`'s `Declaration::Default` arm is a
//! no-op (step-4). They go GREEN in step-6 when the warning emission is wired up.

use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

// ─── Top-level form ──────────────────────────────────────────────────────────

/// A standalone top-level `default Material = steel` declaration compiles without
/// an Error and emits exactly one W_DEFAULT_NOT_WIRED Warning.
///
/// The value expression `steel` is intentionally left unresolved (it names no
/// declared structure in this module); task-A must NOT try to resolve it.
#[test]
fn top_level_default_emits_not_yet_wired_warning() {
    let source = "default Material = steel";
    let module = compile_source_with_stdlib(source);

    let warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("W_DEFAULT_NOT_WIRED"))
        .collect();

    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one W_DEFAULT_NOT_WIRED warning for top-level default; got: {:?}",
        module.diagnostics
    );

    // The declaration must be accepted: no Error diagnostic should be attributable
    // to the default keyword or the declaration (parse errors would fire earlier).
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for top-level default declaration; got: {:?}",
        errors
    );
}

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
