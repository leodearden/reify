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

    // ── Task 3938: named-field payload-binding grammar tests ─────────────────
    // These tests are RED until grammar step-2 lands (step-2 adds
    // variant_binding_pattern + field_binding rules to grammar.js).
    //
    // (1) dce-4-namedbind: `Variant { field: binder }` must parse with no ERROR
    //     nodes and produce variant_binding_pattern / field_binding nodes.
    // (2) dce-0-baseline regression floor: bare + pipe arms still parse clean.

    /// (1) Named-field payload binding: `Circle { radius: r }` in a match arm.
    /// After grammar change: no ERROR, variant_binding_pattern + field_binding present.
    #[test]
    fn test_match_pattern_named_field_binding() {
        let mut parser = make_parser();
        // dce-4-namedbind source (three arms: bare, one-field, two-field)
        let source = b"structure W { \
            let area = match outline { \
                Point => 0mm, \
                Circle { radius: r } => r, \
                Rect { width: w, height: h } => w \
            } \
        }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // (a) No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in dce-4-namedbind source: {kinds:?}"
        );

        // (b) No ERROR kind anywhere in the tree
        assert!(
            !kinds.contains(&"ERROR".to_string()),
            "ERROR node found in dce-4-namedbind parse tree: {kinds:?}"
        );

        // (c) variant_binding_pattern node must be present
        assert!(
            kinds.contains(&"variant_binding_pattern".to_string()),
            "expected variant_binding_pattern node in parse tree, got: {kinds:?}"
        );

        // (d) field_binding node must be present
        assert!(
            kinds.contains(&"field_binding".to_string()),
            "expected field_binding node in parse tree, got: {kinds:?}"
        );

        // (e) Verify variant_binding_pattern fields: variant, field, binder
        let vbp = find_node_by_kind(root, "variant_binding_pattern")
            .expect("variant_binding_pattern not found");
        let variant_field = vbp
            .child_by_field_name("variant")
            .expect("variant_binding_pattern missing 'variant' field");
        assert_eq!(variant_field.kind(), "identifier");

        let fb = find_node_by_kind(vbp, "field_binding")
            .expect("field_binding not found under variant_binding_pattern");
        let field_field = fb
            .child_by_field_name("field")
            .expect("field_binding missing 'field' field");
        let binder_field = fb
            .child_by_field_name("binder")
            .expect("field_binding missing 'binder' field");
        assert_eq!(field_field.kind(), "identifier");
        assert_eq!(binder_field.kind(), "identifier");

        // (f) Multi-field variant: `Rect { width: w, height: h }` must produce two
        //     field_binding nodes.  `find_node_by_kind` returns the first match, so
        //     pin the total count via `collect_kinds` to ensure both bindings are
        //     present in the tree (the corpus test also asserts this, but asserting
        //     here keeps the grammar-unit test self-contained).
        let field_binding_count = kinds
            .iter()
            .filter(|k| k.as_str() == "field_binding")
            .count();
        assert_eq!(
            field_binding_count,
            3,
            "expected 3 field_binding nodes (1 for Circle, 2 for Rect), got: {field_binding_count} \
             (kinds: {kinds:?})"
        );
    }

    /// (2) dce-0-baseline: bare + pipe arms still parse cleanly (regression floor).
    #[test]
    fn test_match_pattern_bare_pipe_baseline() {
        let mut parser = make_parser();
        let source = b"structure S { let x = match d { In => 1, Out | Bidi => 2 } }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        assert!(
            !root.has_error(),
            "unexpected parse error in dce-0-baseline source: {kinds:?}"
        );

        assert!(
            !kinds.contains(&"ERROR".to_string()),
            "ERROR node found in dce-0-baseline parse tree: {kinds:?}"
        );
    }

    // ── Task 3971: trait-assoc-type ιₐ — grammar tests ──────────────────────
    // Two new CONSUMPTION surfaces:
    //   (1) structure-body `type X = Concrete` — admitted via `associated_type` in `_member`
    //   (2) qualified type-expr `Beam::Material` / `Beam::(Trait::Material)` in type position
    //
    // Tests (a)-(f) are RED before grammar step-2 lands.
    // Test (g) REGRESSION PIN — trait-body associated-type — stays GREEN throughout.

    /// (a) Structure-body associated type binding: `structure def Beam : HasMaterial { type Material = Steel }`
    /// After grammar change: no ERROR, `associated_type` node directly under `structure_definition`.
    #[test]
    fn test_structure_body_assoc_type_binding_parses() {
        let mut parser = make_parser();
        let source = b"structure def Beam : HasMaterial { type Material = Steel }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // (1) No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in structure-body assoc-type binding: {kinds:?}"
        );

        // (2) associated_type node must exist in the tree
        assert!(
            kinds.contains(&"associated_type".to_string()),
            "expected associated_type node in parse tree: {kinds:?}"
        );

        // (3) associated_type must appear under structure_definition (not wrapped in trait_member)
        let struct_def = find_node_by_kind(root, "structure_definition")
            .expect("structure_definition not found");
        let assoc_type = find_node_by_kind(struct_def, "associated_type")
            .expect("associated_type not found under structure_definition");

        // (4) name field is present
        let name = assoc_type
            .child_by_field_name("name")
            .expect("associated_type missing 'name' field");
        assert_eq!(name.kind(), "identifier", "expected identifier for name, got: {}", name.kind());

        // (5) default field is present (= Steel)
        let default_type = assoc_type
            .child_by_field_name("default")
            .expect("associated_type missing 'default' field (= Steel)");
        // default field is a type_expr
        assert_eq!(
            default_type.kind(),
            "type_expr",
            "expected type_expr for default, got: {}",
            default_type.kind()
        );
    }

    /// (b) Qualified type-expr bare form: `param m : Beam::Material` in structure body.
    /// After grammar change: no ERROR, `qualified_type` node in type position.
    #[test]
    fn test_qualified_type_expr_bare_form_parses() {
        let mut parser = make_parser();
        let source = b"structure S { param m : Beam::Material }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // (1) No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in qualified type-expr (bare form): {kinds:?}"
        );

        // (2) qualified_type node must exist
        assert!(
            kinds.contains(&"qualified_type".to_string()),
            "expected qualified_type node in parse tree: {kinds:?}"
        );

        // (3) qualified_type must be under type_expr (which is under param_declaration)
        let param_decl = find_node_by_kind(root, "param_declaration")
            .expect("param_declaration not found");
        let type_expr = param_decl
            .child_by_field_name("type")
            .expect("param_declaration missing 'type' field");
        assert_eq!(type_expr.kind(), "type_expr");
        let qual_type = find_node_by_kind(type_expr, "qualified_type")
            .expect("qualified_type not found under type_expr");

        // (4) base field is an identifier
        let base = qual_type
            .child_by_field_name("base")
            .expect("qualified_type missing 'base' field");
        assert_eq!(base.kind(), "identifier");

        // (5) member field is an identifier
        let member = qual_type
            .child_by_field_name("member")
            .expect("qualified_type missing 'member' field");
        assert_eq!(member.kind(), "identifier");

        // (6) trait field must NOT be present (bare form, no parenthesized disambiguator)
        assert!(
            qual_type.child_by_field_name("trait").is_none(),
            "bare form qualified_type must not have a 'trait' field"
        );
    }

    /// (c) Qualified type-expr type-param base: `param y : T::Material`.
    /// After grammar change: no ERROR, `qualified_type` with identifier base `T`.
    #[test]
    fn test_qualified_type_expr_type_param_base_parses() {
        let mut parser = make_parser();
        let source = b"structure S { param y : T::Material }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        assert!(
            !root.has_error(),
            "unexpected parse error in qualified type-expr (type-param base): {kinds:?}"
        );
        assert!(
            kinds.contains(&"qualified_type".to_string()),
            "expected qualified_type node in parse tree: {kinds:?}"
        );
    }

    /// (d) FORK-G disambiguator: `param n : Beam::(HasMaterial::Material)`.
    /// After grammar change: no ERROR, `qualified_type` with `trait` field present.
    #[test]
    fn test_qualified_type_expr_fork_g_disambiguator_parses() {
        let mut parser = make_parser();
        let source = b"structure S { param n : Beam::(HasMaterial::Material) }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // (1) No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in FORK-G disambiguated qualified type-expr: {kinds:?}"
        );

        // (2) qualified_type node must exist
        assert!(
            kinds.contains(&"qualified_type".to_string()),
            "expected qualified_type node in parse tree: {kinds:?}"
        );

        // (3) qualified_type must have `trait` field (distinguishes FORK-G from bare form)
        let param_decl = find_node_by_kind(root, "param_declaration")
            .expect("param_declaration not found");
        let type_expr = param_decl
            .child_by_field_name("type")
            .expect("param_declaration missing 'type' field");
        let qual_type = find_node_by_kind(type_expr, "qualified_type")
            .expect("qualified_type not found under type_expr");

        let trait_field = qual_type
            .child_by_field_name("trait")
            .expect("qualified_type missing 'trait' field (FORK-G disambiguator must have trait name)");
        assert_eq!(trait_field.kind(), "identifier");

        let member_field = qual_type
            .child_by_field_name("member")
            .expect("qualified_type missing 'member' field");
        assert_eq!(member_field.kind(), "identifier");
    }

    // ── Task 3971: fixture-driven tests ─────────────────────────────────────
    // These two tests embed the fixture files via include_str! and are RED until
    // the grammar change (step-2) lands.  They verify `!root.has_error()` for
    // both fixture files as the integration-level parse signal.

    /// (e) Fixture test: structure-body associated-type binding parses cleanly.
    /// Embeds `test/fixtures/trait_assoc_type_bind.ri` via include_str!.
    #[test]
    fn test_trait_assoc_type_bind_fixture_parses_cleanly() {
        let mut parser = make_parser();
        let source = include_str!("../test/fixtures/trait_assoc_type_bind.ri");
        let tree = parser
            .parse(source.as_bytes(), None)
            .expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in trait_assoc_type_bind.ri: {kinds:?}"
        );

        // Both trait_declaration (regression) and structure_definition (new) must appear
        assert!(
            kinds.contains(&"trait_declaration".to_string()),
            "expected trait_declaration in bind fixture: {kinds:?}"
        );
        assert!(
            kinds.contains(&"structure_definition".to_string()),
            "expected structure_definition in bind fixture: {kinds:?}"
        );

        // associated_type must appear (both under trait_member and under structure_definition)
        assert!(
            kinds.contains(&"associated_type".to_string()),
            "expected associated_type node in bind fixture: {kinds:?}"
        );
    }

    /// (f) Fixture test: qualified type-expr uses parse cleanly.
    /// Embeds `test/fixtures/trait_assoc_type_qual.ri` via include_str!.
    #[test]
    fn test_trait_assoc_type_qual_fixture_parses_cleanly() {
        let mut parser = make_parser();
        let source = include_str!("../test/fixtures/trait_assoc_type_qual.ri");
        let tree = parser
            .parse(source.as_bytes(), None)
            .expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in trait_assoc_type_qual.ri: {kinds:?}"
        );

        // qualified_type must appear in the tree
        assert!(
            kinds.contains(&"qualified_type".to_string()),
            "expected qualified_type node in qual fixture: {kinds:?}"
        );
    }

    // ── Task 3971: REGRESSION PIN ────────────────────────────────────────────
    // Trait-body associated-type declaration already works; these tests must stay
    // GREEN throughout the entire task (before and after grammar changes).

    /// (g1) REGRESSION — trait-body associated type without default: `trait T { type Material }`.
    /// Must parse as `associated_type` under `trait_member`, no ERROR.
    #[test]
    fn test_trait_body_assoc_type_no_default_regression() {
        let mut parser = make_parser();
        let source = b"trait T { type Material }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // No parse errors
        assert!(
            !root.has_error(),
            "REGRESSION: trait-body `type Material` (no default) broke: {kinds:?}"
        );

        // trait_member must exist
        let trait_member = find_node_by_kind(root, "trait_member")
            .expect("REGRESSION: trait_member not found");

        // associated_type must be under trait_member
        let assoc_type = find_node_by_kind(trait_member, "associated_type")
            .expect("REGRESSION: associated_type not found under trait_member");

        // name field is present, default field is absent
        assert!(
            assoc_type.child_by_field_name("name").is_some(),
            "REGRESSION: associated_type missing 'name' field"
        );
        assert!(
            assoc_type.child_by_field_name("default").is_none(),
            "REGRESSION: associated_type without default should not have 'default' field"
        );
    }

    /// (g2) REGRESSION — trait-body associated type with default: `trait T { type Material = Steel }`.
    /// Must parse as `associated_type` with `default` field under `trait_member`, no ERROR.
    #[test]
    fn test_trait_body_assoc_type_with_default_regression() {
        let mut parser = make_parser();
        let source = b"trait T { type Material = Steel }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // No parse errors
        assert!(
            !root.has_error(),
            "REGRESSION: trait-body `type Material = Steel` broke: {kinds:?}"
        );

        // trait_member → associated_type → default field
        let trait_member = find_node_by_kind(root, "trait_member")
            .expect("REGRESSION: trait_member not found");
        let assoc_type = find_node_by_kind(trait_member, "associated_type")
            .expect("REGRESSION: associated_type not found under trait_member");

        assert!(
            assoc_type.child_by_field_name("default").is_some(),
            "REGRESSION: associated_type with default must have 'default' field"
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

    // ── Task 3967: string interpolation α — grammar RED tests ────────────────
    // These tests are RED before grammar step-2 lands (step-2 adds
    // `interpolated_string`, `interpolation`, `string_chunk` to grammar.js and
    // the content-run external scanner token to scanner.c).
    //
    // (1) plain-string fast path: `"hello"` → string_literal, NOT interpolated_string
    // (2) simple-hole: `"a {x} b"` → interpolated_string + string_chunk + interpolation + identifier
    // (3) empty-hole: `"{}"` → parse ERROR (interpolation requires $._expression)
    //
    // Tests (1)-(3) are all CLI-independent: they use make_parser + collect_kinds
    // and do not require `tree-sitter` on PATH.

    /// (1) Plain string fast path: brace-free `"hello"` stays a string_literal
    /// and does NOT produce an interpolated_string node.
    /// RED reason: after step-2 narrows string_literal to exclude braces, this test
    /// will still pass (brace-free strings still lex as string_literal).
    /// Before step-2, this test is GREEN (string_literal exists).
    /// It serves as the REGRESSION PIN that must stay GREEN after step-2.
    #[test]
    fn test_plain_string_stays_string_literal() {
        let mut parser = make_parser();
        let source = b"structure S { let x = \"hello\" }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // No parse errors
        assert!(
            !root.has_error(),
            "unexpected parse error in plain string source: {kinds:?}"
        );

        // string_literal must be present
        assert!(
            kinds.contains(&"string_literal".to_string()),
            "expected string_literal node for brace-free string, got: {kinds:?}"
        );

        // interpolated_string must NOT be present
        assert!(
            !kinds.contains(&"interpolated_string".to_string()),
            "interpolated_string must NOT appear for a brace-free string, got: {kinds:?}"
        );
    }

    /// (2) Interpolated string: `"a {x} b"` must produce interpolated_string,
    /// string_chunk (for "a " and " b"), interpolation, and identifier nodes.
    /// RED: today every quoted string lexes as a single opaque string_literal.
    #[test]
    fn test_interpolated_string_simple_hole() {
        let mut parser = make_parser();
        let source = b"structure S { let x = \"a {y} b\" }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        let kinds = collect_kinds(root);

        // interpolated_string must appear
        assert!(
            kinds.contains(&"interpolated_string".to_string()),
            "expected interpolated_string node, got: {kinds:?}"
        );

        // string_chunk must appear (literal content runs)
        assert!(
            kinds.contains(&"string_chunk".to_string()),
            "expected string_chunk node(s) for literal parts, got: {kinds:?}"
        );

        // interpolation must appear
        assert!(
            kinds.contains(&"interpolation".to_string()),
            "expected interpolation node for {{y}}, got: {kinds:?}"
        );

        // identifier must appear inside the interpolation
        assert!(
            kinds.contains(&"identifier".to_string()),
            "expected identifier inside interpolation, got: {kinds:?}"
        );

        // string_literal must NOT appear (brace-bearing strings route to interpolated_string)
        assert!(
            !kinds.contains(&"string_literal".to_string()),
            "string_literal must NOT appear for an interpolated string, got: {kinds:?}"
        );
    }

    /// (3) Empty hole `"{}"` must be a parse error (interpolation requires an expression).
    /// RED: today `"{}"` lexes as a single opaque string_literal (no error).
    #[test]
    fn test_interpolated_string_empty_hole_is_error() {
        let mut parser = make_parser();
        let source = b"structure S { let x = \"{}\" }";
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();

        // Parse error expected — empty {} is not a valid interpolation
        assert!(
            root.has_error(),
            "expected parse ERROR for empty hole \"{{}}\", got no error; \
             kinds: {:?}",
            collect_kinds(root)
        );
    }

}
