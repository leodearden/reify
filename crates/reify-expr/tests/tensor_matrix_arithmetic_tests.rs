//! Matrix arithmetic negative tests and edge cases: Tensor*Tensor rejection,
//! dimension-mismatch propagation through rank-2 matrices, and mixed-dimension
//! dot product rejection.

use reify_expr::{EvalContext, eval_expr};
use reify_stdlib::eval_builtin;
use reify_core::Type;
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};

// ── Helpers ────────────────────────────────────────────────────────────────

/// Build a rank-2 Tensor (matrix) from nested `Vec<Vec<Value>>`.
///
/// The `Type::dimensionless_scalar()` annotation is a dummy — during evaluation, actual
/// type/dimension information is carried by `Value` variants
/// (`Value::Scalar{dimension}`, `Value::Int`, `Value::Real`), not the
/// `Type` field on `CompiledExpr`.
fn mat(rows: Vec<Vec<Value>>) -> Value {
    Value::Tensor(rows.into_iter().map(Value::Tensor).collect())
}

/// Build a rank-1 Tensor (vector) from `Vec<Value>`.
///
/// As with `mat()`, the `Type::dimensionless_scalar()` annotation on any wrapping
/// `CompiledExpr` is a dummy — actual dimensions live in the `Value`
/// variants themselves.
fn vec_lit(elems: Vec<Value>) -> Value {
    Value::Tensor(elems)
}

/// Wrap a `Value` into a `CompiledExpr::literal` with `Type::dimensionless_scalar()` as a
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

    let expr = CompiledExpr::binop(
        BinOp::Mul,
        lit(v, Type::dimensionless_scalar()),
        lit(m, Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    assert_eq!(eval(&expr), Value::Undef);
}

// ── Dimension-mismatch propagation through rank-2 matrices ─────────────────

/// Adding two 2x2 dimensioned matrices where one row has a dimension mismatch
/// should return Undef for the whole result. Row 0 adds cleanly (Length+Length),
/// but row 1 has dimension mismatch (Length+Angle → Undef per element).
/// `componentwise_binop` checks `results.iter().any(|v| v.is_undef())` and
/// propagates the inner row failure to the outer result.
#[test]
fn multi_row_undef_propagation() {
    // A = [[1m, 2m], [3m, 4m]]  — all Length
    let a = mat(vec![
        vec![Value::length(1.0), Value::length(2.0)],
        vec![Value::length(3.0), Value::length(4.0)],
    ]);

    // B = [[5m, 6m], [1rad, 2rad]]  — row 0 is Length, row 1 is Angle
    let b = mat(vec![
        vec![Value::length(5.0), Value::length(6.0)],
        vec![Value::angle(1.0), Value::angle(2.0)],
    ]);

    let expr = CompiledExpr::binop(
        BinOp::Add,
        lit(a, Type::dimensionless_scalar()),
        lit(b, Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    assert_eq!(eval(&expr), Value::Undef);
}

// ── Mixed-dimension dot product rejection ──────────────────────────────────

/// Simulates the dot product step of a matrix multiply where A has mixed
/// dimensions: A=[[1m, 1rad]] × B=[[1m],[1m]]. The dot product of A's row
/// [1m, 1rad] with B's column [1m, 1m] would produce 1m×1m + 1rad×1m =
/// Area + Angle·Length — but `tensor_components_f64` correctly rejects the
/// mixed-dimension input vector `a` before computation begins, because
/// `a[0].dimension() != a[1].dimension()` (LENGTH ≠ ANGLE).
#[test]
fn dot_dimension_mismatch_in_matrix_context() {
    // a = [1m, 1rad] — mixed dimensions (Length, Angle)
    let a = vec_lit(vec![Value::length(1.0), Value::angle(1.0)]);

    // b = [1m, 1m] — uniform dimension (Length)
    let b = vec_lit(vec![Value::length(1.0), Value::length(1.0)]);

    let result = eval_builtin("dot", &[a, b]);
    assert_eq!(result, Value::Undef);
}
