//! Occurrence declaration parsing tests.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("occ_test"));
    (module.declarations, module.errors)
}

// ── step-1: parse basic occurrence ───────────────────────────────────

#[test]
fn parse_basic_occurrence() {
    let (decls, errors) = parse_decls("occurrence def Welding { param method : Length }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let occ = match &decls[0] {
        Declaration::Occurrence(o) => o,
        other => panic!("expected Occurrence, got {:?}", other),
    };

    assert_eq!(occ.name, "Welding");
    assert!(!occ.is_pub);
    assert_eq!(occ.members.len(), 1);

    match &occ.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "method"),
        other => panic!("expected Param, got {:?}", other),
    }
}

// ── step-3: parse occurrence with ports ──────────────────────────────

#[test]
fn parse_occurrence_with_ports() {
    let source = r#"
occurrence def Welding {
    param method : Length
    port workpiece : in StructurePort
    port result : out StructurePort
}
"#;
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let occ = match &decls[0] {
        Declaration::Occurrence(o) => o,
        other => panic!("expected Occurrence, got {:?}", other),
    };

    assert_eq!(occ.name, "Welding");
    assert_eq!(occ.members.len(), 3);

    // First member: param
    match &occ.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "method"),
        other => panic!("expected Param, got {:?}", other),
    }

    // Second member: in port
    match &occ.members[1] {
        MemberDecl::Port(p) => {
            assert_eq!(p.name, "workpiece");
            assert_eq!(p.direction, Some(reify_types::PortDirection::In));
            assert_eq!(p.type_name, "StructurePort");
        }
        other => panic!("expected Port, got {:?}", other),
    }

    // Third member: out port
    match &occ.members[2] {
        MemberDecl::Port(p) => {
            assert_eq!(p.name, "result");
            assert_eq!(p.direction, Some(reify_types::PortDirection::Out));
            assert_eq!(p.type_name, "StructurePort");
        }
        other => panic!("expected Port, got {:?}", other),
    }
}
