//! Complex<Q> arithmetic and method evaluation tests.

use reify_expr::{eval_expr, EvalContext};
use reify_types::{BinOp, CompiledExpr, DimensionVector, Type, UnOp, Value, ValueMap};

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Build a literal expression from a value and type.
fn lit(v: Value, ty: Type) -> CompiledExpr {
    CompiledExpr::literal(v, ty)
}

/// Build a Complex value.
fn complex_val(re: f64, im: f64, dim: DimensionVector) -> Value {
    Value::Complex {
        re,
        im,
        dimension: dim,
    }
}

/// Build a Scalar value.
fn scalar_val(v: f64, dim: DimensionVector) -> Value {
    Value::Scalar {
        si_value: v,
        dimension: dim,
    }
}

/// Evaluate a literal expression and return the result.
fn eval_lit(v: Value, ty: Type) -> Value {
    let values = ValueMap::new();
    eval_expr(&lit(v, ty), &EvalContext::simple(&values))
}

/// Build and evaluate a binop expression.
fn eval_binop(op: BinOp, lv: Value, lt: Type, rv: Value, rt: Type, result_ty: Type) -> Value {
    let expr = CompiledExpr::binop(op, lit(lv, lt), lit(rv, rt), result_ty);
    let values = ValueMap::new();
    eval_expr(&expr, &EvalContext::simple(&values))
}

/// Build and evaluate a unary expression.
fn eval_unop(op: UnOp, v: Value, vt: Type, result_ty: Type) -> Value {
    let expr = CompiledExpr::unop(op, lit(v, vt), result_ty);
    let values = ValueMap::new();
    eval_expr(&expr, &EvalContext::simple(&values))
}

/// Build and evaluate a zero-arg method call.
fn eval_method(obj: Value, obj_ty: Type, method: &str, result_ty: Type) -> Value {
    let expr = CompiledExpr::method_call(
        lit(obj, obj_ty),
        method.to_string(),
        vec![],
        result_ty,
    );
    let values = ValueMap::new();
    eval_expr(&expr, &EvalContext::simple(&values))
}
