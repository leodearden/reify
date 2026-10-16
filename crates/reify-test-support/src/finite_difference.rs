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
