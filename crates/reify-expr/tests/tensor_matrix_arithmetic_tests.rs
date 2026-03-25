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
