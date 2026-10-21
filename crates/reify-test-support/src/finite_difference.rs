//! Central-difference reference for checking automatic-differentiation tangents,
//! and the tolerance an AD column is held to against it.
//!
//! The step shape mirrors that of `reify_expr`'s
//! `calculus::compute_numerical_gradient_at_point`
//!
//! ```text
//! h = 1e-6 * |x_j|.max(1e-3),   f'(x) ≈ (f(x+h) − f(x−h)) / (2h)
//! ```
//!
//! as an independent copy of its constants, not a call into it, so the
//! reference an AD suite is checked against shares no code with production.
//!
//! TOLERANCE, DERIVED — `|ad − cd| <= 1e-6·|cd| + 1e-8`:
//!
//!   * truncation error of central differences is h²·|f'''|/6; at |x| ≈ 1,
//!     h = 1e-6 gives h²/6 ≈ 1.7e-13, and for the residuals the AD suites probe
//!     (polynomials and sin/cos/exp/sqrt/log/atan2 at well-conditioned points)
//!     |f'''| ≲ 10, so ≲ 2e-12;
//!   * cancellation error is ε_mach·|f|/h ≈ 2.2e-16·|f|/1e-6 = 2.2e-10·|f|;
//!     for |f| ≲ 10 that is ≲ 2.2e-9.
//!
//! Total reference error ≲ 3e-9 absolute, i.e. ≲ 3e-8 relative against a
//! derivative of magnitude ≥ 0.1.  The 1e-6 relative bound clears the
//! reference's own floor by ~1.5 orders; asserting anything tighter would be
//! testing the reference rather than the subject.
//!
//! Every probe point must have |∂r/∂x_j| >= 0.1 in SI units, which makes the
//! RELATIVE arm binding and leaves the 1e-8 absolute floor guarding exact-zero
//! columns only.  [`assert_column_matches_central_difference`] asserts that
//! precondition itself, so a suite cannot quietly decay into comparing two
//! numbers that are both ~0.

const RELATIVE_TOLERANCE: f64 = 1e-6;
const ABSOLUTE_FLOOR: f64 = 1e-8;
const MIN_PROBE_SLOPE: f64 = 0.1;

/// Central-difference estimate of `f'(x0)`: relative step for large `|x0|`, with
/// an absolute floor so the step does not collapse to zero near the origin.
pub fn central_difference(x0: f64, f: impl Fn(f64) -> f64) -> f64 {
    let h = 1e-6_f64 * x0.abs().max(1e-3);
    (f(x0 + h) - f(x0 - h)) / (2.0 * h)
}

/// Assert that the AD tangent `ad` of `column` agrees with its central-difference
/// reference `cd` within the tolerance derived in the module doc, after checking
/// that `cd` is steep enough for the relative arm of that tolerance to bind.
#[track_caller]
pub fn assert_column_matches_central_difference(label: &str, column: usize, ad: f64, cd: f64) {
    assert!(
        cd.abs() >= MIN_PROBE_SLOPE,
        "{label} column {column}: probe point must have |∂r/∂x_j| >= 0.1 in SI units so the \
         RELATIVE arm of the tolerance binds; got {cd:?}. Fix the probe point, not the \
         tolerance."
    );
    let tol = RELATIVE_TOLERANCE * cd.abs() + ABSOLUTE_FLOOR;
    assert!(
        (ad - cd).abs() <= tol,
        "{label} column {column}: ad={ad:?} vs central difference {cd:?} (tol {tol:?})"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn central_difference_of_a_cubic_matches_its_analytic_derivative() {
        // h = 2e-6; truncation h²·f'''/6 = 4e-12; rounding ≈ 8·2.2e-16/4e-6 plus the
        // x+h representation shift 12·4.4e-16/4e-6, ≈ 2e-9 in total — 1e-7 has ~50x margin.
        let cd = central_difference(2.0, |x| x * x * x);
        assert!((cd - 12.0).abs() <= 1e-7, "cd = {cd:?}");
    }

    #[test]
    fn central_difference_keeps_an_absolute_step_floor_at_the_origin() {
        // With the 1e-3 floor h = 1e-9 and (h − (−h)) / (2h) is exactly 1.0 in IEEE
        // arithmetic; without it h = 0 and the quotient is 0/0 = NaN.
        assert_eq!(central_difference(0.0, |x| x), 1.0);
    }

    #[test]
    fn a_tangent_within_the_relative_tolerance_is_accepted() {
        assert_column_matches_central_difference("ok", 0, 1.0, 1.0 + 5e-7);
    }

    #[test]
    #[should_panic(expected = "central difference")]
    fn a_tangent_outside_the_tolerance_is_rejected() {
        assert_column_matches_central_difference("off", 0, 1.0, 1.0 + 1e-5);
    }

    #[test]
    #[should_panic(expected = "probe point")]
    fn a_probe_point_with_a_near_flat_reference_is_rejected() {
        assert_column_matches_central_difference("flat", 0, 0.05, 0.05);
    }
}
