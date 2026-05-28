//! Constraint definition tests.
//!
//! Tests for `constraint def Name { params, predicate_lines }` declarations.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("constraint_def_test"),
    );
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

    assert_eq!(
        cd.predicates.len(),
        3,
        "expected 3 predicates (conjunction)"
    );
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

// ── Step 15: syntax error in constraint body ──────────────────────

#[test]
fn parse_constraint_def_body_syntax_error() {
    // `>= }` is not a valid body item — no left operand, no right operand.
    // Tree-sitter produces an ERROR node as a direct child of constraint_definition.
    // With current `_ => {}` catch-all this is silently swallowed (no error reported).
    // After step-16 fix, an explicit "ERROR" arm emits 'syntax error in constraint body'.
    let source = "constraint def Bad { >= }";
    let (decls, errors) = parse_decls(source);
    assert!(
        !errors.is_empty(),
        "expected parse errors for invalid syntax inside constraint body, got none"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("syntax error in constraint body")),
        "expected an error message containing 'syntax error in constraint body', got: {:?}",
        errors
    );
    // The constraint def should still be constructed (with empty predicates).
    assert_eq!(
        decls.len(),
        1,
        "expected constraint decl to be constructed despite body error"
    );
    match &decls[0] {
        Declaration::Constraint(c) => {
            assert_eq!(c.name, "Bad");
            assert!(
                c.predicates.is_empty(),
                "expected no predicates due to body error"
            );
        }
        other => panic!("expected Declaration::Constraint, got {:?}", other),
    }
}

// ── Step 17: error param in constraint body ───────────────────────

#[test]
fn parse_constraint_def_error_param() {
    // `param wall : Box<,>` — a comma with no second type arg causes tree-sitter
    // to insert a MISSING identifier node inside type_args, making the
    // param_declaration node have `has_error() == true`.
    //
    // NOTE: `Box<>` (empty angle brackets) does NOT trigger this — tree-sitter
    // resolves that ambiguity by treating `Box` as a plain identifier type_expr
    // and consuming `<>  x > 0` as a range-expression predicate (no error node).
    // `Box<,>` correctly produces the MISSING node because the comma implies a
    // second type argument that is absent.
    //
    // Without `check_and_lower!` (before step-18), `self.lower_param()` is called
    // directly: it succeeds (name "wall" is found) and silently adds the malformed
    // param to params with no diagnostic pushed.
    //
    // After step-18 fix, `check_and_lower!` detects `has_error()`, emits
    // 'invalid constraint param: ...', and skips the param entirely.
    let source = "constraint def Bad { param wall : Box<,>  x > 0 }";
    let (decls, errors) = parse_decls(source);
    assert!(
        !errors.is_empty(),
        "expected parse errors for malformed param_declaration (Box<,> has MISSING type arg), got none"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("invalid constraint param")),
        "expected an error message containing 'invalid constraint param', got: {:?}",
        errors
    );
    // The constraint should still be constructed: 0 params (bad param skipped), 1 valid predicate.
    assert_eq!(
        decls.len(),
        1,
        "expected constraint decl to be constructed despite param error"
    );
    match &decls[0] {
        Declaration::Constraint(c) => {
            assert_eq!(c.name, "Bad");
            assert_eq!(
                c.params.len(),
                0,
                "expected 0 params (malformed param skipped by check_and_lower!)"
            );
            assert_eq!(c.predicates.len(), 1, "expected 1 valid predicate (x > 0)");
        }
        other => panic!("expected Declaration::Constraint, got {:?}", other),
    }
}
