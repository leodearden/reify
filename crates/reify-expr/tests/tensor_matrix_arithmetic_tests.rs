//! Matrix arithmetic tests: canonicalization through eval_binop, eval_unop, and eval_method_call.

use reify_expr::{eval_expr, EvalContext};
use reify_types::{BinOp, CompiledExpr, Type, UnOp, Value, ValueMap};

/// Build a 2×2 Matrix from four f64 values.
fn matrix_2x2(a: f64, b: f64, c: f64, d: f64) -> Value {
    Value::Matrix(vec![
        vec![Value::Real(a), Value::Real(b)],
        vec![Value::Real(c), Value::Real(d)],
    ])
}

// ── step-11: Matrix canonicalization through eval_binop and eval_unop ──────

#[test]
fn matrix_plus_matrix() {
    // matrix_2x2(1,2,3,4) + matrix_2x2(10,20,30,40) => canonicalized tensor arithmetic
    let lhs = CompiledExpr::literal(matrix_2x2(1.0, 2.0, 3.0, 4.0), Type::matrix(2, 2, Type::Real));
    let rhs = CompiledExpr::literal(matrix_2x2(10.0, 20.0, 30.0, 40.0), Type::matrix(2, 2, Type::Real));
    let expr = CompiledExpr::binop(BinOp::Add, lhs, rhs, Type::tensor(2, 2, Type::Real));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let expected = Value::Tensor(vec![
        Value::Tensor(vec![Value::Real(11.0), Value::Real(22.0)]),
        Value::Tensor(vec![Value::Real(33.0), Value::Real(44.0)]),
    ]);
    assert_eq!(result, expected);
}

#[test]
fn scalar_times_matrix() {
    // Real(2.0) * matrix_2x2(1,2,3,4) => scaled nested Tensor
    let lhs = CompiledExpr::literal(Value::Real(2.0), Type::Real);
    let rhs = CompiledExpr::literal(matrix_2x2(1.0, 2.0, 3.0, 4.0), Type::matrix(2, 2, Type::Real));
    let expr = CompiledExpr::binop(BinOp::Mul, lhs, rhs, Type::tensor(2, 2, Type::Real));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let expected = Value::Tensor(vec![
        Value::Tensor(vec![Value::Real(2.0), Value::Real(4.0)]),
        Value::Tensor(vec![Value::Real(6.0), Value::Real(8.0)]),
    ]);
    assert_eq!(result, expected);
}

#[test]
fn neg_matrix() {
    // -matrix_2x2(1,2,3,4) => negated nested Tensor
    let operand = CompiledExpr::literal(matrix_2x2(1.0, 2.0, 3.0, 4.0), Type::matrix(2, 2, Type::Real));
    let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::tensor(2, 2, Type::Real));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let expected = Value::Tensor(vec![
        Value::Tensor(vec![Value::Real(-1.0), Value::Real(-2.0)]),
        Value::Tensor(vec![Value::Real(-3.0), Value::Real(-4.0)]),
    ]);
    assert_eq!(result, expected);
}
