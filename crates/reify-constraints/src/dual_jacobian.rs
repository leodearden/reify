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
    BranchRecord, DEPENDENT_MARKER, DualEnv, KinkSite, NonDifferentiable, Seeds, Tangent,
    eval_dual_with_env, first_divergence, jacobian_row_with_env,
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
        // The same "first position at which two ordered sequences diverge" rule
        // `BranchRecord::differs_from` applies to entries, applied here to rows
        // — one statement of it, in `reify_expr::first_divergence`.
        let row = first_divergence(&self.branch_records, &other.branch_records)?;
        let site = match (self.branch_records.get(row), other.branch_records.get(row)) {
            (Some(a), Some(b)) => a
                .differs_from(b)
                .expect("records at a divergent row must disagree with each other"),
            // Different row counts are a different PROBLEM, not a branch flip.
            // The extra row's first kink is the closest thing to a site; when
            // that row is smooth there is none, and `root` stands in.
            (Some(r), None) | (None, Some(r)) => {
                r.entries().first().map(|e| e.site.clone()).unwrap_or_else(KinkSite::root)
            }
            (None, None) => unreachable!("first_divergence returns an index one side holds"),
        };
        Some((row, site))
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
    // It hands back the dependent cells' own branch records as well as their
    // tangents: a kink inside a derived cell is a kink on every row that reads
    // it, and a record that stopped at the residual's own root would report
    // "smooth" for a point sitting on a clamp bound one hop away.
    let (env, dependent_prelude) =
        fold_dependent_duals(&values, auto_params, dependent_cells, functions, dispatch, &seeds);

    let mut rows = Vec::with_capacity(residual_exprs.len());
    let mut residuals = Vec::with_capacity(residual_exprs.len());
    let mut branch_records = Vec::with_capacity(residual_exprs.len());

    for (i, expr) in residual_exprs.iter().enumerate() {
        // Seeded with the prelude, never appended to it afterwards:
        // `jacobian_row_with_env` only ever pushes, so the residual's own sites
        // land after the prelude's and `signature_key` / `differs_from` need no
        // change to see a dependent-cell flip.
        let mut record = dependent_prelude.clone();
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
/// It also returns the cells' own [`BranchRecord`]s, concatenated in stored
/// order, each re-sited under `[DEPENDENT_MARKER, k]` for its `enumerate`
/// index `k`.  A cell that could NOT be differentiated carries its REASON the
/// same way — bound alongside the poisoned tangent and re-sited under the same
/// prefix — so `JacobianError::cause` names the construct inside the derived
/// cell rather than a generic root-sited fallback, and names it only to the
/// rows that read that cell.  That prefix is what keeps a derived cell's kink separately
/// addressable from the residual's own and from its sibling cells' — the
/// reserved segment can never be a structural child index, so a dependent-cell
/// site can never alias a real node.
///
/// # Why EVERY cell's record, not just the bound ones
///
/// A cell's record is collected whether or not its tangent is bound.
/// [`DualEnv::bind`] deliberately drops a `Tangent::Zero`, but a zero-tangent
/// cell can still flip a branch and JUMP its value — `if q > 5 then 100 else
/// 200` is flat on both sides and discontinuous between them, which is
/// precisely the point η must not secant across.  Collecting only bound cells
/// would miss exactly the case the record exists to catch.
///
/// # Deliberate over-reporting
///
/// Every row carries every dependent cell's branches, including cells that row
/// does not read.  That is the safe direction, and it is cheap: a dependent
/// cell is in the slice because the CLUSTER's residuals depend on it.
/// Under-reporting is the defect being fixed here — it tells η a kinked row is
/// smooth.  Over-reporting only makes η contract a trust region it could have
/// kept, and makes λ see an alternation slightly sooner.
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
) -> (DualEnv, BranchRecord) {
    let mut env = DualEnv::new();
    let mut prelude = BranchRecord::new();
    if dependent_cells.is_empty() {
        // No cells, no prefix block, not even an empty one — a non-clustered
        // solve keeps precisely the sites it had before this fold existed.
        return (env, prelude);
    }
    debug_assert!(
        dependent_cells.len() < DEPENDENT_MARKER as usize,
        "fold_dependent_duals: {} dependent cells — an index at or above DEPENDENT_MARKER \
         would alias a reserved path segment and make two different kinks compare equal",
        dependent_cells.len()
    );
    let ctx = ctx_with(values, functions, dispatch);
    // `enumerate` over the WHOLE slice, so a cell skipped by the collision
    // backstop below does not renumber its successors: a site stays put across
    // a change that has nothing to do with it.
    for (k, (id, expr)) in dependent_cells.iter().enumerate() {
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
        //
        // The cell is evaluated against the RUNNING overlay, so chaining
        // composes for free: a later cell's prefix block already contains the
        // branches taken by whatever it read.
        let mut record = BranchRecord::new();
        let dual = eval_dual_with_env(expr, &ctx, seeds, &env, &mut record);
        let prefix = [DEPENDENT_MARKER, k as u16];
        prelude.extend_from(&record.prefixed(&prefix));
        // Drained NOW, unconditionally: `eval_dual_with_env` clears the slot on
        // entry, so a reason left here is a reason the NEXT traversal — the
        // next cell, or the first residual row — silently discards.  That is
        // how a refusal originating in a derived cell used to reach the caller
        // as the generic root-sited "an expression with no derivative rule".
        let cause = seeds.take_refusal();
        match dual.tangent {
            // Flat cells bind nothing: unbound already means zero.
            Tangent::Zero => {}
            // The cause travels WITH the binding rather than in the shared
            // slot, so it is raised only by a row that actually reads this
            // cell — and it names the cell, because its site is re-rooted
            // under the same `[DEPENDENT_MARKER, k]` prefix the record half
            // uses.
            Tangent::None => env.bind_refused(
                id.clone(),
                cause
                    .unwrap_or(NonDifferentiable::UnsupportedKind {
                        kind: "a dependent cell with no derivative rule",
                        site: KinkSite::root(),
                    })
                    .prefixed(&prefix),
            ),
            tangent => env.bind(id.clone(), tangent),
        }
    }
    (env, prelude)
}
