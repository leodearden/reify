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
        let source = b"structure S { forall v in vents: connect v.inlet -> housing.air_channel }";
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
    fn test_forall_expression_form_unchanged() {
        let mut parser = make_parser();
        // The expression-form must still be parsed as quantifier_expression
        // nested inside a constraint_declaration — not as a forall_statement.
        let source = b"structure S { let items = [1, 2, 3] constraint forall x in items: x > 0 }";
        let tree = parser.parse(source, None).expect("parse failed");
        let kinds = collect_kinds(tree.root_node());

        // (a) No parse errors
        assert!(
            !tree.root_node().has_error(),
            "unexpected parse error in forall expression-form source: {kinds:?}"
        );

        // (b) quantifier_expression must appear
        assert!(
            kinds.contains(&"quantifier_expression".to_string()),
            "expected quantifier_expression node, got: {kinds:?}"
        );

        // (c) forall_statement must NOT appear
        assert!(
            !kinds.contains(&"forall_statement".to_string()),
            "forall_statement should not appear for expression-form, got: {kinds:?}"
        );

        // (d) quantifier_expression must be nested inside constraint_declaration
        let constraint_node = find_node_by_kind(tree.root_node(), "constraint_declaration")
            .expect("constraint_declaration not found in tree");
        let expr_field = constraint_node
            .child_by_field_name("expr")
            .expect("constraint_declaration missing 'expr' field");
        assert_eq!(
            expr_field.kind(),
            "quantifier_expression",
            "expected constraint_declaration.expr to be quantifier_expression, got: {}",
            expr_field.kind()
        );
    }

    /// PRD §3.1 contiguity invariant, §7 negative fixture:
    /// "5 kg" (space between number and unit) must NOT fuse into a quantity_literal.
    /// The external scanner (_unit_expr_start) must refuse to emit the boundary token
    /// when whitespace precedes the unit name.
    #[test]
    fn test_space_after_number_breaks_quantity_literal() {
        let mut parser = make_parser();
        // Byte layout of "structure S { let x = 5 kg + 0 }":
        //   '5' is at byte 22, space at byte 23, 'k' at byte 24.
        // A whitespace-blind scanner would fuse them: quantity_literal(22..26).
        // Correct behavior: no quantity_literal spans both byte 22 ('5') and byte 24 ('k').
        let source = b"structure S { let x = 5 kg + 0 }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();

        // Positive assertion: the source must produce a number_literal for the '5'
        // at byte 22. Without this, the test would pass silently if parsing failed
        // catastrophically or if number_literal were renamed — giving false confidence.
        assert!(
            find_node_by_kind(root, "number_literal").is_some(),
            "expected a number_literal node for '5' in the parse tree; \
             the test is not exercising the contiguity check if the parse failed catastrophically"
        );

        if let Some(ql) = find_node_by_kind(root, "quantity_literal") {
            let start = ql.start_byte();
            let end = ql.end_byte();
            assert!(
                !(start <= 22 && end >= 25),
                "scanner must NOT fuse '5 kg' into a quantity_literal (PRD §3.1 \
                 contiguity invariant); found quantity_literal at bytes {}..{}",
                start,
                end
            );
        }
        // No assertion about parse errors: an ERROR node around 'kg' is acceptable —
        // the key invariant is that no fused quantity_literal spans both tokens.
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

    // ── Task 3935: trait-assoc-fn α ───────────────────────────────────────────
    // These four tests exercise the new grammar shapes introduced in task 3935:
    //   - fn with body inside a trait body (function_definition under trait_member)
    //   - fn without body inside a trait body (function_signature under trait_member)
    //   - optional leading `self` receiver in fn_param_list
    //   - regression: top-level bodyless fn still produces an ERROR (not function_signature)
    //
    // Tests (1)-(3) are RED before grammar step-2 lands; test (4) is a regression
    // guard that remains GREEN throughout.  Canonical CI signal per project precedent.

    /// (1) Default assoc fn with body and self receiver inside a trait body.
    /// Source: `trait T { fn f(self, x: Int) -> Int { x } }`
    /// After grammar change, must parse cleanly as function_definition under trait_member
    /// with the `self` receiver accessible via child_by_field_name("receiver").
    #[test]
    fn test_trait_default_assoc_fn_with_body_and_self_parses() {
        let mut parser = make_parser();
        let source = b"trait T { fn f(self, x: Int) -> Int { x } }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // (a) No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in trait default assoc fn source: {kinds:?}"
        );

        // (b) trait_member node must exist
        let trait_member = find_node_by_kind(root, "trait_member")
            .expect("trait_member not found in parse tree");

        // (c) trait_member's child is a function_definition
        let fn_def = find_node_by_kind(trait_member, "function_definition")
            .expect("function_definition not found under trait_member");

        // (d) fn_param_list exposes receiver = self via field name
        let fn_param_list = find_node_by_kind(fn_def, "fn_param_list")
            .expect("fn_param_list not found under function_definition");
        let receiver = fn_param_list
            .child_by_field_name("receiver")
            .expect("fn_param_list missing 'receiver' field");
        assert_eq!(
            receiver.kind(),
            "self",
            "expected receiver to be 'self', got: {}",
            receiver.kind()
        );

        // (e) at least one fn_param is present (x: Int)
        assert!(
            collect_kinds(fn_param_list).contains(&"fn_param".to_string()),
            "expected at least one fn_param in fn_param_list, got: {:?}",
            collect_kinds(fn_param_list)
        );
    }

    /// (2) Required (bodyless) assoc fn inside a trait body.
    /// Source: `trait T { fn g(self, x: Int) -> Int }`
    /// After grammar change, must parse as function_signature (not function_definition)
    /// under trait_member, with no fn_body child and self receiver via field.
    #[test]
    fn test_trait_required_assoc_fn_bodyless_parses() {
        let mut parser = make_parser();
        let source = b"trait T { fn g(self, x: Int) -> Int }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // (a) No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in trait required assoc fn source: {kinds:?}"
        );

        // (b) function_signature appears under trait_member
        let trait_member = find_node_by_kind(root, "trait_member")
            .expect("trait_member not found in parse tree");
        let fn_sig = find_node_by_kind(trait_member, "function_signature")
            .expect("function_signature not found under trait_member");

        // (c) function_signature has NO fn_body child
        assert!(
            find_node_by_kind(fn_sig, "fn_body").is_none(),
            "function_signature must not have an fn_body child"
        );

        // (d) fn_param_list exposes receiver = self via field name
        let fn_param_list = find_node_by_kind(fn_sig, "fn_param_list")
            .expect("fn_param_list not found under function_signature");
        let receiver = fn_param_list
            .child_by_field_name("receiver")
            .expect("fn_param_list missing 'receiver' field");
        assert_eq!(
            receiver.kind(),
            "self",
            "expected receiver to be 'self', got: {}",
            receiver.kind()
        );
    }

    /// (3) Default assoc fn with body but NO self receiver inside a trait body.
    /// Source: `trait T { fn h(x: Int) -> Int { x } }`
    /// After grammar change, must parse as function_definition under trait_member
    /// with fn_param_list's receiver field being None.
    #[test]
    fn test_trait_assoc_fn_no_self_still_parses() {
        let mut parser = make_parser();
        let source = b"trait T { fn h(x: Int) -> Int { x } }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // (a) No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in trait assoc fn (no self) source: {kinds:?}"
        );

        // (b) function_definition under trait_member
        let trait_member = find_node_by_kind(root, "trait_member")
            .expect("trait_member not found in parse tree");
        let fn_def = find_node_by_kind(trait_member, "function_definition")
            .expect("function_definition not found under trait_member");

        // (c) fn_param_list has no receiver field (self is optional)
        let fn_param_list = find_node_by_kind(fn_def, "fn_param_list")
            .expect("fn_param_list not found under function_definition");
        assert!(
            fn_param_list.child_by_field_name("receiver").is_none(),
            "fn_param_list should not have a 'receiver' field when no self is declared"
        );
    }

    // ── Task 3935: fixture-driven tests (step-3) ─────────────────────────────
    // These two tests embed the fixture files via include_str! and are RED until
    // the fixtures are created in step-4.  They fail at compile time when the
    // files are absent (compile-time include_str! path resolution).

    /// Fixture test: default assoc fn (with body) parses cleanly.
    /// Embeds `test/fixtures/trait_assoc_fn_default.ri` via include_str!.
    ///
    /// The fixture has two trait_member arms (param + function_definition).
    /// We verify by checking both node kinds appear in the fully-collected tree
    /// (find_node_by_kind returns only the first match, which is the param arm).
    #[test]
    fn test_trait_assoc_fn_default_fixture_parses_cleanly() {
        let mut parser = make_parser();
        let source = include_str!("../test/fixtures/trait_assoc_fn_default.ri");
        let tree = parser
            .parse(source.as_bytes(), None)
            .expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in trait_assoc_fn_default.ri: {kinds:?}"
        );

        // trait_member and function_definition must both appear in the tree
        // (fixture has param_declaration and function_definition as sibling trait_members)
        assert!(
            kinds.contains(&"trait_member".to_string()),
            "expected trait_member in default fixture: {kinds:?}"
        );
        assert!(
            kinds.contains(&"function_definition".to_string()),
            "expected function_definition in default fixture (under trait_member): {kinds:?}"
        );
    }

    /// Fixture test: required (bodyless) assoc fn parses cleanly as function_signature.
    /// Embeds `test/fixtures/trait_assoc_fn_required.ri` via include_str!.
    ///
    /// The fixture has two trait_member arms (param + function_signature).
    #[test]
    fn test_trait_assoc_fn_required_fixture_parses_cleanly() {
        let mut parser = make_parser();
        let source = include_str!("../test/fixtures/trait_assoc_fn_required.ri");
        let tree = parser
            .parse(source.as_bytes(), None)
            .expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in trait_assoc_fn_required.ri: {kinds:?}"
        );

        // trait_member and function_signature must both appear in the tree
        assert!(
            kinds.contains(&"trait_member".to_string()),
            "expected trait_member in required fixture: {kinds:?}"
        );
        assert!(
            kinds.contains(&"function_signature".to_string()),
            "expected function_signature in required fixture (under trait_member): {kinds:?}"
        );

        // function_definition must NOT appear — required fixture covers bodyless form only
        assert!(
            !kinds.contains(&"function_definition".to_string()),
            "required fixture should contain only function_signature, not function_definition: {kinds:?}"
        );
    }

    /// (4) REGRESSION: top-level bodyless fn must still produce an ERROR.
    /// Source: `fn f(x: Int) -> Int` (no body, at source_file scope)
    /// function_signature is scoped to trait_member only (not in _declaration),
    /// so this must NOT parse cleanly as function_signature at file scope.
    #[test]
    fn test_top_level_fn_bodyless_still_errors_regression() {
        let mut parser = make_parser();
        let source = b"fn f(x: Int) -> Int";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // (a) Parse error expected — top-level fn always requires a body
        assert!(
            root.has_error(),
            "expected parse ERROR for top-level bodyless fn, got no error: {kinds:?}"
        );

        // (b) function_signature must NOT appear at file scope
        // (function_signature is only reachable via trait_member, not _declaration)
        assert!(
            !kinds.contains(&"function_signature".to_string()),
            "function_signature must not appear at top-level scope: {kinds:?}"
        );
    }
}
