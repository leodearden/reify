//! Pins the value-layer rule "Int arithmetic overflow yields `Undef`, never a
//! panic or a wrapped value" for every `(Int, Int)` operator arm.
//!
//! Every row drives only the public `reify_expr::eval_expr` seam. Overflow rows
//! and stays-`Int` boundary rows live in separate `#[test]` fns, so a panic in
//! an overflow fn never hides the boundary fns that prove nothing is
//! over-rejected.

use std::cell::RefCell;

use reify_core::{DiagnosticCode, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{BinOp, CompiledExpr, UndefCause, Value, ValueMap};

const MAX: i64 = i64::MAX;
const MIN: i64 = i64::MIN;

// ── Driver ───────────────────────────────────────────────────────────────────

/// Build the literal-operand expression `lv OP rv`. The evaluator dispatches on
/// the `Value` variant, never on the declared `Type`, so the dimensionless
/// annotation is a valid placeholder for any operands.
fn binop_expr(op: BinOp, lv: Value, rv: Value) -> CompiledExpr {
    let left = CompiledExpr::literal(lv, Type::dimensionless_scalar());
    let right = CompiledExpr::literal(rv, Type::dimensionless_scalar());
    CompiledExpr::binop(op, left, right, Type::dimensionless_scalar())
}

fn eval_binop(op: BinOp, lv: Value, rv: Value) -> Value {
    let values = ValueMap::new();
    eval_expr(&binop_expr(op, lv, rv), &EvalContext::simple(&values))
}

/// Assert `a OP b` over two `Value::Int` operands evaluates to `expected`; the
/// failure message names the expression.
#[track_caller]
fn assert_int_binop(op: BinOp, a: i64, b: i64, expected: Value) {
    assert_eq!(
        eval_binop(op, Value::Int(a), Value::Int(b)),
        expected,
        "{a} {op:?} {b}"
    );
}

// ── Addition ─────────────────────────────────────────────────────────────────

#[test]
fn int_add_overflow_is_undef() {
    assert_int_binop(BinOp::Add, MAX, 1, Value::Undef);
    assert_int_binop(BinOp::Add, MIN, -1, Value::Undef);
    assert_int_binop(BinOp::Add, 1, MAX, Value::Undef);
}

#[test]
fn int_add_at_boundary_stays_int() {
    assert_int_binop(BinOp::Add, MAX - 1, 1, Value::Int(MAX));
    assert_int_binop(BinOp::Add, MIN, MAX, Value::Int(-1));
    assert_int_binop(BinOp::Add, 2, 3, Value::Int(5));
}

// ── Subtraction ──────────────────────────────────────────────────────────────

#[test]
fn int_sub_overflow_is_undef() {
    assert_int_binop(BinOp::Sub, MIN, 1, Value::Undef);
    assert_int_binop(BinOp::Sub, MAX, -1, Value::Undef);
    assert_int_binop(BinOp::Sub, 0, MIN, Value::Undef);
}

#[test]
fn int_sub_at_boundary_stays_int() {
    assert_int_binop(BinOp::Sub, MIN + 1, 1, Value::Int(MIN));
    assert_int_binop(BinOp::Sub, 0, MAX, Value::Int(-MAX));
    assert_int_binop(BinOp::Sub, -1, MAX, Value::Int(MIN));
}

// ── Multiplication ───────────────────────────────────────────────────────────

#[test]
fn int_mul_overflow_is_undef() {
    assert_int_binop(BinOp::Mul, MAX, 2, Value::Undef);
    assert_int_binop(BinOp::Mul, MIN, -1, Value::Undef);
    assert_int_binop(BinOp::Mul, -1, MIN, Value::Undef);
    assert_int_binop(BinOp::Mul, 4_294_967_296, 4_294_967_296, Value::Undef);
    assert_int_binop(BinOp::Mul, 3_037_000_500, 3_037_000_500, Value::Undef);
}

#[test]
fn int_mul_at_boundary_stays_int() {
    assert_int_binop(BinOp::Mul, MAX, 1, Value::Int(MAX));
    assert_int_binop(BinOp::Mul, MIN, 1, Value::Int(MIN));
    assert_int_binop(BinOp::Mul, -1, MAX, Value::Int(-MAX));
    assert_int_binop(
        BinOp::Mul,
        3_037_000_499,
        3_037_000_499,
        Value::Int(9_223_372_030_926_249_001),
    );
}

// ── Division ─────────────────────────────────────────────────────────────────

#[test]
fn int_div_overflow_is_undef() {
    assert_int_binop(BinOp::Div, MIN, -1, Value::Undef);
}

#[test]
fn int_div_at_boundary_keeps_semantics() {
    assert_int_binop(BinOp::Div, 6, 3, Value::Int(2));
    assert_int_binop(BinOp::Div, 7, 2, Value::Real(3.5));
    assert_int_binop(BinOp::Div, -7, 2, Value::Real(-3.5));
    assert_int_binop(BinOp::Div, MIN, 1, Value::Int(MIN));
    assert_int_binop(BinOp::Div, MIN, 2, Value::Int(-4_611_686_018_427_387_904));
    assert_int_binop(BinOp::Div, MIN, -2, Value::Int(4_611_686_018_427_387_904));
    assert_int_binop(BinOp::Div, MAX, -1, Value::Int(-MAX));
    assert_int_binop(BinOp::Div, 7, 0, Value::Undef);
}

// ── Remainder ────────────────────────────────────────────────────────────────

#[test]
fn int_rem_at_boundary_keeps_semantics() {
    assert_int_binop(BinOp::Mod, 7, 3, Value::Int(1));
    assert_int_binop(BinOp::Mod, -7, 3, Value::Int(-1));
    assert_int_binop(BinOp::Mod, 7, -3, Value::Int(1));
    assert_int_binop(BinOp::Mod, MAX, -1, Value::Int(0));
    assert_int_binop(BinOp::Mod, MIN, -1, Value::Int(0));
    assert_int_binop(BinOp::Mod, MIN, 1, Value::Int(0));
    assert_int_binop(BinOp::Mod, MIN, 2, Value::Int(0));
    assert_int_binop(BinOp::Mod, 7, 0, Value::Undef);
}

// ── Exponentiation ───────────────────────────────────────────────────────────

#[test]
fn int_pow_overflow_is_undef() {
    assert_int_binop(BinOp::Pow, 2, 63, Value::Undef);
    assert_int_binop(BinOp::Pow, 10, 19, Value::Undef);
    assert_int_binop(BinOp::Pow, -3, 40, Value::Undef);
}

#[test]
fn int_pow_at_boundary_stays_int() {
    assert_int_binop(BinOp::Pow, 2, 62, Value::Int(4_611_686_018_427_387_904));
    assert_int_binop(BinOp::Pow, -2, 63, Value::Int(MIN));
    assert_int_binop(BinOp::Pow, 10, 18, Value::Int(1_000_000_000_000_000_000));
    assert_int_binop(BinOp::Pow, 3, 0, Value::Int(1));
    assert_int_binop(BinOp::Pow, 0, 0, Value::Int(1));
}

#[test]
fn int_pow_negative_exponent_keeps_real_semantics() {
    assert_int_binop(BinOp::Pow, 2, -1, Value::Real(0.5));
    assert_int_binop(BinOp::Pow, 2, -2, Value::Real(0.25));
    assert_int_binop(BinOp::Pow, 0, -1, Value::Undef);
}

#[test]
fn int_pow_exponent_beyond_i32_is_exact_or_undef() {
    assert_int_binop(BinOp::Pow, 1, 4_294_967_295, Value::Int(1));
    assert_int_binop(BinOp::Pow, -1, 4_294_967_295, Value::Int(-1));
    assert_int_binop(BinOp::Pow, -1, 4_294_967_294, Value::Int(1));
    assert_int_binop(BinOp::Pow, 0, 3_000_000_000, Value::Int(0));
    assert_int_binop(BinOp::Pow, 2, 3_000_000_000, Value::Undef);
    assert_int_binop(BinOp::Pow, 2, 4_294_967_296, Value::Undef);
    assert_int_binop(BinOp::Pow, 2, -4_294_967_296, Value::Undef);
}

// ── Reaching the Int arm through composite values and the op sink ────────────

#[test]
fn int_overflow_inside_vector_add_is_undef() {
    let lhs = Value::Vector(vec![Value::Int(MAX), Value::Int(1)]);
    let rhs = Value::Vector(vec![Value::Int(1), Value::Int(1)]);
    assert_eq!(
        eval_binop(BinOp::Add, lhs, rhs),
        Value::Undef,
        "[MAX, 1] + [1, 1] overflows its first component"
    );
}

#[test]
fn int_div_overflow_inside_vector_is_undef() {
    let numerators = Value::Vector(vec![Value::Int(MIN), Value::Int(2)]);
    assert_eq!(
        eval_binop(BinOp::Div, numerators, Value::Int(-1)),
        Value::Undef,
        "[MIN, 2] / -1 overflows its first component"
    );
}

#[test]
fn int_overflow_reports_op_contract_failure() {
    let expr = binop_expr(BinOp::Add, Value::Int(MAX), Value::Int(1));
    let values = ValueMap::new();
    let sink = RefCell::new(Vec::new());
    let ctx = EvalContext::simple(&values).with_undef_cause_sink(&sink);

    assert_eq!(eval_expr(&expr, &ctx), Value::Undef, "MAX + 1");

    let causes = sink.borrow();
    assert_eq!(
        causes.len(),
        1,
        "exactly one cause expected, got {causes:?}"
    );
    assert!(
        matches!(
            &causes[0],
            UndefCause::OpContractFailed {
                code: DiagnosticCode::OpContractViolation,
                ..
            }
        ),
        "expected OpContractFailed {{ OpContractViolation }}, got {:?}",
        causes[0]
    );
}
