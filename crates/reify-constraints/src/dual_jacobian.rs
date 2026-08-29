//! Forward-mode AD at the solver seam — task #6672 (solver-unification ε).
//!
//! Design reference: `docs/prds/v0_6/geometry-algebra-solver-unification.md`
//! §7.7.
//!
//! This module is the adapter between `reify-expr`'s dual evaluator and the
//! solver's own notion of a trial point.  It publishes exactly one call, and
//! three consumers are waiting for it:
//!
//! - **η (#6675)** — the Gauss-Newton / Levenberg-Marquardt step, which needs
//!   `J` with columns in `auto_params` order and the `‖Jᵀr‖` stationarity
//!   certificate;
//! - **μ (#6680)** — the reduced gradient, which is this same call with a
//!   single objective expression;
//! - **λ (#6679)** — the per-row [`BranchRecord`]s.
//!
//! # What this module deliberately does NOT do
//!
//! ε adds a derivative source *beside* the existing solver.  It does not touch
//! `ConstraintCostFunction`, the Nelder-Mead loop, `comparison_violation`'s
//! `+1e-12` kinks, or `PENALTY_WEIGHT` — replacing the loop is η's job, and a
//! half-replacement would leave two solvers disagreeing about the same model.
//!
//! # Reuse, not re-implementation
//!
//! The trial point is materialised by the solver's own
//! [`crate::solver::build_trial_values`] and the context by
//! [`crate::solver::ctx_with`] — the site whose doc comment already designates
//! it "the single place a `reify_expr::EvalContext` is constructed in the
//! solver".  Routing the dual path through both is what guarantees the Jacobian
//! is taken at *the same point the solver is standing on*, rather than at a
//! reconstruction of it that could drift.

use reify_expr::{BranchRecord, NonDifferentiable, Seeds, jacobian_row};
use reify_core::ValueCellId;
use reify_ir::{AutoParam, CompiledExpr, CompiledFunction, ValueMap};

use crate::solver::{build_trial_values, ctx_with};

/// A Jacobian evaluated at one trial point.
///
/// `rows[i]` is `∂residual_i/∂x_j` for each auto param `j`, in `auto_params`
/// order; `residuals[i]` is that residual's own SI value at the same point.
/// The two are produced by ONE traversal each, so they cannot describe
/// different points.
#[derive(Debug, Clone, PartialEq)]
pub struct Jacobian {
    /// One gradient row per residual, each exactly `auto_params.len()` wide.
    pub rows: Vec<Vec<f64>>,
    /// The residual values at this point, in the same order as `rows`.
    pub residuals: Vec<f64>,
    /// The non-smooth branches each row's traversal actually took.
    /// `branch_records[i]` corresponds to `rows[i]`.
    pub branch_records: Vec<BranchRecord>,
}

/// Why a Jacobian could not be assembled, and WHICH residual is responsible.
///
/// The row index is carried because a Jacobian is only as good as its worst
/// row, and "some residual is not differentiable" is not something a user can
/// act on.  η turns this into a tier-2 refusal rather than stepping along a
/// matrix with a fabricated zero row in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JacobianError {
    /// Index into `residual_exprs` of the residual that could not be
    /// differentiated.
    pub row: usize,
    /// What went wrong in that residual.
    pub cause: NonDifferentiable,
}

impl std::fmt::Display for JacobianError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "residual {} has no usable gradient row: {}", self.row, self.cause)
    }
}

impl std::error::Error for JacobianError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

/// Assemble the Jacobian of `residual_exprs` with respect to `auto_params`, at
/// the trial point `x`.
///
/// Column `j` of every row is `∂r/∂x_j` for `auto_params[j]` — the SAME
/// positional mapping `build_trial_values` uses when it writes `x[i]` into
/// `params[i]`, so a caller never has to reconcile two orderings.
///
/// Derivatives are `d(SI)/d(SI)`: the dual carries `si_value` tangents, so a
/// column needs no unit bookkeeping. Rescaling columns for conditioning is ζ's
/// concern, not ε's; ε's contract is that the raw SI derivative is right.
///
/// # Errors
///
/// Returns the FIRST row that could not be differentiated, together with the
/// reason.  A row is never silently zero-filled: a zero row is the claim "this
/// residual does not move when you move this variable", and asserting it when
/// the truth is "we could not tell" leaves the solver believing it is already
/// stationary in a direction it has never actually probed.
#[allow(clippy::too_many_arguments)]
pub fn residual_jacobian(
    auto_params: &[AutoParam],
    residual_exprs: &[CompiledExpr],
    base_values: &ValueMap,
    x: &[f64],
    dependent_cells: &[(ValueCellId, CompiledExpr)],
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> Result<Jacobian, JacobianError> {
    assert_eq!(
        auto_params.len(),
        x.len(),
        "auto_params and x must have the same length — the mapping is positional"
    );

    // The same trial point the solver itself would stand on, built by the same
    // function, including the dependent-cell fold.
    let values =
        build_trial_values(base_values, auto_params, x, dependent_cells, functions, dispatch);
    let ctx = ctx_with(&values, functions, dispatch);

    // Seed columns in `auto_params` order — this IS the column order.
    let columns: Vec<ValueCellId> = auto_params.iter().map(|p| p.id.clone()).collect();
    let seeds = Seeds::new(&columns);

    let mut rows = Vec::with_capacity(residual_exprs.len());
    let mut residuals = Vec::with_capacity(residual_exprs.len());
    let mut branch_records = Vec::with_capacity(residual_exprs.len());

    for (i, expr) in residual_exprs.iter().enumerate() {
        let mut record = BranchRecord::new();
        match jacobian_row(expr, &ctx, &seeds, &mut record) {
            Ok((value, row)) => {
                residuals.push(value);
                rows.push(row);
                branch_records.push(record);
            }
            Err(cause) => return Err(JacobianError { row: i, cause }),
        }
    }

    Ok(Jacobian { rows, residuals, branch_records })
}
