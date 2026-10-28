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

// ── Relational inverses: the backward half of HC4 ───────────────────────────
//
// Each inverse narrows ONE operand's current domain given that the operation's
// result lies in `z`, and returns `None` when nothing in that domain can.

fn non_negative() -> Interval {
    iv(0.0, f64::INFINITY)
}

fn non_positive() -> Interval {
    iv(f64::NEG_INFINITY, 0.0)
}

/// Sample points whose images under every operation below are exact in f64.
const SAMPLES: [f64; 15] = [
    -4.0, -3.0, -2.5, -2.0, -1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0,
];

/// Soundness: every sample in the current domain whose image lies in `z`
/// survives the narrowing.
fn assert_keeps_preimage(
    narrowed: Option<Interval>,
    domain: Interval,
    z: Interval,
    image: impl Fn(f64) -> Option<f64>,
) {
    for p in SAMPLES {
        let Some(v) = image(p) else { continue };
        if domain.contains(p) && z.contains(v) {
            let kept = narrowed.is_some_and(|n| n.contains(p));
            assert!(
                kept,
                "preimage point {p} (image {v} ∈ {z:?}) lost: {narrowed:?}"
            );
        }
    }
}

#[test]
fn even_power_preimage_respects_the_domain_sign() {
    let z = iv(1.0, 4.0);
    let square = |p: f64| Some(p * p);
    for (domain, expected) in [
        (Interval::ENTIRE, iv(-2.0, 2.0)),
        (non_negative(), iv(1.0, 2.0)),
        (non_positive(), iv(-2.0, -1.0)),
    ] {
        let narrowed = Interval::powi_preimage(z, domain, 2);
        assert_eq!(narrowed, Some(expected), "domain {domain:?}");
        assert_keeps_preimage(narrowed, domain, z, square);
    }
    assert_eq!(
        Interval::powi_preimage(iv(-5.0, -1.0), Interval::ENTIRE, 2),
        None
    );
    assert_eq!(
        Interval::powi_preimage(iv(16.0, 16.0), Interval::ENTIRE, 4),
        Some(iv(-2.0, 2.0))
    );
}

#[test]
fn odd_power_preimage_is_monotone() {
    let z = iv(-8.0, 27.0);
    let narrowed = Interval::powi_preimage(z, Interval::ENTIRE, 3);
    assert_eq!(narrowed, Some(iv(-2.0, 3.0)));
    assert_keeps_preimage(narrowed, Interval::ENTIRE, z, |p| Some(p * p * p));
    assert_eq!(Interval::powi_preimage(z, iv(5.0, 6.0), 3), None);
}

#[test]
fn inexact_root_preimage_brackets_the_true_root() {
    let narrowed = Interval::powi_preimage(iv(2.0, 2.0), non_negative(), 2).expect("non-empty");
    assert!(
        narrowed.lo().mul_add(narrowed.lo(), -2.0) <= 0.0,
        "{narrowed:?}"
    );
    assert!(
        narrowed.hi().mul_add(narrowed.hi(), -2.0) >= 0.0,
        "{narrowed:?}"
    );
    assert_eq!(narrowed.hi(), narrowed.lo().next_up(), "{narrowed:?}");

    let cube = Interval::powi_preimage(iv(2.0, 2.0), Interval::ENTIRE, 3).expect("non-empty");
    let cubed = |v: f64| v * v * v;
    assert!(cube.lo() < cube.hi(), "{cube:?}");
    assert!(
        cubed(cube.lo()) <= 2.0 && cubed(cube.hi()) >= 2.0,
        "{cube:?}"
    );
}

#[test]
fn zeroth_power_preimage_is_all_or_nothing() {
    let domain = iv(-3.0, 3.0);
    assert_eq!(
        Interval::powi_preimage(iv(0.0, 2.0), domain, 0),
        Some(domain)
    );
    assert_eq!(Interval::powi_preimage(iv(2.0, 3.0), domain, 0), None);
}

#[test]
fn sqrt_preimage_squares_the_non_negative_part() {
    let root = |p: f64| (p >= 0.0).then(|| p.sqrt());
    for (z, expected) in [(iv(1.0, 2.0), iv(1.0, 4.0)), (iv(-1.0, 2.0), iv(0.0, 4.0))] {
        let narrowed = Interval::sqrt_preimage(z, Interval::ENTIRE);
        assert_eq!(narrowed, Some(expected), "z {z:?}");
        assert_keeps_preimage(narrowed, Interval::ENTIRE, z, root);
    }
    assert_eq!(
        Interval::sqrt_preimage(iv(-3.0, -1.0), Interval::ENTIRE),
        None
    );
}

#[test]
fn abs_preimage_is_the_hull_of_both_branches() {
    let z = iv(1.0, 2.0);
    for (domain, expected) in [
        (Interval::ENTIRE, iv(-2.0, 2.0)),
        (non_negative(), iv(1.0, 2.0)),
    ] {
        let narrowed = Interval::abs_preimage(z, domain);
        assert_eq!(narrowed, Some(expected), "domain {domain:?}");
        assert_keeps_preimage(narrowed, domain, z, |p| Some(p.abs()));
    }
    assert_eq!(
        Interval::abs_preimage(iv(-2.0, -1.0), Interval::ENTIRE),
        None
    );
}

#[test]
fn product_preimage_divides_only_by_a_zero_free_factor() {
    let z = iv(2.0, 6.0);
    let narrowed = Interval::mul_preimage(z, Interval::ENTIRE, pt(2.0));
    assert_eq!(narrowed, Some(iv(1.0, 3.0)));
    assert_keeps_preimage(narrowed, Interval::ENTIRE, z, |p| Some(p * 2.0));

    let domain = iv(-10.0, 10.0);
    assert_eq!(
        Interval::mul_preimage(z, domain, iv(-1.0, 1.0)),
        Some(domain),
        "a factor containing 0 leaves the operand unchanged"
    );
    assert_eq!(Interval::mul_preimage(z, iv(10.0, 20.0), pt(2.0)), None);
}

#[test]
fn quotient_preimages_for_numerator_and_denominator() {
    let z = iv(1.0, 2.0);
    let numerator = Interval::div_numerator_preimage(z, Interval::ENTIRE, iv(2.0, 3.0));
    assert_eq!(numerator, Some(iv(2.0, 6.0)));
    assert_keeps_preimage(numerator, Interval::ENTIRE, z, |p| Some(p / 2.0));

    let denominator = Interval::div_denominator_preimage(iv(2.0, 3.0), Interval::ENTIRE, pt(6.0));
    assert_eq!(denominator, Some(iv(2.0, 3.0)));
    assert_keeps_preimage(denominator, Interval::ENTIRE, iv(2.0, 3.0), |p| {
        (p != 0.0).then(|| 6.0 / p)
    });

    let domain = iv(-5.0, 5.0);
    assert_eq!(
        Interval::div_denominator_preimage(iv(-1.0, 1.0), domain, pt(6.0)),
        Some(domain),
        "a quotient range containing 0 leaves the denominator unchanged"
    );
}

#[test]
fn min_and_max_preimages() {
    let z = iv(3.0, 5.0);
    assert_eq!(
        Interval::min_preimage(z, Interval::ENTIRE, Interval::ENTIRE),
        Some(iv(3.0, f64::INFINITY))
    );
    assert_eq!(
        Interval::min_preimage(z, Interval::ENTIRE, iv(6.0, 9.0)),
        Some(z),
        "the other operand cannot be the minimum, so this one is"
    );
    assert_eq!(
        Interval::min_preimage(z, iv(0.0, 2.0), Interval::ENTIRE),
        None
    );
    assert_keeps_preimage(
        Interval::min_preimage(z, Interval::ENTIRE, pt(4.0)),
        Interval::ENTIRE,
        z,
        |p| Some(p.min(4.0)),
    );

    assert_eq!(
        Interval::max_preimage(z, Interval::ENTIRE, Interval::ENTIRE),
        Some(iv(f64::NEG_INFINITY, 5.0))
    );
    assert_eq!(
        Interval::max_preimage(z, Interval::ENTIRE, iv(0.0, 2.0)),
        Some(z),
        "the other operand cannot be the maximum, so this one is"
    );
    assert_eq!(
        Interval::max_preimage(z, iv(6.0, 9.0), Interval::ENTIRE),
        None
    );
    assert_keeps_preimage(
        Interval::max_preimage(z, Interval::ENTIRE, pt(4.0)),
        Interval::ENTIRE,
        z,
        |p| Some(p.max(4.0)),
    );
}
