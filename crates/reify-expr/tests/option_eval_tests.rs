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

// ── step-3: OptionSome wrapping literal values ────────────────────────────────

/// `some(5)` should evaluate to `Value::Option(Some(Box::new(Value::Int(5))))`.
#[test]
fn eval_some_wraps_int() {
    let inner = CompiledExpr::literal(Value::Int(5), Type::Int);
    let expr = CompiledExpr::option_some(inner, Type::Option(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Int(5)))));
}

/// `some(true)` should evaluate to `Value::Option(Some(Box::new(Value::Bool(true))))`.
#[test]
fn eval_some_wraps_bool() {
    let inner = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let expr = CompiledExpr::option_some(inner, Type::Option(Box::new(Type::Bool)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Bool(true)))));
}

/// `some(3.14)` should evaluate to `Value::Option(Some(Box::new(Value::Real(3.14))))`.
#[test]
fn eval_some_wraps_real() {
    let inner = CompiledExpr::literal(Value::Real(3.14), Type::Real);
    let expr = CompiledExpr::option_some(inner, Type::Option(Box::new(Type::Real)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Real(3.14)))));
}
