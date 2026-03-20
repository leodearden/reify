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

// ─── step-5: Index access evaluation ───

#[test]
fn eval_index_access_list() {
    // [10, 20, 30][1] -> 20
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(10), Type::Int),
            CompiledExpr::literal(Value::Int(20), Type::Int),
            CompiledExpr::literal(Value::Int(30), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let idx = CompiledExpr::literal(Value::Int(1), Type::Int);
    let expr = CompiledExpr::index_access(list, idx, Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(20));
}

#[test]
fn eval_index_access_list_out_of_bounds() {
    let list = CompiledExpr::list_literal(
        vec![CompiledExpr::literal(Value::Int(1), Type::Int)],
        Type::List(Box::new(Type::Int)),
    );
    let idx = CompiledExpr::literal(Value::Int(5), Type::Int);
    let expr = CompiledExpr::index_access(list, idx, Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "out of bounds should be Undef");
}

#[test]
fn eval_index_access_map() {
    // map{"a" => 1, "b" => 2}["b"] -> 2
    let map = CompiledExpr::map_literal(
        vec![
            (
                CompiledExpr::literal(Value::String("a".to_string()), Type::String),
                CompiledExpr::literal(Value::Int(1), Type::Int),
            ),
            (
                CompiledExpr::literal(Value::String("b".to_string()), Type::String),
                CompiledExpr::literal(Value::Int(2), Type::Int),
            ),
        ],
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let key = CompiledExpr::literal(Value::String("b".to_string()), Type::String);
    let expr = CompiledExpr::index_access(map, key, Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(2));
}

#[test]
fn eval_index_access_map_missing_key() {
    let map = CompiledExpr::map_literal(
        vec![(
            CompiledExpr::literal(Value::String("a".to_string()), Type::String),
            CompiledExpr::literal(Value::Int(1), Type::Int),
        )],
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let key = CompiledExpr::literal(Value::String("z".to_string()), Type::String);
    let expr = CompiledExpr::index_access(map, key, Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "missing key should be Undef");
}

#[test]
fn eval_index_access_undef_collection() {
    // undef[0] -> Undef
    let id = ValueCellId::new("S", "missing");
    let obj = CompiledExpr::value_ref(id, Type::List(Box::new(Type::Int)));
    let idx = CompiledExpr::literal(Value::Int(0), Type::Int);
    let expr = CompiledExpr::index_access(obj, idx, Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "indexing Undef should be Undef");
}

#[test]
fn eval_index_access_undef_index() {
    // [1,2,3][undef] -> Undef
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let undef_id = ValueCellId::new("S", "missing");
    let idx = CompiledExpr::value_ref(undef_id, Type::Int);
    let expr = CompiledExpr::index_access(list, idx, Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "indexing with Undef should be Undef");
}

// ─── step-7: MethodCall .count ───

#[test]
fn eval_method_count_list() {
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "count".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(3));
}

#[test]
fn eval_method_count_set() {
    let set = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(set, "count".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(3));
}

#[test]
fn eval_method_count_map() {
    let map = CompiledExpr::map_literal(
        vec![(
            CompiledExpr::literal(Value::String("a".to_string()), Type::String),
            CompiledExpr::literal(Value::Int(1), Type::Int),
        )],
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(map, "count".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(1));
}

#[test]
fn eval_method_count_empty_list() {
    let list = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let expr = CompiledExpr::method_call(list, "count".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(0));
}

// ─── step-9: MethodCall .sum ───

#[test]
fn eval_method_sum_ints() {
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(6));
}

#[test]
fn eval_method_sum_reals() {
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
            CompiledExpr::literal(Value::Real(2.0), Type::Real),
        ],
        Type::List(Box::new(Type::Real)),
    );
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::Real);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Real(3.0));
}

#[test]
fn eval_method_sum_scalars() {
    let dim = reify_types::DimensionVector::LENGTH;
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Scalar { si_value: 0.001, dimension: dim }, Type::length()),
            CompiledExpr::literal(Value::Scalar { si_value: 0.002, dimension: dim }, Type::length()),
        ],
        Type::List(Box::new(Type::length())),
    );
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    match result {
        Value::Scalar { si_value, dimension } => {
            assert!((si_value - 0.003).abs() < 1e-12);
            assert_eq!(dimension, dim);
        }
        other => panic!("expected Scalar, got {:?}", other),
    }
}

#[test]
fn eval_method_sum_empty() {
    let list = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(0));
}

#[test]
fn eval_method_sum_with_undef_element() {
    let id = ValueCellId::new("S", "missing");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::value_ref(id, Type::Int), // will be Undef
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), ".sum with Undef element should be Undef");
}

// ─── step-11: MethodCall .contains ───

#[test]
fn eval_method_contains_list_found() {
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        list,
        "contains".to_string(),
        vec![CompiledExpr::literal(Value::Int(2), Type::Int)],
        Type::Bool,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn eval_method_contains_list_not_found() {
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        list,
        "contains".to_string(),
        vec![CompiledExpr::literal(Value::Int(5), Type::Int)],
        Type::Bool,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn eval_method_contains_set_found() {
    let set = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        set,
        "contains".to_string(),
        vec![CompiledExpr::literal(Value::Int(2), Type::Int)],
        Type::Bool,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn eval_method_contains_set_not_found() {
    let set = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        set,
        "contains".to_string(),
        vec![CompiledExpr::literal(Value::Int(5), Type::Int)],
        Type::Bool,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}
