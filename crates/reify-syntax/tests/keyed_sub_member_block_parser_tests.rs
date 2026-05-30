//! RED tests for the `sub xs : Keyed<T> { "k" => { overrides } }` grammar production (task 3929).
//!
//! Step-1 (CST shape) tests FAIL until step-2 lands the grammar change.
//! Step-3 (AST lowering) tests are added separately and FAIL until step-4.
//! No-regression pins for List<Foo>, specialization body, empty brace, and
//! map literals are GREEN from the start and serve as guard rails.
//!
//! User-observable signal:
//!   cargo test -p reify-syntax -- keyed_sub_member_block

use reify_core::ModulePath;

mod common;
use common::{find_cst_node, make_ts_parser};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Collect all named children of `node` with the given `kind`.
fn named_children_of_kind<'tree>(
    node: tree_sitter::Node<'tree>,
    kind: &str,
) -> Vec<tree_sitter::Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| c.kind() == kind)
        .collect()
}

// ── CST-shape tests (step-1 RED) ─────────────────────────────────────────────

/// PRIMARY SIGNAL: a keyed sub-member block parses without any ERROR nodes.
///
/// Fails today because `"intake" =>` inside a specialization body produces an
/// ERROR node (the grammar has no keyed production yet).
#[test]
fn keyed_sub_member_block_parses_without_error() {
    let source = r#"structure S {
        sub vents : Keyed<Vent> {
            "intake"  => { area = 5mm }
            "exhaust" => { area = 8mm }
        }
    }"#;

    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("tree-sitter parse failed");

    assert!(
        !tree.root_node().has_error(),
        "expected NO CST ERROR nodes for keyed sub-member block; \
         has_error() returned true.\n\
         This test is RED until the grammar change (step-2) lands.\n\
         Source: {source:?}",
    );
}

/// CST structure: the sub_declaration body must be a `keyed_member_block` node.
///
/// Fails today because the grammar has no `keyed_member_block` production.
#[test]
fn keyed_sub_body_is_keyed_member_block_node() {
    let source = r#"structure S {
        sub vents : Keyed<Vent> {
            "intake"  => { area = 5mm }
            "exhaust" => { area = 8mm }
        }
    }"#;

    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("tree-sitter parse failed");

    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node in the CST");

    let body = sub_decl
        .child_by_field_name("body")
        .expect("sub_declaration must have a `body` field for keyed form");

    assert_eq!(
        body.kind(),
        "keyed_member_block",
        "body child must be of kind 'keyed_member_block', got: {:?}",
        body.kind(),
    );
}

/// CST structure: the keyed_member_block must contain exactly 2 keyed_member_entry children.
///
/// Fails today because the grammar has no `keyed_member_block` production.
#[test]
fn keyed_sub_member_block_has_two_entries() {
    let source = r#"structure S {
        sub vents : Keyed<Vent> {
            "intake"  => { area = 5mm }
            "exhaust" => { area = 8mm }
        }
    }"#;

    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("tree-sitter parse failed");

    let block = find_cst_node(tree.root_node(), "keyed_member_block")
        .expect("expected a keyed_member_block node in the CST");

    let entries = named_children_of_kind(block, "keyed_member_entry");
    assert_eq!(
        entries.len(),
        2,
        "keyed_member_block must have exactly 2 keyed_member_entry children, got: {}",
        entries.len(),
    );
}

/// CST structure: each keyed_member_entry has a `key` field (string_literal) and an
/// `overrides` field (specialization_body).
///
/// Fails today because the grammar has no `keyed_member_entry` production.
#[test]
fn keyed_sub_member_entries_have_key_and_overrides_fields() {
    let source = r#"structure S {
        sub vents : Keyed<Vent> {
            "intake"  => { area = 5mm }
            "exhaust" => { area = 8mm }
        }
    }"#;

    let expected_keys = ["\"intake\"", "\"exhaust\""];

    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("tree-sitter parse failed");

    let block = find_cst_node(tree.root_node(), "keyed_member_block")
        .expect("expected a keyed_member_block node in the CST");

    let entries = named_children_of_kind(block, "keyed_member_entry");
    assert_eq!(entries.len(), 2, "expected 2 entries");

    for (i, entry) in entries.iter().enumerate() {
        let key_node = entry
            .child_by_field_name("key")
            .unwrap_or_else(|| panic!("entry[{i}] must have a `key` field"));
        assert_eq!(
            key_node.kind(),
            "string_literal",
            "entry[{i}] key must be of kind 'string_literal', got: {:?}",
            key_node.kind(),
        );
        let key_text = key_node
            .utf8_text(source.as_bytes())
            .expect("key node must be valid utf8");
        assert_eq!(
            key_text, expected_keys[i],
            "entry[{i}] key text must be {:?}, got: {key_text:?}",
            expected_keys[i],
        );

        let overrides_node = entry
            .child_by_field_name("overrides")
            .unwrap_or_else(|| panic!("entry[{i}] must have an `overrides` field"));
        assert_eq!(
            overrides_node.kind(),
            "specialization_body",
            "entry[{i}] overrides must be of kind 'specialization_body', got: {:?}",
            overrides_node.kind(),
        );
    }
}

// ── No-regression pins (GREEN from the start) ────────────────────────────────

/// Regression: `sub xs : List<Foo>` still parses cleanly (collection arm unchanged).
#[test]
fn regression_collection_form_still_parses() {
    let source = "structure S { sub xs : List<Foo> }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("tree-sitter parse failed");
    assert!(
        !tree.root_node().has_error(),
        "REGRESSION: `sub xs : List<Foo>` must parse without CST ERROR nodes",
    );
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "REGRESSION: `sub xs : List<Foo>` must have zero parse errors, got: {:?}",
        module.errors,
    );
}

/// Regression: `sub n : Foo { let a = 1mm }` (specialization body) still parses cleanly.
#[test]
fn regression_specialization_body_still_parses() {
    let source = "structure S { sub n : Foo { let a = 1mm } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("tree-sitter parse failed");
    assert!(
        !tree.root_node().has_error(),
        "REGRESSION: specialization body must parse without CST ERROR nodes",
    );
    // The body should still be a specialization_body, not keyed_member_block.
    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let body = sub_decl
        .child_by_field_name("body")
        .expect("sub_declaration must have a body field");
    assert_eq!(
        body.kind(),
        "specialization_body",
        "REGRESSION: `sub n : Foo {{ let a = 1mm }}` body must still be 'specialization_body', got: {:?}",
        body.kind(),
    );
}

/// Regression: empty `sub n : Foo {}` still parses cleanly as specialization_body (not keyed).
#[test]
fn regression_empty_brace_body_is_specialization_body() {
    let source = "structure S { sub n : Foo {} }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("tree-sitter parse failed");
    assert!(
        !tree.root_node().has_error(),
        "REGRESSION: empty brace body must parse without CST ERROR nodes",
    );
    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let body = sub_decl
        .child_by_field_name("body")
        .expect("sub_declaration must have a body field for empty brace form");
    assert_eq!(
        body.kind(),
        "specialization_body",
        "REGRESSION: empty `sub n : Foo {{}}` body must be 'specialization_body', \
         got: {:?}. `repeat1` in keyed_member_block ensures `{{}}` is unambiguously \
         specialization_body.",
        body.kind(),
    );
}

/// Regression: `map{{ "k" => v }}` literal still parses cleanly (map_entry `=>` unchanged).
#[test]
fn regression_map_literal_still_parses() {
    let source = "structure S { let m = map{ \"k\" => 1 } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("tree-sitter parse failed");
    assert!(
        !tree.root_node().has_error(),
        "REGRESSION: `map{{ \"k\" => 1 }}` must parse without CST ERROR nodes",
    );
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "REGRESSION: `map{{ \"k\" => 1 }}` must have zero parse errors, got: {:?}",
        module.errors,
    );
}
