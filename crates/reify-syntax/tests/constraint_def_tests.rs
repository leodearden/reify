//! Constraint definition tests.
//!
//! Tests for `constraint def Name { params, predicate_lines }` declarations.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("constraint_def_test"));
    (module.declarations, module.errors)
}

// ── Step 1: basic constraint def ─────────────────────────────────

#[test]
fn parse_basic_constraint_def() {
    let source = "constraint def Foo { x > 0 }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    match &decls[0] {
        Declaration::Constraint(c) => {
            assert_eq!(c.name, "Foo");
        }
        other => panic!("expected Declaration::Constraint, got {:?}", other),
    }
}

// ── Step 3: predicate expression content ─────────────────────────

#[test]
fn parse_constraint_def_predicate_expr() {
    let source = "constraint def Check { a > 5 }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(cd.predicates.len(), 1, "expected 1 predicate");

    // Verify the predicate is a BinOp with op '>' and left Ident('a')
    match &cd.predicates[0].kind {
        ExprKind::BinOp { op, left, .. } => {
            assert_eq!(op, ">", "expected op '>'");
            match &left.kind {
                ExprKind::Ident(name) => assert_eq!(name, "a"),
                other => panic!("expected Ident('a'), got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

// ── Step 5: params extraction ─────────────────────────────────────

#[test]
fn parse_constraint_def_with_params() {
    let source = "constraint def MinWall {
        param wall : Length
        param process : Process
        wall >= process.min_wall
    }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(cd.params.len(), 2, "expected 2 params");
    assert_eq!(cd.params[0].name, "wall");
    assert_eq!(cd.params[1].name, "process");
    assert_eq!(cd.predicates.len(), 1, "expected 1 predicate");
}

// ── Step 7: multiple predicates ───────────────────────────────────

#[test]
fn parse_constraint_def_multiple_predicates() {
    let source = "constraint def Multi {
        param x : Scalar
        x > 0
        x < 100
        x != 50
    }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(cd.predicates.len(), 3, "expected 3 predicates (conjunction)");
}

// ── Step 9: pub constraint def ────────────────────────────────────

#[test]
fn parse_pub_constraint_def() {
    let source = "pub constraint def Visible { x > 0 }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert!(cd.is_pub, "expected is_pub == true");
    assert_eq!(cd.name, "Visible");
}

// ── Step 11: type parameters ──────────────────────────────────────

#[test]
fn parse_constraint_def_with_type_params() {
    let source = "constraint def Aligned<T : Rigid> { param t : T  t.aligned }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(cd.type_params.len(), 1, "expected 1 type param");
    assert_eq!(cd.type_params[0].name, "T");
}

// ── Step 13: complex integration test ────────────────────────────

#[test]
fn parse_constraint_def_complex() {
    // A realistic constraint def from the spec style (DFM-like)
    let source = "pub constraint def MinWallThickness<M : ManufacturingProcess> {
        param wall : Length
        param process : M
        wall >= process.min_wall_thickness
        wall > 0
    }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(cd.name, "MinWallThickness");
    assert!(cd.is_pub);
    assert_eq!(cd.type_params.len(), 1);
    assert_eq!(cd.type_params[0].name, "M");
    assert_eq!(cd.params.len(), 2);
    assert_eq!(cd.params[0].name, "wall");
    assert_eq!(cd.params[1].name, "process");
    assert_eq!(cd.predicates.len(), 2);
}
