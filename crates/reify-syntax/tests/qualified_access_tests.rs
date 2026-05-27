//! Qualified access (`::`) and instance qualified access (`.(...)`) parsing integration tests.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("qualified_test"));
    (module.declarations, module.errors)
}

// ── Step 1: basic qualified access ──────────────────────────────────────────

#[test]
fn parse_basic_qualified_access() {
    let (decls, errors) = parse_decls("structure S { let x = Foo::bar }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::QualifiedAccess { qualifier, member } => {
            assert!(matches!(&qualifier.kind, ExprKind::Ident(n) if n == "Foo"));
            assert_eq!(member, "bar");
        }
        other => panic!("expected QualifiedAccess, got {:?}", other),
    }
}

// ── Step 3: chained qualified access ────────────────────────────────────────

#[test]
fn parse_chained_qualified_access() {
    let (decls, errors) = parse_decls("structure S { let x = A::B::c }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    // Outer: QualifiedAccess { qualifier: QualifiedAccess { A, B }, member: "c" }
    match &let_decl.value.kind {
        ExprKind::QualifiedAccess { qualifier, member } => {
            assert_eq!(member, "c");
            // Inner qualifier is A::B
            match &qualifier.kind {
                ExprKind::QualifiedAccess {
                    qualifier: inner_qual,
                    member: inner_member,
                } => {
                    assert!(matches!(&inner_qual.kind, ExprKind::Ident(n) if n == "A"));
                    assert_eq!(inner_member, "B");
                }
                other => panic!("expected inner QualifiedAccess, got {:?}", other),
            }
        }
        other => panic!("expected QualifiedAccess, got {:?}", other),
    }
}

// ── Step 5: precedence over && ──────────────────────────────────────────────

#[test]
fn parse_qualified_access_precedence_over_and() {
    let (decls, errors) = parse_decls("structure S { constraint Foo::bar && Baz::qux }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let constraint = match &structure.members[0] {
        MemberDecl::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    // Top-level should be BinOp { && } with QualifiedAccess on both sides
    match &constraint.expr.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "&&");
            match &left.kind {
                ExprKind::QualifiedAccess { qualifier, member } => {
                    assert!(matches!(&qualifier.kind, ExprKind::Ident(n) if n == "Foo"));
                    assert_eq!(member, "bar");
                }
                other => panic!("expected left QualifiedAccess, got {:?}", other),
            }
            match &right.kind {
                ExprKind::QualifiedAccess { qualifier, member } => {
                    assert!(matches!(&qualifier.kind, ExprKind::Ident(n) if n == "Baz"));
                    assert_eq!(member, "qux");
                }
                other => panic!("expected right QualifiedAccess, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

// ── Step 7: instance qualified access ───────────────────────────────────────

#[test]
fn parse_instance_qualified_access() {
    let (decls, errors) = parse_decls("structure S { let x = obj.(Foo::bar) }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "obj"));
            match &qualified.kind {
                ExprKind::QualifiedAccess { qualifier, member } => {
                    assert!(matches!(&qualifier.kind, ExprKind::Ident(n) if n == "Foo"));
                    assert_eq!(member, "bar");
                }
                other => panic!("expected inner QualifiedAccess, got {:?}", other),
            }
        }
        other => panic!("expected InstanceQualifiedAccess, got {:?}", other),
    }
}

// ── Step 9: invalid instance qualified access ───────────────────────────────

#[test]
fn parse_invalid_instance_qualified_numeric() {
    let (_decls, errors) = parse_decls("structure S { let x = obj.(42) }");
    // Lowering validates the inner expression and emits a specific error;
    // assert errors are always non-empty — no silent data loss.
    assert!(
        !errors.is_empty(),
        "obj.(42) should produce parse errors, got none",
    );
}

// ── Step 11: member access not shadowed ─────────────────────────────────────

#[test]
fn parse_member_access_not_shadowed() {
    let (decls, errors) = parse_decls("structure S { let x = a.b }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::MemberAccess { object, member } => {
            assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "a"));
            assert_eq!(member, "b");
        }
        other => panic!("expected MemberAccess, got {:?}", other),
    }
}

// ── Step 15: invalid instance qualified access emits specific diagnostic ────

#[test]
fn parse_invalid_instance_qualified_emits_diagnostic() {
    let (_decls, errors) = parse_decls("structure S { let x = obj.(42) }");
    // The lowering must emit a specific diagnostic — not just a generic tree-sitter
    // ERROR node message — so the user knows what went wrong.
    assert!(
        !errors.is_empty(),
        "expected errors for obj.(42), but got none",
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("qualified_access") || e.message.contains("requires")),
        "expected at least one error mentioning 'qualified_access' or 'requires', got: {:?}",
        errors,
    );
}

// ── Step 13: qualified access in arithmetic ─────────────────────────────────

#[test]
fn parse_qualified_in_arithmetic() {
    let (decls, errors) = parse_decls("structure S { let x = Foo::bar + 1 }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    // Top-level should be BinOp { + } with QualifiedAccess on left, NumberLiteral on right
    match &let_decl.value.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "+");
            match &left.kind {
                ExprKind::QualifiedAccess { qualifier, member } => {
                    assert!(matches!(&qualifier.kind, ExprKind::Ident(n) if n == "Foo"));
                    assert_eq!(member, "bar");
                }
                other => panic!("expected QualifiedAccess as left, got {:?}", other),
            }
            assert!(
                matches!(&right.kind, ExprKind::NumberLiteral { value: n, .. } if *n == 1.0),
                "expected NumberLiteral(1), got {:?}",
                right.kind
            );
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}
