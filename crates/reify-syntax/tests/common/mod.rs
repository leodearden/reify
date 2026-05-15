//! Shared tree-sitter CST helpers for `reify-syntax` integration-test binaries.
//!
//! Include in a test binary with `mod common;` at the top of the file.
//! Helpers are `pub` so they are visible after `use common::{...}`.
//!
//! Helpers:
//! - [`make_ts_parser`] — build a tree-sitter parser loaded with the Reify grammar
//! - [`find_cst_node`] — depth-first search for the first node of a given kind
//! - [`find_outermost_cst_nodes`] — depth-first search for all outermost nodes of a given kind

// Not every test binary that includes `mod common;` uses every helper;
// suppress the resulting dead-code warnings for the entire module.
#![allow(dead_code)]

/// Build a tree-sitter parser loaded with the Reify grammar.
pub fn make_ts_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Depth-first search — returns the first node with the given kind.
pub fn find_cst_node<'a>(root: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    if root.kind() == kind {
        return Some(root);
    }
    let mut cursor = root.walk();
    if cursor.goto_first_child() {
        loop {
            if let Some(found) = find_cst_node(cursor.node(), kind) {
                return Some(found);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

/// Depth-first search — returns all **outermost** nodes with the given kind.
///
/// **No-nesting precondition**: when a matching node is found, the search does
/// not recurse into its children.  This is correct for node kinds that cannot
/// legitimately nest (e.g. `auto_type_arg`), but is a footgun for kinds that
/// can (e.g. `type_expr`).  Only call this helper for non-nesting node kinds.
pub fn find_outermost_cst_nodes<'a>(
    root: tree_sitter::Node<'a>,
    kind: &str,
) -> Vec<tree_sitter::Node<'a>> {
    let mut results = Vec::new();
    if root.kind() == kind {
        results.push(root);
        // Do not descend — children of a matching node are not separate
        // top-level occurrences of the same kind.
        return results;
    }
    let mut cursor = root.walk();
    if cursor.goto_first_child() {
        loop {
            results.extend(find_outermost_cst_nodes(cursor.node(), kind));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    results
}
