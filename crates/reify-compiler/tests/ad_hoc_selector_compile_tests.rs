//! Compiler behavior for ad-hoc selector (@) expressions.
//!
//! Verifies that the compiler emits a Diagnostic::error instead of panicking
//! when it encounters an AdHocSelector expression.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning the CompiledModule without
/// asserting on compile errors. Used to inspect diagnostics directly.
fn compile_module_with_diagnostics(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

#[test]
fn compile_ad_hoc_selector_emits_diagnostic() {
    // The compiler does not yet implement ad-hoc selector (@) support.
    // It should emit a Severity::Error diagnostic rather than panicking.
    let source = r#"
structure S {
    let x = port @ face("top")
}
"#;

    // This should NOT panic — it should return with a diagnostic.
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected compile error for ad-hoc selector, but got none"
    );
    let has_selector_error = errors
        .iter()
        .any(|d| d.message.contains("ad-hoc selector (@) is not yet supported"));
    assert!(
        has_selector_error,
        "expected diagnostic about unsupported ad-hoc selector, got: {:?}",
        errors
    );
}
