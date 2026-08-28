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
use reify_expr::branch_signature::{BranchChoice, BranchRecord, KinkKind, ReductionKind};
use reify_expr::dual::Tangent;
use reify_expr::dual_eval::{Seeds, eval_dual};
use reify_expr::{EvalContext, eval_expr};
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
fn mod_as_a_builtin_records_the_same_quotient_cell() {
    // The stdlib `mod` binding is Int-only.  7 mod 3 = 1 with quotient cell 2.
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
