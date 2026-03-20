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
