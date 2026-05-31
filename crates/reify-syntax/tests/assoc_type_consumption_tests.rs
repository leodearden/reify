//! Lowering tests for associated-type consumption surfaces (task 3971, ιₐ).
//!
//! All tests in this file are GREEN after the full task-3971 implementation:
//!
//! (a) Characterization: structure-body `type X = Concrete` binding
//!     → `MemberDecl::AssociatedType` (rides the existing `lower_member` arm;
//!     no new Rust code was required — grammar step-2 sufficed).
//! (b) Bare qualified type-expr in `param`/`let` type position
//!     → `TypeExprKind::QualifiedAssoc { base, trait_name: None, member }`
//!     (step-6 added the `qualified_type` branch in `lower_type_expr_node`).
//! (d) Disambiguated FORK-G form: `Beam::(HasMaterial::Material)`
//!     → `TypeExprKind::QualifiedAssoc { base, trait_name: Some("HasMaterial"), member }`
//!     (step-8 extended the lowering for the parenthesised trait disambiguator).

use reify_ast::*;

fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("assoc_type_test"));
    (module.declarations, module.errors)
}

fn as_structure(decls: &[Declaration]) -> &StructureDef {
    match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Declaration::Structure, got {:?}", other),
    }
}

fn as_assoc_type_member(member: &MemberDecl) -> &AssociatedTypeDecl {
    match member {
        MemberDecl::AssociatedType(a) => a,
        other => panic!("expected MemberDecl::AssociatedType, got {:?}", other),
    }
}

fn as_param_member(member: &MemberDecl) -> &ParamDecl {
    match member {
        MemberDecl::Param(p) => p,
        other => panic!("expected MemberDecl::Param, got {:?}", other),
    }
}

fn as_let_member(member: &MemberDecl) -> &LetDecl {
    match member {
        MemberDecl::Let(l) => l,
        other => panic!("expected MemberDecl::Let, got {:?}", other),
    }
}

// ── (a) Characterization: structure-body associated-type binding ─────────────
//
// This test pins that `type Material = Steel` inside a structure body lowers to
// `MemberDecl::AssociatedType` with name="Material" and default_type=Some(Named "Steel").
// No new Rust code required — `lower_member`'s `"associated_type"` arm already handles it;
// this test becomes GREEN as soon as the grammar admits `associated_type` in `_member`
// (step-2).

#[test]
fn structure_body_associated_type_lowers_to_member_assoc_type() {
    let (decls, errors) = parse_decls(
        "trait HasMaterial { type Material }
         structure def Beam : HasMaterial { type Material = Steel }",
    );
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);
    assert_eq!(decls.len(), 2, "expected 2 declarations");

    // Second declaration is the structure.
    let s = match &decls[1] {
        Declaration::Structure(s) => s,
        other => panic!("expected Declaration::Structure, got {:?}", other),
    };
    assert_eq!(s.members.len(), 1, "expected 1 member");

    let a = as_assoc_type_member(&s.members[0]);
    assert_eq!(a.name, "Material");

    let default_ty = a.default_type.as_ref().expect("expected Some(default_type)");
    match &default_ty.kind {
        TypeExprKind::Named { name, type_args } => {
            assert_eq!(name, "Steel");
            assert!(type_args.is_empty(), "expected no type args");
        }
        other => panic!("expected TypeExprKind::Named(Steel), got {:?}", other),
    }
}

// ── (b) New: bare qualified type-expr in param type position ────────────────
//
// RED until step-6: `lower_type_expr_node` currently mis-lowers `qualified_type`
// nodes to `Named { name: "Beam::Material" }` instead of `QualifiedAssoc`.

#[test]
fn param_bare_qualified_type_lowers_to_qualified_assoc() {
    let (decls, errors) = parse_decls(
        "structure def UseAssoc { param m : Beam::Material }",
    );
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);

    let s = as_structure(&decls);
    assert_eq!(s.members.len(), 1);

    let p = as_param_member(&s.members[0]);
    assert_eq!(p.name, "m");

    let ty = p.type_expr.as_ref().expect("expected Some(type_expr) for param m");
    match &ty.kind {
        TypeExprKind::QualifiedAssoc { base, trait_name, member } => {
            match &base.kind {
                TypeExprKind::Named { name, .. } => assert_eq!(name, "Beam"),
                other => panic!("expected base Named(Beam), got {:?}", other),
            }
            assert!(trait_name.is_none(), "expected trait_name = None for bare form");
            assert_eq!(member, "Material");
        }
        other => panic!(
            "expected TypeExprKind::QualifiedAssoc, got {:?}",
            other
        ),
    }
}

// ── (c) New: bare qualified type-expr in let type position (type-param base) ─
//
// RED until step-6: same as (b) but exercises the `T::Material` form where the
// base is a type parameter identifier rather than a concrete structure name.

#[test]
fn let_bare_qualified_type_with_type_param_base_lowers_to_qualified_assoc() {
    let (decls, errors) = parse_decls(
        "structure def UseAssoc<T> { let y : T::Material = steel_instance }",
    );
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);

    let s = as_structure(&decls);
    assert_eq!(s.members.len(), 1);

    let l = as_let_member(&s.members[0]);
    assert_eq!(l.name, "y");

    let ty = l.type_expr.as_ref().expect("expected Some(type_expr) for let y");
    match &ty.kind {
        TypeExprKind::QualifiedAssoc { base, trait_name, member } => {
            match &base.kind {
                TypeExprKind::Named { name, .. } => assert_eq!(name, "T"),
                other => panic!("expected base Named(T), got {:?}", other),
            }
            assert!(trait_name.is_none(), "expected trait_name = None for bare form");
            assert_eq!(member, "Material");
        }
        other => panic!(
            "expected TypeExprKind::QualifiedAssoc, got {:?}",
            other
        ),
    }
}

// ── (d) Disambiguated FORK-G form: Beam::(HasMaterial::Material) ─────────────
//
// Tests that the parenthesized `(trait :: member)` form lowers to
// `TypeExprKind::QualifiedAssoc` with `trait_name: Some("HasMaterial")`.
// Step-7 (RED) adds this test; step-8 extends the lowering.

#[test]
fn param_disambiguated_qualified_type_lowers_to_qualified_assoc_with_trait_name() {
    let (decls, errors) = parse_decls(
        "structure def UseAssoc { param n : Beam::(HasMaterial::Material) }",
    );
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);

    let s = as_structure(&decls);
    assert_eq!(s.members.len(), 1);

    let p = as_param_member(&s.members[0]);
    assert_eq!(p.name, "n");

    let ty = p.type_expr.as_ref().expect("expected Some(type_expr) for param n");
    match &ty.kind {
        TypeExprKind::QualifiedAssoc { base, trait_name, member } => {
            match &base.kind {
                TypeExprKind::Named { name, .. } => assert_eq!(name, "Beam"),
                other => panic!("expected base Named(Beam), got {:?}", other),
            }
            assert_eq!(
                trait_name.as_deref(),
                Some("HasMaterial"),
                "expected trait_name = Some(\"HasMaterial\")"
            );
            assert_eq!(member, "Material");
        }
        other => panic!(
            "expected TypeExprKind::QualifiedAssoc (disambiguated), got {:?}",
            other
        ),
    }
}
