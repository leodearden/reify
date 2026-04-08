//! Pragma compilation tests.
//!
//! Tests for compiling `#name` and `#name(args)` pragmas at module and block level.

/// Helper: parse and compile source, return compiled module (no prelude).
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("pragma_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: parse and compile source using the full stdlib prelude.
fn compile_module_with_stdlib(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("pragma_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile_with_stdlib(&parsed)
}

/// Helper: return only error-severity diagnostics (ignoring warnings).
fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect()
}

/// Helper: return only warning-severity diagnostics.
fn warnings_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Warning)
        .collect()
}

/// Helper: filter warnings whose message contains the given substring.
fn pragma_warnings<'a>(
    module: &'a reify_compiler::CompiledModule,
    substr: &str,
) -> Vec<&'a reify_types::Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| d.message.contains(substr))
        .collect()
}

// ── Step 1: module-level unknown pragma warnings ─────────────────────────────

/// Unknown module-level pragma `#optimize` should emit an "unknown pragma" warning.
#[test]
fn unknown_module_pragma_emits_warning() {
    let module = compile_module("#optimize\nstructure S { param x : Real }");
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        !warns.is_empty(),
        "expected an 'unknown pragma' warning for #optimize, got none; all warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns.iter().any(|d| d.message.contains("optimize")),
        "warning should mention 'optimize', got: {:?}",
        warns
    );
}

/// Known module-level pragma `#precision` should NOT emit an unknown-pragma warning.
#[test]
fn known_module_pragma_no_warning() {
    let module = compile_module("#precision(value=64)\nstructure S { param x : Real }");
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        warns.is_empty(),
        "expected no 'unknown pragma' warning for #precision, got: {:?}",
        warns
    );
}
