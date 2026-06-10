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

// ─── Warning count equals declaration count ────────────────────────────────────

/// Two top-level `default` declarations each produce their own W_DEFAULT_NOT_WIRED
/// warning — the per-declaration emission arm fires once per declaration and does
/// not collapse or deduplicate.
#[test]
fn two_top_level_defaults_emit_two_warnings() {
    let source = "default Material = steel\ndefault Fluid = water";
    let module = compile_source_with_stdlib(source);

    let warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("W_DEFAULT_NOT_WIRED"))
        .collect();

    assert_eq!(
        warnings.len(),
        2,
        "expected exactly 2 W_DEFAULT_NOT_WIRED warnings for 2 top-level defaults; got: {:?}",
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
