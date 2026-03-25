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

// ── Step 3: connect with connector ────────────────────────────────

#[test]
fn parse_connect_with_connector() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = 8.8 } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.connector_type.as_deref(), Some("BoltSet"));
    assert_eq!(connect.params.len(), 1);
    assert_eq!(connect.params[0].0, "grade");
    match &connect.params[0].1.kind {
        ExprKind::NumberLiteral(n) => assert!((*n - 8.8).abs() < 1e-10),
        other => panic!("expected NumberLiteral(8.8), got {:?}", other),
    }
}

// ── Step 5: connect with port mapping ─────────────────────────────

#[test]
fn parse_connect_with_port_mapping() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b { shaft -> input_bore } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert!(connect.connector_type.is_none());
    assert_eq!(connect.port_mappings.len(), 1);
    assert_eq!(connect.port_mappings[0].0, "shaft");
    assert_eq!(connect.port_mappings[0].1, "input_bore");
}

// ── Step 7: bidirectional connect ─────────────────────────────────

#[test]
fn parse_connect_bidirectional() {
    let (decls, errors) = parse_decls(
        "structure S { port a : bidi T  port b : bidi T  connect a <-> b }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.operator, ConnectOp::Bidirectional);
}

// ── Step 9: simple chain ──────────────────────────────────────────

#[test]
fn parse_chain_simple() {
    let (decls, errors) = parse_decls("structure S { chain a -> b -> c }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.members.len(), 1);
    let chain = match &structure.members[0] {
        MemberDecl::Chain(c) => c,
        other => panic!("expected Chain, got {:?}", other),
    };

    assert_eq!(chain.elements.len(), 3);
    match &chain.elements[0].kind {
        ExprKind::Ident(name) => assert_eq!(name, "a"),
        other => panic!("expected Ident('a'), got {:?}", other),
    }
    match &chain.elements[1].kind {
        ExprKind::Ident(name) => assert_eq!(name, "b"),
        other => panic!("expected Ident('b'), got {:?}", other),
    }
    match &chain.elements[2].kind {
        ExprKind::Ident(name) => assert_eq!(name, "c"),
        other => panic!("expected Ident('c'), got {:?}", other),
    }
}

// ── Step 11: connect with member access ───────────────────────────

#[test]
fn parse_connect_with_member_access() {
    let (decls, errors) = parse_decls(
        "structure S { sub m = Motor()  sub c = Coupling()  connect m.shaft -> c.driver }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    match &connect.left.expr.kind {
        ExprKind::MemberAccess { object, member } => {
            match &object.kind {
                ExprKind::Ident(name) => assert_eq!(name, "m"),
                other => panic!("expected Ident('m'), got {:?}", other),
            }
            assert_eq!(member, "shaft");
        }
        other => panic!("expected MemberAccess, got {:?}", other),
    }

    match &connect.right.expr.kind {
        ExprKind::MemberAccess { object, member } => {
            match &object.kind {
                ExprKind::Ident(name) => assert_eq!(name, "c"),
                other => panic!("expected Ident('c'), got {:?}", other),
            }
            assert_eq!(member, "driver");
        }
        other => panic!("expected MemberAccess, got {:?}", other),
    }
}

// ── task-246: mixed params and mappings ───────────────────────────

#[test]
fn parse_connect_mixed_params_and_mappings() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = 8.8, shaft -> input_bore } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.connector_type.as_deref(), Some("BoltSet"));

    assert_eq!(connect.params.len(), 1, "expected 1 param, got {:?}", connect.params);
    assert_eq!(connect.params[0].0, "grade");
    match &connect.params[0].1.kind {
        ExprKind::NumberLiteral(n) => assert!((*n - 8.8).abs() < 1e-10, "expected 8.8, got {}", n),
        other => panic!("expected NumberLiteral(8.8), got {:?}", other),
    }

    assert_eq!(connect.port_mappings.len(), 1, "expected 1 port_mapping, got {:?}", connect.port_mappings);
    assert_eq!(connect.port_mappings[0].0, "shaft");
    assert_eq!(connect.port_mappings[0].1, "input_bore");
}

// ── parse_connect_reverse ─────────────────────────────────────────

#[test]
fn parse_connect_reverse() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a <- b }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.operator, ConnectOp::Reverse);

    match &connect.left.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "a"),
        other => panic!("expected Ident('a'), got {:?}", other),
    }
    match &connect.right.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "b"),
        other => panic!("expected Ident('b'), got {:?}", other),
    }
}
