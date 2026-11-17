//! Pins the value-layer rule "a `*` or `/` whose combined dimension overflows
//! `Rational` yields `Undef`, never a panic" for every dimension-combining arm
//! of `eval_mul` and `eval_div`.
//!
//! Every row drives only the public `reify_expr::eval_expr` seam. Overflow rows
//! and in-range boundary rows live in separate `#[test]` fns, so a panic in an
//! overflow fn never hides the boundary fns that prove nothing is
//! over-rejected.

use reify_core::{DimensionVector, Rational, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};

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

/// Assert `lv OP rv` evaluates to `expected`; the failure message names the
/// expression.
#[track_caller]
fn assert_binop(op: BinOp, lv: Value, rv: Value, expected: Value) {
    let shown = format!("{lv:?} {op:?} {rv:?}");
    assert_eq!(eval_binop(op, lv, rv), expected, "{shown}");
}

// ── Fixtures ─────────────────────────────────────────────────────────────────

/// `LENGTH` to the rational power `num/den`, which must not overflow.
fn length_pow(num: i16, den: i16) -> DimensionVector {
    DimensionVector::LENGTH
        .checked_pow_rational(Rational::new(num, den))
        .expect("a small power of LENGTH does not overflow")
}

/// `LENGTH^(68/1147)`, the dimension of `1m^1/31 * 1m^1/37`. Combined with
/// `LENGTH^(±1/41)` its Length exponent needs denominator 47027.
fn a() -> DimensionVector {
    length_pow(68, 1147)
}

fn b() -> DimensionVector {
    length_pow(1, 41)
}

fn b_neg() -> DimensionVector {
    length_pow(-1, 41)
}

/// A Length slot of `i16::MIN`, whose reciprocal `0 - i16::MIN` does not fit.
fn length_min() -> DimensionVector {
    let mut dimension = DimensionVector::DIMENSIONLESS;
    dimension.0[0] = Rational::new(i16::MIN, 1);
    dimension
}

fn scalar(dimension: DimensionVector) -> Value {
    Value::Scalar {
        si_value: 2.0,
        dimension,
    }
}

fn complex(dimension: DimensionVector) -> Value {
    Value::Complex {
        re: 3.0,
        im: 4.0,
        dimension,
    }
}

// ── Overflow rows ────────────────────────────────────────────────────────────

#[test]
fn mul_whose_dimension_overflows_is_undef() {
    assert_binop(BinOp::Mul, scalar(a()), scalar(b()), Value::Undef);
    assert_binop(BinOp::Mul, complex(a()), complex(b()), Value::Undef);
    assert_binop(BinOp::Mul, complex(a()), scalar(b()), Value::Undef);
    assert_binop(BinOp::Mul, scalar(b()), complex(a()), Value::Undef);
}

#[test]
fn div_whose_dimension_overflows_is_undef() {
    assert_binop(BinOp::Div, scalar(a()), scalar(b_neg()), Value::Undef);
    assert_binop(BinOp::Div, complex(a()), complex(b_neg()), Value::Undef);
    assert_binop(BinOp::Div, complex(a()), scalar(b_neg()), Value::Undef);
}

#[test]
fn reciprocal_whose_dimension_overflows_is_undef() {
    assert_binop(
        BinOp::Div,
        Value::Real(1.0),
        scalar(length_min()),
        Value::Undef,
    );
    assert_binop(
        BinOp::Div,
        Value::Int(1),
        scalar(length_min()),
        Value::Undef,
    );
}

// ── Boundary rows ────────────────────────────────────────────────────────────

/// `LENGTH^(1/127)` combined with `LENGTH^(±1/126)`: the widest Length
/// exponent two `^1/q` literals can reach, `253/16002`.
fn widest() -> DimensionVector {
    length_pow(253, 16002)
}

#[test]
fn mul_at_boundary_stays_defined() {
    let (p, q) = (length_pow(1, 127), length_pow(1, 126));
    assert_binop(
        BinOp::Mul,
        scalar(p),
        scalar(q),
        Value::Scalar {
            si_value: 4.0,
            dimension: widest(),
        },
    );
    assert_binop(
        BinOp::Mul,
        complex(p),
        complex(q),
        Value::Complex {
            re: -7.0,
            im: 24.0,
            dimension: widest(),
        },
    );
    let scaled = Value::Complex {
        re: 6.0,
        im: 8.0,
        dimension: widest(),
    };
    assert_binop(BinOp::Mul, complex(p), scalar(q), scaled.clone());
    assert_binop(BinOp::Mul, scalar(q), complex(p), scaled);
}

#[test]
fn div_at_boundary_stays_defined() {
    let (p, q_neg) = (length_pow(1, 127), length_pow(-1, 126));
    assert_binop(
        BinOp::Div,
        scalar(p),
        scalar(q_neg),
        Value::Scalar {
            si_value: 1.0,
            dimension: widest(),
        },
    );
    assert_binop(
        BinOp::Div,
        complex(p),
        complex(q_neg),
        Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: widest(),
        },
    );
    assert_binop(
        BinOp::Div,
        complex(p),
        scalar(q_neg),
        Value::Complex {
            re: 1.5,
            im: 2.0,
            dimension: widest(),
        },
    );
}

#[test]
fn reciprocal_at_boundary_stays_defined() {
    let reciprocal = Value::Scalar {
        si_value: 0.5,
        dimension: length_pow(32767, 1),
    };
    let denominator = scalar(length_pow(-32767, 1));
    assert_binop(
        BinOp::Div,
        Value::Real(1.0),
        denominator.clone(),
        reciprocal.clone(),
    );
    assert_binop(BinOp::Div, Value::Int(1), denominator, reciprocal);
}
