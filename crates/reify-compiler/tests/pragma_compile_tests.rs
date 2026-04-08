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

// ── Step 3: CompiledModule.pragmas stores all module-level pragmas ────────────

/// Module-level pragmas are stored on CompiledModule.pragmas with correct names/args.
#[test]
fn module_pragmas_stored_on_compiled_module() {
    let module = compile_module(
        "#precision(value=64)\n#version(1)\nstructure S { param x : Real }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    // Should have stored both pragmas
    assert_eq!(
        module.pragmas.len(),
        2,
        "expected 2 pragmas, got {}: {:?}",
        module.pragmas.len(),
        module.pragmas
    );

    let precision = module.pragmas.iter().find(|p| p.name == "precision");
    assert!(precision.is_some(), "#precision pragma not found in module.pragmas");
    let precision = precision.unwrap();
    assert_eq!(precision.args.len(), 1, "expected 1 arg on #precision");
    match &precision.args[0] {
        reify_syntax::PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "value");
            assert_eq!(value, &reify_syntax::PragmaValue::Number(64.0));
        }
        other => panic!("expected KeyValue arg on #precision, got: {:?}", other),
    }

    let version = module.pragmas.iter().find(|p| p.name == "version");
    assert!(version.is_some(), "#version pragma not found in module.pragmas");
    let version = version.unwrap();
    assert_eq!(version.args.len(), 1, "expected 1 arg on #version");
    match &version.args[0] {
        reify_syntax::PragmaArg::Bare(reify_syntax::PragmaValue::Number(n)) => {
            assert_eq!(n, &1.0_f64, "expected version 1, got {n}");
        }
        other => panic!("expected Bare(Number) arg on #version, got: {:?}", other),
    }
}

// ── Step 5: #no_prelude suppresses stdlib ─────────────────────────────────────

/// With #no_prelude and no stdlib-specific names, compilation should succeed.
#[test]
fn no_prelude_simple_structure_compiles_clean() {
    let module = compile_module_with_stdlib("#no_prelude\nstructure S { param x : Real = 1.0 }");
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for simple #no_prelude structure, got: {:?}",
        errs
    );
}

/// With #no_prelude, stdlib units like `mm` should NOT be resolved — expect errors.
/// (Currently fails because #no_prelude has no effect.)
#[test]
fn no_prelude_suppresses_stdlib_units() {
    let module = compile_module_with_stdlib(
        "#no_prelude\nstructure S { param x : Length = 10mm }",
    );
    let errs = errors_only(&module);
    assert!(
        !errs.is_empty(),
        "expected errors when using stdlib unit `mm` with #no_prelude, but got none"
    );
}

/// Without #no_prelude, stdlib units like `mm` should resolve cleanly.
#[test]
fn without_no_prelude_stdlib_units_resolve() {
    let module = compile_module_with_stdlib("structure S { param x : Length = 10mm }");
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors when using stdlib unit `mm` without #no_prelude, got: {:?}",
        errs
    );
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
