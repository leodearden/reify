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
