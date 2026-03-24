//! Qualified access expression tests.
//!
//! Tests for `TypeName::ident` (qualified trait access) and
//! `expr.(TypeName::ident)` (instance-level qualified trait access).

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("qualified_access_test"));
    (module.declarations, module.errors)
}

// ── Step 1: basic qualified access ────────────────────────────────────────
// ── Step 3: chained qualified access ──────────────────────────────────────

#[test]
fn parse_chained_qualified_access() {
    let (decls, errors) = parse_decls("structure S { let x = A::B::c }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let s = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &s.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    // Expected: QualifiedAccess { qualifier: QualifiedAccess { qualifier: Ident("A"), member: "B" }, member: "c" }
    match &let_decl.value.kind {
        ExprKind::QualifiedAccess { qualifier, member } => {
            assert_eq!(member, "c");
            match &qualifier.kind {
                ExprKind::QualifiedAccess { qualifier: inner_qualifier, member: inner_member } => {
                    assert_eq!(inner_member, "B");
                    match &inner_qualifier.kind {
                        ExprKind::Ident(name) => assert_eq!(name, "A"),
                        other => panic!("expected Ident inner qualifier, got {:?}", other),
                    }
                }
                other => panic!("expected inner QualifiedAccess, got {:?}", other),
            }
        }
        other => panic!("expected outer QualifiedAccess, got {:?}", other),
    }
}

// ── Step 1: basic qualified access ────────────────────────────────

#[test]
fn parse_basic_qualified_access() {
    let (decls, errors) = parse_decls("structure S { let x = Rigid::mass }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let s = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &s.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::QualifiedAccess { qualifier, member } => {
            assert_eq!(member, "mass");
            match &qualifier.kind {
                ExprKind::Ident(name) => assert_eq!(name, "Rigid"),
                other => panic!("expected Ident qualifier, got {:?}", other),
            }
        }
        other => panic!("expected QualifiedAccess, got {:?}", other),
    }
}
