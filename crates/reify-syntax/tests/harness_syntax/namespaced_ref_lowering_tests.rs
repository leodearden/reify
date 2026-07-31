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

// ── Expression position — the qualified CALL form ───────────────────────────
//
// Step-7 RED: `namespaced_call` has no `lower_expr` dispatch arm, so
// `pp.Pulley()` lowers to nothing.
// Step-8 GREEN: `lower_namespaced_call` emits
// `ExprKind::FunctionCall { name: "pp.Pulley", .. }` — the same variant the
// unqualified path uses, with the qualifier carried in the dot-joined `name`.

/// Unwrap the first member as a `let`, returning its value expression.
fn first_let_value(source: &str) -> Expr {
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let structure = only_structure(&decls);
    match &structure.members[0] {
        MemberDecl::Let(l) => l.value.clone(),
        other => panic!("expected MemberDecl::Let, got {other:?}"),
    }
}

/// Unwrap an `ExprKind::FunctionCall` arm, panicking on mismatch.
fn as_function_call(expr: &Expr) -> (&str, &[Expr], &[Option<String>]) {
    match &expr.kind {
        ExprKind::FunctionCall {
            name,
            args,
            arg_names,
        } => (name.as_str(), args.as_slice(), arg_names.as_slice()),
        other => panic!("expected ExprKind::FunctionCall, got {other:?}"),
    }
}

/// `let f = pp.Pulley()` → `FunctionCall { name: "pp.Pulley", args: [], .. }`.
#[test]
fn nullary_qualified_call_lowers_to_dot_joined_function_call() {
    let value = first_let_value("structure def S { let f = pp.Pulley() }");
    let (name, args, arg_names) = as_function_call(&value);
    assert_eq!(name, "pp.Pulley");
    assert!(args.is_empty(), "expected no args; got {args:?}");
    assert!(arg_names.is_empty());
}

/// `let g = pp.compute(1, scale: 2)` → named/positional handling is at exact
/// parity with the bare `function_call` path (both walk the same shared
/// argument-lowering helper, so they cannot drift).
#[test]
fn qualified_call_named_and_positional_args_at_parity() {
    let value = first_let_value("structure def S { let g = pp.compute(1, scale: 2) }");
    let (name, args, arg_names) = as_function_call(&value);
    assert_eq!(name, "pp.compute");
    assert_eq!(args.len(), 2, "expected two args; got {args:?}");
    assert_eq!(
        arg_names,
        &[None, Some("scale".to_string())],
        "arg_names must be parallel to args, `None` for positional"
    );
}

/// Whitespace normalisation applies to the call form too.
#[test]
fn qualified_call_whitespace_is_normalised() {
    let value = first_let_value("structure def S { let f = pp . Pulley() }");
    let (name, _, _) = as_function_call(&value);
    assert_eq!(name, "pp.Pulley");
}

/// OUT-OF-SCOPE GUARD (D-7 / PRD §9): bare full-path qualification.
///
/// The grammar accepts a 3+-segment callee because `namespaced_call`'s callee
/// is a full `member_access` (restricting it inline would collide with
/// `member_access` as a reduce-reduce ambiguity). Lowering must therefore
/// reject it with a specific diagnostic — NOT panic, and NOT silently fabricate
/// a 3-segment name.
#[test]
fn three_segment_callee_is_rejected_with_a_diagnostic() {
    let (_decls, errors) = parse_decls("structure def S { let h = a.b.c() }");
    assert!(
        !errors.is_empty(),
        "a 3-segment callee must produce a ParseError, not lower silently"
    );
    let joined = errors
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("binding.Name("),
        "the diagnostic must name the supported form `binding.Name(...)`; got: {joined}"
    );
    assert!(
        joined.contains("full-path qualification"),
        "the diagnostic must say full-path qualification is out of scope; got: {joined}"
    );
}

/// The 3-segment rejection must not fabricate a dotted name anywhere in the
/// lowered AST.
#[test]
fn three_segment_callee_fabricates_no_dotted_name() {
    let (decls, _errors) = parse_decls("structure def S { let h = a.b.c() }");
    let rendered = format!("{decls:?}");
    assert!(
        !rendered.contains("a.b.c"),
        "lowering must not fabricate a 3-segment name; got: {rendered}"
    );
}

// ── Expression-position negative controls ───────────────────────────────────

/// `let r = pp.FitClass.Clearance` still lowers to nested `MemberAccess` — the
/// enum-access half of NS-Q2 stays on ν's D-9 resolution-fixup path.
#[test]
fn call_less_dotted_chain_stays_nested_member_access() {
    let value = first_let_value("structure def S { let r = pp.FitClass.Clearance }");
    match &value.kind {
        ExprKind::MemberAccess { object, member } => {
            assert_eq!(member, "Clearance");
            match &object.kind {
                ExprKind::MemberAccess { object, member } => {
                    assert_eq!(member, "FitClass");
                    assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "pp"));
                }
                other => panic!("expected a nested MemberAccess, got {other:?}"),
            }
        }
        other => panic!("expected ExprKind::MemberAccess, got {other:?}"),
    }
}

/// `let s = obj.width` still lowers to `MemberAccess`.
#[test]
fn plain_member_access_lowering_unchanged() {
    let value = first_let_value("structure def S { let s = obj.width }");
    match &value.kind {
        ExprKind::MemberAccess { object, member } => {
            assert_eq!(member, "width");
            assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "obj"));
        }
        other => panic!("expected ExprKind::MemberAccess, got {other:?}"),
    }
}

/// `let t = plain(1)` still lowers to an unqualified `FunctionCall`.
#[test]
fn unqualified_call_lowering_unchanged() {
    let value = first_let_value("structure def S { let t = plain(1) }");
    let (name, args, _) = as_function_call(&value);
    assert_eq!(name, "plain");
    assert_eq!(args.len(), 1);
    assert!(
        !name.contains('.'),
        "an unqualified callee must not acquire a dot — that is ν's discriminator"
    );
}

/// An in-file enum still lowers `Direction.In` to `EnumAccess` — the
/// `known_enums` path in `lower_member_access` is untouched by μ.
#[test]
fn enum_access_lowering_unchanged() {
    let source = "enum Direction { In, Out }\nstructure def S { let d = Direction.In }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let structure = match decls.iter().find(|d| matches!(d, Declaration::Structure(_))) {
        Some(Declaration::Structure(s)) => s,
        _ => panic!("expected a structure declaration in {decls:?}"),
    };
    let value = match &structure.members[0] {
        MemberDecl::Let(l) => &l.value,
        other => panic!("expected MemberDecl::Let, got {other:?}"),
    };
    match &value.kind {
        ExprKind::EnumAccess { type_name, variant } => {
            assert_eq!(type_name, "Direction");
            assert_eq!(variant, "In");
        }
        other => panic!("expected ExprKind::EnumAccess, got {other:?}"),
    }
}
