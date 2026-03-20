//! Collection evaluation tests (list, set, map literals, index access, methods).

use std::collections::{BTreeMap, BTreeSet};

use reify_expr::{eval_expr, EvalContext};
use reify_types::{BinOp, CompiledExpr, Type, Value, ValueCellId, ValueMap};

// ─── step-1: List literal evaluation ───

#[test]
fn eval_list_literal_ints() {
    let elems = vec![
        CompiledExpr::literal(Value::Int(1), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        CompiledExpr::literal(Value::Int(3), Type::Int),
    ];
    let expr = CompiledExpr::list_literal(elems, Type::List(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
}

#[test]
fn eval_list_literal_empty() {
    let expr = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::List(vec![]));
}

#[test]
fn eval_list_literal_nested_expr() {
    // [1 + 2, 3 * 4]
    let elems = vec![
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            Type::Int,
        ),
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::literal(Value::Int(3), Type::Int),
            CompiledExpr::literal(Value::Int(4), Type::Int),
            Type::Int,
        ),
    ];
    let expr = CompiledExpr::list_literal(elems, Type::List(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::List(vec![Value::Int(3), Value::Int(12)]));
}

// ─── step-3: Set and Map literal evaluation ───

#[test]
fn eval_set_literal() {
    let elems = vec![
        CompiledExpr::literal(Value::Int(1), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        CompiledExpr::literal(Value::Int(3), Type::Int),
    ];
    let expr = CompiledExpr::set_literal(elems, Type::Set(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let expected: BTreeSet<Value> = [Value::Int(1), Value::Int(2), Value::Int(3)]
        .into_iter()
        .collect();
    assert_eq!(result, Value::Set(expected));
}

#[test]
fn eval_set_literal_dedup() {
    // set{1, 2, 2, 3} should dedup to {1, 2, 3}
    let elems = vec![
        CompiledExpr::literal(Value::Int(1), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        CompiledExpr::literal(Value::Int(3), Type::Int),
    ];
    let expr = CompiledExpr::set_literal(elems, Type::Set(Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    match &result {
        Value::Set(s) => assert_eq!(s.len(), 3, "set should deduplicate"),
        other => panic!("expected Value::Set, got {:?}", other),
    }
}

#[test]
fn eval_map_literal() {
    let entries = vec![
        (
            CompiledExpr::literal(Value::String("a".to_string()), Type::String),
            CompiledExpr::literal(Value::Int(1), Type::Int),
        ),
        (
            CompiledExpr::literal(Value::String("b".to_string()), Type::String),
            CompiledExpr::literal(Value::Int(2), Type::Int),
        ),
    ];
    let expr = CompiledExpr::map_literal(entries, Type::Map(Box::new(Type::String), Box::new(Type::Int)));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let mut expected = BTreeMap::new();
    expected.insert(Value::String("a".to_string()), Value::Int(1));
    expected.insert(Value::String("b".to_string()), Value::Int(2));
    assert_eq!(result, Value::Map(expected));
}
