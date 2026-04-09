//! Tests for the `@test` marker: `is_test` field on `TopologyTemplate`,
//! `is_test()` method on `ConstraintDef`, `CompiledModule` filter helpers.
//!
//! Task 267: @test compiler support.

// ── Helpers ──────────────────────────────────────────────────────────────────

fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_marker_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect()
}

fn warnings_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Warning)
        .collect()
}

fn annotation_warnings<'a>(
    module: &'a reify_compiler::CompiledModule,
    substr: &str,
) -> Vec<&'a reify_types::Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| d.message.contains(substr))
        .collect()
}

// ── Step 1: is_test field on TopologyTemplate ─────────────────────────────────

#[test]
fn template_marked_is_test_when_test_annotation_present() {
    let module = compile_module("@test structure S { param x : Real }");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    assert!(
        module.templates[0].is_test,
        "expected is_test == true for @test-annotated structure"
    );
}
