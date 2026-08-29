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

use reify_constraints::dual_jacobian::residual_jacobian;

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
fn trial_values(base: &ValueMap, params: &[AutoParam], x: &[f64]) -> ValueMap {
    assert_eq!(params.len(), x.len());
    let mut values = base.clone();
    for (param, &val) in params.iter().zip(x.iter()) {
        let dimension = match &param.param_type {
            Type::Scalar { dimension } => *dimension,
            _ => DimensionVector::DIMENSIONLESS,
        };
        values.insert(param.id.clone(), Value::Scalar { si_value: val, dimension });
    }
    values
}

fn eval_at(expr: &CompiledExpr, base: &ValueMap, params: &[AutoParam], x: &[f64]) -> f64 {
    let values = trial_values(base, params, x);
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
) -> f64 {
    let h = 1e-6_f64 * x[j].abs().max(1e-3);
    let mut plus = x.to_vec();
    plus[j] += h;
    let mut minus = x.to_vec();
    minus[j] -= h;
    (eval_at(expr, base, params, &plus) - eval_at(expr, base, params, &minus)) / (2.0 * h)
}

fn assert_row_matches_cd(
    label: &str,
    row: &[f64],
    expr: &CompiledExpr,
    base: &ValueMap,
    params: &[AutoParam],
    x: &[f64],
) {
    assert_eq!(row.len(), params.len(), "{label}: every row is exactly auto_params wide");
    for j in 0..params.len() {
        let cd = central_difference(expr, base, params, x, j);
        assert!(
            cd.abs() >= 0.1,
            "{label} column {j}: probe point must have |∂r/∂x_j| >= 0.1 in SI units so the \
             relative arm of the tolerance binds; got {cd:?}"
        );
        let tol = 1e-6 * cd.abs() + 1e-8;
        assert!(
            (row[j] - cd).abs() <= tol,
            "{label} column {j}: ad={:?} vs central difference {cd:?} (tol {tol:?})",
            row[j]
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
        assert_row_matches_cd(&format!("r{i}"), &j.rows[i], expr, &base, &params, &x);
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
        let expected = eval_at(expr, &base, &params, &x);
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

    assert_row_matches_cd("mixed_units", &j.rows[0], &expr, &base, &params, &x);
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
