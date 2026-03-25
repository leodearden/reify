//! Annotation compilation tests.
//!
//! Tests for compiling `@name(args...)` annotations on various declaration types.

/// Helper: parse and compile source, return compiled module.
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("annotation_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: return only error-severity diagnostics (ignoring warnings).
fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module.diagnostics.iter().filter(|d| d.severity == reify_types::Severity::Error).collect()
}

// ── Step 3: annotation on structure propagates ──────────────────────────

#[test]
fn annotation_on_structure_propagates() {
    let module = compile_module("@test structure S { param x : Real }");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    assert_eq!(module.templates.len(), 1, "expected 1 template");

    let template = &module.templates[0];
    assert_eq!(template.annotations.len(), 1, "expected 1 annotation, got {:?}", template.annotations);
    assert_eq!(template.annotations[0].name, "test");
    assert!(template.annotations[0].args.is_empty());
}

// ── Step 5: annotation with args on function ────────────────────────────

#[test]
fn annotation_with_args_on_function_propagates() {
    let module = compile_module(
        r#"@deprecated("use new_calc") fn old_calc(x: Real) -> Real { x }"#,
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    assert_eq!(module.functions.len(), 1, "expected 1 function");

    let func = &module.functions[0];
    assert_eq!(func.annotations.len(), 1, "expected 1 annotation, got {:?}", func.annotations);
    assert_eq!(func.annotations[0].name, "deprecated");
    assert_eq!(func.annotations[0].args.len(), 1);
    assert_eq!(
        func.annotations[0].args[0],
        reify_types::AnnotationArg::String("use new_calc".into())
    );
}
