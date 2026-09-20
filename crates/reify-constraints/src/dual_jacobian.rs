//! Forward-mode AD at the solver seam — task #6672 (solver-unification ε).
//!
//! Design reference: `docs/prds/v0_6/geometry-algebra-solver-unification.md`
//! §7.7.
//!
//! This module is the adapter between `reify-expr`'s dual evaluator and the
//! solver's own notion of a trial point.  Three consumers are waiting for it:
//! **η (#6675)** needs `J` with columns in `auto_params` order and the `‖Jᵀr‖`
//! stationarity certificate, **μ (#6680)** the same call with a single
//! objective expression, **λ (#6679)** the per-row [`BranchRecord`]s.
//!
//! # What this module deliberately does NOT do
//!
//! ε adds a derivative source *beside* the existing solver.  It does not touch
//! `ConstraintCostFunction`, the Nelder-Mead loop, `comparison_violation`'s
//! `+1e-12` kinks, or `PENALTY_WEIGHT` — replacing the loop is η's job, and a
//! half-replacement would leave two solvers disagreeing about the same model.
//!
//! It also only EMITS branch records (PRD §7.7, "Kinks — active-branch (Clarke)
//! Jacobian").  Assembling the Clarke Jacobian, contracting the trust region on
//! a signature change, raising `W_SOLVER_NONSMOOTH_STALL` after K alternations
//! and choosing the derivative-free fallback are all λ's: a derivative source
//! that also decided when to distrust itself would be answering a question only
//! the optimiser has the context to answer.
//!
//! **A note λ needs:** a `KinkSite` is a STRUCTURAL child-index path, not a
//! `SourceSpan` — `CompiledExpr` carries no general span field (only
//! `StructureInstanceCtor` has one, `reify-ir/src/expr.rs:209`), so there is no
//! user-facing position to record here even in principle.  λ resolves the span
//! for `W_SOLVER_NONSMOOTH_STALL` from the owning constraint's
//! `ConstraintNodeId`, which `reify-constraints` already carries alongside
//! every `CompiledExpr`.
//!
//! The trial point is materialised by the solver's own
//! [`crate::solver::build_trial_values`] and the context by
//! [`crate::solver::ctx_with`], so the Jacobian is taken at *the same point the
//! solver is standing on* rather than at a reconstruction of it that could
//! drift.

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

// The cause is flattened into `Display` and deliberately NOT also reported
// through `source()`: doing both renders the reason twice under any
// chain-aware printer, and reify has none — every caller prints `{err}`, so
// the flattened message is the one that has to carry the detail.  A consumer
// that wants the cause TYPED reads the `cause` field, which is strictly more
// useful than the `&dyn Error` a `source()` could hand back.
impl std::error::Error for JacobianError {}

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
/// # Panics
///
/// `x` must be as long as `auto_params`, and `auto_params` ids must be UNIQUE:
/// both are the positional column mapping, and a repeated id has no single
/// column to be.  Checked rather than assumed because the failure is silent —
/// [`Seeds`] keeps a duplicate's FIRST column while `build_trial_values` keeps
/// its LAST value, so the derivative would be attributed to one column and
/// taken at the other point.
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
    // Seed columns in `auto_params` order — this IS the column order.
    let columns: Vec<ValueCellId> = auto_params.iter().map(|p| p.id.clone()).collect();
    residual_jacobian_with_seeds(
        &Seeds::new(&columns),
        auto_params,
        residual_exprs,
        base_values,
        x,
        dependent_cells,
        functions,
        dispatch,
    )
}

/// [`residual_jacobian`] against a CALLER-OWNED seed set, so an iterating
/// consumer can hoist it out of its loop.
///
/// η (#6675) calls this once per Gauss-Newton / Levenberg-Marquardt trial
/// point with a seed set that is CONSTANT for the whole solve, and a `Seeds`
/// owns the `depends_on_seed` / `subtree_has_kink` memos.  Those two predicates
/// are pure functions of `(subtree, seed set)` — structural only, with no
/// dependence on the trial VALUES — so one `Seeds` is sound across every point
/// of a solve, and rebuilding it per point would throw away exactly the memos
/// that stop a cold descent re-walking each node's subtree.
///
/// # Panics
///
/// On [`residual_jacobian`]'s two preconditions, and on a `seeds` that is not
/// seeded on exactly `auto_params`, in order — a seed set is the column
/// contract, so one that disagrees would label every column with another
/// variable's name.
#[allow(clippy::too_many_arguments)]
pub fn residual_jacobian_with_seeds(
    seeds: &Seeds,
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
    // A hard assert, like its neighbour above and unlike the fold's drift
    // backstop below: there is no value-path sibling to stay quieter than, and
    // the damage lands in release, where η runs.
    if let Some(id) = repeated_auto_id(auto_params) {
        panic!(
            "auto_params names {id:?} twice — column j IS auto_params[j], so a repeated id has \
             no single column to be: its derivative would be attributed to the FIRST column \
             while `build_trial_values` stands the solve at the LAST one's value"
        );
    }
    assert!(
        seeds.columns().len() == auto_params.len()
            && seeds.columns().iter().zip(auto_params).all(|(c, p)| *c == p.id),
        "seeds must be seeded on exactly `auto_params`, in order — every row is reported as \
         ∂r/∂auto_params[j] in column j"
    );

    // The same trial point the solver itself would stand on, built by the same
    // function, including the dependent-cell fold.
    let values =
        build_trial_values(base_values, auto_params, x, dependent_cells, functions, dispatch);
    let ctx = ctx_with(&values, functions, dispatch);

    // The derivative sibling of the value fold `build_trial_values` just ran.
    // It hands back the dependent cells' own branch records as well as their
    // tangents: a kink inside a derived cell is a kink on every row that reads
    // it, and a record that stopped at the residual's own root would report
    // "smooth" for a point sitting on a clamp bound one hop away.
    //
    // Colliding ids come back reported rather than asserted on: the drift alarm
    // is `solver::fold_dependent_cells`', which `build_trial_values` has just
    // run over this same slice with this same predicate.  The SKIP is the half
    // that matters here, and it is asserted directly in `mod tests`.
    let (env, dependent_prelude, _collisions) =
        fold_dependent_duals(&values, auto_params, dependent_cells, functions, dispatch, seeds);

    let mut rows = Vec::with_capacity(residual_exprs.len());
    let mut residuals = Vec::with_capacity(residual_exprs.len());
    let mut branch_records = Vec::with_capacity(residual_exprs.len());

    for (i, expr) in residual_exprs.iter().enumerate() {
        // Seeded with the prelude, never appended to it afterwards:
        // `jacobian_row_with_env` only ever pushes, so the residual's own sites
        // land after the prelude's and `signature_key` / `differs_from` need no
        // change to see a dependent-cell flip.
        //
        // Every row gets its OWN copy, which is R×K small clones per evaluation
        // (K = prelude entries, and K = 0 for every non-clustered solve).  That
        // is deliberate: a flattened row record is self-contained, so λ can
        // read, compare or prune one row without having to remember to fold a
        // shared prefix back in — and forgetting that fold is precisely the
        // under-reporting the prelude exists to prevent.  An `Rc`-shared prefix
        // is what to reach for if a cluster ever carries enough dependent-cell
        // kinks for those clones to show up next to the traversal itself.
        let mut record = dependent_prelude.clone();
        match jacobian_row_with_env(expr, &ctx, seeds, &env, &mut record) {
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

/// The first `auto_params` id that appears more than once, if any.
///
/// A linear scan, for the same reason `build_trial_values`' own collision guard
/// is one: at the expected 1–3 autos (`WHOLE_MODEL_CLUSTER_DIM_CAP` = 12 is the
/// ceiling) it beats building a set, and this runs once per trial point.
fn repeated_auto_id(auto_params: &[AutoParam]) -> Option<&ValueCellId> {
    auto_params
        .iter()
        .enumerate()
        .find(|(i, p)| auto_params[..*i].iter().any(|earlier| earlier.id == p.id))
        .map(|(_, p)| &p.id)
}

/// The derivative sibling of [`crate::solver::fold_dependent_cells`]: that fold
/// has already put the dependent cells' VALUES into `values`, and this
/// recomputes the same cells' TANGENTS into a [`DualEnv`] overlay, so a
/// residual that reads a derived cell resolves to that cell's real derivative
/// instead of `Tangent::Zero`.
///
/// Membership, consumption in STORED topological order, and the rule that a
/// cell colliding with an auto param is skipped are the value fold's contract,
/// stated once on [`crate::solver::fold_dependent_cells`] and consumed here
/// over the same slice.  Colliding ids are RETURNED rather than asserted on:
/// that fold's `debug_assert!` has already run over this slice under the same
/// predicate, so a second alarm would be unreachable, while the skip stays
/// observable in both profiles.
///
/// Three things belong to the tangent half alone:
///
/// - **Why an auto must never be bound.** An auto's tangent IS its seed column
///   `e_j`, and the overlay OUTRANKS the seed columns — it is the inner scope,
///   so a callee's parameter can shadow an outer seed — which means binding an
///   auto here would not be dead, it would silently replace that basis vector
///   with a computed one and leave the solver reporting a solved value for a
///   direction it never probed.
/// - **Why EVERY cell's record, not just the bound ones.** The fold binds no
///   `Tangent::Zero` (at the residual root, unbound and zero-bound resolve
///   identically), but a zero-tangent cell can still flip a branch and JUMP its
///   value: `if q > 5 then 100 else 200` is flat on both sides and
///   discontinuous between them, which is exactly what η must not secant
///   across.
/// - **Why every row carries every cell's branches**, including cells it does
///   not read.  Over-reporting only makes η contract a trust region it could
///   have kept; under-reporting tells η a kinked row is smooth.
///
/// Records — and the REASON a cell could not be differentiated — are re-sited
/// under `[DEPENDENT_MARKER, k]` for the `enumerate` index `k`, so a derived
/// cell's kink is separately addressable from the residual's own and from its
/// siblings', and `JacobianError::cause` names the construct inside the derived
/// cell rather than a generic root-sited fallback.  An empty `dependent_cells`
/// returns an empty overlay, which is indistinguishable from no overlay at all,
/// so every non-clustered solve keeps precisely the sites it had before this
/// fold existed.
#[inline]
fn fold_dependent_duals(
    values: &ValueMap,
    auto_params: &[AutoParam],
    dependent_cells: &[(ValueCellId, CompiledExpr)],
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
    seeds: &Seeds,
) -> (DualEnv, BranchRecord, Vec<ValueCellId>) {
    let mut collisions = Vec::new();
    let mut env = DualEnv::new();
    let mut prelude = BranchRecord::new();
    if dependent_cells.is_empty() {
        // Not even an empty prefix block, and `Vec::new()` does not allocate,
        // so the non-clustered path stays allocation-free.
        return (env, prelude, collisions);
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
            // Leaving the cell UNBOUND is what preserves the auto's own seed
            // column `e_j`; reporting it is how a caller can see the drift.
            collisions.push(id.clone());
            continue;
        }
        // The VALUE is already in `values` (folded by `build_trial_values`), so
        // only the tangent is taken here.
        //
        // The cell is evaluated against the RUNNING overlay, so TANGENT chaining
        // composes for free.  Records do NOT nest, and the distinction matters:
        // evaluating cell k reads an earlier cell j as a `ValueRef`, taking j's
        // VALUE from `values` and j's TANGENT from `env` without re-traversing
        // j's expression — so j's kinks live ONLY under `[DEPENDENT_MARKER, j]`.
        // Each cell contributes its own kinks under its own prefix exactly once,
        // and the prelude is their union in stored order.  A reader who took a
        // later block to be self-contained could prune an earlier one and lose
        // it.
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
    (env, prelude, collisions)
}

#[cfg(test)]
mod tests {
    use reify_core::{DimensionVector, Type, ValueCellId};
    use reify_expr::{Seeds, Tangent};
    use reify_ir::{AutoParam, BinOp, Value, ValueMap};
    use reify_test_support::builders::expr::{binop, literal, value_ref_typed};

    use super::{CompiledExpr, fold_dependent_duals, repeated_auto_id};

    const ENT: &str = "part";

    fn cell(name: &str) -> ValueCellId {
        ValueCellId::new(ENT, name)
    }

    fn ty() -> Type {
        Type::Scalar { dimension: DimensionVector::DIMENSIONLESS }
    }

    fn auto(name: &str) -> AutoParam {
        AutoParam { id: cell(name), param_type: ty(), bounds: None, free: false }
    }

    fn dref(name: &str) -> CompiledExpr {
        value_ref_typed(ENT, name, ty())
    }

    /// `2 * <name>` — a cell whose tangent is non-flat, so a binding that lands
    /// is visible in the overlay and one that is skipped is visible by its
    /// absence.
    fn twice(name: &str) -> CompiledExpr {
        binop(BinOp::Mul, literal(Value::Real(2.0)), dref(name))
    }

    fn values_with(pairs: &[(&str, f64)]) -> ValueMap {
        let mut values = ValueMap::new();
        for (name, v) in pairs {
            values.insert(cell(name), Value::Real(*v));
        }
        values
    }

    #[test]
    fn a_repeated_auto_param_is_detected_and_the_repeat_is_named() {
        // The precondition `residual_jacobian_with_seeds` panics on, checked
        // here in both profiles: `Seeds` keeps a duplicate's FIRST column while
        // `build_trial_values` keeps its LAST value, so an unchecked repeat is
        // a derivative attributed to one column and taken at another point.
        assert_eq!(repeated_auto_id(&[auto("q"), auto("r")]), None, "distinct ids are fine");
        assert_eq!(
            repeated_auto_id(&[auto("q"), auto("r"), auto("q")]),
            Some(&cell("q")),
            "the REPEAT is named, so the panic can say which cell"
        );
        assert_eq!(repeated_auto_id(&[]), None, "an empty solve has nothing to repeat");
    }

    #[test]
    fn a_dependent_cell_colliding_with_an_auto_is_reported_and_skipped() {
        // The invariant whose failure is SILENT: an auto's tangent IS its seed
        // column `e_j`, and the overlay OUTRANKS the seed columns, so binding
        // an auto here would replace that basis vector with a computed one and
        // the solver would report a solved value for a direction it never
        // probed.  BOTH halves are asserted, because reporting without skipping
        // would still clobber the column.
        let params = [auto("q")];
        let dependent = vec![(cell("q"), twice("q"))];
        let values = values_with(&[("q", 5.0)]);
        let seeds = Seeds::new(&[cell("q")]);

        let (env, _prelude, collisions) = fold_dependent_duals(
            &values, &params, &dependent, &[], None, &seeds,
        );

        assert_eq!(collisions, vec![cell("q")], "the colliding id is reported, in stored order");
        assert!(
            env.get(&cell("q")).is_none(),
            "the auto's seed column must SURVIVE: leaving `q` unbound in the overlay is what \
             keeps the basis vector e_j outranked by nothing"
        );
    }

    #[test]
    fn a_collision_does_not_renumber_its_successors_sites() {
        // `enumerate` runs over the WHOLE slice, so a skipped entry leaves its
        // index spent rather than shifting everyone after it down one.  The
        // contract is stated in a comment at the loop head and checked by
        // nothing; a site must stay put across a change that has nothing to do
        // with it.
        let params = [auto("q")];
        let values = values_with(&[("q", 5.0), ("a", 3.0)]);
        let seeds = Seeds::new(&[cell("q")]);

        // `d` sits at index 1 in both lists; only the entry BEFORE it differs.
        let with_collision = vec![(cell("q"), twice("q")), (cell("d"), twice("a"))];
        let without = vec![(cell("other"), twice("a")), (cell("d"), twice("a"))];

        let (_e1, prelude_collided, collisions) = fold_dependent_duals(
            &values, &params, &with_collision, &[], None, &seeds,
        );
        let (_e2, prelude_clean, none) = fold_dependent_duals(
            &values, &params, &without, &[], None, &seeds,
        );

        assert_eq!(collisions, vec![cell("q")]);
        assert!(none.is_empty());
        assert_eq!(
            prelude_collided.signature_key(),
            prelude_clean.signature_key(),
            "a skipped entry must not renumber the sites of the entries after it"
        );
    }

    #[test]
    fn no_collision_returns_an_empty_vector_and_the_ordinary_overlay() {
        // The only correct steady state, which also keeps the fold honest about
        // the common path: an empty vector, and every cell bound.
        let params = [auto("q")];
        let dependent = vec![(cell("b"), twice("q")), (cell("c"), twice("b"))];
        let values = values_with(&[("q", 5.0), ("b", 10.0)]);
        let seeds = Seeds::new(&[cell("q")]);

        let (env, _prelude, collisions) = fold_dependent_duals(
            &values, &params, &dependent, &[], None, &seeds,
        );

        assert!(collisions.is_empty(), "the steady state reports nothing");
        assert_eq!(
            env.get(&cell("b")),
            Some(&Tangent::Scalar(vec![2.0])),
            "b = 2q binds ∂b/∂q = 2"
        );
        assert_eq!(
            env.get(&cell("c")),
            Some(&Tangent::Scalar(vec![4.0])),
            "c = 2b chains through the RUNNING overlay to ∂c/∂q = 4"
        );
    }
}
