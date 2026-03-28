//! Integration tests for Complex<Q> stdlib functions.
//!
//! Tests the public API `reify_stdlib::eval_builtin()` for:
//! - complex() constructor
//! - re/im accessors
//! - conjugate, phase, complex_magnitude
//! - complex_add, complex_mul
//! - error cases (dimension mismatch, wrong arg count, non-numeric)
//! - Complex<Impedance> real-world workflow

use reify_stdlib::eval_builtin;
use reify_types::{DimensionVector, Value};

const TOLERANCE: f64 = 1e-12;

// ── Helper functions ─────────────────────────────────────────────────────────

/// Build a Complex value directly for use as test input.
fn complex_val(re: f64, im: f64, dimension: DimensionVector) -> Value {
    Value::Complex { re, im, dimension }
}

/// Assert that a Value is Complex with the expected re, im, and dimension.
fn assert_complex_eq(actual: &Value, expected_re: f64, expected_im: f64, expected_dim: DimensionVector) {
    match actual {
        Value::Complex { re, im, dimension } => {
            assert!(
                (*re - expected_re).abs() < TOLERANCE,
                "expected re={}, got {}",
                expected_re, re
            );
            assert!(
                (*im - expected_im).abs() < TOLERANCE,
                "expected im={}, got {}",
                expected_im, im
            );
            assert_eq!(
                *dimension, expected_dim,
                "expected dimension {:?}, got {:?}",
                expected_dim, dimension
            );
        }
        other => panic!(
            "expected Complex{{re={}, im={}, dim={:?}}}, got {:?}",
            expected_re, expected_im, expected_dim, other
        ),
    }
}

/// Assert that a Value is Real and approximately equal to expected.
fn assert_real_approx(actual: &Value, expected: f64) {
    match actual {
        Value::Real(v) => {
            assert!(
                (*v - expected).abs() < TOLERANCE,
                "expected Real({}), got Real({})",
                expected, v
            );
        }
        other => panic!("expected Real({}), got {:?}", expected, other),
    }
}

/// Assert that a Value is Scalar with the expected SI value and dimension.
fn assert_scalar_approx(actual: &Value, expected_si: f64, expected_dim: DimensionVector) {
    match actual {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - expected_si).abs() < TOLERANCE,
                "expected si_value={}, got {}",
                expected_si, si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "expected dimension {:?}, got {:?}",
                expected_dim, dimension
            );
        }
        other => panic!(
            "expected Scalar{{si={}, dim={:?}}}, got {:?}",
            expected_si, expected_dim, other
        ),
    }
}

// ── Constructor tests (step-1) ───────────────────────────────────────────────

#[test]
fn construct_complex_real_real_dimensionless() {
    // complex(Real(3.0), Real(4.0)) -> Complex { re: 3.0, im: 4.0, DIMENSIONLESS }
    let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(4.0)]);
    assert_complex_eq(&result, 3.0, 4.0, DimensionVector::DIMENSIONLESS);
}

#[test]
fn construct_complex_int_int_dimensionless() {
    // complex(Int(5), Int(-2)) -> Complex { re: 5.0, im: -2.0, DIMENSIONLESS }
    let result = eval_builtin("complex", &[Value::Int(5), Value::Int(-2)]);
    assert_complex_eq(&result, 5.0, -2.0, DimensionVector::DIMENSIONLESS);
}

#[test]
fn construct_complex_int_real_mixed_coercion() {
    // complex(Int(1), Real(2.5)) -> Complex { re: 1.0, im: 2.5, DIMENSIONLESS }
    let result = eval_builtin("complex", &[Value::Int(1), Value::Real(2.5)]);
    assert_complex_eq(&result, 1.0, 2.5, DimensionVector::DIMENSIONLESS);
}

#[test]
fn construct_complex_scalar_length_preserves_dimension() {
    // complex(Scalar{0.005, LENGTH}, Scalar{0.003, LENGTH}) -> Complex{0.005, 0.003, LENGTH}
    let result = eval_builtin(
        "complex",
        &[
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.003,
                dimension: DimensionVector::LENGTH,
            },
        ],
    );
    assert_complex_eq(&result, 0.005, 0.003, DimensionVector::LENGTH);
}
