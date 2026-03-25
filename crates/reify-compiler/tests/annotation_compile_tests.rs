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

// ── Step 7: annotation on trait, field, and purpose ─────────────────────

#[test]
fn annotation_on_trait_propagates() {
    let module = compile_module("@deprecated trait Measurable { param width : Length }");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    assert_eq!(module.trait_defs.len(), 1, "expected 1 trait");

    let trait_def = &module.trait_defs[0];
    assert_eq!(trait_def.annotations.len(), 1, "expected 1 annotation on trait, got {:?}", trait_def.annotations);
    assert_eq!(trait_def.annotations[0].name, "deprecated");
}

#[test]
fn annotation_on_field_propagates() {
    let module = compile_module(
        "field def temp_field : Point3 -> Real = analytical { |p| 0.0 }",
    );
    // Note: @deprecated on field is tested separately; first verify basic field compiles
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));

    let module = compile_module(
        "@deprecated field def temp_field : Point3 -> Real = analytical { |p| 0.0 }",
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    assert_eq!(module.fields.len(), 1);
    assert_eq!(module.fields[0].annotations.len(), 1, "expected 1 annotation on field");
    assert_eq!(module.fields[0].annotations[0].name, "deprecated");
}

#[test]
fn annotation_on_purpose_propagates() {
    let source = r#"
        structure S { param x : Length = 80mm }
        @deprecated purpose P(subject : Structure) {
            constraint 80mm > 0mm
        }
    "#;
    let module = compile_module(source);
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    assert_eq!(module.compiled_purposes.len(), 1, "expected 1 purpose");
    assert_eq!(
        module.compiled_purposes[0].annotations.len(), 1,
        "expected 1 annotation on purpose, got {:?}", module.compiled_purposes[0].annotations
    );
    assert_eq!(module.compiled_purposes[0].annotations[0].name, "deprecated");
}
