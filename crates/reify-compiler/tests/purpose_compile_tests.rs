//! Purpose compilation tests.
//!
//! Tests for compiling purpose declarations into CompiledPurpose entries.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning the CompiledModule.
/// Asserts no parse errors and no compile-level Severity::Error diagnostics.
fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    compiled
}

// ── Step 9: basic purpose compilation ───────────────────────────

#[test]
fn compile_basic_purpose() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    param height : Length = 60mm
    constraint width > 0mm
}

purpose mfg_ready(subject : Structure) {
    constraint 80mm > 0mm
}
"#;

    let module = compile_module(source);

    // Should have 1 template (Bracket) and 1 compiled purpose
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    assert_eq!(module.compiled_purposes.len(), 1, "expected 1 compiled purpose");

    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "mfg_ready");
    assert!(!purpose.is_pub);
    assert_eq!(purpose.params.len(), 1);
    assert_eq!(purpose.params[0].name, "subject");
    assert_eq!(purpose.params[0].entity_kind, "Structure");
    assert_eq!(purpose.constraints.len(), 1);
    assert!(purpose.objective.is_none());
}

// ── Step 11: reflective schema query subject.params ───────────────────────────

#[test]
fn compile_purpose_with_reflective_params_query() {
    let source = r#"
structure Widget {
    param width : Length = 80mm
    param height : Length = 60mm
    let area = width * height
    constraint width > 0mm
}

purpose check_params(subject : Widget) {
    constraint 1 > 0
}
"#;

    let module = compile_module(source);

    assert_eq!(module.compiled_purposes.len(), 1, "expected 1 compiled purpose");
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "check_params");
    assert_eq!(purpose.params[0].entity_kind, "Widget");

    // The reflective query subject.params should resolve to the list of
    // param ValueCellIds from the Widget template: ["width", "height"]
    // (not "area" which is a let, not a param).
    assert_eq!(
        purpose.resolved_queries.len(),
        1,
        "expected 1 resolved reflective query"
    );
    let query = &purpose.resolved_queries[0];
    assert_eq!(query.param_name, "subject");
    assert_eq!(query.query_kind, "params");
    assert_eq!(query.resolved_ids.len(), 2);
    // Should contain width and height ValueCellIds
    let id_names: Vec<&str> = query
        .resolved_ids
        .iter()
        .map(|id: &ValueCellId| id.member.as_str())
        .collect();
    assert!(id_names.contains(&"width"), "should contain width");
    assert!(id_names.contains(&"height"), "should contain height");
}

// ── Step 19: compile_module helper should catch compile errors ───────────────

#[test]
#[should_panic(expected = "compile errors")]
fn compile_module_rejects_purpose_with_unknown_identifier() {
    // The compile_module helper should fail when a purpose references
    // an unknown identifier. Without diagnostic checking, this silently passes.
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose broken(subject : Structure) {
    constraint nonexistent_var > 0mm
}
"#;

    let _module = compile_module(source);
}

// ── Step 23: let bindings in purposes should emit error ───────────────

#[test]
#[should_panic(expected = "compile errors")]
fn compile_purpose_rejects_let_bindings() {
    // Let bindings in purpose bodies are not yet supported: the compiled
    // expression is discarded and constraints referencing let-bound names
    // would produce ValueCellIds with no backing eval graph node.
    // The compiler should emit a Severity::Error diagnostic.
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose check(subject : Structure) {
    let half_w = 80mm / 2
    constraint half_w > 10mm
}
"#;

    let _module = compile_module(source);
}

// ── Step 25: unsupported member variants should emit error ───────────────

/// Helper: parse source and compile, returning the CompiledModule without
/// asserting on compile errors. Used to inspect diagnostics directly.
fn compile_module_with_diagnostics(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

#[test]
fn compile_purpose_rejects_guarded_blocks() {
    // The grammar's purpose_member reuses guarded_block, so a where-guarded
    // constraint block parses into MemberDecl::GuardedGroup. The compiler
    // should emit a Severity::Error diagnostic rather than silently dropping it.
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    param height : Length = 60mm
}

purpose check(subject : Structure) {
    where 80mm > 10mm {
        constraint 60mm > 5mm
    }
}
"#;

    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected compile error for guarded block in purpose, but got none"
    );
    let has_guarded_error = errors
        .iter()
        .any(|d| d.message.contains("guarded blocks in purpose bodies are not yet supported"));
    assert!(
        has_guarded_error,
        "expected diagnostic about unsupported guarded blocks, got: {:?}",
        errors
    );
}

#[test]
fn compile_purpose_no_false_positives_from_explicit_arms() {
    // Verify that a valid purpose with only constraints compiles cleanly
    // (no false positives from the explicit error arms added in step 26).
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose ok(subject : Structure) {
    constraint 80mm > 0mm
}
"#;

    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no compile errors for valid purpose, got: {:?}",
        errors
    );
}
