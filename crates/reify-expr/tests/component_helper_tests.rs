//! Edge-case tests for component helper functions (componentwise_binop,
//! scale_components, negate_value). These exercise the refactored helpers
//! through the public eval_expr API.

use reify_expr::{eval_expr, EvalContext};
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
