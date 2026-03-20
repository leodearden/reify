//! Occurrence compilation tests.
//!
//! Tests for compiling occurrence definitions into TopologyTemplates with EntityKind::Occurrence.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning the CompiledModule.
fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("occ_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: parse source and compile, returning first template + diagnostics.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let template = module.templates.into_iter().next().expect("expected 1 template");
    (template, module.diagnostics)
}

// ── step-9: compile basic occurrence ─────────────────────────────────

#[test]
fn compile_occurrence_basic() {
    let source = "occurrence def Welding { param method : Length = 10mm }";
    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert_eq!(template.name, "Welding");
    assert_eq!(template.entity_kind, EntityKind::Occurrence);
    assert!(!template.value_cells.is_empty(), "expected at least 1 value cell");
}
