//! Active-branch tangents: the three shapes the source singles out as
//! load-bearing but no probe discriminated — a clamp bound that moves with the
//! seeds, a clamp exactly on either bound, and `mod` at a negative dividend.
//!
//! Each of these had full-looking coverage over operand values at which the
//! choice under test makes NO difference — so the code could have been written
//! the other way and every existing probe would still pass.

use reify_expr::{BranchRecord, EvalContext, Seeds, eval_dual, eval_expr};
use reify_ir::{BinOp, Value, ValueMap};
use reify_test_support::builders::expr::{binop, literal};
use reify_test_support::finite_difference::assert_column_matches_central_difference;

use crate::support::{calln, cell, central_difference, pref, probe};

// ---------------------------------------------------------------------------
// (1) A clamp bound that is itself a function of the seeds
// ---------------------------------------------------------------------------

#[test]
fn a_clamped_residual_outside_its_bounds_carries_the_bounds_own_derivative() {
    use reify_expr::{BranchChoice, KinkKind};

    // `select(value, &duals[i])` for i in {1, 2} is documented as giving "a
    // real tangent when the bound is itself a function of the seeds" — but
    // every clamp probe in the suite uses LITERAL bounds, where the adopted
    // tangent is `Tangent::Zero` and therefore indistinguishable from a
    // hardcoded zero row.  Here the bound moves.
    let (values, seed_cells) = probe(&[("q", 10.0), ("p", 2.0)]);
    let moving_bound = || binop(BinOp::Mul, literal(Value::Real(2.0)), pref("p"));

    for (label, expr, choice) in [
        // hi = 2p = 4, q = 10 is above it: the result IS the hi bound.
        (
            "AboveHi",
            calln("clamp", vec![pref("q"), literal(Value::Real(0.0)), moving_bound()]),
            BranchChoice::AboveHi,
        ),
        // lo = 2p = 4, q = 10 is below a hi of 100 but the lo now dominates.
        (
            "BelowLo",
            calln("clamp", vec![
                literal(Value::Real(-5.0)),
                moving_bound(),
                literal(Value::Real(100.0)),
            ]),
            BranchChoice::BelowLo,
        ),
    ] {
        let ctx = EvalContext::simple(&values);
        let seeds = Seeds::new(&seed_cells);
        let mut record = BranchRecord::new();
        let dual = eval_dual(&expr, &ctx, &seeds, &mut record);

        assert_eq!(dual.value, eval_expr(&expr, &ctx), "{label}: the primal invariant");
        assert_eq!(dual.value, Value::Real(4.0), "{label}: the result is the bound, 2p = 4");

        let clamp = record
            .entries()
            .iter()
            .find(|e| e.kind == KinkKind::Clamp)
            .unwrap_or_else(|| panic!("{label}: the clamp must be recorded"));
        assert_eq!(clamp.choice, choice, "{label}: the active region");

        let row = dual
            .tangent
            .materialize(2)
            .unwrap_or_else(|| panic!("{label}: a moving bound is differentiable"));
        assert_eq!(
            row,
            vec![0.0, 2.0],
            "{label}: flat in q (clamped away) and d(2p)/dp = 2 in p — a zero row here would \
             claim the residual cannot be moved at all"
        );
        // The independent reference, for the column that actually moves.  `q`'s
        // column is a true zero, which `assert_ad_matches_cd` deliberately
        // refuses to accept as a probe point, so only `p` is differenced.
        let cd = central_difference(&expr, &values, &seed_cells[1], &[]);
        assert_column_matches_central_difference(label, 1, row[1], cd);
    }
}

#[test]
fn a_clamp_exactly_on_either_bound_resolves_to_the_interior() {
    use reify_expr::{BranchChoice, KinkKind};

    // The tie-break at `xs[0] == xs[1]` and `xs[0] == xs[2]`.  `BelowLo` and
    // `AboveHi` are chosen with STRICT `<` / `>`, so a value sitting exactly on
    // a bound is `Interior` and keeps the argument's own tangent — a tie
    // resolved the other way would report the bound's derivative (here zero)
    // and read to λ as a different branch.  The min/max tie-break is argued for
    // at length and covered; this one was not.
    for (label, x) in [("on lo", 2.0), ("on hi", 5.0)] {
        let (values, seed_cells) = probe(&[("x", x)]);
        let expr = calln("clamp", vec![
            pref("x"),
            literal(Value::Real(2.0)),
            literal(Value::Real(5.0)),
        ]);
        let ctx = EvalContext::simple(&values);
        let seeds = Seeds::new(&seed_cells);
        let mut record = BranchRecord::new();
        let dual = eval_dual(&expr, &ctx, &seeds, &mut record);

        assert_eq!(dual.value, eval_expr(&expr, &ctx), "{label}: the primal invariant");
        let clamp = record
            .entries()
            .iter()
            .find(|e| e.kind == KinkKind::Clamp)
            .unwrap_or_else(|| panic!("{label}: the clamp must be recorded"));
        assert_eq!(clamp.choice, BranchChoice::Interior, "{label}: a tie is interior");
        assert_eq!(
            dual.tangent.materialize(1).unwrap_or_else(|| panic!("{label}: differentiable")),
            vec![1.0],
            "{label}: interior means the argument's own tangent, not the bound's zero"
        );
    }
}

// ---------------------------------------------------------------------------
// (2) `mod` at a NEGATIVE operand — the only place trunc and floor differ
// ---------------------------------------------------------------------------

#[test]
fn mod_at_a_negative_dividend_records_the_truncated_quotient_and_matches_its_tangent() {
    use reify_expr::{BranchChoice, KinkKind};

    // `mod_quotient_choice`'s doc says labelling with `floor` "would disagree
    // with the tangent — and mislabel the branch — for every negative operand",
    // yet every probe in the suite uses non-negative operands where
    // trunc == floor.  At (-7, 3) they part company: trunc(-7/3) = -2, so the
    // recorded cell is ModQuotient(-2) and d/db = -trunc(a/b) = +2.  With floor
    // it would be -3 and +3 — a self-consistent pair that is simply not what
    // reify's `%` computes.
    //
    // The two spellings need different operand TYPES: the `mod` builtin is
    // Int-only (`reify-stdlib/src/numeric.rs:89`), while `BinOp::Mod` goes
    // through the value-semantics helper.  Both are seeded, so both reach the
    // chain rule rather than the seed-independent fast path.
    let seed_cells = vec![cell("a"), cell("b")];
    let spellings = [
        ("BinOp", binop(BinOp::Mod, pref("a"), pref("b")), Value::Real(-1.0), -7.0, 3.0),
        (
            "builtin",
            calln("mod", vec![pref("a"), pref("b")]),
            Value::Int(-1),
            -7.0,
            3.0,
        ),
    ];
    for (label, expr, expected, a, b) in spellings {
        let mut values = ValueMap::new();
        if matches!(expected, Value::Int(_)) {
            values.insert(cell("a"), Value::Int(a as i64));
            values.insert(cell("b"), Value::Int(b as i64));
        } else {
            values.insert(cell("a"), Value::Real(a));
            values.insert(cell("b"), Value::Real(b));
        }
        let ctx = EvalContext::simple(&values);
        let seeds = Seeds::new(&seed_cells);
        let mut record = BranchRecord::new();
        let dual = eval_dual(&expr, &ctx, &seeds, &mut record);

        assert_eq!(dual.value, eval_expr(&expr, &ctx), "{label}: the primal invariant");
        assert_eq!(
            dual.value, expected,
            "{label}: -7 % 3 is the TRUNCATED remainder -1, not the floored +2"
        );

        let entry = record
            .entries()
            .iter()
            .find(|e| e.kind == KinkKind::Mod)
            .unwrap_or_else(|| panic!("{label}: the mod must be recorded"));
        assert_eq!(
            entry.choice,
            BranchChoice::ModQuotient(-2),
            "{label}: the truncated quotient cell; floor would say -3"
        );
        assert_eq!(
            dual.tangent.materialize(2).unwrap_or_else(|| panic!("{label}: differentiable")),
            vec![1.0, 2.0],
            "{label}: d/da = 1 and d/db = -trunc(a/b) = +2, the SAME quotient the record names"
        );
    }
}
