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

// ── Option-collect pattern behavior tests ──────────────────────────────────

/// componentwise_binop with first element producing Undef should return Undef.
/// Adding a Length scalar to a dimensionless Real causes dimension mismatch
/// on the first element, so the whole result is Undef.
/// Behavior preserved before and after Option-collect refactor.
#[test]
fn componentwise_binop_first_element_undef_returns_undef() {
    // Tensor([Length(1.0), Int(2)]) + Tensor([Real(1.0), Int(3)])
    // First pair: Length(1.0) + Real(1.0) → Undef (dimension mismatch)
    let a = lit(
        Value::Tensor(vec![Value::length(1.0), Value::Int(2)]),
        Type::Real,
    );
    let b = lit(
        Value::Tensor(vec![Value::Real(1.0), Value::Int(3)]),
        Type::Real,
    );
    let expr = CompiledExpr::binop(BinOp::Add, a, b, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

/// scale_components with first component operation producing Undef.
/// Multiplying a Tensor containing a Bool by a scalar produces Undef on first op.
/// Behavior preserved before and after Option-collect refactor.
#[test]
fn scale_components_first_element_undef_returns_undef() {
    // Tensor([Bool(true), Int(2)]) * Int(3)
    // First: Bool(true) * Int(3) → Undef (type mismatch)
    let t = lit(
        Value::Tensor(vec![Value::Bool(true), Value::Int(2)]),
        Type::Real,
    );
    let s = lit(Value::Int(3), Type::Real);
    let expr = CompiledExpr::binop(BinOp::Mul, t, s, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── neg_scalar regression tests ────────────────────────────────────────────

/// Negate an Int value.
#[test]
fn negate_int() {
    let expr = CompiledExpr::unop(UnOp::Neg, lit(Value::Int(42), Type::Real), Type::Real);
    assert_eq!(eval(&expr), Value::Int(-42));
}

/// Negate a Real value.
#[test]
fn negate_real() {
    let expr = CompiledExpr::unop(UnOp::Neg, lit(Value::Real(3.14), Type::Real), Type::Real);
    assert_eq!(eval(&expr), Value::Real(-3.14));
}

/// Negate a Scalar with dimension (Length).
#[test]
fn negate_scalar_with_dimension() {
    let expr = CompiledExpr::unop(
        UnOp::Neg,
        lit(Value::length(2.5), Type::length()),
        Type::length(),
    );
    assert_eq!(eval(&expr), Value::length(-2.5));
}

/// Negate a Complex with dimension (Length).
#[test]
fn negate_complex_with_dimension() {
    let c = Value::Complex {
        re: 3.0,
        im: 4.0,
        dimension: DimensionVector::LENGTH,
    };
    let expr = CompiledExpr::unop(
        UnOp::Neg,
        lit(c, Type::complex(Type::length())),
        Type::complex(Type::length()),
    );
    assert_eq!(
        eval(&expr),
        Value::Complex {
            re: -3.0,
            im: -4.0,
            dimension: DimensionVector::LENGTH,
        }
    );
}

/// Negating Int::MIN (i64::MIN) overflows checked_neg → Undef.
#[test]
fn negate_int_min_returns_undef() {
    let expr = CompiledExpr::unop(
        UnOp::Neg,
        lit(Value::Int(i64::MIN), Type::Real),
        Type::Real,
    );
    assert_eq!(eval(&expr), Value::Undef);
}
