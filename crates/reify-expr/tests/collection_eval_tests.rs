//! Collection evaluation tests (list, set, map literals, index access, methods).

// Value::Set/Map use BTreeSet<Value> / BTreeMap<Value, Value>; Value's interior-mutable
// SampledField (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::collections::{BTreeMap, BTreeSet};

use reify_expr::{EvalContext, eval_expr};
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
    assert_eq!(
        result,
        Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
    );
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
    let expr = CompiledExpr::map_literal(
        entries,
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
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
fn eval_index_access_negative_index() {
    // [10, 20, 30][-1] -> Undef (negative indices are rejected)
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(10), Type::Int),
            CompiledExpr::literal(Value::Int(20), Type::Int),
            CompiledExpr::literal(Value::Int(30), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let idx = CompiledExpr::literal(Value::Int(-1), Type::Int);
    let expr = CompiledExpr::index_access(list, idx, Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "negative index should be Undef");
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
            CompiledExpr::literal(
                Value::Scalar {
                    si_value: 0.001,
                    dimension: dim,
                },
                Type::length(),
            ),
            CompiledExpr::literal(
                Value::Scalar {
                    si_value: 0.002,
                    dimension: dim,
                },
                Type::length(),
            ),
        ],
        Type::List(Box::new(Type::length())),
    );
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
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
fn eval_method_sum_empty_real_list() {
    // [].sum() with result_type=Real should return Real(0.0), not Int(0)
    let list = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Real)));
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::Real);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Real(0.0),
        "empty Real list sum should return Real(0.0)"
    );
}

#[test]
fn eval_method_sum_empty_scalar_list() {
    // [].sum() with result_type=Scalar{LENGTH} should return Scalar{0.0, LENGTH}
    let dim = reify_types::DimensionVector::LENGTH;
    let list = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::length())));
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!((si_value - 0.0).abs() < 1e-12, "si_value should be 0.0");
            assert_eq!(dimension, dim, "dimension should be LENGTH");
        }
        other => panic!("expected Scalar, got {:?}", other),
    }
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
    CompiledExpr::literal(
        lambda,
        Type::Function {
            params: vec![],
            return_type: Box::new(Type::Int),
        },
    )
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
    assert_eq!(
        result,
        Value::List(vec![Value::Int(2), Value::Int(4), Value::Int(6)])
    );
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

// ─── step-39: .filter conservative undef retention ───

#[test]
fn eval_method_filter_undef_propagation() {
    // [1, undef, 3].filter(|x| x > 0) -> [1, undef, 3]
    // When x is Undef, Gt(Undef, 0) returns Undef, and filter conservatively retains the element.
    let x_id = ValueCellId::new("$lambda_filter_u.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let undef_id = ValueCellId::new("S", "missing_filter_u");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::value_ref(undef_id, Type::Int),
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
    assert_eq!(
        result,
        Value::List(vec![Value::Int(1), Value::Undef, Value::Int(3)]),
        "[1, undef, 3].filter(|x| x > 0) should conservatively retain undef elements"
    );
}

// ─── task-166 step-3: .filter mixed undef and false results ───

#[test]
fn eval_method_filter_undef_mixed_results() {
    // [1, undef, -1, undef, 5].filter(|x| x > 0) -> [1, undef, undef, 5]
    // true results are included, false results are excluded, undef results are conservatively retained.
    let x_id = ValueCellId::new("$lambda_filter_mix.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let undef_id1 = ValueCellId::new("S", "missing_mix_1");
    let undef_id2 = ValueCellId::new("S", "missing_mix_2");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::value_ref(undef_id1, Type::Int),
            CompiledExpr::literal(Value::Int(-1), Type::Int),
            CompiledExpr::value_ref(undef_id2, Type::Int),
            CompiledExpr::literal(Value::Int(5), Type::Int),
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
    assert_eq!(
        result,
        Value::List(vec![
            Value::Int(1),
            Value::Undef,
            Value::Undef,
            Value::Int(5)
        ]),
        "[1, undef, -1, undef, 5].filter(|x| x > 0) should return [1, undef, undef, 5]"
    );
}

// ─── task-166 step-4: .filter all-undef list ───

#[test]
fn eval_method_filter_all_undef() {
    // [undef, undef].filter(|x| x > 0) -> [undef, undef]
    // All elements have unknown predicate value; all are conservatively retained.
    let x_id = ValueCellId::new("$lambda_filter_all_u.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let undef_id1 = ValueCellId::new("S", "missing_all_u_1");
    let undef_id2 = ValueCellId::new("S", "missing_all_u_2");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::value_ref(undef_id1, Type::Int),
            CompiledExpr::value_ref(undef_id2, Type::Int),
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
    assert_eq!(
        result,
        Value::List(vec![Value::Undef, Value::Undef]),
        "[undef, undef].filter(|x| x > 0) should return [undef, undef]"
    );
}

// ─── task-166 step-5: .filter non-Bool predicate returns Undef ───

#[test]
fn eval_method_filter_non_bool_predicate() {
    // [1, 2, 3].filter(|x| x * 2) where predicate returns Int (not Bool)
    // -> Value::Undef for the entire filter (type error, not incomplete information)
    let x_id = ValueCellId::new("$lambda_filter_nb.S", "x");
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
        "filter".to_string(),
        vec![lambda_arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "[1, 2, 3].filter(|x| x * 2) with non-Bool predicate should return Undef (type error)"
    );
}

// ─── task-166 step-6: .filter empty list ───

#[test]
fn eval_method_filter_empty_list() {
    // [].filter(|x| x > 0) -> []
    // The degenerate case: loop body never executes, result is always an empty list.
    let x_id = ValueCellId::new("$lambda_filter_emp.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let list = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let expr = CompiledExpr::method_call(
        list,
        "filter".to_string(),
        vec![lambda_arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::List(vec![]),
        "[].filter(|x| x > 0) should return []"
    );
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
    let expr =
        CompiledExpr::method_call(list, "fold".to_string(), vec![init, lambda_arg], Type::Int);
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
    let expr =
        CompiledExpr::method_call(list, "fold".to_string(), vec![init, lambda_arg], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Int(16));
}

#[test]
fn eval_method_fold_wrong_arity_lambda_empty_list() {
    // [].fold(0, |x| x + 1) → should be Undef (lambda has 1 param, fold needs 2)
    // On empty lists, fold currently returns init without validating lambda arity
    let x_id = ValueCellId::new("$lambda_fold_bad.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(1), Type::Int),
        Type::Int,
    );
    // 1-param lambda (fold requires 2)
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let init = CompiledExpr::literal(Value::Int(0), Type::Int);
    let list = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let expr =
        CompiledExpr::method_call(list, "fold".to_string(), vec![init, lambda_arg], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "fold with wrong-arity lambda should return Undef even on empty list"
    );
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
    assert!(
        result.is_undef(),
        ".all with undef element and no false -> Undef"
    );
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

// ─── step-37: Kleene 3-valued logic tests for .any ───

#[test]
fn eval_method_any_kleene_undef() {
    // [undef, false].any(|x| x) -> Undef (no true present, undef present => indeterminate)
    let x_id = ValueCellId::new("$lambda_anyk.S", "x");
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Bool);
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let undef_id = ValueCellId::new("S", "missing_any_k");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::value_ref(undef_id, Type::Bool),
            CompiledExpr::literal(Value::Bool(false), Type::Bool),
        ],
        Type::List(Box::new(Type::Bool)),
    );
    let expr = CompiledExpr::method_call(list, "any".to_string(), vec![lambda_arg], Type::Bool);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "[undef, false].any(|x| x) should be Undef"
    );
}

#[test]
fn eval_method_any_kleene_true_wins() {
    // [true, undef].any(|x| x) -> Bool(true) (true dominates undef in Kleene OR)
    let x_id = ValueCellId::new("$lambda_anyk2.S", "x");
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Bool);
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let undef_id = ValueCellId::new("S", "missing_any_k2");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
            CompiledExpr::value_ref(undef_id, Type::Bool),
        ],
        Type::List(Box::new(Type::Bool)),
    );
    let expr = CompiledExpr::method_call(list, "any".to_string(), vec![lambda_arg], Type::Bool);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
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
        Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4)
        ])
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
        vec![CompiledExpr::literal(
            Value::String("a".to_string()),
            Type::String,
        )],
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
        vec![CompiledExpr::literal(
            Value::String("z".to_string()),
            Type::String,
        )],
        Type::Bool,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

// ─── step-27/28: Undef propagation edge cases ───

#[test]
fn eval_undef_count() {
    // undef.count -> Undef
    let id = ValueCellId::new("S", "missing_list");
    let obj = CompiledExpr::value_ref(id, Type::List(Box::new(Type::Int)));
    let expr = CompiledExpr::method_call(obj, "count".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "undef.count should be Undef");
}

#[test]
fn eval_list_with_undef_count() {
    // [1, undef, 3].count -> Undef (count must propagate uncertainty when any element is Undef — three-valued logic)
    let undef_id = ValueCellId::new("S", "missing_elem");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::value_ref(undef_id, Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "count".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "[1,undef,3].count should be Undef (uncertain membership)"
    );
}

#[test]
fn eval_method_count_set_with_undef() {
    // {1, undef, 3}.count -> Undef (Set arm of .count() fix matches List arm)
    let undef_id = ValueCellId::new("S", "missing_set_elem");
    let set = CompiledExpr::set_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::value_ref(undef_id, Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::Set(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(set, "count".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "{{1,undef,3}}.count should be Undef (uncertain membership)"
    );
}

#[test]
fn eval_method_count_definite_list() {
    // [1, 2, 3].count -> Int(3) — regression guard: undef-check must not break the normal case
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
    assert_eq!(result, Value::Int(3), "[1,2,3].count should be Int(3)");
}

#[test]
fn eval_list_with_undef_sum() {
    // [1, undef, 3].sum -> Undef
    let undef_id = ValueCellId::new("S", "missing_elem2");
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::value_ref(undef_id, Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "[1,undef,3].sum should be Undef");
}

#[test]
fn eval_undef_index() {
    // undef[0] -> Undef (already covered in step-5, but verify here too)
    let id = ValueCellId::new("S", "missing_coll");
    let obj = CompiledExpr::value_ref(id, Type::List(Box::new(Type::Int)));
    let idx = CompiledExpr::literal(Value::Int(0), Type::Int);
    let expr = CompiledExpr::index_access(obj, idx, Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "undef[0] should be Undef");
}

#[test]
fn eval_list_undef_index() {
    // [1,2,3][undef] -> Undef (already covered, verify again)
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let undef_id = ValueCellId::new("S", "missing_idx");
    let idx = CompiledExpr::value_ref(undef_id, Type::Int);
    let expr = CompiledExpr::index_access(list, idx, Type::Int);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "[1,2,3][undef] should be Undef");
}

#[test]
fn eval_undef_map_method() {
    // undef.map(lambda) -> Undef
    let x_id = ValueCellId::new("$lambda_undef.S", "x");
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Int);
    let lambda_arg = lambda_literal(vec![("x", x_id)], body, ValueMap::new());

    let id = ValueCellId::new("S", "missing_for_map");
    let obj = CompiledExpr::value_ref(id, Type::List(Box::new(Type::Int)));
    let expr = CompiledExpr::method_call(
        obj,
        "map".to_string(),
        vec![lambda_arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "undef.map(lambda) should be Undef");
}

// ─── step-41: UserFunctionCall inside lambda body in collection method ───

#[test]
fn eval_method_map_with_user_function() {
    // Create a CompiledFunction 'double' that returns x * 2
    // Then create [1, 2, 3].map(|x| double(x)) and verify it produces [2, 4, 6]
    use reify_types::{CompiledExprKind, CompiledFnBody, CompiledFunction, ContentHash};

    let params = vec![("x".to_string(), Type::Int)];
    let double_fn = CompiledFunction {
        name: "double".to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Int,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(ValueCellId::new("double", "x"), Type::Int),
                CompiledExpr::literal(Value::Int(2), Type::Int),
                Type::Int,
            ),
        },
        content_hash: ContentHash::of(b"double_int"),
        annotations: vec![],
        optimized_target: None,
    };

    // Lambda body: double(x) — a UserFunctionCall node
    let x_id = ValueCellId::new("$lambda_uf.S", "x");
    let lambda_body = CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: "double".to_string(),
            args: vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
        },
        result_type: Type::Int,
        content_hash: ContentHash::of(b"double_call"),
    };
    let lambda_arg = lambda_literal(vec![("x", x_id)], lambda_body, ValueMap::new());

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
    let functions = vec![double_fn];
    let ctx = EvalContext::new(&values, &functions);
    let result = eval_expr(&expr, &ctx);
    assert_eq!(
        result,
        Value::List(vec![Value::Int(2), Value::Int(4), Value::Int(6)]),
        "Lambda body with UserFunctionCall should have access to the function registry"
    );
}

// ─── step-3: .filter with user-defined predicate ───

#[test]
fn eval_method_filter_with_user_function() {
    // [1,2,3,4,5].filter(|x| is_even(x)) where is_even checks x % 2 == 0
    // With EvalContext::new containing is_even, should return [2, 4]
    use reify_types::{CompiledExprKind, CompiledFnBody, CompiledFunction, ContentHash};

    // Define user function: is_even(x) = x % 2 == 0
    let params = vec![("x".to_string(), Type::Int)];
    let is_even_fn = CompiledFunction {
        name: "is_even".to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Bool,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::binop(
                BinOp::Eq,
                CompiledExpr::binop(
                    BinOp::Mod,
                    CompiledExpr::value_ref(ValueCellId::new("is_even", "x"), Type::Int),
                    CompiledExpr::literal(Value::Int(2), Type::Int),
                    Type::Int,
                ),
                CompiledExpr::literal(Value::Int(0), Type::Int),
                Type::Bool,
            ),
        },
        content_hash: ContentHash::of(b"is_even_fn"),
        annotations: vec![],
        optimized_target: None,
    };

    // Lambda body: is_even(x)
    let x_id = ValueCellId::new("$lambda_filter_uf.S", "x");
    let lambda_body = CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: "is_even".to_string(),
            args: vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
        },
        result_type: Type::Bool,
        content_hash: ContentHash::of(b"is_even_call"),
    };
    let lambda_arg = lambda_literal(vec![("x", x_id)], lambda_body, ValueMap::new());

    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
            CompiledExpr::literal(Value::Int(4), Type::Int),
            CompiledExpr::literal(Value::Int(5), Type::Int),
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
    let functions = vec![is_even_fn];
    let ctx = EvalContext::new(&values, &functions);
    let result = eval_expr(&expr, &ctx);
    assert_eq!(
        result,
        Value::List(vec![Value::Int(2), Value::Int(4)]),
        ".filter with user-defined predicate should use the function registry"
    );
}

// ─── step-5: .fold with user-defined combiner ───

#[test]
fn eval_method_fold_with_user_function() {
    // [1,2,3].fold(0, |acc, x| add(acc, x)) where add(a, b) = a + b
    // With EvalContext::new containing add, should return Int(6)
    use reify_types::{CompiledExprKind, CompiledFnBody, CompiledFunction, ContentHash};

    // Define user function: add(a, b) = a + b
    let params = vec![("a".to_string(), Type::Int), ("b".to_string(), Type::Int)];
    let add_fn = CompiledFunction {
        name: "add".to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Int,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::binop(
                BinOp::Add,
                CompiledExpr::value_ref(ValueCellId::new("add", "a"), Type::Int),
                CompiledExpr::value_ref(ValueCellId::new("add", "b"), Type::Int),
                Type::Int,
            ),
        },
        content_hash: ContentHash::of(b"add_fn"),
        annotations: vec![],
        optimized_target: None,
    };

    // Lambda body: add(acc, x)
    let acc_id = ValueCellId::new("$lambda_fold_uf.S", "acc");
    let x_id = ValueCellId::new("$lambda_fold_uf.S", "x");
    let lambda_body = CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: "add".to_string(),
            args: vec![
                CompiledExpr::value_ref(acc_id.clone(), Type::Int),
                CompiledExpr::value_ref(x_id.clone(), Type::Int),
            ],
        },
        result_type: Type::Int,
        content_hash: ContentHash::of(b"add_call"),
    };
    let lambda_arg = lambda_literal(
        vec![("acc", acc_id), ("x", x_id)],
        lambda_body,
        ValueMap::new(),
    );

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
        vec![CompiledExpr::literal(Value::Int(0), Type::Int), lambda_arg],
        Type::Int,
    );

    let values = ValueMap::new();
    let functions = vec![add_fn];
    let ctx = EvalContext::new(&values, &functions);
    let result = eval_expr(&expr, &ctx);
    assert_eq!(
        result,
        Value::Int(6),
        ".fold with user-defined combiner should use the function registry"
    );
}

// ─── step-7: .all/.any with user-defined predicates ───

#[test]
fn eval_method_all_any_with_user_function() {
    // [2,4,6].all(|x| is_even(x)) → true
    // [1,2,3].any(|x| is_even(x)) → true
    // [1,3,5].any(|x| is_even(x)) → false
    use reify_types::{CompiledExprKind, CompiledFnBody, CompiledFunction, ContentHash};

    // Define user function: is_even(x) = x % 2 == 0
    let params = vec![("x".to_string(), Type::Int)];
    let is_even_fn = CompiledFunction {
        name: "is_even".to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Bool,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::binop(
                BinOp::Eq,
                CompiledExpr::binop(
                    BinOp::Mod,
                    CompiledExpr::value_ref(ValueCellId::new("is_even", "x"), Type::Int),
                    CompiledExpr::literal(Value::Int(2), Type::Int),
                    Type::Int,
                ),
                CompiledExpr::literal(Value::Int(0), Type::Int),
                Type::Bool,
            ),
        },
        content_hash: ContentHash::of(b"is_even_fn_all_any"),
        annotations: vec![],
        optimized_target: None,
    };

    let x_id = ValueCellId::new("$lambda_all_any_uf.S", "x");
    let make_is_even_lambda = |x_id: ValueCellId| {
        let lambda_body = CompiledExpr {
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "is_even".to_string(),
                args: vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
            },
            result_type: Type::Bool,
            content_hash: ContentHash::of(b"is_even_call_all_any"),
        };
        lambda_literal(vec![("x", x_id)], lambda_body, ValueMap::new())
    };

    let values = ValueMap::new();
    let functions = vec![is_even_fn];
    let ctx = EvalContext::new(&values, &functions);

    // [2,4,6].all(|x| is_even(x)) → true
    let all_evens = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(4), Type::Int),
            CompiledExpr::literal(Value::Int(6), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let all_expr = CompiledExpr::method_call(
        all_evens,
        "all".to_string(),
        vec![make_is_even_lambda(x_id.clone())],
        Type::Bool,
    );
    let all_result = eval_expr(&all_expr, &ctx);
    assert_eq!(
        all_result,
        Value::Bool(true),
        "[2,4,6].all(is_even) should be true"
    );

    // [1,2,3].any(|x| is_even(x)) → true
    let mixed = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let any_true_expr = CompiledExpr::method_call(
        mixed,
        "any".to_string(),
        vec![make_is_even_lambda(x_id.clone())],
        Type::Bool,
    );
    let any_true_result = eval_expr(&any_true_expr, &ctx);
    assert_eq!(
        any_true_result,
        Value::Bool(true),
        "[1,2,3].any(is_even) should be true"
    );

    // [1,3,5].any(|x| is_even(x)) → false
    let all_odds = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
            CompiledExpr::literal(Value::Int(5), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let any_false_expr = CompiledExpr::method_call(
        all_odds,
        "any".to_string(),
        vec![make_is_even_lambda(x_id)],
        Type::Bool,
    );
    let any_false_result = eval_expr(&any_false_expr, &ctx);
    assert_eq!(
        any_false_result,
        Value::Bool(false),
        "[1,3,5].any(is_even) should be false"
    );
}

// ─── step-1/2: concat Undef propagation ───

#[test]
fn eval_method_concat_undef_object() {
    // undef.concat([1, 2]) -> Undef
    // The MethodCall dispatch short-circuits on obj.is_undef() (line 202 of lib.rs).
    let undef_id = ValueCellId::new("S", "missing_concat_obj");
    let obj = CompiledExpr::value_ref(undef_id, Type::List(Box::new(Type::Int)));
    let arg = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        obj,
        "concat".to_string(),
        vec![arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "concat on Undef object should be Undef");
}

#[test]
fn eval_method_concat_undef_arg() {
    // [1, 2].concat(undef) -> Undef
    // The (List, List) match catch-all returns Undef when arg is not a List.
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let undef_id = ValueCellId::new("S", "missing_concat_arg");
    let arg = CompiledExpr::value_ref(undef_id, Type::List(Box::new(Type::Int)));
    let expr = CompiledExpr::method_call(
        list,
        "concat".to_string(),
        vec![arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "concat with Undef arg should be Undef");
}

// ─── step-3/4: concat empty list identity ───

#[test]
fn eval_method_concat_empty_left() {
    // [].concat([1, 2]) -> [1, 2]
    let empty = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let right = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        empty,
        "concat".to_string(),
        vec![right],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::List(vec![Value::Int(1), Value::Int(2)]));
}

#[test]
fn eval_method_concat_empty_right() {
    // [1, 2].concat([]) -> [1, 2]
    let left = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let empty = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let expr = CompiledExpr::method_call(
        left,
        "concat".to_string(),
        vec![empty],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::List(vec![Value::Int(1), Value::Int(2)]));
}

#[test]
fn eval_method_concat_both_empty() {
    // [].concat([]) -> []
    let left = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let right = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let expr = CompiledExpr::method_call(
        left,
        "concat".to_string(),
        vec![right],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::List(vec![]));
}

// ─── step-5/6: concat different element types and Undef elements ───

#[test]
fn eval_method_concat_string_lists() {
    // ["a", "b"].concat(["c"]) -> ["a", "b", "c"]
    let left = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::String("a".to_string()), Type::String),
            CompiledExpr::literal(Value::String("b".to_string()), Type::String),
        ],
        Type::List(Box::new(Type::String)),
    );
    let right = CompiledExpr::list_literal(
        vec![CompiledExpr::literal(
            Value::String("c".to_string()),
            Type::String,
        )],
        Type::List(Box::new(Type::String)),
    );
    let expr = CompiledExpr::method_call(
        left,
        "concat".to_string(),
        vec![right],
        Type::List(Box::new(Type::String)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::List(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
            Value::String("c".to_string()),
        ])
    );
}

#[test]
fn eval_method_concat_with_undef_elements() {
    // [1, undef].concat([undef, 4]) -> [1, undef, undef, 4]
    // Undef elements inside lists are preserved, not filtered.
    let undef_id1 = ValueCellId::new("S", "missing_elem_1");
    let undef_id2 = ValueCellId::new("S", "missing_elem_2");
    let left = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::value_ref(undef_id1, Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let right = CompiledExpr::list_literal(
        vec![
            CompiledExpr::value_ref(undef_id2, Type::Int),
            CompiledExpr::literal(Value::Int(4), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        left,
        "concat".to_string(),
        vec![right],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::List(vec![
            Value::Int(1),
            Value::Undef,
            Value::Undef,
            Value::Int(4)
        ])
    );
}

// ─── step-7/8: concat error / edge cases ───

#[test]
fn eval_method_concat_non_list_arg() {
    // [1, 2].concat(42) -> Undef (non-list argument falls through match catch-all)
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let non_list_arg = CompiledExpr::literal(Value::Int(42), Type::Int);
    let expr = CompiledExpr::method_call(
        list,
        "concat".to_string(),
        vec![non_list_arg],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        result.is_undef(),
        "concat with non-list arg should be Undef"
    );
}

#[test]
fn eval_method_concat_zero_args() {
    // [1, 2].concat() -> Undef (wrong arg count: 0)
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        list,
        "concat".to_string(),
        vec![],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "concat with 0 args should be Undef");
}

#[test]
fn eval_method_concat_two_args() {
    // [1, 2].concat([3], [4]) -> Undef (wrong arg count: 2)
    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let arg1 = CompiledExpr::list_literal(
        vec![CompiledExpr::literal(Value::Int(3), Type::Int)],
        Type::List(Box::new(Type::Int)),
    );
    let arg2 = CompiledExpr::list_literal(
        vec![CompiledExpr::literal(Value::Int(4), Type::Int)],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr::method_call(
        list,
        "concat".to_string(),
        vec![arg1, arg2],
        Type::List(Box::new(Type::Int)),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef(), "concat with 2 args should be Undef");
}
