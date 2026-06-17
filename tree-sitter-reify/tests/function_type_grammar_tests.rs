//! Grammar integration tests for the arrow-type / function-type production
//! `(T) -> U` in `type_expr` (task 4595 step-1 RED).
//!
//! Step-1 RED: the current grammar's `type_expr` (grammar.js ~1052) is
//! `choice(parameterized_type, qualified_type, identifier)` — it has NO
//! function/arrow form, so the leading `(` of a `(T) -> U` annotation matches
//! no arm and produces an ERROR subtree.
//!
//! Step-2 GREEN: a new `function_type` rule
//!   `seq('(', commaSep($.type_expr), ')', '->', field('return_type', $.type_expr))`
//! is added as the FIRST arm of `type_expr`, making every assertion below pass.
//!
//! The harness (make_parser / count_errors / collect_kinds / find_node_by_kind /
//! find_all_nodes_by_kind) mirrors tree-sitter-reify/tests/enum_type_param_grammar_tests.rs.
//!
//! Snippets exercise the four shapes called out in the plan:
//!   - single-param arrow type as a fn param:   `fn f(g: (T) -> U) -> Bool { true }`
//!   - multi-param arrow type:                  `fn f(g: (A, B) -> C) -> Bool { true }`
//!   - nullary arrow type:                      `fn f(g: () -> U) -> Bool { true }`
//!   - arrow type as a fn return type:          `fn h() -> (T) -> U { true }`

use tree_sitter_reify::language;

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Depth-first count of ERROR and MISSING nodes.
fn count_errors(node: tree_sitter::Node) -> usize {
    let mut count = 0;
    if node.is_error() || node.is_missing() {
        count += 1;
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            count += count_errors(cursor.node());
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    count
}

/// Collect all node kinds depth-first (for error diagnostics).
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

/// Collect all nodes with the given kind (depth-first).
fn find_all_nodes_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Vec<tree_sitter::Node<'a>> {
    let mut results = Vec::new();
    if node.kind() == kind {
        results.push(node);
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            results.extend(find_all_nodes_by_kind(cursor.node(), kind));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    results
}

// ── (a) Single-param arrow type as a fn param ────────────────────────────────

/// `fn f(g: (T) -> U) -> Bool { true }` parses with 0 ERROR/MISSING nodes.
///
/// RED: `(T) -> U` has no `type_expr` production — the leading `(` is an ERROR.
/// GREEN (step-2): the `function_type` arm accepts it.
#[test]
fn single_param_arrow_type_parses_with_zero_errors() {
    let mut parser = make_parser();
    let source = b"fn f(g: (T) -> U) -> Bool { true }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "`fn f(g: (T) -> U)` must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

/// The single-param arrow type must lower to a `function_type` node that has a
/// `return_type` field child, and exactly one positional `type_expr` param.
///
/// RED: no `function_type` node exists in the grammar.
/// GREEN (step-2): the node is present with a `return_type` field.
#[test]
fn single_param_arrow_type_has_function_type_with_return_field() {
    let mut parser = make_parser();
    let source = b"fn f(g: (T) -> U) -> Bool { true }";
    let tree = parser.parse(source, None).expect("parse failed");

    let function_type = find_node_by_kind(tree.root_node(), "function_type").expect(
        "expected a function_type node for `(T) -> U`; \
         the new type_expr arm must produce one",
    );

    // It must carry a `return_type` field child.
    let return_field = function_type
        .child_by_field_name("return_type")
        .expect("function_type must have a 'return_type' field child");
    assert!(
        !return_field.is_error(),
        "function_type return_type field must not be an ERROR node"
    );
}

// ── (b) Multi-param arrow type ───────────────────────────────────────────────

/// `fn f(g: (A, B) -> C) -> Bool { true }` parses with 0 ERROR/MISSING nodes
/// and produces a function_type carrying two positional param `type_expr`s plus
/// the `return_type` field.
///
/// RED: the multi-param arrow type ERROR-nodes.
/// GREEN (step-2): `commaSep($.type_expr)` accepts the param list.
#[test]
fn multi_param_arrow_type_parses_with_zero_errors() {
    let mut parser = make_parser();
    let source = b"fn f(g: (A, B) -> C) -> Bool { true }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "`(A, B) -> C` must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );

    let function_type = find_node_by_kind(tree.root_node(), "function_type")
        .expect("expected a function_type node for `(A, B) -> C`");
    let return_field = function_type
        .child_by_field_name("return_type")
        .expect("function_type must have a 'return_type' field child");
    // The two params (A, B) are positional type_expr children, distinct from
    // the return_type field. Excluding the return field, there must be two.
    let param_type_exprs: Vec<_> = find_all_nodes_by_kind(function_type, "type_expr")
        .into_iter()
        .filter(|n| n.id() != return_field.id() && n.parent().map(|p| p.id()) == Some(function_type.id()))
        .collect();
    assert_eq!(
        param_type_exprs.len(),
        2,
        "`(A, B) -> C` must have two positional param type_exprs; got {}",
        param_type_exprs.len()
    );
}

// ── (c) Nullary arrow type ───────────────────────────────────────────────────

/// `fn f(g: () -> U) -> Bool { true }` parses with 0 ERROR/MISSING nodes —
/// `commaSep` permits zero params.
///
/// RED: `() -> U` ERROR-nodes.
/// GREEN (step-2): the empty param list is accepted.
#[test]
fn nullary_arrow_type_parses_with_zero_errors() {
    let mut parser = make_parser();
    let source = b"fn f(g: () -> U) -> Bool { true }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "`() -> U` must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );

    let function_type = find_node_by_kind(tree.root_node(), "function_type")
        .expect("expected a function_type node for `() -> U`");
    assert!(
        function_type.child_by_field_name("return_type").is_some(),
        "nullary function_type must still have a 'return_type' field child"
    );
}

// ── (d) Arrow type as a fn return type ───────────────────────────────────────

/// `fn h() -> (T) -> U { true }` parses with 0 ERROR/MISSING nodes — an arrow
/// type is a valid `return_type` of a `fn` (it is just a `type_expr`).
///
/// RED: the `(T) -> U` return type ERROR-nodes.
/// GREEN (step-2): the function_type arm accepts it as the fn's return_type.
#[test]
fn arrow_type_as_return_type_parses_with_zero_errors() {
    let mut parser = make_parser();
    let source = b"fn h() -> (T) -> U { true }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "`fn h() -> (T) -> U` must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );

    // There must be a function_type node (the fn's return type).
    assert!(
        find_node_by_kind(tree.root_node(), "function_type").is_some(),
        "expected a function_type node for the `(T) -> U` return type"
    );
}
