//! Trait declaration tests.
//!
//! Tests for `trait Name { ... }` declarations with refinements, type parameters,
//! associated types, pub visibility, and structure trait bounds.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("trait_test"));
    (module.declarations, module.errors)
}

// ── Step 1: basic trait ────────────────────────────────────────────

#[test]
fn parse_basic_trait() {
    let (decls, errors) = parse_decls("trait Rigid { param mass : Mass }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.name, "Rigid");
    assert!(!trait_decl.is_pub);
    assert!(trait_decl.refinements.is_empty());
    assert!(trait_decl.type_params.is_empty());
    assert_eq!(trait_decl.members.len(), 1);

    match &trait_decl.members[0] {
        MemberDecl::Param(p) => {
            assert_eq!(p.name, "mass");
            assert_eq!(p.type_expr.as_ref().unwrap().name, "Mass");
        }
        other => panic!("expected Param, got {:?}", other),
    }
}

// ── Step 3: trait with refinement ──────────────────────────────────

#[test]
fn parse_trait_with_refinement() {
    let (decls, errors) = parse_decls("trait Fastener : Rigid { param thread_pitch : Length }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.name, "Fastener");
    assert_eq!(trait_decl.refinements, vec!["Rigid"]);
    assert_eq!(trait_decl.members.len(), 1);

    match &trait_decl.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "thread_pitch"),
        other => panic!("expected Param, got {:?}", other),
    }
}
