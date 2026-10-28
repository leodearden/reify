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

// ── HC4 over CompiledExpr ───────────────────────────────────────────────────
//
// Single-relation cases compile ONE leaf and revise it alone from an
// all-ENTIRE box; fixpoint cases contract a set of relations together.

mod hc4_over_compiled_exprs {
    use super::{Interval, iv};
    use reify_constraints::hc4::{
        Contraction, OperandClassifier, RefClass, Relation, Revision, Slack, contract,
    };
    use reify_core::scalar_tolerance::{LENGTH_FLOOR_M, scalars_equal};
    use reify_core::{DimensionVector, Type, ValueCellId};
    use reify_ir::{BinOp, CompiledExpr, CompiledFunction, ValueMap};
    use reify_test_support::builders::{
        binop, conditional_expr, eq, ge, gt, le, literal, lt, ne, neg, or, std_call,
        value_ref_typed,
    };
    use reify_test_support::values::{int_val, meters, real_val, vcid};

    const ACCEPTANCE: Slack = Slack::Acceptance { inequality: 1e-12 };

    fn real() -> Type {
        Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }
    }

    fn length() -> Type {
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }
    }

    fn real_ref(name: &str) -> CompiledExpr {
        value_ref_typed("m", name, real())
    }

    fn length_ref(name: &str) -> CompiledExpr {
        value_ref_typed("m", name, length())
    }

    fn num(v: f64) -> CompiledExpr {
        literal(real_val(v))
    }

    fn metres(v: f64) -> CompiledExpr {
        literal(meters(v))
    }

    /// Test classifier: named Variables by position, named Varying refs, and
    /// everything else Fixed, evaluated against an empty value map.
    struct Classes {
        variables: Vec<ValueCellId>,
        varying: Vec<ValueCellId>,
        values: ValueMap,
    }

    impl Classes {
        fn of(variables: &[&str]) -> Self {
            Self {
                variables: variables.iter().map(|n| vcid("m", n)).collect(),
                varying: Vec::new(),
                values: ValueMap::new(),
            }
        }

        fn varying(mut self, name: &str) -> Self {
            self.varying.push(vcid("m", name));
            self
        }
    }

    impl OperandClassifier for Classes {
        fn classify(&self, id: &ValueCellId) -> RefClass {
            if let Some(i) = self.variables.iter().position(|v| v == id) {
                RefClass::Variable(i)
            } else if self.varying.contains(id) {
                RefClass::Varying
            } else {
                RefClass::Fixed
            }
        }

        fn evaluate(&self, expr: &CompiledExpr) -> Option<f64> {
            let functions: [CompiledFunction; 0] = [];
            let ctx = reify_expr::EvalContext::new(&self.values, &functions);
            reify_expr::eval_expr(expr, &ctx)
                .as_f64()
                .filter(|v| v.is_finite())
        }
    }

    struct Revised {
        relation: Relation,
        revision: Revision,
        bx: Vec<Interval>,
    }

    fn revise_alone(leaf: &CompiledExpr, classes: &Classes, slack: Slack) -> Revised {
        let relation = Relation::compile(leaf, classes).expect("leaf compiles to a relation");
        let mut bx = vec![Interval::ENTIRE; classes.variables.len()];
        let revision = relation.revise(&mut bx, slack);
        Revised {
            relation,
            revision,
            bx,
        }
    }

    fn revise_x(leaf: &CompiledExpr) -> Interval {
        let revised = revise_alone(leaf, &Classes::of(&["x"]), Slack::Exact);
        assert_eq!(revised.revision, Revision::Consistent, "{leaf:?}");
        revised.bx[0]
    }

    #[test]
    fn scaled_mul_bounds_in_both_operand_orders() {
        let t = || length_ref("t");
        for leaf in [
            lt(binop(BinOp::Mul, num(2.0), t()), metres(0.03)),
            lt(binop(BinOp::Mul, t(), num(2.0)), metres(0.03)),
        ] {
            let revised = revise_alone(&leaf, &Classes::of(&["t"]), Slack::Exact);
            assert_eq!(revised.bx[0].hi(), 0.015, "exact rearrangement stays exact");
            assert_eq!(revised.bx[0].lo(), f64::NEG_INFINITY);
            assert!(revised.relation.is_strict());
        }
        let leaf = ge(binop(BinOp::Mul, t(), num(2.0)), metres(0.004));
        let revised = revise_alone(&leaf, &Classes::of(&["t"]), Slack::Exact);
        assert_eq!(revised.bx[0].lo(), 0.002);
        assert_eq!(revised.bx[0].hi(), f64::INFINITY);
        assert!(!revised.relation.is_strict());
    }

    #[test]
    fn quotient_bound() {
        let x = revise_x(&le(binop(BinOp::Div, real_ref("x"), num(2.0)), num(1.0)));
        assert_eq!(x.hi(), 2.0);
    }

    #[test]
    fn sqrt_clearance_algebra() {
        let x = || real_ref("x");
        let leaf = le(
            std_call(
                "sqrt",
                vec![binop(BinOp::Add, binop(BinOp::Mul, x(), x()), num(9.0))],
                real(),
            ),
            num(5.0),
        );
        let x = revise_x(&leaf);
        assert!((x.lo() + 4.0).abs() <= 1e-12 && x.lo() <= -4.0, "{x:?}");
        assert!((x.hi() - 4.0).abs() <= 1e-12 && x.hi() >= 4.0, "{x:?}");
        assert!(x.contains(3.999) && x.contains(-3.999), "{x:?}");
    }

    #[test]
    fn power_abs_min_max_clamp_and_negation() {
        let x = || real_ref("x");
        let pow = revise_x(&le(binop(BinOp::Pow, x(), literal(int_val(2))), num(4.0)));
        assert_eq!(pow, iv(-2.0, 2.0));

        let abs = revise_x(&le(
            std_call("abs", vec![binop(BinOp::Sub, x(), num(1.0))], real()),
            num(2.0),
        ));
        assert_eq!(abs, iv(-1.0, 3.0));

        let min = revise_x(&ge(std_call("min", vec![x(), num(10.0)], real()), num(3.0)));
        assert_eq!(min.lo(), 3.0);
        let max = revise_x(&le(std_call("max", vec![x(), num(1.0)], real()), num(4.0)));
        assert_eq!(max.hi(), 4.0);
        let clamp = revise_x(&ge(
            std_call("clamp", vec![x(), num(0.0), num(10.0)], real()),
            num(2.0),
        ));
        assert_eq!(clamp.lo(), 2.0);

        let negated = revise_x(&ge(neg(x()), num(-5.0)));
        assert_eq!(negated.hi(), 5.0);
    }

    #[test]
    fn equality_contracts_to_a_point_and_is_not_an_order_relation() {
        let revised = revise_alone(
            &eq(real_ref("x"), num(3.0)),
            &Classes::of(&["x"]),
            Slack::Exact,
        );
        assert_eq!(revised.bx[0], iv(3.0, 3.0));
        assert!(!revised.relation.is_strict());
        assert!(!revised.relation.is_order());

        let classes = Classes::of(&["x"]);
        for (leaf, strict) in [
            (ge(real_ref("x"), num(1.0)), false),
            (gt(real_ref("x"), num(1.0)), true),
            (le(real_ref("x"), num(1.0)), false),
            (lt(real_ref("x"), num(1.0)), true),
        ] {
            let relation = Relation::compile(&leaf, &classes).expect("order relation");
            assert!(relation.is_order(), "{leaf:?}");
            assert_eq!(relation.is_strict(), strict, "{leaf:?}");
        }
    }

    #[test]
    fn conditional_far_side_is_the_hull_of_its_branches() {
        let condition = value_ref_typed("m", "c", Type::Bool);
        let leaf = ge(
            real_ref("t"),
            conditional_expr(condition, num(5.0), num(3.0)),
        );
        let revised = revise_alone(&leaf, &Classes::of(&["t", "c"]), Slack::Exact);
        assert_eq!(
            revised.bx[0].lo(),
            3.0,
            "the hull's low end, not a branch snapshot"
        );
        assert!(
            revised.bx[1].is_entire(),
            "the condition is never contracted"
        );
    }

    #[test]
    fn opaque_operands_contract_nothing() {
        let classes = Classes::of(&["x"]).varying("v");
        let field = value_ref_typed("m", "samples", Type::List(Box::new(real())));
        for leaf in [
            le(std_call("foo", vec![real_ref("x")], real()), num(1.0)),
            le(real_ref("x"), real_ref("v")),
            le(
                std_call("min", vec![field, real_ref("x")], real()),
                num(1.0),
            ),
        ] {
            let revised = revise_alone(&leaf, &classes, Slack::Exact);
            assert_eq!(revised.revision, Revision::Consistent, "{leaf:?}");
            assert!(
                revised.bx[0].is_entire(),
                "{leaf:?} contracted {:?}",
                revised.bx[0]
            );
            assert!(!revised.relation.is_fully_representable(), "{leaf:?}");
        }
        let supported =
            Relation::compile(&le(real_ref("x"), num(1.0)), &classes).expect("compiles");
        assert!(supported.is_fully_representable());
    }

    #[test]
    fn non_relations_do_not_compile() {
        let classes = Classes::of(&["x"]);
        let x = || real_ref("x");
        for leaf in [
            ne(x(), num(5.0)),
            or(ge(x(), num(1.0)), le(x(), num(0.0))),
            binop(BinOp::Implies, ge(x(), num(1.0)), le(x(), num(0.0))),
            value_ref_typed("m", "flag", Type::Bool),
            eq(length_ref("x"), num(3.0)),
        ] {
            assert!(Relation::compile(&leaf, &classes).is_none(), "{leaf:?}");
        }
    }

    #[test]
    fn acceptance_slack_relaxes_an_order_relation_by_the_threshold() {
        let revised = revise_alone(
            &ge(real_ref("x"), num(1.0)),
            &Classes::of(&["x"]),
            ACCEPTANCE,
        );
        let lo = revised.bx[0].lo();
        assert!(lo <= 1.0 - 1e-12 && lo > 1.0 - 2e-12, "{lo:e}");
    }

    #[test]
    fn acceptance_slack_relaxes_an_equality_by_the_tolerance_policy() {
        let pin = 0.01;
        let leaf = eq(length_ref("x"), metres(pin));
        let accepted = revise_alone(&leaf, &Classes::of(&["x"]), ACCEPTANCE).bx[0];
        for inside in [pin - 0.99 * LENGTH_FLOOR_M, pin + 0.99 * LENGTH_FLOOR_M] {
            assert!(scalars_equal(inside, pin, DimensionVector::LENGTH));
            assert!(accepted.contains(inside), "{inside:e} ∉ {accepted:?}");
        }
        for outside in [pin - 2.0 * LENGTH_FLOOR_M, pin + 2.0 * LENGTH_FLOOR_M] {
            assert!(!accepted.contains(outside), "{outside:e} ∈ {accepted:?}");
        }
        let exact = revise_alone(&leaf, &Classes::of(&["x"]), Slack::Exact).bx[0];
        assert_eq!(exact, iv(pin, pin));
    }

    #[test]
    fn a_self_contradictory_leaf_revises_to_empty() {
        let x = || real_ref("x");
        let leaf = le(binop(BinOp::Mul, x(), x()), num(-1.0));
        let revised = revise_alone(&leaf, &Classes::of(&["x"]), Slack::Exact);
        assert_eq!(revised.revision, Revision::Empty);
    }

    // ── Multi-relation fixpoint contraction ─────────────────────────────────

    fn compile_all(leaves: &[CompiledExpr], classes: &Classes) -> Vec<Relation> {
        leaves
            .iter()
            .map(|leaf| Relation::compile(leaf, classes).expect("leaf compiles"))
            .collect()
    }

    fn contract_from(
        leaves: &[CompiledExpr],
        classes: &Classes,
        start: Vec<Interval>,
        slack: Slack,
    ) -> (Contraction, Vec<Interval>) {
        let relations = compile_all(leaves, classes);
        let mut bx = start;
        let outcome = contract(&relations, &mut bx, slack);
        (outcome, bx)
    }

    fn assert_contains_point(bx: &[Interval], point: &[f64]) {
        for (interval, &p) in bx.iter().zip(point) {
            assert!(interval.contains(p), "feasible {point:?} lost: {bx:?}");
        }
    }

    #[test]
    fn contraction_propagates_across_constraints() {
        let classes = Classes::of(&["x", "y"]);
        let (x, y) = (|| real_ref("x"), || real_ref("y"));
        let leaves = [
            ge(x(), num(1.0)),
            le(y(), binop(BinOp::Sub, num(5.0), x())),
            ge(y(), num(3.0)),
        ];
        let (outcome, bx) =
            contract_from(&leaves, &classes, vec![Interval::ENTIRE; 2], Slack::Exact);
        assert_eq!(outcome, Contraction::Consistent);
        assert!(bx[0].lo() >= 1.0 && bx[0].hi() <= 2.0, "x: {:?}", bx[0]);
        assert!(bx[1].lo() >= 3.0 && bx[1].hi() <= 4.0, "y: {:?}", bx[1]);
        assert_contains_point(&bx, &[1.5, 3.5]);
    }

    #[test]
    fn contradictory_pins_are_attributed_to_the_relation_that_emptied_the_box() {
        let classes = Classes::of(&["x"]);
        let ten = eq(length_ref("x"), metres(0.01));
        let twenty = eq(length_ref("x"), metres(0.02));
        for leaves in [[ten.clone(), twenty.clone()], [twenty, ten]] {
            let (outcome, _) = contract_from(&leaves, &classes, vec![Interval::ENTIRE], ACCEPTANCE);
            assert_eq!(outcome, Contraction::Empty { relation: 1 }, "{leaves:?}");
        }
    }

    #[test]
    fn opposed_strict_bounds_are_empty() {
        let classes = Classes::of(&["x"]);
        let leaves = [gt(real_ref("x"), num(5.0)), lt(real_ref("x"), num(1.0))];
        let (outcome, _) = contract_from(&leaves, &classes, vec![Interval::ENTIRE], Slack::Exact);
        assert_eq!(outcome, Contraction::Empty { relation: 1 });
    }

    #[test]
    fn acceptance_slack_agrees_with_the_solver_on_order_rows() {
        let classes = Classes::of(&["x"]);
        let leaves = [
            ge(real_ref("x"), num(1.0)),
            le(real_ref("x"), num(1.0 - 1e-13)),
        ];
        let (exact, _) = contract_from(&leaves, &classes, vec![Interval::ENTIRE], Slack::Exact);
        assert_eq!(exact, Contraction::Empty { relation: 1 });
        let (accepted, bx) = contract_from(&leaves, &classes, vec![Interval::ENTIRE], ACCEPTANCE);
        assert_eq!(accepted, Contraction::Consistent);
        assert_contains_point(&bx, &[1.0 - 0.5e-13]);
    }

    #[test]
    fn acceptance_slack_agrees_with_the_solver_on_equality_rows() {
        let classes = Classes::of(&["x"]);
        let pin = 0.01;
        let near = pin + 0.5 * LENGTH_FLOOR_M;
        assert!(scalars_equal(pin, near, DimensionVector::LENGTH));
        let near_pair = [
            eq(length_ref("x"), metres(pin)),
            eq(length_ref("x"), metres(near)),
        ];
        let (exact, _) = contract_from(&near_pair, &classes, vec![Interval::ENTIRE], Slack::Exact);
        assert_eq!(exact, Contraction::Empty { relation: 1 });
        let (accepted, bx) =
            contract_from(&near_pair, &classes, vec![Interval::ENTIRE], ACCEPTANCE);
        assert_eq!(accepted, Contraction::Consistent);
        assert_contains_point(&bx, &[pin]);

        let far = pin + 2.0 * LENGTH_FLOOR_M;
        assert!(!scalars_equal(pin, far, DimensionVector::LENGTH));
        let far_pair = [
            eq(length_ref("x"), metres(pin)),
            eq(length_ref("x"), metres(far)),
        ];
        let (refuted, _) = contract_from(&far_pair, &classes, vec![Interval::ENTIRE], ACCEPTANCE);
        assert_eq!(refuted, Contraction::Empty { relation: 1 });
    }

    #[test]
    fn contraction_runs_on_a_caller_supplied_sub_box() {
        let classes = Classes::of(&["x"]);
        let x = || real_ref("x");
        let leaves = [ge(binop(BinOp::Mul, x(), x()), num(1.0))];
        let (outcome, _) = contract_from(&leaves, &classes, vec![iv(0.0, 0.5)], Slack::Exact);
        assert_eq!(outcome, Contraction::Empty { relation: 0 });
        let (outcome, bx) = contract_from(&leaves, &classes, vec![iv(0.0, 2.0)], Slack::Exact);
        assert_eq!(outcome, Contraction::Consistent);
        assert_eq!(bx[0], iv(1.0, 2.0));
    }

    #[test]
    fn slow_convergence_terminates_on_its_budget_deterministically() {
        let classes = Classes::of(&["x", "y"]);
        let half_plus_one = |name: &str| {
            binop(
                BinOp::Add,
                binop(BinOp::Div, real_ref(name), num(2.0)),
                num(1.0),
            )
        };
        let leaves = [
            eq(real_ref("x"), half_plus_one("y")),
            eq(real_ref("y"), half_plus_one("x")),
        ];
        let start = vec![iv(0.0, 10.0); 2];
        let (first, first_box) = contract_from(&leaves, &classes, start.clone(), Slack::Exact);
        let (second, second_box) = contract_from(&leaves, &classes, start, Slack::Exact);
        assert_eq!(first, Contraction::Consistent);
        assert_contains_point(&first_box, &[2.0, 2.0]);
        assert!(
            first_box[0].hi() - first_box[0].lo() < 10.0,
            "it contracted: {first_box:?}"
        );
        assert_eq!((first, first_box), (second, second_box), "deterministic");
    }
}
