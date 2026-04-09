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
