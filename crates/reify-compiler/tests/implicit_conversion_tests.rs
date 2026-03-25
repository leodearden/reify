//! Tests for the `implicitly_converts_to` function.
//!
//! These tests verify all four implicit conversion rules directionally:
//!   1. Vector<N,Q> <-> Tensor<1,N,Q>  (bidirectional)
//!   2. Scalar<Q> <-> Tensor<0,N,Q>    (bidirectional, N ignored)
//!   3. Tensor<2,N,Q> -> Matrix<N,N,Q>  (one-way: Tensor2 -> square Matrix)
//!   4. Matrix -> Tensor                (NOT implicit)

use reify_compiler::implicitly_converts_to;
use reify_types::{DimensionVector, Type};

// ── Helper type constructors ────────────────────────────────────────────────

fn length_scalar() -> Type {
    Type::Scalar { dimension: DimensionVector::LENGTH }
}

fn angle_scalar() -> Type {
    Type::Scalar { dimension: DimensionVector::ANGLE }
}

// ── Rule 1: Vector<N,Q> <-> Tensor<1,N,Q> (bidirectional) ──────────────────

/// (a) Vector<3,Real> -> Tensor<1,3,Real> is allowed.
#[test]
fn vector_to_tensor1_same_type() {
    let from = Type::Vector { n: 3, quantity: Box::new(Type::Real) };
    let to = Type::Tensor { rank: 1, n: 3, quantity: Box::new(Type::Real) };
    assert!(implicitly_converts_to(&from, &to), "Vector<3,Real> should convert to Tensor<1,3,Real>");
}

/// (b) Tensor<1,3,Real> -> Vector<3,Real> is allowed (reverse direction).
#[test]
fn tensor1_to_vector_same_type() {
    let from = Type::Tensor { rank: 1, n: 3, quantity: Box::new(Type::Real) };
    let to = Type::Vector { n: 3, quantity: Box::new(Type::Real) };
    assert!(implicitly_converts_to(&from, &to), "Tensor<1,3,Real> should convert to Vector<3,Real>");
}

/// (c) Vector<3,Scalar[m]> -> Tensor<1,3,Scalar[m]> — with a Scalar quantity type.
#[test]
fn vector_to_tensor1_scalar_quantity() {
    let from = Type::Vector { n: 3, quantity: Box::new(length_scalar()) };
    let to = Type::Tensor { rank: 1, n: 3, quantity: Box::new(length_scalar()) };
    assert!(implicitly_converts_to(&from, &to), "Vector<3,Scalar[m]> should convert to Tensor<1,3,Scalar[m]>");
}

/// (d) Vector<2,Real> -> Tensor<1,3,Real> is NOT allowed — N mismatch.
#[test]
fn vector_to_tensor1_n_mismatch() {
    let from = Type::Vector { n: 2, quantity: Box::new(Type::Real) };
    let to = Type::Tensor { rank: 1, n: 3, quantity: Box::new(Type::Real) };
    assert!(!implicitly_converts_to(&from, &to), "Vector<2,Real> should NOT convert to Tensor<1,3,Real> (N mismatch)");
}

/// (e) Vector<3,Real> -> Tensor<1,3,Scalar[m]> is NOT allowed — quantity mismatch.
#[test]
fn vector_to_tensor1_quantity_mismatch() {
    let from = Type::Vector { n: 3, quantity: Box::new(Type::Real) };
    let to = Type::Tensor { rank: 1, n: 3, quantity: Box::new(length_scalar()) };
    assert!(!implicitly_converts_to(&from, &to), "Vector<3,Real> should NOT convert to Tensor<1,3,Scalar[m]> (quantity mismatch)");
}

// ── Rule 2: Scalar<Q> <-> Tensor<0,N,Q> (bidirectional, N ignored) ─────────

/// (a) Scalar[m] -> Tensor<0,3,Scalar[m]> is allowed.
#[test]
fn scalar_to_tensor0() {
    let from = length_scalar();
    let to = Type::Tensor { rank: 0, n: 3, quantity: Box::new(length_scalar()) };
    assert!(implicitly_converts_to(&from, &to), "Scalar[m] should convert to Tensor<0,3,Scalar[m]>");
}

/// (b) Tensor<0,3,Scalar[m]> -> Scalar[m] is allowed.
#[test]
fn tensor0_to_scalar() {
    let from = Type::Tensor { rank: 0, n: 3, quantity: Box::new(length_scalar()) };
    let to = length_scalar();
    assert!(implicitly_converts_to(&from, &to), "Tensor<0,3,Scalar[m]> should convert to Scalar[m]");
}

/// (c) Scalar[angle] -> Tensor<0,5,Scalar[angle]> is allowed — different N is fine for rank-0.
#[test]
fn scalar_to_tensor0_different_n() {
    let from = angle_scalar();
    let to = Type::Tensor { rank: 0, n: 5, quantity: Box::new(angle_scalar()) };
    assert!(implicitly_converts_to(&from, &to), "Scalar[angle] should convert to Tensor<0,5,Scalar[angle]> (N ignored for rank-0)");
}

/// (d) Scalar[m] -> Tensor<0,3,Scalar[angle]> is NOT allowed — dimension mismatch.
#[test]
fn scalar_to_tensor0_dimension_mismatch() {
    let from = length_scalar();
    let to = Type::Tensor { rank: 0, n: 3, quantity: Box::new(angle_scalar()) };
    assert!(!implicitly_converts_to(&from, &to), "Scalar[m] should NOT convert to Tensor<0,3,Scalar[angle]>");
}

/// (e) Type::Real -> Tensor<0,3,Real> is allowed — dimensionless scalar-like.
#[test]
fn real_to_tensor0() {
    let from = Type::Real;
    let to = Type::Tensor { rank: 0, n: 3, quantity: Box::new(Type::Real) };
    assert!(implicitly_converts_to(&from, &to), "Real should convert to Tensor<0,3,Real>");
}

/// (f) Tensor<0,2,Real> -> Type::Real is allowed.
#[test]
fn tensor0_to_real() {
    let from = Type::Tensor { rank: 0, n: 2, quantity: Box::new(Type::Real) };
    let to = Type::Real;
    assert!(implicitly_converts_to(&from, &to), "Tensor<0,2,Real> should convert to Real");
}
