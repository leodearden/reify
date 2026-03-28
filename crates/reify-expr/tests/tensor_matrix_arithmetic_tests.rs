//! Matrix arithmetic negative tests and edge cases: Tensor*Tensor rejection,
//! dimension-mismatch propagation through rank-2 matrices, and mixed-dimension
//! dot product rejection.

use reify_expr::{eval_expr, EvalContext};
use reify_stdlib::eval_builtin;
use reify_types::{BinOp, CompiledExpr, Type, Value, ValueMap};

// ── Helpers ────────────────────────────────────────────────────────────────

/// Build a rank-2 Tensor (matrix) from nested `Vec<Vec<Value>>`.
///
/// The `Type::Real` annotation is a dummy — during evaluation, actual
/// type/dimension information is carried by `Value` variants
/// (`Value::Scalar{dimension}`, `Value::Int`, `Value::Real`), not the
/// `Type` field on `CompiledExpr`.
fn mat(rows: Vec<Vec<Value>>) -> Value {
    Value::Tensor(
        rows.into_iter()
            .map(|r| Value::Tensor(r))
            .collect(),
    )
}

/// Build a rank-1 Tensor (vector) from `Vec<Value>`.
///
/// As with `mat()`, the `Type::Real` annotation on any wrapping
/// `CompiledExpr` is a dummy — actual dimensions live in the `Value`
/// variants themselves.
fn vec_lit(elems: Vec<Value>) -> Value {
    Value::Tensor(elems)
}

/// Wrap a `Value` into a `CompiledExpr::literal` with `Type::Real` as a
/// dummy type annotation. The evaluator does not consult the `Type` field
/// at runtime — it dispatches on the `Value` variant instead.
fn lit(v: Value, ty: Type) -> CompiledExpr {
    CompiledExpr::literal(v, ty)
}

/// Evaluate an expression with an empty `ValueMap`.
fn eval(expr: &CompiledExpr) -> Value {
    let values = ValueMap::new();
    eval_expr(expr, &EvalContext::simple(&values))
}

// ── Tensor*Tensor multiplication rejection ─────────────────────────────────

/// Multiplying a rank-1 Tensor (vector) by a rank-2 Tensor (matrix) should
/// return Undef. The evaluator's `eval_mul` only handles Scalar*Tensor (and
/// Tensor*Scalar); all other combinations — including Tensor*Tensor — fall
/// through to the catch-all `_ => Value::Undef` arm.
#[test]
fn vector_times_matrix_returns_undef() {
    // v = [1, 2, 3]  (rank-1 tensor)
    let v = vec_lit(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);

    // M = identity 3x3 matrix (rank-2 tensor)
    let m = mat(vec![
        vec![Value::Int(1), Value::Int(0), Value::Int(0)],
        vec![Value::Int(0), Value::Int(1), Value::Int(0)],
        vec![Value::Int(0), Value::Int(0), Value::Int(1)],
    ]);

    let expr = CompiledExpr::binop(BinOp::Mul, lit(v, Type::Real), lit(m, Type::Real), Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}
