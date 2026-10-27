//! The seed set's own contract: which column a cell owns, and the two
//! per-subtree predicates whose `false` answers the fast path acts on as
//! proofs.
//!
//! A `false` from `subtree_has_kink` lets a seed-independent subtree be handed
//! to `eval_expr` whole, recording no branch — so every node that is not
//! smooth, or whose interior the walk cannot see, must answer `true`, or λ's
//! "an empty record means no kink" guarantee silently breaks.

use reify_expr::Seeds;
use reify_ir::{BinOp, CompiledExpr, Value};
use reify_test_support::builders::expr::{binop, conditional_expr, literal, user_fn_call};

use crate::support::{calln, cell, dl, jrow, pref, probe};

#[test]
fn a_duplicated_seed_cell_keeps_its_first_column() {
    let (x, y) = (cell("x"), cell("y"));
    let seeds = Seeds::new(&[x.clone(), y.clone(), x.clone()]);
    assert_eq!(seeds.width(), 3, "every column given is a column of the row");
    assert_eq!(seeds.columns(), [x.clone(), y.clone(), x.clone()]);
    assert_eq!(seeds.column_of(&x), Some(0), "the FIRST occurrence owns the cell");
    assert_eq!(seeds.column_of(&y), Some(1));
    assert_eq!(seeds.column_of(&cell("z")), None, "an unseeded cell has no column");

    let (values, _) = probe(&[("x", 2.0), ("y", 3.0)]);
    let (_, row) = jrow(&pref("x"), &values, &[x.clone(), y, x]).expect("a seed is differentiable");
    assert_eq!(row, vec![1.0, 0.0, 0.0], "the derivative is not split across the duplicate");
}

#[test]
fn subtree_has_kink_claims_every_non_smooth_or_opaque_node() {
    let cond = || conditional_expr(binop(BinOp::Gt, pref("x"), pref("y")), pref("x"), pref("y"));
    let cases: Vec<(&str, CompiledExpr, bool)> = vec![
        ("a conditional", cond(), true),
        ("a comparison", binop(BinOp::Lt, pref("x"), pref("y")), true),
        ("a Kleene connective", binop(BinOp::And, pref("x"), pref("y")), true),
        ("mod", binop(BinOp::Mod, pref("x"), pref("y")), true),
        ("a user-function call", user_fn_call("f", vec![pref("x")], dl()), true),
        ("an arity-1 builtin a field shadow may replace", calln("sqrt", vec![pref("x")]), true),
        ("binary min", calln("min", vec![pref("x"), pref("y")]), true),
        ("clamp", calln("clamp", vec![pref("x"), pref("y"), pref("z")]), true),
        ("the bounded-reduction shape", calln("argmax", vec![pref("x"), pref("y")]), true),
        ("a nested kink", binop(BinOp::Add, pref("x"), cond()), true),
        ("a literal", literal(Value::Real(1.0)), false),
        ("a value ref", pref("x"), false),
        ("a sum of refs", binop(BinOp::Add, pref("x"), pref("y")), false),
        ("a product of refs", binop(BinOp::Mul, pref("x"), pref("y")), false),
        ("atan2", calln("atan2", vec![pref("x"), pref("y")]), false),
        ("pow", calln("pow", vec![pref("x"), pref("y")]), false),
    ];
    for (label, expr, expected) in &cases {
        let seeds = Seeds::new(&[cell("x")]);
        assert_eq!(seeds.subtree_has_kink(expr), *expected, "{label}");
    }
}

#[test]
fn depends_on_seed_is_true_exactly_when_a_seed_is_referenced() {
    let seeds = Seeds::new(&[cell("x")]);
    assert!(seeds.depends_on_seed(&binop(BinOp::Add, pref("x"), pref("u"))));
    assert!(!seeds.depends_on_seed(&binop(BinOp::Add, pref("u"), literal(Value::Real(1.0)))));
}
