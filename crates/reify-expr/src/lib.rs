use reify_types::{BinOp, CompiledExpr, CompiledExprKind, UnOp, Value, ValueCellId, ValueMap};

/// Evaluate a compiled expression against a set of values.
///
/// Pure recursive evaluator implementing:
/// - Undef propagation (strict for arithmetic, Kleene for logic)
/// - Dimensional arithmetic (add/sub require same dimension, mul/div combine dimensions)
/// - Division by zero → Undef
pub fn eval_expr(expr: &CompiledExpr, values: &ValueMap) -> Value {
    match &expr.kind {
        CompiledExprKind::Literal(v) => v.clone(),

        CompiledExprKind::ValueRef(id) => values.get_or_undef(id),

        CompiledExprKind::BinOp { op, left, right } => {
            eval_binop(*op, left, right, values)
        }

        CompiledExprKind::UnOp { op, operand } => {
            eval_unop(*op, operand, values)
        }

        CompiledExprKind::FunctionCall { function, args } => {
            // M1: no stdlib functions implemented yet
            let _ = function;
            let _ = args;
            Value::Undef
        }

        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            let cond = eval_expr(condition, values);
            match cond {
                Value::Bool(true) => eval_expr(then_branch, values),
                Value::Bool(false) => eval_expr(else_branch, values),
                Value::Undef => Value::Undef,
                _ => Value::Undef, // type error: condition is not bool
            }
        }
    }
}

/// Evaluate a compiled expression with dependency tracing.
///
/// Identical to `eval_expr` but calls `on_read(id)` for every ValueRef read,
/// enabling the caller to record which value cells contribute to the result.
/// Only actually-evaluated branches are traced (e.g., in conditionals, only
/// the taken branch is traced).
pub fn eval_expr_traced(
    expr: &CompiledExpr,
    values: &ValueMap,
    on_read: &mut dyn FnMut(&ValueCellId),
) -> Value {
    match &expr.kind {
        CompiledExprKind::Literal(v) => v.clone(),

        CompiledExprKind::ValueRef(id) => {
            on_read(id);
            values.get_or_undef(id)
        }

        CompiledExprKind::BinOp { op, left, right } => {
            eval_binop_traced(*op, left, right, values, on_read)
        }

        CompiledExprKind::UnOp { op, operand } => {
            eval_unop_traced(*op, operand, values, on_read)
        }

        CompiledExprKind::FunctionCall { function, args } => {
            let _ = function;
            let _ = args;
            Value::Undef
        }

        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            let cond = eval_expr_traced(condition, values, on_read);
            match cond {
                Value::Bool(true) => eval_expr_traced(then_branch, values, on_read),
                Value::Bool(false) => eval_expr_traced(else_branch, values, on_read),
                Value::Undef => Value::Undef,
                _ => Value::Undef,
            }
        }
    }
}

fn eval_binop_traced(
    op: BinOp,
    left: &CompiledExpr,
    right: &CompiledExpr,
    values: &ValueMap,
    on_read: &mut dyn FnMut(&ValueCellId),
) -> Value {
    match op {
        BinOp::And => return eval_and_traced(left, right, values, on_read),
        BinOp::Or => return eval_or_traced(left, right, values, on_read),
        _ => {}
    }

    let lv = eval_expr_traced(left, values, on_read);
    let rv = eval_expr_traced(right, values, on_read);

    if lv.is_undef() || rv.is_undef() {
        return Value::Undef;
    }

    match op {
        BinOp::Add => eval_add(&lv, &rv),
        BinOp::Sub => eval_sub(&lv, &rv),
        BinOp::Mul => eval_mul(&lv, &rv),
        BinOp::Div => eval_div(&lv, &rv),
        BinOp::Mod => eval_mod(&lv, &rv),
        BinOp::Pow => eval_pow(&lv, &rv),
        BinOp::Eq => eval_eq(&lv, &rv),
        BinOp::Ne => eval_ne(&lv, &rv),
        BinOp::Lt => eval_cmp(&lv, &rv, |a, b| a < b),
        BinOp::Le => eval_cmp(&lv, &rv, |a, b| a <= b),
        BinOp::Gt => eval_cmp(&lv, &rv, |a, b| a > b),
        BinOp::Ge => eval_cmp(&lv, &rv, |a, b| a >= b),
        BinOp::And | BinOp::Or => unreachable!(),
    }
}

fn eval_and_traced(
    left: &CompiledExpr,
    right: &CompiledExpr,
    values: &ValueMap,
    on_read: &mut dyn FnMut(&ValueCellId),
) -> Value {
    let lv = eval_expr_traced(left, values, on_read);
    match lv {
        Value::Bool(false) => Value::Bool(false),
        Value::Bool(true) => {
            let rv = eval_expr_traced(right, values, on_read);
            match rv {
                Value::Bool(b) => Value::Bool(b),
                Value::Undef => Value::Undef,
                _ => Value::Undef,
            }
        }
        Value::Undef => {
            let rv = eval_expr_traced(right, values, on_read);
            match rv {
                Value::Bool(false) => Value::Bool(false),
                _ => Value::Undef,
            }
        }
        _ => Value::Undef,
    }
}

fn eval_or_traced(
    left: &CompiledExpr,
    right: &CompiledExpr,
    values: &ValueMap,
    on_read: &mut dyn FnMut(&ValueCellId),
) -> Value {
    let lv = eval_expr_traced(left, values, on_read);
    match lv {
        Value::Bool(true) => Value::Bool(true),
        Value::Bool(false) => {
            let rv = eval_expr_traced(right, values, on_read);
            match rv {
                Value::Bool(b) => Value::Bool(b),
                Value::Undef => Value::Undef,
                _ => Value::Undef,
            }
        }
        Value::Undef => {
            let rv = eval_expr_traced(right, values, on_read);
            match rv {
                Value::Bool(true) => Value::Bool(true),
                _ => Value::Undef,
            }
        }
        _ => Value::Undef,
    }
}

fn eval_unop_traced(
    op: UnOp,
    operand: &CompiledExpr,
    values: &ValueMap,
    on_read: &mut dyn FnMut(&ValueCellId),
) -> Value {
    let v = eval_expr_traced(operand, values, on_read);
    if v.is_undef() {
        return Value::Undef;
    }
    match op {
        UnOp::Neg => match v {
            Value::Int(i) => Value::Int(-i),
            Value::Real(r) => Value::Real(-r),
            Value::Scalar { si_value, dimension } => Value::Scalar {
                si_value: -si_value,
                dimension,
            },
            _ => Value::Undef,
        },
        UnOp::Not => match v {
            Value::Bool(b) => Value::Bool(!b),
            _ => Value::Undef,
        },
    }
}

fn eval_binop(op: BinOp, left: &CompiledExpr, right: &CompiledExpr, values: &ValueMap) -> Value {
    // Kleene three-valued logic: short-circuit with Undef support
    match op {
        BinOp::And => return eval_and(left, right, values),
        BinOp::Or => return eval_or(left, right, values),
        _ => {}
    }

    let lv = eval_expr(left, values);
    let rv = eval_expr(right, values);

    // Strict undef propagation for arithmetic/comparison
    if lv.is_undef() || rv.is_undef() {
        return Value::Undef;
    }

    match op {
        BinOp::Add => eval_add(&lv, &rv),
        BinOp::Sub => eval_sub(&lv, &rv),
        BinOp::Mul => eval_mul(&lv, &rv),
        BinOp::Div => eval_div(&lv, &rv),
        BinOp::Mod => eval_mod(&lv, &rv),
        BinOp::Pow => eval_pow(&lv, &rv),
        BinOp::Eq => eval_eq(&lv, &rv),
        BinOp::Ne => eval_ne(&lv, &rv),
        BinOp::Lt => eval_cmp(&lv, &rv, |a, b| a < b),
        BinOp::Le => eval_cmp(&lv, &rv, |a, b| a <= b),
        BinOp::Gt => eval_cmp(&lv, &rv, |a, b| a > b),
        BinOp::Ge => eval_cmp(&lv, &rv, |a, b| a >= b),
        BinOp::And | BinOp::Or => unreachable!(),
    }
}

/// Kleene AND: false ∧ Undef = false
fn eval_and(left: &CompiledExpr, right: &CompiledExpr, values: &ValueMap) -> Value {
    let lv = eval_expr(left, values);
    match lv {
        Value::Bool(false) => Value::Bool(false),
        Value::Bool(true) => {
            let rv = eval_expr(right, values);
            match rv {
                Value::Bool(b) => Value::Bool(b),
                Value::Undef => Value::Undef,
                _ => Value::Undef,
            }
        }
        Value::Undef => {
            let rv = eval_expr(right, values);
            match rv {
                Value::Bool(false) => Value::Bool(false),
                _ => Value::Undef,
            }
        }
        _ => Value::Undef,
    }
}

/// Kleene OR: true ∨ Undef = true
fn eval_or(left: &CompiledExpr, right: &CompiledExpr, values: &ValueMap) -> Value {
    let lv = eval_expr(left, values);
    match lv {
        Value::Bool(true) => Value::Bool(true),
        Value::Bool(false) => {
            let rv = eval_expr(right, values);
            match rv {
                Value::Bool(b) => Value::Bool(b),
                Value::Undef => Value::Undef,
                _ => Value::Undef,
            }
        }
        Value::Undef => {
            let rv = eval_expr(right, values);
            match rv {
                Value::Bool(true) => Value::Bool(true),
                _ => Value::Undef,
            }
        }
        _ => Value::Undef,
    }
}

fn eval_add(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a + b),
        (Value::Real(a), Value::Real(b)) => Value::Real(a + b),
        (Value::Int(a), Value::Real(b)) | (Value::Real(b), Value::Int(a)) => {
            Value::Real(*a as f64 + b)
        }
        (
            Value::Scalar {
                si_value: a,
                dimension: ad,
            },
            Value::Scalar {
                si_value: b,
                dimension: bd,
            },
        ) => {
            if ad != bd {
                Value::Undef // dimension mismatch
            } else {
                Value::Scalar {
                    si_value: a + b,
                    dimension: *ad,
                }
            }
        }
        (Value::String(a), Value::String(b)) => Value::String(format!("{}{}", a, b)),
        _ => Value::Undef,
    }
}

fn eval_sub(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a - b),
        (Value::Real(a), Value::Real(b)) => Value::Real(a - b),
        (Value::Int(a), Value::Real(b)) => Value::Real(*a as f64 - b),
        (Value::Real(a), Value::Int(b)) => Value::Real(a - *b as f64),
        (
            Value::Scalar {
                si_value: a,
                dimension: ad,
            },
            Value::Scalar {
                si_value: b,
                dimension: bd,
            },
        ) => {
            if ad != bd {
                Value::Undef // dimension mismatch
            } else {
                Value::Scalar {
                    si_value: a - b,
                    dimension: *ad,
                }
            }
        }
        _ => Value::Undef,
    }
}

fn eval_mul(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a * b),
        (Value::Real(a), Value::Real(b)) => Value::Real(a * b),
        (Value::Int(a), Value::Real(b)) | (Value::Real(b), Value::Int(a)) => {
            Value::Real(*a as f64 * b)
        }
        // Scalar * Scalar: multiply values, add dimension exponents
        (
            Value::Scalar {
                si_value: a,
                dimension: ad,
            },
            Value::Scalar {
                si_value: b,
                dimension: bd,
            },
        ) => Value::Scalar {
            si_value: a * b,
            dimension: ad.mul(bd),
        },
        // Scalar * dimensionless numeric
        (Value::Scalar { si_value, dimension }, Value::Int(n))
        | (Value::Int(n), Value::Scalar { si_value, dimension }) => Value::Scalar {
            si_value: si_value * *n as f64,
            dimension: *dimension,
        },
        (Value::Scalar { si_value, dimension }, Value::Real(r))
        | (Value::Real(r), Value::Scalar { si_value, dimension }) => Value::Scalar {
            si_value: si_value * r,
            dimension: *dimension,
        },
        _ => Value::Undef,
    }
}

fn eval_div(lv: &Value, rv: &Value) -> Value {
    // Check for division by zero
    if let Some(denom) = rv.as_f64()
        && denom == 0.0
    {
        return Value::Undef;
    }

    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                Value::Undef
            } else if a % b == 0 {
                Value::Int(a / b)
            } else {
                Value::Real(*a as f64 / *b as f64)
            }
        }
        (Value::Real(a), Value::Real(b)) => Value::Real(a / b),
        (Value::Int(a), Value::Real(b)) => Value::Real(*a as f64 / b),
        (Value::Real(a), Value::Int(b)) => Value::Real(a / *b as f64),
        // Scalar / Scalar: divide values, subtract dimension exponents
        (
            Value::Scalar {
                si_value: a,
                dimension: ad,
            },
            Value::Scalar {
                si_value: b,
                dimension: bd,
            },
        ) => {
            let result_dim = ad.div(bd);
            if result_dim.is_dimensionless() {
                Value::Real(a / b)
            } else {
                Value::Scalar {
                    si_value: a / b,
                    dimension: result_dim,
                }
            }
        }
        // Scalar / dimensionless
        (Value::Scalar { si_value, dimension }, Value::Int(n)) => Value::Scalar {
            si_value: si_value / *n as f64,
            dimension: *dimension,
        },
        (Value::Scalar { si_value, dimension }, Value::Real(r)) => Value::Scalar {
            si_value: si_value / r,
            dimension: *dimension,
        },
        _ => Value::Undef,
    }
}

fn eval_mod(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                Value::Undef
            } else {
                Value::Int(a % b)
            }
        }
        (Value::Real(a), Value::Real(b)) => {
            if *b == 0.0 {
                Value::Undef
            } else {
                Value::Real(a % b)
            }
        }
        _ => Value::Undef,
    }
}

fn eval_pow(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Int(base), Value::Int(exp)) => {
            if *exp >= 0 && *exp <= i32::MAX as i64 {
                Value::Int(base.pow(*exp as u32))
            } else {
                Value::Real((*base as f64).powi(*exp as i32))
            }
        }
        (Value::Real(base), Value::Int(exp)) => Value::Real(base.powi(*exp as i32)),
        (Value::Real(base), Value::Real(exp)) => Value::Real(base.powf(*exp)),
        (Value::Int(base), Value::Real(exp)) => Value::Real((*base as f64).powf(*exp)),
        // Scalar ^ Int: raise value, multiply dimension exponents
        (Value::Scalar { si_value, dimension }, Value::Int(n)) => Value::Scalar {
            si_value: si_value.powi(*n as i32),
            dimension: dimension.pow(*n as i8),
        },
        _ => Value::Undef,
    }
}

fn eval_eq(lv: &Value, rv: &Value) -> Value {
    match (lv, rv) {
        (Value::Bool(a), Value::Bool(b)) => Value::Bool(a == b),
        (Value::Int(a), Value::Int(b)) => Value::Bool(a == b),
        (Value::String(a), Value::String(b)) => Value::Bool(a == b),
        _ => {
            // For numeric comparisons, compare as f64
            match (lv.as_f64(), rv.as_f64()) {
                (Some(a), Some(b)) => Value::Bool(a == b),
                _ => Value::Undef,
            }
        }
    }
}

fn eval_ne(lv: &Value, rv: &Value) -> Value {
    match eval_eq(lv, rv) {
        Value::Bool(b) => Value::Bool(!b),
        other => other,
    }
}

fn eval_cmp(lv: &Value, rv: &Value, cmp: fn(f64, f64) -> bool) -> Value {
    match (lv.as_f64(), rv.as_f64()) {
        (Some(a), Some(b)) => Value::Bool(cmp(a, b)),
        _ => Value::Undef,
    }
}

fn eval_unop(op: UnOp, operand: &CompiledExpr, values: &ValueMap) -> Value {
    let v = eval_expr(operand, values);
    if v.is_undef() {
        return Value::Undef;
    }
    match op {
        UnOp::Neg => match v {
            Value::Int(i) => Value::Int(-i),
            Value::Real(r) => Value::Real(-r),
            Value::Scalar { si_value, dimension } => Value::Scalar {
                si_value: -si_value,
                dimension,
            },
            _ => Value::Undef,
        },
        UnOp::Not => match v {
            Value::Bool(b) => Value::Bool(!b),
            _ => Value::Undef,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{DimensionVector, Type, ValueCellId};

    // Helper to build a literal expression
    fn lit(v: Value, ty: Type) -> CompiledExpr {
        CompiledExpr::literal(v, ty)
    }

    fn vref(entity: &str, member: &str, ty: Type) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new(entity, member), ty)
    }

    fn mm_val(v: f64) -> Value {
        Value::Scalar {
            si_value: v * 0.001,
            dimension: DimensionVector::LENGTH,
        }
    }

    #[test]
    fn literal_evaluation() {
        let expr = lit(Value::Int(42), Type::Int);
        let values = ValueMap::new();
        match eval_expr(&expr, &values) {
            Value::Int(42) => {}
            other => panic!("expected Int(42), got {:?}", other),
        }
    }

    #[test]
    fn value_ref_found() {
        let expr = vref("Bracket", "width", Type::length());
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("Bracket", "width"), mm_val(80.0));
        let result = eval_expr(&expr, &values);
        assert!(!result.is_undef());
        let v = result.as_f64().unwrap();
        assert!((v - 0.08).abs() < 1e-12);
    }

    #[test]
    fn value_ref_missing_returns_undef() {
        let expr = vref("Bracket", "width", Type::length());
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &values).is_undef());
    }

    #[test]
    fn add_same_dimension() {
        let left = lit(mm_val(80.0), Type::length());
        let right = lit(mm_val(20.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::length());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &values);
        let v = result.as_f64().unwrap();
        assert!((v - 0.1).abs() < 1e-12);
    }

    #[test]
    fn add_different_dimensions_is_undef() {
        let length = lit(mm_val(80.0), Type::length());
        let mass = lit(
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Type::Scalar {
                dimension: DimensionVector::MASS,
            },
        );
        let expr = CompiledExpr::binop(BinOp::Add, length, mass, Type::length());
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &values).is_undef());
    }

    #[test]
    fn mul_dimensions_add_exponents() {
        let width = lit(mm_val(80.0), Type::length());
        let height = lit(mm_val(100.0), Type::length());
        let expr = CompiledExpr::binop(
            BinOp::Mul,
            width,
            height,
            Type::Scalar {
                dimension: DimensionVector::AREA,
            },
        );
        let values = ValueMap::new();
        let result = eval_expr(&expr, &values);
        match &result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 0.008).abs() < 1e-12);
                assert_eq!(*dimension, DimensionVector::AREA);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn div_by_zero_is_undef() {
        let left = lit(Value::Int(42), Type::Int);
        let right = lit(Value::Int(0), Type::Int);
        let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::Int);
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &values).is_undef());
    }

    #[test]
    fn gt_comparison() {
        let left = lit(mm_val(5.0), Type::length());
        let right = lit(mm_val(2.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Gt, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &values) {
            Value::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }

    #[test]
    fn undef_propagation_arithmetic() {
        let left = lit(Value::Undef, Type::length());
        let right = lit(mm_val(2.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::length());
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &values).is_undef());
    }

    #[test]
    fn kleene_and_false_undef() {
        // false AND Undef = false
        let left = lit(Value::Bool(false), Type::Bool);
        let right = lit(Value::Undef, Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &values) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn kleene_and_undef_false() {
        // Undef AND false = false
        let left = lit(Value::Undef, Type::Bool);
        let right = lit(Value::Bool(false), Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &values) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn kleene_or_true_undef() {
        // true OR Undef = true
        let left = lit(Value::Bool(true), Type::Bool);
        let right = lit(Value::Undef, Type::Bool);
        let expr = CompiledExpr::binop(BinOp::Or, left, right, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &values) {
            Value::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }

    #[test]
    fn kleene_and_undef_undef() {
        // Undef AND true = Undef
        let left = lit(Value::Undef, Type::Bool);
        let right = lit(Value::Bool(true), Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &values).is_undef());
    }

    #[test]
    fn negation() {
        let operand = lit(mm_val(5.0), Type::length());
        let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::length());
        let values = ValueMap::new();
        let result = eval_expr(&expr, &values);
        let v = result.as_f64().unwrap();
        assert!((v - (-0.005)).abs() < 1e-12);
    }

    #[test]
    fn not_bool() {
        let operand = lit(Value::Bool(true), Type::Bool);
        let expr = CompiledExpr::unop(UnOp::Not, operand, Type::Bool);
        let values = ValueMap::new();
        match eval_expr(&expr, &values) {
            Value::Bool(false) => {}
            other => panic!("expected Bool(false), got {:?}", other),
        }
    }

    #[test]
    fn conditional_true() {
        let cond = lit(Value::Bool(true), Type::Bool);
        let then_branch = lit(Value::Int(1), Type::Int);
        let else_branch = lit(Value::Int(2), Type::Int);
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[99]),
            result_type: Type::Int,
            kind: CompiledExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
        };
        let values = ValueMap::new();
        match eval_expr(&expr, &values) {
            Value::Int(1) => {}
            other => panic!("expected Int(1), got {:?}", other),
        }
    }

    #[test]
    fn conditional_undef_condition() {
        let cond = lit(Value::Undef, Type::Bool);
        let then_branch = lit(Value::Int(1), Type::Int);
        let else_branch = lit(Value::Int(2), Type::Int);
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[99]),
            result_type: Type::Int,
            kind: CompiledExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
        };
        let values = ValueMap::new();
        assert!(eval_expr(&expr, &values).is_undef());
    }

    #[test]
    fn scalar_pow_int() {
        // (3mm)^2 = 9mm² = 9e-6 m²
        let base = lit(mm_val(3.0), Type::length());
        let exp = lit(Value::Int(2), Type::Int);
        let expr = CompiledExpr::binop(
            BinOp::Pow,
            base,
            exp,
            Type::Scalar {
                dimension: DimensionVector::AREA,
            },
        );
        let values = ValueMap::new();
        let result = eval_expr(&expr, &values);
        match &result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 9e-6).abs() < 1e-15);
                assert_eq!(*dimension, DimensionVector::AREA);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn volume_computation() {
        // width * height * thickness
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "width"), mm_val(80.0));
        values.insert(ValueCellId::new("B", "height"), mm_val(100.0));
        values.insert(ValueCellId::new("B", "thickness"), mm_val(5.0));

        let w = vref("B", "width", Type::length());
        let h = vref("B", "height", Type::length());
        let t = vref("B", "thickness", Type::length());

        let wh = CompiledExpr::binop(
            BinOp::Mul,
            w,
            h,
            Type::Scalar {
                dimension: DimensionVector::AREA,
            },
        );
        let volume = CompiledExpr::binop(
            BinOp::Mul,
            wh,
            t,
            Type::Scalar {
                dimension: DimensionVector::VOLUME,
            },
        );

        let result = eval_expr(&volume, &values);
        match &result {
            Value::Scalar { si_value, dimension } => {
                // 0.08 * 0.1 * 0.005 = 4e-5 m³
                assert!((si_value - 4e-5).abs() < 1e-15);
                assert_eq!(*dimension, DimensionVector::VOLUME);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn div_same_dimension_yields_dimensionless() {
        // 80mm / 20mm = 4.0 (dimensionless Real)
        let left = lit(mm_val(80.0), Type::length());
        let right = lit(mm_val(20.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::Real);
        let values = ValueMap::new();
        let result = eval_expr(&expr, &values);
        match &result {
            Value::Real(v) => assert!((v - 4.0).abs() < 1e-12),
            other => panic!("expected Real, got {:?}", other),
        }
    }

    // --- eval_expr_traced tests ---

    #[test]
    fn traced_value_ref_calls_callback() {
        let expr = vref("Bracket", "width", Type::length());
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("Bracket", "width"), mm_val(80.0));

        let mut reads = Vec::new();
        let result = eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert!(!result.is_undef());
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0], ValueCellId::new("Bracket", "width"));
    }

    #[test]
    fn traced_literal_no_callback() {
        let expr = lit(Value::Int(42), Type::Int);
        let values = ValueMap::new();

        let mut reads = Vec::new();
        let result = eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert_eq!(result, Value::Int(42));
        assert!(reads.is_empty());
    }

    #[test]
    fn traced_binop_two_value_refs() {
        let left = vref("B", "width", Type::length());
        let right = vref("B", "height", Type::length());
        let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::length());

        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "width"), mm_val(80.0));
        values.insert(ValueCellId::new("B", "height"), mm_val(100.0));

        let mut reads = Vec::new();
        eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert_eq!(reads.len(), 2);
        assert_eq!(reads[0], ValueCellId::new("B", "width"));
        assert_eq!(reads[1], ValueCellId::new("B", "height"));
    }

    #[test]
    fn traced_volume_three_reads() {
        // volume = width * height * thickness
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "width"), mm_val(80.0));
        values.insert(ValueCellId::new("B", "height"), mm_val(100.0));
        values.insert(ValueCellId::new("B", "thickness"), mm_val(5.0));

        let w = vref("B", "width", Type::length());
        let h = vref("B", "height", Type::length());
        let t = vref("B", "thickness", Type::length());

        let wh = CompiledExpr::binop(
            BinOp::Mul,
            w,
            h,
            Type::Scalar {
                dimension: DimensionVector::AREA,
            },
        );
        let volume = CompiledExpr::binop(
            BinOp::Mul,
            wh,
            t,
            Type::Scalar {
                dimension: DimensionVector::VOLUME,
            },
        );

        let mut reads = Vec::new();
        let result = eval_expr_traced(&volume, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        // Same result as eval_expr
        let expected = eval_expr(&volume, &values);
        assert_eq!(result.as_f64().unwrap(), expected.as_f64().unwrap());

        // Exactly 3 reads: width, height, thickness
        assert_eq!(reads.len(), 3);
        assert_eq!(reads[0], ValueCellId::new("B", "width"));
        assert_eq!(reads[1], ValueCellId::new("B", "height"));
        assert_eq!(reads[2], ValueCellId::new("B", "thickness"));
    }

    #[test]
    fn traced_conditional_only_traces_taken_branch() {
        // if true then width else height
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "width"), mm_val(80.0));
        values.insert(ValueCellId::new("B", "height"), mm_val(100.0));

        let cond = lit(Value::Bool(true), Type::Bool);
        let then_branch = vref("B", "width", Type::length());
        let else_branch = vref("B", "height", Type::length());
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[42]),
            result_type: Type::length(),
            kind: CompiledExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
        };

        let mut reads = Vec::new();
        eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        // Only the then-branch should be traced (width), not else (height)
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0], ValueCellId::new("B", "width"));
    }

    #[test]
    fn traced_conditional_false_traces_else_branch() {
        // if false then width else height
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "width"), mm_val(80.0));
        values.insert(ValueCellId::new("B", "height"), mm_val(100.0));

        let cond = lit(Value::Bool(false), Type::Bool);
        let then_branch = vref("B", "width", Type::length());
        let else_branch = vref("B", "height", Type::length());
        let expr = CompiledExpr {
            content_hash: reify_types::ContentHash::of(&[43]),
            result_type: Type::length(),
            kind: CompiledExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
        };

        let mut reads = Vec::new();
        eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        // Only the else-branch should be traced (height), not then (width)
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0], ValueCellId::new("B", "height"));
    }

    // --- AND/OR short-circuit tracing tests ---

    #[test]
    fn traced_and_false_short_circuits() {
        // false AND vref(X) → short-circuits, on_read NOT called for X
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "x"), Value::Bool(true));

        let left = lit(Value::Bool(false), Type::Bool);
        let right = vref("B", "x", Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);

        let mut reads = Vec::new();
        let result = eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert_eq!(result, Value::Bool(false));
        assert!(reads.is_empty(), "false AND should short-circuit: right side must NOT be traced");
    }

    #[test]
    fn traced_and_true_evaluates_right() {
        // true AND vref(X) → evaluates right, on_read IS called for X
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "x"), Value::Bool(true));

        let left = lit(Value::Bool(true), Type::Bool);
        let right = vref("B", "x", Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);

        let mut reads = Vec::new();
        let result = eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert_eq!(result, Value::Bool(true));
        assert_eq!(reads.len(), 1, "true AND should evaluate right side");
        assert_eq!(reads[0], ValueCellId::new("B", "x"));
    }

    #[test]
    fn traced_or_true_short_circuits() {
        // true OR vref(X) → short-circuits, on_read NOT called for X
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "x"), Value::Bool(false));

        let left = lit(Value::Bool(true), Type::Bool);
        let right = vref("B", "x", Type::Bool);
        let expr = CompiledExpr::binop(BinOp::Or, left, right, Type::Bool);

        let mut reads = Vec::new();
        let result = eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert_eq!(result, Value::Bool(true));
        assert!(reads.is_empty(), "true OR should short-circuit: right side must NOT be traced");
    }

    #[test]
    fn traced_or_false_evaluates_right() {
        // false OR vref(X) → evaluates right, on_read IS called for X
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "x"), Value::Bool(true));

        let left = lit(Value::Bool(false), Type::Bool);
        let right = vref("B", "x", Type::Bool);
        let expr = CompiledExpr::binop(BinOp::Or, left, right, Type::Bool);

        let mut reads = Vec::new();
        let result = eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert_eq!(result, Value::Bool(true));
        assert_eq!(reads.len(), 1, "false OR should evaluate right side");
        assert_eq!(reads[0], ValueCellId::new("B", "x"));
    }

    #[test]
    fn traced_and_undef_false_both_traced() {
        // Undef AND vref(X)=false → on_read IS called for X, result is Bool(false)
        // Kleene: Undef AND false = false (false is absorbing for AND)
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "x"), Value::Bool(false));

        let left = lit(Value::Undef, Type::Bool);
        let right = vref("B", "x", Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);

        let mut reads = Vec::new();
        let result = eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert_eq!(result, Value::Bool(false));
        assert_eq!(reads.len(), 1, "Undef AND should evaluate right side");
        assert_eq!(reads[0], ValueCellId::new("B", "x"));
    }

    #[test]
    fn traced_and_undef_true_both_traced() {
        // Undef AND vref(X)=true → on_read IS called for X, result is Undef
        // Kleene: Undef AND true = Undef
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "x"), Value::Bool(true));

        let left = lit(Value::Undef, Type::Bool);
        let right = vref("B", "x", Type::Bool);
        let expr = CompiledExpr::binop(BinOp::And, left, right, Type::Bool);

        let mut reads = Vec::new();
        let result = eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert!(result.is_undef());
        assert_eq!(reads.len(), 1, "Undef AND should evaluate right side");
        assert_eq!(reads[0], ValueCellId::new("B", "x"));
    }

    #[test]
    fn traced_or_undef_true_both_traced() {
        // Undef OR vref(X)=true → on_read IS called for X, result is Bool(true)
        // Kleene: Undef OR true = true (true is absorbing for OR)
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("B", "x"), Value::Bool(true));

        let left = lit(Value::Undef, Type::Bool);
        let right = vref("B", "x", Type::Bool);
        let expr = CompiledExpr::binop(BinOp::Or, left, right, Type::Bool);

        let mut reads = Vec::new();
        let result = eval_expr_traced(&expr, &values, &mut |id: &ValueCellId| {
            reads.push(id.clone());
        });

        assert_eq!(result, Value::Bool(true));
        assert_eq!(reads.len(), 1, "Undef OR should evaluate right side");
        assert_eq!(reads[0], ValueCellId::new("B", "x"));
    }
}
