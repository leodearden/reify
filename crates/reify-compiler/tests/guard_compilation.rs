//! Guard compilation tests.
//!
//! Tests for compiling where-clauses and guarded blocks into
//! CompiledGuardedGroup entries in TopologyTemplate.

use reify_compiler::*;
use reify_test_support::*;
use reify_types::*;

/// Helper: parse source and compile, returning first template.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let template = compiled.templates.into_iter().next().expect("expected 1 template");
    (template, compiled.diagnostics)
}

/// Parse `param x : Scalar = 5mm where active` — the per-declaration where clause
/// should compile into a CompiledGuardedGroup with x as a guarded member.
#[test]
fn compile_param_with_where_clause() {
    let source = r#"
structure S {
    param active : Bool = true
    param x : Scalar = 5mm where active
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics expected
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // x should NOT be in top-level value_cells (it's guarded)
    assert!(
        !template.value_cells.iter().any(|vc| vc.id.member == "x"),
        "guarded param x should not be in top-level value_cells"
    );

    // active should be in top-level value_cells
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "active"),
        "unguarded param active should be in top-level value_cells"
    );

    // Should have 1 guarded group
    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group = &template.guarded_groups[0];

    // Guard value cell should be in structure_controlling
    assert!(
        template.structure_controlling.contains(&group.guard_value_cell),
        "guard_value_cell should be in structure_controlling"
    );

    // Members should contain x
    assert_eq!(group.members.len(), 1, "expected 1 member in guarded group");
    assert_eq!(group.members[0].id.member, "x");

    // No else members
    assert!(group.else_members.is_empty(), "expected no else members");
}
