//! Quantifier evaluation tests.

use reify_expr::{eval_expr, EvalContext};
use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, QuantifierKind, Type, Value, ValueCellId, ValueMap,
};

/// Helper: create a quantifier CompiledExpr.
fn make_quantifier(
    kind: QuantifierKind,
    var_name: &str,
    var_id: ValueCellId,
    collection: CompiledExpr,
    predicate: CompiledExpr,
) -> CompiledExpr {
    CompiledExpr::quantifier(kind, var_name.to_string(), var_id, collection, predicate)
}

/// step-5: forall over [1,2,3] with x>0 -> true
#[test]
fn forall_all_true() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

/// step-5: forall over [1,-1,3] with x>0 -> false
#[test]
fn forall_has_false() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(-1), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

/// step-5: exists over [1,2,3] with x>2 -> true
#[test]
fn exists_has_true() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

/// step-5: exists over [1,2,3] with x>5 -> false
#[test]
fn exists_all_false() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(5), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}
