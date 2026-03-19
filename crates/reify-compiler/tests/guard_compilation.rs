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

/// Nested guards: `where a { where b { param x : Scalar = 1mm } }`
/// should produce 2 guarded groups. The inner guard_expr should be
/// AND(ValueRef(outer_guard), ValueRef(b)).
#[test]
fn compile_nested_guards() {
    let source = r#"
structure S {
    param a : Bool = true
    param b : Bool = true
    where a {
        where b {
            param x : Scalar = 1mm
        }
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // a, b in top-level; x should not be
    assert!(template.value_cells.iter().any(|vc| vc.id.member == "a"));
    assert!(template.value_cells.iter().any(|vc| vc.id.member == "b"));
    assert!(!template.value_cells.iter().any(|vc| vc.id.member == "x"));

    // Should have 2 guarded groups (one per nesting level)
    assert_eq!(template.guarded_groups.len(), 2, "expected 2 guarded groups (outer + inner)");

    // Find the inner group (the one with x as a member)
    let inner = template.guarded_groups.iter()
        .find(|g| g.members.iter().any(|m| m.id.member == "x"))
        .expect("expected inner group with member x");

    // Inner guard_expr should be BinOp::And
    assert!(
        matches!(&inner.guard_expr.kind, CompiledExprKind::BinOp { op: BinOp::And, .. }),
        "inner guard_expr should be AND conjunction, got {:?}", inner.guard_expr.kind
    );
}

/// Else block: `where cond { param a } else { param b }`
/// should have members=[a] and else_members=[b] in the same guarded group.
#[test]
fn compile_else_block() {
    let source = r#"
structure S {
    param cond : Bool = true
    where cond {
        param a : Scalar = 1mm
    } else {
        param b : Scalar = 2mm
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Only 'cond' in top-level value_cells
    assert_eq!(template.value_cells.len(), 1, "expected only 'cond' in top-level");
    assert_eq!(template.value_cells[0].id.member, "cond");

    // 1 guarded group
    assert_eq!(template.guarded_groups.len(), 1);
    let group = &template.guarded_groups[0];

    // members=[a], else_members=[b]
    assert_eq!(group.members.len(), 1);
    assert_eq!(group.members[0].id.member, "a");

    assert_eq!(group.else_members.len(), 1);
    assert_eq!(group.else_members[0].id.member, "b");

    // Same guard_value_cell
    assert!(template.structure_controlling.contains(&group.guard_value_cell));
}

/// Reference safety: unguarded `let y = x` referencing guarded `param x` should
/// produce a diagnostic error about unsafe/unguarded reference.
#[test]
fn reference_safety_unguarded_to_guarded_error() {
    let source = r#"
structure S {
    param active : Bool = true
    param x : Scalar = 5mm where active
    let y = x
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // Should contain a diagnostic about unguarded reference
    let guard_errors: Vec<_> = diagnostics.iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("unguarded") || msg.contains("guarded")
        })
        .collect();

    assert!(
        !guard_errors.is_empty(),
        "expected diagnostic about unguarded reference to guarded cell, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Reference safety: within the same guard block, references are safe.
#[test]
fn reference_safety_same_guard_ok() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        param x : Scalar = 5mm
        let y = x
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // No reference safety errors
    let guard_errors: Vec<_> = diagnostics.iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("unguarded") || msg.contains("guarded")
        })
        .collect();

    assert!(
        guard_errors.is_empty(),
        "should not have reference safety errors for same-guard references: {:?}",
        guard_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
