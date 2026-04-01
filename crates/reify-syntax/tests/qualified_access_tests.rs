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
