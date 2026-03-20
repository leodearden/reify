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

// ─── Helpers for lambda construction ───

/// Build a Value::Lambda from param names/ids, body CompiledExpr, and captures.
fn make_value_lambda(
    params: Vec<(&str, ValueCellId)>,
    body: CompiledExpr,
    captures: ValueMap,
) -> Value {
    Value::Lambda {
        params: params
            .into_iter()
            .map(|(n, id)| (n.to_string(), id))
            .collect(),
        body: Box::new(body),
        captures,
    }
}

/// Build a CompiledExpr::Literal containing a lambda value.
fn lambda_literal(
    params: Vec<(&str, ValueCellId)>,
    body: CompiledExpr,
    captures: ValueMap,
) -> CompiledExpr {
    let lambda = make_value_lambda(params, body, captures);
    CompiledExpr::literal(lambda, Type::Function {
        params: vec![],
        return_type: Box::new(Type::Int),
    })
}

// ─── step-13/14: MethodCall .map ───

#[test]
fn eval_method_map_list() {
    // [1, 2, 3].map(|x| x * 2) -> [2, 4, 6]
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Int,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

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
        "map".to_string(),
        vec![lambda_arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::List(vec![Value::Int(2), Value::Int(4), Value::Int(6)]));
}

// ─── step-15/16: MethodCall .filter ───

#[test]
fn eval_method_filter_list() {
    // [1, 2, 3, 4].filter(|x| x > 2) -> [3, 4]
    let x_id = ValueCellId::new("$lambda_filter.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
            CompiledExpr::literal(Value::Int(4), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        list,
        "filter".to_string(),
        vec![lambda_arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::List(vec![Value::Int(3), Value::Int(4)]));
}

#[test]
fn eval_method_filter_empty_result() {
    // [1, 2, 3].filter(|x| x > 10) -> []
    let x_id = ValueCellId::new("$lambda_filter2.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(10), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

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
        "filter".to_string(),
        vec![lambda_arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::List(vec![]));
}

// ─── step-17/18: MethodCall .fold ───

#[test]
fn eval_method_fold_sum() {
    // [1, 2, 3].fold(0, |acc, x| acc + x) -> Int(6)
    let acc_id = ValueCellId::new("$lambda_fold.S", "acc");
    let x_id = ValueCellId::new("$lambda_fold.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(acc_id.clone(), Type::Int),
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        Type::Int,
    );
    let lambda_arg = lambda_literal(vec![("acc", acc_id), ("x", x_id)], body, ValueMap::new());

    let init = CompiledExpr::literal(Value::Int(0), Type::Int);
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
        "fold".to_string(),
        vec![init, lambda_arg],
        Type::Int,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(6));
}

#[test]
fn eval_method_fold_with_initial() {
    // [1, 2, 3].fold(10, |acc, x| acc + x) -> Int(16)
    let acc_id = ValueCellId::new("$lambda_fold2.S", "acc");
    let x_id = ValueCellId::new("$lambda_fold2.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(acc_id.clone(), Type::Int),
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        Type::Int,
    );
    let lambda_arg = lambda_literal(vec![("acc", acc_id), ("x", x_id)], body, ValueMap::new());

    let init = CompiledExpr::literal(Value::Int(10), Type::Int);
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
        "fold".to_string(),
        vec![init, lambda_arg],
        Type::Int,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(16));
}

// ─── step-19/20: MethodCall .all and .any ───

#[test]
fn eval_method_all_true() {
    // [1, 2, 3].all(|x| x > 0) -> Bool(true)
    let x_id = ValueCellId::new("$lambda_all.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "all".to_string(), vec![lambda_arg], Type::Bool);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn eval_method_all_false() {
    // [1, 2, 3].all(|x| x > 2) -> Bool(false)
    let x_id = ValueCellId::new("$lambda_all2.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "all".to_string(), vec![lambda_arg], Type::Bool);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn eval_method_any_true() {
    // [1, 2, 3].any(|x| x > 2) -> Bool(true)
    let x_id = ValueCellId::new("$lambda_any.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "any".to_string(), vec![lambda_arg], Type::Bool);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn eval_method_any_false() {
    // [1, 2, 3].any(|x| x > 5) -> Bool(false)
    let x_id = ValueCellId::new("$lambda_any2.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(5), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "any".to_string(), vec![lambda_arg], Type::Bool);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn eval_method_all_kleene_undef() {
    // [1, undef, 3].all(|x| x > 0) -> Undef (no false, but undef present)
    let x_id = ValueCellId::new("$lambda_allk.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let undef_id = ValueCellId::new("S", "missing");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::value_ref(undef_id, Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "all".to_string(), vec![lambda_arg], Type::Bool);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), ".all with undef element and no false -> Undef");
}

#[test]
fn eval_method_all_kleene_false_wins() {
    // [false, undef].all(|x| x) -> Bool(false) (false dominates undef)
    // We need a lambda that just returns its argument (identity)
    let x_id = ValueCellId::new("$lambda_allk2.S", "x");
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Bool);
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let undef_id = ValueCellId::new("S", "missing2");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Bool(false), Type::Bool),
            CompiledExpr::value_ref(undef_id, Type::Bool),
        ],
        Type::List(Box::new(Type::Bool)),
    );
    let expr = CompiledExpr::method_call(list, "all".to_string(), vec![lambda_arg], Type::Bool);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

// ─── step-21/22: MethodCall .concat and .generate ───

#[test]
fn eval_method_concat_lists() {
    // [1, 2].concat([3, 4]) -> [1, 2, 3, 4]
    let list1 = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let list2_arg = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(3), Type::Int),
            CompiledExpr::literal(Value::Int(4), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    // concat takes 1 arg (the other list), but the arg is a CompiledExpr
    // that is evaluated to Value::List by the MethodCall dispatch.
    let expr = CompiledExpr::method_call(
        list1,
        "concat".to_string(),
        vec![list2_arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)])
    );
}

#[test]
fn eval_method_generate_list() {
    // [].generate(3, |i| i * 2) -> [0, 2, 4]
    let i_id = ValueCellId::new("$lambda_gen.S", "i");
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(i_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Int,
    );
    let lambda_arg = lambda_literal(vec![("i", i_id)], body, ValueMap::new());

    let list = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let count_arg = CompiledExpr::literal(Value::Int(3), Type::Int);
    let expr = CompiledExpr::method_call(
        list,
        "generate".to_string(),
        vec![count_arg, lambda_arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::List(vec![Value::Int(0), Value::Int(2), Value::Int(4)])
    );
}

// ─── step-23/24: Set operations ───

#[test]
fn eval_method_set_union() {
    // set{1, 2, 3}.union(set{3, 4}) -> Set({1, 2, 3, 4})
    let set1 = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let set2_arg = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(3), Type::Int),
            CompiledExpr::literal(Value::Int(4), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        set1,
        "union".to_string(),
        vec![set2_arg],
        Type::Set(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let expected: BTreeSet<Value> = [Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)]
        .into_iter()
        .collect();
    assert_eq!(result, Value::Set(expected));
}

#[test]
fn eval_method_set_intersection() {
    // set{1, 2, 3}.intersection(set{2, 3, 4}) -> Set({2, 3})
    let set1 = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let set2_arg = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
            CompiledExpr::literal(Value::Int(4), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        set1,
        "intersection".to_string(),
        vec![set2_arg],
        Type::Set(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let expected: BTreeSet<Value> = [Value::Int(2), Value::Int(3)].into_iter().collect();
    assert_eq!(result, Value::Set(expected));
}

#[test]
fn eval_method_set_difference() {
    // set{1, 2, 3}.difference(set{2, 3, 4}) -> Set({1})
    let set1 = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let set2_arg = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
            CompiledExpr::literal(Value::Int(4), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        set1,
        "difference".to_string(),
        vec![set2_arg],
        Type::Set(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let expected: BTreeSet<Value> = [Value::Int(1)].into_iter().collect();
    assert_eq!(result, Value::Set(expected));
}

// ─── step-25/26: Map methods (.keys, .values, .contains_key) ───

fn make_ab_map() -> CompiledExpr {
    CompiledExpr::map_literal(
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
    )
}

#[test]
fn eval_method_map_keys() {
    let map = make_ab_map();
    let expr = CompiledExpr::method_call(
        map,
        "keys".to_string(),
        vec![],
        Type::List(Box::new(Type::String)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    // BTreeMap keys are sorted, so "a" < "b"
    assert_eq!(
        result,
        Value::List(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
        ])
    );
}

#[test]
fn eval_method_map_values() {
    let map = make_ab_map();
    let expr = CompiledExpr::method_call(
        map,
        "values".to_string(),
        vec![],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    // BTreeMap values ordered by key, so a=>1 before b=>2
    assert_eq!(result, Value::List(vec![Value::Int(1), Value::Int(2)]));
}

#[test]
fn eval_method_map_contains_key_found() {
    let map = make_ab_map();
    let expr = CompiledExpr::method_call(
        map,
        "contains_key".to_string(),
        vec![CompiledExpr::literal(Value::String("a".to_string()), Type::String)],
        Type::Bool,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn eval_method_map_contains_key_not_found() {
    let map = make_ab_map();
    let expr = CompiledExpr::method_call(
        map,
        "contains_key".to_string(),
        vec![CompiledExpr::literal(Value::String("z".to_string()), Type::String)],
        Type::Bool,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}
