//! Forward-mode dual-number automatic differentiation — task #6672
//! (solver-unification ε).
//!
//! Design reference: `docs/prds/v0_6/geometry-algebra-solver-unification.md`
//! §7.7 ("Derivatives — three sources").  This file is a *separate test
//! binary*, so it exercises only the *published public surface* — the
//! `reify_expr` crate ROOT, which since the AD modules were made private is the
//! only path to them, so this is exactly the surface the downstream consumers
//! see: η (#6675, the Gauss-Newton/LM step and its
//! `‖Jᵀr‖` certificate), μ (#6680, the reduced gradient) and λ (#6679, kink
//! chatter detection).
//!
//! The chain rules themselves are stated once, in `dual_eval`, and are
//! exercised here only through the traversal that applies them: a rule that
//! no evaluation reaches is a rule nothing depends on.

#[path = "dual_number_ad_tests/support.rs"]
mod support;

#[path = "dual_number_ad_tests/traversal.rs"]
mod traversal;

#[path = "dual_number_ad_tests/central_differences.rs"]
mod central_differences;

#[path = "dual_number_ad_tests/user_functions.rs"]
mod user_functions;

#[path = "dual_number_ad_tests/refusals.rs"]
mod refusals;

#[path = "dual_number_ad_tests/field_reductions.rs"]
mod field_reductions;

#[path = "dual_number_ad_tests/contribution_masking.rs"]
mod contribution_masking;

use reify_core::{Type, ValueCellId};
use reify_expr::{BranchRecord, EvalContext, NonDifferentiable, Seeds, eval_dual, eval_expr};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};
use reify_test_support::builders::expr::{binop, literal};
use reify_test_support::finite_difference::assert_column_matches_central_difference;
use reify_test_support::sampled_field_fixtures::extremum_probe_field;

use reify_ir::CompiledFunction;
use reify_test_support::builders::expr::user_fn_call;
use reify_test_support::builders::function::compiled_function;

use support::{
    assert_close, assert_row_close, calln, cell, central_difference, dl, jrow, pref, probe,
};

// ===========================================================================
// Amendment: the three shapes the source singles out as load-bearing but no
// probe discriminated
// ===========================================================================
//
// Each of these had full-looking coverage over operand values at which the
// choice under test makes NO difference — so the code could have been written
// the other way and every existing probe would still pass.

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

// ===========================================================================
// Review follow-up: the undifferentiated-builtin primal
// ===========================================================================

#[test]
fn an_undifferentiated_builtin_that_eval_expr_intercepts_by_name_keeps_its_primal() {
    // For a name in neither derivative table the primal must still come from
    // `eval_expr`, because `eval_expr` owns a long by-name interception table
    // (`sample`, `gradient`, `von_mises`, `flat_map`, the field reductions,
    // `std::__interp_render`) that this module must never restate.  `gradient`
    // is one of those names and reaches the undifferentiated arm, so it pins
    // that the arm still routes through the interception rather than straight
    // to `reify_stdlib::eval_builtin` — which answers `Undef` for a Field.
    let mut values = ValueMap::new();
    values.insert(cell("f"), extremum_probe_field());
    let seed_cells = vec![cell("w")];
    values.insert(cell("w"), Value::Real(1.0));
    let expr = calln("gradient", vec![pref("f")]);

    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &seeds, &mut record);

    let plain = eval_expr(&expr, &ctx);
    assert!(
        !plain.is_undef(),
        "the premise: `eval_expr` INTERCEPTS `gradient` rather than handing it to \
         `eval_builtin`, which would answer Undef — without that this test proves nothing"
    );
    assert_eq!(dual.value, plain, "the primal invariant across the interception");
}

#[test]
fn nested_undifferentiated_builtins_keep_the_primal_invariant() {
    // Arguments are dual-evaluated before the undifferentiated arm is reached,
    // so that arm must not re-evaluate them — and substituting the already
    // computed primals must not change the answer at any nesting depth.
    let (values, seed_cells) = probe(&[("w", 2.0)]);
    let inner = calln("vec2", vec![pref("w"), literal(Value::Real(3.0))]);
    let expr = calln("norm", vec![inner]);

    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &seeds, &mut record);
    assert_eq!(dual.value, eval_expr(&expr, &ctx), "the primal invariant, nested two deep");
}

// ===========================================================================
// Step-37: the MIRRORING obligation on the `UserFunctionCall` arm
// ===========================================================================
//
// The module's one maintenance rule says: wherever `eval_expr` INTERCEPTS a
// call, `dual_eval` must intercept it in the same place, on the same operand
// shapes.  That rule was scoped to interception before
// `reify_stdlib::eval_builtin` — the field reductions — so nothing ever
// checked the FOUR families `eval_expr` intercepts before
// `eval_user_function_call`:
//
//   lib.rs:751  `solve_load_cases`/5 and /6
//   lib.rs:783  every `option_recovery::is_combinator` name
//   lib.rs:803  `map_or`/3
//   lib.rs:824  `map_err`/2
//
// `eval_dual_user_fn` mirrors none of them.  It descends the arguments, then
// applies a strict-`Undef` check the intercepted families deliberately do NOT
// have, then resolves the callee's `.ri` body — the exact route the intercepts
// exist to bypass (the pure `.ri` body is blocked by a grammar gap, PRD §4.4
// raf-12).  Two live failures follow, and the tests below discriminate both: a
// WRONG PRIMAL, and a residual whose dual-path value disagrees with the value
// Nelder-Mead computes at the same trial point — "two solvers disagreeing
// about the same model", the failure ε exists to prevent.

use std::collections::BTreeMap;

use reify_test_support::mm;

fn opt_len() -> Type {
    Type::Option(Box::new(Type::length()))
}

fn result_ok(v: Value) -> Value {
    Value::Enum {
        type_name: "Result".to_string(),
        variant: "Ok".to_string(),
        payload: vec![("value".to_string(), v)],
    }
}

/// The placeholder `pub fn unwrap_or` that `stdlib/option_recovery.ri`
/// declares — resolvable by name, arity and param types, with a body that
/// returns a value the intercept never can.  Reaching this body at all is the
/// defect: the intercept exists precisely because the `.ri` body route is
/// blocked.
fn placeholder_unwrap_or() -> CompiledFunction {
    CompiledFunction {
        is_pub: true,
        ..compiled_function(
            "unwrap_or",
            &[("subject", opt_len()), ("dflt", Type::length())],
            vec![],
            CompiledExpr::literal(mm(999.0), Type::length()),
        )
    }
}

#[test]
fn unwrap_or_with_an_undetermined_default_is_not_strict_on_the_dual_path() {
    // Recovery is SUBJECT-tag-driven, not strict-all-args: `unwrap_or(some(5mm),
    // undef)` is 5mm on the value path (lib.rs:780-782, and the CRITICAL note
    // in `option_recovery.rs`).  The dual path's own strict-`Undef` check fires
    // first today and answers `Undef` — a WRONG PRIMAL, not merely a lost
    // tangent.
    let expr = user_fn_call(
        "unwrap_or",
        vec![
            CompiledExpr::literal(Value::Option(Some(Box::new(mm(5.0)))), opt_len()),
            CompiledExpr::literal(Value::Undef, Type::length()),
        ],
        Type::length(),
    );
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&[]);
    let mut record = BranchRecord::new();

    assert_eq!(eval_expr(&expr, &ctx), mm(5.0), "precondition: the evaluator is non-strict here");
    assert_eq!(
        eval_dual(&expr, &ctx, &seeds, &mut record).value,
        mm(5.0),
        "the primal invariant: the dual path must not invent a strictness the evaluator lacks"
    );
}

#[test]
fn a_determined_option_combinator_does_not_evaluate_its_ri_body() {
    let fns = [placeholder_unwrap_or()];
    let values = ValueMap::new();
    let ctx = EvalContext::new(&values, &fns);
    let expr = user_fn_call(
        "unwrap_or",
        vec![
            CompiledExpr::literal(Value::Option(Some(Box::new(mm(5.0)))), opt_len()),
            CompiledExpr::literal(mm(0.0), Type::length()),
        ],
        Type::length(),
    );
    let seeds = Seeds::new(&[]);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &seeds, &mut record);

    assert_eq!(
        eval_expr(&expr, &ctx),
        mm(5.0),
        "precondition: the value-path intercept shadows the .ri body"
    );
    assert_ne!(
        dual.value,
        mm(999.0),
        "the dual path resolved and evaluated the placeholder .ri body the intercept exists to bypass"
    );
    assert_eq!(dual.value, eval_expr(&expr, &ctx), "the primal invariant");
}

#[test]
fn a_seed_independent_option_combinator_is_flat_rather_than_a_refusal() {
    // No argument can reach a seed, so the derivative exists and is exactly
    // zero.  A construct ε does not differentiate must not poison a residual it
    // cannot move — the `refuse_or_constant` contract.
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let expr = user_fn_call(
        "unwrap_or",
        vec![
            CompiledExpr::literal(
                Value::Option(Some(Box::new(Value::Real(5.0)))),
                Type::Option(Box::new(dl())),
            ),
            literal(Value::Real(0.0)),
        ],
        dl(),
    );
    let (primal, row) = jrow(&expr, &values, &seed_cells)
        .expect("a combinator no seed can reach is a CONSTANT, not a refusal");
    assert_close(primal, 5.0, "unwrap_or(some(5), 0)");
    assert_row_close(&row, &[0.0], "no seed reaches this residual");
}

#[test]
fn a_seed_dependent_option_combinator_refuses_and_names_itself() {
    // `unwrap_or(some(2x), 3)` MOVES with x, and ε has no chain rule for the
    // recovery combinators — so the honest answer is a refusal naming the
    // family, with the primal still intact.  What must never happen is a
    // finite row, or an `Undef` primal for a residual the evaluator values
    // perfectly well at the same trial point.
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let expr = user_fn_call(
        "unwrap_or",
        vec![
            CompiledExpr::option_some(
                binop(BinOp::Mul, pref("x"), literal(Value::Real(2.0))),
                Type::Option(Box::new(dl())),
            ),
            literal(Value::Real(3.0)),
        ],
        dl(),
    );
    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &seeds, &mut record);

    assert_eq!(eval_expr(&expr, &ctx), Value::Real(6.0), "precondition");
    assert_eq!(
        dual.value,
        eval_expr(&expr, &ctx),
        "the primal invariant holds even where the TANGENT refuses"
    );
    assert!(dual.tangent.is_none(), "a typed refusal, never a fabricated row");

    match jrow(&expr, &values, &seed_cells) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => assert!(
            kind.contains("combinator"),
            "the refusal must name the family responsible, got {kind:?}"
        ),
        other => panic!("expected a refusal naming the combinator family, got {other:?}"),
    }
}

#[test]
fn every_option_recovery_combinator_name_keeps_the_primal_invariant() {
    // Driven off `option_recovery::is_combinator`'s own table at its declared
    // arities — the `sync_drift_check_all_combinators_recognized` convention —
    // so a combinator added there later is covered here by construction.
    // Every input is chosen to produce a KNOWN non-`Undef` value, so the
    // invariant cannot pass vacuously by both paths answering `Undef`.
    let some_5mm = CompiledExpr::literal(Value::Option(Some(Box::new(mm(5.0)))), opt_len());
    let none_len = CompiledExpr::literal(Value::Option(None), opt_len());
    let zero_mm = CompiledExpr::literal(mm(0.0), Type::length());
    let ok_5mm = CompiledExpr::literal(result_ok(mm(5.0)), Type::length());
    let map_k = CompiledExpr::literal(
        Value::Map(BTreeMap::from([(Value::String("k".to_string()), mm(1.0))])),
        Type::length(),
    );
    let key_k = CompiledExpr::literal(Value::String("k".to_string()), Type::String);

    let cases: Vec<(&str, Vec<CompiledExpr>, Value)> = vec![
        ("unwrap_or", vec![some_5mm.clone(), zero_mm.clone()], mm(5.0)),
        ("or_default", vec![some_5mm.clone(), zero_mm.clone()], mm(5.0)),
        ("fallback", vec![some_5mm.clone(), zero_mm.clone()], mm(5.0)),
        ("or_else", vec![some_5mm.clone(), none_len.clone()], Value::Option(Some(Box::new(
            mm(5.0),
        )))),
        ("is_some", vec![some_5mm.clone()], Value::Bool(true)),
        ("is_none", vec![none_len.clone()], Value::Bool(true)),
        ("get_or", vec![map_k, key_k, zero_mm.clone()], mm(1.0)),
        ("is_ok", vec![ok_5mm.clone()], Value::Bool(true)),
        ("is_err", vec![ok_5mm.clone()], Value::Bool(false)),
        ("ok_or", vec![some_5mm.clone(), zero_mm.clone()], result_ok(mm(5.0))),
    ];

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    for (name, args, expected) in cases {
        let expr = user_fn_call(name, args, Type::length());
        assert_eq!(
            eval_expr(&expr, &ctx),
            expected,
            "{name}: precondition — the value-path intercept fires"
        );
        let seeds = Seeds::new(&[]);
        let mut record = BranchRecord::new();
        assert_eq!(
            eval_dual(&expr, &ctx, &seeds, &mut record).value,
            expected,
            "{name}: the primal invariant on the dual path"
        );
    }
}

#[test]
fn map_or_and_map_err_keep_the_primal_invariant_on_the_dual_path() {
    // The two ctx-aware arrow-type combinators, intercepted at lib.rs:803 and
    // lib.rs:824.  Both APPLY a lambda argument, which is why they sit outside
    // `is_combinator` and need their own gate entries — and why neither is
    // covered by the loop above.
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let double = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        vec![x_id.clone()],
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(x_id, Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            Type::Int,
        ),
        vec![],
        Type::Function { params: vec![Type::Int], return_type: Box::new(Type::Int) },
    );

    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&[]);

    // map_or(some(5), 99, |x| x*2) = 10 — the lambda is applied.
    let map_or_call = user_fn_call(
        "map_or",
        vec![
            CompiledExpr::literal(
                Value::Option(Some(Box::new(Value::Int(5)))),
                Type::Option(Box::new(Type::Int)),
            ),
            CompiledExpr::literal(Value::Int(99), Type::Int),
            double.clone(),
        ],
        Type::Int,
    );
    assert_eq!(eval_expr(&map_or_call, &ctx), Value::Int(10), "precondition: map_or intercept");
    let mut record = BranchRecord::new();
    assert_eq!(
        eval_dual(&map_or_call, &ctx, &seeds, &mut record).value,
        Value::Int(10),
        "map_or: the primal invariant on the dual path"
    );

    // map_err(Err{error: 5}, |e| e*2) = Err{error: 10}.
    let err_5 = Value::Enum {
        type_name: "Result".to_string(),
        variant: "Err".to_string(),
        payload: vec![("error".to_string(), Value::Int(5))],
    };
    let map_err_call = user_fn_call(
        "map_err",
        vec![CompiledExpr::literal(err_5, Type::Int), double],
        Type::Int,
    );
    let expected_map_err = eval_expr(&map_err_call, &ctx);
    assert_ne!(expected_map_err, Value::Undef, "precondition: map_err intercept fires");
    let mut record = BranchRecord::new();
    assert_eq!(
        eval_dual(&map_err_call, &ctx, &seeds, &mut record).value,
        expected_map_err,
        "map_err: the primal invariant on the dual path"
    );
}
