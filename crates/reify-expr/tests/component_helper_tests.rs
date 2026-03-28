//! Edge-case tests for component helper functions (componentwise_binop,
//! scale_components, negate_value). These exercise the refactored helpers
//! through the public eval_expr API.

use reify_expr::{eval_expr, EvalContext};
#[allow(unused_imports)]
use reify_types::{BinOp, CompiledExpr, DimensionVector, Type, UnOp, Value, ValueMap};

// ── Helpers ────────────────────────────────────────────────────────────────

/// Wrap a `Value` into a `CompiledExpr::literal` with the given type.
fn lit(v: Value, ty: Type) -> CompiledExpr {
    CompiledExpr::literal(v, ty)
}

/// Evaluate an expression with an empty `ValueMap`.
fn eval(expr: &CompiledExpr) -> Value {
    let values = ValueMap::new();
    eval_expr(expr, &EvalContext::simple(&values))
}

// ── Empty-components guard tests ───────────────────────────────────────────

/// Adding two empty Tensors should return Undef (empty components are malformed).
/// Exercises componentwise_binop's empty-components guard.
#[test]
fn add_empty_tensors_returns_undef() {
    let a = lit(Value::Tensor(vec![]), Type::Real);
    let b = lit(Value::Tensor(vec![]), Type::Real);
    let expr = CompiledExpr::binop(BinOp::Add, a, b, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

/// Scaling an empty Vector by a scalar should return Undef.
/// Exercises scale_components' empty-components guard.
#[test]
fn scale_empty_vector_returns_undef() {
    let v = lit(Value::Vector(vec![]), Type::Real);
    let s = lit(Value::Int(2), Type::Real);
    let expr = CompiledExpr::binop(BinOp::Mul, v, s, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── Early Undef-scalar guard tests ─────────────────────────────────────────

/// Multiplying a valid Vector by Undef scalar should return Undef.
/// After refactoring, scale_components checks scalar.is_undef() up front
/// instead of iterating all components to discover Undef.
#[test]
fn scale_vector_by_undef_scalar_returns_undef() {
    let v = lit(
        Value::Vector(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        Type::Real,
    );
    let s = lit(Value::Undef, Type::Real);
    let expr = CompiledExpr::binop(BinOp::Mul, v, s, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}
