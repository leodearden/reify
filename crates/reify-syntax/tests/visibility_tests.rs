//! Visibility (pub) keyword tests for the parser.
//!
//! Tests that `pub` keyword is correctly parsed on structure, let, and enum declarations.

use reify_ast::*;

/// Helper: parse source and return the first declaration.
fn parse_first_decl(source: &str) -> (Declaration, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("vis_test"));
    let decl = module
        .declarations
        .into_iter()
        .next()
        .expect("expected at least one declaration");
    (decl, module.errors)
}

/// Helper: parse source and return the first structure's members.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("vis_test"));
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

// ── Step 3: pub let ────────────────────────────────────────────────

#[test]
fn parse_pub_let() {
    let source = r#"structure S {
    pub let x = 5
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1);
    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "x");
    assert!(let_decl.is_pub, "expected is_pub == true for pub let");
}

#[test]
fn parse_let_default_not_pub() {
    let source = r#"structure S {
    param a: Scalar = 1mm
    let y = a * 2
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let let_decl = members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) => Some(l),
            _ => None,
        })
        .expect("expected a let declaration");
    assert_eq!(let_decl.name, "y");
    assert!(!let_decl.is_pub, "expected is_pub == false for non-pub let");
}

// ── Step 5: pub enum ──────────────────────────────────────────────

#[test]
fn parse_pub_enum() {
    let source = "pub enum Direction { In, Out }";
    let (decl, errors) = parse_first_decl(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    match decl {
        Declaration::Enum(e) => {
            assert_eq!(e.name, "Direction");
            assert!(e.is_pub, "expected is_pub == true for pub enum");
            let variant_names: Vec<&str> = e.variants.iter().map(|v| v.name.as_str()).collect();
            assert_eq!(variant_names, vec!["In", "Out"]);
        }
        other => panic!("expected Enum, got {:?}", other),
    }
}

#[test]
fn parse_enum_default_not_pub() {
    let source = "enum Direction { In, Out }";
    let (decl, errors) = parse_first_decl(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    match decl {
        Declaration::Enum(e) => {
            assert_eq!(e.name, "Direction");
            assert!(!e.is_pub, "expected is_pub == false for non-pub enum");
        }
        other => panic!("expected Enum, got {:?}", other),
    }
}
