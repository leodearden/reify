//! Contribution masking: a partial the chain rule never reads must not veto a
//! row, while a moving argument still refuses — and the two spellings of a
//! power agree.
//!
//! Every refusal in `dual_eval` is MASKED BY CONTRIBUTION — a partial the chain
//! rule never reads cannot take out a derivative that exists.  Three guards
//! broke that rule, and each block below pins one fix together with its
//! negative control: the mask is deliberately NOT a blanket amnesty, so a
//! non-finite partial, or a kink, on a MOVING argument must still refuse.
//!
//! THE `pow` FINITENESS GUARD.
//! `eval_dual_builtin` refuses the whole row whenever ANY entry of
//! `builtin_partials` is non-finite.  For `pow` the exponent partial is
//! `x^y·ln x`, which does not exist for `x <= 0` — so `pow(w, 2)` at a NEGATIVE
//! or ZERO base refuses, even though `dR/dw = 2w` is perfectly finite and
//! `combine` already skips the constant exponent's zero tangent so the offending
//! partial is multiplied by nothing at all.
//!
//! w = 0.0 is the commonest initial guess a solver ever starts from and w < 0 is
//! reachable at any Newton step, so this is not an exotic corner: it takes out
//! the whole Jacobian of any residual containing a squared unknown written as
//! `pow(w, 2)`.
//!
//! The fix has two independent halves, and both are pinned here:
//!
//!   (1) mask the guard by which arguments actually CARRY a tangent, so the
//!       guard can never refuse over a partial `combine` provably discards;
//!   (2) mirror `pow_tangent`'s constant-exponent identity — `d/dx (x^0) = 0`
//!       exactly — into `builtin_partials`, which masking alone cannot reach
//!       because that partial belongs to the argument that DOES move.
//!
//! THE `abs` KINK GUARD.  The `pow` fix masked the SMOOTH-builtin finiteness
//! guard by which arguments actually carry a tangent.  `abs` is the one kink arm
//! that was left refusing unconditionally: it fires whenever `xs[0] == 0.0`,
//! with no check that the argument contributes anything, so it can veto a
//! derivative that provably exists — the exact failure `contributes`' own doc
//! forbids ("a partial the chain rule never reads must not be able to veto a
//! derivative that exists").
//!
//! It is reachable for a SEED-INDEPENDENT `abs`.  The fast path only swallows a
//! subtree that hides no kink, and `abs` is a kink builtin, so a constant
//! `abs(...)` is always descended into; the kink dispatch then runs BEFORE the
//! all-zero-tangent shortcut.  A residual containing `abs(offset)` where
//! `offset` is a base-map constant that happens to be exactly 0.0 — a
//! symmetric-tolerance default, a zeroed eccentricity, both routine in
//! engineering models — therefore returns `Tangent::None`, the parent
//! propagates opaque, and `jacobian_row` fails the WHOLE row: η gets a tier-2
//! refusal for a residual that is differentiable in every seeded variable.
//!
//! A CONSTANT NON-SCALAR OPERAND.  The kink builtins' "operand is not a scalar"
//! early-outs were the exception, and `abs` is where that exception bites:
//! `eval_builtin("abs", [Complex])` returns a finite modulus while
//! `Value::Complex::as_f64()` is `None`, so a CONSTANT complex operand made the
//! node `Tangent::None` and condemned the whole row.

use reify_core::DimensionVector;
use reify_expr::{BranchRecord, EvalContext, NonDifferentiable, Seeds, Tangent, eval_expr};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};
use reify_test_support::builders::expr::{binop, literal};

use crate::support::{
    assert_ad_matches_cd, assert_close, assert_row_close, calln, cell, jrow, pref, probe,
};

// ===========================================================================
// The `pow` finiteness guard, and the two spellings of a power
// ===========================================================================

/// The `pow(x, k)` FunctionCall spelling and the `x ^ k` BinOp spelling of the
/// same power, as an `(ad_tangent, primal)` pair each, seeded on `x` alone.
fn both_pow_spellings(x0: f64, k: i64) -> ((f64, f64), (f64, f64)) {
    let (values, seed_cells) = probe(&[("x", x0)]);
    let exponent = literal(Value::Int(k));
    let call = calln("pow", vec![pref("x"), exponent.clone()]);
    let binop_form = binop(BinOp::Pow, pref("x"), exponent);

    let unwrap_one = |label: &str, e: &CompiledExpr| -> (f64, f64) {
        let (primal, row) = jrow(e, &values, &seed_cells)
            .unwrap_or_else(|err| panic!("{label} at x={x0}, k={k}: refused with {err:?}"));
        assert_eq!(row.len(), 1, "{label}: one seed, one column");
        (primal, row[0])
    };
    (unwrap_one("pow(x, k)", &call), unwrap_one("x ^ k", &binop_form))
}

// ---------------------------------------------------------------------------
// (1) A constant exponent must not be refused over a base it never reads
// ---------------------------------------------------------------------------

#[test]
fn pow_with_a_constant_exponent_differentiates_at_a_negative_base() {
    // `powf` returns 9.0 for an integral exponent at a negative base (IEEE
    // 754-2008 §9.2.1), so the PRIMAL is an ordinary 9.0 — nothing about this
    // point is undefined.  Only the unread `x^y·ln x` column is NaN.
    let ((primal, ad), _) = both_pow_spellings(-3.0, 2);
    assert_eq!(primal, 9.0, "pow(-3, 2) is an ordinary finite primal");
    assert_close(ad, -6.0, "d(x²)/dx at x = −3");
}

#[test]
fn pow_with_a_constant_exponent_differentiates_at_a_zero_base() {
    // x = 0 is the commonest initial guess there is; `0^2·ln 0` = `0·−inf` = NaN.
    let ((primal, ad), _) = both_pow_spellings(0.0, 2);
    assert_eq!(primal, 0.0, "pow(0, 2) is an ordinary finite primal");
    assert_eq!(ad, 0.0, "d(x²)/dx at x = 0 is exactly zero");
}

#[test]
fn pow_with_an_odd_constant_exponent_at_a_negative_base_agrees_with_central_differences() {
    // ∂(x³)/∂x = 3x² = 12 at x = −2, comfortably clear of the 0.1 floor.
    let (values, seed_cells) = probe(&[("x", -2.0)]);
    let expr = calln("pow", vec![pref("x"), literal(Value::Int(3))]);
    assert_ad_matches_cd("pow(x, 3) at x = -2", &expr, &values, &seed_cells);
}

// ---------------------------------------------------------------------------
// (2) The two spellings of one power are the SAME function
// ---------------------------------------------------------------------------

#[test]
fn the_call_and_binop_spellings_of_a_constant_power_produce_bit_identical_tangents() {
    // `pow_tangent` (the BinOp path) already carries the constant-exponent
    // identity; the FunctionCall path never got it.  Nothing about a residual's
    // gradient may depend on which of two synonyms the author typed, so this is
    // asserted on the BITS, not within a tolerance.
    for k in [2_i64, 3] {
        for x0 in [-3.0_f64, 0.0, 2.0] {
            let ((call_primal, call_ad), (binop_primal, binop_ad)) = both_pow_spellings(x0, k);
            assert_eq!(
                call_primal.to_bits(),
                binop_primal.to_bits(),
                "pow(x, {k}) vs x ^ {k} at x = {x0}: primals must agree bit-for-bit"
            );
            assert_eq!(
                call_ad.to_bits(),
                binop_ad.to_bits(),
                "pow(x, {k}) vs x ^ {k} at x = {x0}: tangents must agree bit-for-bit \
                 (got {call_ad:?} vs {binop_ad:?})"
            );
        }
    }
}

#[test]
fn a_zero_exponent_collapses_to_an_exactly_zero_tangent_in_both_spellings() {
    // `d/dx (x^0) = d/dx (1) = 0` exactly, for EVERY x including 0 — whereas
    // the general base partial `y·x^(y−1)` evaluates `0 · 0^(−1)` = `0 · inf`
    // = NaN.  Masking cannot rescue this one: the NaN sits on the argument that
    // genuinely moves, so `builtin_partials` has to state the identity itself.
    for x0 in [-3.0_f64, 0.0, 2.0] {
        let ((call_primal, call_ad), (binop_primal, binop_ad)) = both_pow_spellings(x0, 0);
        assert_eq!(call_primal, 1.0, "x⁰ = 1 at x = {x0}");
        assert_eq!(binop_primal, 1.0, "x⁰ = 1 at x = {x0}");
        assert_eq!(call_ad, 0.0, "d(x⁰)/dx is exactly zero at x = {x0}");
        assert_eq!(
            call_ad.to_bits(),
            binop_ad.to_bits(),
            "pow(x, 0) vs x ^ 0 at x = {x0}: bit-identical zero tangents"
        );
    }
}

// ---------------------------------------------------------------------------
// (3) The mask is not a blanket amnesty
// ---------------------------------------------------------------------------

#[test]
fn a_non_finite_partial_on_a_moving_argument_still_refuses() {
    // At arity 1 the mask is a PROVABLE no-op: the all-zero-tangent case has
    // already returned `DualValue::constant` before the guard runs, so the sole
    // argument always contributes.  These three keep refusing byte for byte.
    let unary_refusals: [(&str, f64); 2] = [("sqrt", 0.0), ("asin", 1.0)];
    for (name, x0) in unary_refusals {
        let (values, seed_cells) = probe(&[("x", x0)]);
        let expr = calln(name, vec![pref("x")]);
        match jrow(&expr, &values, &seed_cells) {
            Err(NonDifferentiable::UnsupportedKind { kind, .. }) => assert_eq!(
                kind, "a builtin evaluated where its derivative does not exist",
                "{name}({x0}) must keep naming the derivative, not the primal"
            ),
            other => panic!("{name}({x0}): expected UnsupportedKind, got {other:?}"),
        }
    }

    // A MULTI-argument case where the mask is genuinely live and must not fire:
    // both base and exponent are seeded, so `x^y·ln x` belongs to an argument
    // that really does move, and `ln(−3)` really does not exist.
    let (values, seed_cells) = probe(&[("x", -3.0), ("y", 2.0)]);
    let expr = calln("pow", vec![pref("x"), pref("y")]);
    let ctx = EvalContext::simple(&values);
    assert_eq!(
        eval_expr(&expr, &ctx),
        Value::Real(9.0),
        "the primal is finite — that is exactly what makes a silent zero row dangerous here"
    );
    match jrow(&expr, &values, &seed_cells) {
        Err(NonDifferentiable::UnsupportedKind { .. }) => {}
        other => {
            panic!("pow(x, y) with both seeded at x = −3: expected UnsupportedKind, got {other:?}")
        }
    }
}

#[test]
fn a_builtin_whose_primal_is_undef_refuses_at_the_primal_cliff_regardless_of_the_mask() {
    // The other half of "not an amnesty", and a deliberate correction to the
    // naive reading that every non-finite-derivative point surfaces as
    // `UnsupportedKind`: `log(0)` = −inf and a degenerate `remap` range both
    // SANITIZE to `Value::Undef` (`reify-expr/src/sanitize.rs:24`), so the
    // Undef cliff fires strictly BEFORE the partials are ever computed.  The
    // masked guard is therefore unreachable for these, which is the point —
    // whichever refusal arrives first, a refusal is what must arrive.
    let (values, seed_cells) = probe(&[("x", 0.0)]);
    let log0 = calln("log", vec![pref("x")]);
    match jrow(&log0, &values, &seed_cells) {
        Err(NonDifferentiable::UndefPrimal { .. }) => {}
        other => panic!("log(0): expected UndefPrimal, got {other:?}"),
    }

    // remap with flo == fhi: every partial, ∂/∂x included, is ±inf or NaN.
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    let degenerate = calln("remap", vec![
        pref("x"),
        literal(Value::Real(1.0)),
        literal(Value::Real(1.0)),
        literal(Value::Real(0.0)),
        literal(Value::Real(10.0)),
    ]);
    match jrow(&degenerate, &values, &seed_cells) {
        Err(NonDifferentiable::UndefPrimal { .. }) => {}
        other => {
            panic!("remap with a degenerate source range: expected UndefPrimal, got {other:?}")
        }
    }
}

// ---------------------------------------------------------------------------
// `exponent_is_constant` must not be a claim about the tangent's REPRESENTATION
// ---------------------------------------------------------------------------

#[test]
fn an_all_zero_exponent_row_still_takes_the_constant_exponent_power_rule() {
    use reify_expr::{DualEnv, jacobian_row_with_env};

    // `Tangent::from_row` collapses an all-zero row to `Tangent::Zero`, so
    // inside the traversal `is_zero()` and "is provably flat" coincide.
    // `DualEnv::bind` is PUBLIC and stores what the caller hands it, so the
    // overlay is the one place a `Tangent::Scalar(vec![0.0; w])` can enter —
    // and reading `is_zero()` off it asks about the REPRESENTATION rather than
    // the value.  Answering "the exponent moves" for a provably flat exponent
    // drives `x ^ k` into the general `a^b·(b'·ln a + b·a'/a)` form, whose
    // `ln a` is NaN at a NEGATIVE base — turning d/dx (−3)² = −6, a perfectly
    // finite derivative, into a refusal.
    let mut values = ValueMap::new();
    values.insert(cell("x"), Value::Real(-3.0));
    values.insert(cell("k"), Value::Real(2.0));
    let seed_cells = vec![cell("x")];
    let expr = binop(BinOp::Pow, pref("x"), pref("k"));

    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&seed_cells);
    let mut env = DualEnv::new();
    env.bind(cell("k"), Tangent::Scalar(vec![0.0]));
    let mut record = BranchRecord::new();

    let (primal, row) = jacobian_row_with_env(&expr, &ctx, &seeds, &env, &mut record)
        .expect("a flat exponent is a CONSTANT exponent, however it is spelled");
    assert_close(primal, 9.0, "(-3)^2");
    assert_row_close(&row, &[-6.0], "d/dx x^2 at x = -3 is 2x = -6");
}

// ---------------------------------------------------------------------------
// The seeded-exponent branch of `pow_tangent`, which no test reached
// ---------------------------------------------------------------------------

#[test]
fn a_seeded_exponent_agrees_with_central_differences_in_both_spellings() {
    // Every `Pow` in this suite had a literal exponent, so
    // `exponent_is_constant` was always true and the general
    // `a^b·(b'·ln a + b·a'/a)` form was dead code.  At (2, 3):
    // ∂/∂x = y·x^(y−1) = 12 and ∂/∂y = x^y·ln x ≈ 5.545, both comfortably
    // above the 0.1 floor the relative tolerance needs.
    let (values, seed_cells) = probe(&[("x", 2.0), ("y", 3.0)]);
    assert_ad_matches_cd(
        "x ^ y, BinOp spelling, both seeded",
        &binop(BinOp::Pow, pref("x"), pref("y")),
        &values,
        &seed_cells,
    );
    assert_ad_matches_cd(
        "pow(x, y), call spelling, both seeded",
        &calln("pow", vec![pref("x"), pref("y")]),
        &values,
        &seed_cells,
    );
}

#[test]
fn a_seeded_exponent_over_a_negative_base_refuses_in_both_spellings() {
    // `ln a` does not exist at a negative base, so the general form has no
    // value to report.  THE POINT IS THAT NEITHER SPELLING RETURNS A ROW: the
    // two refuse through different channels — the call path's
    // contribution-masked finiteness guard names the construct and its site,
    // while the BinOp path's NaN is caught by `jacobian_row`'s root-level
    // column scan — and that division of labour is deliberate, not a gap.
    // What would be a defect is one of them answering `Ok` with a NaN in it.
    let (values, seed_cells) = probe(&[("x", -3.0), ("y", 2.0)]);
    for (label, expr) in [
        ("BinOp", binop(BinOp::Pow, pref("x"), pref("y"))),
        ("call", calln("pow", vec![pref("x"), pref("y")])),
    ] {
        match jrow(&expr, &values, &seed_cells) {
            Err(err) => assert!(
                !err.to_string().is_empty(),
                "{label}: the refusal must carry a message η can show"
            ),
            Ok((p, row)) => {
                panic!("{label}: expected a refusal, got primal {p:?} row {row:?}")
            }
        }
    }
}

// ===========================================================================
// The `abs` kink guard
// ===========================================================================

// ---------------------------------------------------------------------------
// (1) A constant `abs` at the origin must not veto the row it never touches
// ---------------------------------------------------------------------------

#[test]
fn a_constant_abs_at_the_origin_still_yields_a_full_row() {
    // `abs(0) + w`, seeded on `w` alone.  ∂R/∂w = 1 exactly; the `abs` node
    // contributes nothing to that column and must not be able to withhold it.
    let (values, seed_cells) = probe(&[("w", 0.0)]);
    let expr = binop(BinOp::Add, calln("abs", vec![literal(Value::Real(0.0))]), pref("w"));
    let (primal, row) = jrow(&expr, &values, &seed_cells)
        .unwrap_or_else(|err| panic!("abs(0) + w: refused with {err:?}"));
    assert_eq!(primal, 0.0, "the primal is an ordinary 0.0 — nothing here is undefined");
    assert_eq!(row, vec![1.0], "∂(abs(0) + w)/∂w = 1");
}

#[test]
fn a_constant_abs_at_the_origin_nested_a_level_down_still_yields_a_full_row() {
    // `w · (abs(0) + 1)` — the same node in a different syntactic position, so
    // the fix is a property of the NODE and not of one shape.  ∂R/∂w = 1.
    let (values, seed_cells) = probe(&[("w", 0.0)]);
    let inner =
        binop(BinOp::Add, calln("abs", vec![literal(Value::Real(0.0))]), literal(Value::Real(1.0)));
    let expr = binop(BinOp::Mul, pref("w"), inner);
    let (primal, row) = jrow(&expr, &values, &seed_cells)
        .unwrap_or_else(|err| panic!("w * (abs(0) + 1): refused with {err:?}"));
    assert_eq!(primal, 0.0, "0 · 1");
    assert_eq!(row, vec![1.0], "∂(w·(abs(0) + 1))/∂w = abs(0) + 1 = 1");
}

#[test]
fn a_constant_abs_has_no_cliff_at_zero() {
    // What "a constant contributes nothing" MEANS: the row does not depend on
    // where the constant argument sits relative to the kink.  A constant 0 and
    // a constant 2 must produce the SAME row — anything else is a cliff in the
    // Jacobian at a point no seeded variable can even move through.
    let (values, seed_cells) = probe(&[("w", 0.0)]);
    let at_kink = binop(BinOp::Add, calln("abs", vec![literal(Value::Real(0.0))]), pref("w"));
    let away = binop(BinOp::Add, calln("abs", vec![literal(Value::Real(2.0))]), pref("w"));

    let (_, row_at_kink) = jrow(&at_kink, &values, &seed_cells)
        .unwrap_or_else(|err| panic!("abs(0) + w: refused with {err:?}"));
    let (_, row_away) = jrow(&away, &values, &seed_cells)
        .unwrap_or_else(|err| panic!("abs(2) + w: refused with {err:?}"));
    assert_eq!(
        row_at_kink, row_away,
        "a constant argument contributes nothing, so the kink's position cannot move the row"
    );
}

// ---------------------------------------------------------------------------
// (2) The mask is not a blanket amnesty
// ---------------------------------------------------------------------------

#[test]
fn a_seeded_abs_sitting_exactly_on_its_kink_still_refuses() {
    // The argument genuinely moves, so there genuinely is no two-sided
    // derivative.  A one-sided derivative dressed up as two-sided is exactly
    // what the refusal prevents, and masking must not reach this case.
    const ABS_KINK: &str = "`abs` evaluated exactly at its kink (x = 0)";

    let (values, seed_cells) = probe(&[("x", 0.0)]);
    let direct = calln("abs", vec![pref("x")]);
    match jrow(&direct, &values, &seed_cells) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            assert_eq!(kind, ABS_KINK, "abs(x) at x = 0 must keep naming the abs kink");
        }
        other => panic!("abs(x) at x = 0: expected UnsupportedKind, got {other:?}"),
    }

    // And through a subtraction: seeded `x` sitting exactly ON the constant it
    // is measured against is the commonest way a solver arrives at the kink.
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let shifted = calln("abs", vec![binop(BinOp::Sub, pref("x"), literal(Value::Real(3.0)))]);
    match jrow(&shifted, &values, &seed_cells) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            assert_eq!(kind, ABS_KINK, "abs(x − 3) at x = 3 must keep naming the abs kink");
        }
        other => panic!("abs(x - 3) at x = 3: expected UnsupportedKind, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (3) `abs` is the lone outlier — pin that the other kinks stay that way
// ---------------------------------------------------------------------------

#[test]
fn every_other_all_constant_kink_already_yields_a_full_row() {
    // `min`/`max`/`clamp` adopt the chosen constant operand's `Tangent::Zero`
    // through `select`, `floor` returns a `DualValue::constant` outright, and
    // `mod` goes through `combine` — none of them can withhold a row.  They do
    // this today; this pins that they keep doing it, so the step-30 mask cannot
    // be read as "abs was special" and quietly regress elsewhere.
    let (values, seed_cells) = probe(&[("w", 0.0)]);
    let c = |v: f64| literal(Value::Real(v));
    // `mod` is the one Int-only stdlib binding (numeric.rs:89), so it gets Int
    // constants; 7 mod 3 = 1, which is also why the expected primal is per-case.
    let cases: [(&str, CompiledExpr, f64); 5] = [
        ("min(0, 0)", calln("min", vec![c(0.0), c(0.0)]), 0.0),
        ("max(0, 0)", calln("max", vec![c(0.0), c(0.0)]), 0.0),
        ("clamp(0, 0, 1)", calln("clamp", vec![c(0.0), c(0.0), c(1.0)]), 0.0),
        ("floor(0)", calln("floor", vec![c(0.0)]), 0.0),
        ("mod(7, 3)", calln("mod", vec![literal(Value::Int(7)), literal(Value::Int(3))]), 1.0),
    ];
    for (label, kink, expected_primal) in cases {
        let expr = binop(BinOp::Add, kink, pref("w"));
        let (primal, row) = jrow(&expr, &values, &seed_cells)
            .unwrap_or_else(|err| panic!("{label} + w: refused with {err:?}"));
        assert_eq!(primal, expected_primal, "{label} + w at w = 0");
        assert_eq!(row, vec![1.0], "{label} + w: ∂/∂w = 1");
    }
}

// ===========================================================================
// A constant complex operand of `abs`
// ===========================================================================

#[test]
fn a_constant_complex_abs_does_not_veto_an_otherwise_differentiable_row() {
    // r(w) = w − |z|, z a compile-time constant Complex.  |3+4i| = 5, so
    // r = w − 5 with ∂r/∂w = 1 — nothing about this row is in doubt.
    let (values, seed_cells) = probe(&[("w", 2.0)]);
    let z = literal(Value::Complex {
        re: 3.0,
        im: 4.0,
        dimension: DimensionVector::DIMENSIONLESS,
    });
    let expr = binop(BinOp::Sub, pref("w"), calln("abs", vec![z]));

    let (primal, row) = jrow(&expr, &values, &seed_cells)
        .expect("a constant complex operand is not a reason to refuse the row");
    assert_close(primal, -3.0, "r = 2 − |3+4i| = −3");
    assert_row_close(&row, &[1.0], "∂r/∂w = 1 regardless of what |z| is");
}

#[test]
fn a_seeded_complex_abs_still_refuses_because_it_really_is_unresolved() {
    // The mirror direction: once the complex operand CONTRIBUTES, `as_f64`
    // failing is a genuine refusal and must stay one.  `re(..)` is not
    // differentiated, so a seeded cell holding the Complex is the shape that
    // reaches `abs` with a live tangent.
    let mut values = ValueMap::new();
    values.insert(cell("z"), Value::Complex {
        re: 3.0,
        im: 4.0,
        dimension: DimensionVector::DIMENSIONLESS,
    });
    let seed_cells = vec![cell("z")];
    let expr = calln("abs", vec![pref("z")]);

    let err = jrow(&expr, &values, &seed_cells)
        .expect_err("a SEEDED complex operand has no scalar tangent to offer");
    let msg = err.to_string();
    assert!(
        msg.contains("non-scalar"),
        "the refusal must name the seeded non-scalar cell, not a generic fallback: {msg}"
    );
}
