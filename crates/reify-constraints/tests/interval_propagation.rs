//! Interval propagation (task #6655): the closed-interval arithmetic in
//! `reify_constraints::interval`, driven through its public API only.
//!
//! Exact operations must stay exact (the γ SteppedInside clamp and the
//! exact-value derivation tests depend on it); inexact ones must bracket the
//! true result one ulp wide.

use reify_constraints::interval::Interval;

fn iv(lo: f64, hi: f64) -> Interval {
    Interval::new(lo, hi).expect("well-formed interval")
}

fn pt(v: f64) -> Interval {
    Interval::point(v).expect("finite point")
}

fn assert_endpoints(actual: Interval, lo: f64, hi: f64) {
    assert_eq!(
        (actual.lo(), actual.hi()),
        (lo, hi),
        "expected [{lo}, {hi}], got {actual:?}"
    );
}

fn assert_one_ulp(actual: Interval) {
    assert!(
        actual.lo() < actual.hi(),
        "inexact result must widen: {actual:?}"
    );
    assert_eq!(
        actual.hi(),
        actual.lo().next_up(),
        "inexact result must be one ulp wide: {actual:?}"
    );
}

// ── (a) construction ────────────────────────────────────────────────────────

#[test]
fn construction_rejects_reversed_and_nan_endpoints() {
    assert_eq!(Interval::new(2.0, 1.0), None);
    assert_eq!(Interval::new(f64::NAN, 1.0), None);
    assert_eq!(Interval::new(0.0, f64::NAN), None);
    assert_eq!(Interval::new(f64::INFINITY, f64::INFINITY), None);
    assert_eq!(Interval::new(f64::NEG_INFINITY, f64::NEG_INFINITY), None);
    assert_endpoints(iv(1.0, 2.0), 1.0, 2.0);
}

#[test]
fn point_rejects_non_finite_values() {
    assert_eq!(Interval::point(f64::INFINITY), None);
    assert_eq!(Interval::point(f64::NEG_INFINITY), None);
    assert_eq!(Interval::point(f64::NAN), None);
    assert_endpoints(pt(0.25), 0.25, 0.25);
}

#[test]
fn entire_spans_the_extended_reals() {
    assert_endpoints(Interval::ENTIRE, f64::NEG_INFINITY, f64::INFINITY);
    assert!(Interval::ENTIRE.is_entire());
    assert!(!iv(0.0, f64::INFINITY).is_entire());
}

#[test]
fn intersect_and_hull() {
    assert_eq!(iv(0.0, 1.0).intersect(iv(2.0, 3.0)), None);
    assert_eq!(iv(0.0, 2.0).intersect(iv(1.0, 3.0)), Some(iv(1.0, 2.0)));
    assert_eq!(iv(0.0, 1.0).intersect(iv(1.0, 3.0)), Some(pt(1.0)));
    assert_eq!(iv(0.0, 1.0).hull(iv(2.0, 3.0)), iv(0.0, 3.0));
    assert_eq!(iv(-1.0, 5.0).hull(iv(2.0, 3.0)), iv(-1.0, 5.0));
}

// ── (b) exact operations are not widened ────────────────────────────────────

#[test]
fn exact_operations_stay_exact() {
    assert_endpoints(pt(0.03) / pt(2.0), 0.015, 0.015);
    assert_endpoints(pt(1.0) + pt(2.0), 3.0, 3.0);
    assert_endpoints(pt(1.5) * pt(2.0), 3.0, 3.0);
    assert_endpoints(pt(100.0) - pt(2.0), 98.0, 98.0);
    assert_endpoints(pt(9.0).sqrt().expect("non-negative"), 3.0, 3.0);
}

// ── (c) inexact operations bracket tightly ──────────────────────────────────

#[test]
fn inexact_quotient_brackets_one_third() {
    let q = pt(1.0) / pt(3.0);
    assert_one_ulp(q);
    assert!(
        q.lo().mul_add(3.0, -1.0) <= 0.0,
        "lo must not exceed 1/3: {q:?}"
    );
    assert!(
        q.hi().mul_add(3.0, -1.0) >= 0.0,
        "hi must not fall below 1/3: {q:?}"
    );
}

#[test]
fn inexact_sum_brackets_the_nearest_sum() {
    let s = pt(0.1) + pt(0.2);
    let nearest = 0.1 + 0.2;
    assert_one_ulp(s);
    assert!(
        s.lo() <= nearest && nearest <= s.hi(),
        "{s:?} must contain {nearest}"
    );
}

#[test]
fn inexact_sqrt_brackets_root_two() {
    let s = pt(2.0).sqrt().expect("non-negative");
    assert_one_ulp(s);
    assert!(
        s.lo().mul_add(s.lo(), -2.0) <= 0.0,
        "lo must not exceed √2: {s:?}"
    );
    assert!(
        s.hi().mul_add(s.hi(), -2.0) >= 0.0,
        "hi must not fall below √2: {s:?}"
    );
}

#[test]
fn inexact_product_and_difference_bracket_the_true_value() {
    let p = pt(0.1) * pt(3.0);
    assert_one_ulp(p);
    let residual_lo = 0.1f64.mul_add(3.0, -p.lo());
    let residual_hi = 0.1f64.mul_add(3.0, -p.hi());
    assert!(residual_lo >= 0.0, "lo must not exceed 0.1·3: {p:?}");
    assert!(residual_hi <= 0.0, "hi must not fall below 0.1·3: {p:?}");

    let d = pt(0.3) - pt(0.1);
    assert!(d.lo() <= 0.3 - 0.1 && 0.3 - 0.1 <= d.hi(), "{d:?}");
}

// ── (d) infinities and degenerate cases ─────────────────────────────────────

#[test]
fn infinities_and_degenerate_divisors() {
    assert_eq!(Interval::ENTIRE + pt(1.0), Interval::ENTIRE);
    assert_endpoints(pt(0.0) * Interval::ENTIRE, 0.0, 0.0);
    assert_eq!(iv(1.0, 2.0) / iv(-1.0, 1.0), Interval::ENTIRE);
    assert_eq!(iv(1.0, 2.0) / pt(0.0), Interval::ENTIRE);
    assert_endpoints(-iv(1.0, 2.0), -2.0, -1.0);
    assert_endpoints(-iv(1.0, f64::INFINITY), f64::NEG_INFINITY, -1.0);
}

#[test]
fn half_unbounded_arithmetic_keeps_the_finite_side() {
    let x = iv(1.0, f64::INFINITY);
    assert_endpoints(x + pt(2.0), 3.0, f64::INFINITY);
    assert_endpoints(x - pt(2.0), -1.0, f64::INFINITY);
    assert_endpoints(x * pt(2.0), 2.0, f64::INFINITY);
    assert_endpoints(x * pt(-2.0), f64::NEG_INFINITY, -2.0);
    assert_endpoints(x / pt(2.0), 0.5, f64::INFINITY);
    assert_endpoints(pt(2.0) / x, 0.0, 2.0);
    assert_endpoints(x / x, 0.0, f64::INFINITY);
}

#[test]
fn product_of_signed_intervals_takes_the_corner_hull() {
    assert_endpoints(iv(-2.0, 3.0) * iv(-1.0, 4.0), -8.0, 12.0);
    assert_endpoints(iv(-2.0, -1.0) * iv(-3.0, -2.0), 2.0, 6.0);
    assert_endpoints(iv(2.0, 6.0) / iv(-2.0, -1.0), -6.0, -1.0);
}

// ── (e) sqrt, powi, abs, min, max ───────────────────────────────────────────

#[test]
fn sqrt_clips_to_its_domain() {
    assert_eq!(iv(-4.0, 9.0).sqrt(), Some(iv(0.0, 3.0)));
    assert_eq!(iv(-4.0, -1.0).sqrt(), None);
    assert_eq!(iv(4.0, f64::INFINITY).sqrt(), Some(iv(2.0, f64::INFINITY)));
}

#[test]
fn integer_powers() {
    assert_endpoints(iv(-2.0, 3.0).powi(2), 0.0, 9.0);
    assert_endpoints(iv(-3.0, -2.0).powi(2), 4.0, 9.0);
    assert_endpoints(iv(-2.0, 3.0).powi(3), -8.0, 27.0);
    assert_endpoints(iv(-2.0, 3.0).powi(0), 1.0, 1.0);
    assert_endpoints(iv(-2.0, 3.0).powi(1), -2.0, 3.0);
    assert_endpoints(Interval::ENTIRE.powi(2), 0.0, f64::INFINITY);
}

#[test]
fn abs_min_max() {
    assert_endpoints(iv(-3.0, 2.0).abs(), 0.0, 3.0);
    assert_endpoints(iv(-3.0, -2.0).abs(), 2.0, 3.0);
    assert_endpoints(iv(1.0, 2.0).abs(), 1.0, 2.0);
    assert_endpoints(iv(1.0, 5.0).min(iv(2.0, 3.0)), 1.0, 3.0);
    assert_endpoints(iv(1.0, 5.0).max(iv(2.0, 3.0)), 2.0, 5.0);
}
