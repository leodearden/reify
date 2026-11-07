//! Trait associated-fn member lowering round-trip tests.
//!
//! Step 1 (RED): these tests fail to compile until step-2 adds:
//!   - `MemberDecl::Fn(FnDef)` variant
//!   - `FnDef.body: Option<FnBody>`
//!   - `FnParam.is_self: bool`

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("trait_fn_test"));
    (module.declarations, module.errors)
}

/// Helper: unwrap a `Declaration::Trait` from the first declaration.
fn as_trait(decls: &[Declaration]) -> &TraitDecl {
    match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    }
}

/// Helper: unwrap `MemberDecl::Fn` from a member.
fn as_fn(member: &MemberDecl) -> &FnDef {
    match member {
        MemberDecl::Fn(f) => f,
        other => panic!("expected MemberDecl::Fn, got {:?}", other),
    }
}

// ── Test 1: trait-body fn with body and self receiver ─────────────────────

#[test]
fn trait_body_fn_with_self_has_body_and_self_param() {
    let (decls, errors) =
        parse_decls("trait C { fn area(self) -> Scalar { diameter } }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let t = as_trait(&decls);
    assert_eq!(t.members.len(), 1);

    let fndef = as_fn(&t.members[0]);
    assert_eq!(fndef.name, "area");
    assert!(fndef.body.is_some(), "expected body to be Some");
    assert!(fndef.return_type.is_some(), "expected return_type to be Some");
    assert_eq!(fndef.params.len(), 1);
    assert!(
        fndef.params[0].is_self,
        "expected params[0].is_self == true"
    );
}

// ── Test 2: bodyless (required) fn with self receiver ─────────────────────

#[test]
fn trait_body_fn_bodyless_has_none_body() {
    let (decls, errors) = parse_decls("trait C { fn req(self) -> Real }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let t = as_trait(&decls);
    assert_eq!(t.members.len(), 1);

    let fndef = as_fn(&t.members[0]);
    assert_eq!(fndef.name, "req");
    assert!(fndef.body.is_none(), "expected body to be None (bodyless)");
    assert_eq!(fndef.params.len(), 1);
    assert!(
        fndef.params[0].is_self,
        "expected params[0].is_self == true"
    );
}

// ── Test 3: trait-static fn (no self) with body ───────────────────────────

#[test]
fn trait_static_fn_no_self_has_empty_params() {
    let (decls, errors) = parse_decls("trait C { fn make() -> Real { 1.0 } }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let t = as_trait(&decls);
    assert_eq!(t.members.len(), 1);

    let fndef = as_fn(&t.members[0]);
    assert_eq!(fndef.name, "make");
    assert!(fndef.body.is_some(), "expected body to be Some");
    // Static fn: no self receiver → empty params
    assert!(
        fndef.params.is_empty(),
        "expected no params for static fn, got {:?}",
        fndef.params
    );
}

// ── Test 4: typed param after self ────────────────────────────────────────

#[test]
fn trait_fn_typed_param_after_self() {
    let (decls, errors) =
        parse_decls("trait C { fn f(self, x: Length) -> Real { x } }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let t = as_trait(&decls);
    assert_eq!(t.members.len(), 1);

    let fndef = as_fn(&t.members[0]);
    assert_eq!(fndef.name, "f");
    assert_eq!(fndef.params.len(), 2);
    assert!(fndef.params[0].is_self, "params[0].is_self should be true");
    assert_eq!(fndef.params[1].name, "x");
    assert!(!fndef.params[1].is_self, "params[1].is_self should be false");
}
