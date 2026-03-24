//! Option expression evaluation tests — some(expr) and none.

use reify_expr::{eval_expr, EvalContext};
use reify_types::{CompiledExpr, Type, Value, ValueMap};

// ── step-1: OptionNone tests ─────────────────────────────────────────────────

/// Evaluating `none` should produce `Value::Option(None)`.
#[test]
fn eval_none_produces_option_none() {
    let expr = CompiledExpr::option_none(Type::Option(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Option(None));
}

/// Evaluating `none` must NOT produce `Value::Undef`.
#[test]
fn eval_none_is_not_undef() {
    let expr = CompiledExpr::option_none(Type::Option(Box::new(Type::Bool)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_ne!(result, Value::Undef);
}
