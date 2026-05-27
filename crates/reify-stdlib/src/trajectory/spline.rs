//! Pure-Rust spline math for the trajectory stdlib module.
//!
//! Implements interpolating cubic and quintic B-splines used by
//! `piecewise_polynomial` / `evaluate_profile*` / `profile_duration`.
//!
//! This module has no `reify_types` dependency — all inputs and outputs are
//! plain `f64` / `Vec<f64>`.  Value marshalling lives in `mod.rs`.

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn cubic_p(t: f64) -> f64 {
        1.0 + 2.0 * t - 0.5 * t * t + 0.3 * t * t * t
    }
    fn cubic_dp(t: f64) -> f64 {
        2.0 - t + 0.9 * t * t
    }
    #[allow(dead_code)]
    fn cubic_ddp(t: f64) -> f64 {
        -1.0 + 1.8 * t
    }

    // ── Step-1: natural cubic — corrected contract ────────────────────────────

    /// Natural cubic spline satisfies:
    /// (i)  at-knot interpolation within TOL (by construction), and
    /// (ii) endpoint second-derivative == 0 within TOL (the defining BC invariant).
    ///
    /// Off-knot exact reproduction is NOT asserted — it is mathematically
    /// impossible for Natural BC when the source data has non-zero endpoint
    /// curvature (see plan analysis / design_decisions).
    #[test]
    fn cubic_natural_spline_interpolates_at_knots_and_satisfies_natural_bc() {
        let ts = [0.0, 1.0, 2.5, 4.0];
        let vs: Vec<f64> = ts.iter().map(|&t| cubic_p(t)).collect();
        let spline = CubicSpline::fit(&ts, &vs, &BoundaryCondition::Natural)
            .expect("fit should succeed");

        // (i) at-knot interpolation
        for &t in &ts {
            let got = spline.eval(t);
            assert!(
                (got - cubic_p(t)).abs() < TOL,
                "eval at knot t={t}: got {got}, want {}, diff {}",
                cubic_p(t),
                (got - cubic_p(t)).abs()
            );
        }

        // (ii) natural BC invariant: M[0] = M[N] = 0
        let ddot_start = spline.eval_ddot(ts[0]);
        assert!(
            ddot_start.abs() < TOL,
            "natural BC: eval_ddot(t_0)={ddot_start}, want 0"
        );
        let ddot_end = spline.eval_ddot(ts[3]);
        assert!(
            ddot_end.abs() < TOL,
            "natural BC: eval_ddot(t_N)={ddot_end}, want 0"
        );
    }

    #[test]
    fn cubic_spline_duration_equals_last_minus_first_knot() {
        let ts = [0.5, 1.0, 2.5, 4.0];
        let vs: Vec<f64> = ts.iter().map(|&t| cubic_p(t)).collect();
        let spline = CubicSpline::fit(&ts, &vs, &BoundaryCondition::Natural)
            .expect("fit should succeed");
        assert!(
            (spline.duration() - 3.5).abs() < TOL,
            "duration: got {}, want 3.5",
            spline.duration()
        );
    }

    #[test]
    fn cubic_fit_returns_none_for_single_knot() {
        assert!(
            CubicSpline::fit(&[1.0], &[1.0], &BoundaryCondition::Natural).is_none(),
            "single knot should return None"
        );
    }

    #[test]
    fn cubic_fit_returns_none_for_non_increasing_knots() {
        assert!(
            CubicSpline::fit(&[0.0, 1.0, 0.5], &[1.0, 2.0, 3.0], &BoundaryCondition::Natural)
                .is_none(),
            "non-increasing knots should return None"
        );
    }

    // NOTE: cubic_dp and cubic_ddp are used in later steps (step-3, step-9).
    // Suppress unused-function warnings at this stage.
    #[allow(unused)]
    fn _use_helpers() {
        let _ = cubic_dp(0.0);
        let _ = cubic_ddp(0.0);
    }
}
