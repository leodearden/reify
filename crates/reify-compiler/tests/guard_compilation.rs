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

/// Block guard: `where active { param x .. param y .. constraint x > 2mm }`
/// should compile into one guarded group with 2 member value cells and 1 constraint.
#[test]
fn compile_block_guard() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        param x : Scalar = 5mm
        param y : Scalar = 10mm
        constraint x > 2mm
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // active should be in top-level value_cells
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "active"),
        "unguarded param active should be in top-level value_cells"
    );

    // x, y should NOT be in top-level value_cells
    assert!(
        !template.value_cells.iter().any(|vc| vc.id.member == "x"),
        "guarded param x should not be in top-level value_cells"
    );
    assert!(
        !template.value_cells.iter().any(|vc| vc.id.member == "y"),
        "guarded param y should not be in top-level value_cells"
    );

    // Should have 1 guarded group from the block guard
    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group = &template.guarded_groups[0];

    // 2 members (x, y)
    assert_eq!(group.members.len(), 2, "expected 2 members in guarded group");
    let member_names: Vec<_> = group.members.iter().map(|m| m.id.member.as_str()).collect();
    assert!(member_names.contains(&"x"), "expected member x");
    assert!(member_names.contains(&"y"), "expected member y");

    // 1 constraint
    assert_eq!(group.constraints.len(), 1, "expected 1 constraint in guarded group");

    // No top-level constraints (all guarded)
    assert!(template.constraints.is_empty(), "expected no top-level constraints");

    // Guard value cell in structure_controlling
    assert!(
        template.structure_controlling.contains(&group.guard_value_cell),
        "guard_value_cell should be in structure_controlling"
    );
}
