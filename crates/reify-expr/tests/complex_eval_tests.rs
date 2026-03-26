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

// ─── step-1: Complex + Complex ─────────────────────────────────────────────

/// Adding two Complex values with the same dimension sums re and im components.
#[test]
fn complex_add_same_dimension() {
    let result = eval_binop(
        BinOp::Add,
        complex_val(1.0, 2.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        complex_val(3.0, 4.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(4.0, 6.0, DimensionVector::LENGTH));
}

/// Adding two Complex values with mismatched dimensions returns Undef.
#[test]
fn complex_add_dimension_mismatch() {
    let result = eval_binop(
        BinOp::Add,
        complex_val(1.0, 2.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        complex_val(3.0, 4.0, DimensionVector::TIME),
        Type::complex(Type::Real),
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}

// ─── step-3: Complex - Complex ─────────────────────────────────────────────

/// Subtracting two Complex values with the same dimension differences re and im.
#[test]
fn complex_sub_same_dimension() {
    let result = eval_binop(
        BinOp::Sub,
        complex_val(5.0, 7.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        complex_val(2.0, 3.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(3.0, 4.0, DimensionVector::LENGTH));
}

/// Subtracting Complex values with mismatched dimensions returns Undef.
#[test]
fn complex_sub_dimension_mismatch() {
    let result = eval_binop(
        BinOp::Sub,
        complex_val(5.0, 7.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        complex_val(2.0, 3.0, DimensionVector::TIME),
        Type::complex(Type::Real),
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}
