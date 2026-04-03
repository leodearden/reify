//! Compiler tests for constraint instantiation.
//!
//! Tests for `constraint ConstraintName(arg: expr, ...)` inside structure bodies.
//! Validates that the compiler resolves the constraint def, binds args to params,
//! substitutes param references in predicate expressions, and injects resulting
//! constraints into the parent entity's constraint list.

use reify_compiler::*;
use reify_types::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Parse and compile, returning the template with the given name + diagnostics.
fn compile_template(source: &str, name: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let diags = module.diagnostics.clone();
    let tmpl = module
        .templates
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("expected template '{name}' in compiled module"));
    (tmpl, diags)
}

/// Collect only error diagnostics from a list.
fn error_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ── Step 7: basic single-arg instantiation ───────────────────────────────────

/// Constraint def with single param, structure with one instantiation.
/// After substitution, constraint expr should be `thickness > 2`.
#[test]
fn basic_constraint_inst_compiles() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
}
structure S {
    param thickness: Length
    constraint MinWall(wall: thickness)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 1, "expected exactly 1 constraint");

    let cc = &tmpl.constraints[0];
    // The compiled expr should be BinOp(Gt, ValueRef(S.thickness), Literal(2.0))
    match &cc.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Gt, "expected Gt operator");
            match &left.kind {
                CompiledExprKind::ValueRef(id) => {
                    assert_eq!(id.entity, "S", "left.entity should be S");
                    assert_eq!(id.member, "thickness", "left.member should be thickness");
                }
                other => panic!("expected ValueRef for left, got {:?}", other),
            }
            // Number literal `2` compiles to Int(2).
            match &right.kind {
                CompiledExprKind::Literal(Value::Int(v)) => {
                    assert_eq!(*v, 2, "right should be 2, got {v}");
                }
                CompiledExprKind::Literal(Value::Real(v)) => {
                    assert!((v - 2.0).abs() < 1e-9, "right should be 2.0, got {v}");
                }
                other => panic!("expected Literal(2 or 2.0) for right, got {:?}", other),
            }
        }
        other => panic!("expected BinOp for constraint expr, got {:?}", other),
    }
}

// ── Step 9: multi-predicate constraint def ───────────────────────────────────

/// Constraint def with 3 params and 2 predicates; structure instantiates with literals.
#[test]
fn multi_predicate_constraint_inst() {
    let source = r#"
constraint def Bounded {
    param x: Length
    param lo: Length
    param hi: Length
    x >= lo
    x <= hi
}
structure S {
    param w: Length
    constraint Bounded(x: w, lo: 1, hi: 10)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 2, "expected exactly 2 constraints");

    // First constraint: w >= 1
    match &tmpl.constraints[0].expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Ge, "first constraint should be Ge (>=)");
            assert!(
                matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == "w"),
                "left should be ValueRef(S.w)"
            );
            let right_is_one = match &right.kind {
                CompiledExprKind::Literal(Value::Int(v)) => *v == 1,
                CompiledExprKind::Literal(Value::Real(v)) => (v - 1.0).abs() < 1e-9,
                _ => false,
            };
            assert!(right_is_one, "right should be Literal(1), got {:?}", right.kind);
        }
        other => panic!("expected BinOp for first constraint, got {:?}", other),
    }

    // Second constraint: w <= 10
    match &tmpl.constraints[1].expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Le, "second constraint should be Le (<=)");
            assert!(
                matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == "w"),
                "left should be ValueRef(S.w)"
            );
            let right_is_ten = match &right.kind {
                CompiledExprKind::Literal(Value::Int(v)) => *v == 10,
                CompiledExprKind::Literal(Value::Real(v)) => (v - 10.0).abs() < 1e-9,
                _ => false,
            };
            assert!(right_is_ten, "right should be Literal(10), got {:?}", right.kind);
        }
        other => panic!("expected BinOp for second constraint, got {:?}", other),
    }
}

// ── Step 11: complex expression substitution ─────────────────────────────────

/// Constraint def with complex predicates; validates substitution through BinOp nesting.
#[test]
fn complex_expr_substitution() {
    let source = r#"
constraint def Ratio {
    param a: Length
    param b: Length
    a / b > 0.5
    a / b < 2
}
structure S {
    param width: Length
    param height: Length
    constraint Ratio(a: width, b: height)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 2, "expected 2 constraints");

    // Both constraints should have BinOp with nested BinOp(Div) left-hand side
    for cc in &tmpl.constraints {
        match &cc.expr.kind {
            CompiledExprKind::BinOp { left, right: _, .. } => {
                match &left.kind {
                    CompiledExprKind::BinOp { op, left: inner_left, right: inner_right } => {
                        assert_eq!(*op, BinOp::Div, "inner op should be Div");
                        assert!(
                            matches!(&inner_left.kind, CompiledExprKind::ValueRef(id) if id.member == "width"),
                            "inner left should be ValueRef(width)"
                        );
                        assert!(
                            matches!(&inner_right.kind, CompiledExprKind::ValueRef(id) if id.member == "height"),
                            "inner right should be ValueRef(height)"
                        );
                    }
                    other => panic!("expected BinOp(Div) as left side of constraint, got {:?}", other),
                }
            }
            other => panic!("expected outer BinOp, got {:?}", other),
        }
    }
}

// ── Step 13: unknown constraint def name ─────────────────────────────────────

#[test]
fn unknown_constraint_def_name() {
    let source = r#"
structure S {
    param t: Length
    constraint UnknownDef(x: t)
}
"#;
    let module = compile_module(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for unknown constraint def"
    );
    let found = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        (msg.contains("unknown") || msg.contains("not found"))
            && d.message.contains("UnknownDef")
    });
    assert!(found, "expected error mentioning 'unknown'/'not found' and 'UnknownDef', got: {:?}", errors);
}

// ── Step 15: missing required argument ───────────────────────────────────────

#[test]
fn missing_required_argument() {
    let source = r#"
constraint def TwoParam {
    param a: Length
    param b: Length
    a > b
}
structure S {
    param x: Length
    constraint TwoParam(a: x)
}
"#;
    let module = compile_module(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for missing argument 'b'"
    );
    let found = errors.iter().any(|d| d.message.contains('b') || d.message.to_lowercase().contains("missing"));
    assert!(found, "expected error mentioning missing argument 'b', got: {:?}", errors);
}

// ── Step 17: extra/unknown argument ──────────────────────────────────────────

#[test]
fn unknown_argument_name() {
    let source = r#"
constraint def OneParam {
    param a: Length
    a > 0
}
structure S {
    param x: Length
    constraint OneParam(a: x, b: 5)
}
"#;
    let module = compile_module(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for unknown argument 'b'"
    );
    let found = errors.iter().any(|d| d.message.contains('b') || d.message.to_lowercase().contains("unknown"));
    assert!(found, "expected error mentioning unknown argument 'b', got: {:?}", errors);
}

// ── Step 19: where-clause on constraint instantiation ────────────────────────

#[test]
fn constraint_inst_with_where_clause() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
}
structure S {
    param mode: Bool
    param t: Length
    constraint MinWall(wall: t) where mode
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    // The constraint should NOT appear in top-level constraints
    assert!(
        tmpl.constraints.is_empty(),
        "constraint should not be in top-level constraints when guarded, got: {:?}",
        tmpl.constraints
    );

    // The constraint SHOULD appear in a guarded_group
    assert!(
        !tmpl.guarded_groups.is_empty(),
        "expected at least one guarded_group"
    );
    let total_guarded_constraints: usize = tmpl.guarded_groups.iter().map(|g| g.constraints.len()).sum();
    assert_eq!(
        total_guarded_constraints, 1,
        "expected 1 guarded constraint, found {total_guarded_constraints}"
    );
}

// ── Step 1 (task-198): constraint instantiation labels ───────────────────────

/// Single-predicate constraint def instantiation should produce a CompiledConstraint
/// with label == Some("MinWall[0]").
#[test]
fn constraint_inst_label_single_predicate() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
}
structure S {
    param thickness: Length
    constraint MinWall(wall: thickness)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 1, "expected exactly 1 constraint");

    let cc = &tmpl.constraints[0];
    assert_eq!(
        cc.label,
        Some("MinWall[0]".to_string()),
        "expected label Some(\"MinWall[0]\"), got: {:?}",
        cc.label
    );
}

/// Multi-predicate constraint def instantiation should produce labeled constraints
/// Some("Bounded[0]") and Some("Bounded[1]") respectively.
#[test]
fn constraint_inst_label_multi_predicate() {
    let source = r#"
constraint def Bounded {
    param x: Length
    param lo: Length
    param hi: Length
    x >= lo
    x <= hi
}
structure S {
    param w: Length
    constraint Bounded(x: w, lo: 1, hi: 10)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(tmpl.constraints.len(), 2, "expected exactly 2 constraints");

    assert_eq!(
        tmpl.constraints[0].label,
        Some("Bounded[0]".to_string()),
        "expected first constraint label Some(\"Bounded[0]\"), got: {:?}",
        tmpl.constraints[0].label
    );
    assert_eq!(
        tmpl.constraints[1].label,
        Some("Bounded[1]".to_string()),
        "expected second constraint label Some(\"Bounded[1]\"), got: {:?}",
        tmpl.constraints[1].label
    );
}
