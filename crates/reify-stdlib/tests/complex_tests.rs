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
use reify_core::DimensionVector;
use reify_ir::Value;

const TOLERANCE: f64 = 1e-12;

// ── Helper functions ─────────────────────────────────────────────────────────

/// Build a Complex value directly for use as test input.
fn complex_val(re: f64, im: f64, dimension: DimensionVector) -> Value {
    Value::Complex { re, im, dimension }
}

/// Assert that a Value is Complex with the expected re, im, and dimension.
fn assert_complex_eq(
    actual: &Value,
    expected_re: f64,
    expected_im: f64,
    expected_dim: DimensionVector,
) {
    match actual {
        Value::Complex { re, im, dimension } => {
            assert!(
                (*re - expected_re).abs() < TOLERANCE,
                "expected re={}, got {}",
                expected_re,
                re
            );
            assert!(
                (*im - expected_im).abs() < TOLERANCE,
                "expected im={}, got {}",
                expected_im,
                im
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
                expected,
                v
            );
        }
        other => panic!("expected Real({}), got {:?}", expected, other),
    }
}

/// Assert that a Value is Scalar with the expected SI value and dimension.
fn assert_scalar_approx(actual: &Value, expected_si: f64, expected_dim: DimensionVector) {
    match actual {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (*si_value - expected_si).abs() < TOLERANCE,
                "expected si_value={}, got {}",
                expected_si,
                si_value
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

// ── Error-case tests (step-3) ────────────────────────────────────────────────

#[test]
fn construct_complex_dimension_mismatch_returns_undef() {
    // complex(Scalar{LENGTH}, Scalar{TIME}) -> Undef (dimension mismatch)
    let result = eval_builtin(
        "complex",
        &[
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::TIME,
            },
        ],
    );
    assert!(
        result.is_undef(),
        "expected Undef for LENGTH+TIME mismatch, got {:?}",
        result
    );
}

#[test]
fn construct_complex_wrong_arg_count_returns_undef() {
    // complex() with 0 args -> Undef
    let result_zero = eval_builtin("complex", &[]);
    assert!(
        result_zero.is_undef(),
        "expected Undef for 0 args, got {:?}",
        result_zero
    );

    // complex() with 3 args -> Undef
    let result_three = eval_builtin(
        "complex",
        &[Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
    );
    assert!(
        result_three.is_undef(),
        "expected Undef for 3 args, got {:?}",
        result_three
    );
}

#[test]
fn construct_complex_non_numeric_returns_undef() {
    // complex(Bool, Real) -> Undef (Bool is non-numeric)
    let result = eval_builtin("complex", &[Value::Bool(true), Value::Real(3.0)]);
    assert!(
        result.is_undef(),
        "expected Undef for non-numeric arg, got {:?}",
        result
    );
}

// ── Arithmetic tests (step-5) ────────────────────────────────────────────────

#[test]
fn complex_add_same_dimension_sums_components() {
    // complex_add((1+2i)[LENGTH], (3+4i)[LENGTH]) = (4+6i)[LENGTH]
    let a = complex_val(1.0, 2.0, DimensionVector::LENGTH);
    let b = complex_val(3.0, 4.0, DimensionVector::LENGTH);
    let result = eval_builtin("complex_add", &[a, b]);
    assert_complex_eq(&result, 4.0, 6.0, DimensionVector::LENGTH);
}

#[test]
fn complex_mul_cross_dimension_combines() {
    // complex_mul((1+2i)[LENGTH], (3+4i)[TIME]) = (-5+10i)[LENGTH*TIME]
    // (1+2i)(3+4i) = (1*3 - 2*4) + (1*4 + 2*3)i = -5 + 10i
    let a = complex_val(1.0, 2.0, DimensionVector::LENGTH);
    let b = complex_val(3.0, 4.0, DimensionVector::TIME);
    let result = eval_builtin("complex_mul", &[a, b]);
    let expected_dim = DimensionVector::LENGTH.mul(&DimensionVector::TIME);
    assert_complex_eq(&result, -5.0, 10.0, expected_dim);
}

// ── Magnitude / Phase / Conjugate tests (step-7) ─────────────────────────────

#[test]
fn complex_magnitude_3_4i_returns_5() {
    // complex_magnitude(3+4i) = sqrt(9+16) = 5.0 as Real (dimensionless)
    let z = complex_val(3.0, 4.0, DimensionVector::DIMENSIONLESS);
    let result = eval_builtin("complex_magnitude", &[z]);
    assert_real_approx(&result, 5.0);
}

#[test]
fn phase_1_1i_returns_pi_over_4() {
    // phase(1+1i) = atan2(1,1) = pi/4, returned as Scalar with ANGLE dimension
    let z = complex_val(1.0, 1.0, DimensionVector::DIMENSIONLESS);
    let result = eval_builtin("phase", &[z]);
    assert_scalar_approx(&result, std::f64::consts::FRAC_PI_4, DimensionVector::ANGLE);
}

#[test]
fn phase_zero_complex_returns_undef() {
    // phase(0+0i) is mathematically undefined (zero vector has no direction)
    let z = complex_val(0.0, 0.0, DimensionVector::DIMENSIONLESS);
    let result = eval_builtin("phase", &[z]);
    assert!(
        result.is_undef(),
        "phase(0+0i) should be Undef, got {:?}",
        result
    );
}

#[test]
fn phase_dimensioned_complex_returns_angle() {
    // phase() discards the input dimension and always returns ANGLE
    let impedance = DimensionVector::MASS
        .mul(&DimensionVector::LENGTH.pow(2))
        .div(&DimensionVector::TIME.pow(3))
        .div(&DimensionVector::CURRENT.pow(2));
    let z = complex_val(50.0, -25.0, impedance);
    let result = eval_builtin("phase", &[z]);
    let expected_phase = (-25.0_f64).atan2(50.0);
    assert_scalar_approx(&result, expected_phase, DimensionVector::ANGLE);
}

#[test]
fn conjugate_3_4i_negates_imaginary() {
    // conjugate(3+4i) = 3-4i, preserving dimension
    let z = complex_val(3.0, 4.0, DimensionVector::LENGTH);
    let result = eval_builtin("conjugate", &[z]);
    assert_complex_eq(&result, 3.0, -4.0, DimensionVector::LENGTH);
}

// ── Accessor tests (step-9) ──────────────────────────────────────────────────

#[test]
fn re_im_dimensionless_returns_real() {
    // re(Complex{3,4,DIMLESS}) -> Real(3.0), im(Complex{3,4,DIMLESS}) -> Real(4.0)
    let z = complex_val(3.0, 4.0, DimensionVector::DIMENSIONLESS);
    let re = eval_builtin("re", std::slice::from_ref(&z));
    let im = eval_builtin("im", std::slice::from_ref(&z));
    assert_real_approx(&re, 3.0);
    assert_real_approx(&im, 4.0);
}

#[test]
fn re_im_dimensioned_returns_scalar() {
    // re(Complex{3,4,LENGTH}) -> Scalar{3.0, LENGTH}, im -> Scalar{4.0, LENGTH}
    let z = complex_val(3.0, 4.0, DimensionVector::LENGTH);
    let re = eval_builtin("re", std::slice::from_ref(&z));
    let im = eval_builtin("im", std::slice::from_ref(&z));
    assert_scalar_approx(&re, 3.0, DimensionVector::LENGTH);
    assert_scalar_approx(&im, 4.0, DimensionVector::LENGTH);
}

// ── Complex<Impedance> integration test (step-11) ────────────────────────────

#[test]
fn complex_impedance_workflow() {
    // Build impedance dimension: kg·m²·s⁻³·A⁻² (ohms in SI)
    let impedance = DimensionVector::MASS
        .mul(&DimensionVector::LENGTH.pow(2))
        .div(&DimensionVector::TIME.pow(3))
        .div(&DimensionVector::CURRENT.pow(2));

    // Construct complex impedance: Z = 50 - 25i (ohms)
    let z = eval_builtin(
        "complex",
        &[
            Value::Scalar {
                si_value: 50.0,
                dimension: impedance,
            },
            Value::Scalar {
                si_value: -25.0,
                dimension: impedance,
            },
        ],
    );
    assert_complex_eq(&z, 50.0, -25.0, impedance);

    // re(Z) -> Scalar{50, impedance}
    let re = eval_builtin("re", std::slice::from_ref(&z));
    assert_scalar_approx(&re, 50.0, impedance);

    // im(Z) -> Scalar{-25, impedance}
    let im = eval_builtin("im", std::slice::from_ref(&z));
    assert_scalar_approx(&im, -25.0, impedance);

    // complex_magnitude(Z) = sqrt(50² + (-25)²) = sqrt(3125) ≈ 55.9017
    let mag = eval_builtin("complex_magnitude", std::slice::from_ref(&z));
    let expected_mag = (50.0_f64.powi(2) + 25.0_f64.powi(2)).sqrt();
    assert_scalar_approx(&mag, expected_mag, impedance);

    // phase(Z) = atan2(-25, 50) ≈ -0.4636 rad, returned as Scalar with ANGLE
    let ph = eval_builtin("phase", std::slice::from_ref(&z));
    let expected_phase = (-25.0_f64).atan2(50.0);
    assert_scalar_approx(&ph, expected_phase, DimensionVector::ANGLE);

    // conjugate(Z) = 50 + 25i (impedance)
    let conj = eval_builtin("conjugate", &[z]);
    assert_complex_eq(&conj, 50.0, 25.0, impedance);
}
