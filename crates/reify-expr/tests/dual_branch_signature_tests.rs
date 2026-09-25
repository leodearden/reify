//! Branch signatures over the dual evaluator — task #6672
//! (solver-unification ε), SECOND DELIVERABLE.
//!
//! Design reference: `docs/prds/v0_6/geometry-algebra-solver-unification.md`
//! §7.7, "Kinks — active-branch (Clarke) Jacobian".
//!
//! Forward-mode AD through `if`, `match`, a comparison, a Kleene connective,
//! `min`/`max`/`abs`/`clamp`/`sign`/`floor`/`ceil`/`round`/`mod`, or a field
//! reduction returns the derivative of the branch that was ACTUALLY TAKEN — a
//! valid element of the Clarke subdifferential, and the right answer locally.
//! But it is only *interpretable* alongside a record of which branch that was:
//! two evaluations that took different branches are two different smooth
//! functions, and a solver that treats their Jacobians as samples of one
//! function chatters across the kink forever.
//!
//! λ (#6679) is the consumer of that record.  Every test below asserts BOTH
//! halves of the contract:
//!
//!   (a) exactly one `BranchEntry` is appended, with the expected `KinkKind`
//!       and `BranchChoice`; and
//!   (b) the tangent is the active branch's derivative.
//!
//! **A note λ needs:** `CompiledExpr` carries no general `span` field (only
//! `StructureInstanceCtor` has one), so `KinkSite` is the STRUCTURAL
//! child-index path from the residual root — deliberately not a pre-order
//! visit counter, because short-circuiting `Conditional`/`And`/`Or` means a
//! counter is not stable across branch flips, and stability under flips is
//! exactly what chatter detection needs.  λ resolves the user-facing span from
//! the owning constraint's `ConstraintNodeId`, which reify-constraints already
//! carries.

#![allow(clippy::mutable_key_type)]

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
use reify_expr::{
    BranchChoice, BranchRecord, EvalContext, KinkKind, ReductionKind, Seeds, Tangent, eval_dual,
    eval_expr,
};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledMatchArm, CompiledPattern, FieldSourceKind,
    InterpolationKind, SampledField, SampledGridKind, Value, ValueMap,
};
use reify_test_support::builders::expr::{
    binop, conditional_expr, fn_call, literal, neg, value_ref_typed,
};

const ENT: &str = "part";

fn cell(member: &str) -> ValueCellId {
    ValueCellId::new(ENT, member)
}

fn dimensionless() -> Type {
    Type::Scalar { dimension: DimensionVector::DIMENSIONLESS }
}

fn vref(member: &str) -> CompiledExpr {
    value_ref_typed(ENT, member, dimensionless())
}

fn call(name: &str, args: Vec<CompiledExpr>) -> CompiledExpr {
    fn_call(name, &format!("std::{name}"), args, dimensionless())
}

/// Evaluate `expr` with the given cells, seeding `seed_names` in order.
/// Returns the primal, the tangent, and the branch record.
fn run(
    expr: &CompiledExpr,
    cells: &[(&str, Value)],
    seed_names: &[&str],
) -> (Value, Tangent, BranchRecord) {
    let mut values = ValueMap::new();
    for (name, v) in cells {
        values.insert(cell(name), v.clone());
    }
    let ctx = EvalContext::simple(&values);
    let seed_cells: Vec<ValueCellId> = seed_names.iter().map(|n| cell(n)).collect();
    let seeds = Seeds::new(&seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(expr, &ctx, &seeds, &mut record);
    // The primal invariant holds at every kink too: branch SELECTION and VALUE
    // can never disagree with `eval_expr`, because both come from the same
    // helpers.
    assert_eq!(dual.value, eval_expr(expr, &ctx), "primal invariant at a kink");
    (dual.value, dual.tangent, record)
}

/// Assert the record holds exactly one entry with this site/kind/choice.
fn assert_single_entry(
    record: &BranchRecord,
    site: &[u16],
    kind: KinkKind,
    choice: BranchChoice,
    label: &str,
) {
    assert_eq!(
        record.len(),
        1,
        "{label}: expected exactly one branch entry, got {:?}",
        record.entries()
    );
    let e = &record.entries()[0];
    assert_eq!(e.site.path(), site, "{label}: kink site");
    assert_eq!(e.kind, kind, "{label}: kink kind");
    assert_eq!(e.choice, choice, "{label}: branch choice");
}

fn row(t: &Tangent, width: usize, label: &str) -> Vec<f64> {
    t.materialize(width).unwrap_or_else(|| panic!("{label}: expected a tangent, got None"))
}

// ---------------------------------------------------------------------------
// The negative control
// ---------------------------------------------------------------------------

#[test]
fn a_purely_smooth_expression_records_no_branch_entries_at_all() {
    // x*x + sqrt(x) has no non-smooth node anywhere.  An EMPTY record is what
    // tells λ "this residual's Jacobian row is an ordinary derivative, not a
    // Clarke selection" — so an empty record must mean exactly that, and never
    // "we forgot to look".
    let expr = binop(BinOp::Add, binop(BinOp::Mul, vref("x"), vref("x")), call("sqrt", vec![
        vref("x"),
    ]));
    let (_, t, rec) = run(&expr, &[("x", Value::Real(4.0))], &["x"]);
    assert!(rec.is_empty(), "smooth expression must record nothing, got {:?}", rec.entries());
    // 2x + 1/(2√x) = 8 + 0.25
    assert!((row(&t, 1, "smooth")[0] - 8.25).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Conditional
// ---------------------------------------------------------------------------

#[test]
fn conditional_records_the_then_branch_and_never_traverses_the_untaken_else() {
    // The else branch carries its OWN kink (`abs`).  If the evaluator walked
    // it, a second entry would appear — so this single assertion pins both the
    // choice and the do-not-traverse-the-untaken-branch rule.
    let expr = conditional_expr(
        vref("flag"),
        binop(BinOp::Mul, vref("x"), vref("x")),
        call("abs", vec![vref("y")]),
    );
    let cells = [("flag", Value::Bool(true)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))];
    let (v, t, rec) = run(&expr, &cells, &["x", "y"]);
    assert_eq!(v, Value::Real(4.0));
    assert_single_entry(&rec, &[], KinkKind::Conditional, BranchChoice::Then, "conditional_then");
    assert_eq!(row(&t, 2, "cond_then"), vec![4.0, 0.0], "tangent is d(x·x)/dx = 2x");
}

#[test]
fn conditional_records_the_else_branch_and_takes_its_derivative() {
    let expr = conditional_expr(
        vref("flag"),
        binop(BinOp::Mul, vref("x"), vref("x")),
        binop(BinOp::Mul, literal(Value::Real(3.0)), vref("y")),
    );
    let cells = [("flag", Value::Bool(false)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))];
    let (v, t, rec) = run(&expr, &cells, &["x", "y"]);
    assert_eq!(v, Value::Real(-9.0));
    assert_single_entry(&rec, &[], KinkKind::Conditional, BranchChoice::Else, "conditional_else");
    assert_eq!(row(&t, 2, "cond_else"), vec![0.0, 3.0], "tangent is d(3y)/dy = 3");
}

// ---------------------------------------------------------------------------
// Match
// ---------------------------------------------------------------------------

fn match_expr(discriminant: CompiledExpr, arms: Vec<CompiledMatchArm>) -> CompiledExpr {
    let result_type = arms[0].body.result_type.clone();
    let mut content_hash = ContentHash::of(b"match").combine(discriminant.content_hash);
    for arm in &arms {
        content_hash = content_hash.combine(arm.body.content_hash);
    }
    CompiledExpr {
        kind: CompiledExprKind::Match { discriminant: Box::new(discriminant), arms },
        result_type,
        content_hash,
    }
}

fn enum_value(variant: &str, payload: Vec<(String, Value)>) -> Value {
    Value::Enum { type_name: "Mode".into(), variant: variant.into(), payload }
}

#[test]
fn match_records_the_index_of_the_selected_arm() {
    let expr = match_expr(vref("mode"), vec![
        CompiledMatchArm {
            patterns: vec![CompiledPattern::variant("Slow")],
            body: call("abs", vec![vref("y")]),
        },
        CompiledMatchArm {
            patterns: vec![CompiledPattern::variant("Fast")],
            body: binop(BinOp::Mul, vref("x"), vref("x")),
        },
        CompiledMatchArm {
            patterns: vec![CompiledPattern::wildcard()],
            body: literal(Value::Real(0.0)),
        },
    ]);
    let cells = [
        ("mode", enum_value("Fast", vec![])),
        ("x", Value::Real(2.0)),
        ("y", Value::Real(-3.0)),
    ];
    let (v, t, rec) = run(&expr, &cells, &["x", "y"]);
    assert_eq!(v, Value::Real(4.0));
    // Arm 0 carries its own kink; selecting arm 1 must leave it untraversed.
    assert_single_entry(&rec, &[], KinkKind::Match, BranchChoice::Arm(1), "match_arm");
    assert_eq!(row(&t, 2, "match"), vec![4.0, 0.0]);
}

#[test]
fn match_variant_bind_arm_binds_the_payload_into_the_body_scope() {
    let bound = cell("bound");
    let expr = match_expr(vref("mode"), vec![CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Some".into(),
            binders: vec![("v".into(), bound.clone())],
        }],
        body: binop(BinOp::Mul, vref("x"), value_ref_typed(ENT, "bound", dimensionless())),
    }]);
    let cells = [
        ("mode", enum_value("Some", vec![("v".into(), Value::Real(5.0))])),
        ("x", Value::Real(2.0)),
    ];
    let (v, t, rec) = run(&expr, &cells, &["x"]);
    assert_eq!(v, Value::Real(10.0), "the binder must be visible to the arm body");
    assert_single_entry(&rec, &[], KinkKind::Match, BranchChoice::Arm(0), "variant_bind");
    assert_eq!(row(&t, 1, "variant_bind"), vec![5.0], "d(x·bound)/dx = bound = 5");
}

// ---------------------------------------------------------------------------
// Comparisons — Bool-valued, so there is no derivative to take
// ---------------------------------------------------------------------------

#[test]
fn every_comparison_operator_records_satisfied_or_unsatisfied_and_refuses_a_tangent() {
    // x = 2, threshold = 5.
    let cases: [(BinOp, bool); 6] = [
        (BinOp::Lt, true),
        (BinOp::Le, true),
        (BinOp::Gt, false),
        (BinOp::Ge, false),
        (BinOp::Eq, false),
        (BinOp::Ne, true),
    ];
    for (op, satisfied) in cases {
        let expr = binop(op, vref("x"), literal(Value::Real(5.0)));
        let (v, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
        assert_eq!(v, Value::Bool(satisfied), "{op:?} primal");
        assert_single_entry(
            &rec,
            &[],
            KinkKind::Comparison(op),
            if satisfied { BranchChoice::Satisfied } else { BranchChoice::Unsatisfied },
            &format!("{op:?}"),
        );
        assert_eq!(
            t,
            Tangent::None,
            "{op:?}: a Bool has no scalar derivative — reporting Zero would let a \
             comparison masquerade as a flat numeric residual"
        );
    }
}

// ---------------------------------------------------------------------------
// Kleene connectives — the short-circuit outcome IS the branch
// ---------------------------------------------------------------------------

#[test]
fn kleene_and_records_left_absorbing_when_the_left_operand_is_false() {
    // False ∧ anything = False, and the right operand is NOT evaluated — so
    // the `abs` kink on the right must leave no entry.
    let expr = binop(
        BinOp::And,
        vref("f"),
        binop(BinOp::Gt, call("abs", vec![vref("x")]), literal(Value::Real(0.0))),
    );
    let cells = [("f", Value::Bool(false)), ("x", Value::Real(2.0))];
    let (v, t, rec) = run(&expr, &cells, &["x"]);
    assert_eq!(v, Value::Bool(false));
    assert_single_entry(
        &rec,
        &[],
        KinkKind::Kleene(BinOp::And),
        BranchChoice::LeftAbsorbing,
        "and_absorbing",
    );
    assert_eq!(t, Tangent::None);
}

#[test]
fn kleene_or_records_left_absorbing_when_the_left_operand_is_true() {
    let expr = binop(
        BinOp::Or,
        vref("f"),
        binop(BinOp::Gt, call("abs", vec![vref("x")]), literal(Value::Real(0.0))),
    );
    let cells = [("f", Value::Bool(true)), ("x", Value::Real(2.0))];
    let (v, _, rec) = run(&expr, &cells, &["x"]);
    assert_eq!(v, Value::Bool(true));
    assert_single_entry(
        &rec,
        &[],
        KinkKind::Kleene(BinOp::Or),
        BranchChoice::LeftAbsorbing,
        "or_absorbing",
    );
}

#[test]
fn kleene_implies_records_left_absorbing_on_a_false_antecedent() {
    // False ⇒ anything = True, vacuously; the consequent is not evaluated.
    let expr = binop(
        BinOp::Implies,
        vref("f"),
        binop(BinOp::Gt, call("abs", vec![vref("x")]), literal(Value::Real(0.0))),
    );
    let cells = [("f", Value::Bool(false)), ("x", Value::Real(2.0))];
    let (v, _, rec) = run(&expr, &cells, &["x"]);
    assert_eq!(v, Value::Bool(true));
    assert_single_entry(
        &rec,
        &[],
        KinkKind::Kleene(BinOp::Implies),
        BranchChoice::LeftAbsorbing,
        "implies_absorbing",
    );
}

#[test]
fn kleene_connectives_record_both_evaluated_when_no_short_circuit_fires() {
    for (op, left) in
        [(BinOp::And, true), (BinOp::Or, false), (BinOp::Implies, true)]
    {
        let expr = binop(op, vref("f"), vref("g"));
        let cells = [("f", Value::Bool(left)), ("g", Value::Bool(true)), ("x", Value::Real(2.0))];
        let (_, t, rec) = run(&expr, &cells, &["x"]);
        assert_single_entry(
            &rec,
            &[],
            KinkKind::Kleene(op),
            BranchChoice::BothEvaluated,
            &format!("{op:?}_both"),
        );
        assert_eq!(t, Tangent::None);
    }
}

#[test]
fn kleene_records_left_type_error_when_the_left_operand_is_not_boolean() {
    for op in [BinOp::And, BinOp::Or, BinOp::Implies] {
        // A numeric left operand is a type error: `eval_expr` returns Undef
        // WITHOUT evaluating the right operand, and the record must say so.
        let expr = binop(op, vref("x"), vref("g"));
        let cells = [("x", Value::Real(2.0)), ("g", Value::Bool(true))];
        let (v, _, rec) = run(&expr, &cells, &["x"]);
        assert_eq!(v, Value::Undef, "{op:?}: a non-boolean left operand is Undef");
        assert_single_entry(
            &rec,
            &[],
            KinkKind::Kleene(op),
            BranchChoice::LeftTypeError,
            &format!("{op:?}_type_error"),
        );
    }
}

// ---------------------------------------------------------------------------
// min / max
// ---------------------------------------------------------------------------

#[test]
fn min_and_max_record_the_selected_operand_and_carry_its_tangent() {
    let cells = [("x", Value::Real(2.0)), ("y", Value::Real(-3.0))];

    let mn = call("min", vec![vref("x"), vref("y")]);
    let (v, t, rec) = run(&mn, &cells, &["x", "y"]);
    assert_eq!(v, Value::Real(-3.0));
    assert_single_entry(&rec, &[], KinkKind::Min, BranchChoice::Operand(1), "min");
    assert_eq!(row(&t, 2, "min"), vec![0.0, 1.0], "min takes the SELECTED operand's derivative");

    let mx = call("max", vec![vref("x"), vref("y")]);
    let (v, t, rec) = run(&mx, &cells, &["x", "y"]);
    assert_eq!(v, Value::Real(2.0));
    assert_single_entry(&rec, &[], KinkKind::Max, BranchChoice::Operand(0), "max");
    assert_eq!(row(&t, 2, "max"), vec![1.0, 0.0]);
}

#[test]
fn a_min_max_tie_is_broken_deterministically_towards_operand_zero_and_recorded() {
    // AT the kink both operands are equal, so the primal cannot distinguish
    // them — but the TANGENT can, and an unrecorded arbitrary choice is exactly
    // what makes a solver chatter.  The tie goes to operand 0, and the record
    // says so, so λ sees the flip when the tie breaks the other way.
    let cells = [("x", Value::Real(2.0)), ("y", Value::Real(2.0))];
    for (name, kind) in [("min", KinkKind::Min), ("max", KinkKind::Max)] {
        let expr = call(name, vec![vref("x"), vref("y")]);
        let (_, t, rec) = run(&expr, &cells, &["x", "y"]);
        assert_single_entry(&rec, &[], kind, BranchChoice::Operand(0), name);
        assert_eq!(row(&t, 2, name), vec![1.0, 0.0], "{name}: tie resolves to operand 0");
    }
}

// ---------------------------------------------------------------------------
// abs
// ---------------------------------------------------------------------------

#[test]
fn abs_records_its_sign_branch_and_takes_the_matching_tangent() {
    let expr = call("abs", vec![vref("x")]);

    let (v, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
    assert_eq!(v, Value::Real(2.0));
    assert_single_entry(&rec, &[], KinkKind::Abs, BranchChoice::Positive, "abs_pos");
    assert_eq!(row(&t, 1, "abs_pos"), vec![1.0]);

    let (v, t, rec) = run(&expr, &[("x", Value::Real(-3.0))], &["x"]);
    assert_eq!(v, Value::Real(3.0));
    assert_single_entry(&rec, &[], KinkKind::Abs, BranchChoice::Negative, "abs_neg");
    assert_eq!(row(&t, 1, "abs_neg"), vec![-1.0], "on the negative branch d|x|/dx = −1");
}

#[test]
fn abs_at_the_origin_records_zero_and_refuses_a_tangent() {
    // |x| has no derivative at 0.  `signum()` would confidently return +1;
    // emitting that would hand the solver a one-sided derivative dressed up as
    // a two-sided one.
    let expr = call("abs", vec![vref("x")]);
    let (v, t, rec) = run(&expr, &[("x", Value::Real(0.0))], &["x"]);
    assert_eq!(v, Value::Real(0.0));
    assert_single_entry(&rec, &[], KinkKind::Abs, BranchChoice::Zero, "abs_zero");
    assert_eq!(t, Tangent::None, "at the kink itself there is no derivative to report");
}

#[test]
fn a_constant_abs_at_the_origin_records_its_kink_yet_refuses_nothing() {
    // The SAME kink, on an argument that does not move: `abs(0)` where the 0 is
    // a base-map constant (a symmetric-tolerance default, a zeroed eccentricity)
    // and the seed is some OTHER cell.  |x| still has no two-sided derivative at
    // 0 — but this `abs` never reaches the chain rule at all, so refusing over
    // it would veto a row that is differentiable in every seeded variable.  That
    // is the failure the module's own `contributes` doc forbids: a partial the
    // chain rule never reads must not be able to veto a derivative that exists.
    //
    // The RECORD is unconditional either way.  λ still needs to see that a kink
    // was evaluated and WHERE, so it can watch that site flip on a later step;
    // suppressing the entry because the tangent happened to be zero would read
    // to λ as "no kink here", the same silent blindness the dependent-cell fold
    // had to fix.
    let expr = call("abs", vec![literal(Value::Real(0.0))]);
    let (v, t, rec) = run(&expr, &[("w", Value::Real(4.0))], &["w"]);
    assert_eq!(v, Value::Real(0.0));
    assert_single_entry(&rec, &[], KinkKind::Abs, BranchChoice::Zero, "constant_abs_at_zero");
    assert_eq!(
        t,
        Tangent::Zero,
        "a constant `abs` contributes nothing to any column, so its tangent is a genuine \
         zero — not a refusal that takes the whole row down with it"
    );
}

#[test]
fn a_nested_kink_records_its_structural_child_index_path_not_a_visit_counter() {
    // −|x| : the `abs` node sits at child index 0 of the `neg` node.
    let expr = neg(call("abs", vec![vref("x")]));
    let (_, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
    assert_single_entry(&rec, &[0], KinkKind::Abs, BranchChoice::Positive, "nested_abs");
    assert_eq!(row(&t, 1, "nested_abs"), vec![-1.0]);
}

// ---------------------------------------------------------------------------
// clamp
// ---------------------------------------------------------------------------

#[test]
fn clamp_records_which_of_the_three_regions_is_active() {
    let lo = literal(Value::Real(0.0));
    let hi = literal(Value::Real(1.0));

    let interior = call("clamp", vec![vref("x"), lo.clone(), hi.clone()]);
    let (v, t, rec) = run(&interior, &[("x", Value::Real(0.5))], &["x"]);
    assert_eq!(v, Value::Real(0.5));
    assert_single_entry(&rec, &[], KinkKind::Clamp, BranchChoice::Interior, "clamp_interior");
    assert_eq!(row(&t, 1, "clamp_interior"), vec![1.0], "only the interior passes f′ through");

    let (v, t, rec) = run(&interior, &[("x", Value::Real(-2.0))], &["x"]);
    assert_eq!(v, Value::Real(0.0));
    assert_single_entry(&rec, &[], KinkKind::Clamp, BranchChoice::BelowLo, "clamp_below");
    assert_eq!(row(&t, 1, "clamp_below"), vec![0.0], "clamped to a constant lo ⇒ flat");

    let (v, t, rec) = run(&interior, &[("x", Value::Real(7.0))], &["x"]);
    assert_eq!(v, Value::Real(1.0));
    assert_single_entry(&rec, &[], KinkKind::Clamp, BranchChoice::AboveHi, "clamp_above");
    assert_eq!(row(&t, 1, "clamp_above"), vec![0.0]);
}

// ---------------------------------------------------------------------------
// sign / floor / ceil / round — derivative 0 almost everywhere
// ---------------------------------------------------------------------------

#[test]
fn sign_floor_ceil_and_round_record_their_integer_cell_and_are_flat() {
    let cases: [(&str, KinkKind, i64, Value); 4] = [
        ("sign", KinkKind::Sign, 1, Value::Real(1.0)),
        ("floor", KinkKind::Floor, 2, Value::Int(2)),
        ("ceil", KinkKind::Ceil, 3, Value::Int(3)),
        ("round", KinkKind::Round, 2, Value::Int(2)),
    ];
    for (name, kind, cell_index, expected) in cases {
        let expr = call(name, vec![vref("x")]);
        let (v, t, rec) = run(&expr, &[("x", Value::Real(2.4))], &["x"]);
        assert_eq!(v, expected, "{name} primal");
        assert_single_entry(&rec, &[], kind, BranchChoice::IntegerCell(cell_index), name);
        assert_eq!(
            t,
            Tangent::Zero,
            "{name} is piecewise constant: its derivative is exactly zero almost everywhere, \
             which is a genuine Zero and not a refusal"
        );
    }
}

// ---------------------------------------------------------------------------
// mod — both spellings
// ---------------------------------------------------------------------------

#[test]
fn mod_as_a_binop_records_its_quotient_cell_and_carries_the_one_minus_k_tangent() {
    // 2.0 % 1.5 = 0.5 with quotient cell 1.  ∂/∂x = 1, ∂/∂divisor = −k.
    let expr = binop(BinOp::Mod, vref("x"), vref("d"));
    let cells = [("x", Value::Real(2.0)), ("d", Value::Real(1.5))];
    let (v, t, rec) = run(&expr, &cells, &["x", "d"]);
    assert_eq!(v, Value::Real(0.5));
    assert_single_entry(&rec, &[], KinkKind::Mod, BranchChoice::ModQuotient(1), "binop_mod");
    assert_eq!(row(&t, 2, "binop_mod"), vec![1.0, -1.0]);
}

#[test]
fn mod_as_an_int_builtin_records_its_own_quotient_cell() {
    // The stdlib `mod` binding is Int-only, so this spelling cannot be run on
    // the BinOp sibling's `(2.0, 1.5)` operands and the two tests compare
    // NOTHING with each other — each pins one spelling's own quotient cell.
    // The genuine cross-spelling check, on operands both spellings accept, is
    // `mod_at_a_negative_dividend_records_the_truncated_quotient_and_matches_its_tangent`
    // in `dual_number_ad_tests.rs`, which is also where trunc and floor part
    // company.  7 mod 3 = 1 with quotient cell 2.
    let expr = call("mod", vec![vref("n"), literal(Value::Int(3))]);
    let (v, t, rec) = run(&expr, &[("n", Value::Int(7))], &["n"]);
    assert_eq!(v, Value::Int(1));
    assert_single_entry(&rec, &[], KinkKind::Mod, BranchChoice::ModQuotient(2), "builtin_mod");
    assert_eq!(row(&t, 1, "builtin_mod"), vec![1.0]);
}

// ---------------------------------------------------------------------------
// Field reductions
// ---------------------------------------------------------------------------

/// A 1-D sampled field over x ∈ {0, 1, 2, 3} with data {5, 9, 2, 7}:
/// max = 9 at x = 1, min = 2 at x = 2.
fn sampled_field() -> Value {
    let axis = vec![0.0, 1.0, 2.0, 3.0];
    let sf = SampledField {
        name: "probe".into(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        axis_grids: vec![axis],
        interpolation: InterpolationKind::Linear,
        data: vec![5.0, 9.0, 2.0, 7.0],
        oob_emitted: AtomicBool::new(false),
    };
    Value::Field {
        domain_type: dimensionless(),
        codomain_type: dimensionless(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

#[test]
fn field_reductions_record_the_selected_argument_and_stay_flat_when_seed_independent() {
    // The field is a literal, so the reduction cannot move with the seeds and
    // its tangent is a genuine Zero.  The ENTRY is still emitted: λ needs to
    // know a reduction sits on this path even when it is currently frozen.
    let cases: [(&str, KinkKind, f64); 4] = [
        ("max", KinkKind::FieldReduction(ReductionKind::Max), 1.0),
        ("min", KinkKind::FieldReduction(ReductionKind::Min), 2.0),
        ("argmax", KinkKind::FieldReduction(ReductionKind::ArgMax), 1.0),
        ("argmin", KinkKind::FieldReduction(ReductionKind::ArgMin), 2.0),
    ];
    for (name, kind, arg_coord) in cases {
        let expr = call(name, vec![literal(sampled_field())]);
        let (_, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
        assert_eq!(rec.len(), 1, "{name}: one entry per reduction");
        let e = &rec.entries()[0];
        assert_eq!(e.kind, kind, "{name}: kind");
        match &e.choice {
            BranchChoice::FieldArgExtremum(v) => {
                let got = v.as_f64().unwrap_or_else(|| {
                    panic!("{name}: expected a scalar argextremum, got {v:?}")
                });
                assert!(
                    (got - arg_coord).abs() < 1e-12,
                    "{name}: selected argument should be {arg_coord}, got {got}"
                );
            }
            other => panic!("{name}: expected FieldArgExtremum, got {other:?}"),
        }
        assert_eq!(t, Tangent::Zero, "{name}: a frozen field reduction is flat, not a refusal");
    }
}

#[test]
fn a_seed_dependent_field_reduction_refuses_a_tangent_but_still_records_its_branch() {
    // The field itself is a seeded cell, so the reduction DOES move with the
    // seeds — but this task does not differentiate through a field.  The honest
    // answer is Tangent::None; a zero row here would tell the solver the
    // residual is flat in a variable it genuinely depends on.
    let expr = call("max", vec![value_ref_typed(ENT, "fld", dimensionless())]);
    let (_, t, rec) = run(&expr, &[("fld", sampled_field())], &["fld"]);
    assert_eq!(rec.len(), 1, "the branch entry is emitted even when the tangent is refused");
    assert_eq!(rec.entries()[0].kind, KinkKind::FieldReduction(ReductionKind::Max));
    assert_eq!(t, Tangent::None);
}

// ===========================================================================
// Step-9: λ's consumption contract
// ===========================================================================
//
// λ (#6679) does not read individual entries — it compares whole records
// across solver iterations.  Two primitives carry that:
//
//   * `differs_from` — "did the active branch set change, and if so WHERE?",
//     the signal that contracts η's trust region; and
//   * `signature_key` — a hashable identity for a branch set, so alternation
//     between two signatures more than K times can be detected without
//     retaining every record.
//
// The load-bearing property underneath both is SITE STABILITY UNDER FLIPS: a
// kink's identity must not move when some *other* branch flips.  A pre-order
// visit counter would fail this — short-circuiting `Conditional`/`And`/`Or`
// changes how many nodes precede a given kink — which is exactly why
// `KinkSite` is a structural path from the root.

/// `if flag then abs(x) else min(x, y)` — a kink in EACH branch of an outer
/// conditional, so flipping `flag` swaps which nested kink is traversed while
/// leaving both of their sites where they are.
fn two_sided_conditional() -> CompiledExpr {
    conditional_expr(
        vref("flag"),
        call("abs", vec![vref("x")]),
        call("min", vec![vref("x"), vref("y")]),
    )
}

fn record_of(expr: &CompiledExpr, cells: &[(&str, Value)], seed_names: &[&str]) -> BranchRecord {
    run(expr, cells, seed_names).2
}

// ---------------------------------------------------------------------------
// (1) differs_from: None when nothing moved, Some(site) naming what did
// ---------------------------------------------------------------------------

#[test]
fn differs_from_is_none_for_two_points_that_take_the_same_branches() {
    let expr = two_sided_conditional();
    let a = record_of(
        &expr,
        &[("flag", Value::Bool(true)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))],
        &["x", "y"],
    );
    // A different point, but the same side of every kink: x stays positive.
    let b = record_of(
        &expr,
        &[("flag", Value::Bool(true)), ("x", Value::Real(9.5)), ("y", Value::Real(-3.0))],
        &["x", "y"],
    );
    assert_eq!(
        a.differs_from(&b),
        None,
        "same branches at both points ⇒ one smooth function ⇒ no trust-region contraction"
    );
}

#[test]
fn differs_from_names_the_exact_conditional_that_flipped() {
    let expr = two_sided_conditional();
    let then_side = record_of(
        &expr,
        &[("flag", Value::Bool(true)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))],
        &["x", "y"],
    );
    let else_side = record_of(
        &expr,
        &[("flag", Value::Bool(false)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))],
        &["x", "y"],
    );
    let site = then_side
        .differs_from(&else_side)
        .expect("a Then→Else flip must be reported, not silently absorbed");
    assert_eq!(
        site.path(),
        &[] as &[u16],
        "the OUTER conditional is the node that flipped, so its root site is what λ is handed"
    );
    // The relation is symmetric in *which* site it names.
    assert_eq!(else_side.differs_from(&then_side), Some(site));
}

#[test]
fn differs_from_reports_a_kink_that_flips_without_any_conditional_moving() {
    // abs(x) alone: positive at one point, negative at another.
    let expr = call("abs", vec![vref("x")]);
    let pos = record_of(&expr, &[("x", Value::Real(2.0))], &["x"]);
    let neg = record_of(&expr, &[("x", Value::Real(-2.0))], &["x"]);
    let site = pos.differs_from(&neg).expect("Positive→Negative is a branch change");
    assert_eq!(site.path(), &[] as &[u16]);
}

// ---------------------------------------------------------------------------
// (2) SITE STABILITY UNDER FLIPS — the load-bearing property
// ---------------------------------------------------------------------------

#[test]
fn a_nested_kink_keeps_its_site_whichever_way_the_outer_conditional_went() {
    // `if flag then abs(x) else min(x, y)`:
    //   - then-branch is child index 1, so `abs` sits at [1]
    //   - else-branch is child index 2, so `min` sits at [2]
    // Those paths are properties of the TREE, not of the traversal, so they do
    // not move when `flag` flips.  A pre-order visit counter would renumber
    // them: on the else path the whole then-subtree is skipped, so every later
    // kink would shift by however many nodes it contains.
    let expr = two_sided_conditional();

    let then_rec = record_of(
        &expr,
        &[("flag", Value::Bool(true)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))],
        &["x", "y"],
    );
    assert_eq!(then_rec.len(), 2, "the Conditional plus the kink inside the taken branch");
    assert_eq!(then_rec.entries()[0].site.path(), &[] as &[u16], "the Conditional is at the root");
    assert_eq!(then_rec.entries()[0].choice, BranchChoice::Then);
    assert_eq!(then_rec.entries()[1].kind, KinkKind::Abs);
    assert_eq!(then_rec.entries()[1].site.path(), &[1], "abs sits at then-branch child index 1");

    let else_rec = record_of(
        &expr,
        &[("flag", Value::Bool(false)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))],
        &["x", "y"],
    );
    assert_eq!(else_rec.len(), 2);
    assert_eq!(else_rec.entries()[0].site.path(), &[] as &[u16]);
    assert_eq!(else_rec.entries()[0].choice, BranchChoice::Else);
    assert_eq!(else_rec.entries()[1].kind, KinkKind::Min);
    assert_eq!(
        else_rec.entries()[1].site.path(),
        &[2],
        "min sits at else-branch child index 2, unchanged by the flip"
    );
}

// ---------------------------------------------------------------------------
// (3) The record is what was TRAVERSED, not what exists
// ---------------------------------------------------------------------------

#[test]
fn kinks_under_an_untaken_branch_appear_in_neither_record() {
    let expr = two_sided_conditional();
    let then_rec = record_of(
        &expr,
        &[("flag", Value::Bool(true)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))],
        &["x", "y"],
    );
    let else_rec = record_of(
        &expr,
        &[("flag", Value::Bool(false)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))],
        &["x", "y"],
    );
    assert!(
        !then_rec.entries().iter().any(|e| e.kind == KinkKind::Min),
        "the else-branch `min` was never evaluated, so it must not be recorded"
    );
    assert!(
        !else_rec.entries().iter().any(|e| e.kind == KinkKind::Abs),
        "the then-branch `abs` was never evaluated, so it must not be recorded"
    );
}

// ---------------------------------------------------------------------------
// (4) signature_key
// ---------------------------------------------------------------------------

#[test]
fn signature_key_is_stable_across_repeated_evaluation_at_the_same_point() {
    let expr = two_sided_conditional();
    let cells = [("flag", Value::Bool(true)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))];
    let first = record_of(&expr, &cells, &["x", "y"]).signature_key();
    for _ in 0..4 {
        assert_eq!(
            record_of(&expr, &cells, &["x", "y"]).signature_key(),
            first,
            "the key must be a function of the branch set alone — λ counts alternations \
             between keys, so a key that wobbles would manufacture phantom chatter"
        );
    }
}

#[test]
fn signature_key_agrees_for_equal_records_and_separates_any_single_choice_change() {
    let expr = two_sided_conditional();
    let base = [("flag", Value::Bool(true)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))];

    // Same branches, different point ⇒ same key.
    let moved = [("flag", Value::Bool(true)), ("x", Value::Real(9.5)), ("y", Value::Real(-3.0))];
    assert_eq!(
        record_of(&expr, &base, &["x", "y"]).signature_key(),
        record_of(&expr, &moved, &["x", "y"]).signature_key()
    );

    // One `choice` differs (the Conditional flips) ⇒ different key.
    let flipped =
        [("flag", Value::Bool(false)), ("x", Value::Real(2.0)), ("y", Value::Real(-3.0))];
    assert_ne!(
        record_of(&expr, &base, &["x", "y"]).signature_key(),
        record_of(&expr, &flipped, &["x", "y"]).signature_key()
    );

    // A nested kink's own flip, with the outer conditional held fixed, must
    // also separate: `abs` goes Positive → Negative.
    let abs_neg = [("flag", Value::Bool(true)), ("x", Value::Real(-2.0)), ("y", Value::Real(-3.0))];
    assert_ne!(
        record_of(&expr, &base, &["x", "y"]).signature_key(),
        record_of(&expr, &abs_neg, &["x", "y"]).signature_key(),
        "a flip anywhere in the record must move the key, or λ would miss the chatter"
    );
}

#[test]
fn signature_key_of_an_empty_record_is_reachable_and_distinct_from_a_populated_one() {
    let smooth = binop(BinOp::Mul, vref("x"), vref("x"));
    let empty = record_of(&smooth, &[("x", Value::Real(2.0))], &["x"]);
    assert!(empty.is_empty());
    // The kink is NESTED — `neg(abs(x))` puts it at child index 0, not at the
    // root — so the site reported below is the one carried by the only entry
    // either record holds.  A root-sited kink would have let an implementation
    // that returned `KinkSite::root()` unconditionally on a length mismatch
    // pass this identically.
    let populated =
        record_of(&neg(call("abs", vec![vref("x")])), &[("x", Value::Real(2.0))], &["x"]);
    let nested_site = populated
        .entries()
        .first()
        .expect("the premise: `neg(abs(x))` records one entry")
        .site
        .clone();
    assert_ne!(nested_site, reify_expr::KinkSite::root(), "the premise: it is not root-sited");
    assert_ne!(empty.signature_key(), populated.signature_key());
    assert_eq!(
        empty.differs_from(&populated),
        Some(nested_site.clone()),
        "the side that ran out contributes nothing, so the EXTRA entry is the divergence"
    );
    assert_eq!(
        populated.differs_from(&empty),
        Some(nested_site),
        "and the relation is symmetric in the site it names"
    );
}

// ---------------------------------------------------------------------------
// (5) The site is a PATH, not a content hash
// ---------------------------------------------------------------------------

#[test]
fn two_structurally_identical_sibling_kinks_get_distinct_sites() {
    // `abs(x) + abs(x)` — both operands have the SAME `content_hash`, so a
    // content-addressed site would collapse them into one and λ could never
    // tell which of the two moved.  As child indices 0 and 1 of the `+` they
    // are distinct.
    // Built TWICE, into separate bindings, so the premise is established rather
    // than assumed: comparing one binding to itself cannot fail, and would keep
    // passing if `call` ever stopped producing equal hashes for equal input —
    // at which point this would silently no longer be the collision case.
    let a = call("abs", vec![vref("x")]);
    let b = call("abs", vec![vref("x")]);
    assert_eq!(
        a.content_hash, b.content_hash,
        "the two operands must be content-identical, or this is not the collision case"
    );
    let expr = binop(BinOp::Add, a, b);
    let rec = record_of(&expr, &[("x", Value::Real(2.0))], &["x"]);
    assert_eq!(rec.len(), 2, "each traversed kink gets its own entry");
    assert_eq!(rec.entries()[0].site.path(), &[0]);
    assert_eq!(rec.entries()[1].site.path(), &[1]);
    assert_ne!(
        rec.entries()[0].site, rec.entries()[1].site,
        "identical content must NOT collapse to one site"
    );
}

// ===========================================================================
// Step-23: BOUNDED field reductions — the record must name the winner INSIDE
// the bounds
// ===========================================================================
//
// `eval_expr` intercepts field reductions at TWO arities: the whole-field form
// (lib.rs:459-484) and the BOUNDED form `max/min/argmax/argmin(field,
// bounds: BoundingBox)` (lib.rs:490-533), which restricts the reduction to the
// grid nodes inside a bounding box.  The dual path mirrors both, so a bounded
// reduction is a kink like any other and gets a `BranchEntry`.
//
// The entry's `BranchChoice` is what λ (#6679) compares across solver
// iterations, and for the bounded form it has to name the BOUNDS-RESTRICTED
// winner.  Reusing the whole-field `compute_argmax`/`compute_argmin` sibling
// would record a grid node that may sit outside the box entirely, and λ would
// then see a PHANTOM flip when the global extremum moves while the bounded
// winner did not — and, worse, MISS a real flip when the bounded winner moves
// while the global one did not.
//
// `sampled_field()` above is built for exactly this: its global max (9 at
// x = 1) lies outside the x ∈ [2, 3] box used throughout this block, so the
// bounded winner (7 at x = 3) and the global one are different grid nodes by
// construction.

/// The canonical `bounding_box(solid)` shape: two 3-component `Value::Point`
/// corners.  `sampled_field()` has one axis, so only the x span is consulted.
fn bbox_x(lo: f64, hi: f64) -> Value {
    Value::BoundingBox {
        min: Box::new(Value::Point(vec![Value::Real(lo), Value::Real(0.0), Value::Real(0.0)])),
        max: Box::new(Value::Point(vec![Value::Real(hi), Value::Real(0.0), Value::Real(0.0)])),
    }
}

fn bounded_call(name: &str, lo: f64, hi: f64) -> CompiledExpr {
    call(name, vec![literal(sampled_field()), literal(bbox_x(lo, hi))])
}

/// `(builtin name, kink kind, bounded argextremum coord over x ∈ [2, 3])`.
const BOUNDED_REDUCTIONS: [(&str, KinkKind, f64); 4] = [
    ("max", KinkKind::FieldReduction(ReductionKind::Max), 3.0),
    ("min", KinkKind::FieldReduction(ReductionKind::Min), 2.0),
    ("argmax", KinkKind::FieldReduction(ReductionKind::ArgMax), 3.0),
    ("argmin", KinkKind::FieldReduction(ReductionKind::ArgMin), 2.0),
];

fn sole_arg_extremum(rec: &BranchRecord, label: &str) -> f64 {
    assert_eq!(rec.len(), 1, "{label}: expected exactly one entry, got {:?}", rec.entries());
    match &rec.entries()[0].choice {
        BranchChoice::FieldArgExtremum(v) => v
            .as_f64()
            .unwrap_or_else(|| panic!("{label}: expected a scalar argextremum, got {v:?}")),
        other => panic!("{label}: expected FieldArgExtremum, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (1) The bounded form is recorded at all
// ---------------------------------------------------------------------------

#[test]
fn a_bounded_field_reduction_records_exactly_one_entry_of_its_own_kind() {
    // An EMPTY record has to mean "no kink here", never "we did not look" —
    // that guarantee is the whole basis on which λ reads a record.  A bounded
    // reduction genuinely selects an extremum grid node, so it is a kink and
    // must appear.
    for (name, kind, _) in BOUNDED_REDUCTIONS {
        let (_, _, rec) = run(&bounded_call(name, 2.0, 3.0), &[("x", Value::Real(2.0))], &["x"]);
        assert_eq!(
            rec.len(),
            1,
            "{name}(field, bbox): one entry per reduction, got {:?}",
            rec.entries()
        );
        assert_eq!(rec.entries()[0].kind, kind, "{name}(field, bbox): kink kind");
    }
}

// ---------------------------------------------------------------------------
// (2) THE DISCRIMINATING ASSERTION — the winner is the one inside the box
// ---------------------------------------------------------------------------

#[test]
fn the_recorded_winner_of_a_bounded_reduction_is_inside_the_bounds() {
    // The box excludes the global maximiser at x = 1 (value 9).  A record that
    // named x = 1 would be describing a grid node the reduction never even
    // considered.
    for (name, _, expected) in BOUNDED_REDUCTIONS {
        let (_, _, rec) = run(&bounded_call(name, 2.0, 3.0), &[("x", Value::Real(2.0))], &["x"]);
        let got = sole_arg_extremum(&rec, name);
        assert!(
            (got - expected).abs() < 1e-12,
            "{name}(field, bbox over x ∈ [2,3]): the recorded winner must be the BOUNDED \
             one at x = {expected}, got x = {got} (the unbounded global winner is x = 1 \
             for max and x = 2 for min)"
        );
    }
}

#[test]
fn the_recorded_winner_moves_with_the_box_even_when_the_global_winner_does_not() {
    // The sharpest form of the same property: the field never changes, so the
    // whole-field `compute_argmax` answer is x = 1 in BOTH evaluations.  Only
    // the box moves, and the record has to follow it.
    let wide = run(&bounded_call("max", 0.0, 3.0), &[("x", Value::Real(2.0))], &["x"]).2;
    let narrow = run(&bounded_call("max", 2.0, 3.0), &[("x", Value::Real(2.0))], &["x"]).2;
    assert!(
        (sole_arg_extremum(&wide, "max over [0,3]") - 1.0).abs() < 1e-12,
        "the whole field is in range, so the winner is the global one at x = 1"
    );
    assert!(
        (sole_arg_extremum(&narrow, "max over [2,3]") - 3.0).abs() < 1e-12,
        "restricted to x ∈ [2,3] the winner moves to x = 3"
    );
}

// ---------------------------------------------------------------------------
// (3) λ's flip primitive works across a bounds change
// ---------------------------------------------------------------------------

#[test]
fn differs_from_names_the_reduction_when_a_bounds_change_moves_the_winner() {
    let cells = [("x", Value::Real(2.0))];
    let wide = run(&bounded_call("max", 0.0, 3.0), &cells, &["x"]).2;
    let narrow = run(&bounded_call("max", 2.0, 3.0), &cells, &["x"]).2;

    let site = wide
        .differs_from(&narrow)
        .expect("the selected grid node moved, so λ must see a flip");
    assert_eq!(
        site.path(),
        wide.entries()[0].site.path(),
        "the reported site must name the reduction that flipped"
    );
    assert_ne!(
        wide.signature_key(),
        narrow.signature_key(),
        "two different branch sets must not share a signature key"
    );
}

#[test]
fn two_boxes_that_select_the_same_node_are_indistinguishable_to_lambda() {
    // x ∈ [2, 3] and x ∈ [1.5, 3.2] both put the maximum at x = 3.  Nothing
    // flipped, so λ must see no flip: a spurious `Some(site)` here would
    // contract η's trust region for no reason.
    let cells = [("x", Value::Real(2.0))];
    let a = run(&bounded_call("max", 2.0, 3.0), &cells, &["x"]).2;
    let b = run(&bounded_call("max", 1.5, 3.2), &cells, &["x"]).2;
    assert_eq!(a.differs_from(&b), None, "same winning node ⇒ no flip");
    assert_eq!(a.signature_key(), b.signature_key(), "same branch set ⇒ same signature key");
}

// ---------------------------------------------------------------------------
// (4) An unresolvable bounded reduction records `Unresolved`
// ---------------------------------------------------------------------------

/// An `Analytical`-source field whose stored lambda is not applicable, so
/// every grid node sampled inside the box yields nothing and
/// `compute_*_bounded` returns `Value::Undef`.
fn unresolvable_analytical_field() -> Value {
    Value::Field {
        domain_type: dimensionless(),
        codomain_type: dimensionless(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(Value::Real(0.0)),
    }
}

#[test]
fn an_unresolvable_bounded_reduction_records_unresolved_rather_than_vanishing() {
    // The reduction is still a kink — λ has to know it is on this path even
    // when the evaluator cannot say which node won.  Silently omitting the
    // entry would read to λ as "this row is a smooth-function sample".
    for (name, kind, _) in BOUNDED_REDUCTIONS {
        let expr = call(name, vec![
            literal(unresolvable_analytical_field()),
            literal(bbox_x(2.0, 3.0)),
        ]);
        let (value, _, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
        assert_eq!(value, Value::Undef, "{name}: the fixture must really be unresolvable");
        assert_eq!(rec.len(), 1, "{name}: the entry is emitted even when the winner is unknown");
        assert_eq!(rec.entries()[0].kind, kind, "{name}: kink kind");
        assert_eq!(
            rec.entries()[0].choice,
            BranchChoice::Unresolved,
            "{name}: an unknown winner is recorded as Unresolved, never invented"
        );
    }
}

// ===========================================================================
// Step-27: the reserved path-segment namespace
// ===========================================================================
//
// A `KinkSite` path mixes STRUCTURAL child indices with two reserved segments:
// `CALLEE_MARKER` (a descent out of a call site into the callee's body) and
// `DEPENDENT_MARKER` (a descent into a solver dependent cell's own expression).
// Both are defined together in `branch_signature` precisely so they cannot
// drift into collision — and this test is what makes "cannot" enforced rather
// than merely intended.
//
// A collision would not fail loudly.  It would name the wrong node in a
// `W_SOLVER_NONSMOOTH_STALL` diagnostic, and — worse — make two genuinely
// different kinks compare EQUAL, so λ would see one signature where there are
// two and never count the alternation.

#[test]
fn the_reserved_path_segments_are_distinct_and_sit_above_every_structural_child_index() {
    use reify_expr::{CALLEE_MARKER, DEPENDENT_MARKER, RESERVED_PATH_SEGMENTS};

    assert_ne!(
        CALLEE_MARKER, DEPENDENT_MARKER,
        "a callee-body site and a dependent-cell site must never be the same segment"
    );
    // The two reserved values occupy the TOP of the `u16` range, contiguously,
    // so every value below them is available as a structural child index and
    // the reserved region cannot be walked into by counting upwards.
    assert_eq!(CALLEE_MARKER, u16::MAX, "the callee marker is the top of the range");
    assert_eq!(DEPENDENT_MARKER, u16::MAX - 1, "and the dependent marker sits directly below it");
    // Taken over the set `branch_signature` EXPORTS, not a copy written here: a
    // third marker added there lands in `RESERVED_PATH_SEGMENTS` and this is
    // what then notices that the index space available to structural children
    // has shrunk.  A local literal could not — it would keep agreeing with
    // itself.
    assert_eq!(
        RESERVED_PATH_SEGMENTS.iter().copied().min().expect("the reserved set is non-empty"),
        u16::MAX - 1,
        "exactly two values are reserved — a third would silently shrink the index space"
    );
    assert!(
        RESERVED_PATH_SEGMENTS.contains(&CALLEE_MARKER)
            && RESERVED_PATH_SEGMENTS.contains(&DEPENDENT_MARKER),
        "the exported set must hold both markers, or the floor above is about the wrong set"
    );

    // The behavioural half: a real kink at a real child index must land well
    // clear of the reserved region.  `clamp` sits as child 1 of the outer `Sub`,
    // itself child 0 of the negation — an ordinary structural path.
    let expr = neg(binop(
        BinOp::Sub,
        vref("x"),
        call("clamp", vec![vref("x"), literal(Value::Real(1.0)), literal(Value::Real(4.0))]),
    ));
    let (_, _, rec) = run(&expr, &[("x", Value::Real(2.5))], &["x"]);
    let clamp = rec
        .entries()
        .iter()
        .find(|e| e.kind == KinkKind::Clamp)
        .expect("the clamp must be recorded");
    assert!(
        !clamp.site.path().is_empty(),
        "the fixture must actually nest the kink, or this asserts nothing"
    );
    for (depth, segment) in clamp.site.path().iter().enumerate() {
        assert!(
            *segment < DEPENDENT_MARKER,
            "structural segment {depth} of {:?} must sit below the reserved region",
            clamp.site.path()
        );
    }
}

// ===========================================================================
// Review follow-up: the `Unresolved` arms, where the primal is hand-written
// ===========================================================================
//
// `eval_dual_conditional` and `eval_dual_match` are the ONLY places this module
// writes a primal itself (`DualValue::opaque(Value::Undef)`) instead of taking
// one from the existing value semantics — which is exactly the "second
// evaluator" both module headers forbid.  Their five unresolved shapes had no
// coverage, so nothing checked that the `Undef` they write is the `Undef`
// `eval_expr` produces.  `run` asserts the primal invariant on every call, so
// each case below is that check plus the entry λ is owed.

#[test]
fn a_conditional_on_a_non_boolean_condition_is_undef_and_records_unresolved() {
    // A numeric condition is a type error: `eval_expr` yields Undef without
    // evaluating either branch, so neither branch's kinks may appear either.
    let expr = conditional_expr(vref("x"), call("abs", vec![vref("x")]), vref("x"));
    let (v, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
    assert_eq!(v, Value::Undef, "a non-Bool condition is Undef");
    assert!(t.is_none(), "and Undef carries no tangent — never a plausible zero row");
    assert_single_entry(&rec, &[], KinkKind::Conditional, BranchChoice::Unresolved, "cond_type");
}

#[test]
fn a_conditional_on_an_undef_condition_is_undef_and_records_unresolved() {
    // `u` is absent from the value map, so the condition is Undef rather than
    // merely the wrong type — a different arm of the same `match` on the
    // condition's value.
    let expr = conditional_expr(vref("u"), vref("x"), vref("x"));
    let (v, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
    assert_eq!(v, Value::Undef);
    assert!(t.is_none());
    assert_single_entry(&rec, &[], KinkKind::Conditional, BranchChoice::Unresolved, "cond_undef");
}

#[test]
fn a_match_over_an_undef_discriminant_short_circuits_before_any_arm() {
    // §9.2.5 — an undef discriminant is refused before a pattern is tried, so
    // the arm bodies' kinks must not appear in the record.
    let expr = match_expr(vref("u"), vec![CompiledMatchArm {
        patterns: vec![CompiledPattern::wildcard()],
        body: call("abs", vec![vref("x")]),
    }]);
    let (v, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
    assert_eq!(v, Value::Undef);
    assert!(t.is_none());
    assert_single_entry(&rec, &[], KinkKind::Match, BranchChoice::Unresolved, "match_undef");
}

#[test]
fn a_match_over_a_non_enum_discriminant_is_undef_and_records_unresolved() {
    // A `Real` discriminant has no variant tag to select on — a distinct arm
    // from the undef one above, and one that must not fall through to the
    // wildcard and answer confidently.
    let expr = match_expr(vref("x"), vec![CompiledMatchArm {
        patterns: vec![CompiledPattern::wildcard()],
        body: literal(Value::Real(7.0)),
    }]);
    let (v, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
    assert_eq!(v, Value::Undef, "a wildcard does not rescue a non-Enum discriminant");
    assert!(t.is_none());
    assert_single_entry(&rec, &[], KinkKind::Match, BranchChoice::Unresolved, "match_non_enum");
}

#[test]
fn a_match_whose_arms_all_miss_the_variant_is_undef_and_records_unresolved() {
    // The fallthrough past the arm loop: a real `Value::Enum`, no pattern that
    // selects it, and no wildcard.
    let expr = match_expr(vref("m"), vec![CompiledMatchArm {
        patterns: vec![CompiledPattern::variant("Slow")],
        body: literal(Value::Real(1.0)),
    }]);
    let cells = [("m", enum_value("Fast", vec![])), ("x", Value::Real(2.0))];
    let (v, t, rec) = run(&expr, &cells, &["x"]);
    assert_eq!(v, Value::Undef, "no arm selects `Fast`, so there is no value to report");
    assert!(t.is_none());
    assert_single_entry(&rec, &[], KinkKind::Match, BranchChoice::Unresolved, "match_no_arm");
}

// ---------------------------------------------------------------------------
// A kink whose OPERAND is Undef: no entry is owed, and this says why
// ---------------------------------------------------------------------------

#[test]
fn a_kink_builtin_with_an_undef_operand_refuses_before_kink_dispatch_and_records_nothing() {
    // `eval_dual_builtin`'s strict-`Undef` check sits BEFORE kink dispatch —
    // mirroring `eval_expr`'s own short-circuit, which is load-bearing for the
    // field-shadow ordering — so `abs(undef)` records no entry even though
    // `node_is_kink` identifies the node.
    //
    // THAT IS THE CONTRACT, not an oversight, and the reason is reachability:
    // reify propagates `Undef` strictly, so an `Undef` operand makes the whole
    // residual `Undef` and `jacobian_row` returns `Err` — nothing ever READS
    // this record.  The same argument `refuse_or_constant`'s doc makes for its
    // refusing branch.  An entry here would be an entry for a row that does not
    // exist.
    let expr = call("abs", vec![vref("u")]);
    let (v, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
    assert_eq!(v, Value::Undef, "abs of Undef is Undef");
    assert!(t.is_none(), "and it carries no tangent");
    assert!(
        rec.is_empty(),
        "no entry is owed for a node whose row cannot exist: {:?}",
        rec.entries()
    );

    // The reachability claim, asserted rather than argued: the Undef really does
    // reach the root of a residual that merely CONTAINS the kink.
    let nested = binop(BinOp::Add, vref("x"), call("abs", vec![vref("u")]));
    let (v, _, _) = run(&nested, &[("x", Value::Real(2.0))], &["x"]);
    assert_eq!(v, Value::Undef, "Undef propagates strictly, so the whole row is refused");
}

// ===========================================================================
// Step-39: the strict-`Undef` PRECHECK `eval_binop` has and the dual path lacks
// ===========================================================================
//
// `eval_binop` evaluates both operands and then, FIRST, short-circuits:
//
//     if lv.is_undef() || rv.is_undef() { return Value::Undef }   // lib.rs:3852
//
// `eval_dual_comparison` and `eval_dual_binop` call `crate::eval_eq` /
// `eval_ne` / `eval_cmp` / the arithmetic helpers DIRECTLY, so neither ever
// reaches that precheck.  For arithmetic the omission is benign only by
// accident of the helpers' `_ => Undef` fall-through arms.  For EQUALITY it is
// a live wrong value, because `eval_eq` has two arms that answer `Bool(false)`
// without ever consulting the other operand:
//
//     (Value::Enum { .. }, _) | (_, Value::Enum { .. })              lib.rs:5007
//     (Value::Scalar { dimension, .. }, _) if !dimension.is_dimensionless()
//                                                                    lib.rs:5018
//
// `eval_binop` never reaches either for an `Undef` operand.  The dual path
// does, and the damage lands in three places at once: a WRONG PRIMAL, a
// RESOLVED `BranchChoice` where the evaluator had none (λ #6679 counts a
// phantom alternation the moment the cell becomes determined), and a finite
// gradient row for a residual whose actual cost is `Undef` (η #6675 steps
// along it).

fn mm5() -> Value {
    Value::Scalar { si_value: 0.005, dimension: DimensionVector::LENGTH }
}

#[test]
fn a_dimensioned_scalar_compared_to_undef_is_undef_on_both_paths() {
    // The `!dimension.is_dimensionless()` arm short-circuits to `Bool(false)`
    // without looking at the right operand, so the dual path answers "these
    // are definitely different" about a value that is not known at all.
    let expr = binop(BinOp::Eq, vref("w"), vref("u"));
    let (value, _t, _rec) = run(&expr, &[("w", mm5()), ("u", Value::Undef)], &["w"]);
    assert_eq!(value, Value::Undef, "a comparison against an undetermined cell has no answer");
}

#[test]
fn an_enum_compared_to_undef_is_undef_on_both_paths() {
    // The sibling `Enum` arm (lib.rs:5007).  Asserted separately because the
    // two arms are INDEPENDENT short-circuits: one test cannot discriminate
    // both, and a fix that reached only one would leave the other live.
    let expr = binop(BinOp::Eq, vref("e"), vref("u"));
    let (value, _t, _rec) = run(
        &expr,
        &[("e", enum_value("Ok", vec![])), ("u", Value::Undef)],
        &["x"],
    );
    assert_eq!(value, Value::Undef, "a comparison against an undetermined cell has no answer");
}

#[test]
fn a_comparison_against_undef_records_unresolved_not_unsatisfied() {
    // The λ (#6679) half.  Today the recorded choice is `Unsatisfied` — a
    // RESOLVED branch where the evaluator resolved nothing — so λ sees a
    // settled comparison and will count a phantom alternation as soon as the
    // cell becomes determined.  The entry must still be PRESENT: a kink was
    // genuinely traversed, and `Unresolved` is the honest choice for it.
    let expr = binop(BinOp::Eq, vref("w"), vref("u"));
    let (_v, _t, rec) = run(&expr, &[("w", mm5()), ("u", Value::Undef)], &["w"]);
    assert_single_entry(
        &rec,
        &[],
        KinkKind::Comparison(BinOp::Eq),
        BranchChoice::Unresolved,
        "eq against an undetermined operand",
    );
}

#[test]
fn a_conditional_on_an_undef_comparison_descends_into_neither_branch() {
    // The η (#6675) half.  `eval_expr` returns `Undef` for a conditional whose
    // condition is `Undef`, WITHOUT evaluating either branch (lib.rs:731-732).
    // The dual path resolves that condition to `Bool(false)`, descends the else
    // branch and returns a finite gradient row — so η would take a step along a
    // gradient for a residual whose actual cost is `Undef`.
    //
    // BOTH branches carry their own kink, so any descent at all shows up as a
    // third record entry (the `conditional_records_..._never_traverses_the_
    // untaken_else` convention, applied to the case where NEITHER is taken).
    let expr = conditional_expr(
        binop(BinOp::Eq, vref("w"), vref("u")),
        call("abs", vec![vref("x")]),
        binop(BinOp::Mul, call("abs", vec![neg(vref("x"))]), literal(Value::Real(3.0))),
    );
    let cells = [("w", mm5()), ("u", Value::Undef), ("x", Value::Real(2.0))];
    let (value, tangent, rec) = run(&expr, &cells, &["x"]);

    assert_eq!(value, Value::Undef, "the conditional's cost is Undef");
    assert_eq!(tangent, Tangent::None, "no finite row for a residual whose cost is Undef");
    assert_eq!(
        rec.len(),
        2,
        "expected only the comparison and the conditional — a third entry means a \
         branch was traversed that `eval_expr` never evaluates: {:?}",
        rec.entries()
    );
    assert_eq!(rec.entries()[0].kind, KinkKind::Comparison(BinOp::Eq));
    assert_eq!(rec.entries()[0].site.path(), &[0], "the condition is child 0");
    assert_eq!(rec.entries()[1].kind, KinkKind::Conditional);
    assert_eq!(rec.entries()[1].choice, BranchChoice::Unresolved);
}

#[test]
fn an_undef_operand_in_a_mod_binop_still_records_its_kink_as_unresolved() {
    // The NEGATIVE control: the precheck's placement must not DROP the `Mod`
    // entry the current code emits.  `mod_quotient_choice` already yields
    // `Unresolved` on an `Undef` operand (its `as_f64()` `None` arm), so this
    // record must come out bit-identical before and after the fix.
    let expr = binop(BinOp::Mod, vref("x"), vref("u"));
    let (value, tangent, rec) =
        run(&expr, &[("x", Value::Real(7.0)), ("u", Value::Undef)], &["x"]);
    assert_eq!(value, Value::Undef);
    assert_eq!(tangent, Tangent::None);
    assert_single_entry(
        &rec,
        &[],
        KinkKind::Mod,
        BranchChoice::Unresolved,
        "mod against an undetermined divisor",
    );
}

#[test]
fn comparisons_and_arithmetic_with_both_operands_determined_are_unchanged() {
    // The control that stops the precheck becoming a blanket `Undef`.  Every
    // comparison operator still resolves, and every arithmetic operator still
    // produces its value and its tangent, when nothing is undetermined.
    for (op, satisfied) in [
        (BinOp::Lt, true),
        (BinOp::Le, true),
        (BinOp::Gt, false),
        (BinOp::Ge, false),
        (BinOp::Eq, false),
        (BinOp::Ne, true),
    ] {
        let expr = binop(op, vref("x"), literal(Value::Real(5.0)));
        let (v, _t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
        assert_eq!(v, Value::Bool(satisfied), "{op:?}: still resolves");
        assert_single_entry(
            &rec,
            &[],
            KinkKind::Comparison(op),
            if satisfied { BranchChoice::Satisfied } else { BranchChoice::Unsatisfied },
            &format!("{op:?} determined"),
        );
    }

    // A dimensioned scalar compared to a DETERMINED operand keeps taking the
    // `Bool(false)` arm — the precheck gates on `Undef`, not on dimension.
    let expr = binop(BinOp::Eq, vref("w"), literal(Value::Real(5.0)));
    let (v, _t, _rec) = run(&expr, &[("w", mm5())], &["w"]);
    assert_eq!(v, Value::Bool(false), "a dimensioned Scalar is never equal to a bare number");

    for (op, expected) in [
        (BinOp::Add, 9.0),
        (BinOp::Sub, 5.0),
        (BinOp::Mul, 14.0),
        (BinOp::Div, 3.5),
    ] {
        let expr = binop(op, vref("x"), literal(Value::Real(2.0)));
        let (v, t, rec) = run(&expr, &[("x", Value::Real(7.0))], &["x"]);
        assert_eq!(v, Value::Real(expected), "{op:?}: still evaluates");
        assert!(rec.is_empty(), "{op:?}: smooth arithmetic records no kink");
        assert!(!t.is_none(), "{op:?}: still carries a tangent");
    }
}
