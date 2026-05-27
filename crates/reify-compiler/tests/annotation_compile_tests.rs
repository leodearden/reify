//! Annotation compilation tests.
//!
//! Tests for compiling `@name(args...)` annotations on various declaration types.

use reify_test_support::{compile_source, errors_only, warnings_only};

/// Helper: filter warnings whose message contains the given substring.
fn annotation_warnings<'a>(
    module: &'a reify_compiler::CompiledModule,
    substr: &str,
) -> Vec<&'a reify_core::Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| d.message.contains(substr))
        .collect()
}

// ── Step 3: annotation on structure propagates ──────────────────────────

#[test]
fn annotation_on_structure_propagates() {
    let module = compile_source("@test structure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.templates.len(), 1, "expected 1 template");

    let template = &module.templates[0];
    assert_eq!(
        template.annotations.len(),
        1,
        "expected 1 annotation, got {:?}",
        template.annotations
    );
    assert_eq!(template.annotations[0].name, "test");
    assert!(template.annotations[0].args.is_empty());
}

// ── Step 5: annotation with args on function ────────────────────────────

#[test]
fn annotation_with_args_on_function_propagates() {
    let module =
        compile_source(r#"@deprecated("use new_calc") fn old_calc(x: Real) -> Real { x }"#);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.functions.len(), 1, "expected 1 function");

    let func = &module.functions[0];
    assert_eq!(
        func.annotations.len(),
        1,
        "expected 1 annotation, got {:?}",
        func.annotations
    );
    assert_eq!(func.annotations[0].name, "deprecated");
    assert_eq!(func.annotations[0].args.len(), 1);
    assert_eq!(
        func.annotations[0].args[0],
        reify_ir::AnnotationArg::positional(reify_ir::AnnotationArgValue::String("use new_calc".into()))
    );
}

// ── Step 7: annotation on trait, field, and purpose ─────────────────────

#[test]
fn annotation_on_trait_propagates() {
    let module = compile_source("@deprecated trait Measurable { param width : Length }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.trait_defs.len(), 1, "expected 1 trait");

    let trait_def = &module.trait_defs[0];
    assert_eq!(
        trait_def.annotations.len(),
        1,
        "expected 1 annotation on trait, got {:?}",
        trait_def.annotations
    );
    assert_eq!(trait_def.annotations[0].name, "deprecated");
}

#[test]
fn annotation_on_field_propagates() {
    let module =
        compile_source("field def temp_field : Point3 -> Real { source = analytical { |p| 0.0 } }");
    // Note: @deprecated on field is tested separately; first verify basic field compiles
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let module = compile_source(
        "@deprecated field def temp_field : Point3 -> Real { source = analytical { |p| 0.0 } }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.fields.len(), 1);
    assert_eq!(
        module.fields[0].annotations.len(),
        1,
        "expected 1 annotation on field"
    );
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
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.compiled_purposes.len(), 1, "expected 1 purpose");
    assert_eq!(
        module.compiled_purposes[0].annotations.len(),
        1,
        "expected 1 annotation on purpose, got {:?}",
        module.compiled_purposes[0].annotations
    );
    assert_eq!(
        module.compiled_purposes[0].annotations[0].name,
        "deprecated"
    );
}

// ── Step 9: annotation context validation ───────────────────────────────

#[test]
fn known_annotation_valid_context_no_warning() {
    // @test is valid on structure context — no warnings expected
    let module = compile_source("@test structure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let ann_warns = annotation_warnings(&module, "@test");
    assert!(
        ann_warns.is_empty(),
        "unexpected annotation warnings: {:?}",
        ann_warns
    );
}

#[test]
fn known_annotation_invalid_context_produces_warning() {
    // @test is NOT valid on field context — should produce a warning
    let module =
        compile_source("@test field def f : Point3 -> Real { source = analytical { |p| 0.0 } }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let ann_warns = annotation_warnings(&module, "@test");
    assert!(
        !ann_warns.is_empty(),
        "expected a warning about @test on field, got none"
    );
    assert!(
        ann_warns[0].message.contains("field"),
        "expected warning to mention 'field', got: {}",
        ann_warns[0].message
    );
}

#[test]
fn deprecated_valid_on_any_context() {
    // @deprecated should be valid on any declaration context — no warnings
    let module_fn = compile_source("@deprecated fn f(x: Real) -> Real { x }");
    assert!(errors_only(&module_fn).is_empty());
    let ann_warns_fn = annotation_warnings(&module_fn, "@deprecated");
    assert!(
        ann_warns_fn.is_empty(),
        "unexpected @deprecated warning on fn: {:?}",
        ann_warns_fn
    );

    let module_struct = compile_source("@deprecated structure S { param x : Real }");
    assert!(errors_only(&module_struct).is_empty());
    let ann_warns_struct = annotation_warnings(&module_struct, "@deprecated");
    assert!(
        ann_warns_struct.is_empty(),
        "unexpected @deprecated warning on structure: {:?}",
        ann_warns_struct
    );
}

#[test]
fn unknown_annotation_produces_warning() {
    // @foobar is not a known annotation — should produce a warning mentioning 'unknown'
    let module = compile_source("@foobar structure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let ann_warns = annotation_warnings(&module, "unknown");
    assert!(
        !ann_warns.is_empty(),
        "expected a warning about unknown annotation, got none"
    );
    assert!(
        ann_warns[0].message.contains("foobar"),
        "expected warning to mention 'foobar', got: {}",
        ann_warns[0].message
    );
}

// ── Step 11: comprehensive edge-case tests ──────────────────────────────

#[test]
fn multiple_annotations_all_preserved() {
    let module = compile_source(r#"@test @deprecated("old") structure S { param x : Real }"#);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let template = &module.templates[0];
    assert_eq!(
        template.annotations.len(),
        2,
        "expected 2 annotations, got {:?}",
        template.annotations
    );
    assert_eq!(template.annotations[0].name, "test");
    assert!(template.annotations[0].args.is_empty());
    assert_eq!(template.annotations[1].name, "deprecated");
    assert_eq!(template.annotations[1].args.len(), 1);
    assert_eq!(
        template.annotations[1].args[0],
        reify_ir::AnnotationArg::positional(reify_ir::AnnotationArgValue::String("old".into()))
    );
}

#[test]
fn annotation_arg_types_lowered() {
    // 1.5 (deliberately not a famous mathematical constant to avoid clippy::approx_constant)
    let module = compile_source(
        r#"@config("name", 42, 1.5, true, mechanical) structure S { param x : Real }"#,
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let template = &module.templates[0];
    assert_eq!(template.annotations.len(), 1);
    let args = &template.annotations[0].args;
    assert_eq!(args.len(), 5, "expected 5 args, got {:?}", args);
    assert_eq!(args[0], reify_ir::AnnotationArg::positional(reify_ir::AnnotationArgValue::String("name".into())));
    assert_eq!(args[1], reify_ir::AnnotationArg::positional(reify_ir::AnnotationArgValue::Int(42)));
    assert_eq!(args[2], reify_ir::AnnotationArg::positional(reify_ir::AnnotationArgValue::Real(1.5)));
    assert_eq!(args[3], reify_ir::AnnotationArg::positional(reify_ir::AnnotationArgValue::Bool(true)));
    assert_eq!(
        args[4],
        reify_ir::AnnotationArg::positional(reify_ir::AnnotationArgValue::Ident("mechanical".into()))
    );
}

#[test]
fn annotation_on_occurrence_propagates() {
    let module = compile_source("@test occurrence Heat { param temp : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    let template = &module.templates[0];
    assert_eq!(
        template.entity_kind,
        reify_compiler::EntityKind::Occurrence,
        "expected Occurrence entity_kind"
    );
    assert_eq!(
        template.annotations.len(),
        1,
        "expected 1 annotation on occurrence"
    );
    assert_eq!(template.annotations[0].name, "test");
}

// ── Task 3555 / annotation-args δ: non-literal expression args ──────────────
//
// A non-literal annotation argument (`@shell(linear_taper(1.0))`) must lower to
// `AnnotationArg { name: None, value: AnnotationArgValue::Expr(<call>) }` — preserved
// unevaluated through lowering instead of being warned-about and dropped (the pre-δ
// behaviour). @shell's Phase-1 schema accepts only a numeric-literal thickness, so
// validation still emits its schema-mismatch warning; the test pins BOTH the preserved
// IR shape and the surviving diagnostic.

#[test]
fn shell_non_literal_expr_arg_lowers_to_expr_variant_and_still_warns() {
    let module = compile_source(include_str!("fixtures/annotation_expr_arg.ri"));
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.templates.len(), 1, "expected 1 template (Plate)");

    let template = &module.templates[0];
    assert_eq!(template.name, "Plate");
    assert_eq!(
        template.annotations.len(),
        1,
        "expected 1 @shell annotation, got {:?}",
        template.annotations
    );
    let ann = &template.annotations[0];
    assert_eq!(ann.name, "shell");
    assert_eq!(ann.args.len(), 1, "expected 1 arg, got {:?}", ann.args);

    // IR shape: positional arg whose value is the unevaluated `linear_taper(1.0)` call.
    match &ann.args[0] {
        reify_ir::AnnotationArg {
            name: None,
            value: reify_ir::AnnotationArgValue::Expr(expr),
        } => match &expr.kind {
            reify_ast::ExprKind::FunctionCall { name, args } => {
                assert_eq!(name, "linear_taper", "unexpected call target");
                assert_eq!(args.len(), 1, "expected linear_taper(<1 arg>)");
                assert!(
                    matches!(
                        args[0].kind,
                        reify_ast::ExprKind::NumberLiteral { value, .. } if value == 1.0
                    ),
                    "expected literal 1.0 argument, got {:?}",
                    args[0].kind
                );
            }
            other => panic!("expected FunctionCall linear_taper(..), got {other:?}"),
        },
        other => panic!("expected positional Expr-valued arg, got {other:?}"),
    }

    // Diagnostic preserved: @shell's Phase-1 schema rejects the non-numeric thickness.
    let mismatch = annotation_warnings(&module, "must be a numeric literal");
    assert!(
        !mismatch.is_empty(),
        "expected @shell schema-mismatch warning; all warnings: {:?}",
        warnings_only(&module)
    );
}
