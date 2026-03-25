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

// ── step-13: Matrix*Vector canonicalization (DO NOW #9) ─────────────────────

#[test]
#[ignore] // eval_mul has no Tensor*Tensor path; matrix-vector dot product not yet implemented (esc-404-57)
fn matrix_literal_times_tensor_vector() {
    // Matrix(2x2) * Tensor(2) → dot products: [1*1+2*1, 3*1+4*1] = [3, 7]
    let lhs = CompiledExpr::literal(
        Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(2.0)],
            vec![Value::Real(3.0), Value::Real(4.0)],
        ]),
        Type::matrix(2, 2, Type::Real),
    );
    let rhs = CompiledExpr::literal(
        Value::Tensor(vec![Value::Real(1.0), Value::Real(1.0)]),
        Type::tensor(1, 2, Type::Real),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, lhs, rhs, Type::tensor(1, 2, Type::Real));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let expected = Value::Tensor(vec![Value::Real(3.0), Value::Real(7.0)]);
    assert_eq!(result, expected);
}

// ── step-14: List.sum() with Matrix elements (DO NOW #11) ──────────────────

#[test]
fn list_sum_matrix_elements() {
    // List([matrix_2x2(1,0,0,1), matrix_2x2(2,3,4,5)]).sum() => Tensor sum after canonicalization
    let list = Value::List(vec![matrix_2x2(1.0, 0.0, 0.0, 1.0), matrix_2x2(2.0, 3.0, 4.0, 5.0)]);
    let obj_expr = CompiledExpr::literal(list, Type::List(Box::new(Type::matrix(2, 2, Type::Real))));
    let expr = CompiledExpr::method_call(
        obj_expr,
        "sum".to_string(),
        vec![],
        Type::tensor(2, 2, Type::Real),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let expected = Value::Tensor(vec![
        Value::Tensor(vec![Value::Real(3.0), Value::Real(3.0)]),
        Value::Tensor(vec![Value::Real(4.0), Value::Real(6.0)]),
    ]);
    assert_eq!(result, expected);
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
