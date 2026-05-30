//! Complex<Q> arithmetic and method evaluation tests.

use reify_expr::{EvalContext, eval_expr};
use reify_core::{DimensionVector, Type};
use reify_ir::{BinOp, CompiledExpr, UnOp, Value, ValueMap};

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
    let expr = CompiledExpr::method_call(lit(obj, obj_ty), method.to_string(), vec![], result_ty);
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

// ─── step-5: Complex * Complex ─────────────────────────────────────────────

/// (1+2i)*(3+4i) = (1*3-2*4)+(1*4+2*3)i = -5+10i, dimensionless.
#[test]
fn complex_mul_dimensionless() {
    let result = eval_binop(
        BinOp::Mul,
        complex_val(1.0, 2.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        complex_val(3.0, 4.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    assert_eq!(
        result,
        complex_val(-5.0, 10.0, DimensionVector::DIMENSIONLESS)
    );
}

/// Complex<Length> * Complex<Time> combines dimensions via mul().
#[test]
fn complex_mul_dimension_product() {
    // (2+3i)*Length * (4+5i)*Time = (2*4-3*5)+(2*5+3*4)i = (-7+22i) Length*Time
    let result = eval_binop(
        BinOp::Mul,
        complex_val(2.0, 3.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        complex_val(4.0, 5.0, DimensionVector::TIME),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    let expected_dim = DimensionVector::LENGTH.mul(&DimensionVector::TIME);
    assert_eq!(result, complex_val(-7.0, 22.0, expected_dim));
}

// ─── step-7: Complex * Scalar / Int / Real (mixed) ─────────────────────────

/// Complex<Length> * Scalar<Time> scales re/im and combines dimensions.
#[test]
fn complex_mul_scalar_right() {
    // (2+3i)*Length * 4*Time = (8+12i) Length*Time
    let result = eval_binop(
        BinOp::Mul,
        complex_val(2.0, 3.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        scalar_val(4.0, DimensionVector::TIME),
        Type::Real,
        Type::complex(Type::Real),
    );
    let expected_dim = DimensionVector::LENGTH.mul(&DimensionVector::TIME);
    assert_eq!(result, complex_val(8.0, 12.0, expected_dim));
}

/// Scalar<Time> * Complex<Length> is commutative.
#[test]
fn scalar_mul_complex_left() {
    let result = eval_binop(
        BinOp::Mul,
        scalar_val(4.0, DimensionVector::TIME),
        Type::Real,
        complex_val(2.0, 3.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::Real),
    );
    let expected_dim = DimensionVector::TIME.mul(&DimensionVector::LENGTH);
    assert_eq!(result, complex_val(8.0, 12.0, expected_dim));
}

/// Complex * Int: dimensionless integer multiplier preserves Complex dimension.
#[test]
fn complex_mul_int() {
    let result = eval_binop(
        BinOp::Mul,
        complex_val(2.0, 3.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Value::Int(5),
        Type::Int,
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(10.0, 15.0, DimensionVector::LENGTH));
}

/// Int * Complex: commutative.
#[test]
fn int_mul_complex() {
    let result = eval_binop(
        BinOp::Mul,
        Value::Int(5),
        Type::Int,
        complex_val(2.0, 3.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(10.0, 15.0, DimensionVector::LENGTH));
}

/// Complex * Real: dimensionless real multiplier preserves Complex dimension.
#[test]
fn complex_mul_real() {
    let result = eval_binop(
        BinOp::Mul,
        complex_val(2.0, 3.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Value::Real(0.5),
        Type::Real,
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(1.0, 1.5, DimensionVector::LENGTH));
}

/// Real * Complex: commutative.
#[test]
fn real_mul_complex() {
    let result = eval_binop(
        BinOp::Mul,
        Value::Real(0.5),
        Type::Real,
        complex_val(2.0, 3.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(1.0, 1.5, DimensionVector::LENGTH));
}

// ─── step-9: Complex division ──────────────────────────────────────────────

/// Complex<Area> / Scalar<Length> returns Complex<Length>.
#[test]
fn complex_div_scalar() {
    let result = eval_binop(
        BinOp::Div,
        complex_val(6.0, 8.0, DimensionVector::AREA),
        Type::complex(Type::Real),
        scalar_val(2.0, DimensionVector::LENGTH),
        Type::length(),
        Type::complex(Type::length()),
    );
    let expected_dim = DimensionVector::AREA.div(&DimensionVector::LENGTH);
    assert_eq!(result, complex_val(3.0, 4.0, expected_dim));
}

/// Complex / Int halves re/im, preserves dimension.
#[test]
fn complex_div_int() {
    let result = eval_binop(
        BinOp::Div,
        complex_val(6.0, 8.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Value::Int(2),
        Type::Int,
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(3.0, 4.0, DimensionVector::LENGTH));
}

/// Complex / Real halves re/im, preserves dimension.
#[test]
fn complex_div_real() {
    let result = eval_binop(
        BinOp::Div,
        complex_val(6.0, 8.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Value::Real(2.0),
        Type::Real,
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(3.0, 4.0, DimensionVector::LENGTH));
}

/// Complex / 0 (Int) returns Undef.
#[test]
fn complex_div_by_zero_int() {
    let result = eval_binop(
        BinOp::Div,
        complex_val(6.0, 8.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Value::Int(0),
        Type::Int,
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}

/// Complex / Scalar(0.0) returns Undef.
#[test]
fn complex_div_by_zero_scalar() {
    let result = eval_binop(
        BinOp::Div,
        complex_val(6.0, 8.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        scalar_val(0.0, DimensionVector::LENGTH),
        Type::length(),
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}

/// Complex<DIMENSIONLESS> / Complex<DIMENSIONLESS>: 1/i = -i.
/// Pins formula re=(ac+bd)/denom, im=(bc-ad)/denom with a=1,b=0,c=0,d=1: denom=1, re=0, im=-1.
#[test]
fn complex_div_complex_dimensionless() {
    let result = eval_binop(
        BinOp::Div,
        complex_val(1.0, 0.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        complex_val(0.0, 1.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(0.0, -1.0, DimensionVector::DIMENSIONLESS));
}

/// Complex<Area> / Complex<Length>: dimension quotient is Length; re=3, im=4.
/// a=6,b=8,c=2,d=0: denom=4, re=12/4=3, im=16/4=4.
#[test]
fn complex_div_complex_dimension_quotient() {
    let expected_dim = DimensionVector::AREA.div(&DimensionVector::LENGTH);
    let result = eval_binop(
        BinOp::Div,
        complex_val(6.0, 8.0, DimensionVector::AREA),
        Type::complex(Type::Real),
        complex_val(2.0, 0.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(3.0, 4.0, expected_dim));
}

/// Complex / Complex(0+0i) returns Undef via the arm's explicit denom==0 guard.
/// Also proves Complex.as_f64()==None so the top-level guard does not misfire.
#[test]
fn complex_div_complex_by_zero_returns_undef() {
    let result = eval_binop(
        BinOp::Div,
        complex_val(6.0, 8.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        complex_val(0.0, 0.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}

/// Operator-path overflow propagates as an Inf-bearing Complex (no sanitize_value by design).
/// denom = 0.25; re = MAX/0.25 = Inf; im = 0/0.25 = 0.
/// Pins the deliberate operator-vs-builtin divergence: the complex_div builtin sanitizes
/// (Inf→Undef), but the operator arm does not — matching eval_mul Complex*Complex convention.
#[test]
fn complex_div_complex_overflow_propagates_infinity() {
    let result = eval_binop(
        BinOp::Div,
        complex_val(f64::MAX, 0.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        complex_val(0.5, 0.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    assert!(
        matches!(&result, Value::Complex { re, im, .. } if re.is_infinite() && *im == 0.0),
        "expected Inf-bearing Complex, got: {result:?}",
    );
}

// ─── step-11: Unary negation ───────────────────────────────────────────────

/// Negating a Complex value negates both re and im, preserves dimension.
#[test]
fn complex_negation() {
    let result = eval_unop(
        UnOp::Neg,
        complex_val(3.0, 4.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(-3.0, -4.0, DimensionVector::LENGTH));
}

// ─── step-13: .re and .im methods ──────────────────────────────────────────

/// .re on dimensionless Complex returns Real.
#[test]
fn method_re_dimensionless() {
    let result = eval_method(
        complex_val(3.0, 4.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        "re",
        Type::Real,
    );
    assert_eq!(result, Value::Real(3.0));
}

/// .re on dimensioned Complex returns Scalar with that dimension.
#[test]
fn method_re_dimensioned() {
    let result = eval_method(
        complex_val(3.0, 4.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        "re",
        Type::length(),
    );
    assert_eq!(result, scalar_val(3.0, DimensionVector::LENGTH));
}

/// .im on dimensionless Complex returns Real.
#[test]
fn method_im_dimensionless() {
    let result = eval_method(
        complex_val(3.0, 4.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        "im",
        Type::Real,
    );
    assert_eq!(result, Value::Real(4.0));
}

/// .im on dimensioned Complex returns Scalar with that dimension.
#[test]
fn method_im_dimensioned() {
    let result = eval_method(
        complex_val(3.0, 4.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        "im",
        Type::length(),
    );
    assert_eq!(result, scalar_val(4.0, DimensionVector::LENGTH));
}

// ─── step-15: .magnitude() ─────────────────────────────────────────────────

/// .magnitude() on dimensionless Complex returns Real (3+4i → 5.0).
#[test]
fn method_magnitude_dimensionless() {
    let result = eval_method(
        complex_val(3.0, 4.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        "magnitude",
        Type::Real,
    );
    assert_eq!(result, Value::Real(5.0));
}

/// .magnitude() on dimensioned Complex returns Scalar with that dimension.
#[test]
fn method_magnitude_dimensioned() {
    let result = eval_method(
        complex_val(3.0, 4.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        "magnitude",
        Type::length(),
    );
    assert_eq!(result, scalar_val(5.0, DimensionVector::LENGTH));
}

// ─── step-17: .phase() ─────────────────────────────────────────────────────

/// .phase() returns Scalar with ANGLE dimension regardless of input dimension.
#[test]
fn method_phase() {
    // Complex(1.0, 1.0) → phase = atan2(1.0, 1.0) = PI/4
    let result = eval_method(
        complex_val(1.0, 1.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        "phase",
        Type::Real,
    );
    let expected = scalar_val(std::f64::consts::FRAC_PI_4, DimensionVector::ANGLE);
    assert_eq!(result, expected);
}

// ─── step-19: .conjugate() ──────────────────────────────────────────────────

/// .conjugate() negates the imaginary part, preserves re and dimension.
#[test]
fn method_conjugate() {
    let result = eval_method(
        complex_val(3.0, 4.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        "conjugate",
        Type::complex(Type::length()),
    );
    assert_eq!(result, complex_val(3.0, -4.0, DimensionVector::LENGTH));
}

// Non-finite sanitization: any NaN or Inf component returns Undef.
// Signed-zero and both-components-non-finite cases are intentionally omitted.
fn assert_conjugate_undef(re: f64, im: f64) {
    let result = eval_method(
        complex_val(re, im, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        "conjugate",
        Type::complex(Type::Real),
    );
    assert!(result.is_undef());
}

#[test]
fn conjugate_nan_re_undef() {
    assert_conjugate_undef(f64::NAN, 1.0);
}
#[test]
fn conjugate_nan_im_undef() {
    assert_conjugate_undef(1.0, f64::NAN);
}
#[test]
fn conjugate_inf_re_undef() {
    assert_conjugate_undef(f64::INFINITY, 1.0);
}
#[test]
fn conjugate_neg_inf_re_undef() {
    assert_conjugate_undef(f64::NEG_INFINITY, 1.0);
}
#[test]
fn conjugate_neg_inf_im_undef() {
    assert_conjugate_undef(1.0, f64::NEG_INFINITY);
}
#[test]
fn conjugate_pos_inf_im_undef() {
    assert_conjugate_undef(1.0, f64::INFINITY);
}

/// .conjugate() with non-finite re and a LENGTH dimension returns Undef.
/// Verifies the pre-guard isn't short-circuited by dimension-aware branches.
#[test]
fn conjugate_inf_re_undef_dimensioned() {
    let result = eval_method(
        complex_val(f64::INFINITY, 1.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        "conjugate",
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}

/// .conjugate() on pure-imaginary input: (0+5i).conjugate == (0-5i).
/// Guards against the non-finite pre-guard accidentally rejecting finite values.
/// Uses DIMENSIONLESS (orthogonal to method_conjugate which uses LENGTH).
#[test]
fn conjugate_pure_imaginary() {
    let result = eval_method(
        complex_val(0.0, 5.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        "conjugate",
        Type::complex(Type::Real),
    );
    assert_eq!(
        result,
        complex_val(0.0, -5.0, DimensionVector::DIMENSIONLESS)
    );
}

/// .conjugate() with negative imaginary part flips it to positive: (2-3i).conjugate == (2+3i).
/// Verifies that negating a negative imaginary part correctly produces a positive result.
#[test]
fn conjugate_negative_im() {
    let result = eval_method(
        complex_val(2.0, -3.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        "conjugate",
        Type::complex(Type::Real),
    );
    assert_eq!(
        result,
        complex_val(2.0, 3.0, DimensionVector::DIMENSIONLESS)
    );
}

// ─── step-21: Edge-case tests ───────────────────────────────────────────────

/// Complex + Int returns Undef (not supported).
#[test]
fn complex_add_non_complex_undef() {
    let result = eval_binop(
        BinOp::Add,
        complex_val(1.0, 2.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Value::Int(3),
        Type::Int,
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}

/// Calling .re on a Scalar (non-Complex) returns Undef.
#[test]
fn method_re_on_non_complex_undef() {
    let result = eval_method(
        scalar_val(5.0, DimensionVector::LENGTH),
        Type::length(),
        "re",
        Type::Real,
    );
    assert!(result.is_undef());
}

/// .magnitude(1) with unexpected args returns Undef.
#[test]
fn method_magnitude_with_args_undef() {
    let expr = CompiledExpr::method_call(
        lit(
            complex_val(3.0, 4.0, DimensionVector::DIMENSIONLESS),
            Type::complex(Type::Real),
        ),
        "magnitude".to_string(),
        vec![lit(Value::Int(1), Type::Int)],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(result.is_undef());
}

// ─── task-3950 step-1: Real/Int + Complex addition (dimensionless promotion) ─

/// Real(3.2) + Complex{0,4.1,DIMENSIONLESS} → Complex{3.2,4.1,DIMENSIONLESS}.
/// Promotes the Real scalar to a dimensionless Complex and sums.
#[test]
fn real_add_complex_dimensionless() {
    let result = eval_binop(
        BinOp::Add,
        Value::Real(3.2),
        Type::Real,
        complex_val(0.0, 4.1, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(3.2, 4.1, DimensionVector::DIMENSIONLESS));
}

/// Complex{0,4.1,DIMENSIONLESS} + Real(3.2) → Complex{3.2,4.1,DIMENSIONLESS} (commutative).
#[test]
fn complex_add_real_dimensionless() {
    let result = eval_binop(
        BinOp::Add,
        complex_val(0.0, 4.1, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Value::Real(3.2),
        Type::Real,
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(3.2, 4.1, DimensionVector::DIMENSIONLESS));
}

/// Int(3) + Complex{0,4,DIMENSIONLESS} → Complex{3,4,DIMENSIONLESS}.
#[test]
fn int_add_complex_dimensionless() {
    let result = eval_binop(
        BinOp::Add,
        Value::Int(3),
        Type::Int,
        complex_val(0.0, 4.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(3.0, 4.0, DimensionVector::DIMENSIONLESS));
}

/// Complex{0,4,DIMENSIONLESS} + Int(3) → Complex{3,4,DIMENSIONLESS} (commutative).
#[test]
fn complex_add_int_dimensionless() {
    let result = eval_binop(
        BinOp::Add,
        complex_val(0.0, 4.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Value::Int(3),
        Type::Int,
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(3.0, 4.0, DimensionVector::DIMENSIONLESS));
}

/// Dimensionless-only guard: Real(3.2) + Complex{1,2,LENGTH} → Undef.
/// The complex operand carries a LENGTH dimension; promotion is refused.
#[test]
fn real_add_dimensioned_complex_undef() {
    let result = eval_binop(
        BinOp::Add,
        Value::Real(3.2),
        Type::Real,
        complex_val(1.0, 2.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}

/// Regression guard: Scalar{0.005,LENGTH}(=5mm) + Complex{0,4.1,DIMENSIONLESS} → Undef.
/// Dimensioned Scalar does not match the new Real/Int arms; falls through to `_ => Undef`.
#[test]
fn scalar_add_complex_dimensionless_undef() {
    let result = eval_binop(
        BinOp::Add,
        scalar_val(0.005, DimensionVector::LENGTH),
        Type::length(),
        complex_val(0.0, 4.1, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    assert!(result.is_undef());
}

// ─── task-3950 step-3: Real/Int - Complex subtraction (non-commutative) ──────

/// Real(5.0) - Complex{1,2,DIMENSIONLESS} → Complex{4,-2,DIMENSIONLESS}.
/// re: 5-1=4; im: 0-2=-2 (negated imaginary part).
#[test]
fn real_sub_complex_dimensionless() {
    let result = eval_binop(
        BinOp::Sub,
        Value::Real(5.0),
        Type::Real,
        complex_val(1.0, 2.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(4.0, -2.0, DimensionVector::DIMENSIONLESS));
}

/// Complex{5,7,DIMENSIONLESS} - Real(2.0) → Complex{3,7,DIMENSIONLESS}.
/// re: 5-2=3; im: 7 (unchanged).
#[test]
fn complex_sub_real_dimensionless() {
    let result = eval_binop(
        BinOp::Sub,
        complex_val(5.0, 7.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Value::Real(2.0),
        Type::Real,
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(3.0, 7.0, DimensionVector::DIMENSIONLESS));
}

/// Int(5) - Complex{1,2,DIMENSIONLESS} → Complex{4,-2,DIMENSIONLESS}.
#[test]
fn int_sub_complex_dimensionless() {
    let result = eval_binop(
        BinOp::Sub,
        Value::Int(5),
        Type::Int,
        complex_val(1.0, 2.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(4.0, -2.0, DimensionVector::DIMENSIONLESS));
}

/// Complex{5,7,DIMENSIONLESS} - Int(2) → Complex{3,7,DIMENSIONLESS}.
#[test]
fn complex_sub_int_dimensionless() {
    let result = eval_binop(
        BinOp::Sub,
        complex_val(5.0, 7.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Value::Int(2),
        Type::Int,
        Type::complex(Type::Real),
    );
    assert_eq!(result, complex_val(3.0, 7.0, DimensionVector::DIMENSIONLESS));
}

/// Dimensionless-only guard: Real(5.0) - Complex{1,2,LENGTH} → Undef.
#[test]
fn real_sub_dimensioned_complex_undef() {
    let result = eval_binop(
        BinOp::Sub,
        Value::Real(5.0),
        Type::Real,
        complex_val(1.0, 2.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}

/// Regression guard: Scalar{0.005,LENGTH}(=5mm) - Complex{0,4.1,DIMENSIONLESS} → Undef.
/// Dimensioned Scalar does not match the new Real/Int arms; falls through to `_ => Undef`.
#[test]
fn scalar_sub_complex_dimensionless_undef() {
    let result = eval_binop(
        BinOp::Sub,
        scalar_val(0.005, DimensionVector::LENGTH),
        Type::length(),
        complex_val(0.0, 4.1, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        Type::complex(Type::Real),
    );
    assert!(result.is_undef());
}

/// Undef + Complex returns Undef (propagation through eval_binop).
#[test]
fn complex_undef_propagation() {
    let result = eval_binop(
        BinOp::Add,
        Value::Undef,
        Type::Real,
        complex_val(1.0, 2.0, DimensionVector::LENGTH),
        Type::complex(Type::length()),
        Type::complex(Type::length()),
    );
    assert!(result.is_undef());
}

// ─── Zero-complex edge cases ───────────��──────────────────────────────────

/// .magnitude() on zero complex returns Real(0.0) — zero vector has zero length.
#[test]
fn method_magnitude_zero_complex_returns_zero() {
    let result = eval_method(
        complex_val(0.0, 0.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        "magnitude",
        Type::Real,
    );
    assert_eq!(result, Value::Real(0.0));
}

/// .phase() on zero complex returns Undef (zero vector has no direction).
#[test]
fn method_phase_zero_complex_returns_undef() {
    let result = eval_method(
        complex_val(0.0, 0.0, DimensionVector::DIMENSIONLESS),
        Type::complex(Type::Real),
        "phase",
        Type::Real,
    );
    assert!(
        result.is_undef(),
        "phase(0+0i) should be Undef, got {:?}",
        result
    );
}
