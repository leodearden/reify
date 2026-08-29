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
//! # Branch records: ε emits, λ interprets
//!
//! Every row carries the non-smooth branches its traversal actually took (PRD
//! §7.7, "Kinks — active-branch (Clarke) Jacobian").  Emitting that record is
//! ε's job and it ends there.  Everything downstream of it is λ (#6679)'s:
//! assembling the Clarke Jacobian, contracting the trust region when the
//! signature changes, raising `W_SOLVER_NONSMOOTH_STALL` after K alternations
//! between two signatures, and choosing the derivative-free fallback.  ε
//! deliberately takes none of those decisions — a derivative source that also
//! decided when to distrust itself would be answering a question only the
//! optimiser has the context to answer.
//!
//! **A note λ needs:** a `KinkSite` is a STRUCTURAL child-index path, not a
//! `SourceSpan` — `CompiledExpr` carries no general span field (only
//! `StructureInstanceCtor` has one, `reify-ir/src/expr.rs:209`), so there is no
//! user-facing position to record here even in principle.  λ resolves the span
//! for `W_SOLVER_NONSMOOTH_STALL` from the owning constraint's
//! `ConstraintNodeId`, which `reify-constraints` already carries alongside
//! every `CompiledExpr`.
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

use reify_expr::{
    BranchRecord, DualEnv, KinkSite, NonDifferentiable, Seeds, Tangent, eval_dual_with_env,
    jacobian_row_with_env,
};
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

impl Jacobian {
    /// A stable, order-sensitive key for this Jacobian's whole branch set.
    ///
    /// Equal branch sets hash equal; a flip in ANY row moves the key.  λ counts
    /// alternations between keys, so the key is a pure function of the records
    /// and reproduces exactly when re-evaluated at the same point.
    pub fn signature_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.branch_records.len().hash(&mut hasher);
        for record in &self.branch_records {
            record.signature_key().hash(&mut hasher);
        }
        hasher.finish()
    }

    /// The first `(row, site)` at which these two Jacobians took different
    /// branches, or `None` when every row took the same ones.
    ///
    /// `Some(..)` means the two trial points sampled two DIFFERENT smooth
    /// functions, so their rows are not two samples of one — which is exactly
    /// when η must contract its trust region rather than trust a secant.
    pub fn differs_from(&self, other: &Jacobian) -> Option<(usize, KinkSite)> {
        for (i, (a, b)) in self.branch_records.iter().zip(other.branch_records.iter()).enumerate() {
            if let Some(site) = a.differs_from(b) {
                return Some((i, site));
            }
        }
        // Different row counts are a different PROBLEM, not a branch flip, so
        // there is no site to name; report the first row only one side has.
        let (longer, n) = if self.branch_records.len() > other.branch_records.len() {
            (self, other.branch_records.len())
        } else if other.branch_records.len() > self.branch_records.len() {
            (other, self.branch_records.len())
        } else {
            return None;
        };
        Some((
            n,
            longer.branch_records[n]
                .entries()
                .first()
                .map(|e| e.site.clone())
                .unwrap_or_else(KinkSite::root),
        ))
    }
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

    // The derivative sibling of the value fold `build_trial_values` just ran.
    let env = fold_dependent_duals(&values, auto_params, dependent_cells, functions, dispatch, &seeds);

    let mut rows = Vec::with_capacity(residual_exprs.len());
    let mut residuals = Vec::with_capacity(residual_exprs.len());
    let mut branch_records = Vec::with_capacity(residual_exprs.len());

    for (i, expr) in residual_exprs.iter().enumerate() {
        let mut record = BranchRecord::new();
        match jacobian_row_with_env(expr, &ctx, &seeds, &env, &mut record) {
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

/// The derivative sibling of [`crate::solver::fold_dependent_cells`].
///
/// `build_trial_values` has already folded the dependent cells' VALUES into
/// `values`; this recomputes the same cells' TANGENTS into a [`DualEnv`]
/// overlay, so a residual that reads a derived cell resolves to that cell's
/// real derivative instead of `Tangent::Zero`.
///
/// The value fold's doc comment warns "do NOT copy this body into a caller",
/// and this is not a copy: the values are consumed as `build_trial_values` left
/// them, and only the tangent half is computed here.  Everything the two folds
/// share — membership, stored order, the auto-collision backstop — is stated
/// once in each and MEANS the same thing, because both consume the same
/// `dependent_cells` slice.
///
/// `dependent_cells` is consumed IN STORED ORDER against a RUNNING overlay, so
/// an earlier dependent cell's tangent is visible to a later one.  That order
/// is a topologically-sorted guarantee produced once by `build_dependent_cells`
/// (reify-eval) and CONSUMED here — never re-derived, so the two can never
/// disagree.
///
/// # INVARIANTS
///
/// - An empty `dependent_cells` returns an empty overlay, and an empty overlay
///   is indistinguishable from no overlay at all — so every non-clustered solve
///   takes exactly the path it took before.
/// - The fold must NEVER bind an auto param's own cell.  An auto's tangent is
///   its seed column `e_j`, and `Seeds` resolves it before the overlay is even
///   consulted; binding it here would be dead at best and, if the resolution
///   order ever changed, would silently replace a basis vector with a computed
///   one.  Membership already excludes autos by construction, so this is a
///   backstop against upstream DRIFT — enforced rather than assumed, because a
///   clobbered auto column is silent: the solver would report a solved value
///   for a direction it never actually probed.
fn fold_dependent_duals(
    values: &ValueMap,
    auto_params: &[AutoParam],
    dependent_cells: &[(ValueCellId, CompiledExpr)],
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
    seeds: &Seeds,
) -> DualEnv {
    let mut env = DualEnv::new();
    if dependent_cells.is_empty() {
        return env;
    }
    let ctx = ctx_with(values, functions, dispatch);
    for (id, expr) in dependent_cells {
        if auto_params.iter().any(|p| &p.id == id) {
            debug_assert!(
                false,
                "fold_dependent_duals: dependent cell {id:?} collides with an auto param —                  reify-eval's `build_dependent_cells` excludes autos by construction, so this                  means upstream membership drifted. Skipping the entry to keep the auto's seed                  column."
            );
            continue;
        }
        // The VALUE is already in `values` (folded by `build_trial_values`), so
        // only the tangent is taken here; the primal this produces is
        // necessarily the same one, because both come from the same evaluator
        // over the same map.
        let mut discard = BranchRecord::new();
        let dual = eval_dual_with_env(expr, &ctx, seeds, &env, &mut discard);
        if !matches!(dual.tangent, Tangent::Zero) {
            env.bind(id.clone(), dual.tangent);
        }
    }
    env
}
