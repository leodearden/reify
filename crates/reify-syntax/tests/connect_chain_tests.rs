//! Connect and chain statement tests.
//!
//! Tests for `connect a -> b` and `chain a -> b -> c` declarations.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("connect_test"));
    (module.declarations, module.errors)
}

// ── Step 1: simple connect ────────────────────────────────────────

#[test]
fn parse_connect_simple() {
    let (decls, errors) = parse_decls("structure S { port a : out T  port b : in T  connect a -> b }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    // Two ports + one connect = 3 members
    assert_eq!(structure.members.len(), 3, "expected 3 members, got {:?}", structure.members);

    assert!(matches!(&structure.members[0], MemberDecl::Port(_)));
    assert!(matches!(&structure.members[1], MemberDecl::Port(_)));

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    // Check left port ref
    match &connect.left.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "a"),
        other => panic!("expected Ident('a'), got {:?}", other),
    }

    // Check operator
    assert_eq!(connect.operator, ConnectOp::Forward);

    // Check right port ref
    match &connect.right.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "b"),
        other => panic!("expected Ident('b'), got {:?}", other),
    }

    // No connector or body
    assert!(connect.connector_type.is_none());
    assert!(connect.params.is_empty());
    assert!(connect.port_mappings.is_empty());
}
