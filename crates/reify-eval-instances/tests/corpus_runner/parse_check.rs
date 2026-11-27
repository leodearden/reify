//! Whether a corpus source parses, meaning what the tracked-`.ri` parse
//! guard means by a clean file (crates/reify-syntax/tests/harness_syntax/
//! tracked_ri_parse_guard.rs, `Expectation::Clean`): no ERROR or MISSING
//! node in the tree-sitter tree, and no error from `reify_syntax::parse`.
//! Both are asked because the lowering can drop a tree fault silently.
//! The guard does not sweep the corpus, so this is the corpus's parse check.

use reify_core::ModulePath;
use reify_test_support::bare_scalar::line_has_bare_scalar;
use tree_sitter::Node;

use crate::bookkeeping::Built;
use crate::needs::Need;

/// The first fault that keeps `source` from parsing, or `None` if it parses.
pub fn first_parse_fault(source: &str) -> Option<String> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("the Reify grammar loads");
    let tree = parser
        .parse(source, None)
        .expect("tree-sitter returns a tree for any source");
    if let Some(fault) = first_tree_fault(tree.root_node()) {
        return Some(fault);
    }
    let parsed = reify_syntax::parse(source, ModulePath::single("corpus_entry"));
    parsed.render_errors(source).into_iter().next()
}

/// The first ERROR or MISSING node in pre-order, as `<kind> at <line>:<column>`.
fn first_tree_fault(node: Node<'_>) -> Option<String> {
    if node.is_error() || node.is_missing() {
        let start = node.start_position();
        let kind = if node.is_missing() {
            format!("MISSING {}", node.kind())
        } else {
            "ERROR".to_string()
        };
        return Some(format!("{kind} at {}:{}", start.row + 1, start.column + 1));
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.has_error())
        .find_map(first_tree_fault)
}

/// A source must parse unless its entry applies a purpose and the purpose
/// grammar is not built; then it must not.
pub fn expected_to_parse(needs: &[Need], built: &Built) -> bool {
    !needs.contains(&Need::GrammarPurpose) || built.contains(Need::GrammarPurpose)
}

/// The 1-based numbers of the lines with a bare `Scalar`.
pub fn bare_scalar_lines(source: &str) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| line_has_bare_scalar(line))
        .map(|(index, _)| index + 1)
        .collect()
}
