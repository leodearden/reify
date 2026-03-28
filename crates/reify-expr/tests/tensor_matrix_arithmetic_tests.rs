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
