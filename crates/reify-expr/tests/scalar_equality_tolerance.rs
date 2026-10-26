//! `==` / `!=` verdicts on Scalar and Real operands are toleranced through
//! `reify_core::scalar_tolerance` (task #6653), an Int meeting a Real is
//! promoted and toleranced too, while Int-to-Int equality, dimension
//! mismatch, Kleene propagation and ordering comparisons keep their exact
//! semantics.

use reify_core::DimensionVector;
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{CompiledExpr, Value, ValueMap};
use reify_test_support::{eq, ge, kg, literal, meters, mm, ne, value_ref};

fn eval(expr: &CompiledExpr) -> Value {
    let values = ValueMap::new();
    eval_expr(expr, &EvalContext::simple(&values))
}

fn eq_verdict(a: Value, b: Value) -> Value {
    eval(&eq(literal(a), literal(b)))
}

fn ne_verdict(a: Value, b: Value) -> Value {
    eval(&ne(literal(a), literal(b)))
}

#[test]
fn solver_noise_on_a_length_is_equal() {
    assert_eq!(
        eq_verdict(meters(0.12299999999999939), meters(0.123)),
        Value::Bool(true),
        "auto_probe solved x == 123mm"
    );
    assert_eq!(
        ne_verdict(meters(0.12299999999999939), meters(0.123)),
        Value::Bool(false),
        "auto_probe solved x != 123mm"
    );
}

#[test]
fn one_percent_length_drift_is_not_equal() {
    assert_eq!(eq_verdict(mm(10.0), mm(10.1)), Value::Bool(false));
    assert_eq!(ne_verdict(mm(10.0), mm(10.1)), Value::Bool(true));
}

#[test]
fn real_float_noise_is_equal() {
    assert_eq!(
        eq_verdict(Value::Real(0.1 + 0.2), Value::Real(0.3)),
        Value::Bool(true)
    );
}

#[test]
fn absolute_floor_follows_the_operand_dimension() {
    assert_eq!(
        eq_verdict(Value::Real(0.0), Value::Real(5e-7)),
        Value::Bool(false),
        "dimensionless 0 vs 5e-7 sits above the default floor"
    );
    assert_eq!(
        eq_verdict(meters(0.0), meters(5e-7)),
        Value::Bool(true),
        "Length 0 vs 5e-7 m sits inside the Length floor"
    );
}

#[test]
fn integer_equality_stays_exact() {
    assert_eq!(
        eq_verdict(Value::Int(1_000_000_000), Value::Int(1_000_000_001)),
        Value::Bool(false)
    );
}

#[test]
fn an_int_meeting_a_real_is_compared_toleranced() {
    assert_eq!(
        eq_verdict(Value::Int(3), Value::Real(0.3 / 0.1)),
        Value::Bool(true),
        "float noise on the Real side of an Int comparison"
    );
    assert_eq!(
        eq_verdict(Value::Int(1_000_000_001), Value::Real(1e9)),
        Value::Bool(true),
        "the promoted Int takes the relative tolerance"
    );
    assert_eq!(
        eq_verdict(Value::Int(1_000_000_001), Value::Int(1_000_000_000)),
        Value::Bool(false),
        "the same pair as two Ints stays exact"
    );
}

#[test]
fn dimension_mismatch_is_not_equal() {
    assert_eq!(eq_verdict(meters(0.005), kg(0.005)), Value::Bool(false));
}

#[test]
fn undefined_operand_stays_undefined() {
    let expr = eq(literal(meters(0.01)), value_ref("absent", "cell"));
    assert_eq!(eval(&expr), Value::Undef);
}

#[test]
fn ordering_comparisons_stay_exact() {
    let a = || literal(mm(10.0));
    let b = || {
        literal(Value::Scalar {
            si_value: 0.01 + 1e-10,
            dimension: DimensionVector::LENGTH,
        })
    };
    assert_eq!(
        eval(&ge(a(), b())),
        Value::Bool(false),
        "10mm >= 10mm + 1e-10 m"
    );
    assert_eq!(
        eval(&eq(a(), b())),
        Value::Bool(true),
        "10mm == 10mm + 1e-10 m"
    );
}
