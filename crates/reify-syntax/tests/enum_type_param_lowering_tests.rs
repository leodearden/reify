//! Lowering tests for type parameters on enum declarations (task 4029 α).
//!
//! step-5 RED: `lower_enum` in ts_parser.rs still sets the step-4 placeholder
//! `type_params: vec![]`, so assertions (a)/(b)/(c)/(e) below fail because
//! the returned EnumDecl.type_params is always empty.
//!
//! step-6 GREEN: replace the placeholder with
//! `self.lower_type_parameters(node)` — identical to lower_structure.

use reify_ast::{Declaration, TypeExprKind, VariantPayload};
use reify_core::ModulePath;

fn parse_enum(src: &str) -> reify_ast::EnumDecl {
    let module = reify_syntax::parse(src, ModulePath::single("test_gde"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors; got: {:?}",
        module.errors
    );
    match module.declarations.into_iter().next() {
        Some(Declaration::Enum(e)) => e,
        other => panic!("expected Enum declaration, got: {:?}", other),
    }
}

// ── (a) Simple single type-param ─────────────────────────────────────────────

/// `enum Maybe<T> { Nothing, Just }` lowers to EnumDecl with one type_param "T"
/// (no bounds, no default).
///
/// RED: placeholder `type_params: vec![]` — type_params is empty.
/// GREEN (step-6): `self.lower_type_parameters(node)` populates it.
#[test]
fn maybe_t_lowers_to_one_type_param_named_t() {
    let e = parse_enum("enum Maybe<T> { Nothing, Just }");
    assert_eq!(e.name, "Maybe");
    assert_eq!(
        e.type_params.len(),
        1,
        "Maybe<T> must have 1 type_param; got {:?}",
        e.type_params.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert_eq!(e.type_params[0].name, "T", "type_param name must be 'T'");
    assert!(
        e.type_params[0].bounds.is_empty(),
        "T must have no bounds"
    );
    assert!(
        e.type_params[0].default.is_none(),
        "T must have no default"
    );
}

// ── (b) Two type-params + named-field payload referencing params ──────────────

/// `enum Result<T, E> { Ok { value: T }, Err { error: E } }` lowers to:
///   - type_params names ["T", "E"]
///   - Ok variant has Named payload; `value` field is TypeExprKind::Named { name: "T", type_args: [] }
///
/// RED: placeholder returns empty type_params.
#[test]
fn result_te_lowers_to_two_type_params_and_payload_fields() {
    let e = parse_enum("enum Result<T, E> { Ok { value: T }, Err { error: E } }");
    assert_eq!(e.name, "Result");

    // Two type params in source order.
    assert_eq!(
        e.type_params.len(),
        2,
        "Result<T, E> must have 2 type_params; got {:?}",
        e.type_params.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert_eq!(e.type_params[0].name, "T");
    assert_eq!(e.type_params[1].name, "E");

    // Ok variant payload field 'value' must be TypeExprKind::Named { name: "T" }.
    let ok_variant = e
        .variants
        .iter()
        .find(|v| v.name == "Ok")
        .expect("expected Ok variant");
    match &ok_variant.payload {
        VariantPayload::Named(fields) => {
            assert_eq!(fields.len(), 1, "Ok must have 1 field");
            assert_eq!(fields[0].0, "value");
            match &fields[0].1.kind {
                TypeExprKind::Named { name, type_args } => {
                    assert_eq!(name, "T", "value field type must be 'T'");
                    assert!(type_args.is_empty(), "T must have no type_args");
                }
                other => panic!("expected Named TypeExpr for value, got {:?}", other),
            }
        }
        other => panic!("Ok must have Named payload, got {:?}", other),
    }
}

// ── (c) Recursive form — parameterized_type payload ──────────────────────────

/// `enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }`
///
/// Node's `left` field must lower to:
///   TypeExprKind::Named { name: "Tree", type_args: [Named("T")] }
///
/// RED: placeholder — type_params is empty (but payload lowering itself is fine
/// once grammar is correct; this also checks the payload via independent path).
#[test]
fn tree_t_recursive_node_left_is_parameterized_type() {
    let e = parse_enum(
        "enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }",
    );
    assert_eq!(e.name, "Tree");

    // type_params must be ["T"].
    assert_eq!(
        e.type_params.len(),
        1,
        "Tree<T> must have 1 type_param; got {:?}",
        e.type_params.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert_eq!(e.type_params[0].name, "T");

    // Node variant's `left` field: Tree<T>.
    let node_variant = e
        .variants
        .iter()
        .find(|v| v.name == "Node")
        .expect("expected Node variant");
    match &node_variant.payload {
        VariantPayload::Named(fields) => {
            assert!(!fields.is_empty(), "Node must have at least 1 field");
            let left_field = fields.iter().find(|(n, _)| n == "left").expect("Node must have 'left' field");
            match &left_field.1.kind {
                TypeExprKind::Named { name, type_args } => {
                    assert_eq!(name, "Tree", "left field type must be 'Tree'");
                    assert_eq!(type_args.len(), 1, "Tree<T> must have 1 type_arg");
                    match &type_args[0].kind {
                        TypeExprKind::Named { name: inner_name, .. } => {
                            assert_eq!(inner_name, "T", "Tree's type_arg must be 'T'");
                        }
                        other => panic!("expected Named TypeExpr for T inside Tree<T>, got {:?}", other),
                    }
                }
                other => panic!(
                    "expected Named TypeExpr for left: Tree<T>, got {:?}",
                    other
                ),
            }
        }
        other => panic!("Node must have Named payload, got {:?}", other),
    }
}

// ── (d) INV-6 regression: bare enum → empty type_params ──────────────────────

/// `enum Dir { In, Out }` (non-generic) must lower to EnumDecl with empty type_params.
///
/// This must stay GREEN before and after step-6 (invariant INV-6).
#[test]
fn bare_enum_lowers_to_empty_type_params() {
    let e = parse_enum("enum Dir { In, Out }");
    assert_eq!(e.name, "Dir");
    assert!(
        e.type_params.is_empty(),
        "non-generic enum must have empty type_params; got {:?}",
        e.type_params.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

// ── (e) Bound form — type_param with a trait bound ───────────────────────────

/// `enum E<T: Numeric> { V }` lowers type_params[0].bounds == ["Numeric"].
///
/// RED: placeholder — type_params is empty.
/// GREEN (step-6): lower_type_parameters handles bounds.
#[test]
fn enum_with_bounded_type_param_lowers_bounds() {
    let e = parse_enum("enum E<T: Numeric> { V }");
    assert_eq!(
        e.type_params.len(),
        1,
        "E<T: Numeric> must have 1 type_param; got {:?}",
        e.type_params.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert_eq!(e.type_params[0].name, "T");
    assert_eq!(
        e.type_params[0].bounds,
        vec!["Numeric".to_string()],
        "T must have bound 'Numeric'"
    );
}
