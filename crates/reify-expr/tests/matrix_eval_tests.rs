//! Matrix arithmetic evaluation tests: rank-2 detection, empty-tensor guards, jagged validation.

use reify_expr::{EvalContext, eval_expr};
use reify_types::{BinOp, CompiledExpr, Type, Value, ValueMap};

/// Helper to build a literal expression.
fn lit(v: Value, ty: Type) -> CompiledExpr {
    CompiledExpr::literal(v, ty)
}

/// Simple tensor type for test expressions.
fn tensor_ty() -> Type {
    Type::Tensor {
        rank: 1,
        n: 0,
        quantity: Box::new(Type::Int),
    }
}

/// Helper to build an add expression.
fn add(a: CompiledExpr, b: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Add, a, b, tensor_ty())
}

/// Helper to build a sub expression.
fn sub(a: CompiledExpr, b: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Sub, a, b, tensor_ty())
}

/// Helper to evaluate an expression with an empty ValueMap.
fn eval(expr: &CompiledExpr) -> Value {
    let values = ValueMap::new();
    eval_expr(expr, &EvalContext::simple(&values))
}

// ── Rank-1 empty tensor baseline ────────────────────────────────────────────

/// Rank-1 empty tensors: Tensor([]) + Tensor([]) → Tensor([]).
/// This baseline test confirms rank-1 empty tensors are NOT affected by
/// rank-2 guards added later.
#[test]
fn empty_rank1_tensor_add_returns_empty_tensor() {
    let a = lit(Value::Tensor(vec![]), tensor_ty());
    let b = lit(Value::Tensor(vec![]), tensor_ty());
    let expr = add(a, b);
    assert_eq!(eval(&expr), Value::Tensor(vec![]));
}
