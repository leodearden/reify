//! Collection sub-structure tests (task 64).

// ─── step-1: parse collection sub form ───

#[test]
fn parse_collection_sub_form() {
    let source = "structure S { sub bolts : List<Bolt> }";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let structure = match &parsed.declarations[0] {
        reify_syntax::Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let sub = match &structure.members[0] {
        reify_syntax::MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };
    assert_eq!(sub.name, "bolts");
    assert_eq!(sub.structure_name, "Bolt");
    assert!(sub.is_collection, "expected is_collection=true for List<Bolt>");
    assert!(sub.args.is_empty(), "collection sub should have no args");
}

#[test]
fn parse_instantiation_sub_form() {
    let source = "structure S { sub rib = Rib() }";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let structure = match &parsed.declarations[0] {
        reify_syntax::Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let sub = match &structure.members[0] {
        reify_syntax::MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };
    assert_eq!(sub.name, "rib");
    assert_eq!(sub.structure_name, "Rib");
    assert!(!sub.is_collection, "expected is_collection=false for = form");
}
