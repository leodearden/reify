//! Compile-typing integration tests for the value-level `^` operator (task 3805).
//!
//! Tests the BinOp compile site in `expr.rs` for correct result_type inference:
//!
//! - `Scalar<Q> ^ n → Scalar<Q^n>` (PRD §4.3)
//! - `Int ^ Int → Int` and `Real ^ Real → Real` (via existing `left.clone()` path)
//!
//! # RED / GREEN structure
//!
//! **Happy-path tests (step-4 / step-5):**
//! - `5mm ^ 2`: RED in step-4 because `infer_binop_type(Pow, Scalar{LENGTH}, Int)`
//!   returns `left.clone() = Scalar{LENGTH}`, not `Scalar{AREA}`.
//!   GREEN after step-5 adds the Scalar dimension-scaling branch.
//! - `5mm ^ -2`: Same path — RED typing as `LENGTH`, should be `LENGTH^-2`.
//! - `2 ^ 3 → Int` and `2.0 ^ 3.0 → Real`: Already GREEN via `left.clone()`.
//!
//! **Error-path tests (step-6 / step-7):** see the bottom of this file.
//!
//! Note: `mm` is a built-in unit (hardcoded in `units.rs::unit_to_scalar`), so
//! `compile_source` (no stdlib) correctly resolves `5mm` to `Scalar{LENGTH, 0.005}`.

mod common;

use common::expect_binop;
use reify_test_support::{compile_source, errors_only};
use reify_core::{DimensionVector, Type};
use reify_ir::BinOp;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Compile `structure def S { let p = <expr> }` and return the default_expr of
/// the `p` cell.  Panics if there are compilation errors or if the cell is absent.
fn compile_let_expr(expr: &str) -> reify_ir::CompiledExpr {
    let source = format!("structure def S {{ let p = {expr} }}");
    let module = compile_source(&source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "unexpected errors for `{expr}`: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .unwrap_or_else(|| panic!("template S not found for `{expr}`"));
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .unwrap_or_else(|| panic!("cell p not found for `{expr}`"));
    cell.default_expr
        .clone()
        .unwrap_or_else(|| panic!("cell p has no default_expr for `{expr}`"))
}

// ── Happy-path tests ──────────────────────────────────────────────────────────

/// `5mm ^ 2` must compile with op `BinOp::Pow` and `result_type = Scalar{AREA}`.
///
/// RED (step-4): `infer_binop_type(Pow, Scalar{LENGTH}, Int)` returns
/// `left.clone() = Scalar{LENGTH}` — the dimension is not scaled.
/// GREEN (step-5): the Scalar branch scales `LENGTH.pow(2) = AREA`.
#[test]
fn pow_5mm_2_result_type_is_area() {
    let expr = compile_let_expr("5mm ^ 2");
    assert_eq!(
        expr.result_type,
        Type::Scalar { dimension: DimensionVector::AREA },
        "5mm ^ 2 result_type should be Scalar{{AREA}}, got {:?}",
        expr.result_type
    );
    let (op, _, _) = expect_binop(&expr);
    assert_eq!(*op, BinOp::Pow, "op should be BinOp::Pow");
}

/// `5mm ^ -2` must compile with op `BinOp::Pow` and
/// `result_type = Scalar{LENGTH^-2}`.
///
/// The negative exponent is represented in the AST as
/// `ExprKind::UnOp{op:"-", operand: NumberLiteral{2.0, false}}`
/// because `^` binds tighter than unary `-`, so `5mm ^ -2 = 5mm ^ (-2)`.
///
/// RED (step-4): result_type is `Scalar{LENGTH}` (unchanged).
/// GREEN (step-5): result_type is `Scalar{LENGTH.pow(-2)}`.
#[test]
fn pow_5mm_neg2_result_type_is_inv_length_sq() {
    let expr = compile_let_expr("5mm ^ -2");
    let expected = Type::Scalar {
        dimension: DimensionVector::LENGTH.pow(-2),
    };
    assert_eq!(
        expr.result_type, expected,
        "5mm ^ -2 result_type should be Scalar{{LENGTH^-2}}, got {:?}",
        expr.result_type
    );
    let (op, _, _) = expect_binop(&expr);
    assert_eq!(*op, BinOp::Pow, "op should be BinOp::Pow");
}

/// `2 ^ 3` must compile with op `BinOp::Pow` and `result_type = Int`.
///
/// The existing `infer_binop_type(Pow, Int, Int) = left.clone() = Int` already
/// returns the correct type — this test is GREEN on arrival and serves as a
/// regression guard for the dimensionless Int path.
#[test]
fn pow_int_int_result_type_is_int() {
    let expr = compile_let_expr("2 ^ 3");
    assert_eq!(
        expr.result_type,
        Type::Int,
        "2 ^ 3 result_type should be Int, got {:?}",
        expr.result_type
    );
    let (op, _, _) = expect_binop(&expr);
    assert_eq!(*op, BinOp::Pow, "op should be BinOp::Pow");
}

/// `2.0 ^ 3.0` must compile with op `BinOp::Pow` and `result_type = Real`.
///
/// The existing `infer_binop_type(Pow, Real, Real) = left.clone() = Real` already
/// returns the correct type — this test is GREEN on arrival.
#[test]
fn pow_real_real_result_type_is_real() {
    let expr = compile_let_expr("2.0 ^ 3.0");
    assert_eq!(
        expr.result_type,
        Type::Real,
        "2.0 ^ 3.0 result_type should be Real, got {:?}",
        expr.result_type
    );
    let (op, _, _) = expect_binop(&expr);
    assert_eq!(*op, BinOp::Pow, "op should be BinOp::Pow");
}

// ── Error-path tests ──────────────────────────────────────────────────────────
//
// These tests are RED in step-6 for two reasons:
//   1. `DiagnosticCode::NonIntegerExponentOnDimensioned` does not yet exist
//      (causes a compile-time error in this test file).
//   2. No such diagnostic is emitted by the compiler yet.
// GREEN after step-7 adds the variant to `DiagnosticCode` and the error emission
// in the Pow+Scalar branch of `expr.rs`.

use reify_core::DiagnosticCode;

/// Helper: compile `structure def S { param x : Int = 2  let p = <expr> }` and
/// return all Error-severity diagnostics.  Used for error-path tests where we
/// expect compilation to fail with a specific diagnostic code.
fn compile_let_expr_errors(expr: &str) -> Vec<reify_core::Diagnostic> {
    use reify_test_support::compile_source;
    let source = format!("structure def S {{ param x : Int = 2  let p = {expr} }}");
    let module = compile_source(&source);
    module
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect()
}

/// `5mm ^ 1.5` must produce `DiagnosticCode::NonIntegerExponentOnDimensioned`.
///
/// A real-literal exponent (is_real:true) is rejected on a dimensioned base
/// because PRD §4.3 requires an integer LITERAL for `Scalar<Q>^n → Scalar<Q^n>`.
///
/// RED (step-6): the variant does not yet exist; the compiler emits no diagnostic.
/// GREEN (step-7): variant added; error emitted in the Pow+Scalar None branch.
#[test]
fn pow_dimensioned_real_exp_flagged() {
    let errors = compile_let_expr_errors("5mm ^ 1.5");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::NonIntegerExponentOnDimensioned));
    assert!(
        flagged,
        "5mm ^ 1.5 should produce DiagnosticCode::NonIntegerExponentOnDimensioned, \
         got errors: {:?}",
        errors
    );
}

/// `5mm ^ 2.0` must produce `DiagnosticCode::NonIntegerExponentOnDimensioned`.
///
/// A real-literal exponent with an integer value (2.0, is_real:true) is still
/// rejected — PRD §4.3 requires an integer LITERAL; the author must write `5mm ^ 2`.
///
/// RED (step-6): the variant does not yet exist; the compiler emits no diagnostic.
/// GREEN (step-7): same path as the 1.5 case.
#[test]
fn pow_dimensioned_real_int_valued_exp_flagged() {
    let errors = compile_let_expr_errors("5mm ^ 2.0");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::NonIntegerExponentOnDimensioned));
    assert!(
        flagged,
        "5mm ^ 2.0 should produce DiagnosticCode::NonIntegerExponentOnDimensioned, \
         got errors: {:?}",
        errors
    );
}

/// `5mm ^ x` (identifier exponent) must produce `DiagnosticCode::NonIntegerExponentOnDimensioned`.
///
/// A non-literal exponent (here: a parameter reference) is rejected on a dimensioned
/// base — the dimension-scaling `Q^n` must be computed at compile time, so only
/// integer-literal exponents are allowed (PRD §4.3).
///
/// RED (step-6): the variant does not yet exist; the compiler emits no diagnostic.
/// GREEN (step-7): same path — `x` is not a NumberLiteral or UnOp{"-", NumberLiteral},
/// so the None arm fires.
#[test]
fn pow_dimensioned_ident_exp_flagged() {
    let errors = compile_let_expr_errors("5mm ^ x");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::NonIntegerExponentOnDimensioned));
    assert!(
        flagged,
        "5mm ^ x should produce DiagnosticCode::NonIntegerExponentOnDimensioned, \
         got errors: {:?}",
        errors
    );
}

/// `2.0 ^ 1.5` (dimensionless base) must NOT be flagged with
/// `DiagnosticCode::NonIntegerExponentOnDimensioned`.
///
/// The constraint only applies to dimensioned (`Scalar`) bases; a dimensionless
/// `Real ^ Real` raises no dimensional error and compiles cleanly.
///
/// GREEN on arrival (no change needed): the Pow+Scalar branch only fires when
/// `compiled_left.result_type` is `Type::Scalar{..}`.
#[test]
fn pow_dimensionless_real_exp_not_flagged() {
    let errors = compile_let_expr_errors("2.0 ^ 1.5");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::NonIntegerExponentOnDimensioned));
    assert!(
        !flagged,
        "2.0 ^ 1.5 should NOT produce DiagnosticCode::NonIntegerExponentOnDimensioned, \
         got errors: {:?}",
        errors
    );
}

// ── ExponentOutOfRange error-path tests (task 4106) ──────────────────────────
//
// These tests are RED in step-1 for two reasons:
//   1. `DiagnosticCode::ExponentOutOfRange` does not yet exist
//      (causes a compile-time error in this test file).
//   2. Even once it exists, no such diagnostic is emitted yet (step-2 adds that).
// GREEN after step-2 adds the variant and the guarded emission in expr.rs.

/// `5mm ^ 256` must produce `DiagnosticCode::ExponentOutOfRange`.
///
/// 256 overflows i8 (max 127), so the value-level `^` compile site must emit
/// this diagnostic and poison the result type.  The unit-literal path already
/// does this via `i8::try_from` at units.rs:680-681; this brings value-level
/// `^` into conformance.
///
/// RED (step-1): `ExponentOutOfRange` does not exist yet, then no diagnostic is
/// emitted (lossy `as i8` truncation silently wraps 256 → 0).
/// GREEN (step-2): variant added; `i8::try_from(n)` Err → diagnostic emitted.
#[test]
fn pow_dimensioned_exp_overflow_positive_flagged() {
    let errors = compile_let_expr_errors("5mm ^ 256");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ExponentOutOfRange));
    assert!(
        flagged,
        "5mm ^ 256 should produce DiagnosticCode::ExponentOutOfRange \
         (256 overflows i8), got errors: {:?}",
        errors
    );
}

/// `5mm ^ -200` must produce `DiagnosticCode::ExponentOutOfRange`.
///
/// -200 is below i8::MIN (-128), so the value-level `^` compile site must emit
/// this diagnostic.
///
/// RED (step-1): `ExponentOutOfRange` does not exist yet, then no diagnostic is
/// emitted (lossy `as i8` truncation silently wraps -200 → 56).
/// GREEN (step-2): same Err branch as the positive-overflow case.
#[test]
fn pow_dimensioned_exp_overflow_negative_flagged() {
    let errors = compile_let_expr_errors("5mm ^ -200");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ExponentOutOfRange));
    assert!(
        flagged,
        "5mm ^ -200 should produce DiagnosticCode::ExponentOutOfRange \
         (-200 underflows i8), got errors: {:?}",
        errors
    );
}

/// `5mm ^ 127` must NOT produce `DiagnosticCode::ExponentOutOfRange`.
///
/// 127 is i8::MAX — the largest in-range exponent.  This is a boundary
/// regression-guard: the check must accept 127 and scale the dimension
/// correctly to `Scalar{LENGTH^127}`.
///
/// GREEN on arrival (once step-2 lands): the `i8::try_from(127)` Ok branch
/// keeps the existing scaled-dimension logic untouched.
#[test]
fn pow_dimensioned_exp_127_not_flagged() {
    let errors = compile_let_expr_errors("5mm ^ 127");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ExponentOutOfRange));
    assert!(
        !flagged,
        "5mm ^ 127 should NOT produce DiagnosticCode::ExponentOutOfRange \
         (127 is i8::MAX, in range), got errors: {:?}",
        errors
    );
}

/// `5mm ^ -128` must NOT produce `DiagnosticCode::ExponentOutOfRange`.
///
/// -128 is i8::MIN — the most-negative in-range exponent.  This is the
/// symmetric lower-boundary regression-guard mirroring the 127 (i8::MAX)
/// upper-boundary test: `i8::try_from(-128)` succeeds, so no diagnostic
/// should be emitted.
#[test]
fn pow_dimensioned_exp_neg128_not_flagged() {
    let errors = compile_let_expr_errors("5mm ^ -128");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ExponentOutOfRange));
    assert!(
        !flagged,
        "5mm ^ -128 should NOT produce DiagnosticCode::ExponentOutOfRange \
         (-128 is i8::MIN, in range), got errors: {:?}",
        errors
    );
}

/// `5mm ^ -129` must produce `DiagnosticCode::ExponentOutOfRange`.
///
/// -129 is one below i8::MIN (-128) — the just-over-lower-boundary case.
/// `i8::try_from(-129)` fails, so the compile guard must emit the diagnostic,
/// symmetric to the 256 (one above i8::MAX) positive-overflow test.
#[test]
fn pow_dimensioned_exp_neg129_flagged() {
    let errors = compile_let_expr_errors("5mm ^ -129");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ExponentOutOfRange));
    assert!(
        flagged,
        "5mm ^ -129 should produce DiagnosticCode::ExponentOutOfRange \
         (-129 underflows i8::MIN), got errors: {:?}",
        errors
    );
}
