//! Qualified access (`::`) and instance qualified access (`.(...)`) parsing integration tests.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("qualified_test"));
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
    let (decls, errors) = parse_decls("structure S { let x = obj.(42) }");
    // Grammar constrains inner to $.qualified_access, so obj.(42) should trigger
    // tree-sitter error recovery and produce parse errors.
    assert!(
        !errors.is_empty() || decls.is_empty() || {
            // Even if structure parses, the let value should be missing (lowering
            // returns None because the inner isn't a qualified_access CST node)
            match &decls[0] {
                Declaration::Structure(s) => s.members.is_empty(),
                _ => true,
            }
        },
        "obj.(42) should fail: decls={:?}, errors={:?}",
        decls,
        errors
    );
}
