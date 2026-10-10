//! A trailing binder body (quantifier predicate, conditional `else` branch,
//! lambda body) lowers with the whole keyword-operator chain that follows it.
//! Ruling: `docs/reify-language-spec.md` §16.

use reify_ast::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("binder_body_test"));
    let structure = match &module
        .declarations
        .iter()
        .find(|d| matches!(d, Declaration::Structure(_)))
    {
        Some(Declaration::Structure(s)) => s.clone(),
        other => panic!("expected Structure, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

/// The value of the single `let` in `structure S { <member> }`.
fn sole_let_value(member: &str) -> Expr {
    let (members, errors) = parse_members(&format!("structure S {{\n    {member}\n}}\n"));
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    match members.as_slice() {
        [MemberDecl::Let(l)] => l.value.clone(),
        other => panic!("expected one Let, got {:?}", other),
    }
}

/// Assert `expr` is `BinOp(expected)` and return its operands.
fn bin_op<'e>(expr: &'e Expr, expected: &str) -> (&'e Expr, &'e Expr) {
    match &expr.kind {
        ExprKind::BinOp { op, left, right } if op == expected => (left, right),
        other => panic!("expected BinOp({expected}), got {:?}", other),
    }
}

#[test]
fn quantifier_predicate_lowers_with_whole_conjunction() {
    let value = sole_let_value("let r = forall v in xs: v > 0 and v < 3");
    let ExprKind::Quantifier { predicate, .. } = &value.kind else {
        panic!("expected Quantifier, got {:?}", value.kind);
    };
    let (left, right) = bin_op(predicate, "and");
    bin_op(left, ">");
    bin_op(right, "<");
}

#[test]
fn conditional_else_lowers_with_whole_conjunction() {
    let value = sole_let_value("let r = if true then true else false and false");
    let ExprKind::Conditional { else_branch, .. } = &value.kind else {
        panic!("expected Conditional, got {:?}", value.kind);
    };
    bin_op(else_branch, "and");
}

#[test]
fn lambda_body_lowers_with_whole_conjunction() {
    let value = sole_let_value("let f = |v| v > 0 and v < 3");
    let ExprKind::Lambda { body, .. } = &value.kind else {
        panic!("expected Lambda, got {:?}", value.kind);
    };
    bin_op(body, "and");
}
