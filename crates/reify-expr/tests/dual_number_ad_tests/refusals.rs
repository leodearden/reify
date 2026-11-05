//! The refusal is LOUD, never a silent zero row.
//!
//! PRD §7.7: "Non-numeric operands stop contributing phantom gradients."
//! INV-SF-7: "a well-typed wrong value is the worst shape."
//!
//! `jacobian_row` is the boundary where a tangent becomes a number η can put in
//! a matrix.  A zero row is a CLAIM — "this residual does not move when you
//! move this variable" — and when the truth is "we could not tell", that claim
//! is false in the most damaging possible way: the solver believes the residual
//! is already stationary in that direction and stops pushing on it.  So every
//! path that cannot produce a derivative returns a typed refusal naming the
//! construct responsible, which η turns into a tier-2 refusal instead of
//! stalling on a gradient it has no reason to trust.

use reify_core::DimensionVector;
use reify_expr::{EvalContext, NonDifferentiable, eval_expr};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};
use reify_test_support::builders::expr::{binop, conditional_expr, literal};
use reify_test_support::mm;

use crate::support::{calln, cell, dl, jrow, pref, probe};

// ---------------------------------------------------------------------------
// (1) The happy path
// ---------------------------------------------------------------------------

#[test]
fn jacobian_row_returns_the_si_primal_and_a_full_width_row_for_a_smooth_residual() {
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    // r = sqrt(x² + y²) − 5
    let expr = binop(
        BinOp::Sub,
        calln("sqrt", vec![binop(
            BinOp::Add,
            binop(BinOp::Mul, pref("x"), pref("x")),
            binop(BinOp::Mul, pref("y"), pref("y")),
        )]),
        literal(Value::Real(5.0)),
    );
    let (primal, row) = jrow(&expr, &values, &seed_cells).expect("smooth residual");

    let ctx = EvalContext::simple(&values);
    assert_eq!(
        primal,
        eval_expr(&expr, &ctx).as_f64().unwrap(),
        "the returned primal is the residual's own SI value, not a recomputation"
    );
    assert_eq!(row.len(), seed_cells.len(), "one column per seed, always");
    assert!((row[0] - 0.6).abs() < 1e-12);
    assert!((row[1] - 0.8).abs() < 1e-12);
}

#[test]
fn a_residual_that_truly_does_not_move_gets_an_honest_zero_row() {
    // The distinction this whole step exists to preserve: a PROVABLE zero and
    // an unavailable derivative must not look alike.  Here the zero is real.
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let expr = literal(Value::Real(7.0));
    let (primal, row) = jrow(&expr, &values, &seed_cells).expect("a constant is differentiable");
    assert_eq!(primal, 7.0);
    assert_eq!(row, vec![0.0, 0.0]);
}

// ---------------------------------------------------------------------------
// (2) An Undef primal
// ---------------------------------------------------------------------------

#[test]
fn jacobian_row_refuses_an_undef_primal_instead_of_returning_a_zero_row() {
    let mut values = ValueMap::new();
    values.insert(cell("x"), Value::Scalar { si_value: 3.0, dimension: DimensionVector::LENGTH });
    values.insert(cell("d"), Value::Real(0.0));
    let seed_cells = vec![cell("x"), cell("d")];

    // Dimension-mismatched addition: Scalar{LENGTH} + Real.
    let mismatch = binop(BinOp::Add, pref("x"), literal(Value::Real(1.0)));
    assert!(
        matches!(
            jrow(&mismatch, &values, &seed_cells),
            Err(NonDifferentiable::UndefPrimal { .. })
        ),
        "a dimension mismatch must refuse, got {:?}",
        jrow(&mismatch, &values, &seed_cells)
    );

    // Division by zero.
    let div0 = binop(BinOp::Div, pref("x"), pref("d"));
    assert!(matches!(
        jrow(&div0, &values, &seed_cells),
        Err(NonDifferentiable::UndefPrimal { .. })
    ));
}

// ---------------------------------------------------------------------------
// (3) An unsupported kind — but ONLY when it is seed-dependent
// ---------------------------------------------------------------------------

#[test]
fn a_seed_dependent_unsupported_kind_is_named_in_the_refusal() {
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    // [x, 1.0][0] — IndexAccess is not differentiated, and the list depends on
    // the seed, so the derivative is genuinely unknown here.
    let expr = CompiledExpr::index_access(
        reify_test_support::builders::expr::list_expr(vec![pref("x"), literal(Value::Real(1.0))]),
        literal(Value::Int(0)),
        dl(),
    );
    match jrow(&expr, &values, &seed_cells) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            // `kind` is a `&'static str` from a closed set, none of which is
            // empty, so `!is_empty()` never checked the name this test is named
            // for.  The construct here is the `IndexAccess`.
            assert_eq!(
                kind, "IndexAccess",
                "the refusal must NAME the construct, or the user cannot act on it"
            );
        }
        other => panic!("expected UnsupportedKind, got {other:?}"),
    }
}

#[test]
fn the_same_unsupported_kind_is_an_honest_zero_row_when_it_is_seed_independent() {
    // The contrast that makes the refusal meaningful: an `IndexAccess` that
    // cannot reach a seed has a derivative, and it is zero.  Refusing here
    // would make every residual containing any unsupported construct
    // undifferentiable, however irrelevant that construct is.
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let constant_index = CompiledExpr::index_access(
        reify_test_support::builders::expr::list_expr(vec![
            literal(Value::Real(2.0)),
            literal(Value::Real(1.0)),
        ]),
        literal(Value::Int(0)),
        dl(),
    );
    // r = x + [2.0, 1.0][0]
    let expr = binop(BinOp::Add, pref("x"), constant_index);
    let (primal, row) = jrow(&expr, &values, &seed_cells).expect("seed-independent ⇒ zero, not refusal");
    assert_eq!(primal, 5.0);
    assert_eq!(row, vec![1.0]);
}

// ---------------------------------------------------------------------------
// (4) A non-scalar root
// ---------------------------------------------------------------------------

#[test]
fn jacobian_row_rejects_a_non_scalar_root_naming_what_it_got() {
    let (values, seed_cells) = probe(&[("x", 3.0)]);

    // Bool root — a comparison is not a residual.
    let boolean = binop(BinOp::Gt, pref("x"), literal(Value::Real(5.0)));
    match jrow(&boolean, &values, &seed_cells) {
        Err(NonDifferentiable::NonScalarResult { got }) => assert_eq!(got, "Bool"),
        other => panic!("expected NonScalarResult, got {other:?}"),
    }

    // Point root.
    let point = literal(Value::Point(vec![Value::Real(1.0), Value::Real(2.0)]));
    match jrow(&point, &values, &seed_cells) {
        Err(NonDifferentiable::NonScalarResult { got }) => assert_eq!(got, "Point"),
        other => panic!("expected NonScalarResult, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (5) A non-finite tangent component
// ---------------------------------------------------------------------------

#[test]
fn jacobian_row_rejects_a_non_finite_tangent_even_when_the_primal_is_finite() {
    // r = y / x with x = 1e-200: the primal is a perfectly ordinary 1e200, but
    // the quotient rule's denominator x² UNDERFLOWS to exactly zero, so both
    // columns come out ±Inf.  Handing those to a linear solve would poison the
    // whole step, and the finite primal gives no hint anything is wrong.
    let (values, seed_cells) = probe(&[("y", 1.0), ("x", 1e-200)]);
    let expr = binop(BinOp::Div, pref("y"), pref("x"));

    let ctx = EvalContext::simple(&values);
    assert_eq!(
        eval_expr(&expr, &ctx),
        Value::Real(1e200),
        "the primal really is finite — that is what makes this case dangerous"
    );
    match jrow(&expr, &values, &seed_cells) {
        Err(NonDifferentiable::NonFiniteTangent { column }) => {
            // `column < 2` is unconditionally true for a two-wide row — the
            // discriminating claim is WHICH column, and it is the first one
            // `position` reaches.
            assert_eq!(column, 0, "the refusal names which column blew up");
        }
        other => panic!("expected NonFiniteTangent, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (6) Display
// ---------------------------------------------------------------------------

#[test]
fn every_non_differentiable_variant_display_names_the_offending_construct() {
    use reify_expr::KinkSite;
    // Each variant is paired with the ONE thing its message must carry for η's
    // tier-2 refusal to be actionable without the surrounding code.  A
    // prose-length proxy (`len() > 20`) would pass for any sentence at all; the
    // discriminant is what the reader actually needs.
    let cases: [(NonDifferentiable, &str); 4] = [
        (NonDifferentiable::UndefPrimal { site: KinkSite::new(vec![1, 2]) }, "[1, 2]"),
        (
            NonDifferentiable::UnsupportedKind {
                kind: "IndexAccess",
                site: KinkSite::new(vec![0]),
            },
            "IndexAccess",
        ),
        (NonDifferentiable::NonScalarResult { got: "Point" }, "Point"),
        (NonDifferentiable::NonFiniteTangent { column: 3 }, "3"),
    ];
    for (case, must_name) in &cases {
        let text = case.to_string();
        assert!(
            text.contains(must_name),
            "{case:?} must name {must_name:?} in its message, got {text:?}"
        );
    }
    // It is an error type, so `?` works in η's assembly loop.
    fn assert_is_error<E: std::error::Error>(_: &E) {}
    assert_is_error(&NonDifferentiable::NonFiniteTangent { column: 0 });
}

// ---------------------------------------------------------------------------
// (7) prefixed
// ---------------------------------------------------------------------------

#[test]
fn prefixed_re_roots_a_sited_refusal_and_leaves_a_siteless_one_unchanged() {
    // The solver's dependent-cell fold reports a refusal raised inside a
    // SUBSIDIARY expression against the residual that read it, so the prefix
    // must come first: it names the path down to where that expression hangs.
    use reify_expr::KinkSite;

    assert_eq!(
        NonDifferentiable::UndefPrimal { site: KinkSite::new(vec![1, 2]) }.prefixed(&[9, 0]),
        NonDifferentiable::UndefPrimal { site: KinkSite::new(vec![9, 0, 1, 2]) },
    );

    match (NonDifferentiable::UnsupportedKind { kind: "probe", site: KinkSite::new(vec![3]) })
        .prefixed(&[5])
    {
        NonDifferentiable::UnsupportedKind { kind, site } => {
            assert_eq!(kind, "probe", "re-rooting must not rename the construct");
            assert_eq!(site, KinkSite::new(vec![5, 3]));
        }
        other => panic!("prefixed changed the variant: {other:?}"),
    }

    assert_eq!(
        NonDifferentiable::UndefPrimal { site: KinkSite::root() }.prefixed(&[4]),
        NonDifferentiable::UndefPrimal { site: KinkSite::new(vec![4]) },
        "a root-sited refusal lands exactly at the prefix",
    );

    let sited = NonDifferentiable::UnsupportedKind { kind: "probe", site: KinkSite::new(vec![6, 1]) };
    assert_eq!(sited.prefixed(&[]), sited, "an empty prefix is the identity");

    for siteless in [
        NonDifferentiable::NonScalarResult { got: "Point" },
        NonDifferentiable::NonFiniteTangent { column: 2 },
    ] {
        assert_eq!(siteless.prefixed(&[8, 8]), siteless, "{siteless:?} carries no site to move");
    }
}

// ---------------------------------------------------------------------------
// (8) a refused overlay cell
// ---------------------------------------------------------------------------

#[test]
fn a_refused_overlay_cell_names_its_cause_only_in_rows_that_read_it() {
    // A solver-derived cell the binder could not differentiate is bound with
    // its cause.  The cause is raised at the READ: a row that reads the cell
    // names the cell's own construct rather than the generic fallback, a row
    // that never reads it is not condemned, and a real rebind retires it.
    use reify_expr::{BranchRecord, DualEnv, KinkSite, Seeds, Tangent, jacobian_row_with_env};

    const CAUSE: &str = "probe: the derived cell's own construct";
    let mut values = ValueMap::new();
    values.insert(cell("x"), Value::Real(2.0));
    values.insert(cell("d"), Value::Real(3.0));
    let ctx = EvalContext::simple(&values);
    let seed_cells = [cell("x")];
    let row_with = |expr: &CompiledExpr, env: &DualEnv| {
        jacobian_row_with_env(expr, &ctx, &Seeds::new(&seed_cells), env, &mut BranchRecord::new())
    };

    let mut env = DualEnv::new();
    env.bind_refused(cell("d"), NonDifferentiable::UnsupportedKind {
        kind: CAUSE,
        site: KinkSite::new(vec![7]),
    });
    let reads_d = binop(BinOp::Add, pref("x"), pref("d"));

    match row_with(&reads_d, &env) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            assert_eq!(kind, CAUSE, "the binder's cause, not the generic bound-cell fallback");
        }
        other => panic!("expected the recorded cause, got {other:?}"),
    }

    let ignores_d = binop(BinOp::Mul, pref("x"), literal(Value::Real(2.0)));
    let (_, row) = row_with(&ignores_d, &env).expect("a row that never reads d is not condemned");
    assert_eq!(row, vec![2.0]);

    env.bind(cell("d"), Tangent::from_row(vec![1.0]));
    let (_, row) = row_with(&reads_d, &env).expect("a real rebind makes the read differentiable");
    assert_eq!(row, vec![2.0], "d/dx (x + d) with d' = 1");

    // The retirement is only visible once a later binder that knows no cause
    // makes the cell non-differentiable again: the read must not resurrect
    // the cause of a binding that has since been displaced.
    env.bind(cell("d"), Tangent::None);
    match row_with(&reads_d, &env) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            assert_ne!(kind, CAUSE, "a rebind retires the stale cause");
        }
        other => panic!("expected a refusal for a cell bound with no tangent, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// A `match` whose discriminant refused a tangent must not answer confidently.
// ---------------------------------------------------------------------------

#[test]
fn a_match_over_a_discriminant_that_refused_a_tangent_refuses_the_row() {
    // `eval_dual_match` never consulted `disc.tangent`.  A `VariantBind` arm
    // inserts payload VALUES into a child scope but binds no TANGENTS, so each
    // binder resolves to `Tangent::Zero` and the arm body returns a confident
    // flat row — while the discriminant's real refusal sits unread in the
    // first-wins slot and is discarded by the next traversal.
    //
    // Reachable here through a SEEDED cell holding a `Value::Enum`: the value
    // is a legitimate discriminant, and `as_f64()` is `None`, so the read
    // refuses exactly as a non-scalar seeded cell must.
    use reify_ir::{CompiledMatchArm, CompiledPattern};

    let mut values = ValueMap::new();
    values.insert(cell("tag"), Value::Enum {
        type_name: "Fit".into(),
        variant: "Loose".into(),
        payload: vec![],
    });
    let seed_cells = vec![cell("tag")];
    let arms = vec![CompiledMatchArm {
        patterns: vec![CompiledPattern::wildcard()],
        body: literal(Value::Real(7.0)),
    }];
    let discriminant = pref("tag");
    let expr = CompiledExpr::match_expr(discriminant, arms, dl());

    let err = jrow(&expr, &values, &seed_cells).expect_err(
        "a discriminant with no tangent cannot yield a confident row for the arm body",
    );
    let msg = err.to_string();
    assert!(
        msg.contains("non-scalar") || msg.contains("seed-dependent"),
        "the refusal must name the discriminant, not be swallowed: {msg}"
    );
}

// ---------------------------------------------------------------------------
// The η-side consequence of the missing strict-`Undef` precheck
// ---------------------------------------------------------------------------

#[test]
fn a_conditional_on_an_undef_comparison_refuses_rather_than_returning_a_row() {
    // The typed refusal η (#6675) actually consumes.  `eval_binop` short-
    // circuits an `Undef` operand before any comparison helper sees it
    // (lib.rs:3852-3855); the dual path calls `eval_eq` directly, which
    // answers `Bool(false)` from an arm that never consults the right operand.
    // The conditional then descends a branch and hands η a FINITE gradient row
    // for a residual whose actual cost is `Undef` — a step along a direction
    // the value path cannot even evaluate.
    let mut values = ValueMap::new();
    values.insert(cell("x"), Value::Real(2.0));
    values.insert(cell("w"), mm(5.0));
    values.insert(cell("u"), Value::Undef);
    let seed_cells = vec![cell("x")];

    let expr = conditional_expr(
        binop(BinOp::Eq, pref("w"), pref("u")),
        binop(BinOp::Mul, pref("x"), pref("x")),
        binop(BinOp::Mul, literal(Value::Real(3.0)), pref("x")),
    );

    let ctx = EvalContext::simple(&values);
    assert_eq!(eval_expr(&expr, &ctx), Value::Undef, "precondition: the residual has no value");
    match jrow(&expr, &values, &seed_cells) {
        Err(_) => {}
        Ok((primal, row)) => panic!(
            "expected a refusal for a residual whose cost is Undef, got primal {primal:?} row {row:?}"
        ),
    }
}
