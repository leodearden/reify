//! Grammar integration tests for `annotation` nodes inside `trait_member`.
//!
//! Task #4683, step-1 (TDD RED): verifies that a trait body containing an
//! `@deprecated(...)` annotation on a `fn` declaration parses without an
//! ERROR node.  RED today because `trait_member` in grammar.js does not yet
//! include `$.annotation` as a choice.  GREEN after the step-2 grammar edit.
//!
//! Baseline (a) is GREEN before and after and acts as a regression guard.

use tree_sitter_reify::language;

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
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

// ── Regression baseline (GREEN before and after grammar change) ───────────────

/// Baseline (a): a plain un-annotated trait with a static fn parses cleanly.
/// Must remain GREEN throughout all steps.
#[test]
fn baseline_plain_trait_fn_parses() {
    let mut parser = make_parser();
    let source = b"trait Factory { fn make_item() -> Real { 1.0 } }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "plain trait fn must parse cleanly (regression baseline)"
    );
}

// ── Step-1 RED fixtures (GREEN after step-2 grammar change) ──────────────────

/// Fixture (b): a trait body with `@deprecated(\"...\") fn ...` must parse with
/// no ERROR node and must contain an `annotation` node inside the
/// `trait_declaration`.
///
/// RED today: `trait_member` does not include `$.annotation` as a choice,
/// so the annotation causes an ERROR node in the parse tree.
/// GREEN after the grammar.js edit in step-2.
#[test]
fn trait_fn_with_deprecated_annotation_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"trait Factory { @deprecated(\"use Factory2\") fn make_old() -> Real { 1.0 } }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "trait body with @deprecated fn must parse cleanly after grammar change; \
         root node: {}",
        root.to_sexp()
    );
}

/// Fixture (c): an `annotation` node must be present inside the
/// `trait_declaration` after step-2's grammar change.
///
/// RED today: without the grammar change the annotation causes a parse ERROR
/// rather than an `annotation` CST node.
/// GREEN after the grammar.js edit in step-2.
#[test]
fn trait_fn_with_deprecated_annotation_has_annotation_node() {
    let mut parser = make_parser();
    let source = b"trait Factory { @deprecated(\"use Factory2\") fn make_old() -> Real { 1.0 } }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    // The trait_declaration must contain an annotation node.
    let trait_decl = find_node_by_kind(root, "trait_declaration")
        .expect("trait_declaration not found in parse tree");
    assert!(
        find_node_by_kind(trait_decl, "annotation").is_some(),
        "trait_declaration must contain an annotation node after grammar change; \
         tree: {}",
        root.to_sexp()
    );
}
