//! Tests for the `implicitly_converts_to` and `type_compatible` functions.
//!
//! These tests verify all four implicit conversion rules directionally:
//!   1. Vector<N,Q> <-> Tensor<1,N,Q>  (bidirectional)
//!   2. Scalar<Q> <-> Tensor<0,N,Q>    (bidirectional, N ignored)
//!   3. Tensor<2,N,Q> -> Matrix<N,N,Q>  (one-way: Tensor2 -> square Matrix)
//!   4. Matrix -> Tensor                (NOT implicit)

use reify_compiler::{implicitly_converts_to, type_compatible};
use reify_core::Type;

// ── Rule 1: Vector<N,Q> <-> Tensor<1,N,Q> (bidirectional) ──────────────────

/// (a) Vector<3,Real> -> Tensor<1,3,Real> is allowed.
#[test]
fn vector_to_tensor1_same_type() {
    let from = Type::vec3(Type::Real);
    let to = Type::tensor(1, 3, Type::Real);
    assert!(
        implicitly_converts_to(&from, &to),
        "Vector<3,Real> should convert to Tensor<1,3,Real>"
    );
}

/// (b) Tensor<1,3,Real> -> Vector<3,Real> is allowed (reverse direction).
#[test]
fn tensor1_to_vector_same_type() {
    let from = Type::tensor(1, 3, Type::Real);
    let to = Type::vec3(Type::Real);
    assert!(
        implicitly_converts_to(&from, &to),
        "Tensor<1,3,Real> should convert to Vector<3,Real>"
    );
}

/// (c) Vector<3,Scalar[m]> -> Tensor<1,3,Scalar[m]> — with a Scalar quantity type.
#[test]
fn vector_to_tensor1_scalar_quantity() {
    let from = Type::vec3(Type::length());
    let to = Type::tensor(1, 3, Type::length());
    assert!(
        implicitly_converts_to(&from, &to),
        "Vector<3,Scalar[m]> should convert to Tensor<1,3,Scalar[m]>"
    );
}

/// (d) Vector<2,Real> -> Tensor<1,3,Real> is NOT allowed — N mismatch.
#[test]
fn vector_to_tensor1_n_mismatch() {
    let from = Type::vec2(Type::Real);
    let to = Type::tensor(1, 3, Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Vector<2,Real> should NOT convert to Tensor<1,3,Real> (N mismatch)"
    );
}

/// (e) Vector<3,Real> -> Tensor<1,3,Scalar[m]> is NOT allowed — quantity mismatch.
#[test]
fn vector_to_tensor1_quantity_mismatch() {
    let from = Type::vec3(Type::Real);
    let to = Type::tensor(1, 3, Type::length());
    assert!(
        !implicitly_converts_to(&from, &to),
        "Vector<3,Real> should NOT convert to Tensor<1,3,Scalar[m]> (quantity mismatch)"
    );
}

// ── Rule 2: Scalar<Q> <-> Tensor<0,N,Q> (bidirectional, N ignored) ─────────

/// (a) Scalar[m] -> Tensor<0,3,Scalar[m]> is allowed.
#[test]
fn scalar_to_tensor0() {
    let from = Type::length();
    let to = Type::tensor(0, 3, Type::length());
    assert!(
        implicitly_converts_to(&from, &to),
        "Scalar[m] should convert to Tensor<0,3,Scalar[m]>"
    );
}

/// (b) Tensor<0,3,Scalar[m]> -> Scalar[m] is allowed.
#[test]
fn tensor0_to_scalar() {
    let from = Type::tensor(0, 3, Type::length());
    let to = Type::length();
    assert!(
        implicitly_converts_to(&from, &to),
        "Tensor<0,3,Scalar[m]> should convert to Scalar[m]"
    );
}

/// (c) Scalar[angle] -> Tensor<0,5,Scalar[angle]> is allowed — different N is fine for rank-0.
#[test]
fn scalar_to_tensor0_different_n() {
    let from = Type::angle();
    let to = Type::tensor(0, 5, Type::angle());
    assert!(
        implicitly_converts_to(&from, &to),
        "Scalar[angle] should convert to Tensor<0,5,Scalar[angle]> (N ignored for rank-0)"
    );
}

/// (d) Scalar[m] -> Tensor<0,3,Scalar[angle]> is NOT allowed — dimension mismatch.
#[test]
fn scalar_to_tensor0_dimension_mismatch() {
    let from = Type::length();
    let to = Type::tensor(0, 3, Type::angle());
    assert!(
        !implicitly_converts_to(&from, &to),
        "Scalar[m] should NOT convert to Tensor<0,3,Scalar[angle]>"
    );
}

/// (e) Type::Real -> Tensor<0,3,Real> is allowed — dimensionless scalar-like.
#[test]
fn real_to_tensor0() {
    let from = Type::Real;
    let to = Type::tensor(0, 3, Type::Real);
    assert!(
        implicitly_converts_to(&from, &to),
        "Real should convert to Tensor<0,3,Real>"
    );
}

/// (f) Tensor<0,2,Real> -> Type::Real is allowed.
#[test]
fn tensor0_to_real() {
    let from = Type::tensor(0, 2, Type::Real);
    let to = Type::Real;
    assert!(
        implicitly_converts_to(&from, &to),
        "Tensor<0,2,Real> should convert to Real"
    );
}

// ── Rule 3: Tensor<2,N,Q> -> Matrix<N,N,Q> (one-way only) ──────────────────

/// (a) Tensor<2,3,Real> -> Matrix<3,3,Real> is allowed (square, matching N).
#[test]
fn tensor2_to_square_matrix_real() {
    let from = Type::tensor(2, 3, Type::Real);
    let to = Type::matrix(3, 3, Type::Real);
    assert!(
        implicitly_converts_to(&from, &to),
        "Tensor<2,3,Real> should convert to Matrix<3,3,Real>"
    );
}

/// (b) Tensor<2,3,Scalar[m]> -> Matrix<3,3,Scalar[m]> is allowed.
#[test]
fn tensor2_to_square_matrix_length() {
    let from = Type::tensor(2, 3, Type::length());
    let to = Type::matrix(3, 3, Type::length());
    assert!(
        implicitly_converts_to(&from, &to),
        "Tensor<2,3,Scalar[m]> should convert to Matrix<3,3,Scalar[m]>"
    );
}

/// (c) Matrix<3,3,Real> -> Tensor<2,3,Real> is NOT allowed (Matrix->Tensor is rejected).
#[test]
fn matrix_to_tensor2_rejected() {
    let from = Type::matrix(3, 3, Type::Real);
    let to = Type::tensor(2, 3, Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Matrix<3,3,Real> should NOT convert to Tensor<2,3,Real> (one-way rule)"
    );
}

/// (d) Tensor<2,3,Real> -> Matrix<3,4,Real> is NOT allowed (non-square matrix).
#[test]
fn tensor2_to_non_square_matrix_rejected() {
    let from = Type::tensor(2, 3, Type::Real);
    let to = Type::matrix(3, 4, Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<2,3,Real> should NOT convert to Matrix<3,4,Real> (non-square)"
    );
}

/// (e) Tensor<2,3,Real> -> Matrix<4,4,Real> is NOT allowed (N mismatch).
#[test]
fn tensor2_to_matrix_n_mismatch() {
    let from = Type::tensor(2, 3, Type::Real);
    let to = Type::matrix(4, 4, Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<2,3,Real> should NOT convert to Matrix<4,4,Real> (N mismatch)"
    );
}

/// (f) Tensor<1,3,Real> -> Matrix<3,3,Real> is NOT allowed (wrong rank, rank-1 not rank-2).
#[test]
fn tensor1_to_matrix_rejected() {
    let from = Type::tensor(1, 3, Type::Real);
    let to = Type::matrix(3, 3, Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<1,3,Real> should NOT convert to Matrix<3,3,Real> (rank-1, not rank-2)"
    );
}

// ── Edge cases and negative tests ──────────────────────────────────────────

/// (a) Identity: implicitly_converts_to(Real, Real) == true.
#[test]
fn identity_real() {
    assert!(
        implicitly_converts_to(&Type::Real, &Type::Real),
        "Real -> Real should always be true (identity)"
    );
}

/// (b) Int->Real widening is NOT handled by implicitly_converts_to (it's a separate concern).
#[test]
fn int_to_real_not_an_implicit_conversion() {
    assert!(
        !implicitly_converts_to(&Type::Int, &Type::Real),
        "Int -> Real is NOT an implicit tensor conversion"
    );
}

/// (c) Point<3,Real> -> Tensor<1,3,Real> is NOT allowed — Point is not Vector.
#[test]
fn point_to_tensor1_rejected() {
    let from = Type::Point {
        n: 3,
        quantity: Box::new(Type::Real),
    };
    let to = Type::tensor(1, 3, Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Point<3,Real> should NOT convert to Tensor<1,3,Real>"
    );
}

/// (d) Vector<3,Real> -> Matrix<3,3,Real> is NOT allowed (no Vector->Matrix shortcut).
#[test]
fn vector_to_matrix_rejected() {
    let from = Type::vec3(Type::Real);
    let to = Type::matrix(3, 3, Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Vector<3,Real> should NOT directly convert to Matrix<3,3,Real>"
    );
}

/// (e) Tensor<3,2,Real> -> anything other than itself is NOT allowed.
#[test]
fn tensor_rank3_to_vector_rejected() {
    let from = Type::tensor(3, 2, Type::Real);
    let to = Type::vec2(Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<3,2,Real> should NOT convert to Vector"
    );
}

// ── Rule 2c: Tensor<0,M,Q> <-> Tensor<0,N,Q> (same Q, different N, rank-0) ──
//
// Spec: N is irrelevant for rank-0. By transitivity (Q → Tensor<0,M,Q> and
// Q → Tensor<0,N,Q> both work via Rule 2a), direct Tensor<0,M,Q> ↔ Tensor<0,N,Q>
// conversion must also be supported. Without Rule 2c, a trait requiring
// Tensor<0,5,Real> would reject a structure providing Tensor<0,3,Real> despite
// them being semantically identical. Covers suggestions #14 and #18.

/// Tensor<0,3,Real> -> Tensor<0,5,Real> should be allowed (rule 2c: same Q, any N).
#[test]
fn tensor0_different_n_same_quantity_convertible_forward() {
    let from = Type::tensor(0, 3, Type::Real);
    let to = Type::tensor(0, 5, Type::Real);
    assert!(
        implicitly_converts_to(&from, &to),
        "Tensor<0,3,Real> should convert to Tensor<0,5,Real> (N irrelevant for rank-0)"
    );
}

/// Tensor<0,5,Real> -> Tensor<0,3,Real> should be allowed (rule 2c: symmetric, any N).
#[test]
fn tensor0_different_n_same_quantity_convertible_reverse() {
    let from = Type::tensor(0, 5, Type::Real);
    let to = Type::tensor(0, 3, Type::Real);
    assert!(
        implicitly_converts_to(&from, &to),
        "Tensor<0,5,Real> should convert to Tensor<0,3,Real> (N irrelevant for rank-0)"
    );
}

/// (f) Tensor<0,3,Real> -> Tensor<1,3,Real> is NOT allowed (different ranks, no rule covers this).
#[test]
fn tensor0_to_tensor1_rejected() {
    let from = Type::tensor(0, 3, Type::Real);
    let to = Type::tensor(1, 3, Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<0,3,Real> should NOT convert to Tensor<1,3,Real>"
    );
}

/// Dimensioned-scalar variant of Rule 2c: Tensor<0,3,Scalar[m]> -> Tensor<0,5,Scalar[m]>
/// must be allowed (same dimensioned quantity, different N, rank-0).
#[test]
fn tensor0_different_n_scalar_quantity_convertible() {
    let from = Type::tensor(0, 3, Type::length());
    let to = Type::tensor(0, 5, Type::length());
    assert!(
        implicitly_converts_to(&from, &to),
        "Tensor<0,3,Scalar[m]> should convert to Tensor<0,5,Scalar[m]> (same dimensioned quantity, N ignored for rank-0)"
    );
}

/// Regression-pin for the q1 == q2 guard in Rule 2c: Tensor<0,3,Scalar[m]> must NOT
/// convert to Tensor<0,5,Scalar[angle]> (quantity mismatch).
/// Without this case, a future refactor that dropped the equality guard would still pass
/// the Real-only tests above (Real == Real is trivially true).
#[test]
fn tensor0_different_n_scalar_quantity_mismatch_rejected() {
    let from = Type::tensor(0, 3, Type::length());
    let to = Type::tensor(0, 5, Type::angle());
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<0,3,Scalar[m]> must NOT convert to Tensor<0,5,Scalar[angle]> (quantity mismatch — Rule 2c requires q1 == q2)"
    );
}

// ── Rule 2c compound-Q guard tests ────────────────────────────────────────
//
// Rule 2c's transitivity rationale ("if Q→Tensor<0,M,Q> and Q→Tensor<0,N,Q>
// both hold, then Tensor<0,M,Q>→Tensor<0,N,Q> must also hold") only applies
// when Rules 2a/2b themselves fire — i.e. when Q is a scalar-like leaf.
// Compound Q types (Vector, Tensor, Matrix, Point) must be rejected by Rule 2c
// for consistency with Rules 2a/2b. Covers review suggestions from task-395.

/// Rule 2c must NOT fire when Q is a compound Vector type (different N).
/// Tensor<0,3,Vector<3,Real>> -> Tensor<0,5,Vector<3,Real>> must be rejected:
/// transitivity from Rules 2a/2b fails because 2a/2b reject compound Q,
/// so Rule 2c inheriting their transitivity must also require leaf-Q.
#[test]
fn rule_2c_rejects_compound_q_vector_different_n() {
    let q = Type::vec3(Type::Real);
    let from = Type::tensor(0, 3, q.clone());
    let to = Type::tensor(0, 5, q);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<0,3,Vector<3,Real>> -> Tensor<0,5,Vector<3,Real>> must be rejected (compound Q — Rule 2c requires leaf-Q)"
    );
}

/// Rule 2c must NOT fire when Q is a compound Tensor<2> type (different N).
/// Tensor<0,3,Tensor<2,3,Real>> -> Tensor<0,5,Tensor<2,3,Real>> must be rejected.
/// Parallels `rule_2a_rejects_compound_from_tensor2`.
#[test]
fn rule_2c_rejects_compound_q_tensor_different_n() {
    let q = Type::tensor(2, 3, Type::Real);
    let from = Type::tensor(0, 3, q.clone());
    let to = Type::tensor(0, 5, q);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<0,3,Tensor<2,3,Real>> -> Tensor<0,5,Tensor<2,3,Real>> must be rejected (compound Q — Rule 2c requires leaf-Q)"
    );
}

/// Rule 2c must NOT fire when Q is a compound Point type (different N).
/// Tensor<0,3,Point<3,Real>> -> Tensor<0,5,Point<3,Real>> must be rejected.
/// Parallels `rule_2a_rejects_compound_from_point`.
#[test]
fn rule_2c_rejects_compound_q_point_different_n() {
    let q = Type::point3(Type::Real);
    let from = Type::tensor(0, 3, q.clone());
    let to = Type::tensor(0, 5, q);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<0,3,Point<3,Real>> -> Tensor<0,5,Point<3,Real>> must be rejected (compound Q — Rule 2c requires leaf-Q)"
    );
}

// ── Rules 2a/2b compound-type guard tests ─────────────────────────────────
//
// Rules 2a/2b use wildcard arms (`from_ty` / `to_ty`). Without a guard, any
// compound type (Vector, Tensor, Matrix) can match the wildcard and produce a
// spurious true. These tests pin the required rejection: a compound type must
// never serve as the "Q" side of a rank-0 tensor conversion rule. Covers
// review suggestions #2, #7, #17.

/// Rule 2a must NOT fire when `from_ty` is a compound Vector type.
/// Vector<3,Real> -> Tensor<0,3,Vector<3,Real>> should be rejected.
#[test]
fn rule_2a_rejects_compound_from_vector() {
    let from = Type::vec3(Type::Real);
    let to = Type::tensor(0, 3, Type::vec3(Type::Real));
    assert!(
        !implicitly_converts_to(&from, &to),
        "Vector<3,Real> -> Tensor<0,3,Vector<3,Real>> must be rejected (compound from_ty)"
    );
}

/// Rule 2b must NOT fire when `to_ty` is a compound Vector type.
/// Tensor<0,3,Vector<3,Real>> -> Vector<3,Real> should be rejected.
#[test]
fn rule_2b_rejects_compound_to_vector() {
    let from = Type::tensor(0, 3, Type::vec3(Type::Real));
    let to = Type::vec3(Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<0,3,Vector<3,Real>> -> Vector<3,Real> must be rejected (compound to_ty)"
    );
}

/// Rule 2a must NOT fire when `from_ty` is a compound Tensor<2> type.
/// Tensor<2,3,Real> -> Tensor<0,3,Tensor<2,3,Real>> should be rejected —
/// ensuring Rule 3's Tensor<2>->Matrix asymmetry isn't accidentally subverted.
#[test]
fn rule_2a_rejects_compound_from_tensor2() {
    let from = Type::tensor(2, 3, Type::Real);
    let to = Type::tensor(0, 3, Type::tensor(2, 3, Type::Real));
    assert!(
        !implicitly_converts_to(&from, &to),
        "Tensor<2,3,Real> -> Tensor<0,3,Tensor<2,3,Real>> must be rejected (compound from_ty)"
    );
}

/// Rule 2a must NOT fire when `from_ty` is a compound Point type.
/// Point<3,Real> -> Tensor<0,3,Point<3,Real>> should be rejected — Point is an
/// aggregate, not a scalar-like leaf, so it must not serve as the "Q" side.
/// Covers reviewer suggestion #1 (robustness of the compound-type guard).
#[test]
fn rule_2a_rejects_compound_from_point() {
    let from = Type::point3(Type::Real);
    let to = Type::tensor(0, 3, Type::point3(Type::Real));
    assert!(
        !implicitly_converts_to(&from, &to),
        "Point<3,Real> -> Tensor<0,3,Point<3,Real>> must be rejected (compound from_ty)"
    );
}

/// (g) Rule 2a: any Q -> Tensor<0,_,Q>. Bool -> Tensor<0,1,Bool> is allowed.
#[test]
fn bool_to_tensor0_same_quantity_allowed() {
    let from = Type::Bool;
    let to = Type::tensor(0, 1, Type::Bool);
    // Rule 2a: any Q -> Tensor<0,_,Q>. Since Bool == Bool quantity, this is allowed.
    assert!(
        implicitly_converts_to(&from, &to),
        "Bool -> Tensor<0,1,Bool> is allowed (any Q -> Tensor<0,_,Q>)"
    );
}

/// (g2) Bool -> Tensor<0,...,Real> is NOT allowed (type mismatch).
#[test]
fn bool_to_tensor0_real_rejected() {
    let from = Type::Bool;
    let to = Type::tensor(0, 1, Type::Real);
    assert!(
        !implicitly_converts_to(&from, &to),
        "Bool -> Tensor<0,1,Real> should NOT be allowed (Bool != Real)"
    );
}

// ── type_compatible() integration tests ────────────────────────────────────

/// (a) type_compatible(Tensor<1,3,Real>, Vector<3,Real>) == true (bidirectional).
#[test]
fn type_compatible_tensor1_vector_bidirectional_a() {
    let t = Type::tensor(1, 3, Type::Real);
    let v = Type::vec3(Type::Real);
    assert!(
        type_compatible(&t, &v),
        "type_compatible(Tensor<1,3,Real>, Vector<3,Real>) should be true"
    );
}

/// (b) type_compatible(Vector<3,Real>, Tensor<1,3,Real>) == true (other direction).
#[test]
fn type_compatible_tensor1_vector_bidirectional_b() {
    let v = Type::vec3(Type::Real);
    let t = Type::tensor(1, 3, Type::Real);
    assert!(
        type_compatible(&v, &t),
        "type_compatible(Vector<3,Real>, Tensor<1,3,Real>) should be true"
    );
}

/// (c) type_compatible(Real, Int) == true — existing Int->Real widening preserved.
#[test]
fn type_compatible_int_real_widening_preserved() {
    assert!(
        type_compatible(&Type::Real, &Type::Int),
        "type_compatible(Real, Int) should be true (Int->Real widening)"
    );
}

/// (c2) type_compatible(Int, Real) == false — Int->Real widening is one-way.
/// Real->Int narrowing is NOT allowed; only Int->Real widening fires (param=Real, arg=Int).
/// Covers suggestion #15: asymmetric widening characterization.
#[test]
fn type_compatible_int_real_widening_is_asymmetric() {
    assert!(
        !type_compatible(&Type::Int, &Type::Real),
        "type_compatible(Int, Real) should be false — Real->Int narrowing is not allowed"
    );
}

/// (d) type_compatible(Tensor<2,3,Real>, Matrix<3,3,Real>) == true.
#[test]
fn type_compatible_tensor2_matrix() {
    let t = Type::tensor(2, 3, Type::Real);
    let m = Type::matrix(3, 3, Type::Real);
    assert!(
        type_compatible(&t, &m),
        "type_compatible(Tensor<2,3,Real>, Matrix<3,3,Real>) should be true"
    );
}

/// (e) type_compatible(Matrix<3,3,Real>, Tensor<2,3,Real>) == true.
/// type_compatible is symmetric — checks both directions, so even though
/// Matrix->Tensor is not a direct implicit conversion, the reverse (Tensor->Matrix) is.
#[test]
fn type_compatible_matrix_tensor2_symmetric() {
    let m = Type::matrix(3, 3, Type::Real);
    let t = Type::tensor(2, 3, Type::Real);
    assert!(
        type_compatible(&m, &t),
        "type_compatible(Matrix<3,3,Real>, Tensor<2,3,Real>) should be true (symmetric check)"
    );
}

// ── type_compatible() identity regression guard (step-9 prerequisite) ─────
//
// These tests pin type_compatible(T, T) == true for representative types.
// type_compatible currently returns true for identical types via an explicit
// `if param_ty == arg_ty { return true; }` guard. Step 9 removes that guard
// as redundant (implicitly_converts_to already has its own identity check at
// type_compat.rs:29). These tests ensure the removal does not regress identity
// behavior for any representative type. Covers suggestions #5 and #20.

/// type_compatible(Real, Real) == true.
#[test]
fn type_compatible_identity_real() {
    assert!(
        type_compatible(&Type::Real, &Type::Real),
        "type_compatible(Real, Real) must be true (identity)"
    );
}

/// type_compatible(Int, Int) == true.
#[test]
fn type_compatible_identity_int() {
    assert!(
        type_compatible(&Type::Int, &Type::Int),
        "type_compatible(Int, Int) must be true (identity)"
    );
}

/// type_compatible(Vector<3,Real>, Vector<3,Real>) == true.
#[test]
fn type_compatible_identity_vector() {
    let t = Type::vec3(Type::Real);
    assert!(
        type_compatible(&t, &t),
        "type_compatible(Vector<3,Real>, Vector<3,Real>) must be true (identity)"
    );
}

/// type_compatible(Tensor<2,3,Real>, Tensor<2,3,Real>) == true.
#[test]
fn type_compatible_identity_tensor2() {
    let t = Type::tensor(2, 3, Type::Real);
    assert!(
        type_compatible(&t, &t),
        "type_compatible(Tensor<2,3,Real>, Tensor<2,3,Real>) must be true (identity)"
    );
}

/// type_compatible(Matrix<3,3,Real>, Matrix<3,3,Real>) == true.
#[test]
fn type_compatible_identity_matrix() {
    let t = Type::matrix(3, 3, Type::Real);
    assert!(
        type_compatible(&t, &t),
        "type_compatible(Matrix<3,3,Real>, Matrix<3,3,Real>) must be true (identity)"
    );
}

// ── Type::Error wildcard contract (task-1912) ──────────────────────────────
//
// Task 448 introduced a `Type::Error` poison-value sentinel and added
// anti-cascade early-return guards to `implicitly_converts_to` and
// `type_compatible`: when either operand `is_error()` the functions return
// `true` immediately, suppressing follow-on "type mismatch" diagnostics for
// an operand whose error was already reported at its producer site.
//
// These tests PIN that contract so that a future refactor cannot silently
// strip the `is_error()` early-returns without breaking compilation.

/// (task-1912 req-a) `implicitly_converts_to(Error, Real) == true`.
/// Anti-cascade guard: when `from` is the poison sentinel, suppress any
/// downstream type-mismatch diagnostic — the originating error is already
/// reported.
#[test]
fn error_wildcard_implicit_from_error_to_real() {
    assert!(
        implicitly_converts_to(&Type::Error, &Type::Real),
        "implicitly_converts_to(Error, Real) must be true (anti-cascade guard, task-1912)"
    );
}

/// `implicitly_converts_to(Error, Int) == true`.
#[test]
fn error_wildcard_implicit_error_to_int() {
    assert!(
        implicitly_converts_to(&Type::Error, &Type::Int),
        "implicitly_converts_to(Error, Int) must be true (anti-cascade guard, task-1912)"
    );
}

/// `implicitly_converts_to(Error, Bool) == true`.
#[test]
fn error_wildcard_implicit_error_to_bool() {
    assert!(
        implicitly_converts_to(&Type::Error, &Type::Bool),
        "implicitly_converts_to(Error, Bool) must be true (anti-cascade guard, task-1912)"
    );
}

/// `implicitly_converts_to(Error, String) == true`.
#[test]
fn error_wildcard_implicit_error_to_string() {
    assert!(
        implicitly_converts_to(&Type::Error, &Type::String),
        "implicitly_converts_to(Error, String) must be true (anti-cascade guard, task-1912)"
    );
}

/// `implicitly_converts_to(Error, List<Int>) == true` — compound type.
#[test]
fn error_wildcard_implicit_error_to_list() {
    let to = Type::List(Box::new(Type::Int));
    assert!(
        implicitly_converts_to(&Type::Error, &to),
        "implicitly_converts_to(Error, List<Int>) must be true (anti-cascade guard, task-1912)"
    );
}

/// `implicitly_converts_to(Error, Option<Real>) == true` — compound type.
#[test]
fn error_wildcard_implicit_error_to_option() {
    let to = Type::Option(Box::new(Type::Real));
    assert!(
        implicitly_converts_to(&Type::Error, &to),
        "implicitly_converts_to(Error, Option<Real>) must be true (anti-cascade guard, task-1912)"
    );
}

/// `implicitly_converts_to(Error, Scalar[m]) == true` — dimensioned scalar.
#[test]
fn error_wildcard_implicit_error_to_scalar() {
    assert!(
        implicitly_converts_to(&Type::Error, &Type::length()),
        "implicitly_converts_to(Error, Scalar[m]) must be true (anti-cascade guard, task-1912)"
    );
}

/// `implicitly_converts_to(Error, Vector<3,Real>) == true` — shape-carrying type.
#[test]
fn error_wildcard_implicit_error_to_vector() {
    let to = Type::vec3(Type::Real);
    assert!(
        implicitly_converts_to(&Type::Error, &to),
        "implicitly_converts_to(Error, Vector<3,Real>) must be true (anti-cascade guard, task-1912)"
    );
}

/// `implicitly_converts_to(Error, Tensor<2,3,Real>) == true` — shape-carrying type.
#[test]
fn error_wildcard_implicit_error_to_tensor() {
    let to = Type::tensor(2, 3, Type::Real);
    assert!(
        implicitly_converts_to(&Type::Error, &to),
        "implicitly_converts_to(Error, Tensor<2,3,Real>) must be true (anti-cascade guard, task-1912)"
    );
}

/// `implicitly_converts_to(Error, Matrix<3,3,Real>) == true` — shape-carrying type.
#[test]
fn error_wildcard_implicit_error_to_matrix() {
    let to = Type::matrix(3, 3, Type::Real);
    assert!(
        implicitly_converts_to(&Type::Error, &to),
        "implicitly_converts_to(Error, Matrix<3,3,Real>) must be true (anti-cascade guard, task-1912)"
    );
}

// ── Consumer-side Error contract: implicitly_converts_to (task-1918) ──────────
//
// Task 1918 tightens the error-wildcard contract: `Type::Error` must never appear
// on the consumer/target side (`to`) of `implicitly_converts_to` — declared
// annotations always resolve to a concrete type via `resolve_type_with_aliases`.
// A `debug_assert!` fires in debug builds to catch this bug class immediately.
// (The release short-circuit still returns `true` for cascade safety as a
// single-line belt-and-braces fallback; its behavior is mechanically obvious
// and does not require a test.)
//
// Two representative types (primitive Real + shape-carrying Scalar[m]) are
// sufficient because the guard is a single `!to.is_error()` check that is
// type-agnostic — it fires for every possible `from` type identically.  If
// anyone special-cased the short-circuit by type variant, the full producer-side
// suite (Real/Int/Bool/String/Scalar/List/Option/Vector/Tensor/Matrix, above)
// would catch the regression immediately.

/// Consumer-side contract (debug): `implicitly_converts_to(Real, Error)` panics.
/// The debug_assert fires because `to=Type::Error` is never legitimate.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "consumer/target side of implicitly_converts_to")]
fn error_wildcard_implicit_to_error_debug_panics_real() {
    let _ = implicitly_converts_to(&Type::Real, &Type::Error);
}

/// Consumer-side contract (debug): `implicitly_converts_to(Scalar[m], Error)` panics.
/// Shape-carrying type — the debug_assert fires regardless of the `from` type.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "consumer/target side of implicitly_converts_to")]
fn error_wildcard_implicit_to_error_debug_panics_scalar() {
    let _ = implicitly_converts_to(&Type::length(), &Type::Error);
}

// ── type_compatible() arg-side error-wildcard tests (task-1912 / task-1918) ──
//
// Task 1918 tightens the error-wildcard contract for `type_compatible`:
// `Type::Error` must never appear on the param/expected side (`param_ty`) —
// declared annotations always resolve to a concrete type. A debug_assert fires
// in debug builds to catch this bug class. (The release short-circuit still
// returns `true` for cascade safety as a single-line belt-and-braces fallback;
// its behavior is mechanically obvious and does not require a test.)
//
// Arg-side tests (`arg_ty=Error`, legitimate producer path) are KEPT below.

// ── Param-side Error contract (task-1918) ────────────────────────────────────
//
// Two representative arg types (primitive Real + compound List<Int>) are
// sufficient because the guard is a single `!param_ty.is_error()` check that is
// type-agnostic — it fires for every possible `arg_ty` value identically.  The
// arg-side producer tests below apply the same rationale in reverse: a single
// table-driven test covers all representative `param_ty` variants because the
// `arg_ty.is_error()` guard is likewise type-agnostic w.r.t. `param_ty`.

/// Param-side contract (debug): `type_compatible(Error, Real)` panics.
/// The debug_assert fires because `param_ty=Type::Error` is never legitimate.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "param/expected side of type_compatible")]
fn type_compatible_param_error_debug_panics_real() {
    let _ = type_compatible(&Type::Error, &Type::Real);
}

/// Param-side contract (debug): `type_compatible(Error, List<Int>)` panics.
/// Compound arg type — the debug_assert fires regardless of `arg_ty`.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "param/expected side of type_compatible")]
fn type_compatible_param_error_debug_panics_list() {
    let _ = type_compatible(&Type::Error, &Type::List(Box::new(Type::Int)));
}

// ── Arg-side error-wildcard tests (producer path, task-1912) ─────────────────
//
// One table-driven test covers all representative `param_ty` variants because
// the guard `arg_ty.is_error()` is type-agnostic w.r.t. `param_ty` — it
// returns true for every possible `param_ty` value identically.  A single
// function keeps the variant list in one place and eliminates per-variant
// maintenance drag.

/// Mirror: `type_compatible(_, Error) == true` for all representative `param_ty` variants.
/// Pins the anti-cascade producer path where `arg_ty` is the poison sentinel (task-1912).
#[test]
fn type_compatible_error_wildcard_mirror_all_variants() {
    let cases: Vec<(&str, Type)> = vec![
        ("Real", Type::Real),
        ("Int", Type::Int),
        ("Bool", Type::Bool),
        ("String", Type::String),
        ("Scalar[m]", Type::length()),
        ("List<Int>", Type::List(Box::new(Type::Int))),
        ("Option<Real>", Type::Option(Box::new(Type::Real))),
        ("Vector<3,Real>", Type::vec3(Type::Real)),
        ("Matrix<3,3,Real>", Type::matrix(3, 3, Type::Real)),
    ];
    for (label, param_ty) in &cases {
        assert!(
            type_compatible(param_ty, &Type::Error),
            "type_compatible({label}, Error) must be true (anti-cascade guard checks arg_ty, task-1912)"
        );
    }
}
