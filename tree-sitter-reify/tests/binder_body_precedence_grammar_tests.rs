//! CST pins for the extent of a trailing binder body: a quantifier predicate,
//! a conditional's `else` branch and a lambda body each extend as far right as
//! possible, past every keyword operator including `implies`. The ruling lives
//! in `docs/reify-language-spec.md` §16. The Lezer editor grammar's twin is
//! `gui/src/__tests__/reifyGrammarBinderBody.test.ts`.

use tree_sitter::{Node, Tree};
use tree_sitter_reify::language;

const KEYWORD_OPS: [&str; 3] = ["and", "or", "implies"];

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

fn parse(label: &str, src: &str) -> Tree {
    let tree = make_parser().parse(src, None).expect("parse failed");
    assert_no_error(label, &tree);
    tree
}

fn assert_no_error(label: &str, tree: &Tree) {
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "{label}: expected 0 ERROR/MISSING nodes\nroot s-expression:\n{}",
        root.to_sexp()
    );
}

fn find_first<'t>(node: Node<'t>, matches: &dyn Fn(Node<'t>) -> bool) -> Option<Node<'t>> {
    if matches(node) {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'t>> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .find_map(|child| find_first(child, matches))
}

/// The `value` field of the `let_declaration` named `let_name`.
fn value_of<'t>(tree: &'t Tree, src: &str, let_name: &str) -> Node<'t> {
    let is_named_let = |node: Node<'t>| {
        node.kind() == "let_declaration"
            && node
                .child_by_field_name("name")
                .is_some_and(|name| name.utf8_text(src.as_bytes()).unwrap() == let_name)
    };
    let decl = find_first(tree.root_node(), &is_named_let)
        .unwrap_or_else(|| panic!("no `let {let_name}` in:\n{}", tree.root_node().to_sexp()));
    field(decl, "value")
}

fn field<'t>(node: Node<'t>, name: &str) -> Node<'t> {
    node.child_by_field_name(name)
        .unwrap_or_else(|| panic!("no `{name}` field on:\n{}", node.to_sexp()))
}

fn assert_kind(label: &str, node: Node, kind: &str) {
    assert_eq!(node.kind(), kind, "{label}: got\n{}", node.to_sexp());
}

fn assert_binary(label: &str, node: Node, op: &str) {
    assert_kind(label, node, "binary_expression");
    assert_eq!(
        field(node, "op").kind(),
        op,
        "{label}: got\n{}",
        node.to_sexp()
    );
}

fn text<'s>(node: Node, src: &'s str) -> &'s str {
    node.utf8_text(src.as_bytes()).unwrap()
}

#[test]
fn quantifier_predicate_absorbs_keyword_operators() {
    for q in ["forall", "exists"] {
        for op in KEYWORD_OPS {
            let label = format!("{q} … {op}");
            let expr = format!("{q} v in xs: v > 0 {op} v < 3");
            let src = format!("structure S {{ let r = {expr} }}");
            let tree = parse(&label, &src);
            let value = value_of(&tree, &src, "r");
            assert_kind(&label, value, "quantifier_expression");
            assert_eq!(text(value, &src), expr, "{label}: quantifier span");
            assert_binary(&label, field(value, "predicate"), op);
        }
    }
}

#[test]
fn conditional_else_branch_absorbs_keyword_operators() {
    for op in KEYWORD_OPS {
        let label = format!("if … else y {op} z");
        let src = format!("structure S {{ let r = if k then x else y {op} z }}");
        let tree = parse(&label, &src);
        let value = value_of(&tree, &src, "r");
        assert_kind(&label, value, "conditional_expression");
        assert_binary(&label, field(value, "else"), op);
    }
}

#[test]
fn lambda_body_absorbs_keyword_operators() {
    for op in KEYWORD_OPS {
        for lambda in [format!("|v| v > 0 {op} v < 3"), format!("|| a {op} b")] {
            let label = format!("lambda `{lambda}`");
            let src = format!("structure S {{ let f = {lambda} }}");
            let tree = parse(&label, &src);
            let value = value_of(&tree, &src, "f");
            assert_kind(&label, value, "lambda_expression");
            assert_binary(&label, field(value, "body"), op);
        }
    }
}

#[test]
fn binder_under_not_still_extends() {
    let label = "not forall … and …";
    let src = "structure S { let r = not forall v in xs: v > 0 and v < 3 }";
    let tree = parse(label, src);
    let value = value_of(&tree, src, "r");
    assert_kind(label, value, "unary_expression");
    let quantifier = field(value, "operand");
    assert_kind(label, quantifier, "quantifier_expression");
    assert_binary(label, field(quantifier, "predicate"), "and");
}

#[test]
fn binder_as_right_operand_extends() {
    let label = "a and forall … or …";
    let src = "structure S { let r = a and forall v in xs: p or q }";
    let tree = parse(label, src);
    let value = value_of(&tree, src, "r");
    assert_binary(label, value, "and");
    let quantifier = field(value, "right");
    assert_kind(label, quantifier, "quantifier_expression");
    assert_binary(label, field(quantifier, "predicate"), "or");
}

#[test]
fn nested_binders_share_the_extent() {
    let label = "forall … exists … and …";
    let src = "structure S { let r = forall v in xs: exists w in ys: v < w and w < 3 }";
    let tree = parse(label, src);
    let outer = value_of(&tree, src, "r");
    assert_kind(label, outer, "quantifier_expression");
    let inner = field(outer, "predicate");
    assert_kind(label, inner, "quantifier_expression");
    assert_binary(label, field(inner, "predicate"), "and");
}

// Regression floor: GREEN before and after the ruling.

#[test]
fn parenthesised_binder_ends_the_body() {
    let label = "(forall … p) and q";
    let src = "structure S { let r = (forall v in xs: p) and q }";
    let tree = parse(label, src);
    let value = value_of(&tree, src, "r");
    assert_binary(label, value, "and");
    let parens = field(value, "left");
    assert_kind(label, parens, "parenthesized_expression");
    let quantifier = parens.named_child(0).unwrap();
    assert_kind(label, quantifier, "quantifier_expression");
    let predicate = field(quantifier, "predicate");
    assert_kind(label, predicate, "identifier");
    assert_eq!(text(predicate, src), "p", "{label}: predicate");
}

#[test]
fn forall_statement_unchanged() {
    let label = "forall … : constraint p and q";
    let src = "structure S { forall x in xs: constraint p and q }";
    let tree = parse(label, src);
    let statement = find_first(tree.root_node(), &|n| n.kind() == "forall_statement")
        .unwrap_or_else(|| {
            panic!(
                "{label}: no forall_statement in\n{}",
                tree.root_node().to_sexp()
            )
        });
    let body = field(statement, "body");
    assert_kind(label, body, "constraint_declaration");
    assert_binary(label, field(body, "expr"), "and");
}

#[test]
fn condition_and_then_positions_unchanged() {
    let label = "if a and b then c and d else e";
    let src = "structure S { let r = if a and b then c and d else e }";
    let tree = parse(label, src);
    let value = value_of(&tree, src, "r");
    assert_kind(label, value, "conditional_expression");
    assert_binary(label, field(value, "condition"), "and");
    assert_binary(label, field(value, "then"), "and");
    let else_branch = field(value, "else");
    assert_kind(label, else_branch, "identifier");
    assert_eq!(text(else_branch, src), "e", "{label}: else branch");
}

#[test]
fn tighter_operators_already_absorbed() {
    let label = "forall … v .. 3";
    let src = "structure S { let r = forall v in xs: v .. 3 }";
    let tree = parse(label, src);
    let value = value_of(&tree, src, "r");
    assert_kind(label, value, "quantifier_expression");
    assert_kind(label, field(value, "predicate"), "range_expression");
}
