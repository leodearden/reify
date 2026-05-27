//! Constraint instantiation parser tests.
//!
//! Tests for `constraint ConstraintName(arg: expr, ...)` member declarations.

use reify_syntax::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_structure_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("constraint_inst_test"),
    );
    let errors = module.errors.clone();
    // Find the first structure declaration and return its members.
    for decl in &module.declarations {
        if let Declaration::Structure(s) = decl {
            return (s.members.clone(), errors);
        }
    }
    (vec![], errors)
}

// ── Step 1: basic single-arg constraint instantiation ────────────

#[test]
fn parse_basic_constraint_inst() {
    let source = "structure S { param t: Length  constraint MinWall(wall: t) }";
    let (members, errors) = parse_structure_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    // Find the ConstraintInst member
    let inst = members
        .iter()
        .find_map(|m| {
            if let MemberDecl::ConstraintInst(ci) = m {
                Some(ci)
            } else {
                None
            }
        })
        .expect("expected a ConstraintInst member");

    assert_eq!(inst.name, "MinWall");
    assert_eq!(inst.args.len(), 1, "expected 1 arg");
    assert_eq!(inst.args[0].0, "wall", "arg name should be 'wall'");
    match &inst.args[0].1.kind {
        ExprKind::Ident(name) => assert_eq!(name, "t", "arg value should be Ident('t')"),
        other => panic!("expected Ident('t'), got {:?}", other),
    }
    assert!(inst.where_clause.is_none(), "expected no where_clause");
}

// ── Step 3: multi-arg constraint instantiation ───────────────────

#[test]
fn parse_multi_arg_constraint_inst() {
    let source = "structure S { param t: Length  constraint Bounded(lo: 1, hi: t, x: t) }";
    let (members, errors) = parse_structure_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let inst = members
        .iter()
        .find_map(|m| {
            if let MemberDecl::ConstraintInst(ci) = m {
                Some(ci)
            } else {
                None
            }
        })
        .expect("expected a ConstraintInst member");

    assert_eq!(inst.name, "Bounded");
    assert_eq!(inst.args.len(), 3, "expected 3 args");
    assert_eq!(inst.args[0].0, "lo");
    assert_eq!(inst.args[1].0, "hi");
    assert_eq!(inst.args[2].0, "x");

    // lo: 1 → NumberLiteral(1.0)
    match &inst.args[0].1.kind {
        ExprKind::NumberLiteral { value: n, .. } => assert!((n - 1.0).abs() < 1e-9, "expected 1.0"),
        other => panic!("expected NumberLiteral(1.0) for 'lo', got {:?}", other),
    }
    // hi: t → Ident('t')
    match &inst.args[1].1.kind {
        ExprKind::Ident(name) => assert_eq!(name, "t"),
        other => panic!("expected Ident('t') for 'hi', got {:?}", other),
    }
    // x: t → Ident('t')
    match &inst.args[2].1.kind {
        ExprKind::Ident(name) => assert_eq!(name, "t"),
        other => panic!("expected Ident('t') for 'x', got {:?}", other),
    }
}

// ── Step 5: constraint instantiation with where-clause ───────────

#[test]
fn parse_constraint_inst_with_where_clause() {
    let source =
        "structure S { param mode: Bool  param t: Length  constraint MinWall(wall: t) where mode }";
    let (members, errors) = parse_structure_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let inst = members
        .iter()
        .find_map(|m| {
            if let MemberDecl::ConstraintInst(ci) = m {
                Some(ci)
            } else {
                None
            }
        })
        .expect("expected a ConstraintInst member");

    assert_eq!(inst.name, "MinWall");
    let wc = inst
        .where_clause
        .as_ref()
        .expect("expected Some(where_clause)");
    match &wc.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "mode"),
        other => panic!("expected Ident('mode') in where_clause, got {:?}", other),
    }
}
