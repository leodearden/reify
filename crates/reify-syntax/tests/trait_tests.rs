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

// ── Step 5: multiple refinements ──────────────────────────────────

#[test]
fn parse_trait_multiple_refinements() {
    let (decls, errors) = parse_decls("trait A : B + C { }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.name, "A");
    assert_eq!(trait_decl.refinements, vec!["B", "C"]);
    assert!(trait_decl.members.is_empty());
}

// ── Step 7: various members ───────────────────────────────────────

#[test]
fn parse_trait_various_members() {
    let source = "trait Full {\n  param mass : Mass\n  let density = mass / volume\n  constraint mass > 0\n  sub inner = Component()\n}";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.members.len(), 4);

    match &trait_decl.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "mass"),
        other => panic!("expected Param, got {:?}", other),
    }
    match &trait_decl.members[1] {
        MemberDecl::Let(l) => assert_eq!(l.name, "density"),
        other => panic!("expected Let, got {:?}", other),
    }
    assert!(matches!(&trait_decl.members[2], MemberDecl::Constraint(_)));
    match &trait_decl.members[3] {
        MemberDecl::Sub(s) => assert_eq!(s.name, "inner"),
        other => panic!("expected Sub, got {:?}", other),
    }
}

// ── Step 9: associated types ──────────────────────────────────────

#[test]
fn parse_trait_associated_type() {
    let (decls, errors) = parse_decls("trait WithType { type Material = Steel }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.members.len(), 1);
    match &trait_decl.members[0] {
        MemberDecl::AssociatedType(a) => {
            assert_eq!(a.name, "Material");
            assert!(a.default_type.is_some());
            assert_eq!(a.default_type.as_ref().unwrap().name, "Steel");
        }
        other => panic!("expected AssociatedType, got {:?}", other),
    }
}

#[test]
fn parse_trait_associated_type_no_default() {
    let (decls, errors) = parse_decls("trait Bare { type Output }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.members.len(), 1);
    match &trait_decl.members[0] {
        MemberDecl::AssociatedType(a) => {
            assert_eq!(a.name, "Output");
            assert!(a.default_type.is_none());
        }
        other => panic!("expected AssociatedType, got {:?}", other),
    }
}

// ── Step 11: pub trait ────────────────────────────────────────────

#[test]
fn parse_pub_trait() {
    let (decls, errors) = parse_decls("pub trait Visible { param color : Scalar }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert!(trait_decl.is_pub);
    assert_eq!(trait_decl.name, "Visible");
    assert_eq!(trait_decl.members.len(), 1);
}

// ── Step 13: structure with trait bounds ───────────────────────────

#[test]
fn parse_structure_with_trait_bounds() {
    let (decls, errors) = parse_decls("structure def Bolt : Fastener + Rigid { param length : Length = 20mm }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.name, "Bolt");
    assert_eq!(structure.trait_bounds, vec!["Fastener", "Rigid"]);
    assert!(!structure.is_pub);
    assert!(structure.type_params.is_empty());
    assert_eq!(structure.members.len(), 1);
}

// ── Step 15: type parameters ──────────────────────────────────────

#[test]
fn parse_trait_with_type_params() {
    let (decls, errors) = parse_decls("trait Container<T: Rigid> { param capacity : Scalar }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.name, "Container");
    assert_eq!(trait_decl.type_params.len(), 1);
    assert_eq!(trait_decl.type_params[0].name, "T");
    assert_eq!(trait_decl.type_params[0].bounds, vec!["Rigid"]);
}

#[test]
fn parse_trait_with_multi_type_params() {
    let (decls, errors) = parse_decls("trait Pair<A: Rigid, B> { }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.type_params.len(), 2);
    assert_eq!(trait_decl.type_params[0].name, "A");
    assert_eq!(trait_decl.type_params[0].bounds, vec!["Rigid"]);
    assert_eq!(trait_decl.type_params[1].name, "B");
    assert!(trait_decl.type_params[1].bounds.is_empty());
}

// ── Step 17: backward compatibility ───────────────────────────────

#[test]
fn backward_compat_bracket_source() {
    let source = reify_test_support::bracket_source();
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.name, "Bracket");
    assert!(!structure.is_pub);
    assert!(structure.type_params.is_empty());
    assert!(structure.trait_bounds.is_empty());
    assert_eq!(structure.members.len(), 10);
}

#[test]
fn backward_compat_no_def_keyword() {
    let (decls, errors) = parse_decls("structure S { param x : Scalar = 5mm }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.name, "S");
    assert_eq!(structure.members.len(), 1);
}
