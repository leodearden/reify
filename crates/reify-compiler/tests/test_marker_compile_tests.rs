//! Tests for the `@test` marker: `is_test` field on `TopologyTemplate`,
//! `is_test()` method on `ConstraintDef`, `CompiledModule` filter helpers.
//!
//! Task 267: @test compiler support.

use reify_compiler::find_template;
use reify_test_support::{compile_source, errors_only, warnings_only};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn annotation_warnings<'a>(
    module: &'a reify_compiler::CompiledModule,
    substr: &str,
) -> Vec<&'a reify_types::Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| d.message.contains(substr))
        .collect()
}

fn parse_first_constraint_def(source: &str) -> reify_syntax::ConstraintDef {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_marker_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    parsed
        .declarations
        .into_iter()
        .find_map(|d| {
            if let reify_syntax::Declaration::Constraint(c) = d {
                Some(c)
            } else {
                None
            }
        })
        .expect("expected a ConstraintDef")
}

// ── Step 1: is_test field on TopologyTemplate ─────────────────────────────────

#[test]
fn template_marked_is_test_when_test_annotation_present() {
    let module = compile_source("@test structure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    assert!(
        module.templates[0].is_test(),
        "expected is_test() == true for @test-annotated structure"
    );
}

// ── Step 3: is_test false without annotation; occurrence also marked ──────────

#[test]
fn template_not_marked_is_test_when_no_annotation() {
    let module = compile_source("structure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    assert!(
        !module.templates[0].is_test(),
        "expected is_test() == false for unannotated structure"
    );
}

#[test]
fn occurrence_marked_is_test_when_test_annotation_present() {
    let module = compile_source("@test occurrence Heat { param temp : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    assert_eq!(
        module.templates[0].entity_kind,
        reify_compiler::EntityKind::Occurrence,
        "expected Occurrence entity_kind"
    );
    assert!(
        module.templates[0].is_test(),
        "expected is_test() == true for @test-annotated occurrence"
    );
}

// ── Step 5: ConstraintDef::is_test() helper ──────────────────────────────────

#[test]
fn constraint_def_is_test_returns_true_when_test_annotation() {
    let def =
        parse_first_constraint_def("@test constraint def MinWall { param x : Length\n x > 0 }");
    assert!(
        def.is_test(),
        "expected is_test() == true for @test constraint def"
    );
}

#[test]
fn constraint_def_is_test_returns_false_without_annotation() {
    let def = parse_first_constraint_def("constraint def MinWall { param x : Length\n x > 0 }");
    assert!(
        !def.is_test(),
        "expected is_test() == false for unannotated constraint def"
    );
}

// ── Step 7: validate_annotations called for constraint defs ──────────────────

#[test]
fn unknown_annotation_on_constraint_def_emits_warning() {
    let module = compile_source("@unknownfoo constraint def C { param x : Length\n x > 0 }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let warns = annotation_warnings(&module, "unknown annotation");
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

// ── Step 7b: validate_pragmas called for constraint defs ─────────────────────

#[test]
fn unknown_pragma_on_constraint_def_emits_warning() {
    let module = compile_source("constraint def C { #unknownfoo\n param x : Length\n x > 0 }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let warns = annotation_warnings(&module, "unknown pragma");
    assert!(
        !warns.is_empty(),
        "expected a warning about unknown pragma on constraint def, got none; all diagnostics: {:?}",
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
    let module = compile_source("@test constraint def C { param x : Length\n x > 0 }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
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
    let module =
        compile_source("@test field def f : Point3 -> Real { source = analytical { |p| 0.0 } }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
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
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let test_tmpls: Vec<_> = module.test_templates().collect();
    assert_eq!(
        test_tmpls.len(),
        2,
        "expected 2 test templates, got {:?}",
        test_tmpls.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
    let test_names: std::collections::HashSet<&str> =
        test_tmpls.iter().map(|t| t.name.as_str()).collect();
    assert!(test_names.contains("A"), "expected A in test_templates");
    assert!(test_names.contains("H"), "expected H in test_templates");

    let non_test_tmpls: Vec<_> = module.non_test_templates().collect();
    assert_eq!(non_test_tmpls.len(), 1, "expected 1 non-test template");
    assert_eq!(non_test_tmpls[0].name, "B");
}

// ── Filter helpers return iterators (not Vec) ──────────────────────────────

#[test]
fn filter_helpers_return_iterators() {
    let source = r#"
        @test structure A { param x : Real }
        structure B { param y : Real }
        @test constraint def TC { param x : Length
            x > 0
        }
        constraint def NC { param y : Length
            y > 0
        }
        @test fn tested() -> Real { 1.0 }
        fn normal() -> Real { 2.0 }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    // These calls use iterator chaining (.map, .count, .any) directly on the
    // return value — this only compiles if the return type is an iterator,
    // not Vec (Vec doesn't have .map() or .count()).
    let test_count = module.test_templates().count();
    assert_eq!(test_count, 1);

    let has_b = module.non_test_templates().any(|t| t.name == "B");
    assert!(has_b);

    let test_cd_names: Vec<&str> = module
        .test_constraint_defs()
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(test_cd_names, vec!["TC"]);

    let non_test_cd_count = module.non_test_constraint_defs().count();
    assert_eq!(non_test_cd_count, 1);

    // test_functions / non_test_functions return iterators too
    let test_fn_names: Vec<&str> = module.test_functions().map(|f| f.name.as_str()).collect();
    assert_eq!(test_fn_names, vec!["tested"]);

    let has_normal = module.non_test_functions().any(|f| f.name == "normal");
    assert!(has_normal);
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
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let test_defs: Vec<_> = module.test_constraint_defs().collect();
    assert_eq!(test_defs.len(), 1, "expected 1 test constraint def");
    assert_eq!(test_defs[0].name, "TestC");

    let non_test_defs: Vec<_> = module.non_test_constraint_defs().collect();
    assert_eq!(non_test_defs.len(), 1, "expected 1 non-test constraint def");
    assert_eq!(non_test_defs[0].name, "NormalC");
}

// ── Step 16: multiple annotations with @test — both preserved, is_test still true ─

#[test]
fn multiple_annotations_with_test_marks_template() {
    let module = compile_source(r#"@test @deprecated("old") structure S { param x : Real }"#);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let template = &module.templates[0];

    assert!(template.is_test(), "expected is_test() == true");
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
        reify_types::AnnotationArg::positional(reify_types::AnnotationArgValue::String("old".into()))
    );
}

#[test]
fn multiple_annotations_with_test_marks_constraint_def() {
    let def = parse_first_constraint_def(
        "@test @deprecated(\"old\") constraint def C { param x : Length\n x > 0 }",
    );
    assert!(def.is_test(), "expected is_test() == true");
    assert_eq!(
        def.annotations.len(),
        2,
        "expected both annotations preserved, got {:?}",
        def.annotations.iter().map(|a| &a.name).collect::<Vec<_>>()
    );
    assert_eq!(def.annotations[0].name, "test");
    assert_eq!(def.annotations[1].name, "deprecated");
    assert_eq!(def.annotations[1].args.len(), 1);
    assert!(
        matches!(&def.annotations[1].args[0].kind, reify_syntax::ExprKind::StringLiteral(s) if s == "old"),
        "expected StringLiteral(\"old\") arg on @deprecated, got: {:?}",
        def.annotations[1].args[0].kind
    );
}

// ── TopologyTemplate::is_test() method encapsulation ─────────────────────────

#[test]
fn topology_template_is_test_method_matches_annotation() {
    let source = r#"
        @test structure TestS { param x : Real }
        structure NormalS { param y : Real }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let test_tmpl = find_template(&module.templates, "TestS").unwrap();
    let normal_tmpl = find_template(&module.templates, "NormalS").unwrap();

    assert!(
        test_tmpl.is_test(),
        "expected is_test() == true for @test structure"
    );
    assert!(
        !normal_tmpl.is_test(),
        "expected is_test() == false for normal structure"
    );
}

// ── CompiledFunction::is_test() ─────────────────────────────────────────────

#[test]
fn compiled_function_is_test_returns_correct_values() {
    let source = r#"
        @test fn tested() -> Real { 1.0 }
        fn normal() -> Real { 2.0 }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    let tested_fn = module
        .functions
        .iter()
        .find(|f| f.name == "tested")
        .unwrap();
    let normal_fn = module
        .functions
        .iter()
        .find(|f| f.name == "normal")
        .unwrap();

    assert!(
        tested_fn.is_test(),
        "expected is_test() == true for @test fn"
    );
    assert!(
        !normal_fn.is_test(),
        "expected is_test() == false for normal fn"
    );
}

// ── CompiledModule::test_functions() / non_test_functions() ──────────────────

#[test]
fn compiled_module_function_filter_helpers() {
    let source = r#"
        @test fn tested() -> Real { 1.0 }
        fn normal() -> Real { 2.0 }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );

    assert_eq!(module.test_functions().count(), 1);
    assert!(module.test_functions().any(|f| f.name == "tested"));

    assert_eq!(module.non_test_functions().count(), 1);
    assert!(module.non_test_functions().any(|f| f.name == "normal"));

    // Edge case: module with no functions at all
    let no_fns = compile_source("structure A { param x : Real }");
    assert_eq!(no_fns.test_functions().count(), 0);
    assert_eq!(no_fns.non_test_functions().count(), 0);
}
