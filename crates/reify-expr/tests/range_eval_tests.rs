//! Range expression evaluation tests.
//!
//! Tests for:
//!   - Range constructor evaluation (RangeConstructor)
//!   - .lower / .upper methods
//!   - .contains(val) method
//!   - .span method

use reify_expr::{EvalContext, eval_expr};
use reify_core::{DimensionVector, Type};
use reify_ir::{CompiledExpr, Value, ValueMap};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build a dimensionless Scalar (mm in SI = meters * 0.001).
/// For test convenience, `mm(1)` = 0.001 m.
fn mm(v: f64) -> Value {
    Value::Scalar {
        si_value: v * 0.001,
        dimension: DimensionVector::LENGTH,
    }
}

/// Build a Type::Scalar for LENGTH.
fn t_mm() -> Type {
    Type::Scalar {
        dimension: DimensionVector::LENGTH,
    }
}

/// Evaluate a range-constructor expression.
fn eval_range(
    lower: Option<Value>,
    upper: Option<Value>,
    lower_inclusive: bool,
    upper_inclusive: bool,
    elem_type: Type,
) -> Value {
    let lo_expr = lower
        .as_ref()
        .map(|v| CompiledExpr::literal(v.clone(), elem_type.clone()));
    let hi_expr = upper
        .as_ref()
        .map(|v| CompiledExpr::literal(v.clone(), elem_type.clone()));
    let range_type = Type::Range(Box::new(elem_type));
    let expr = CompiledExpr::range_constructor(
        lo_expr,
        hi_expr,
        lower_inclusive,
        upper_inclusive,
        range_type,
    );
    eval_expr(&expr, &EvalContext::simple(&ValueMap::new()))
}

/// Evaluate `range_expr.method_name(args)`.
fn eval_method(range_val: Value, method: &str, args: Vec<Value>, result_type: Type) -> Value {
    let range_expr = CompiledExpr::literal(range_val, Type::Range(Box::new(Type::Int)));
    let arg_exprs: Vec<CompiledExpr> = args
        .into_iter()
        .map(|v| CompiledExpr::literal(v, Type::Int))
        .collect();
    let expr = CompiledExpr::method_call(range_expr, method.to_string(), arg_exprs, result_type);
    eval_expr(&expr, &EvalContext::simple(&ValueMap::new()))
}

// ── step-1: range constructor evaluation ─────────────────────────────────────

/// `1..10` (both inclusive) evaluates to Range { lower: Some(1), upper: Some(10), lower_inclusive: true, upper_inclusive: true }
#[test]
fn eval_range_two_sided_inclusive() {
    let result = eval_range(
        Some(Value::Int(1)),
        Some(Value::Int(10)),
        true,
        true,
        Type::Int,
    );
    assert_eq!(
        result,
        Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true,)
    );
}

/// `1..<10` (lower inclusive, upper exclusive) evaluates correctly.
#[test]
fn eval_range_exclusive_upper() {
    let result = eval_range(
        Some(Value::Int(1)),
        Some(Value::Int(10)),
        true,
        false,
        Type::Int,
    );
    assert_eq!(
        result,
        Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, false,)
    );
}

/// `>5` (lower only, exclusive) — upper is None, lower_inclusive forced false.
#[test]
fn eval_range_lower_only() {
    let result = eval_range(Some(Value::Int(5)), None, false, false, Type::Int);
    assert_eq!(
        result,
        Value::range(Some(Value::Int(5)), None, false, false)
    );
}

/// `<10` (upper only, exclusive) — lower is None.
#[test]
fn eval_range_upper_only() {
    let result = eval_range(None, Some(Value::Int(10)), false, false, Type::Int);
    assert_eq!(
        result,
        Value::range(None, Some(Value::Int(10)), false, false)
    );
}

/// `undef..10` propagates to Undef.
#[test]
fn eval_range_undef_lower_propagates() {
    let result = eval_range(
        Some(Value::Undef),
        Some(Value::Int(10)),
        true,
        true,
        Type::Int,
    );
    assert_eq!(result, Value::Undef);
}

/// `1..undef` propagates to Undef.
#[test]
fn eval_range_undef_upper_propagates() {
    let result = eval_range(
        Some(Value::Int(1)),
        Some(Value::Undef),
        true,
        true,
        Type::Int,
    );
    assert_eq!(result, Value::Undef);
}

// ── step-2: .lower method — failing tests ────────────────────────────────────

/// `(1..10).lower` → `Option(Some(Int(1)))`
#[test]
fn range_lower_two_sided() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "lower", vec![], Type::Option(Box::new(Type::Int)));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Int(1)))));
}

/// `(>5mm).lower` → `Option(Some(Scalar{5mm}))`
#[test]
fn range_lower_lower_only_scalar() {
    let range = Value::range(Some(mm(5.0)), None, false, false);
    let result = eval_method(range, "lower", vec![], Type::Option(Box::new(t_mm())));
    assert_eq!(result, Value::Option(Some(Box::new(mm(5.0)))));
}

/// `(<10).lower` → `Option(None)` (no lower bound)
#[test]
fn range_lower_upper_only() {
    let range = Value::range(None, Some(Value::Int(10)), false, false);
    let result = eval_method(range, "lower", vec![], Type::Option(Box::new(Type::Int)));
    assert_eq!(result, Value::Option(None));
}

// ── step-4: .upper method — failing tests ────────────────────────────────────

/// `(1..10).upper` → `Option(Some(Int(10)))`
#[test]
fn range_upper_two_sided() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "upper", vec![], Type::Option(Box::new(Type::Int)));
    assert_eq!(result, Value::Option(Some(Box::new(Value::Int(10)))));
}

/// `(>5).upper` → `Option(None)` (no upper bound)
#[test]
fn range_upper_lower_only() {
    let range = Value::range(Some(Value::Int(5)), None, false, false);
    let result = eval_method(range, "upper", vec![], Type::Option(Box::new(Type::Int)));
    assert_eq!(result, Value::Option(None));
}

/// `(<=10mm).upper` → `Option(Some(Scalar{10mm}))`
#[test]
fn range_upper_upper_only_scalar() {
    let range = Value::range(None, Some(mm(10.0)), false, true);
    let result = eval_method(range, "upper", vec![], Type::Option(Box::new(t_mm())));
    assert_eq!(result, Value::Option(Some(Box::new(mm(10.0)))));
}

// ── step-6: .contains(val) on Range — failing tests ──────────────────────────

/// `(1..10).contains(5)` → `Bool(true)`
#[test]
fn range_contains_mid_true() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "contains", vec![Value::Int(5)], Type::Bool);
    assert_eq!(result, Value::Bool(true));
}

/// `(1..10).contains(0)` → `Bool(false)` (below lower bound)
#[test]
fn range_contains_below_lower_false() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "contains", vec![Value::Int(0)], Type::Bool);
    assert_eq!(result, Value::Bool(false));
}

/// `(1..10).contains(11)` → `Bool(false)` (above upper bound)
#[test]
fn range_contains_above_upper_false() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "contains", vec![Value::Int(11)], Type::Bool);
    assert_eq!(result, Value::Bool(false));
}

/// `(1..10).contains(1)` → `Bool(true)` (inclusive lower boundary)
#[test]
fn range_contains_at_lower_inclusive() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "contains", vec![Value::Int(1)], Type::Bool);
    assert_eq!(result, Value::Bool(true));
}

/// `(1..<10).contains(10)` → `Bool(false)` (exclusive upper boundary)
#[test]
fn range_contains_at_upper_exclusive_false() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, false);
    let result = eval_method(range, "contains", vec![Value::Int(10)], Type::Bool);
    assert_eq!(result, Value::Bool(false));
}

/// `(>5).contains(10)` → `Bool(true)` (single-sided: no upper bound)
#[test]
fn range_contains_single_sided_lower_true() {
    let range = Value::range(Some(Value::Int(5)), None, false, false);
    let result = eval_method(range, "contains", vec![Value::Int(10)], Type::Bool);
    assert_eq!(result, Value::Bool(true));
}

/// `(<10).contains(5)` → `Bool(true)` (single-sided: no lower bound)
#[test]
fn range_contains_single_sided_upper_true() {
    let range = Value::range(None, Some(Value::Int(10)), false, false);
    let result = eval_method(range, "contains", vec![Value::Int(5)], Type::Bool);
    assert_eq!(result, Value::Bool(true));
}

// ── step-8: .contains undef propagation and edge cases ───────────────────────

/// `range.contains(Undef)` → `Undef`
#[test]
fn range_contains_undef_needle_propagates() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "contains", vec![Value::Undef], Type::Bool);
    assert_eq!(result, Value::Undef);
}

/// `(1..10).contains(10)` — inclusive upper boundary → `Bool(true)`
#[test]
fn range_contains_at_upper_inclusive() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "contains", vec![Value::Int(10)], Type::Bool);
    assert_eq!(result, Value::Bool(true));
}

/// Scalar range: `(1mm..10mm).contains(5mm)` → `Bool(true)`
#[test]
fn range_contains_scalar_matching_dimension() {
    let range = Value::range(Some(mm(1.0)), Some(mm(10.0)), true, true);
    let result = eval_method(range, "contains", vec![mm(5.0)], Type::Bool);
    assert_eq!(result, Value::Bool(true));
}

/// Dimension mismatch: scalar range, incompatible scalar needle → Undef.
#[test]
fn range_contains_wrong_dimension_scalar_undef() {
    let time_val = Value::Scalar {
        si_value: 5.0,
        dimension: DimensionVector::TIME,
    };
    let range = Value::range(Some(mm(1.0)), Some(mm(10.0)), true, true);
    let result = eval_method(range, "contains", vec![time_val], Type::Bool);
    assert_eq!(result, Value::Undef);
}

// ── step-10: .span method — failing tests ────────────────────────────────────

/// `(1..10).span` → `Int(9)`
#[test]
fn range_span_int() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "span", vec![], Type::Int);
    assert_eq!(result, Value::Int(9));
}

/// `(1.0..10.0).span` → `Real(9.0)`
#[test]
fn range_span_real() {
    let range = Value::range(Some(Value::Real(1.0)), Some(Value::Real(10.0)), true, true);
    let result = eval_method(range, "span", vec![], Type::dimensionless_scalar());
    assert_eq!(result, Value::Real(9.0));
}

/// `(1mm..10mm).span` → `Scalar{9mm}`
#[test]
fn range_span_scalar() {
    let range = Value::range(Some(mm(1.0)), Some(mm(10.0)), true, true);
    let result = eval_method(range, "span", vec![], t_mm());
    assert_eq!(result, mm(9.0));
}

/// `(>5).span` → `Undef` (single-sided, no upper)
#[test]
fn range_span_lower_only_undef() {
    let range = Value::range(Some(Value::Int(5)), None, false, false);
    let result = eval_method(range, "span", vec![], Type::Int);
    assert_eq!(result, Value::Undef);
}

/// `(<10).span` → `Undef` (single-sided, no lower)
#[test]
fn range_span_upper_only_undef() {
    let range = Value::range(None, Some(Value::Int(10)), false, false);
    let result = eval_method(range, "span", vec![], Type::Int);
    assert_eq!(result, Value::Undef);
}

// ── step-12: edge cases ──────────────────────────────────────────────────────

/// `.lower` on a non-Range value → `Undef`
#[test]
fn lower_on_non_range_is_undef() {
    let expr = CompiledExpr::method_call(
        CompiledExpr::literal(Value::Int(42), Type::Int),
        "lower".to_string(),
        vec![],
        Type::Option(Box::new(Type::Int)),
    );
    let result = eval_expr(&expr, &EvalContext::simple(&ValueMap::new()));
    assert_eq!(result, Value::Undef);
}

/// `.upper` on a non-Range value → `Undef`
#[test]
fn upper_on_non_range_is_undef() {
    let expr = CompiledExpr::method_call(
        CompiledExpr::literal(Value::Int(42), Type::Int),
        "upper".to_string(),
        vec![],
        Type::Option(Box::new(Type::Int)),
    );
    let result = eval_expr(&expr, &EvalContext::simple(&ValueMap::new()));
    assert_eq!(result, Value::Undef);
}

/// `.span` on a non-Range value → `Undef`
#[test]
fn span_on_non_range_is_undef() {
    let expr = CompiledExpr::method_call(
        CompiledExpr::literal(Value::Int(42), Type::Int),
        "span".to_string(),
        vec![],
        Type::Int,
    );
    let result = eval_expr(&expr, &EvalContext::simple(&ValueMap::new()));
    assert_eq!(result, Value::Undef);
}

/// `.contains` called with wrong arg count (0 args) → `Undef`
#[test]
fn contains_wrong_arg_count_undef() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    let result = eval_method(range, "contains", vec![], Type::Bool);
    assert_eq!(result, Value::Undef);
}

/// `.span` called with args (should be 0) → `Undef`
#[test]
fn span_with_args_undef() {
    let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
    // Build span call with an extra argument (invalid)
    let range_expr = CompiledExpr::literal(range, Type::Range(Box::new(Type::Int)));
    let extra_arg = CompiledExpr::literal(Value::Int(1), Type::Int);
    let expr =
        CompiledExpr::method_call(range_expr, "span".to_string(), vec![extra_arg], Type::Int);
    let result = eval_expr(&expr, &EvalContext::simple(&ValueMap::new()));
    assert_eq!(result, Value::Undef);
}
