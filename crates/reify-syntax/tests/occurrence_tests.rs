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
