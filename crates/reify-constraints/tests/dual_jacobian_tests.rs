//! Forward-mode AD at the solver seam — task #6672 (solver-unification ε).
//!
//! Design reference: `docs/prds/v0_6/geometry-algebra-solver-unification.md`
//! §7.7.  This is the surface the downstream consumers actually call:
//!
//! - **η (#6675)** — the Gauss-Newton / Levenberg-Marquardt step needs `J` with
//!   columns in `auto_params` order and the `‖Jᵀr‖` stationarity certificate;
//! - **μ (#6680)** — the reduced gradient is the same call with a single
//!   objective expression;
//! - **λ (#6679)** — the per-row `BranchRecord`s.
//!
//! ε supplies the derivatives *beside* the existing solver.  It does not touch
//! `ConstraintCostFunction`, the Nelder-Mead loop, `comparison_violation`'s
//! `+1e-12` kinks, or `PENALTY_WEIGHT` — replacing the loop is η's job.

#![allow(clippy::mutable_key_type)]

use reify_core::{DimensionVector, Type, ValueCellId};
use reify_expr::{EvalContext, NonDifferentiable, eval_expr};
use reify_ir::{AutoParam, BinOp, CompiledExpr, Value, ValueMap};
use reify_test_support::builders::expr::{binop, fn_call, literal, value_ref_typed};

use reify_constraints::residual_jacobian;

const ENT: &str = "model";

fn cell(name: &str) -> ValueCellId {
    ValueCellId::new(ENT, name)
}

fn scalar_ty(dim: DimensionVector) -> Type {
    Type::Scalar { dimension: dim }
}

fn auto(name: &str, dim: DimensionVector) -> AutoParam {
    AutoParam { id: cell(name), param_type: scalar_ty(dim), bounds: None, free: false }
}

fn aref(name: &str, dim: DimensionVector) -> CompiledExpr {
    value_ref_typed(ENT, name, scalar_ty(dim))
}

fn len_lit(v: f64) -> CompiledExpr {
    literal(Value::Scalar { si_value: v, dimension: DimensionVector::LENGTH })
}

fn call(name: &str, args: Vec<CompiledExpr>, dim: DimensionVector) -> CompiledExpr {
    fn_call(name, &format!("std::{name}"), args, scalar_ty(dim))
}

/// `object[index]`.  Built here rather than in `reify-test-support` because
/// this task's scope does not extend to that crate.
fn index_access(object: CompiledExpr, index: CompiledExpr) -> CompiledExpr {
    let content_hash = reify_core::ContentHash::of(b"index_access")
        .combine(object.content_hash)
        .combine(index.content_hash);
    CompiledExpr {
        kind: reify_ir::CompiledExprKind::IndexAccess {
            object: Box::new(object),
            index: Box::new(index),
        },
        result_type: scalar_ty(DimensionVector::LENGTH),
        content_hash,
    }
}

/// The independent trial-point reference.
///
/// This deliberately does NOT call the solver's own `build_trial_values`: it is
/// the reference the AD path is checked against, so sharing code with the thing
/// under test would make the comparison vacuous.  It mirrors the same
/// mapping — `params[i] ↔ x[i]`, each auto inserted as a `Value::Scalar`
/// carrying its declared dimension.
fn trial_values(
    base: &ValueMap,
    params: &[AutoParam],
    x: &[f64],
    dependent_cells: &[(ValueCellId, CompiledExpr)],
) -> ValueMap {
    assert_eq!(params.len(), x.len());
    let mut values = base.clone();
    for (param, &val) in params.iter().zip(x.iter()) {
        let dimension = match &param.param_type {
            Type::Scalar { dimension } => *dimension,
            _ => DimensionVector::DIMENSIONLESS,
        };
        values.insert(param.id.clone(), Value::Scalar { si_value: val, dimension });
    }
    // Fold in STORED ORDER against the RUNNING map, so an earlier dependent
    // cell is visible to a later one — the same guarantee the solver's own fold
    // gives, re-derived here rather than borrowed, because this is the
    // reference the subject is checked against.
    for (id, expr) in dependent_cells {
        let v = {
            let ctx = EvalContext::simple(&values);
            eval_expr(expr, &ctx)
        };
        values.insert(id.clone(), v);
    }
    values
}

fn eval_at(
    expr: &CompiledExpr,
    base: &ValueMap,
    params: &[AutoParam],
    x: &[f64],
    dependent_cells: &[(ValueCellId, CompiledExpr)],
) -> f64 {
    let values = trial_values(base, params, x, dependent_cells);
    let ctx = EvalContext::simple(&values);
    eval_expr(expr, &ctx).as_f64().expect("residual must be a scalar at the probe point")
}

/// `∂expr/∂x_j` by central differences at the same step shape the rest of the
/// codebase uses (`calculus.rs:714`).
fn central_difference(
    expr: &CompiledExpr,
    base: &ValueMap,
    params: &[AutoParam],
    x: &[f64],
    j: usize,
    dependent_cells: &[(ValueCellId, CompiledExpr)],
) -> f64 {
    let h = 1e-6_f64 * x[j].abs().max(1e-3);
    let mut plus = x.to_vec();
    plus[j] += h;
    let mut minus = x.to_vec();
    minus[j] -= h;
    (eval_at(expr, base, params, &plus, dependent_cells)
        - eval_at(expr, base, params, &minus, dependent_cells))
        / (2.0 * h)
}

fn assert_row_matches_cd(
    label: &str,
    row: &[f64],
    expr: &CompiledExpr,
    base: &ValueMap,
    params: &[AutoParam],
    x: &[f64],
    dependent_cells: &[(ValueCellId, CompiledExpr)],
) {
    assert_eq!(row.len(), params.len(), "{label}: every row is exactly auto_params wide");
    for (j, &ad) in row.iter().enumerate() {
        let cd = central_difference(expr, base, params, x, j, dependent_cells);
        assert!(
            cd.abs() >= 0.1,
            "{label} column {j}: probe point must have |∂r/∂x_j| >= 0.1 in SI units so the \
             relative arm of the tolerance binds; got {cd:?}"
        );
        let tol = 1e-6 * cd.abs() + 1e-8;
        assert!(
            (ad - cd).abs() <= tol,
            "{label} column {j}: ad={ad:?} vs central difference {cd:?} (tol {tol:?})"
        );
    }
}

/// A two-auto smooth model: `w` and `h`, both lengths.
///
/// - `r0 = sqrt(w² + h²) − 5m`   (∂/∂w = 0.6, ∂/∂h = 0.8 at (3, 4))
/// - `r1 = w·h − 6m²`            (∂/∂w = h = 4, ∂/∂h = w = 3)
///
/// The two rows have deliberately different sensitivities so a transposed or
/// swapped column is visible rather than plausible.
fn two_auto_model() -> (Vec<AutoParam>, Vec<CompiledExpr>, ValueMap, Vec<f64>) {
    let params = vec![auto("w", DimensionVector::LENGTH), auto("h", DimensionVector::LENGTH)];
    let w = || aref("w", DimensionVector::LENGTH);
    let h = || aref("h", DimensionVector::LENGTH);
    let area = DimensionVector::LENGTH.mul(&DimensionVector::LENGTH);
    let r0 = binop(
        BinOp::Sub,
        call(
            "sqrt",
            vec![binop(
                BinOp::Add,
                binop(BinOp::Mul, w(), w()),
                binop(BinOp::Mul, h(), h()),
            )],
            DimensionVector::LENGTH,
        ),
        len_lit(5.0),
    );
    let r1 = binop(
        BinOp::Sub,
        binop(BinOp::Mul, w(), h()),
        literal(Value::Scalar { si_value: 6.0, dimension: area }),
    );
    (params, vec![r0, r1], ValueMap::new(), vec![3.0, 4.0])
}

fn jac(
    params: &[AutoParam],
    residuals: &[CompiledExpr],
    base: &ValueMap,
    x: &[f64],
) -> reify_constraints::Jacobian {
    residual_jacobian(params, residuals, base, x, &[], &[], None)
        .expect("this model is differentiable at this point")
}

// ---------------------------------------------------------------------------
// (1) Column order
// ---------------------------------------------------------------------------

#[test]
fn jacobian_columns_are_in_auto_params_order_positionally() {
    let (params, residuals, base, x) = two_auto_model();
    let j = jac(&params, &residuals, &base, &x);
    // Row 1 is w·h − 6: ∂/∂w = h = 4, ∂/∂h = w = 3.  A swapped column order
    // would put 3 first.
    assert!((j.rows[1][0] - 4.0).abs() < 1e-9, "column 0 must be ∂/∂w, got {:?}", j.rows[1]);
    assert!((j.rows[1][1] - 3.0).abs() < 1e-9, "column 1 must be ∂/∂h, got {:?}", j.rows[1]);
}

#[test]
fn permuting_auto_params_permutes_the_columns_the_same_way() {
    // The contract η relies on is POSITIONAL — column j is auto_params[j] —
    // not "whatever order the cells happen to hash into".
    let (params, residuals, base, x) = two_auto_model();
    let straight = jac(&params, &residuals, &base, &x);

    let swapped_params = vec![params[1].clone(), params[0].clone()];
    let swapped_x = vec![x[1], x[0]];
    let swapped = jac(&swapped_params, &residuals, &base, &swapped_x);

    assert!((swapped.rows[1][0] - straight.rows[1][1]).abs() < 1e-12);
    assert!((swapped.rows[1][1] - straight.rows[1][0]).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// (2) Shape
// ---------------------------------------------------------------------------

#[test]
fn the_jacobian_has_one_row_per_residual_and_one_column_per_auto() {
    let (params, residuals, base, x) = two_auto_model();
    let j = jac(&params, &residuals, &base, &x);
    assert_eq!(j.rows.len(), residuals.len());
    assert_eq!(j.residuals.len(), residuals.len());
    for row in &j.rows {
        assert_eq!(row.len(), params.len(), "no short rows — η indexes these directly");
    }
}

// ---------------------------------------------------------------------------
// (3) Agreement with central differences
// ---------------------------------------------------------------------------

#[test]
fn every_jacobian_entry_agrees_with_central_differences_over_the_same_residuals() {
    let (params, residuals, base, x) = two_auto_model();
    let j = jac(&params, &residuals, &base, &x);
    for (i, expr) in residuals.iter().enumerate() {
        assert_row_matches_cd(&format!("r{i}"), &j.rows[i], expr, &base, &params, &x, &[]);
    }
}

// ---------------------------------------------------------------------------
// (4) The residual values agree with ordinary evaluation
// ---------------------------------------------------------------------------

#[test]
fn the_reported_residuals_match_ordinary_evaluation_at_the_same_point() {
    // The AD path and the existing evaluation path must not disagree about the
    // VALUE — if they did, η would be stepping along a gradient of one function
    // while measuring the residual of another.
    let (params, residuals, base, x) = two_auto_model();
    let j = jac(&params, &residuals, &base, &x);
    for (i, expr) in residuals.iter().enumerate() {
        let expected = eval_at(expr, &base, &params, &x, &[]);
        assert_eq!(
            j.residuals[i], expected,
            "residual {i}: AD path reported {:?}, ordinary evaluation gives {expected:?}",
            j.residuals[i]
        );
    }
}

// ---------------------------------------------------------------------------
// (5) Mixed dimensions — columns are d(SI)/d(SI)
// ---------------------------------------------------------------------------

#[test]
fn an_angle_auto_and_a_length_auto_both_get_correct_si_unit_columns() {
    // r = sin(a)·w − 1m, with `a` an Angle and `w` a Length.
    //
    // Every tangent is d(SI)/d(SI) BY CONSTRUCTION — the dual carries si_value
    // derivatives, so a column is dimensionless in the sense that matters here
    // and needs no unit bookkeeping.  Rescaling columns for conditioning is ζ's
    // job, not ε's; ε's contract is only that the raw SI derivative is right.
    let params = vec![auto("a", DimensionVector::ANGLE), auto("w", DimensionVector::LENGTH)];
    let expr = binop(
        BinOp::Sub,
        binop(
            BinOp::Mul,
            call("sin", vec![aref("a", DimensionVector::ANGLE)], DimensionVector::DIMENSIONLESS),
            aref("w", DimensionVector::LENGTH),
        ),
        len_lit(1.0),
    );
    let base = ValueMap::new();
    let x = vec![0.5, 3.0];
    let j = jac(&params, std::slice::from_ref(&expr), &base, &x);

    assert_row_matches_cd("mixed_units", &j.rows[0], &expr, &base, &params, &x, &[]);
    // ∂r/∂a = cos(a)·w ≈ 2.633 per radian; ∂r/∂w = sin(a) ≈ 0.479 per metre.
    assert!((j.rows[0][0] - 0.5_f64.cos() * 3.0).abs() < 1e-9);
    assert!((j.rows[0][1] - 0.5_f64.sin()).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// (6) A per-row typed refusal, never a zero row
// ---------------------------------------------------------------------------

#[test]
fn a_residual_with_an_unsupported_seed_dependent_construct_refuses_by_row() {
    // A zero row here would tell η the residual is already stationary in both
    // variables, so it would stop pushing on a constraint it never actually
    // differentiated.  The refusal has to name the row AND the cause.
    let params = vec![auto("w", DimensionVector::LENGTH), auto("h", DimensionVector::LENGTH)];
    let good = binop(
        BinOp::Sub,
        binop(
            BinOp::Mul,
            aref("w", DimensionVector::LENGTH),
            aref("h", DimensionVector::LENGTH),
        ),
        literal(Value::Scalar {
            si_value: 6.0,
            dimension: DimensionVector::LENGTH.mul(&DimensionVector::LENGTH),
        }),
    );
    // `[w, h][0]` — an IndexAccess whose list depends on the autos.
    let bad = index_access(
        reify_test_support::builders::expr::list_expr(vec![
            aref("w", DimensionVector::LENGTH),
            aref("h", DimensionVector::LENGTH),
        ]),
        literal(Value::Int(0)),
    );
    let base = ValueMap::new();
    let x = vec![3.0, 4.0];

    let err = residual_jacobian(&params, &[good, bad], &base, &x, &[], &[], None)
        .expect_err("an undifferentiable residual must refuse, not return zeros");
    assert_eq!(err.row, 1, "the refusal names WHICH residual could not be differentiated");
    assert!(
        matches!(err.cause, NonDifferentiable::UnsupportedKind { .. }),
        "expected UnsupportedKind, got {:?}",
        err.cause
    );
    assert!(
        err.to_string().contains('1'),
        "the row index must survive into the message η shows the user: {err}"
    );
}

// ===========================================================================
// Step-17: dependent-cell tangent propagation
// ===========================================================================
//
// The #5189 β "stale base value" trap, in derivative form.
//
// In a whole-model joint drive the residual typically does NOT read the auto
// directly — it reads a DERIVED cell that is a function of it (the stdlib
// `Costed` trait's `line_cost = unit_cost * quantity_produced` is the canonical
// shape).  `build_trial_values` already re-folds those cells so the VALUE is
// right at every trial point.  The tangent needs the same treatment: without a
// dual-carrying fold, a derived cell is not a seed, so its tangent is
// `Tangent::Zero` and the whole column comes out exactly 0.0 — the residual
// looks flat in a variable it is entirely a function of.
//
// That is the silent-wrong-answer shape the PRD forbids, and it is worse here
// than for values: a stale value is at least a wrong NUMBER, while a zero
// derivative is a confident CLAIM that there is nothing to optimise.

fn dl() -> DimensionVector {
    DimensionVector::DIMENSIONLESS
}

fn dref(name: &str) -> CompiledExpr {
    value_ref_typed(ENT, name, scalar_ty(dl()))
}

fn num(v: f64) -> CompiledExpr {
    literal(Value::Real(v))
}

// ---------------------------------------------------------------------------
// (1) The single-hop case
// ---------------------------------------------------------------------------

#[test]
fn a_residual_reading_a_derived_cell_gets_a_nonzero_column_for_the_auto_behind_it() {
    // line_cost = unit_cost · q ; r = line_cost − 12.  ∂r/∂q = unit_cost = 3.
    let params = vec![auto("q", dl())];
    let mut base = ValueMap::new();
    base.insert(cell("unit_cost"), Value::Real(3.0));
    let dependent = vec![(
        cell("line_cost"),
        binop(BinOp::Mul, dref("unit_cost"), dref("q")),
    )];
    let residual = binop(BinOp::Sub, dref("line_cost"), num(12.0));
    let x = vec![5.0];

    let j = residual_jacobian(&params, std::slice::from_ref(&residual), &base, &x, &dependent, &[], None)
        .expect("differentiable through the fold");

    assert!(
        j.rows[0][0].abs() > 1e-9,
        "without a dual-carrying fold this column is exactly 0.0 — the residual would look \
         flat in the very variable it is a function of"
    );
    assert_row_matches_cd("line_cost", &j.rows[0], &residual, &base, &params, &x, &dependent);
    assert!((j.rows[0][0] - 3.0).abs() < 1e-9, "∂r/∂q = unit_cost = 3");
    assert_eq!(j.residuals[0], 3.0, "the primal still comes from the folded value path");
}

// ---------------------------------------------------------------------------
// (2) A chain, in stored order
// ---------------------------------------------------------------------------

#[test]
fn a_chain_of_dependent_cells_propagates_through_every_hop_in_stored_order() {
    // b = 2q ; c = b·b ; r = c − 90.  At q = 5: b = 10, c = 100, r = 10.
    // ∂c/∂q = 2b · 2 = 40 — only reachable if `b`'s tangent is visible to `c`,
    // which is exactly the stored-order guarantee.
    let params = vec![auto("q", dl())];
    let base = ValueMap::new();
    let dependent = vec![
        (cell("b"), binop(BinOp::Mul, num(2.0), dref("q"))),
        (cell("c"), binop(BinOp::Mul, dref("b"), dref("b"))),
    ];
    let residual = binop(BinOp::Sub, dref("c"), num(90.0));
    let x = vec![5.0];

    let j = residual_jacobian(&params, std::slice::from_ref(&residual), &base, &x, &dependent, &[], None)
        .expect("differentiable through both hops");
    assert_eq!(j.residuals[0], 10.0);
    assert!(
        (j.rows[0][0] - 40.0).abs() < 1e-9,
        "∂r/∂q = 40; got {:?} — a second hop that cannot see the first would give 20",
        j.rows[0][0]
    );
    assert_row_matches_cd("chain", &j.rows[0], &residual, &base, &params, &x, &dependent);
}

// ---------------------------------------------------------------------------
// (3) The auto's own tangent survives the fold
// ---------------------------------------------------------------------------

#[test]
fn the_fold_never_clobbers_an_auto_params_own_seed_column() {
    // The residual reads BOTH the auto directly and a derived cell, so if the
    // fold overwrote the auto's seed the direct term's contribution would
    // vanish.  r = q + line_cost − 12, ∂r/∂q = 1 + 3 = 4.
    let params = vec![auto("q", dl())];
    let mut base = ValueMap::new();
    base.insert(cell("unit_cost"), Value::Real(3.0));
    let dependent = vec![(
        cell("line_cost"),
        binop(BinOp::Mul, dref("unit_cost"), dref("q")),
    )];
    let residual = binop(
        BinOp::Sub,
        binop(BinOp::Add, dref("q"), dref("line_cost")),
        num(12.0),
    );
    let x = vec![5.0];

    let j = residual_jacobian(&params, std::slice::from_ref(&residual), &base, &x, &dependent, &[], None)
        .expect("differentiable");
    assert!(
        (j.rows[0][0] - 4.0).abs() < 1e-9,
        "the direct term contributes 1 and the derived term 3; got {:?}",
        j.rows[0][0]
    );
}

// ---------------------------------------------------------------------------
// (4) An empty dependent_cells list changes nothing
// ---------------------------------------------------------------------------

#[test]
fn an_empty_dependent_cells_list_leaves_the_non_clustered_path_bit_identical() {
    // Pinned to CONTENT, not to a recomputation of itself: comparing an empty
    // fold against `jac(..)` would be comparing one call to the same call with
    // the same arguments, which can only fail under nondeterminism.
    let (params, residuals, base, x) = two_auto_model();
    let empty = residual_jacobian(&params, &residuals, &base, &x, &[], &[], None).unwrap();

    assert_eq!(empty.residuals, vec![0.0, 6.0], "sqrt(3²+4²)−5 = 0 and 3·4−6 = 6");
    assert!((empty.rows[0][0] - 0.6).abs() < 1e-12, "∂r0/∂w, got {:?}", empty.rows[0]);
    assert!((empty.rows[0][1] - 0.8).abs() < 1e-12, "∂r0/∂h, got {:?}", empty.rows[0]);
    assert_eq!(empty.rows[1], vec![4.0, 3.0], "∂r1 = (h, w)");
    for (i, record) in empty.branch_records.iter().enumerate() {
        assert!(record.is_empty(), "row {i} has no fold prelude and no kink, got {record:?}");
    }

    // The contrast that gives the claim teeth: a fold that DOES run, over a
    // derived cell no residual reads.  The ROWS must be untouched — the cell
    // contributes no tangent to anything — while the RECORDS must not be, since
    // every row carries every dependent cell's branches by design.
    let unread = vec![(
        cell("unread"),
        call("clamp", vec![aref("w", DimensionVector::LENGTH), len_lit(1.0), len_lit(4.0)],
            DimensionVector::LENGTH),
    )];
    let folded =
        residual_jacobian(&params, &residuals, &base, &x, &unread, &[], None).unwrap();
    assert_eq!(folded.rows, empty.rows, "an unread derived cell moves no derivative");
    assert_eq!(folded.residuals, empty.residuals, "nor any primal");
    assert!(
        folded.branch_records.iter().all(|r| !r.is_empty()),
        "the unread cell's clamp still reaches every row — that is the fold running, and it \
         is what makes the empty-list assertions above a real claim"
    );
}

// ---------------------------------------------------------------------------
// (5) A non-differentiable dependent cell refuses only the rows that read it
// ---------------------------------------------------------------------------

#[test]
fn a_non_differentiable_dependent_cell_refuses_only_the_rows_that_read_it() {
    // `bad = [q, 1][0]` is seed-dependent and not differentiable.  A residual
    // that reads `bad` must refuse; one that reads only the auto must not — a
    // single poisoned cell cannot be allowed to condemn the whole system.
    let params = vec![auto("q", dl())];
    let base = ValueMap::new();
    let dependent = vec![(
        cell("bad"),
        index_access(
            reify_test_support::builders::expr::list_expr(vec![dref("q"), num(1.0)]),
            literal(Value::Int(0)),
        ),
    )];
    let clean = binop(BinOp::Sub, binop(BinOp::Mul, num(2.0), dref("q")), num(4.0));
    let poisoned = binop(BinOp::Sub, dref("bad"), num(4.0));
    let x = vec![5.0];

    // The clean residual alone is fine.
    let ok = residual_jacobian(&params, std::slice::from_ref(&clean), &base, &x, &dependent, &[], None)
        .expect("a residual that never reads the poisoned cell is unaffected");
    assert!((ok.rows[0][0] - 2.0).abs() < 1e-9);

    // The poisoned one refuses, and names its row.
    let err = residual_jacobian(
        &params,
        &[clean, poisoned],
        &base,
        &x,
        &dependent,
        &[],
        None,
    )
    .expect_err("a residual reading an undifferentiable derived cell must refuse");
    assert_eq!(err.row, 1, "the SECOND residual is the one that reads it");
}

// ===========================================================================
// Step-19: λ's end-to-end consumption contract, through the solver seam
// ===========================================================================
//
// This is what makes B19 implementable by λ (#6679) without re-deriving
// anything: the branch records reach λ through the same call that produces the
// Jacobian, so the record and the row it explains are always the same
// traversal of the same point.

use reify_expr::{BranchChoice, KinkKind};

/// `r = clamp(q, 1, 4) − 2`, over a single auto `q`.
///
/// Inside `[1, 4]` the residual tracks `q` and `∂r/∂q = 1`; outside it is
/// pinned to a constant bound and the derivative is genuinely 0.
fn clamped_model() -> (Vec<AutoParam>, CompiledExpr) {
    let params = vec![auto("q", dl())];
    let residual = binop(
        BinOp::Sub,
        call("clamp", vec![dref("q"), num(1.0), num(4.0)], dl()),
        num(2.0),
    );
    (params, residual)
}

fn clamp_jac(x: f64) -> reify_constraints::Jacobian {
    let (params, residual) = clamped_model();
    residual_jacobian(&params, &[residual], &ValueMap::new(), &[x], &[], &[], None)
        .expect("clamp is differentiable away from its bounds")
}

#[test]
fn residual_jacobian_returns_one_branch_record_per_row_naming_the_active_clamp_region() {
    for (x, expected) in [
        (0.5, BranchChoice::BelowLo),
        (2.5, BranchChoice::Interior),
        (7.0, BranchChoice::AboveHi),
    ] {
        let j = clamp_jac(x);
        assert_eq!(j.branch_records.len(), j.rows.len(), "one record per row, always");
        let entries = j.branch_records[0].entries();
        let clamp = entries
            .iter()
            .find(|e| e.kind == KinkKind::Clamp)
            .unwrap_or_else(|| panic!("x={x}: the clamp must appear in the record"));
        assert_eq!(clamp.choice, expected, "x={x}: the recorded region");
    }
}

#[test]
fn two_trial_points_across_a_clamp_bound_report_the_flip_and_name_its_site() {
    // This IS the primitive η's trust-region contraction and λ's chatter
    // counter consume: the two points are two different smooth functions, so
    // their Jacobians are not two samples of one.
    let inside = clamp_jac(2.5);
    let outside = clamp_jac(7.0);
    let (row, site) = inside
        .differs_from(&outside)
        .expect("crossing a clamp bound is a branch change, not a rounding difference");
    assert_eq!(row, 0, "the refusal names WHICH residual flipped");
    let entry = inside.branch_records[0]
        .entries()
        .iter()
        .find(|e| e.site == site)
        .expect("the named site must be one this record actually holds");
    assert_eq!(entry.kind, KinkKind::Clamp, "and it names the clamp, not some neighbour");
}

#[test]
fn two_trial_points_on_the_same_side_of_a_clamp_agree_on_signature_and_key() {
    let a = clamp_jac(2.0);
    let b = clamp_jac(3.0);
    assert_eq!(a.differs_from(&b), None, "same branches ⇒ one smooth function ⇒ no contraction");
    assert_eq!(
        a.signature_key(),
        b.signature_key(),
        "the problem-level key is a function of the branch set alone"
    );
    assert_ne!(
        a.signature_key(),
        clamp_jac(7.0).signature_key(),
        "and it must move when any row's branch set does"
    );
}

#[test]
fn the_clamp_row_carries_the_active_branch_derivative_and_the_record_explains_the_zero() {
    // Interior: the residual tracks q.
    assert!((clamp_jac(2.5).rows[0][0] - 1.0).abs() < 1e-12);

    // Outside: the derivative really IS zero — and the record is what
    // distinguishes that from "we could not differentiate".  A bare 0.0 in the
    // matrix cannot tell those apart; a `Clamp`/`AboveHi` entry beside it can.
    let above = clamp_jac(7.0);
    assert_eq!(above.rows[0][0], 0.0);
    assert!(
        above.branch_records[0].entries().iter().any(|e| e.kind == KinkKind::Clamp),
        "a zero row with an empty record would be indistinguishable from a refusal"
    );
}

#[test]
fn a_residual_with_no_kink_yields_an_empty_record() {
    // The negative control: an empty record must mean "this row's derivative is
    // an ordinary one", never "we did not look".
    let (params, residuals, base, x) = two_auto_model();
    let j = jac(&params, &residuals, &base, &x);
    for (i, record) in j.branch_records.iter().enumerate() {
        assert!(record.is_empty(), "row {i} is smooth, got {:?}", record.entries());
    }
    assert_eq!(
        j.signature_key(),
        jac(&params, &residuals, &base, &[3.5, 4.5]).signature_key(),
        "a smooth problem has the same signature everywhere"
    );
}

// ===========================================================================
// Step-27: a kink inside a DEPENDENT CELL must reach every row's BranchRecord
// ===========================================================================
//
// `fold_dependent_duals` evaluates each derived cell into a `BranchRecord` it
// throws away, so a kink that lives inside a derived cell is invisible to λ.
// That breaks the headline contract BOTH modules state — "'no entries' must
// mean 'no kink', never 'we did not look'" (`branch_signature.rs`) and "every
// row carries the non-smooth branches its traversal actually took" (this
// module's own header).
//
// It is not a cosmetic gap.  A clustered solve is exactly the case where the
// residuals read derived cells rather than the autos directly, so a `clamp` or
// an `if` in the middle of a cluster's algebra is precisely the kink most
// likely to exist — and today it produces an EMPTY record, which reads to η as
// "this row is a smooth-function sample, secant across it freely" and to λ as
// "no alternation to count".
//
// The fix re-sites each cell's entries under a reserved `DEPENDENT_MARKER`
// prefix and seeds every row's record with the resulting prelude, so the
// records reach λ through the API it already calls.

use reify_expr::DEPENDENT_MARKER;
use reify_test_support::builders::expr::conditional_expr;

/// `line_cost = clamp(q, 1, 4)`, `r = line_cost − 2`, over a single auto `q`.
///
/// Deliberately the same clamp as `clamped_model`, moved one hop away from the
/// residual: the ONLY difference from the step-19 fixture is that the kink now
/// lives in a derived cell, so any divergence between the two is the defect.
fn clamped_dependent_jac(x: f64) -> reify_constraints::Jacobian {
    let params = vec![auto("q", dl())];
    let dependent = vec![(
        cell("line_cost"),
        call("clamp", vec![dref("q"), num(1.0), num(4.0)], dl()),
    )];
    let residual = binop(BinOp::Sub, dref("line_cost"), num(2.0));
    residual_jacobian(&params, &[residual], &ValueMap::new(), &[x], &dependent, &[], None)
        .expect("a clamped derived cell is differentiable on either side of its bounds")
}

// ---------------------------------------------------------------------------
// (1) The record must exist at all
// ---------------------------------------------------------------------------

#[test]
fn a_kink_inside_a_dependent_cell_reaches_the_rows_branch_record() {
    for (x, expected) in [(2.5, BranchChoice::Interior), (7.0, BranchChoice::AboveHi)] {
        let j = clamped_dependent_jac(x);
        assert_eq!(j.branch_records.len(), j.rows.len(), "one record per row, always");
        let entries = j.branch_records[0].entries();
        assert!(
            !entries.is_empty(),
            "x={x}: an empty record claims this row is smooth — the clamp is one hop away, \
             not absent"
        );
        let clamp = entries.iter().find(|e| e.kind == KinkKind::Clamp).unwrap_or_else(|| {
            panic!("x={x}: the derived cell's clamp must appear, got {entries:?}")
        });
        assert_eq!(clamp.choice, expected, "x={x}: the recorded region");
    }
}

// ---------------------------------------------------------------------------
// (2) A flip in a dependent cell must move the signature
// ---------------------------------------------------------------------------

#[test]
fn a_dependent_cell_branch_flip_is_reported_and_moves_the_signature_key() {
    let inside = clamped_dependent_jac(2.5);
    let outside = clamped_dependent_jac(7.0);
    let (row, site) = inside.differs_from(&outside).expect(
        "crossing a bound INSIDE a derived cell is a branch change — without this η would \
         happily secant across the kink",
    );
    assert_eq!(row, 0, "the report names WHICH residual flipped");
    assert!(
        inside.branch_records[0].entries().iter().any(|e| e.site == site),
        "the named site must be one this record actually holds"
    );
    assert_ne!(
        inside.signature_key(),
        outside.signature_key(),
        "λ counts alternations between keys; identical keys are an alternation it cannot see"
    );
}

#[test]
fn two_points_on_the_same_side_of_a_dependent_cells_bound_add_no_chatter() {
    // The negative control that keeps the fix from being "report a flip always".
    let a = clamped_dependent_jac(2.5);
    let b = clamped_dependent_jac(3.0);
    assert_eq!(a.differs_from(&b), None, "same side of the bound ⇒ one smooth function");
    assert_eq!(a.signature_key(), b.signature_key(), "and therefore one signature");
}

// ---------------------------------------------------------------------------
// (3) A flip with a ZERO tangent on both sides
// ---------------------------------------------------------------------------

/// `tier = if q > 5 then 100 else 200`, `r = tier − 150`, over a single auto.
///
/// Flat on both sides, so the row is an honest zero either way — and yet the
/// VALUE jumps by 100 across `q = 5`, which is exactly the discontinuity η must
/// not secant across.
fn flat_flip_jac(x: f64) -> reify_constraints::Jacobian {
    let params = vec![auto("q", dl())];
    let dependent = vec![(
        cell("tier"),
        conditional_expr(binop(BinOp::Gt, dref("q"), num(5.0)), num(100.0), num(200.0)),
    )];
    let residual = binop(BinOp::Sub, dref("tier"), num(150.0));
    residual_jacobian(&params, &[residual], &ValueMap::new(), &[x], &dependent, &[], None)
        .expect("a piecewise-constant derived cell has an honest zero row")
}

#[test]
fn a_dependent_cell_that_flips_without_moving_still_reaches_the_record() {
    // This is the case a "bind only when the tangent is non-zero" design misses
    // by construction: `DualEnv::bind` deliberately DROPS a `Tangent::Zero`, so
    // collecting records only for bound cells would silently lose this one.
    let below = flat_flip_jac(4.0);
    let above = flat_flip_jac(7.0);

    assert_eq!(below.rows[0][0], 0.0, "flat below the threshold");
    assert_eq!(above.rows[0][0], 0.0, "flat above it too");
    assert_ne!(
        below.residuals[0], above.residuals[0],
        "the VALUE jumps across the threshold — that is what makes two zero rows a trap"
    );

    assert!(
        below.differs_from(&above).is_some(),
        "two zero rows either side of a jump discontinuity must NOT look like one smooth \
         function; the record is the only thing that can say so"
    );
    assert_ne!(below.signature_key(), above.signature_key());
}

// ---------------------------------------------------------------------------
// (4) Site hygiene
// ---------------------------------------------------------------------------

#[test]
fn each_dependent_cells_kinks_sit_at_distinct_sites_in_stored_order() {
    // Two derived clamps and one of the residual's own.  All three must be
    // separately addressable, or λ cannot say WHICH kink flipped — and a site
    // that collided with a real child index would name the wrong node.
    let params = vec![auto("q", dl())];
    let dependent = vec![
        (cell("a_cost"), call("clamp", vec![dref("q"), num(1.0), num(4.0)], dl())),
        (cell("b_cost"), call("clamp", vec![dref("q"), num(0.0), num(9.0)], dl())),
    ];
    let residual = binop(
        BinOp::Sub,
        binop(BinOp::Add, dref("a_cost"), dref("b_cost")),
        call("clamp", vec![dref("q"), num(2.0), num(6.0)], dl()),
    );
    let j = residual_jacobian(&params, &[residual], &ValueMap::new(), &[2.5], &dependent, &[], None)
        .expect("differentiable at an interior point of every bound");

    let clamps: Vec<_> =
        j.branch_records[0].entries().iter().filter(|e| e.kind == KinkKind::Clamp).collect();
    assert_eq!(clamps.len(), 3, "two derived clamps plus the residual's own, got {clamps:?}");

    assert_eq!(
        clamps[0].site.path(),
        [DEPENDENT_MARKER, 0],
        "dependent cell 0's clamp, at its own root under the reserved prefix"
    );
    assert_eq!(clamps[1].site.path(), [DEPENDENT_MARKER, 1], "dependent cell 1's clamp");
    assert_eq!(
        clamps[2].site.path(),
        [1_u16],
        "the residual's OWN clamp keeps its root-relative site — child 1 of the outer Sub"
    );

    let sites: std::collections::HashSet<_> = clamps.iter().map(|e| e.site.clone()).collect();
    assert_eq!(sites.len(), 3, "three kinks, three distinct sites");
}

// ---------------------------------------------------------------------------
// (5) No dependent cells ⇒ nothing added
// ---------------------------------------------------------------------------

#[test]
fn no_dependent_cells_leaves_the_records_exactly_as_the_non_clustered_path_produced_them() {
    // The record half of step-17's bit-identical invariant: an empty prelude
    // must add no marker segment and no empty prefix block, so every
    // non-clustered solve keeps precisely the sites it had before.
    let j = clamp_jac(2.5);
    let clamp = j.branch_records[0]
        .entries()
        .iter()
        .find(|e| e.kind == KinkKind::Clamp)
        .expect("the step-19 fixture's own clamp");
    assert_eq!(
        clamp.site.path(),
        [0_u16],
        "root-relative, with no dependent-cell prefix in front of it"
    );
}
