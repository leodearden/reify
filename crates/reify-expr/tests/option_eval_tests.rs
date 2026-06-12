//! Option expression evaluation tests — some(expr) and none.

use reify_expr::{EvalContext, eval_expr};
use reify_core::{Type, ValueCellId};
use reify_ir::{CompiledExpr, Value, ValueMap};

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
    let inner = CompiledExpr::literal(Value::Real(2.78), Type::dimensionless_scalar());
    let expr = CompiledExpr::option_some(inner, Type::Option(Box::new(Type::dimensionless_scalar())));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Real(2.78)))));
}

// ── step-5: Undef semantics — some(undef) wraps, does NOT propagate ───────────

/// `some(undef)` must yield `Value::Option(Some(Box::new(Value::Undef)))`,
/// NOT bare `Value::Undef`.  This preserves the three-way distinction.
#[test]
fn eval_some_undef_produces_option_some_undef() {
    let inner = CompiledExpr::literal(Value::Undef, Type::Int);
    let expr = CompiledExpr::option_some(inner, Type::Option(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Undef))));
}

/// `some(undef)` must NOT equal `Value::Option(None)`.
#[test]
fn eval_some_undef_not_equal_to_none() {
    let inner = CompiledExpr::literal(Value::Undef, Type::Int);
    let expr = CompiledExpr::option_some(inner, Type::Option(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_ne!(result, Value::Option(None));
}

/// `some(undef)` must NOT equal bare `Value::Undef`.
#[test]
fn eval_some_undef_not_equal_to_bare_undef() {
    let inner = CompiledExpr::literal(Value::Undef, Type::Int);
    let expr = CompiledExpr::option_some(inner, Type::Option(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_ne!(result, Value::Undef);
}

// ── step-6: Nesting and value-ref tests ──────────────────────────────────────

/// `some(some(3))` yields a doubly-wrapped `Option(Some(Option(Some(Int(3)))))`.
#[test]
fn eval_some_nested_some() {
    let inner_inner = CompiledExpr::literal(Value::Int(3), Type::Int);
    let inner = CompiledExpr::option_some(inner_inner, Type::Option(Box::new(Type::Int)));
    let expr = CompiledExpr::option_some(
        inner,
        Type::Option(Box::new(Type::Option(Box::new(Type::Int)))),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Option(Some(Box::new(Value::Option(Some(Box::new(Value::Int(3))))))),
    );
}

/// `some(none)` yields `Option(Some(Option(None)))`.
#[test]
fn eval_some_wraps_none() {
    let inner = CompiledExpr::option_none(Type::Option(Box::new(Type::Int)));
    let expr = CompiledExpr::option_some(
        inner,
        Type::Option(Box::new(Type::Option(Box::new(Type::Int)))),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Option(None)))));
}

/// `some(value_ref(x))` where `x = Int(42)` in the ValueMap yields
/// `Option(Some(Int(42)))`.
#[test]
fn eval_some_value_ref() {
    let x_id = ValueCellId::new("S", "x");
    let inner = CompiledExpr::value_ref(x_id.clone(), Type::Int);
    let expr = CompiledExpr::option_some(inner, Type::Option(Box::new(Type::Int)));
    let mut values = ValueMap::new();
    values.insert(x_id, Value::Int(42));
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Int(42)))));
}

/// `some(value_ref(x))` where `x` is absent from the ValueMap yields
/// `Option(Some(Undef))`, NOT bare `Undef`.
#[test]
fn eval_some_missing_ref_wraps_undef() {
    let x_id = ValueCellId::new("S", "x");
    let inner = CompiledExpr::value_ref(x_id, Type::Int);
    let expr = CompiledExpr::option_some(inner, Type::Option(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Undef))));
    assert_ne!(result, Value::Undef);
}
