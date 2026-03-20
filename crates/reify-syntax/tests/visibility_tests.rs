//! Visibility (pub) keyword tests for the parser.
//!
//! Tests that `pub` keyword is correctly parsed on structure, let, and enum declarations.

use reify_syntax::*;

/// Helper: parse source and return the first declaration.
fn parse_first_decl(source: &str) -> (Declaration, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("vis_test"));
    let decl = module.declarations.into_iter().next().expect("expected at least one declaration");
    (decl, module.errors)
}

/// Helper: parse source and return the first structure's members.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("vis_test"));
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

// ── Step 1: pub structure ────────────────────────────────────────────

#[test]
fn parse_pub_structure() {
    let source = r#"pub structure Bracket {
    param w: Scalar = 80mm
}"#;
    let (decl, errors) = parse_first_decl(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    match decl {
        Declaration::Structure(s) => {
            assert_eq!(s.name, "Bracket");
            assert!(s.is_pub, "expected is_pub == true for pub structure");
        }
        other => panic!("expected Structure, got {:?}", other),
    }
}

#[test]
fn parse_structure_default_not_pub() {
    let source = r#"structure Bracket {
    param w: Scalar = 80mm
}"#;
    let (decl, errors) = parse_first_decl(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    match decl {
        Declaration::Structure(s) => {
            assert_eq!(s.name, "Bracket");
            assert!(!s.is_pub, "expected is_pub == false for non-pub structure");
        }
        other => panic!("expected Structure, got {:?}", other),
    }
}
