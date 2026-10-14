//! Behavioural pin of the toleranced Scalar-equality policy (task #6653).
//!
//! Every pair below is derived from a requirement or a measured probe, never
//! from the module's private constants: float noise left by the solver must
//! compare equal, a 1% design drift must not, and the absolute floor must be
//! dimension-aware without ever dropping below the solver's reachable
//! residual.

use reify_core::scalar_tolerance::{DEFAULT_ABSOLUTE_FLOOR, scalars_equal};
use reify_core::{DimensionVector, NAMED_DIMENSIONS};

const LENGTH: DimensionVector = DimensionVector::LENGTH;
const ANGLE: DimensionVector = DimensionVector::ANGLE;
const MASS: DimensionVector = DimensionVector::MASS;
const DIMENSIONLESS: DimensionVector = DimensionVector::DIMENSIONLESS;

fn assert_equal(a: f64, b: f64, dimension: DimensionVector, why: &str) {
    assert!(
        scalars_equal(a, b, dimension),
        "{why}: expected ({a:e}, {b:e}) EQUAL under {dimension:?}"
    );
}

fn assert_not_equal(a: f64, b: f64, dimension: DimensionVector, why: &str) {
    assert!(
        !scalars_equal(a, b, dimension),
        "{why}: expected ({a:e}, {b:e}) NOT equal under {dimension:?}"
    );
}

#[test]
fn float_noise_compares_equal() {
    assert_equal(
        0.12299999999999939,
        0.123,
        LENGTH,
        "auto_probe solved x vs 123mm",
    );
    assert_equal(
        0.018500000000000492 + 0.011499999999999684,
        0.03,
        LENGTH,
        "multi_auto2 solved a + b vs 30mm",
    );
    assert_equal(0.1 + 0.2, 0.3, DIMENSIONLESS, "0.1 + 0.2 vs 0.3");
}

#[test]
fn penalty_offset_residual_compares_equal_for_length() {
    assert_equal(
        0.01,
        0.01 + 5.0e-7,
        LENGTH,
        "ineq_eq penalty offset 1/(2*PENALTY_WEIGHT)",
    );
    assert_equal(
        0.01 + 5.0e-7,
        0.01,
        LENGTH,
        "ineq_eq penalty offset, reversed",
    );
}

#[test]
fn one_percent_drift_is_not_equal() {
    assert_not_equal(0.01, 0.0101, LENGTH, "1% drift at 10mm");
    assert_not_equal(1.0, 1.01, LENGTH, "1% drift at 1m");
    assert_not_equal(2.0, 2.02, MASS, "1% drift at 2kg");
    assert_not_equal(1.0, 1.01, DIMENSIONLESS, "1% drift at 1.0");
    assert_not_equal(
        0.5235987755982988,
        0.5288347633542818,
        ANGLE,
        "1% drift at 30deg",
    );
}

#[test]
fn relative_arm_scales_with_magnitude() {
    assert_equal(
        1e6,
        1e6 * (1.0 + 5e-10),
        DIMENSIONLESS,
        "5e-10 relative at 1e6",
    );
    assert_not_equal(
        1e6,
        1e6 * (1.0 + 1e-8),
        DIMENSIONLESS,
        "1e-8 relative at 1e6",
    );
}

#[test]
fn absolute_floor_is_dimension_aware() {
    assert_equal(0.0, 5.0e-7, LENGTH, "half a micrometre");
    assert_equal(0.0, 5.0e-7, ANGLE, "half a microradian");
    assert_not_equal(0.0, 5.0e-7, MASS, "half a milligram");
    assert_not_equal(0.0, 5.0e-7, DIMENSIONLESS, "5e-7 dimensionless");
    assert_not_equal(0.0, 2.0e-6, LENGTH, "two micrometres");
}

#[test]
fn default_floor_applies_near_zero() {
    assert_equal(0.0, 1e-13, DIMENSIONLESS, "1e-13 dimensionless");
    assert_not_equal(0.0, 1e-10, DIMENSIONLESS, "1e-10 dimensionless");
}

#[test]
fn no_dimension_is_stricter_than_the_solvers_reachable_residual() {
    assert_eq!(DEFAULT_ABSOLUTE_FLOOR, 1e-12);
    let dimensions = NAMED_DIMENSIONS
        .iter()
        .map(|(dimension, name)| (*dimension, *name))
        .chain(std::iter::once((DIMENSIONLESS, "Dimensionless")));
    for (dimension, name) in dimensions {
        assert!(
            scalars_equal(0.0, DEFAULT_ABSOLUTE_FLOOR, dimension),
            "{name}: a residual of DEFAULT_ABSOLUTE_FLOOR near zero must be accepted"
        );
        assert!(
            scalars_equal(1.0, 1.0 + DEFAULT_ABSOLUTE_FLOOR, dimension),
            "{name}: a residual of DEFAULT_ABSOLUTE_FLOOR at unit magnitude must be accepted"
        );
    }
}

#[test]
fn comparison_is_symmetric() {
    let pairs = [
        (0.12299999999999939, 0.123, LENGTH),
        (0.01, 0.0101, LENGTH),
        (0.0, 5.0e-7, LENGTH),
        (0.0, 5.0e-7, MASS),
        (1e6, 1e6 * (1.0 + 5e-10), DIMENSIONLESS),
        (1e6, 1e6 * (1.0 + 1e-8), DIMENSIONLESS),
    ];
    for (a, b, dimension) in pairs {
        assert_eq!(
            scalars_equal(a, b, dimension),
            scalars_equal(b, a, dimension),
            "({a:e}, {b:e}) under {dimension:?} must compare symmetrically"
        );
    }
}

#[test]
fn non_finite_operands_compare_exactly() {
    assert_not_equal(f64::NAN, f64::NAN, LENGTH, "NaN vs NaN");
    assert_equal(f64::INFINITY, f64::INFINITY, LENGTH, "inf vs inf");
    assert_not_equal(f64::NEG_INFINITY, f64::INFINITY, LENGTH, "-inf vs inf");
    assert_not_equal(f64::INFINITY, 1e300, DIMENSIONLESS, "inf vs 1e300");
    assert_equal(0.0, -0.0, LENGTH, "0 vs -0");
}
