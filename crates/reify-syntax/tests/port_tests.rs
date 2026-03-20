//! Port declaration tests.
//!
//! Tests for `port name : [direction] TraitType { ... }` declarations.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("port_test"));
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
    assert_eq!(port.direction, Some(reify_types::PortDirection::In));
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

    assert_eq!(port.direction, Some(reify_types::PortDirection::Out));
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

    assert_eq!(port.direction, Some(reify_types::PortDirection::Bidi));
}
