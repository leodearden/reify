//! CST→AST lowering tests for qualified references through an import binding
//! (`pp.Pulley`) — task 5495 μ, PRD `docs/prds/v0_6/stdlib-namespace.md`
//! §3.3 NS-Q2 / D-7.
//!
//! **Encoding contract pinned here.** μ carries the qualified path as a
//! DOT-JOINED string in the EXISTING `String` name slots — `TypeExprKind::Named
//! { name }`, `SubDecl::structure_name`, `ExprKind::FunctionCall { name }` — and
//! introduces NO new `TypeExprKind` / `ExprKind` variant.  `.` is not a legal
//! identifier character, so `name.contains('.')` is an unambiguous discriminator
//! for the resolution-phase fixup (task ν, PRD §3.3 NS-Q1/Q3).  This mirrors
//! resolution-unification D-9: the parse leaves an under-specified form and
//! resolution rewrites it, exactly as `Foo.Bar` enum access already does.  It is
//! also this AST's existing convention for module paths (`ImportDecl::path`).
//!
//! Step-5 RED: `lower_type_expr_node` and `lower_sub` produce the right string
//! only by ACCIDENT (a `node_text` fallback), which is why
//! `qualified_type_whitespace_is_normalised` — `pp . Pulley` → `"pp.Pulley"` —
//! is here: it fails against raw source text and forces the explicit,
//! field-joined branch that step-6 adds.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("namespaced_test"));
    (module.declarations, module.errors)
}

/// Unwrap the single structure declaration.
fn only_structure(decls: &[Declaration]) -> &StructureDef {
    match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Declaration::Structure, got {other:?}"),
    }
}

/// Unwrap the first member as a `param`, returning its type annotation.
fn first_param_type(source: &str) -> TypeExpr {
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let structure = only_structure(&decls);
    match &structure.members[0] {
        MemberDecl::Param(p) => p
            .type_expr
            .clone()
            .unwrap_or_else(|| panic!("param has no type annotation in `{source}`")),
        other => panic!("expected MemberDecl::Param, got {other:?}"),
    }
}

/// Unwrap the first member as a `sub`.
fn first_sub(source: &str) -> SubDecl {
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let structure = only_structure(&decls);
    match &structure.members[0] {
        MemberDecl::Sub(s) => s.clone(),
        other => panic!("expected MemberDecl::Sub, got {other:?}"),
    }
}

/// Unwrap a `TypeExprKind::Named` arm, panicking on mismatch.
fn as_named(te: &TypeExpr) -> (&str, &[TypeExpr]) {
    match &te.kind {
        TypeExprKind::Named { name, type_args } => (name.as_str(), type_args.as_slice()),
        other => panic!("expected TypeExprKind::Named, got {other:?}"),
    }
}

// ── Type position ───────────────────────────────────────────────────────────

/// `param p : pp.Pulley` → `Named { name: "pp.Pulley", type_args: [] }`.
///
/// No new `TypeExprKind` variant: the qualifier rides in the existing `name`
/// slot for ν's `name.contains('.')` fixup.
#[test]
fn qualified_type_lowers_to_dot_joined_named() {
    let te = first_param_type("structure def S { param p : pp.Pulley }");
    let (name, type_args) = as_named(&te);
    assert_eq!(name, "pp.Pulley");
    assert!(
        type_args.is_empty(),
        "a qualified type carries no type args in μ (qualified generics are out of scope); \
         got {type_args:?}"
    );
}

/// `param q : List<pp.Pulley>` → the qualified name nests as a type ARGUMENT,
/// unchanged in form.
#[test]
fn qualified_type_as_type_argument() {
    let te = first_param_type("structure def S { param q : List<pp.Pulley> }");
    let (name, type_args) = as_named(&te);
    assert_eq!(name, "List");
    assert_eq!(type_args.len(), 1, "expected one type arg; got {type_args:?}");
    let (inner, inner_args) = as_named(&type_args[0]);
    assert_eq!(inner, "pp.Pulley");
    assert!(inner_args.is_empty());
}

/// `param r : pp . Pulley` → `"pp.Pulley"`, NOT the raw source text
/// `"pp . Pulley"`.
///
/// This is the assertion that forces an explicit, field-joined lowering branch
/// rather than relying on the incidental `node_text` fallback: ν's
/// `name.contains('.')` discriminator would otherwise have to cope with
/// arbitrary interior whitespace.
#[test]
fn qualified_type_whitespace_is_normalised() {
    let te = first_param_type("structure def S { param r : pp . Pulley }");
    let (name, _) = as_named(&te);
    assert_eq!(
        name, "pp.Pulley",
        "the dotted name must be joined from the `binding`/`name` CST fields, \
         not read as raw source text"
    );
}

/// Negative control: an unqualified type annotation is unchanged.
#[test]
fn bare_type_still_lowers_to_plain_named() {
    let te = first_param_type("structure def S { param s : Steel }");
    let (name, type_args) = as_named(&te);
    assert_eq!(name, "Steel");
    assert!(type_args.is_empty());
    assert!(
        !name.contains('.'),
        "an unqualified name must not acquire a dot — that is ν's discriminator"
    );
}

// ── `sub` structure_name — all three grammar arms ───────────────────────────

/// Instantiation arm: `sub p = pp.Pulley()` → `structure_name == "pp.Pulley"`.
#[test]
fn sub_instantiation_lowers_dot_joined_structure_name() {
    let sub = first_sub("structure def S { sub p = pp.Pulley() }");
    assert_eq!(sub.structure_name, "pp.Pulley");
    assert!(!sub.is_collection);
}

/// Specialization arm: `sub h : pp.Pulley` → `structure_name == "pp.Pulley"`.
#[test]
fn sub_specialization_lowers_dot_joined_structure_name() {
    let sub = first_sub("structure def S { sub h : pp.Pulley }");
    assert_eq!(sub.structure_name, "pp.Pulley");
    assert!(!sub.is_collection);
}

/// Collection arm: `sub i : List<pp.Pulley>` → `structure_name == "pp.Pulley"`
/// with `is_collection == true` (the `List` keyword is still consumed as the
/// collection marker, not as the structure name).
#[test]
fn sub_collection_lowers_dot_joined_structure_name() {
    let sub = first_sub("structure def S { sub i : List<pp.Pulley> }");
    assert_eq!(sub.structure_name, "pp.Pulley");
    assert!(
        sub.is_collection,
        "`List<...>` must still lower to the collection form"
    );
}

/// Whitespace normalisation applies at `sub` position too.
#[test]
fn sub_structure_name_whitespace_is_normalised() {
    let sub = first_sub("structure def S { sub k = pp . Pulley() }");
    assert_eq!(sub.structure_name, "pp.Pulley");
}

/// Negative control: an unqualified structure name is unchanged.
#[test]
fn bare_sub_structure_name_unchanged() {
    let sub = first_sub("structure def S { sub j = Plain() }");
    assert_eq!(sub.structure_name, "Plain");
    assert!(!sub.structure_name.contains('.'));
}
