//! Tests for `fn_param` default-value expressions (task 3449).
//!
//! User-observable signal: `cargo test -p reify-syntax --test fn_param_default_tests`
//! passes.
//!
//! Two test tiers:
//! 1. **CST-level** (via `tree_sitter::Parser` directly) — catches grammar
//!    regressions independently of the lowering pipeline.
//! 2. **AST-level** (via `reify_syntax::parse`) — catches lowering regressions
//!    and verifies that `FnParam.default` round-trips the source expression.
//!    (Added in step-3 / step-4 of the TDD plan.)
//!
//! Design decision: `fn_param` defaults accept `$._expression` only — NOT
//! `choice($.auto_keyword, $._expression)` like `param_declaration` does.
//! Function parameters are call-site provided, not solver-determined.

// ── CST-level helpers ────────────────────────────────────────────────────────

/// Build a tree-sitter parser loaded with the Reify grammar.
fn make_ts_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Depth-first search — returns the first node with the given kind.
fn find_cst_node<'a>(root: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
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
/// legitimately nest (e.g. `fn_param`).
fn find_outermost_cst_nodes<'a>(
    root: tree_sitter::Node<'a>,
    kind: &str,
) -> Vec<tree_sitter::Node<'a>> {
    let mut results = Vec::new();
    if root.kind() == kind {
        results.push(root);
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

// ── CST-level tests ───────────────────────────────────────────────────────────

/// `fn f(x : T = Foo.bar) -> T { x }` — CST must contain an `fn_param` node
/// with a `default` field child whose kind is `member_access`.
///
/// **RED state (step-1)**: fails because the grammar does not yet accept the
/// `= default` form — the parser produces an ERROR node.
#[test]
fn fn_param_cst_with_default_has_default_field_child() {
    let source = "fn f(x : T = Foo.bar) -> T { x }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    assert!(
        !tree.root_node().has_error(),
        "expected no CST errors for `fn f(x : T = Foo.bar) -> T {{ x }}`; \
         grammar does not yet accept fn_param default values",
    );

    let param = find_cst_node(tree.root_node(), "fn_param")
        .expect("expected an fn_param node in the CST");

    let default_node = param
        .child_by_field_name("default")
        .expect("fn_param with default must have a `default` field child");

    assert_eq!(
        default_node.kind(),
        "member_access",
        "default field child for `Foo.bar` must be `member_access`, got: {:?}",
        default_node.kind(),
    );
}

/// `fn f(x : T) -> T { x }` — CST must contain an `fn_param` node with NO
/// `default` field child (regression guard: the optional path still parses).
#[test]
fn fn_param_cst_without_default_has_no_default_field_child() {
    let source = "fn f(x : T) -> T { x }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    assert!(
        !tree.root_node().has_error(),
        "expected no CST errors for `fn f(x : T) -> T {{ x }}`",
    );

    let param = find_cst_node(tree.root_node(), "fn_param")
        .expect("expected an fn_param node in the CST");

    assert!(
        param.child_by_field_name("default").is_none(),
        "fn_param without default must have no `default` field child; \
         found: {:?}",
        param.child_by_field_name("default").map(|n| n.kind()),
    );
}
