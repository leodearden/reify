//! Collection evaluation tests (list, set, map literals, index access, methods).

// Value::Set/Map use BTreeSet<Value> / BTreeMap<Value, Value>; Value's interior-mutable
// SampledField (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::collections::{BTreeMap, BTreeSet};

use reify_expr::{EvalContext, eval_expr};
use reify_core::{Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};

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
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar()),
        ],
        Type::List(Box::new(Type::dimensionless_scalar())),
    );
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Real(3.0));
}

#[test]
fn eval_method_sum_scalars() {
    let dim = reify_core::DimensionVector::LENGTH;
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
    let list = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::dimensionless_scalar())));
    let expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], Type::dimensionless_scalar());
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
    let dim = reify_core::DimensionVector::LENGTH;
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

// ─── step-25/26: Map methods (.keys, .values) ───

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

// ─── MethodCall is a zero-argument member projection (task #6406, GR-040) ───

/// Reify has no method-call syntax (GR-040), so the compiler only ever emits a
/// zero-argument `MethodCall`. An argument-bearing one evaluates to `Undef`
/// whatever the method name, including the zero-argument members.
#[test]
fn argument_bearing_method_call_evaluates_to_undef() {
    let int = |n: i64| CompiledExpr::literal(Value::Int(n), Type::Int);
    let ints = || Type::List(Box::new(Type::Int));
    let int_set_type = || Type::Set(Box::new(Type::Int));
    let int_list =
        |ns: &[i64]| CompiledExpr::list_literal(ns.iter().map(|&n| int(n)).collect(), ints());
    let int_set = |ns: &[i64]| {
        CompiledExpr::set_literal(ns.iter().map(|&n| int(n)).collect(), int_set_type())
    };
    let string = |s: &str| CompiledExpr::literal(Value::String(s.to_string()), Type::String);

    let v_id = ValueCellId::new("$lambda_contract.S", "v");
    let acc_id = ValueCellId::new("$lambda_contract.S", "acc");
    let v = || CompiledExpr::value_ref(v_id.clone(), Type::Int);
    let lambda_v =
        |body: CompiledExpr| lambda_literal(vec![("v", v_id.clone())], body, ValueMap::new());
    let inc = lambda_v(CompiledExpr::binop(BinOp::Add, v(), int(1), Type::Int));
    let gt1 = lambda_v(CompiledExpr::binop(BinOp::Gt, v(), int(1), Type::Bool));
    let gt0 = lambda_v(CompiledExpr::binop(BinOp::Gt, v(), int(0), Type::Bool));
    let identity = lambda_v(v());
    let add_acc = lambda_literal(
        vec![("acc", acc_id.clone()), ("v", v_id.clone())],
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(acc_id.clone(), Type::Int),
            v(),
            Type::Int,
        ),
        ValueMap::new(),
    );

    let list_calls = vec![
        ("map", vec![inc], ints()),
        ("filter", vec![gt1], ints()),
        ("all", vec![gt0.clone()], Type::Bool),
        ("any", vec![gt0], Type::Bool),
        ("fold", vec![int(0), add_acc], Type::Int),
        ("concat", vec![int_list(&[4])], ints()),
        ("generate", vec![int(3), identity], ints()),
        ("contains", vec![int(2)], Type::Bool),
        ("count", vec![int(1)], Type::Int),
        ("sum", vec![int(1)], Type::Int),
    ];
    let set_calls = vec![
        ("union", vec![int_set(&[3])], int_set_type()),
        ("intersection", vec![int_set(&[2])], int_set_type()),
        ("difference", vec![int_set(&[2])], int_set_type()),
        ("contains", vec![int(1)], Type::Bool),
    ];
    let map_calls = vec![
        ("contains_key", vec![string("a")], Type::Bool),
        ("keys", vec![int(1)], Type::List(Box::new(Type::String))),
    ];
    let range_calls = vec![
        ("contains", vec![int(5)], Type::Bool),
        ("span", vec![int(1)], Type::Int),
    ];

    let a_to_1 = CompiledExpr::map_literal(
        vec![(string("a"), int(1))],
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let one_to_ten = CompiledExpr::literal(
        Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true),
        Type::Range(Box::new(Type::Int)),
    );
    let cases = [
        (int_list(&[1, 2, 3]), list_calls),
        (int_set(&[1, 2]), set_calls),
        (a_to_1, map_calls),
        (one_to_ten, range_calls),
    ];

    let values = ValueMap::new();
    let mut defined = Vec::new();
    for (receiver, calls) in cases {
        let receiver_type = &receiver.result_type;
        for (method, args, result_type) in calls {
            let expr =
                CompiledExpr::method_call(receiver.clone(), method.to_string(), args, result_type);
            let result = eval_expr(&expr, &EvalContext::simple(&values));
            if result != Value::Undef {
                defined.push(format!("{receiver_type:?}.{method}(..) = {result:?}"));
            }
        }
    }
    assert!(
        defined.is_empty(),
        "an argument-bearing MethodCall must evaluate to Undef; these did not: {defined:#?}"
    );
}

// ─── UserFunctionCall inside a lambda body applied by a builtin ───

/// `flat_map([1, 2, 3], |x| [double(x)])` → `[2, 4, 6]`: the lambda applied by
/// a builtin still reaches the caller's function registry.
#[test]
fn flat_map_lambda_body_reaches_the_user_function_registry() {
    use reify_core::ContentHash;
    use reify_ir::{CompiledExprKind, CompiledFnBody, CompiledFunction, ResolvedFunction};

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
        type_params: vec![],
    };

    let x_id = ValueCellId::new("$lambda_flat_map_uf.S", "x");
    let double_call = CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: "double".to_string(),
            args: vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
        },
        result_type: Type::Int,
        content_hash: ContentHash::of(b"double_call"),
    };
    let lambda_body =
        CompiledExpr::list_literal(vec![double_call], Type::List(Box::new(Type::Int)));
    let lambda_arg = lambda_literal(vec![("x", x_id)], lambda_body, ValueMap::new());

    let list = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let expr = CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "flat_map".to_string(),
                qualified_name: "std::flat_map".to_string(),
            },
            args: vec![list, lambda_arg],
        },
        result_type: Type::List(Box::new(Type::Int)),
        content_hash: ContentHash::of(b"flat_map_double"),
    };

    let values = ValueMap::new();
    let functions = vec![double_fn];
    let result = eval_expr(&expr, &EvalContext::new(&values, &functions));
    assert_eq!(
        result,
        Value::List(vec![Value::Int(2), Value::Int(4), Value::Int(6)])
    );
}
