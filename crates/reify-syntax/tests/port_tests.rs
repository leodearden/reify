//! Port declaration tests.
//!
//! Tests for `port name : [direction] TraitType { ... }` declarations.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("port_test"));
    (module.declarations, module.errors)
}

// ── Step 1: minimal port ───────────────────────────────────────────

#[test]
fn parse_minimal_port() {
    let (decls, errors) = parse_decls("structure S { port mount : MechanicalPort }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.members.len(), 1);
    let port = match &structure.members[0] {
        MemberDecl::Port(p) => p,
        other => panic!("expected Port, got {:?}", other),
    };

    assert_eq!(port.name, "mount");
    assert_eq!(port.type_name, "MechanicalPort");
    assert!(port.direction.is_none());
    assert!(port.members.is_empty());
    assert!(port.frame_expr.is_none());
}

// ── Step 3: port with direction ────────────────────────────────────

#[test]
fn parse_port_with_direction() {
    let (decls, errors) = parse_decls("structure S { port shaft : in RotaryPort }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let port = match &structure.members[0] {
        MemberDecl::Port(p) => p,
        other => panic!("expected Port, got {:?}", other),
    };

    assert_eq!(port.name, "shaft");
    assert_eq!(port.type_name, "RotaryPort");
    assert_eq!(port.direction, Some(reify_core::PortDirection::In));
}

#[test]
fn parse_port_direction_out() {
    let (decls, errors) = parse_decls("structure S { port b : out T }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let port = match &structure.members[0] {
        MemberDecl::Port(p) => p,
        other => panic!("expected Port, got {:?}", other),
    };

    assert_eq!(port.direction, Some(reify_core::PortDirection::Out));
}

#[test]
fn parse_port_direction_bidi() {
    let (decls, errors) = parse_decls("structure S { port c : bidi T }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let port = match &structure.members[0] {
        MemberDecl::Port(p) => p,
        other => panic!("expected Port, got {:?}", other),
    };

    assert_eq!(port.direction, Some(reify_core::PortDirection::Bidi));
}

// ── Step 5: port with body ─────────────────────────────────────────

#[test]
fn parse_port_with_body() {
    let source = "structure S { port x : MechPort { direction = out  param d : Length = 10mm  constraint d > 0mm } }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let port = match &structure.members[0] {
        MemberDecl::Port(p) => p,
        other => panic!("expected Port, got {:?}", other),
    };

    assert_eq!(port.name, "x");
    assert_eq!(port.type_name, "MechPort");
    assert_eq!(port.direction, Some(reify_core::PortDirection::Out));
    assert!(port.frame_expr.is_none());

    // Should have param and constraint members
    assert_eq!(port.members.len(), 2);
    match &port.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "d"),
        other => panic!("expected Param, got {:?}", other),
    }
    assert!(matches!(&port.members[1], MemberDecl::Constraint(_)));
}

// ── Step 7: port with frame ────────────────────────────────────────

#[test]
fn parse_port_with_frame() {
    let source = "structure S { port x : MechPort { frame = origin } }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let port = match &structure.members[0] {
        MemberDecl::Port(p) => p,
        other => panic!("expected Port, got {:?}", other),
    };

    assert!(port.frame_expr.is_some());
    match &port.frame_expr.as_ref().unwrap().kind {
        ExprKind::Ident(name) => assert_eq!(name, "origin"),
        other => panic!("expected Ident, got {:?}", other),
    }
}

#[test]
fn parse_port_with_frame_function_call() {
    let source = "structure S { port x : MechPort { frame = frame3(origin) } }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let port = match &structure.members[0] {
        MemberDecl::Port(p) => p,
        other => panic!("expected Port, got {:?}", other),
    };

    assert!(port.frame_expr.is_some());
    match &port.frame_expr.as_ref().unwrap().kind {
        ExprKind::FunctionCall { name, .. } => assert_eq!(name, "frame3"),
        other => panic!("expected FunctionCall, got {:?}", other),
    }
}

// ── Step 9: direction override ──────────────────────────────────────

#[test]
fn parse_port_direction_override() {
    let source = "structure S { port x : in T { direction = out } }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let port = match &structure.members[0] {
        MemberDecl::Port(p) => p,
        other => panic!("expected Port, got {:?}", other),
    };

    // Body direction (out) should override inline direction (in)
    assert_eq!(port.direction, Some(reify_core::PortDirection::Out));
}
