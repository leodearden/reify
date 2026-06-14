//! Grammar integration tests for `default TypeName = expr` declarations.
//!
//! Task 4496, step-1 (TDD RED): verifies the grammar shape for `default_declaration`
//! after step-2 (grammar edit) lands. Until then, the `default …` forms produce
//! ERROR nodes and fail — that is the intended RED signal.
//!
//! Tests:
//!   (a) top-level `default Material = steel` — fixture file ambient-default-1.ri
//!   (b) purpose-nested `default Material = steel` — fixture file ambient-default-2.ri
//!   (c) inline top-level source — asserts default_declaration node present
//!   (d) inline purpose-nested source — asserts default_declaration node present
//!   (e) identifier-collision regression — `default` in expression position
//!       lexes as an identifier, not a keyword

use tree_sitter_reify::language;

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Walk a tree and collect all node kinds (depth-first, including anonymous nodes).
fn collect_kinds(node: tree_sitter::Node) -> Vec<String> {
    let mut kinds = vec![node.kind().to_string()];
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            kinds.extend(collect_kinds(cursor.node()));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    kinds
}

/// Depth-first search for the first node with the given kind.
fn find_node_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            if let Some(found) = find_node_by_kind(cursor.node(), kind) {
                return Some(found);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

// ── Fixture (a): ambient-default-1.ri — top-level default declaration ────

/// Fixture (a): ambient-default-1.ri parses with no ERROR nodes.
///
/// RED until step-2 grammar change: `default` as a top-level declaration
/// is not yet in the grammar.
#[test]
fn fixture_a_top_level_default_parses_cleanly() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/ambient-default-1.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "ambient-default-1.ri (top-level `default Material = steel`) must parse \
         with no ERROR nodes after grammar change; got node kinds: {kinds:?}"
    );
}

/// Fixture (a): ambient-default-1.ri produces a `default_declaration` node.
///
/// RED until step-2.
#[test]
fn fixture_a_top_level_default_has_default_declaration_node() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/ambient-default-1.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    assert!(
        find_node_by_kind(root, "default_declaration").is_some(),
        "ambient-default-1.ri must contain a `default_declaration` node; \
         got node kinds: {:?}",
        collect_kinds(root)
    );
}

// ── Fixture (b): ambient-default-2.ri — purpose-nested default declaration ─

/// Fixture (b): ambient-default-2.ri parses with no ERROR nodes.
///
/// RED until step-2 grammar change: `default` as a purpose member is not
/// yet in the grammar.
#[test]
fn fixture_b_purpose_nested_default_parses_cleanly() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/ambient-default-2.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "ambient-default-2.ri (purpose-nested `default Material = steel`) must parse \
         with no ERROR nodes after grammar change; got node kinds: {kinds:?}"
    );
}

/// Fixture (b): ambient-default-2.ri produces a `default_declaration` node.
///
/// RED until step-2.
#[test]
fn fixture_b_purpose_nested_default_has_default_declaration_node() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/ambient-default-2.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    assert!(
        find_node_by_kind(root, "default_declaration").is_some(),
        "ambient-default-2.ri must contain a `default_declaration` node; \
         got node kinds: {:?}",
        collect_kinds(root)
    );
}

// ── Inline test (c): top-level default_declaration CST shape ─────────────

/// Inline (c): `default Material = steel` at top level has a `default_declaration`
/// node with `type_expr` and `identifier` children.
///
/// RED until step-2.
#[test]
fn inline_top_level_default_produces_default_declaration_node() {
    let mut parser = make_parser();
    let source = b"default Material = steel";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    let kinds = collect_kinds(root);
    assert!(
        !root.has_error(),
        "inline top-level `default Material = steel` must parse with no ERROR; \
         got node kinds: {kinds:?}"
    );
    assert!(
        find_node_by_kind(root, "default_declaration").is_some(),
        "inline top-level source must contain a `default_declaration` node; \
         got node kinds: {kinds:?}"
    );
    // The type field must wrap in type_expr, and value must be an identifier.
    assert!(
        find_node_by_kind(root, "type_expr").is_some(),
        "default_declaration must contain a `type_expr` child for the type field; \
         got node kinds: {kinds:?}"
    );
}

// ── Inline test (d): purpose-nested default_declaration CST shape ─────────

/// Inline (d): purpose-nested `default Material = steel` has a `default_declaration`
/// node inside a `purpose_member`.
///
/// RED until step-2.
#[test]
fn inline_purpose_nested_default_produces_default_declaration_node() {
    let mut parser = make_parser();
    let source = b"purpose Exploration() { default Material = steel }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    let kinds = collect_kinds(root);
    assert!(
        !root.has_error(),
        "inline purpose-nested `default Material = steel` must parse with no ERROR; \
         got node kinds: {kinds:?}"
    );
    assert!(
        find_node_by_kind(root, "default_declaration").is_some(),
        "inline purpose-nested source must contain a `default_declaration` node; \
         got node kinds: {kinds:?}"
    );
    // Must be inside a purpose_member.
    assert!(
        find_node_by_kind(root, "purpose_member").is_some(),
        "purpose-nested default must be wrapped in a `purpose_member` node; \
         got node kinds: {kinds:?}"
    );
}

// ── Regression (e): `default` in expression position is an identifier ─────

/// Regression (e): `default` as an expression value (non-declaration position)
/// still lexes as an `identifier`, NOT as a keyword producing an ERROR.
///
/// This test pins the contextual-keyword property: tree-sitter only makes
/// `default` a parse candidate at `_declaration` / `purpose_member` starts
/// where `default_declaration` is reachable.
///
/// This test must be GREEN before AND after the grammar change (step-2).
#[test]
fn regression_default_as_identifier_in_expression_position() {
    let mut parser = make_parser();
    // `default` is used as the RHS of a let binding inside a structure body.
    let source = b"structure S { let x = default }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "`default` in expression position must parse as an identifier with no ERROR; \
         got node kinds: {:?}",
        collect_kinds(root)
    );
    // Must NOT produce a default_declaration node here.
    assert!(
        find_node_by_kind(root, "default_declaration").is_none(),
        "`default` in expression position must NOT produce a `default_declaration` node; \
         got node kinds: {:?}",
        collect_kinds(root)
    );
}
