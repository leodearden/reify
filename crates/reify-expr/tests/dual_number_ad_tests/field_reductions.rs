//! The field reductions on the dual path: both arities of `eval_expr`'s
//! field-reduction intercept, and where the `__field__::<name>` shadow lookup
//! sits relative to them.
//!
//! `eval_expr` intercepts TWO arities of field reduction, not one.  Alongside
//! the 1-argument arms (reify-expr/src/lib.rs:459-484) it intercepts the
//! BOUNDED form `max/min/argmax/argmin(field, bounds: BoundingBox)` at
//! lib.rs:490-533 and routes it to `field_reductions::compute_*_bounded`.
//!
//! The dual path has to mirror BOTH, because the module's central invariant —
//! `eval_dual(e).value == eval_expr(e)` — is only true "by construction" while
//! the two interception tables agree.  When they do not, the failure is silent
//! and expensive: `max(field, bbox)` is claimed instead by the BINARY NUMERIC
//! `min`/`max` kink, handed to `reify_stdlib::eval_builtin`, and comes back
//! `Value::Undef` because `Value::Field` has no `as_f64` mapping.  η then
//! reports "residual evaluates to undef" for the canonical FEA-in-the-loop
//! constraint `max(von_mises_field, region_bbox) - limit <= 0`, which the
//! ordinary evaluator handles perfectly well.
//!
//! This file asserts the VALUE-semantics half of the contract; the branch
//! record half lives in `dual_branch_signature_tests.rs`.
//!
//! The `__field__::<name>` shadow lookup must sit where `eval_expr` puts it
//! (task #6672, steps 31-32).
//! `eval_expr` resolves a `__field__::<name>` field cell in its catch-all `_`
//! arm (lib.rs:612) — i.e. AFTER every named intercept (the whole-field and
//! bounded reductions, `flat_map`, `worst_case`, `generate`) and BEFORE
//! `reify_stdlib::eval_builtin`.  The dual path must resolve it at the SAME
//! position, or the two paths disagree about which of the two meanings a
//! 1-argument call has.
//!
//! Both directions are pinned here, because the ordering has a wrong answer on
//! each side:
//!
//!   (a) too EARLY — a whole-field reduction name with a shadow cell in scope
//!       takes the lambda on the dual path while `eval_expr` takes the
//!       reduction.  That is the live defect: the lambda is applied to a
//!       `Value::Field`, which is `Undef`, so the node silently collapses.
//!   (b) too LATE — pushing the lookup past the smooth/kinky builtin tables
//!       would send `abs`/`sqrt` to those tables while `eval_expr` applies the
//!       lambda, trading one divergence for three.  Block (b) holds the fix to
//!       exactly the width of the bug.

use std::sync::Arc;

use reify_core::{Type, ValueCellId};
use reify_expr::{
    BranchRecord, DualEnv, EvalContext, NonDifferentiable, Seeds, Tangent, eval_dual, eval_expr,
    jacobian_row_with_env,
};
use reify_ir::{BinOp, CompiledExpr, FieldSourceKind, Value, ValueMap};
use reify_test_support::builders::expr::{binop, literal, value_ref_typed};
use reify_test_support::sampled_field_fixtures::{extremum_probe_field, x_span_bbox};

use crate::support::{ENT, calln, cell, dl, jrow, pref, probe};

/// `(label, expected bounded answer)` for the four bounded reductions over
/// `extremum_probe_field()` restricted to x ∈ [2, 3].
const BOUNDED_CASES: [(&str, f64); 4] =
    [("max", 7.0), ("min", 2.0), ("argmax", 3.0), ("argmin", 2.0)];

/// Evaluate with an explicit cell map, returning `(primal, tangent)` and the
/// `eval_expr` reference primal alongside it.
fn dual_and_plain(
    expr: &CompiledExpr,
    values: &ValueMap,
    seed_cells: &[ValueCellId],
) -> (Value, Tangent, Value) {
    let ctx = EvalContext::simple(values);
    let seeds = Seeds::new(seed_cells);
    let mut record = BranchRecord::new();
    let dv = eval_dual(expr, &ctx, &seeds, &mut record);
    let plain = eval_expr(expr, &ctx);
    (dv.value, dv.tangent, plain)
}

// ===========================================================================
// The BOUNDED field reductions — max/min/argmax/argmin(field, bounds)
// ===========================================================================

// ---------------------------------------------------------------------------
// (1) THE PRIMAL INVARIANT at arity 2
// ---------------------------------------------------------------------------

#[test]
fn bounded_field_reductions_keep_the_primal_invariant_and_are_not_undef() {
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    for (name, expected) in BOUNDED_CASES {
        let expr =
            calln(name, vec![literal(extremum_probe_field()), literal(x_span_bbox(2.0, 3.0))]);
        let (dual_v, _, plain) = dual_and_plain(&expr, &values, &seed_cells);

        // The invariant itself.
        assert_eq!(
            dual_v, plain,
            "{name}(field, bbox): eval_dual must reuse the existing value semantics verbatim"
        );
        // ...with teeth: `Undef == Undef` would satisfy the line above while
        // both paths were broken.  The reference must be a real number.
        assert_ne!(
            plain,
            Value::Undef,
            "{name}(field, bbox): eval_expr must produce the bounded extremum, not Undef"
        );
        let got = plain
            .as_f64()
            .unwrap_or_else(|| panic!("{name}(field, bbox): expected a scalar, got {plain:?}"));
        assert!(
            (got - expected).abs() < 1e-12,
            "{name}(field, bbox) over x ∈ [2,3] must be the BOUNDS-RESTRICTED answer \
             {expected}, got {got} (the unbounded answer would be 9 / x=1)"
        );
    }
}

// ---------------------------------------------------------------------------
// (2) Seed-independent ⇒ Zero, not a refusal
// ---------------------------------------------------------------------------

#[test]
fn a_seed_independent_bounded_reduction_is_flat_rather_than_a_refusal() {
    // Both operands are literals, so the reduction provably cannot move with
    // the seeds.  `Tangent::Zero` is the honest answer; `Tangent::None` here
    // would poison an otherwise perfectly differentiable Jacobian row.
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    for (name, _) in BOUNDED_CASES {
        let expr =
            calln(name, vec![literal(extremum_probe_field()), literal(x_span_bbox(2.0, 3.0))]);
        let (_, tangent, _) = dual_and_plain(&expr, &values, &seed_cells);
        assert_eq!(
            tangent,
            Tangent::Zero,
            "{name}(field, bbox): a frozen bounded reduction is flat, not undifferentiable"
        );
    }
}

#[test]
fn a_bounded_reduction_inside_a_residual_contributes_a_zero_column_not_a_refusal() {
    // The end-to-end shape η actually sees: `x - max(field, bbox)`.  The row
    // must come back finite, with the reduction contributing nothing.
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    let expr = binop(
        BinOp::Sub,
        pref("x"),
        calln("max", vec![literal(extremum_probe_field()), literal(x_span_bbox(2.0, 3.0))]),
    );
    let (primal, row) =
        jrow(&expr, &values, &seed_cells).expect("a frozen bounded reduction is differentiable");
    assert!((primal - (2.0 - 7.0)).abs() < 1e-12, "primal must be 2 − 7 = −5, got {primal}");
    assert_eq!(row, vec![1.0]);
}

// ---------------------------------------------------------------------------
// (3) A seed-dependent FIELD refuses
// ---------------------------------------------------------------------------

#[test]
fn a_bounded_reduction_over_a_seed_dependent_field_refuses_a_tangent() {
    // This task does not differentiate THROUGH a field (PRD §7.7 makes field
    // and geometry derivatives the ANALYTIC source, not the AD one), so the
    // only honest answer is `Tangent::None` — matching the arity-1 rule.
    for (name, _) in BOUNDED_CASES {
        let mut values = ValueMap::new();
        values.insert(cell("fld"), extremum_probe_field());
        let seed_cells = vec![cell("fld")];
        let expr = calln(name, vec![
            value_ref_typed(ENT, "fld", dl()),
            literal(x_span_bbox(2.0, 3.0)),
        ]);
        let (_, tangent, plain) = dual_and_plain(&expr, &values, &seed_cells);
        assert_ne!(plain, Value::Undef, "{name}: the reference primal is still a real number");
        assert_eq!(
            tangent,
            Tangent::None,
            "{name}(seeded field, bbox): a zero row would claim the residual is flat \
             in a variable it genuinely depends on"
        );
    }
}

// ---------------------------------------------------------------------------
// (4) A seed-dependent BOUNDS operand refuses too
// ---------------------------------------------------------------------------

#[test]
fn a_bounded_reduction_whose_bounds_move_with_the_seeds_refuses_a_tangent() {
    // The bounding box is itself a seeded cell, so the box — and therefore
    // WHICH grid node wins — genuinely moves with the solved variables.  A
    // tangent decision taken from the field operand alone would report
    // `Tangent::Zero` here, telling the solver the residual is flat in a
    // variable that in fact selects its value.
    for (name, _) in BOUNDED_CASES {
        let mut values = ValueMap::new();
        values.insert(cell("bb"), x_span_bbox(2.0, 3.0));
        let seed_cells = vec![cell("bb")];
        let expr = calln(name, vec![
            literal(extremum_probe_field()),
            value_ref_typed(ENT, "bb", Type::BoundingBox),
        ]);
        let (_, tangent, plain) = dual_and_plain(&expr, &values, &seed_cells);
        assert_ne!(plain, Value::Undef, "{name}: the reference primal is still a real number");
        assert_eq!(
            tangent,
            Tangent::None,
            "{name}(field, seeded bbox): a seed-dependent box must refuse, never claim Zero"
        );
    }
}

#[test]
fn the_refusal_from_a_moving_bounded_reduction_names_the_field_reduction_itself() {
    // η copies this message into a tier-2 refusal, where the user reads it
    // without the surrounding code, so it has to name the construct.
    //
    // The field is reached through a `DualEnv` overlay rather than a seed
    // column — the shape the solver actually produces for a DERIVED cell — so
    // the reduction node is the FIRST thing on the path with no derivative
    // rule, and its own refusal is the one that survives `note_refusal`'s
    // first-wins rule.  (Seeding the field cell directly instead refuses one
    // level earlier, at "a seeded cell holding a non-scalar value"; that is
    // the arity-1 behaviour too, and is asserted by the tangent tests above.)
    let mut values = ValueMap::new();
    values.insert(cell("fld"), extremum_probe_field());
    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&[cell("x")]);
    let mut env = DualEnv::new();
    env.bind(cell("fld"), Tangent::Scalar(vec![1.0]));
    let expr =
        calln("max", vec![value_ref_typed(ENT, "fld", dl()), literal(x_span_bbox(2.0, 3.0))]);
    let mut record = BranchRecord::new();
    match jacobian_row_with_env(&expr, &ctx, &seeds, &env, &mut record) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            assert!(
                kind.contains("field reduction"),
                "the refusal must name the field reduction, got {kind:?}"
            );
        }
        other => panic!("expected an UnsupportedKind refusal, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (5) NEGATIVE CONTROLS — the fix must stay narrow
// ---------------------------------------------------------------------------

#[test]
fn the_binary_numeric_max_over_two_scalars_is_untouched_by_the_bounded_route() {
    // `max(a, b)` over two ordinary scalars is the BINARY NUMERIC kink and must
    // keep selecting an operand and adopting its tangent.
    let (values, seed_cells) = probe(&[("a", 3.0), ("b", 5.0)]);
    for (name, winner, col) in [("max", 5.0, 1usize), ("min", 3.0, 0usize)] {
        let expr = calln(name, vec![pref("a"), pref("b")]);
        let (primal, row) = jrow(&expr, &values, &seed_cells)
            .unwrap_or_else(|e| panic!("{name}(a, b) must stay differentiable, got {e:?}"));
        assert!((primal - winner).abs() < 1e-12, "{name}(3, 5) = {winner}, got {primal}");
        let expected: Vec<f64> = (0..2).map(|j| if j == col { 1.0 } else { 0.0 }).collect();
        assert_eq!(row, expected, "{name}: the selected operand's tangent, unchanged");
    }
}

#[test]
fn malformed_two_argument_reduction_shapes_still_agree_with_eval_expr_on_undef() {
    // `eval_expr`'s arity-2 guard (lib.rs:490) requires a `BoundingBox` second
    // argument and a `Field` first one; anything else deliberately falls
    // through to `eval_builtin`, which returns `Undef` for these operand types
    // (numeric.rs:67).  The dual path must fall through in exactly the same
    // places — a widened intercept that swallowed these would be a SECOND
    // divergence, in the opposite direction.
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    let cases: [(&str, CompiledExpr); 4] = [
        (
            "max(field, 3.0)",
            calln("max", vec![literal(extremum_probe_field()), literal(Value::Real(3.0))]),
        ),
        (
            "min(field, 3.0)",
            calln("min", vec![literal(extremum_probe_field()), literal(Value::Real(3.0))]),
        ),
        ("max(2.0, bbox)", calln("max", vec![
            literal(Value::Real(2.0)),
            literal(x_span_bbox(2.0, 3.0)),
        ])),
        ("argmax(2.0, bbox)", calln("argmax", vec![
            literal(Value::Real(2.0)),
            literal(x_span_bbox(2.0, 3.0)),
        ])),
    ];
    for (label, expr) in cases {
        let (dual_v, _, plain) = dual_and_plain(&expr, &values, &seed_cells);
        assert_eq!(plain, Value::Undef, "{label}: eval_expr must still fall through to Undef");
        assert_eq!(dual_v, plain, "{label}: the dual path must fall through in the same place");
    }
}

#[test]
fn an_arity_one_reduction_over_a_non_field_operand_records_its_kink_and_names_the_site() {
    use reify_expr::{BranchChoice, KinkKind, ReductionKind};

    // `is_kink_builtin` claims `("max"|"min"|"argmax"|"argmin", 1)`, so
    // `node_is_kink` keeps this node off the seed-independence fast path —
    // while `field_reduction_kind` DECLINES it, because the single operand is
    // not a `Value::Field`.  A node positively identified as a kink still owes
    // the record an entry: "no entries" must mean "no kink", never "we did not
    // look", which is the one guarantee λ (#6679) builds on.
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    for (name, kind) in [
        ("max", ReductionKind::Max),
        ("min", ReductionKind::Min),
        ("argmax", ReductionKind::ArgMax),
        ("argmin", ReductionKind::ArgMin),
    ] {
        // Nested one level down, so the refusal's SITE distinguishes a real
        // record from the root-sited generic fallback `jacobian_row` produces
        // when the traversal noted nothing at all.
        let expr = binop(BinOp::Sub, calln(name, vec![pref("x")]), literal(Value::Real(1.0)));
        let ctx = EvalContext::simple(&values);
        let seeds = Seeds::new(&seed_cells);
        let mut record = BranchRecord::new();
        let dual = eval_dual(&expr, &ctx, &seeds, &mut record);

        assert_eq!(dual.value, eval_expr(&expr, &ctx), "{name}: the primal invariant");
        assert_eq!(
            dual.value,
            Value::Undef,
            "{name}: today the stdlib has no 1-argument scalar binding — that is an accident \
             of the stdlib, and the arm does not depend on it"
        );

        assert_eq!(
            record.len(),
            1,
            "{name}: one kink node was traversed, got {:?}",
            record.entries()
        );
        let entry = &record.entries()[0];
        assert_eq!(entry.kind, KinkKind::FieldReduction(kind), "{name}: the recorded kind");
        assert_eq!(
            entry.choice,
            BranchChoice::Unresolved,
            "{name}: nothing reduced, so there is no winning grid node to name"
        );
        assert_eq!(entry.site.path(), [0_u16], "{name}: child 0 of the subtraction");

        match jrow(&expr, &values, &seed_cells) {
            Err(NonDifferentiable::UndefPrimal { site }) => assert_eq!(
                site.path(),
                [0_u16],
                "{name}: the refusal must name the undef call, not fall back to the root"
            ),
            other => panic!("{name}: expected UndefPrimal at the call, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// An arity-2 reduction shape `field_reduction_kind` declines still owes an
// entry — `node_is_kink` positively identified it.
// ---------------------------------------------------------------------------

#[test]
fn a_two_argument_reduction_over_a_non_box_operand_still_records_its_kink() {
    use reify_expr::{BranchChoice, KinkKind, ReductionKind};

    // `is_bounded_reduction_shaped` claims `("argmax" | "argmin", 2)` to keep
    // the shape off the seed-independence fast path, precisely so a genuine
    // extremum selection cannot be swallowed unrecorded.  When the second
    // operand is not a `BoundingBox`, `field_reduction_kind` declines and
    // `is_kink_builtin` does not claim arity 2 either — so the call used to
    // fall through the "neither smooth nor kinky" arm and record NOTHING,
    // leaving λ (#6679) to read an empty record as "no kink here" for a node
    // this module itself identified as a kink.
    let (values, seed_cells) = probe(&[("w", 2.0)]);
    for (name, kind) in
        [("argmax", ReductionKind::ArgMax), ("argmin", ReductionKind::ArgMin)]
    {
        let expr = calln(name, vec![pref("w"), literal(Value::Real(3.0))]);
        let ctx = EvalContext::simple(&values);
        let seeds = Seeds::new(&seed_cells);
        let mut record = BranchRecord::new();
        let dual = eval_dual(&expr, &ctx, &seeds, &mut record);

        assert_eq!(dual.value, eval_expr(&expr, &ctx), "{name}: the primal invariant");
        let entry = record
            .entries()
            .iter()
            .find(|e| e.kind == KinkKind::FieldReduction(kind))
            .unwrap_or_else(|| {
                panic!("{name}: an entry is OWED — `node_is_kink` claimed this node")
            });
        assert_eq!(
            entry.choice,
            BranchChoice::Unresolved,
            "{name}: no reduction ran, so there is no winning grid node to name"
        );
    }
}

// ===========================================================================
// The `__field__::<name>` shadow lookup
// ===========================================================================

/// A `Value::Field` whose lambda is `p ↦ p + 1000`, registered at
/// `__field__::<name>` so that `<name>(x)` resolves to it.
///
/// The `+1000` offset is a fingerprint: any primal ≥ 1000 below was produced
/// by the shadow lambda, and any primal under it came from the builtin or the
/// reduction.  The `cube` field cell in `user_functions.rs` cannot serve here —
/// it deliberately uses the non-builtin name `cube`, so it resolves identically
/// under both orderings and is blind to exactly the distinction these tests draw.
fn shadow_cell(name: &str) -> (ValueCellId, Value) {
    let p = ValueCellId::new("$lambda_shadow", "p");
    let body =
        binop(BinOp::Add, CompiledExpr::value_ref(p.clone(), dl()), literal(Value::Real(1000.0)));
    let lambda = Value::Lambda {
        params: vec![("p".to_string(), p)],
        body: Box::new(body),
        captures: ValueMap::new(),
    };
    let field = Value::Field {
        domain_type: dl(),
        codomain_type: dl(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };
    (ValueCellId::new(reify_core::FIELD_ENTITY_PREFIX, name), field)
}

/// The four names `eval_expr` intercepts at arity 1 for a `Value::Field`
/// argument — the whole-field reductions, i.e. exactly the names for which a
/// shadow cell and a named intercept compete.
const SHADOWED_REDUCTIONS: [&str; 4] = ["max", "min", "argmax", "argmin"];

// ---------------------------------------------------------------------------
// (a) THE DIVERGENCE — a shadow cell must not outrank a whole-field reduction
// ---------------------------------------------------------------------------

#[test]
fn a_shadow_cell_does_not_outrank_a_whole_field_reduction_on_the_dual_path() {
    for name in SHADOWED_REDUCTIONS {
        let (mut values, seed_cells) = probe(&[("x", 2.0)]);
        let (cell_id, field) = shadow_cell(name);
        values.insert(cell_id, field);

        let expr = calln(name, vec![literal(extremum_probe_field())]);
        let (dual_v, _, plain) = dual_and_plain(&expr, &values, &seed_cells);

        // The invariant itself: `eval_expr` takes the named reduction arm, so
        // the dual path must take it too rather than applying the lambda.
        assert_eq!(
            dual_v, plain,
            "{name}(field) with `__field__::{name}` in scope: eval_dual must resolve the \
             shadow at the same position eval_expr does"
        );
        // ...with teeth: `Undef == Undef` would satisfy the line above while
        // BOTH paths were broken.  The reference must be a real answer, and
        // it must not carry the shadow lambda's +1000 fingerprint.
        assert_ne!(
            plain,
            Value::Undef,
            "{name}(field): eval_expr must produce the whole-field reduction, not Undef"
        );
        let got = plain
            .as_f64()
            .unwrap_or_else(|| panic!("{name}(field): expected a scalar, got {plain:?}"));
        assert!(
            got < 1000.0,
            "{name}(field): the reduction must win over the shadow lambda, but the primal \
             {got} carries the lambda's +1000 fingerprint"
        );
    }
}

// ---------------------------------------------------------------------------
// (b) THE OVER-CORRECTION GUARD — three shapes that agree TODAY
// ---------------------------------------------------------------------------

#[test]
fn a_shadow_cell_still_outranks_the_builtin_tables_on_both_paths() {
    // `eval_expr`'s `_`-arm comment ("Builtins are matched in earlier arms, so
    // they are never shadowed") is about its own NAMED match arms, not about
    // stdlib builtins at large: `abs` and `sqrt` are resolved by
    // `reify_stdlib::eval_builtin` INSIDE the `_` arm, i.e. *after* the shadow
    // lookup.  So a field cell really does shadow them, and the dual path must
    // keep agreeing.  Each expected value is the shadow lambda's answer
    // (`arg + 1000`), never the builtin's.
    let cases: [(&str, f64, f64); 3] = [
        // A KINKY 1-arg builtin: abs(-2) would be 2 if the kink table won.
        ("abs", -2.0, 998.0),
        // A SMOOTH 1-arg builtin: sqrt(4) would be 2 if the smooth table won.
        ("sqrt", 4.0, 1004.0),
        // A reduction NAME at arity 1 over a NON-Field argument:
        // `field_reduction_kind` returns `None` and `eval_expr`'s named arms
        // require a `Value::Field`, so both paths fall through to the shadow.
        ("max", 3.0, 1003.0),
    ];

    for (name, x, expected) in cases {
        let (mut values, seed_cells) = probe(&[("x", x)]);
        let (cell_id, field) = shadow_cell(name);
        values.insert(cell_id, field);

        let expr = calln(name, vec![pref("x")]);
        let (dual_v, tangent, plain) = dual_and_plain(&expr, &values, &seed_cells);

        assert_eq!(
            plain,
            Value::Real(expected),
            "{name}({x}): eval_expr applies the shadow lambda, so the reference is {expected}"
        );
        assert_eq!(
            dual_v, plain,
            "{name}({x}) with `__field__::{name}` in scope: the dual path must keep letting \
             the shadow cell outrank the builtin tables"
        );
        // The lambda is `p ↦ p + 1000`, so the tangent is the argument's,
        // unchanged — pinning that the shadow is DIFFERENTIATED and not merely
        // primal-matched by an opaque fallthrough.
        assert_eq!(
            tangent.materialize(1).unwrap_or_else(|| panic!("{name}({x}): expected a tangent")),
            vec![1.0],
            "{name}({x}): d(x + 1000)/dx = 1 through the applied lambda"
        );
    }
}
