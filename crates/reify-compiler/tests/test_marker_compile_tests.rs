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

// ── Step 3: is_test false without annotation; occurrence also marked ──────────

#[test]
fn template_not_marked_is_test_when_no_annotation() {
    let module = compile_module("structure S { param x : Real }");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    assert!(
        !module.templates[0].is_test,
        "expected is_test == false for unannotated structure"
    );
}

#[test]
fn occurrence_marked_is_test_when_test_annotation_present() {
    let module = compile_module("@test occurrence Heat { param temp : Real }");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    assert_eq!(
        module.templates[0].entity_kind,
        reify_compiler::EntityKind::Occurrence,
        "expected Occurrence entity_kind"
    );
    assert!(
        module.templates[0].is_test,
        "expected is_test == true for @test-annotated occurrence"
    );
}

// ── Step 5: ConstraintDef::is_test() helper ──────────────────────────────────

#[test]
fn constraint_def_is_test_returns_true_when_test_annotation() {
    let source = "@test constraint def MinWall { param x : Length\n x > 0 }";
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_marker_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let def = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_syntax::Declaration::Constraint(c) = d {
                Some(c)
            } else {
                None
            }
        })
        .expect("expected a ConstraintDef");
    assert!(def.is_test(), "expected is_test() == true for @test constraint def");
}

#[test]
fn constraint_def_is_test_returns_false_without_annotation() {
    let source = "constraint def MinWall { param x : Length\n x > 0 }";
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_marker_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let def = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_syntax::Declaration::Constraint(c) = d {
                Some(c)
            } else {
                None
            }
        })
        .expect("expected a ConstraintDef");
    assert!(!def.is_test(), "expected is_test() == false for unannotated constraint def");
}

// ── Step 7: validate_annotations called for constraint defs ──────────────────

#[test]
fn unknown_annotation_on_constraint_def_emits_warning() {
    let module = compile_module(
        "@unknownfoo constraint def C { param x : Length\n x > 0 }",
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let warns = annotation_warnings(&module, "unknown");
    assert!(
        !warns.is_empty(),
        "expected a warning about unknown annotation on constraint def, got none; all diagnostics: {:?}",
        module.diagnostics
    );
    assert!(
        warns.iter().any(|d| d.message.contains("unknownfoo")),
        "expected warning to mention 'unknownfoo', got: {:?}",
        warns
    );
}

// ── Step 9: @test on constraint_def must NOT trigger 'invalid context' warning ─

#[test]
fn test_annotation_on_constraint_def_emits_no_invalid_context_warning() {
    let module = compile_module("@test constraint def C { param x : Length\n x > 0 }");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let bad_warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("@test is not valid"))
        .collect();
    assert!(
        bad_warns.is_empty(),
        "expected no '@test is not valid' warning on constraint def, got: {:?}",
        bad_warns
    );
}

// ── Step 11: regression guard - @test on field still warns ───────────────────

#[test]
fn test_annotation_on_field_still_warns_invalid_context() {
    let module = compile_module(
        "@test field def f : Point3 -> Real { source = analytical { |p| 0.0 } }",
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let bad_warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("@test is not valid"))
        .collect();
    assert!(
        !bad_warns.is_empty(),
        "expected '@test is not valid' warning on field, got none"
    );
    assert!(
        bad_warns.iter().any(|d| d.message.contains("field")),
        "expected warning to mention 'field', got: {:?}",
        bad_warns
    );
}

// ── Step 12: CompiledModule::test_templates() / non_test_templates() ──────────

#[test]
fn compiled_module_test_templates_returns_only_marked() {
    let source = r#"
        @test structure A { param x : Real }
        structure B { param y : Real }
        @test occurrence H { param z : Real }
    "#;
    let module = compile_module(source);
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));

    let test_tmpls = module.test_templates();
    assert_eq!(test_tmpls.len(), 2, "expected 2 test templates, got {:?}", test_tmpls.iter().map(|t| &t.name).collect::<Vec<_>>());
    let test_names: std::collections::HashSet<&str> =
        test_tmpls.iter().map(|t| t.name.as_str()).collect();
    assert!(test_names.contains("A"), "expected A in test_templates");
    assert!(test_names.contains("H"), "expected H in test_templates");

    let non_test_tmpls = module.non_test_templates();
    assert_eq!(non_test_tmpls.len(), 1, "expected 1 non-test template");
    assert_eq!(non_test_tmpls[0].name, "B");
}

// ── Steps 14-15: CompiledModule::test_constraint_defs() / non_test_constraint_defs() ──

#[test]
fn compiled_module_test_constraint_defs_returns_only_marked() {
    let source = r#"
        @test constraint def TestC { param x : Length
            x > 0
        }
        constraint def NormalC { param y : Length
            y > 0
        }
    "#;
    let module = compile_module(source);
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));

    let test_defs = module.test_constraint_defs();
    assert_eq!(test_defs.len(), 1, "expected 1 test constraint def");
    assert_eq!(test_defs[0].name, "TestC");

    let non_test_defs = module.non_test_constraint_defs();
    assert_eq!(non_test_defs.len(), 1, "expected 1 non-test constraint def");
    assert_eq!(non_test_defs[0].name, "NormalC");
}

// ── Step 16: multiple annotations with @test — both preserved, is_test still true ─

#[test]
fn multiple_annotations_with_test_marks_template() {
    let module = compile_module(
        r#"@test @deprecated("old") structure S { param x : Real }"#,
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let template = &module.templates[0];

    assert!(template.is_test, "expected is_test == true");
    assert_eq!(
        template.annotations.len(),
        2,
        "expected both annotations preserved, got {:?}",
        template.annotations
    );
    assert_eq!(template.annotations[0].name, "test");
    assert_eq!(template.annotations[1].name, "deprecated");
    assert_eq!(template.annotations[1].args.len(), 1);
    assert_eq!(
        template.annotations[1].args[0],
        reify_types::AnnotationArg::String("old".into())
    );
}
