//! Tree-sitter grammar for the Reify CAD language.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_reify() -> *const ();
}

/// Returns the tree-sitter [`LanguageFn`] for Reify.
pub fn language() -> LanguageFn {
    unsafe { LanguageFn::from_raw(tree_sitter_reify) }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn find_node_by_kind<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
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

    #[test]
    fn can_load_grammar() {
        make_parser();
    }

    #[test]
    fn test_line_comment_parsed() {
        let mut parser = make_parser();
        let source = b"// this is a line comment\nstructure S {}";
        let tree = parser.parse(source, None).expect("parse failed");
        let kinds = collect_kinds(tree.root_node());
        assert!(
            kinds.contains(&"line_comment".to_string()),
            "expected a line_comment node in the parse tree, got: {kinds:?}"
        );
        assert!(
            !tree.root_node().has_error(),
            "unexpected parse error in source with // comment"
        );
    }

    #[test]
    fn test_block_comment_parsed() {
        let mut parser = make_parser();
        let source = b"structure S { /* a block comment */ param x : Float }";
        let tree = parser.parse(source, None).expect("parse failed");
        let kinds = collect_kinds(tree.root_node());
        assert!(
            kinds.contains(&"block_comment".to_string()),
            "expected a block_comment node in the parse tree, got: {kinds:?}"
        );
        assert!(
            !tree.root_node().has_error(),
            "unexpected parse error in source with /* */ comment"
        );
    }

    #[test]
    fn test_forall_statement_with_connect() {
        let mut parser = make_parser();
        let source =
            b"structure S { forall v in vents: connect v.inlet -> housing.air_channel }";
        let tree = parser.parse(source, None).expect("parse failed");
        let kinds = collect_kinds(tree.root_node());

        // (a) No parse errors
        assert!(
            !tree.root_node().has_error(),
            "unexpected parse error in forall-connect source: {kinds:?}"
        );

        // (b) Both forall_statement and connect_statement nodes must be present
        assert!(
            kinds.contains(&"forall_statement".to_string()),
            "expected forall_statement node, got: {kinds:?}"
        );
        assert!(
            kinds.contains(&"connect_statement".to_string()),
            "expected connect_statement node, got: {kinds:?}"
        );

        // (c) The forall_statement's body field must be a connect_statement
        let forall_node = find_node_by_kind(tree.root_node(), "forall_statement")
            .expect("forall_statement not found in tree");
        let _variable = forall_node
            .child_by_field_name("variable")
            .expect("forall_statement missing 'variable' field");
        let _collection = forall_node
            .child_by_field_name("collection")
            .expect("forall_statement missing 'collection' field");
        let body = forall_node
            .child_by_field_name("body")
            .expect("forall_statement missing 'body' field");
        assert_eq!(
            body.kind(),
            "connect_statement",
            "expected body to be connect_statement, got: {}",
            body.kind()
        );
    }

    #[test]
    fn test_hash_not_parsed_as_comment() {
        let mut parser = make_parser();
        // # is not a valid comment in Reify; it should produce an ERROR node
        let source = b"# not a comment\nstructure S {}";
        let tree = parser.parse(source, None).expect("parse failed");
        let kinds = collect_kinds(tree.root_node());
        assert!(
            tree.root_node().has_error(),
            "expected an ERROR node when # is used (not a valid comment), got: {kinds:?}"
        );
        assert!(
            !kinds.contains(&"line_comment".to_string()),
            "# should not produce a line_comment node, got: {kinds:?}"
        );
    }
}
