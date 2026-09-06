// DimensionalSolver: Nelder-Mead based constraint solver for auto parameters.

use std::collections::{HashMap, HashSet};

use argmin::core::{CostFunction, Error as ArgminError, Executor, State, TerminationReason};
use argmin::solver::neldermead::NelderMead;
use reify_core::{
    ConstraintNodeId, DiagnosticCode, DimensionVector, Type, ValueCellId, hash::ContentHash,
};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, CompiledExprKind, CompiledFunction, ConstraintSolver,
    ObjectiveCombination, ObjectiveSense, ObjectiveSet, ResolutionProblem, SolveResult,
    TAG_CONDITIONAL, Value, ValueMap,
};

/// Maximum iterations for Nelder-Mead.
const MAX_ITERS: u64 = 5000;

/// Residual threshold below which we consider constraints satisfied.
const FEASIBILITY_THRESHOLD: f64 = 1e-12;

/// Penalty weight for constraint violations when optimizing an objective.
/// Large enough to strongly enforce constraints while allowing the objective
/// to steer the solution.
const PENALTY_WEIGHT: f64 = 1e6;

/// Penalty substituted when the objective expression evaluates to a non-numeric
/// value (Undef, NaN, Inf). Large enough to repel Nelder-Mead from non-numeric
/// regions, but not so large as to cause overflow when added to other penalties.
const UNDEF_OBJECTIVE_PENALTY: f64 = f64::MAX / 2.0;

/// Per-simplex-vertex iteration budget when the initial point is already feasible
/// and an objective is present. Nelder-Mead uses an (N+1)-vertex simplex, so the
/// total warm-start budget is `FEASIBLE_OPT_ITERS_PER_DIM * (n_params + 1)`,
/// capped at MAX_ITERS. This scales naturally with problem dimensionality.
const FEASIBLE_OPT_ITERS_PER_DIM: u64 = 500;

/// Standard-deviation tolerance for the Nelder-Mead simplex termination criterion.
///
/// ## Why this value must be ≤ FEASIBILITY_THRESHOLD²
///
/// The Nelder-Mead COST function (`ConstraintCostFunction::cost`) is the **sum of
/// squared** constraint violations: `comparison_violation` returns `d.powi(2)` (the
/// squared pointwise violation), and `compute_total_violation` sums them. Argmin's
/// `sd_tolerance` is the standard deviation of the cost values across the simplex
/// vertices; the solver terminates when that SD falls below this threshold.
///
/// Because the cost is quadratic in the linear residual `d`, a cost-SD floor of `S`
/// corresponds to a **linear residual floor** of approximately `√S`. To guarantee
/// that the linear residual (`max_constraint_residual`, compared against
/// `FEASIBILITY_THRESHOLD = 1e-12` at the final feasibility check) can actually reach
/// the threshold, we need:
///
/// ```text
///   √(NM_SD_TOLERANCE) ≲ FEASIBILITY_THRESHOLD   →   NM_SD_TOLERANCE ≲ 1e-24
/// ```
///
/// Setting `NM_SD_TOLERANCE = 1e-30` gives ~6 orders of margin below `(1e-12)² = 1e-24`.
/// Empirically, starting from a seed 2× away from the solution (e.g. 20 mm when the
/// target is 10 mm), the solver converges to a linear residual of ~1e-16 — well inside
/// the 1e-12 gate.
///
/// The f64 representational floor near typical engineering lengths (ULP² ≈ 1e-36 cost)
/// means Nelder-Mead still terminates quickly; the full reify-constraints test suite
/// (108 lib tests + all integration tests) passes with no measurable slowdown.
///
/// **Scale note — large-magnitude parameters:** `1e-30` is an *absolute* cost floor,
/// calibrated to the squared residual at engineering-length scales (lengths near 1–10 mm).
/// For parameters with large SI magnitudes (lengths near 1–10 m, or non-length dimensions
/// such as areas / volumes / forces with SI magnitudes ≫ 1), the squared-residual SD may
/// not fall below `1e-30` before machine precision, so Nelder-Mead runs to `MAX_ITERS =
/// 5000` rather than exiting early. This is a bounded cost: `MAX_ITERS` is the backstop
/// and the iteration cap is unchanged. It is also not a regression from the pre-#4700
/// value — the absolute `FEASIBILITY_THRESHOLD = 1e-12` already carries the same
/// scale dependence. The "no measurable slowdown" claim holds for the reify-constraints
/// test suite; large-magnitude problems are not represented there.
///
/// **Historical note:** the original value was `1e-15`. That floors the linear residual
/// at ~√(1e-15) ≈ 3e-8, making `FEASIBILITY_THRESHOLD = 1e-12` unreachable whenever
/// an auto param must move from an off-target seed. See task #4700 for the bug report
/// and empirical validation.
const NM_SD_TOLERANCE: f64 = 1e-30;

/// Metadata from a solve run — carries information that cannot be encoded in
/// [`SolveResult`] without a breaking API change across 6+ consumer crates (I1).
/// Threaded alongside `SolveResult` by the internal solver stack so that
/// `solve_ranked` can surface optimality without altering `solve()`'s output.
#[derive(Clone, Copy, Default)]
struct SolveMeta {
    /// `true` when the optimizer hit `MaxItersReached` while chasing an objective.
    /// Only meaningful when the accompanying `SolveResult` is `Solved`; callers
    /// should treat `false` as "converged or not applicable".
    iter_limited: bool,
}

/// Derivative-free constraint solver using Nelder-Mead optimization.
///
/// Solves for auto parameters by minimizing a penalty function that
/// encodes constraint violations. For pure feasibility (no objective),
/// the cost is the sum of squared constraint violations. For optimization,
/// the cost combines the objective value with a weighted penalty term.
pub struct DimensionalSolver;

/// Extract the DimensionVector from a Type, defaulting to DIMENSIONLESS.
fn dimension_of(ty: &Type) -> DimensionVector {
    match ty {
        Type::Scalar { dimension } => *dimension,
        _ => DimensionVector::DIMENSIONLESS,
    }
}

/// Build the solved-values HashMap from auto params and their f64 solutions.
///
/// Each param is mapped to a Value::Scalar with the correct SI value
/// and dimension. Used by early-exit, fallback, and solution construction paths.
fn build_solved_values(params: &[AutoParam], x: &[f64]) -> HashMap<ValueCellId, Value> {
    assert_eq!(
        params.len(),
        x.len(),
        "params and x must have the same length"
    );
    params
        .iter()
        .zip(x.iter())
        .map(|(param, &val)| {
            (
                param.id.clone(),
                Value::Scalar {
                    si_value: val,
                    dimension: dimension_of(&param.param_type),
                },
            )
        })
        .collect()
}

/// Build the per-trial expression-eval context for this module.
///
/// Task #4880: the single place a [`reify_expr::EvalContext`] is constructed in the
/// solver. With `dispatch: None` this is exactly `EvalContext::new(values, functions)`
/// — the pre-#4880 code path, byte for byte — so every existing caller and every
/// existing solver test is unaffected (invariant I1). With `Some(d)`, `@optimized`
/// function calls appearing inside constraint / objective expressions (e.g.
/// `solve_elastic_static(..)`) are resolved by `d` instead of body-evaluating to
/// `Value::Undef`, which is what makes FEA-in-the-loop optimisation possible: the
/// hook fires on EVERY Nelder-Mead trial point, so the cost surface actually varies
/// with the auto params the FEA call depends on.
fn ctx_with<'a>(
    values: &'a ValueMap,
    functions: &'a [CompiledFunction],
    dispatch: Option<&'a dyn reify_ir::ComputeDispatch>,
) -> reify_expr::EvalContext<'a> {
    let ctx = reify_expr::EvalContext::new(values, functions);
    match dispatch {
        Some(d) => ctx.with_compute_dispatch(d),
        None => ctx,
    }
}

/// Build a ValueMap from a base map with trial auto-param values inserted,
/// then recompute the cluster's dependent cells AT that trial point.
///
/// Clones the base map (O(1) via PersistentMap structural sharing) and
/// inserts each auto param as a Value::Scalar with the correct dimension.
/// Maps params directly to avoid the intermediate HashMap allocation that
/// `build_solved_values` would create — this is the hot path called on
/// every Nelder-Mead iteration.
///
/// # Why the fold exists (task #5189 β, PRD §6.2)
///
/// In a whole-model joint drive the objective typically does not read the auto
/// directly — it reads a DERIVED cell that is a function of the auto (the
/// stdlib `Costed` trait's `line_cost = unit_cost * quantity_produced` is the
/// canonical case). Those derived cells are non-auto values living in `base`,
/// so without this fold every trial point re-reads their STALE base value: the
/// objective is constant in the auto and Nelder-Mead has no gradient to follow.
///
/// `dependent_cells` is consumed IN STORED ORDER, and the `EvalContext` is
/// rebuilt against the RUNNING map each iteration so an earlier dependent cell
/// is visible to a later one. That order is a topologically-sorted guarantee
/// produced once by `build_dependent_cells` (reify-eval) and CONSUMED here —
/// never re-derived, so the two can never disagree.
///
/// # INVARIANTS
///
/// - An empty `dependent_cells` skips the fold entirely, leaving the returned
///   map byte-identical to the pre-β behaviour (PRD §6.2). Every non-clustered
///   solve therefore takes exactly the path it took before.
/// - The fold must NEVER overwrite an auto param's trial scalar. Membership
///   (reify-eval's `build_dependent_cells`) already excludes autos by
///   construction — stage (a) keeps only non-auto cells — so this is a
///   backstop against upstream DRIFT, not the primary mechanism. It is
///   enforced rather than assumed because a clobbered auto is silent: the
///   solver would go on to report a solved value for a point it never actually
///   evaluated. A collision is a membership BUG, so debug builds trip a
///   `debug_assert!` naming the offending cell; release builds skip the entry
///   and keep the trial point intact.
fn build_trial_values(
    base: &ValueMap,
    params: &[AutoParam],
    x: &[f64],
    dependent_cells: &[(ValueCellId, CompiledExpr)],
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> ValueMap {
    let mut values = base.clone();
    for (param, &val) in params.iter().zip(x.iter()) {
        values.insert(
            param.id.clone(),
            Value::Scalar {
                si_value: val,
                dimension: dimension_of(&param.param_type),
            },
        );
    }

    // Auto-collision guard: a linear scan over `params` beats a HashSet at the
    // expected 1–3 autos, and this is the per-Nelder-Mead-iteration hot path.
    fold_dependent_cells(
        &mut values,
        dependent_cells,
        functions,
        |id| params.iter().any(|p| &p.id == id),
        dispatch,
    );
    values
}

/// Relative inward nudge applied to a ONE-SIDED constraint-derived seed bound.
///
/// A one-sided derivation has no opposing bound to take a midpoint against (the
/// other side is still the useless `default_bounds_for` end), so the seed is placed
/// just inside the derived bound instead: `bound ± max(SEED_NUDGE_REL × |bound|,
/// SEED_NUDGE_ABS)`.
///
/// `0.1` is 5× [`REL_MARGIN`], so a one-sided seed clears the synthesised
/// robustness floor (`slack ≥ max(REL_MARGIN × |bound|, ABS_FLOOR_SI)`) with
/// headroom rather than landing marginally outside it.
const SEED_NUDGE_REL: f64 = 0.1;

/// Absolute floor for the one-sided seed nudge, so the nudge stays strictly
/// positive when the derived bound is ~0 (mirrors [`ABS_FLOOR_SI`]'s role for the
/// robustness margin).
const SEED_NUDGE_ABS: f64 = 1e-6;

/// Fold `dependent_cells` into `values` IN STORED ORDER — the single fold
/// authority shared by the per-trial cost surface ([`build_trial_values`]) and
/// the post-solve objective scoring ([`build_scoring_values`]).
///
/// Having one implementation rather than two copies is the drift guard: it
/// extends the PRD §6.3 "single authority" principle from the ORDER itself to
/// its consumers, so the cost surface Nelder-Mead minimises, the score the
/// ranker reports, and reify-eval's post-solve write-back cannot disagree about
/// what a dependent cell is worth at a given point.
///
/// The `EvalContext` is rebuilt against the RUNNING map on each iteration, so
/// an earlier dependent cell is visible to a later one — which is exactly what
/// makes the stored topological order load-bearing rather than incidental.
///
/// `is_solver_owned` identifies ids the SOLVER owns at this point (trial autos
/// for the cost path, solved autos for the scoring path). Such an id must never
/// be overwritten by the fold: reify-eval's `build_dependent_cells` excludes
/// autos by construction, so a collision means upstream membership drifted.
/// Debug builds trip a `debug_assert!` naming the cell; release builds skip the
/// entry and keep the solver's own value.
///
/// An empty `dependent_cells` returns without touching `values` OR running any
/// of the guard work — that zero-cost skip is what keeps every non-clustered
/// solve byte-identical to its pre-joint-drive behaviour (PRD §6.2).
///
/// # Hot-path cost model (task #5720)
///
/// This runs on every Nelder-Mead iteration of every multistart, so the cost
/// model is worth stating precisely — a plausible misreading of it leads
/// straight into a borrow-checker dead end.
///
/// - The list arriving here is PRE-FILTERED per component by
///   `SolverRegistry::solve_inner`: a component is handed only the cells whose
///   every transitively-read auto it OWNS. The per-iteration cost is therefore
///   bounded by that component's own cells, not by the whole model's
///   `dependent_cells`. Shrinking the list is the only real lever available,
///   and that filter is it.
/// - The per-cell `reify_expr::EvalContext::new(values, functions)` is NOT the
///   cost. It is a struct of two borrowed references, a zeroed recursion
///   counter and five `None` fields — no allocation, no hashing. The actual
///   per-iteration costs are the expression evaluation and one
///   persistent-`ValueMap` insert per cell, both linear in the (now filtered)
///   list length.
/// - The context CANNOT be hoisted out of the loop. It borrows `values`
///   IMMUTABLY for its whole lifetime, while the loop body needs
///   `values.insert(...)` — a mutable borrow of the same map. A hoist is a
///   borrow-checker impossibility, not a missed optimisation. It is also
///   semantically wrong: rebuilding against the RUNNING map each iteration is
///   exactly what makes an earlier dependent cell visible to a later one, which
///   is what makes the stored topological order load-bearing.
///
/// # Consumers (task #5467)
///
/// PRD2 §3 decision 9 mandates ONE fold body, not per-solver twins. There are
/// now THREE consumer classes, which is why this is `pub(crate)` rather than
/// private (`mod solver` is crate-private, so this is a no-op on the crate's
/// public API):
///
/// 1. `build_trial_values` — the DimensionalSolver residual/cost hot path.
/// 2. `build_scoring_values` — post-solve objective scoring.
/// 3. `cpsat::backtrack` — the CP-SAT forward-check, which must materialise
///    dependent cells per trial assignment or a constraint reading only a
///    dependent cell evaluates to a non-`Bool` and is never able to prune.
///
/// A fourth (ζ's mixed outer loop) is expected. Do NOT copy this body into a
/// caller: the two invariants a copy silently loses are consumption in STORED
/// topological order with the `EvalContext` rebuilt against the RUNNING map
/// each iteration (PRD §6.3 single-authority-on-order), and the
/// `is_solver_owned` guard that stops a fold from clobbering a trial auto.
pub(crate) fn fold_dependent_cells(
    values: &mut ValueMap,
    dependent_cells: &[(ValueCellId, CompiledExpr)],
    functions: &[CompiledFunction],
    is_solver_owned: impl Fn(&ValueCellId) -> bool,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) {
    if dependent_cells.is_empty() {
        return;
    }
    for (id, expr) in dependent_cells {
        if is_solver_owned(id) {
            debug_assert!(
                false,
                "fold_dependent_cells: dependent cell {id:?} collides with an \
                 auto param — reify-eval's `build_dependent_cells` excludes \
                 autos by construction, so this means upstream membership \
                 drifted. Skipping the entry to keep the solver's value."
            );
            continue;
        }
        let v = reify_expr::eval_expr(expr, &ctx_with(values, functions, dispatch));
        values.insert(id.clone(), v);
    }
}

/// Materialise the ValueMap an objective SCORE is read from: the problem's base
/// values, overlaid with the solver's `solved` autos, then folded through
/// `dependent_cells`.
///
/// Every post-solve scoring site used to build this map inline and WITHOUT the
/// fold, which meant they scored a dependent-cell-driven objective at its stale
/// base value. With the per-trial fold in place that made the optimiser and the
/// scorer measure different objectives. There were FOUR such sites, and each
/// failed differently — which is why they are now routed through ONE function
/// rather than fixed one at a time:
///
/// - the multistart scoring loop and `rank_single`: the reported
///   `objective_score` disagreed with the optimum actually achieved, and a
///   ranking over identical stale scores degenerated to a tie broken by start
///   index (PRD §12 Q2);
/// - `solve_cost_robustness_tradeoff`'s two anchor evaluations: `cost_expr`
///   returned the identical stale number at BOTH anchors, so `cost_max −
///   cost_min` collapsed to 0 and [`normalised_blend_term`]'s
///   [`TRADEOFF_NORMALISATION_RANGE_EPS`] guard dropped the cost axis from the
///   blend entirely, for every λ, with no diagnostic;
/// - `solve_lexicographic`'s ε-band anchor in `registry.rs` (esc-5189-7): the
///   band's `obj*` LITERAL was frozen from a stale map while the band's own
///   `cost_expr` is evaluated FOLDED by the next stage, so the two sides of one
///   constraint were measured on different value maps — surfacing as a bogus
///   `ConstraintUnsatisfiable` on a trivially feasible model when the stale
///   value sat on the restrictive side, and as a silently-dropped rank ordering
///   when it sat on the permissive side.
///
/// INVARIANT: no site in this CRATE may materialise a scoring map by hand from
/// `current_values.clone()` overlaid with the solved autos. `SolveResult::Solved`
/// carries only the AUTOS (`build_solved_values`), so a hand-rolled overlay
/// always leaves dependent cells stale — the failure is silent every time, and
/// the fourth occurrence is the argument for enforcing the rule rather than
/// restating it. A `grep` for `current_values.clone()` outside this function and
/// [`build_trial_values`] should return nothing.
///
/// The invariant is deliberately crate-scoped rather than module-scoped: the
/// fourth site lived in `registry.rs`, which a module-scoped rule did not
/// reach. `pub(crate)` exists so that rule is enforceable — a sibling module
/// needing a scoring map calls this rather than rolling its own.
pub(crate) fn build_scoring_values(
    base: &ValueMap,
    solved: &HashMap<ValueCellId, Value>,
    dependent_cells: &[(ValueCellId, CompiledExpr)],
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> ValueMap {
    let mut full = base.clone();
    for (id, v) in solved {
        full.insert(id.clone(), v.clone());
    }
    fold_dependent_cells(
        &mut full,
        dependent_cells,
        functions,
        |id| solved.contains_key(id),
        dispatch,
    );
    full
}

/// Extract initial parameter values from the problem.
///
/// Per auto param, the first applicable of:
///
/// 1. the current value, when present and numeric;
/// 2. the midpoint of an explicit [`AutoParam::bounds`];
/// 3. the **constraint-derived** box (task #5618) — midpoint when both sides were
///    derived, otherwise nudged inward from the single derived bound by
///    `max(SEED_NUDGE_REL × |bound|, SEED_NUDGE_ABS)`, clamped into the box;
/// 4. the fixed `0.01` fallback.
///
/// Arm 3 exists because `AutoParam.bounds` is always `None` in production, so arm 2
/// never fires there and an auto bracketed away from 0 (`q >= 1 ∧ q <= 100`) used to
/// seed at `0.01` — outside the synthesised robustness floor's window, which made
/// Nelder-Mead approach the feasible region from the wrong side and report a false
/// `RobustnessFloorInfeasible`. Strict comparisons DO contribute here
/// (`include_strict = true`): a start point may sit anywhere, unlike a clamp target.
fn extract_initial_point(
    problem: &ResolutionProblem,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> Vec<f64> {
    // Derived once per problem, from the ORIGINAL constraints (the synthesised
    // robustness floor does not exist yet at seed time).
    let intervals = derive_param_intervals(
        &problem.auto_params,
        &problem.constraints,
        &problem.current_values,
        &problem.functions,
        dispatch,
    );

    problem
        .auto_params
        .iter()
        .enumerate()
        .map(|(i, param)| {
            // Try current value first
            if let Some(val) = problem.current_values.get(&param.id)
                && let Some(f) = val.as_f64()
            {
                return f;
            }
            // Fall back to bounds midpoint
            if let Some((lo, hi)) = param.bounds {
                return (lo + hi) / 2.0;
            }
            // Fall back to the constraint-derived box (task #5618).
            if let Some((box_lo, box_hi)) = compose_interval(param, &intervals[i], true) {
                let nudge = |v: f64| (SEED_NUDGE_REL * v.abs()).max(SEED_NUDGE_ABS);
                match (intervals[i].lo, intervals[i].hi) {
                    (Some(_), Some(_)) => return (box_lo + box_hi) / 2.0,
                    (Some((lo, _)), None) => return (box_lo + nudge(lo)).clamp(box_lo, box_hi),
                    (None, Some((hi, _))) => return (box_hi - nudge(hi)).clamp(box_lo, box_hi),
                    (None, None) => {}
                }
            }
            // Default based on dimension
            0.01
        })
        .collect()
}

/// Compute the absolute (L1) residual for a single comparison expression.
///
/// Returns the absolute distance by which the constraint is violated,
/// or 0.0 if satisfied. No squaring, no epsilon offset. Used for
/// accurate feasibility checking (not for optimization cost).
fn comparison_residual(
    op: BinOp,
    left: &CompiledExpr,
    right: &CompiledExpr,
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> f64 {
    let lhs = reify_expr::eval_expr(left, &ctx_with(values, functions, dispatch)).as_f64();
    let rhs = reify_expr::eval_expr(right, &ctx_with(values, functions, dispatch)).as_f64();

    match (lhs, rhs) {
        (Some(l), Some(r)) => match op {
            BinOp::Gt => {
                if l > r {
                    0.0
                } else {
                    r - l
                }
            }
            BinOp::Ge => {
                if l >= r {
                    0.0
                } else {
                    r - l
                }
            }
            BinOp::Lt => {
                if l < r {
                    0.0
                } else {
                    l - r
                }
            }
            BinOp::Le => {
                if l <= r {
                    0.0
                } else {
                    l - r
                }
            }
            BinOp::Eq => {
                let d = (l - r).abs();
                if d < 1e-15 { 0.0 } else { d }
            }
            BinOp::Ne if (l - r).abs() > 1e-15 => 0.0,
            _ => 1.0,
        },
        _ => 1.0,
    }
}

/// Compute the violation magnitude for a single comparison expression.
///
/// For comparison operators (Gt, Ge, Lt, Le), evaluates the left and right
/// sub-expressions to get numeric values and computes a continuous violation.
/// Returns 0.0 if satisfied. For non-decomposable boolean constraints,
/// uses a fixed penalty when violated.
fn comparison_violation(
    op: BinOp,
    left: &CompiledExpr,
    right: &CompiledExpr,
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> f64 {
    let lhs = reify_expr::eval_expr(left, &ctx_with(values, functions, dispatch)).as_f64();
    let rhs = reify_expr::eval_expr(right, &ctx_with(values, functions, dispatch)).as_f64();

    match (lhs, rhs) {
        (Some(l), Some(r)) => match op {
            // For l > r: violation when l <= r, magnitude = (r - l)
            BinOp::Gt => {
                if l > r {
                    0.0
                } else {
                    (r - l + 1e-12).powi(2)
                }
            }
            // For l >= r: violation when l < r
            BinOp::Ge => {
                if l >= r {
                    0.0
                } else {
                    (r - l + 1e-12).powi(2)
                }
            }
            // For l < r: violation when l >= r, magnitude = (l - r)
            BinOp::Lt => {
                if l < r {
                    0.0
                } else {
                    (l - r + 1e-12).powi(2)
                }
            }
            // For l <= r: violation when l > r
            BinOp::Le => {
                if l <= r {
                    0.0
                } else {
                    (l - r + 1e-12).powi(2)
                }
            }
            // For equality: distance squared
            BinOp::Eq => {
                let d = l - r;
                if d.abs() < 1e-15 { 0.0 } else { d.powi(2) }
            }
            BinOp::Ne if (l - r).abs() > 1e-15 => 0.0,
            // Not a comparison
            _ => 1.0,
        },
        // Can't decompose numerically; use fixed penalty
        _ => 1.0,
    }
}

/// Compute the absolute (L1) residual for a single constraint expression.
///
/// Same decomposition structure as `constraint_violation` but returns
/// absolute residual values. For And composites, returns the max of
/// sub-residuals (both must hold). For Or, returns the min (one suffices).
fn constraint_residual(
    expr: &CompiledExpr,
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> f64 {
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            match op {
                BinOp::Gt | BinOp::Ge | BinOp::Lt | BinOp::Le | BinOp::Eq | BinOp::Ne => {
                    comparison_residual(*op, left, right, values, functions, dispatch)
                }
                BinOp::And => {
                    // AND: worst case (max) of sub-residuals
                    let lr = constraint_residual(left, values, functions, dispatch);
                    let rr = constraint_residual(right, values, functions, dispatch);
                    lr.max(rr)
                }
                BinOp::Or => {
                    // OR: best case (min) of sub-residuals
                    let lr = constraint_residual(left, values, functions, dispatch);
                    let rr = constraint_residual(right, values, functions, dispatch);
                    lr.min(rr)
                }
                _ => match reify_expr::eval_expr(expr, &ctx_with(values, functions, dispatch)) {
                    Value::Bool(true) => 0.0,
                    Value::Bool(false) => 1.0,
                    Value::Undef => 10.0,
                    _ => 1.0,
                },
            }
        }
        _ => match reify_expr::eval_expr(expr, &ctx_with(values, functions, dispatch)) {
            Value::Bool(true) => 0.0,
            Value::Bool(false) => 1.0,
            Value::Undef => 10.0,
            _ => 1.0,
        },
    }
}

/// Compute the violation for a single constraint expression.
///
/// Tries to decompose comparison expressions for continuous violation.
/// Falls back to binary penalty for non-decomposable expressions.
fn constraint_violation(
    expr: &CompiledExpr,
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> f64 {
    // First try decomposing into a comparison
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            match op {
                BinOp::Gt | BinOp::Ge | BinOp::Lt | BinOp::Le | BinOp::Eq | BinOp::Ne => {
                    comparison_violation(*op, left, right, values, functions, dispatch)
                }
                BinOp::And => {
                    // AND: sum violations of both sides
                    constraint_violation(left, values, functions, dispatch)
                        + constraint_violation(right, values, functions, dispatch)
                }
                BinOp::Or => {
                    // OR: minimum violation of both sides
                    let lv = constraint_violation(left, values, functions, dispatch);
                    let rv = constraint_violation(right, values, functions, dispatch);
                    lv.min(rv)
                }
                _ => {
                    // Not a logical/comparison op; evaluate as boolean
                    match reify_expr::eval_expr(expr, &ctx_with(values, functions, dispatch)) {
                        Value::Bool(true) => 0.0,
                        Value::Bool(false) => 1.0,
                        Value::Undef => 10.0,
                        _ => 1.0,
                    }
                }
            }
        }
        _ => {
            // Non-binop expression (e.g., literal bool, function call)
            match reify_expr::eval_expr(expr, &ctx_with(values, functions, dispatch)) {
                Value::Bool(true) => 0.0,
                Value::Bool(false) => 1.0,
                Value::Undef => 10.0,
                _ => 1.0,
            }
        }
    }
}

/// Compute the maximum absolute residual across all constraints (L1 feasibility).
///
/// Returns the worst-case per-constraint absolute residual. Zero means
/// all constraints are satisfied. Used for binary feasibility decisions
/// instead of sum-of-squares (which can mask small violations).
fn max_constraint_residual(
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> f64 {
    constraints
        .iter()
        .map(|(_, expr)| constraint_residual(expr, values, functions, dispatch))
        .fold(0.0_f64, f64::max)
}

/// Compute the total violation across all constraints.
///
/// Returns the sum of squared violations. Zero means all constraints
/// are satisfied.
fn compute_total_violation(
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> f64 {
    constraints
        .iter()
        .map(|(_, expr)| constraint_violation(expr, values, functions, dispatch))
        .sum()
}

/// Recursively collect signed-slack expressions from a single constraint expression.
///
/// For each inequality sub-expression, appends a `CompiledExpr` that evaluates
/// to a positive value when the constraint is interior (satisfied with margin)
/// and a negative value when violated:
///
/// - `BinOp::Ge` / `BinOp::Gt`: slack = `left − right`  (positive when `left ≥ right`)
/// - `BinOp::Le` / `BinOp::Lt`: slack = `right − left`  (positive when `right ≥ left`)
/// - `BinOp::And`: recurse into both branches
/// - `Eq`, `Ne`, `Or`, and all other ops: skip (no well-defined signed interior slack)
///
/// **Duplication note**: `engine_eval.rs::has_inequality_slack` mirrors this rule
/// exactly (same ops, same And-recursion, same skips).  The duplication is intentional
/// — the two crates cannot share a common helper without adding a reify-eval →
/// reify-constraints dependency, which would break dependency inversion.  If you change
/// the decomposition rules here, apply the same change to `has_inequality_slack` and
/// vice versa (both functions carry the cross-reference comment).
///
/// **Op-rule pact (three members, in-crate)**: `collect_floor_terms` and
/// `derive_from_expr` (task #5618) reuse this exact Ge/Gt/Le/Lt/And decomposition.
/// Any op-rule change here must be reflected in BOTH of them; all three carry the
/// cross-reference comment.  `derive_from_expr` holds only the OP half: its `And`
/// split was factored out to `for_each_leaf_conjunct`, which its two callers
/// (`derive_param_intervals`, `params_in_underivable_constraints`) drive.  A
/// change to the `And` rule itself therefore lands in three places, not four —
/// here, in `collect_floor_terms`, and in `for_each_leaf_conjunct`.
fn collect_slack_terms(expr: &CompiledExpr, slacks: &mut Vec<CompiledExpr>) {
    if let CompiledExprKind::BinOp { op, left, right } = &expr.kind {
        match op {
            BinOp::Ge | BinOp::Gt => {
                // Interior slack: left − right > 0 when left ≥ right (satisfied, interior)
                let slack_type = left.result_type.clone();
                slacks.push(CompiledExpr::binop(
                    BinOp::Sub,
                    (**left).clone(),
                    (**right).clone(),
                    slack_type,
                ));
            }
            BinOp::Le | BinOp::Lt => {
                // Interior slack: right − left > 0 when right ≥ left (satisfied, interior)
                let slack_type = right.result_type.clone();
                slacks.push(CompiledExpr::binop(
                    BinOp::Sub,
                    (**right).clone(),
                    (**left).clone(),
                    slack_type,
                ));
            }
            BinOp::And => {
                // Recurse: AND composes multiple inequalities
                collect_slack_terms(left, slacks);
                collect_slack_terms(right, slacks);
            }
            // Eq, Ne, Or, arithmetic ops — no well-defined signed interior slack
            _ => {}
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Robustness floor (task #4789 α — PRD docs/prds/v0_6/continuous-cost-minimisation.md §2.2/§8.1)
//
// When the objective is Money-dimensioned and at least one inequality constraint
// is present, the solver synthesises a per-constraint margin floor:
//   slack_i(x) ≥ m_i  where  m_i = max(REL_MARGIN × |bound_i|, ABS_FLOOR_SI)
//
// This parks auto values OFF the constraint boundary instead of on it.
//
// v1 design notes (recorded here per §2.2 breadcrumb requirement):
//   - Rejected: an opt-in `robust` keyword on `minimize` — needs grammar changes.
//   - Rejected: applying the floor to ALL objectives — breaks non-cost objectives
//     like objective_set_weighted.ri (both could be future extensions).
//   - Tolerance-scope finding: the per-purpose tolerance scope CANNOT supply a
//     per-constraint margin; it is entity-keyed only (active_tolerance_for(entity_ref)),
//     with no ConstraintNodeId lookup or margin field on ConstraintInput. Task δ
//     defers per-constraint sourcing to a follow-up; this v1 configurable default
//     (REL_MARGIN / ABS_FLOOR_SI) remains the source.
// ─────────────────────────────────────────────────────────────────────────────

/// Relative margin: 2% of the constraint bound magnitude.
///
/// Per-constraint m_i = max(REL_MARGIN × |bound_operand_at_seed|, ABS_FLOOR_SI).
/// Example: `x > 1mm` → scale = 1mm → m = 20µm → floor: x ≥ 1.02mm.
const REL_MARGIN: f64 = 0.02;

/// Absolute floor for the margin (strict-positivity / degeneracy guard).
///
/// Ensures m > 0 even when the bound operand is ~0 (e.g. `x > 0`).
const ABS_FLOOR_SI: f64 = 1e-9;

/// True iff an `ObjectiveSet` is Money-dimensioned.
///
/// An objective is Money-iff it is non-empty AND every term's expression has
/// `result_type == Scalar { dimension: MONEY }`.
///
/// **Duplication note**: `engine_eval.rs::objective_is_money` mirrors this
/// predicate exactly (same MONEY check, same non-empty guard). The duplication
/// is intentional — the two crates cannot share a helper without adding a
/// reify-eval → reify-constraints src dependency, which would break dependency
/// inversion. If you change the predicate here, apply the same change to
/// `engine_eval.rs::objective_is_money` and vice versa.
///
/// **Structural parity**: uses `matches!(Type::Scalar { dimension } if *dimension == MONEY)`
/// — the same form as `engine_eval.rs::objective_is_money` — so a reviewer can verify
/// the two mirrors at a glance.  Non-Scalar result types fail the `matches!` and return
/// `false` (same outcome as the former `dimension_of(ty)` path, which mapped any
/// non-Scalar to `DimensionVector::DIMENSIONLESS` ≠ MONEY).
fn objective_is_money(obj: &ObjectiveSet) -> bool {
    !obj.terms.is_empty()
        && obj.terms.iter().all(|t| {
            matches!(
                &t.expr.result_type,
                Type::Scalar { dimension } if *dimension == DimensionVector::MONEY
            )
        })
}

/// Recursively collect (slack_expr, bound_expr, slack_type) tuples from a
/// constraint expression, using the same Ge/Gt/Le/Lt/And decomposition as
/// `collect_slack_terms`.
///
/// `bound_expr` is the "far operand" of the inequality — the value we compare
/// against — so `robustness_margin_for` can evaluate it at the seed to derive
/// `scale_i = |eval(bound)|`.
///
/// - `Ge`/`Gt`: slack = left − right, bound = right, type = left.result_type
/// - `Le`/`Lt`: slack = right − left, bound = left,  type = right.result_type
/// - `And`: recurse into both branches
/// - All other ops: skip
///
/// **Parallel to `collect_slack_terms`**: any op-rule change there must also be
/// reflected here — and, since task #5618, in `derive_from_expr` as well. The
/// cross-reference comment in `collect_slack_terms` records the three-member pact;
/// keep all three in sync. `derive_from_expr`'s `And` half lives in
/// `for_each_leaf_conjunct`, so an `And`-rule change lands there rather than in
/// `derive_from_expr` itself.
fn collect_floor_terms(expr: &CompiledExpr, out: &mut Vec<(CompiledExpr, CompiledExpr, Type)>) {
    if let CompiledExprKind::BinOp { op, left, right } = &expr.kind {
        match op {
            BinOp::Ge | BinOp::Gt => {
                // slack = left − right  (positive when left ≥ right)
                // bound = right (the limit we must exceed)
                let slack_type = left.result_type.clone();
                let slack = CompiledExpr::binop(
                    BinOp::Sub,
                    (**left).clone(),
                    (**right).clone(),
                    slack_type.clone(),
                );
                out.push((slack, (**right).clone(), slack_type));
            }
            BinOp::Le | BinOp::Lt => {
                // slack = right − left  (positive when right ≥ left)
                // bound = left (the limit we must stay below)
                let slack_type = right.result_type.clone();
                let slack = CompiledExpr::binop(
                    BinOp::Sub,
                    (**right).clone(),
                    (**left).clone(),
                    slack_type.clone(),
                );
                out.push((slack, (**left).clone(), slack_type));
            }
            BinOp::And => {
                collect_floor_terms(left, out);
                collect_floor_terms(right, out);
            }
            _ => {}
        }
    }
}

/// Compute the robustness margin for one inequality constraint.
///
/// `m_i = max(REL_MARGIN × |eval(bound_expr)|, ABS_FLOOR_SI)`
///
/// `bound_expr` is the "far operand" collected by `collect_floor_terms`.
/// If it cannot be evaluated (Undef), fall back to ABS_FLOOR_SI only.
fn robustness_margin_for(
    bound_expr: &CompiledExpr,
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> f64 {
    let ctx = ctx_with(values, functions, dispatch);
    let scale = reify_expr::eval_expr(bound_expr, &ctx)
        .as_f64()
        .map_or(0.0, |v| v.abs());
    (REL_MARGIN * scale).max(ABS_FLOOR_SI)
}

/// Synthesise robustness floor constraints for a Money-dimensioned objective.
///
/// For each inequality slack collected from `constraints`, appends a synthetic
/// `Ge(slack_i, literal(m_i, dim_i))` constraint to `effective_constraints`.
/// Returns `true` if at least one floor constraint was added.
///
/// Called only when `objective_is_money(obj)` is true.  Non-Money objectives
/// → no call → `effective_constraints` equals `problem.constraints` verbatim
/// → bit-identical solve (invariant ii).
///
/// Emits a SIGKILL-safe breadcrumb into the constraint ID space by cloning
/// the FIRST original ConstraintNodeId for every synthetic floor entry.  The
/// ID is not used for diagnostics (the Infeasible diagnostic is emitted by the
/// caller); any stable ID avoids a panic in the consumption code.
fn synthesise_floor_constraints(
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    effective_constraints: &mut Vec<(ConstraintNodeId, CompiledExpr)>,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> bool {
    let mut floor_terms: Vec<(CompiledExpr, CompiledExpr, Type)> = Vec::new();
    for (_, expr) in constraints {
        collect_floor_terms(expr, &mut floor_terms);
    }
    if floor_terms.is_empty() {
        return false;
    }

    // Use the first original constraint's ID as a stable anchor for all floor
    // constraints (avoids panics; the ID is not diagnostic-significant here).
    let anchor_id = constraints[0].0.clone();

    for (slack_expr, bound_expr, slack_type) in floor_terms {
        let margin = robustness_margin_for(&bound_expr, values, functions, dispatch);
        let margin_literal = CompiledExpr::literal(
            Value::Scalar {
                si_value: margin,
                dimension: dimension_of(&slack_type),
            },
            Type::Scalar {
                dimension: dimension_of(&slack_type),
            },
        );
        let floor_constraint =
            CompiledExpr::binop(BinOp::Ge, slack_expr, margin_literal, Type::Bool);
        effective_constraints.push((anchor_id.clone(), floor_constraint));
    }
    true
}

/// The worst UNMET robustness-floor term at `values`, as `(achieved, required)`.
///
/// Input is the floor tail of `effective_constraints` — the entries
/// `synthesise_floor_constraints` appended, each of the form
/// `Ge(slack_expr, margin_literal)`. Evaluating both operands at `values` gives the
/// slack the returned point actually achieves and the margin it needed; "worst" is
/// the largest shortfall (`required − achieved`).
///
/// Returns `None` when every term is met, when the tail is empty, or when a term does
/// not evaluate numerically — all cases where the caller simply omits the detail
/// clause rather than reporting a number it cannot stand behind (task #5618 step-10).
///
/// Non-`Ge` entries are skipped defensively; `synthesise_floor_constraints` emits only
/// `Ge`, so this is a guard against a future shape change, not a live path.
fn worst_unmet_floor_term(
    floor_constraints: &[(ConstraintNodeId, CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> Option<(f64, f64)> {
    let ctx = ctx_with(values, functions, dispatch);
    floor_constraints
        .iter()
        .filter_map(|(_, expr)| match &expr.kind {
            CompiledExprKind::BinOp {
                op: BinOp::Ge,
                left,
                right,
            } => {
                let achieved = reify_expr::eval_expr(left, &ctx).as_f64()?;
                let required = reify_expr::eval_expr(right, &ctx).as_f64()?;
                (required > achieved).then_some((achieved, required))
            }
            _ => None,
        })
        .max_by(|(a_got, a_need), (b_got, b_need)| (a_need - a_got).total_cmp(&(b_need - b_got)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Constraint-derived parameter bounds (task #5618)
//
// `AutoParam.bounds` is **always `None`** in production — all three construction
// sites hardcode it (`reify-eval/src/engine_eval.rs:1436`, `engine_edit.rs:1470`,
// `:3635`) and no `.ri` surface sets it.  So `effective_bounds` always degrades to
// `default_bounds_for`, which for a dimensionless Real is `(-1e6, 1e6)`: useless as
// a seed source, as a Nelder-Mead step scale, and as a clamp target.  A Money
// objective over an auto bracketed away from 0 (`q >= 1 ∧ q <= 100`) therefore
// seeded at the fixed `0.01`, outside the synthesised robustness floor's window,
// and reported a false `RobustnessFloorInfeasible`.
//
// These helpers recover a usable box from the inequality constraints themselves.
// Two consumers, with different obligations:
//
//   - the SEED box, derived from `problem.constraints` with `include_strict = true`
//     (a start point may sit anywhere, so every inequality contributes);
//   - the CLAMP box, derived from `effective_constraints` — i.e. INCLUDING the
//     synthesised floor — with `include_strict = false`.  A clamp target is a value
//     the solver will actually return, so a `Gt`-sourced bound must never become
//     one: clamping `x > 5mm` to exactly 5mm violates the strict comparison and
//     would trade a false Infeasible for a different false Infeasible.  This costs
//     nothing in the case above: `synthesise_floor_constraints` emits its slack
//     constraints as `Ge`, and the floored bound is strictly interior to the
//     original `>`/`>=` bound by construction, so the clamp still gets it.
//
// The clamp is load-bearing, not just the seed.  Minimising `q + PENALTY_WEIGHT ·
// (1.02 − q)²` places the penalty method's unconstrained minimiser ~5e-7 BELOW the
// floor: a penalty method converges to a boundary it is pulled onto from the
// outside and can never satisfy it to `FEASIBILITY_THRESHOLD = 1e-12`.  Only the
// clamp snaps that undershoot onto the feasible optimum.
// ─────────────────────────────────────────────────────────────────────────────

/// A bounding interval derived for one auto param from the inequality constraints.
///
/// Each side carries `(value, strict)`, where `strict` is `true` when the tightest
/// contributing comparison was strict (`Gt`/`Lt`).  `None` means no usable
/// constraint bounded that side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct DerivedInterval {
    lo: Option<(f64, bool)>,
    hi: Option<(f64, bool)>,
}

impl DerivedInterval {
    /// Record a candidate lower bound, keeping the tightest (largest) value.
    ///
    /// On an exact tie a non-strict candidate displaces a strict one: a non-strict
    /// bound survives `include_strict = false` and is usable as a clamp target,
    /// whereas a strict one is dropped there.
    fn push_lo(&mut self, value: f64, strict: bool) {
        let tighter = match self.lo {
            None => true,
            Some((cur, cur_strict)) => value > cur || (value == cur && cur_strict && !strict),
        };
        if tighter {
            self.lo = Some((value, strict));
        }
    }

    /// Record a candidate upper bound, keeping the tightest (smallest) value.
    /// Same non-strict tie preference as [`DerivedInterval::push_lo`].
    fn push_hi(&mut self, value: f64, strict: bool) {
        let tighter = match self.hi {
            None => true,
            Some((cur, cur_strict)) => value < cur || (value == cur && cur_strict && !strict),
        };
        if tighter {
            self.hi = Some((value, strict));
        }
    }
}

/// Evaluate a constraint operand that must be CONSTANT with respect to the auto
/// params, returning its finite SI value.
///
/// Returns `None` when the operand references any auto param (its value would then
/// vary with the solve, so it is not a bound at all) or when it does not evaluate
/// to a finite `f64`.
///
/// **Reuse note**: this is `robustness_margin_for`'s far-operand evaluation idiom,
/// but with the opposite failure policy — that function falls back to
/// `ABS_FLOOR_SI` when the bound is `Undef`, because a margin that cannot be
/// scaled can still take its absolute floor.  A bound that cannot be evaluated
/// must never become a clamp, so here the whole constraint is skipped.
///
/// The auto-param test uses `CompiledExpr::collect_value_refs()`
/// (`reify-ir/src/expr.rs`), which is exactly the query needed.  It is load-bearing
/// for the CLAMP box: that box is derived against `trial_values`, in which the auto
/// params ARE bound, so an expression naming one would evaluate to a perfectly
/// finite number that is nonetheless not a constant.
fn constant_operand_value(
    expr: &CompiledExpr,
    auto_index: &HashMap<ValueCellId, usize>,
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> Option<f64> {
    if expr
        .collect_value_refs()
        .iter()
        .any(|id| auto_index.contains_key(id))
    {
        return None;
    }
    reify_expr::eval_expr(expr, &ctx_with(values, functions, dispatch))
        .as_f64()
        .filter(|v| v.is_finite())
}

/// Visit every LEAF CONJUNCT of `expr` — i.e. split on `BinOp::And` all the way
/// down and hand `f` each non-`And` node, in left-to-right order. A non-`And`
/// expression is its own single leaf.
///
/// THE ONLY `And`-splitting recursion in the derivation family (review
/// suggestion 3). [`derive_param_intervals`] and
/// [`params_in_underivable_constraints`] both drive it, so the two cannot
/// drift apart: before this extraction each carried its own copy, and the
/// per-conjunct abstention granularity documented on
/// [`params_in_underivable_constraints`] silently depended on those two copies
/// agreeing. If a future change teaches the family another STRUCTURAL
/// connective — splitting `Or`, the blind spot the abstention docs repeatedly
/// name — it belongs here, once, and both callers inherit it.
///
/// Deliberately NOT part of the `collect_slack_terms` op-rule pact: the pact
/// governs the per-leaf OP RULES (`Ge`/`Gt` vs `Le`/`Lt` vs skip), which
/// [`collect_slack_terms`], [`collect_floor_terms`] and [`derive_from_expr`]
/// each still own. This owns only the structural walk down to the leaves.
fn for_each_leaf_conjunct(expr: &CompiledExpr, f: &mut impl FnMut(&CompiledExpr)) {
    if let CompiledExprKind::BinOp {
        op: BinOp::And,
        left,
        right,
    } = &expr.kind
    {
        for_each_leaf_conjunct(left, &mut *f);
        for_each_leaf_conjunct(right, f);
    } else {
        f(expr);
    }
}

/// Derive one [`DerivedInterval`] per auto param (in `auto_params` order) from the
/// inequality constraints.
///
/// Pure function of its inputs — no RNG, clock or mutation of `problem`.
fn derive_param_intervals(
    auto_params: &[AutoParam],
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> Vec<DerivedInterval> {
    let mut out = vec![DerivedInterval::default(); auto_params.len()];
    if auto_params.is_empty() {
        return out;
    }
    let auto_index: HashMap<ValueCellId, usize> = auto_params
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.clone(), i))
        .collect();
    for (_, expr) in constraints {
        for_each_leaf_conjunct(expr, &mut |leaf| {
            derive_from_expr(leaf, &auto_index, values, functions, &mut out, dispatch);
        });
    }
    out
}

/// Per-leaf op-rule worker for [`derive_param_intervals`].
///
/// **Third member of the `collect_slack_terms` op-rule pact**: same
/// `Ge`/`Gt` → left-bounded-below, `Le`/`Lt` → left-bounded-above, everything
/// else (including `Eq`, `Ne`, `Or`) → skip.  Any op-rule change in
/// `collect_slack_terms` or `collect_floor_terms` must be reflected here.
///
/// Takes ONE leaf conjunct and does not recurse: the pact's `And` → recurse
/// half lives in [`for_each_leaf_conjunct`], which every caller drives (review
/// suggestion 3 — [`collect_underivable_in_leaf`] used to carry a second copy
/// of that recursion). Calling this directly on an `A AND B` node therefore
/// derives NOTHING, by design; go through [`for_each_leaf_conjunct`].
fn derive_from_expr(
    expr: &CompiledExpr,
    auto_index: &HashMap<ValueCellId, usize>,
    values: &ValueMap,
    functions: &[CompiledFunction],
    out: &mut [DerivedInterval],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) {
    let CompiledExprKind::BinOp { op, left, right } = &expr.kind else {
        return;
    };
    match op {
        BinOp::Ge | BinOp::Gt => {
            // left ≥ right → `left` bounded BELOW by right, `right` bounded ABOVE by left.
            let strict = matches!(op, BinOp::Gt);
            derive_from_side(
                left, right, true, strict, auto_index, values, functions, out, dispatch,
            );
            derive_from_side(
                right, left, false, strict, auto_index, values, functions, out, dispatch,
            );
        }
        BinOp::Le | BinOp::Lt => {
            // left ≤ right → `left` bounded ABOVE by right, `right` bounded BELOW by left.
            let strict = matches!(op, BinOp::Lt);
            derive_from_side(
                left, right, false, strict, auto_index, values, functions, out, dispatch,
            );
            derive_from_side(
                right, left, true, strict, auto_index, values, functions, out, dispatch,
            );
        }
        // And (split upstream by `for_each_leaf_conjunct`, so it cannot appear
        // here on the intended call path), Eq, Ne, Or and every arithmetic op:
        // no one-sided bound on a single auto.
        _ => {}
    }
}

/// Match `near` against the linear-in-one-auto shapes and, when `far` is constant,
/// record the implied bound.
///
/// `lower == true` means the inequality bounds `near` from BELOW (`near ≥ far`).
/// Four shapes are recognised — the last two are REQUIRED, because
/// `synthesise_floor_constraints` emits exactly that slack form:
///
/// - `p OP far`                → bound = `far`,        direction unchanged
/// - `p − k OP far`            → bound = `far + k`,    direction unchanged
/// - `k − p OP far`            → bound = `k − far`,    direction FLIPPED
///
/// (the "`far OP p`" shape is covered by the caller invoking this function once per
/// operand side).  Anything else is skipped.
#[allow(clippy::too_many_arguments)]
fn derive_from_side(
    near: &CompiledExpr,
    far: &CompiledExpr,
    lower: bool,
    strict: bool,
    auto_index: &HashMap<ValueCellId, usize>,
    values: &ValueMap,
    functions: &[CompiledFunction],
    out: &mut [DerivedInterval],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) {
    let Some(far_value) = constant_operand_value(far, auto_index, values, functions, dispatch)
    else {
        return;
    };
    match &near.kind {
        CompiledExprKind::ValueRef(id) => {
            if let Some(&i) = auto_index.get(id) {
                record_bound(&mut out[i], far_value, lower, strict);
            }
        }
        CompiledExprKind::BinOp {
            op: BinOp::Sub,
            left,
            right,
        } => {
            // `p − k OP far` → `p OP far + k`
            if let CompiledExprKind::ValueRef(id) = &left.kind
                && let Some(&i) = auto_index.get(id)
                && let Some(k) =
                    constant_operand_value(right, auto_index, values, functions, dispatch)
            {
                record_bound(&mut out[i], far_value + k, lower, strict);
                return;
            }
            // `k − p OP far` → `p OP′ k − far`  (multiplying by −1 flips the direction)
            if let CompiledExprKind::ValueRef(id) = &right.kind
                && let Some(&i) = auto_index.get(id)
                && let Some(k) =
                    constant_operand_value(left, auto_index, values, functions, dispatch)
            {
                record_bound(&mut out[i], k - far_value, !lower, strict);
            }
        }
        _ => {}
    }
}

/// Route one derived bound to the correct side of `iv`, dropping non-finite values.
fn record_bound(iv: &mut DerivedInterval, value: f64, lower: bool, strict: bool) {
    if !value.is_finite() {
        return;
    }
    if lower {
        iv.push_lo(value, strict);
    } else {
        iv.push_hi(value, strict);
    }
}

/// Intersect one derived interval with the param's pre-existing effective bounds.
///
/// Returns `None` when the composed box is empty (`!(lo < hi)`) or non-finite; the
/// caller then falls back to the pre-existing box WHOLESALE.  That guard is what
/// keeps a genuinely floor-empty problem reporting `Infeasible` exactly as it does
/// today: for `x > 10mm ∧ x < 10.3mm` the floored pair inverts (lo ≈ 0.0102 >
/// hi ≈ 0.0101), so no derived box is used at all.
///
/// The base is [`effective_bounds`], not `default_bounds_for`, so an EXPLICIT
/// `AutoParam.bounds` is intersected rather than widened.  The two coincide on
/// every production path (`bounds` is always `None` there), so this only ever
/// tightens behaviour relative to the plain default box.
fn compose_interval(
    param: &AutoParam,
    interval: &DerivedInterval,
    include_strict: bool,
) -> Option<(f64, f64)> {
    let (base_lo, base_hi) = effective_bounds(param);
    let usable = |side: Option<(f64, bool)>| {
        side.filter(|&(_, strict)| include_strict || !strict)
            .map(|(v, _)| v)
    };
    let lo = usable(interval.lo).map_or(base_lo, |v| v.max(base_lo));
    let hi = usable(interval.hi).map_or(base_hi, |v| v.min(base_hi));
    (lo.is_finite() && hi.is_finite() && lo < hi).then_some((lo, hi))
}

/// Resolve a usable optimiser box per auto param, composing each derived interval
/// with the param's [`effective_bounds`] and falling back to those bounds
/// wholesale when the composition is degenerate.
///
/// `include_strict = false` for the CLAMP box, `true` for SEED boxes — see the
/// section comment above for why the distinction is load-bearing.
fn resolve_bounds(
    auto_params: &[AutoParam],
    intervals: &[DerivedInterval],
    include_strict: bool,
) -> Vec<(f64, f64)> {
    auto_params
        .iter()
        .enumerate()
        .map(|(i, param)| {
            intervals
                .get(i)
                .and_then(|iv| compose_interval(param, iv, include_strict))
                .unwrap_or_else(|| effective_bounds(param))
        })
        .collect()
}

/// The #5618 constraint-derived SEED box for every `problem.auto_params`
/// entry: composes each param's [`derive_param_intervals`] interval with its
/// [`effective_bounds`] via [`resolve_bounds`], with `include_strict = true`
/// since a seed point may sit anywhere a start vector can legally begin —
/// unlike a CLAMP target (`resolve_bounds`'s `include_strict = false`
/// callers), which must never cross a strict inequality boundary.
///
/// Used by [`multistart_points`] (the multistart corner/midpoint anchors);
/// `verify_uniqueness` (the perturbation anchor) produces the SAME box, but
/// derives its intervals itself — it needs them raw for its γ branch too — and
/// composes them through this function's [`seed_box_from_intervals`] half. Per
/// task #5711 the two boxes must not diverge: a future change to which
/// constraint set feeds the derivation, or to the `include_strict` choice, has
/// exactly one place to land for both call sites.
fn derived_seed_box(
    problem: &ResolutionProblem,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> Vec<(f64, f64)> {
    seed_box_from_intervals(
        &problem.auto_params,
        &derive_param_intervals(
            &problem.auto_params,
            &problem.constraints,
            &problem.current_values,
            &problem.functions,
            dispatch,
        ),
    )
}

/// The COMPOSE half of [`derived_seed_box`], split out for a caller that has
/// already derived the intervals and needs them for a second purpose (today:
/// `verify_uniqueness`, whose γ branch reads the RAW `None`s that composing
/// through [`resolve_bounds`] would erase).
///
/// The split exists so that caller can derive ONCE instead of twice while the
/// `include_strict = true` decision — the thing that actually distinguishes a
/// SEED box from a CLAMP box, and the divergence [`derived_seed_box`]'s doc
/// warns about — still lives in exactly one place. Deriving separately at each
/// call site is what would let the two boxes drift; re-COMPOSING from the same
/// intervals through this one function cannot.
fn seed_box_from_intervals(
    auto_params: &[AutoParam],
    intervals: &[DerivedInterval],
) -> Vec<(f64, f64)> {
    resolve_bounds(auto_params, intervals, true)
}

/// Auto-param indices that appear in at least one constraint conjunct the bound
/// derivation could NOT read as a bound on them (task #5711, esc-5711-3).
///
/// **The rule is GENERAL, and its reach is wide.** A param is recorded here
/// whenever some leaf conjunct MENTIONS it (via
/// `CompiledExpr::collect_value_refs`, which walks every expression kind) while
/// yielding NO bound at all for it. That is deliberately not a list of
/// enumerated shapes: [`derive_from_expr`]/[`derive_from_side`] recognise only
/// `p OP c`, `p − k OP c` and `k − p OP c` on `Ge`/`Gt`/`Le`/`Lt` with a
/// CONSTANT, auto-free far operand, so *everything else* a user can legitimately
/// write derives to `None` and abstains. Examples, NOT an exhaustive taxonomy:
///
/// - `Eq` is skipped outright by the op rule, yet `constraint x == 10mm` is the
///   canonical DSL way to determine a strict auto
///   (`examples/auto_binding_sites.ri`);
/// - coefficient/nonlinear forms (`2*t > 3mm`, `t*t > 4`) match no shape;
/// - a COUPLED bound (`y < 5mm - x`) has a far operand naming another auto, so
///   [`constant_operand_value`] rejects it for BOTH params;
/// - `Or` is skipped by the same op rule as `Eq` (it is not split like `And`),
///   so a disjunctive conjunct abstains for every param it mentions;
/// - a SUM constraint (`x + y > 1mm`) puts the param inside an unrecognised
///   near-side expression;
/// - a DISPATCH-BACKED predicate (`stress(t) < LIMIT`, the FEA shape this
///   crate's own `fea_binding_problem` fixture uses) is unreadable on BOTH
///   sides: `derive_from_side` cannot see a `Call` on the near side, and
///   [`constant_operand_value`] rejects a far side naming the auto.
///
/// Those `None`s are derivation BLIND SPOTS, not evidence that the user left a
/// side unbounded — the distinction [`strict_autos_constraint_bracketed`] needs
/// in order to reserve its `false` verdict for params positively confirmed
/// unbounded.
///
/// ACCEPTED CONSEQUENCE (review, robustness): because the rule is general, a γ
/// model carrying ONE unreadable constraint abstains for every strict auto that
/// constraint mentions — including one whose sides really are
/// [`default_bounds_for`]'s. That over-abstention is the deliberate direction of
/// error: it can only turn a `ConstraintNonUnique` error into a `Solved`
/// verdict, never the reverse, and the alternative (reading a blind spot as
/// evidence) was MEASURED to reject valid, bounded models. Narrowing it means
/// teaching [`derive_from_expr`] the missing shapes, not tightening the test
/// here.
///
/// That consequence is PINNED, not merely narrated (review suggestion 2):
/// `gamma_one_sided_plus_unreadable_conjunct_abstains_to_solved`
/// (`tests/cost_robustness_tradeoff_blend.rs`) builds exactly the losing
/// shape — a genuinely unbounded upper side plus ONE redundant, unreadable
/// conjunct mentioning the same param — and measures both arms: without the
/// conjunct every λ errors `ConstraintNonUnique`; with it every λ reports
/// `Solved { unique: true }`, λ=0 landing on [`default_bounds_for`]'s 10 m
/// `Length` ceiling. Re-deciding the rule means re-deciding that test.
///
/// Conjuncts are split on `And` before the test, by the SAME
/// [`for_each_leaf_conjunct`] walk [`derive_param_intervals`] drives — one
/// recursion, so the two cannot disagree about what a leaf is (review
/// suggestion 3). Granularity is what the split buys: in
/// `x > 1mm AND y < 5mm - x` the first conjunct is readable and the second is
/// not, and per-conjunct scoring is what keeps `x` from being called readable
/// on the strength of a DIFFERENT conjunct while the one that actually
/// mentions it is opaque.
///
/// Pure function of its inputs — no solve, no I/O, no mutation.
fn params_in_underivable_constraints(
    auto_params: &[AutoParam],
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> HashSet<usize> {
    let mut out = HashSet::new();
    if auto_params.is_empty() {
        return out;
    }
    let auto_index: HashMap<ValueCellId, usize> = auto_params
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.clone(), i))
        .collect();
    // One scratch buffer for the whole walk, reset per leaf conjunct rather than
    // reallocated (review suggestion 1): `collect_underivable` scores EVERY leaf,
    // so a per-leaf `vec![DerivedInterval::default(); n]` allocated a fresh Vec
    // for each conjunct of every constraint.
    let mut scratch = vec![DerivedInterval::default(); auto_params.len()];
    for (_, expr) in constraints {
        for_each_leaf_conjunct(expr, &mut |leaf| {
            collect_underivable_in_leaf(
                leaf,
                &auto_index,
                values,
                functions,
                &mut scratch,
                &mut out,
                dispatch,
            );
        });
    }
    out
}

/// Per-leaf worker for [`params_in_underivable_constraints`]: scores ONE leaf
/// conjunct by re-running [`derive_from_expr`] on it into a scratch buffer and
/// comparing what it bounded against what it mentions.
///
/// Does not recurse — the caller drives [`for_each_leaf_conjunct`], the single
/// `And` split shared with [`derive_param_intervals`] (review suggestion 3).
/// Passing an `A AND B` node here directly would score the conjunction as ONE
/// leaf and lose the per-conjunct granularity this function exists to provide.
///
/// `scratch` is caller-owned and RESET (not reallocated) at each leaf; it must be
/// `auto_params.len()` long, since `derive_from_expr` indexes it by auto index.
/// Its contents carry no meaning across leaves — the per-conjunct granularity
/// documented on [`params_in_underivable_constraints`] depends on the reset.
fn collect_underivable_in_leaf(
    leaf: &CompiledExpr,
    auto_index: &HashMap<ValueCellId, usize>,
    values: &ValueMap,
    functions: &[CompiledFunction],
    scratch: &mut [DerivedInterval],
    out: &mut HashSet<usize>,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) {
    scratch.fill(DerivedInterval::default());
    derive_from_expr(leaf, auto_index, values, functions, scratch, dispatch);
    for id in leaf.collect_value_refs() {
        if let Some(&i) = auto_index.get(&id)
            && scratch
                .get(i)
                .is_some_and(|iv| iv.lo.is_none() && iv.hi.is_none())
        {
            out.insert(i);
        }
    }
}

/// Is EVERY strict (`!p.free`) auto param's derived interval bounded on BOTH
/// sides by the user's own constraints?
///
/// Pure predicate — no solve, no I/O, no mutation. Answers PRD
/// `docs/reify-implementation-architecture.md` §11.6 test (2) ("uniquely
/// optimal under the applicable objective") for the γ `cost_robustness_tradeoff`
/// path, where the perturbation machinery `verify_uniqueness` normally uses is
/// structurally inapplicable (see that function's doc for the measured ruling).
///
/// - Both sides constraint-derived ⇒ the objective's argmin is taken over an
///   interval the USER authored, so the resolved value is fixed by the user's
///   model: well-determined.
/// - A side missing AND the param mentioned in no constraint the derivation
///   failed to read ⇒ that side is supplied by [`default_bounds_for`], a
///   solver-internal default the user never wrote, so the resolved value is
///   DEFAULT-BOUNDS-determined rather than model-determined: exactly the
///   non-determinedness §11.6 exists to catch.
/// - A side missing but the param present in `underivable` ⇒ ABSTAIN, counting
///   the param as bracketed (esc-5711-3). The `None` there is a derivation
///   BLIND SPOT, not evidence about the user's model. Everything outside
///   [`derive_from_side`]'s three recognised shapes is invisible to
///   [`derive_param_intervals`] — `Eq`, coefficient, nonlinear, coupled, `Or`,
///   sum and dispatch-backed predicates among them, as EXAMPLES rather than a
///   taxonomy (see [`params_in_underivable_constraints`] for the general rule).
///   Letting one masquerade as "the user did not bound this side" converts
///   a valid, bounded γ model into a user-facing `error: strict auto parameter
///   resolution is not uniquely determined`. MEASURED before the fix, on this
///   branch: γ + `1mm<x<4mm ∧ y>1mm ∧ y < 5mm - x` reported
///   `ConstraintNonUnique` even though the region is bounded (x>1mm ⇒ y<4mm)
///   and the plain-`Minimize` path accepted the IDENTICAL constraints. `false`
///   is thereby reserved for params the derivation POSITIVELY confirms are
///   constraint-unbounded on a side.
///
/// Abstention is checked per-param against a MISSING SIDE, not against "no
/// interval data at all": in the coupled example above `x` has a readable
/// lower bound and only its upper side is opaque, so an all-or-nothing
/// abstention test would still have errored on it.
///
/// MONOTONE in `underivable`: growing that set can only move a param from "not
/// bracketed" to "abstain", never the reverse, so the verdict can only go
/// `false` → `true`. `verify_uniqueness` RELIES on this — it evaluates the
/// predicate against an empty set first and only builds the (per-conjunct,
/// eval-heavy) evidence set if that first answer is `false`. Keep the
/// `underivable.contains(&i) || …` shape; a rule that let the evidence set
/// REMOVE a bracketing would silently break that short-circuit.
/// `strict_autos_constraint_bracketed_abstains_for_underivable_param` pins both
/// directions on one fixture.
///
/// # Known, ACCEPTED gap: a blend that is FLAT over the bracket
///
/// This predicate answers §11.6 test (2) from the CONSTRAINTS alone; it never
/// evaluates the objective. That is exact only when the blend actually has a
/// unique argmin over the derived interval. When the γ cost expression does not
/// reference a bracketed strict auto (or ties across its interval) the argmin is
/// a SET, not a point, and this reports `true` — where the non-γ path's
/// [`classify_uniqueness`] tie arm deliberately reports `NonUnique` for the
/// analogous flat objective (`flat_objective_over_inequality_bracket_reports_non_unique`).
/// The two paths therefore give opposite verdicts on the same §11.6 question,
/// and that divergence is ACCEPTED here rather than fixed:
///
/// - the widening is MONOTONE. Before #5711 amendment 2 the γ path was 100%
///   `ConstraintNonUnique` for every strict auto (see the A/B table on
///   [`verify_uniqueness`]), so a flat-blend model reported an error then and
///   reports `Solved` now — no previously-`Solved` γ model changes verdict, and
///   no non-γ verdict is touched at all;
/// - closing it means evaluating the blend, and the blend is exactly the
///   seed-dependent dispatch this branch exists to route AROUND. Sampling it at
///   the interval endpoints would re-introduce a weaker version of the
///   measurement error the branch removes (an endpoint tie is not a flat
///   region, and a non-tie is not uniqueness).
///
/// `gamma_flat_blend_over_bracket_is_accepted_as_unique`
/// (`tests/cost_robustness_tradeoff_blend.rs`) PINS this gap as measured
/// behaviour rather than leaving it inferred. Deciding it the other way is a
/// §11.6 policy change for γ, and belongs in a task that can re-measure the
/// whole γ fixture set — not in a local tightening here. Already tracked:
/// task #6465 ("make the blend seed-invariant, or give under-determined γ
/// models a precise diagnostic"), filed by #5711's architect for exactly this
/// class of γ quality question. Do not re-file.
///
/// Free params are exempt: they carry no §11.6 obligation at all, and
/// [`finalise_uniqueness`] only reaches `verify_uniqueness` when at least one
/// param is strict.
///
/// PRECEDENCE for a strict param whose index has no corresponding `intervals`
/// entry (a length mismatch — always a caller bug): ABSTENTION WINS. The
/// `underivable.contains(&i) ||` short-circuit is evaluated BEFORE the
/// `intervals.get(i)` lookup, so such a param reads as BRACKETED when it is in
/// the abstention set, and as NOT bracketed — [`solutions_agree`]'s
/// loud-not-silent contract, rather than silently defaulting to "bracketed" —
/// only when it is not. That is deliberate and not merely incidental to the
/// short-circuit: an index the caller never derived an interval for is
/// evidence about the CALLER, never evidence that the user left a side
/// unbounded, so it must not override an explicit abstention. In
/// `verify_uniqueness`'s two-phase evaluation the loud reading is the one that
/// governs the first (empty-`underivable`) call, which is what keeps the bug
/// reachable at all rather than masked by an abstention that has not been
/// computed yet.
/// `strict_autos_constraint_bracketed_index_beyond_intervals_returns_false`
/// pins the loud half and
/// `strict_autos_constraint_bracketed_abstention_outranks_missing_interval`
/// the abstaining half.
///
/// Bound STRICTNESS is deliberately irrelevant — a `>`/`<` bound supplies its
/// side just as a `>=`/`<=` one does, mirroring [`derived_seed_box`]'s
/// `include_strict = true`. The question here is "did the user's constraints
/// supply this side", not "is it a legal clamp target".
///
/// Takes `intervals` rather than deriving them, so the caller can pass
/// [`derive_param_intervals`]' RAW output: composing through [`resolve_bounds`]
/// (as [`derived_seed_box`] does) substitutes [`effective_bounds`] for a missing
/// side and would erase exactly the `None`s this predicate keys on.
fn strict_autos_constraint_bracketed(
    auto_params: &[AutoParam],
    intervals: &[DerivedInterval],
    underivable: &HashSet<usize>,
) -> bool {
    auto_params
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.free)
        .all(|(i, _)| {
            underivable.contains(&i)
                || intervals
                    .get(i)
                    .is_some_and(|iv| iv.lo.is_some() && iv.hi.is_some())
        })
}

/// Build a default Chebyshev-centre (max-min slack) objective for a continuous scope
/// that has inequality constraints but no explicit user objective.
///
/// The synthetic objective `Maximize(min_j slack_j)` drives the solver to the
/// centre of the feasible region, not just any feasible boundary point (PRD η).
///
/// Returns `Some(ObjectiveSet)` when:
/// - All auto params have finite, valid effective bounds.
/// - At least one inequality constraint decomposes into a signed-slack expression.
///
/// Returns `None` when:
/// - Any auto param has non-finite (NaN/Inf) effective bounds → degenerate problem,
///   fall back to first-feasible behaviour to avoid panics in the optimiser's clamp path.
/// - There are no inequality slacks → pure-feasibility / first-feasible behaviour is
///   preserved (equality-only or unconstrained scopes are unaffected).
///
/// **Normalisation**: all slacks are used at raw SI scale (UNIFORM — same divisor for
/// the whole scope). With uniform scale the argmax of `min(slack_0, …, slack_n-1)` is
/// the Chebyshev centre regardless of the scale value (cancelled terms), so dividing
/// by any common constant is a no-op and is omitted for simplicity.
///
/// **Continuous-only guard**: the discrete-type guard (`Type::Scalar` check, B7) is
/// added in step-4; at this step the function is called only on Scalar problems.
///
/// `pub` (re-exported from `lib.rs`, mirroring `SolverRegistry` / the loop-closure
/// items) so the γ cost_robustness_tradeoff blend (task #4791) can use it both
/// internally (the λ=0 robustness anchor) and from integration tests as an
/// independent reference computation for the λ=0 ≡ centrality invariant (PRD
/// `docs/prds/v0_6/continuous-cost-minimisation.md` §8.1).
pub fn build_centrality_objective(
    auto_params: &[AutoParam],
    constraints: &[(ConstraintNodeId, CompiledExpr)],
) -> Option<ObjectiveSet> {
    // Continuous-only guard (PRD η, B7): return None unless every auto param has
    // a Scalar type.  Discrete (Int, Bool, Enum, …) scopes stay first-feasible;
    // the CP-SAT and SolveSpace solvers are separate impls and never reach this
    // function, so they are naturally unaffected.
    for param in auto_params {
        if !matches!(param.param_type, Type::Scalar { .. }) {
            return None;
        }
    }

    // Degenerate bounds guard: skip synthesis for any problem with non-finite
    // (NaN, ±Inf) effective bounds.  Such problems are already degenerate; synthesis
    // would proceed to the optimiser, whose `val.clamp(lo, hi)` panics on NaN bounds.
    for param in auto_params {
        let (lo, hi) = effective_bounds(param);
        if !lo.is_finite() || !hi.is_finite() {
            return None;
        }
    }

    // Collect signed-slack sub-expressions from all inequality constraints.
    let mut slacks: Vec<CompiledExpr> = Vec::new();
    for (_, expr) in constraints {
        collect_slack_terms(expr, &mut slacks);
    }

    // No inequality slacks → preserve first-feasible behaviour.
    if slacks.is_empty() {
        return None;
    }

    // Performance note: the nested-Conditional fold below has O(2^n) expression-tree
    // size in the number of slack terms, because each reduce step clones the accumulator
    // `a` into BOTH the condition (BinOp::Lt) AND the then-branch.  At n=2 this is ~2×;
    // at n=10 it is ~512×; at n=15 it exceeds 16 000 nodes.  Since eval_objective_set
    // traverses the expression on EVERY Nelder-Mead cost call (up to tens of thousands
    // of iterations), high slack counts produce exponential per-eval cost.
    //
    // Current usage: typical scopes have ≤ 6 inequality constraints per auto param, so
    // the blowup is modest (≤ 64×).  Warn when the count is unexpectedly high so
    // pathological cases are visible in logs rather than silently slow.
    const CENTRALITY_SLACK_WARN_THRESHOLD: usize = 10;
    if slacks.len() > CENTRALITY_SLACK_WARN_THRESHOLD {
        let approx_nodes = 1_usize
            .checked_shl(slacks.len() as u32)
            .unwrap_or(usize::MAX);
        tracing::warn!(
            slack_count = slacks.len(),
            approx_nodes,
            "centrality synthesis: {} inequality slacks produce a nested-Conditional \
             min-expression with ~{} nodes (O(2^n)); Nelder-Mead eval cost will be high. \
             Consider reducing inequality constraints in this scope.",
            slacks.len(),
            approx_nodes,
        );
    }

    // Fold slacks into min(s₀, s₁, …) via nested Conditionals.
    // min(a, b) = if a < b then a else b
    let min_expr = slacks.into_iter().reduce(|a, b| {
        let result_type = a.result_type.clone();
        // Condition: a < b  (Bool)
        let condition = CompiledExpr::binop(BinOp::Lt, a.clone(), b.clone(), Type::Bool);
        let cond_hash = ContentHash::of(&[TAG_CONDITIONAL])
            .combine(condition.content_hash)
            .combine(a.content_hash)
            .combine(b.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(a),
                else_branch: Box::new(b),
            },
            result_type,
            content_hash: cond_hash,
        }
    })?;

    // Maximise the minimum slack: x* = Chebyshev centre of the feasible region.
    Some(ObjectiveSet::single(ObjectiveSense::Maximize, min_expr))
}

/// Cost function adapter for argmin's Nelder-Mead solver.
///
/// Evaluates constraint violations (and optionally an objective) given
/// a parameter vector of f64 SI values.
struct ConstraintCostFunction<'a> {
    auto_params: &'a [AutoParam],
    constraints: &'a [(ConstraintNodeId, CompiledExpr)],
    base_values: &'a ValueMap,
    objective: Option<&'a ObjectiveSet>,
    functions: &'a [CompiledFunction],
    /// Clamp box, one entry per `auto_params` entry, from [`resolve_bounds`]
    /// (task #5618). Was `effective_bounds(param)` computed inline, which for a
    /// dimensionless Real is the useless `(-1e6, 1e6)`.
    bounds: &'a [(f64, f64)],
    /// Cluster cells that must be recomputed at every trial point — see
    /// [`build_trial_values`]. Empty for every non-clustered solve, which is
    /// what keeps the legacy cost surface bit-identical.
    dependent_cells: &'a [(ValueCellId, CompiledExpr)],
    /// `@optimized` compute-dispatch hook (task #4880). `None` on every legacy
    /// path, which makes [`ctx_with`] degenerate to `EvalContext::new` and the
    /// cost surface byte-identical to pre-#4880. `Some(d)` lets an `@optimized`
    /// call inside a constraint / objective expression (e.g.
    /// `solve_elastic_static(..)`) resolve to a REAL value at every Nelder-Mead
    /// trial point instead of body-evaluating to `Value::Undef`.
    ///
    /// `reify_ir::ComputeDispatch: Send + Sync`, so `&dyn ComputeDispatch` keeps
    /// `ConstraintCostFunction` `Send + Sync` as argmin's `Executor` requires.
    dispatch: Option<&'a dyn reify_ir::ComputeDispatch>,
}

/// Evaluate an `ObjectiveSet` as a single f64 cost using the I2-preserving
/// additive fold (PRD §6.2 I3):
///
///   acc = 0.0
///   for each term t:
///     v = eval(t.expr)           — returns None if Undef or non-finite
///     Minimize → acc += t.weight * v
///     Maximize → acc -= t.weight * v
///
/// Returns `None` if ANY term evaluates to a non-numeric / non-finite value,
/// preserving the single-term None → UNDEF_OBJECTIVE_PENALTY / NoProgress paths.
///
/// I2 numerical equivalence: for a single term with weight 1.0,
///   Minimize → 0.0 + 1.0·v == v  (IEEE-754, finite v)
///   Maximize → 0.0 − 1.0·v == -v (IEEE-754, finite v)
/// both are numerically equivalent to the former single-variant objective enum eval
/// (modulo signed-zero, which is solver-irrelevant: −0.0 == 0.0 in all IEEE-754
/// comparisons and additions used by Nelder-Mead).
///
/// Lexicographic folds as WeightedSum here (degenerate, PRD §6.3); full
/// ε-band staged solve is task ε.
///
/// `pub(crate)` rather than private (task β #5468): `cpsat`'s `solve_ranked`
/// override scores its enumerated models through THIS function. Both the
/// discrete and the continuous path therefore fold an objective the same way —
/// same weight application, same `Maximize` → negation normalisation to
/// "lower is better" (F-result I2), same non-finite → `None` rejection. A
/// second, cpsat-local fold would be free to disagree with this one about any
/// of those, and nothing would catch it (PRD2 §3.9, G7).
pub(crate) fn eval_objective_set(
    objective: &ObjectiveSet,
    values: &ValueMap,
    functions: &[CompiledFunction],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> Option<f64> {
    // Guard: only WeightedSum is implemented here.  A Lexicographic set must
    // not be silently mis-solved as a weighted sum.  Assert in debug builds;
    // task ε will implement the full ε-band staged solve.
    debug_assert!(
        matches!(objective.combination, ObjectiveCombination::WeightedSum),
        "eval_objective_set: Lexicographic combination is not yet implemented \
         (task ε owns the ε-band staged solve); received {:?}",
        objective.combination,
    );
    // I-UNITS backstop (PRD D2/I-UNITS, task α #5018): this does NOT re-diagnose —
    // the compile-time gate (E_OBJECTIVE_MIXED_DIMENSION, `check_objective_dimension_coherence`
    // in reify-compiler/src/entity.rs) is the sole user-facing diagnostic and already
    // rejects every authored incoherent multi-term objective before it can reach a
    // solve. This assert only guards the upstream-guaranteed invariant against a
    // future ungated ObjectiveSet (e.g. hand-built or solve-time-synthesized).
    debug_assert!(
        reify_ir::objective_terms_coherent(&objective.terms).is_ok(),
        "eval_objective_set: I-UNITS violated (task α #5018) — objective_terms_coherent() \
         reported Err for a set that reached the fold; the compile-time gate \
         (E_OBJECTIVE_MIXED_DIMENSION, reify-compiler/src/entity.rs) should have \
         rejected this ObjectiveSet before it ever reached eval_objective_set"
    );
    let mut acc = 0.0_f64;
    for term in &objective.terms {
        let v = reify_expr::eval_expr(&term.expr, &ctx_with(values, functions, dispatch))
            .as_f64()
            .filter(|v| v.is_finite())?;
        match term.sense {
            ObjectiveSense::Minimize => acc += term.weight * v,
            ObjectiveSense::Maximize => acc -= term.weight * v,
        }
    }
    Some(acc)
}

impl CostFunction for ConstraintCostFunction<'_> {
    type Param = Vec<f64>;
    type Output = f64;

    fn cost(&self, param: &Self::Param) -> Result<Self::Output, ArgminError> {
        // Clamp parameters to effective bounds and accumulate bound penalty
        let mut bound_penalty = 0.0;
        let mut clamped = Vec::with_capacity(param.len());
        for (i, &val) in param.iter().enumerate() {
            let (lo, hi) = self.bounds[i];
            let cv = val.clamp(lo, hi);
            bound_penalty += (val - cv).powi(2);
            clamped.push(cv);
        }

        let values = build_trial_values(
            self.base_values,
            self.auto_params,
            &clamped,
            self.dependent_cells,
            self.functions,
            self.dispatch,
        );
        let violation =
            compute_total_violation(self.constraints, &values, self.functions, self.dispatch);

        let cost = match self.objective {
            Some(obj) => {
                // Combine objective with penalty for constraint violations and bounds
                let obj_value = eval_objective_set(obj, &values, self.functions, self.dispatch)
                    .unwrap_or(UNDEF_OBJECTIVE_PENALTY);
                obj_value + PENALTY_WEIGHT * violation + PENALTY_WEIGHT * bound_penalty
            }
            None => {
                // Pure feasibility: minimize violations + bound penalty
                violation + PENALTY_WEIGHT * bound_penalty
            }
        };

        Ok(cost)
    }
}

/// Build the initial simplex for N-dimensional Nelder-Mead.
///
/// Creates N+1 vertices: the initial point plus N perturbations
/// (one per dimension), each offset by a fraction of the parameter range.
///
/// `bounds` is the caller's resolved box (one entry per dimension). Task #5618
/// changed this from `effective_bounds(&params[i])`: for a dimensionless Real that
/// box is `(-1e6, 1e6)`, so the step was `2e5` — Nelder-Mead's first reflection
/// left the feasible region entirely. A constraint-derived box gives a
/// box-proportional step instead.
fn build_simplex(initial: &[f64], bounds: &[(f64, f64)]) -> Vec<Vec<f64>> {
    let n = initial.len();
    let mut simplex = Vec::with_capacity(n + 1);
    simplex.push(initial.to_vec());

    for i in 0..n {
        let mut vertex = initial.to_vec();
        // Perturb dimension i by a fraction of the resolved range
        let (lo, hi) = bounds[i];
        let delta = (hi - lo) * 0.1;
        vertex[i] += delta;
        vertex[i] = vertex[i].clamp(lo, hi);
        simplex.push(vertex);
    }

    simplex
}

/// Get default bounds based on dimension type when AutoParam.bounds is None.
fn default_bounds_for(ty: &Type) -> (f64, f64) {
    let dim = dimension_of(ty);
    if dim == DimensionVector::LENGTH {
        (1e-6, 10.0) // 1 micron to 10 meters
    } else if dim == DimensionVector::ANGLE {
        (-std::f64::consts::TAU, std::f64::consts::TAU) // -2π to 2π
    } else {
        (-1e6, 1e6) // dimensionless or other
    }
}

/// Get effective bounds for an AutoParam, falling back to dimension-based defaults.
fn effective_bounds(param: &AutoParam) -> (f64, f64) {
    param
        .bounds
        .unwrap_or_else(|| default_bounds_for(&param.param_type))
}

/// Deterministic multistart seed generator for best-of-K multistart
/// (PRD `docs/prds/v0_6/whole-model-objective-coupling.md` §5.3, §11 Q4, task δ).
///
/// Produces exactly `K = 2 * (dim + 1)` start vectors for a `dim`-dimensional
/// problem (`dim = problem.auto_params.len()`):
///   - start #0: the historical [`extract_initial_point`] seed. Anchoring the
///     incumbent as one of the K starts guarantees best-of-K is a superset of
///     today's single start, so `candidate[0]` can never be worse than
///     `solve()`'s result (dominance).
///   - start #1: the all-midpoint point — every axis at its [`resolve_bounds`]
///     midpoint.
///   - starts #2..K-1: per axis `i` (in `auto_params` order), one vector with
///     axis `i` at its low [`resolve_bounds`] bound and one with axis `i` at its
///     high bound, every other axis held at its own midpoint.
///
/// Task #5618 changed starts #1..K-1 from [`effective_bounds`] to the
/// CONSTRAINT-DERIVED box. `AutoParam.bounds` is always `None` in production, so
/// `effective_bounds` degraded to `default_bounds_for` — for a dimensionless Real
/// that is `(-1e6, 1e6)`, and every corner anchor landed ~10⁶ away from a bracket
/// like `q ∈ [1, 100]`. Only start #0 (which [`extract_initial_point`] already
/// derives) could reach the feasible region, so best-of-K silently degenerated to
/// best-of-one. `include_strict = true`: these are SEED points, and a start point
/// may sit anywhere — unlike a clamp target (see [`resolve_bounds`]).
///
/// The derived box comes from `problem.constraints`, i.e. WITHOUT the synthesised
/// robustness floor, which does not exist yet at seed time — `solve_core` re-derives
/// its own clamp box from `effective_constraints` once the floor is added.
///
/// K is unchanged at `2 * (dim + 1)`, as are start #0 and the dominance contract.
///
/// Pure function of `problem` — no RNG, clock, or seed (§3.2 determinism
/// contract; BT5). Two calls on the same `problem` return identical vectors.
fn multistart_points(
    problem: &ResolutionProblem,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> Vec<Vec<f64>> {
    let dim = problem.auto_params.len();
    let mut points = Vec::with_capacity(2 * (dim + 1));

    // Start #0: the historical single-start seed (dominance anchor).
    points.push(extract_initial_point(problem, dispatch));

    // Constraint-derived seed box, one entry per auto param (task #5618).
    // #5711: `verify_uniqueness` builds the same box through this function's
    // shared `seed_box_from_intervals` half, so the composition cannot
    // silently diverge between the two call sites.
    let bounds = derived_seed_box(problem, dispatch);

    // Per-axis midpoint — shared by the all-midpoint point and as the
    // "other axes" value for every corner anchor below.
    let midpoint: Vec<f64> = bounds.iter().map(|&(lo, hi)| (lo + hi) / 2.0).collect();
    points.push(midpoint.clone());

    // Per-axis low/high corner anchors, every other axis held at midpoint.
    for (i, &(lo, hi)) in bounds.iter().enumerate() {
        let mut low = midpoint.clone();
        low[i] = lo;
        points.push(low);

        let mut high = midpoint.clone();
        high[i] = hi;
        points.push(high);
    }

    debug_assert_eq!(
        points.len(),
        2 * (dim + 1),
        "multistart_points must produce exactly K = 2*(dim+1) starts"
    );
    points
}

/// Relative tolerance for uniqueness comparison between two solutions.
const UNIQUENESS_REL_TOL: f64 = 1e-6;

/// Absolute tolerance for uniqueness comparison between two solutions.
const UNIQUENESS_ABS_TOL: f64 = 1e-10;

/// Core solve logic: runs Nelder-Mead from a given initial point using
/// `NM_SD_TOLERANCE` for the simplex termination criterion.
///
/// Returns `SolveResult` with `unique: true` as placeholder — the caller
/// (`DimensionalSolver::solve`) is responsible for setting the correct
/// uniqueness flag based on free/strict auto param classification.
///
/// Both the **main solve** and the **uniqueness re-solve** (`verify_uniqueness`)
/// use this function at the same tight tolerance (`NM_SD_TOLERANCE = 1e-30`).
/// This is correct after task #4710's eval-layer fix: connector-internal autos
/// are no longer injected as unconstrained strict autos into the parent
/// resolution problem, so the tight uniqueness tolerance correctly flags any
/// genuinely unconstrained strict auto as `ConstraintNonUnique` rather than
/// masking it.
///
/// **History (task #4700 → #4710):** task #4700 introduced a separate
/// `solve_core_with_sd_tolerance` wrapper and `UNIQUENESS_SD_TOLERANCE = 1e-15`
/// to work around spurious `ConstraintNonUnique` on `AllFourSites.__connector_0.gain`
/// (esc-4700-34). Task #4710 fixes that at the eval layer
/// (`engine_eval::connector_pin_if_determined`) so the solver-side heuristic
/// is no longer needed; this revert restores the single-tolerance regime.
fn solve_core_with_sd_tolerance(
    problem: &ResolutionProblem,
    initial: &[f64],
    sd_tolerance: f64,
    apply_robustness_floor: bool,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> (SolveResult, SolveMeta) {
    // ── Robustness floor (task #4789 α) ──────────────────────────────────────
    // When the objective is Money-dimensioned, synthesise per-inequality margin
    // constraints (slack_i ≥ m_i) so the solve parks auto values OFF the
    // constraint boundary instead of on it.  `floor_applied` tracks whether any
    // floor constraint was added; used below to emit the distinct diagnostic.
    //
    // ORDERING INVARIANT (load-bearing): `effective_constraints` MUST be built
    // BEFORE the `initially_feasible` check.  A floor-infeasible box that is
    // feasible without the floor must be seen as infeasible at the initial-point
    // check, so the initially_feasible fallback (L965 in original; below) does
    // NOT mask the infeasibility by falling back to Solved.
    //
    // Gate on `problem.objective` money-ness AND `apply_robustness_floor` (task γ
    // #4791: the cost_robustness_tradeoff two-anchor blend passes `false` for all
    // of its floor-free sub-solves — the tradeoff form REPLACES the floor rather
    // than composing with it, PRD §2.4/§8.1).  NOT the synthetic centrality
    // objective, which is built later and is never Money.  When Money AND
    // apply_robustness_floor:
    //   effective_constraints = problem.constraints ++ floor_constraints
    // Otherwise:
    //   effective_constraints = problem.constraints (bit-identical clone)
    // → invariant (ii): non-Money solve (and any floor-free solve) is completely
    // unchanged.
    // Build the initial-point value map ONCE — used for (1) the floor margin
    // computation below, (2) the initial-feasibility check, and (3) the
    // fallback objective validation when the optimizer drifts infeasible.
    // Building it here before the floor block eliminates the redundant second
    // call that was previously inside the floor synthesis branch.
    let trial_values = build_trial_values(
        &problem.current_values,
        &problem.auto_params,
        initial,
        &problem.dependent_cells,
        &problem.functions,
        dispatch,
    );

    let mut effective_constraints: Vec<(ConstraintNodeId, CompiledExpr)> =
        problem.constraints.clone();
    let floor_applied = if apply_robustness_floor && let Some(obj) = &problem.objective {
        if objective_is_money(obj) {
            synthesise_floor_constraints(
                &problem.constraints,
                &trial_values, // reuse — no redundant build_trial_values call
                &problem.functions,
                &mut effective_constraints,
                dispatch,
            )
        } else {
            false
        }
    } else {
        false
    };
    // ─────────────────────────────────────────────────────────────────────────

    // Constraint-derived CLAMP box (task #5618).  Derived from
    // `effective_constraints` — i.e. INCLUDING the synthesised robustness floor —
    // so the box the optimiser is clamped into is the FLOORED window, not just the
    // raw constraint box.  That distinction is load-bearing, not cosmetic: a
    // penalty method minimising `q + PENALTY_WEIGHT·(floor − q)²` places its
    // unconstrained minimiser ~5e-7 BELOW the floor and can never satisfy it to
    // `FEASIBILITY_THRESHOLD = 1e-12`; only clamping to the floored bound snaps
    // that undershoot onto the feasible optimum.  Deriving from the RAW box
    // instead yields a feasible-but-badly-suboptimal answer (the seed, returned
    // via the drift fallback).
    //
    // `include_strict = false`: a clamp target is a value the solver will actually
    // return, so a `Gt`-sourced bound must never become one.  The floor's slack
    // constraints are `Ge`, and the floored bound is strictly interior to the
    // original `>`/`>=` bound by construction, so the clamp still receives it.
    //
    // GATED on `floor_applied` (esc-5618-1); plan.json originally specified an
    // UNCONDITIONAL clamp.  Rationale (untouched by #5711 — this task does not
    // revisit it): the floored box is the solver's OWN synthesised construct
    // with known margin semantics, so confining the optimiser to it is safe.  The
    // RAW user constraint box is not — clamping to it unconditionally would promote
    // every user inequality into a hard wall the optimiser can never cross, i.e.
    // change what a constraint MEANS (something evaluated and reportable, not
    // assumed) for every non-Money solve in the workspace.  Per floor invariant (ii)
    // above, a floor-free solve stays bit-identical.
    //
    // RE-MEASURED (task #5711 step-7, unconditional form, applied to a scratch
    // tree and reverted, `cargo test -p reify-constraints --lib`): EXACTLY ONE
    // failure — `tests::undefined_objective_at_fallback_triggers_no_progress`.
    // `tests::defined_objective_at_fallback_returns_solved` NO LONGER fails under
    // the unconditional form: its earlier failure was the `ConstraintNonUnique` /
    // flat-objective mechanism documented on `verify_uniqueness`, and the
    // uniqueness half of this blocker is now RESOLVED by #5711 — step-5
    // dispositioned this exact fixture with `free: true` since its objective is
    // genuinely flat over the whole feasible region (see `verify_uniqueness`'s
    // doc, § Per-fixture measurement).  The surviving failure is unrelated to
    // uniqueness and stands on its own: a non-Money drift-fallback fixture that
    // deliberately lures the optimiser OUT of the feasible region (constraint
    // `x <= 0.020`, Undef-objective boundary `x <= 0.022`) to exercise the
    // fallback path.  A derived clamp box makes that drift unreachable BY
    // CONSTRUCTION for a single linear one-auto bound, so switching to the
    // unconditional form cannot keep that coverage without contriving a
    // nonlinear/multi-auto replacement fixture — testing a workaround, not the
    // fallback behaviour this fixture exists to cover.  That, together with the
    // semantics rationale above, is the standing reason the gate stays; neither
    // ground depends on the other, and #5711 leaves no open coupling between them.
    let bounds = if floor_applied {
        resolve_bounds(
            &problem.auto_params,
            &derive_param_intervals(
                &problem.auto_params,
                &effective_constraints,
                &trial_values,
                &problem.functions,
                dispatch,
            ),
            false,
        )
    } else {
        problem.auto_params.iter().map(effective_bounds).collect()
    };

    // `trial_values` is used in two places — (1) the feasibility check
    // immediately below, and (2) the fallback objective validation when the
    // optimizer drifts infeasible (see `eval_objective_set(&trial_values, …)`).
    // Do not inline into the feasibility check.
    let initially_feasible = max_constraint_residual(
        &effective_constraints,
        &trial_values,
        &problem.functions,
        dispatch,
    ) <= FEASIBILITY_THRESHOLD;

    // Synthesise a default centrality (Chebyshev-centre) objective when the scope has
    // inequality constraints but no explicit user objective (PRD η).  The synthetic
    // objective is built once and threaded through the cost function exactly like a
    // user-supplied objective; no new cost branch is added.
    //
    // `synth` lives for the rest of the function so the borrow in `effective_objective`
    // remains valid.  Discrete-type guard (Type::Scalar check) is added in step-4.
    //
    // Note: we pass `&effective_constraints` here.  When `problem.objective.is_none()`
    // the floor is never synthesised (floor_applied=false → effective_constraints ==
    // problem.constraints), so this is correct and consistent.
    let synth: Option<ObjectiveSet> = if problem.objective.is_none() {
        build_centrality_objective(&problem.auto_params, &effective_constraints)
    } else {
        None
    };

    // Effective objective: explicit (if any), else synthetic (if any), else None.
    // This is a borrow — `synth` and `problem` both outlive the function body.
    let effective_objective: Option<&ObjectiveSet> = problem.objective.as_ref().or(synth.as_ref());

    // Pure feasibility + already feasible → return immediately.
    // Gate on the EFFECTIVE objective so a centrality scope optimises instead of
    // short-circuiting to the first feasible boundary point.
    if initially_feasible && effective_objective.is_none() {
        let n_params = problem.auto_params.len();
        tracing::debug!(
            n_params,
            "initial point already feasible with no objective; returning early"
        );
        return (
            SolveResult::Solved {
                values: build_solved_values(&problem.auto_params, initial),
                unique: true,
            },
            SolveMeta::default(),
        );
    }

    // Choose iteration budget: scaled by simplex size when warm-starting.
    // Nelder-Mead needs O(N+1) evaluations per simplex sweep, so scale
    // the budget proportionally to give higher-dimensional problems enough
    // iterations to converge.
    // After the early-return above for `initially_feasible && effective_objective.is_none()`,
    // reaching here with `initially_feasible=true` implies `effective_objective.is_some()`.
    let max_iters = if initially_feasible {
        debug_assert!(
            effective_objective.is_some(),
            "warm-start budget path reached without objective — early-return invariant violated"
        );
        let n_params = problem.auto_params.len() as u64;
        (FEASIBLE_OPT_ITERS_PER_DIM * (n_params + 1)).min(MAX_ITERS)
    } else {
        MAX_ITERS
    };

    let cost_fn = ConstraintCostFunction {
        auto_params: &problem.auto_params,
        constraints: &effective_constraints,
        base_values: &problem.current_values,
        objective: effective_objective,
        functions: &problem.functions,
        bounds: &bounds,
        dependent_cells: &problem.dependent_cells,
        dispatch,
    };

    // Build simplex from the provided initial point
    let simplex = build_simplex(initial, &bounds);

    // Configure and run Nelder-Mead
    let solver: NelderMead<Vec<f64>, f64> = NelderMead::new(simplex)
        .with_sd_tolerance(sd_tolerance)
        .expect("sd_tolerance is a positive finite f64 (callers pass NM_SD_TOLERANCE)");

    let executor = Executor::new(cost_fn, solver).configure(|state| state.max_iters(max_iters));

    let result = match executor.run() {
        Ok(res) => res,
        Err(e) => {
            let n_params = problem.auto_params.len();
            tracing::warn!(error = %e, n_params, "solver executor failed");
            return (
                SolveResult::NoProgress {
                    reason: format!("solver error: {}", e),
                },
                SolveMeta::default(),
            );
        }
    };

    // Extract and log convergence information from the solver result.
    let termination_reason = result.state().get_termination_reason().cloned();
    let has_objective = effective_objective.is_some();
    let n_params = problem.auto_params.len();
    let iter_limited =
        termination_reason == Some(TerminationReason::MaxItersReached) && has_objective;
    let meta = SolveMeta { iter_limited };
    if iter_limited {
        tracing::debug!(
            ?termination_reason,
            n_params,
            max_iters,
            has_objective,
            initially_feasible,
            iter_limited,
            "solver completed; hit iteration limit — objective may be suboptimal"
        );
    } else {
        tracing::debug!(
            ?termination_reason,
            n_params,
            max_iters,
            has_objective,
            initially_feasible,
            iter_limited,
            "solver completed"
        );
    }

    let best_param: Vec<f64> = match result.state().get_best_param() {
        Some(p) => p.clone(),
        None => {
            let n_params = problem.auto_params.len();
            tracing::warn!(n_params, "solver returned no best parameter");
            return (
                SolveResult::NoProgress {
                    reason: "solver returned no solution".to_string(),
                },
                meta,
            );
        }
    };

    // Clamp final solution into the resolved box (task #5618: the floored
    // constraint-derived box, not `effective_bounds`).
    let clamped: Vec<f64> = best_param
        .iter()
        .enumerate()
        .map(|(i, val)| {
            let (lo, hi) = bounds[i];
            val.clamp(lo, hi)
        })
        .collect();

    // Check feasibility by re-evaluating constraint violations
    // (best_cost may include the objective term, so we check violations separately)
    let final_values = build_trial_values(
        &problem.current_values,
        &problem.auto_params,
        &clamped,
        &problem.dependent_cells,
        &problem.functions,
        dispatch,
    );
    let final_max_residual = max_constraint_residual(
        &effective_constraints,
        &final_values,
        &problem.functions,
        dispatch,
    );
    if final_max_residual > FEASIBILITY_THRESHOLD {
        // If the initial point was feasible but the optimizer drifted infeasible
        // while chasing an objective, fall back to the initial feasible values
        // rather than reporting a false Infeasible.
        if initially_feasible {
            // Validate that the objective is numeric at the initial point
            // before promoting to Solved. The trial_values ValueMap was built
            // from the same initial point and is still in scope.
            if let Some(obj) = effective_objective
                && eval_objective_set(obj, &trial_values, &problem.functions, dispatch).is_none()
            {
                return (
                    SolveResult::NoProgress {
                        reason: "objective expression evaluated to undefined at fallback point"
                            .to_string(),
                    },
                    meta,
                );
            }
            // Construct fallback HashMap lazily — only on the error path
            // where the optimizer drifted infeasible. The `initial` slice
            // is still in scope from the parameter.
            let fallback = build_solved_values(&problem.auto_params, initial);
            tracing::debug!(
                n_params,
                final_max_residual,
                "optimizer drifted infeasible while chasing objective; \
                 falling back to initial feasible point"
            );
            return (
                SolveResult::Solved {
                    values: fallback,
                    unique: true,
                },
                meta,
            );
        }
        return (
            SolveResult::Infeasible {
                diagnostics: vec![if floor_applied {
                    // ── Diagnostic honesty (task #5618 step-10) ──────────────────
                    // `final_max_residual` above is measured against
                    // `effective_constraints`, i.e. the user's constraints PLUS the
                    // synthesised floor.  It cannot tell "your constraints admit no
                    // solution" apart from "your constraints do, but my 2% margin
                    // does not fit inside them" — and the message used to assert the
                    // former in both cases.  For a tight-but-satisfiable bracket that
                    // is simply false, and it was the original report's sharpest
                    // complaint: the diagnostic sent the user off relaxing a design
                    // that was never over-constrained.
                    //
                    // So re-measure against `problem.constraints` — the ORIGINAL set,
                    // floor excluded — at the point actually being reported.  This is
                    // deliberately a claim about THAT POINT, not about the feasible
                    // region: it is verified, not inferred, so the new wording can
                    // never over-claim.  Deriving the raw box instead (via
                    // `derive_param_intervals`) would be cheaper but unsound —
                    // that helper SKIPs nonlinear and multi-auto shapes, so a
                    // non-degenerate derived box is not evidence of satisfiability.
                    //
                    // KNOWN GAP (not a regression; #5618 does not close it): the
                    // honest branch is only reachable when the returned point stays
                    // inside the user's box.  Under a steep objective it need not —
                    // measured, `x > 10mm ∧ x < 10.3mm` with `5 USD × (x / 1mm)`
                    // (gradient 5000/m vs PENALTY_WEIGHT = 1e6) parks ~1.25e-3 m below
                    // the floored lower bound, outside the 0.3mm-wide user box, so
                    // this check correctly declines and the region-empty wording
                    // stands even though the region is not empty.  Making it reachable
                    // means changing WHICH point a floor-infeasible solve reports —
                    // solver semantics, and its own task: #5714, which carries this
                    // measurement and the reason the cheap box-emptiness shortcut is
                    // unsound.  See the
                    // `margin_only_infeasibility_names_the_margin_not_an_empty_region`
                    // doc comment in `tests/robustness_floor.rs` for the measurement.
                    let original_max_residual = max_constraint_residual(
                        &problem.constraints,
                        &final_values,
                        &problem.functions,
                        dispatch,
                    );
                    if original_max_residual <= FEASIBILITY_THRESHOLD {
                        // The floor terms occupy the tail of `effective_constraints`
                        // past the originals — the ORDERING INVARIANT documented at
                        // the `synthesise_floor_constraints` call site is what makes
                        // this slice well-defined.
                        let shortfall = match worst_unmet_floor_term(
                            &effective_constraints[problem.constraints.len()..],
                            &final_values,
                            &problem.functions,
                            dispatch,
                        ) {
                            Some((achieved, required)) => format!(
                                " (worst slack at that point: {achieved:.3e} achieved vs \
                                 {required:.3e} required)"
                            ),
                            None => String::new(),
                        };
                        reify_core::Diagnostic::error(format!(
                            "infeasible under robustness floor: the original constraints ARE \
                             satisfied at the returned point — it is the synthesised {:.0}% \
                             robustness margin that cannot be met{}; relax opposing \
                             constraints, widen the tolerance margin, or take explicit \
                             control with `minimize cost_robustness_tradeoff(<cost-expr>, λ)`",
                            REL_MARGIN * 100.0,
                            shortfall
                        ))
                        .with_code(DiagnosticCode::RobustnessFloorInfeasible)
                    } else {
                        reify_core::Diagnostic::error(format!(
                            "infeasible under robustness floor: the floored feasible region is \
                             empty (max absolute residual: {:.2e}); relax opposing constraints \
                             or widen the tolerance margin",
                            final_max_residual
                        ))
                        .with_code(DiagnosticCode::RobustnessFloorInfeasible)
                    }
                } else {
                    reify_core::Diagnostic::error(format!(
                        "constraints could not be satisfied (max absolute residual: {:.2e})",
                        final_max_residual
                    ))
                    .with_code(DiagnosticCode::ConstraintUnsatisfiable)
                }],
            },
            meta,
        );
    }

    // Post-solve objective validation: if the objective is still non-numeric
    // at the solution point, report NoProgress rather than Solved.
    if let Some(obj) = effective_objective
        && eval_objective_set(obj, &final_values, &problem.functions, dispatch).is_none()
    {
        return (
            SolveResult::NoProgress {
                reason: "objective expression evaluated to undefined at solution point".to_string(),
            },
            meta,
        );
    }

    // Build solution values
    let values = build_solved_values(&problem.auto_params, &clamped);

    // NOTE: Solved indicates constraint satisfaction but does NOT guarantee objective
    // optimality. The Nelder-Mead optimizer may have hit the iteration limit without
    // full convergence. Convergence quality is logged via tracing::debug! (see above)
    // including TerminationReason, iteration budget, and whether fallback was used.
    // `iter_limited` is now threaded out via `SolveMeta` so `solve_ranked` can surface
    // it as the `BestFound` reason without a breaking change to `SolveResult`.
    (
        SolveResult::Solved {
            values,
            unique: true,
        },
        meta,
    )
}

/// Core solve at the default (main-solve) convergence regime.
///
/// Thin wrapper over [`solve_core_with_sd_tolerance`] passing `NM_SD_TOLERANCE`
/// (1e-30). This is the entry point for the **main** resolution solve, where a
/// strict auto must converge to `FEASIBILITY_THRESHOLD` even from a moved seed
/// (task #4700). As of task #4710 the uniqueness re-solve also routes through
/// here at the same tight tolerance (the prior `UNIQUENESS_SD_TOLERANCE = 1e-15`
/// decoupling was reverted once connector-internal autos were pinned at the
/// eval layer — see [`verify_uniqueness`]).
///
/// Always applies the α robustness floor (`apply_robustness_floor = true`) —
/// the default, unchanged-behaviour path. The γ cost_robustness_tradeoff blend
/// (task #4791) bypasses this wrapper and calls [`solve_core_with_sd_tolerance`]
/// directly with `false` for its floor-free anchor/final sub-solves.
fn solve_core(
    problem: &ResolutionProblem,
    initial: &[f64],
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> (SolveResult, SolveMeta) {
    solve_core_with_sd_tolerance(problem, initial, NM_SD_TOLERANCE, true, dispatch)
}

/// At or below this, an anchor pair's range on a given blend axis (cost or
/// robustness) is treated as degenerate and the corresponding normalised term
/// contributes exactly `0.0` rather than dividing by ~0 (PRD §8.1 guards this
/// explicitly; a divide-by-near-zero would otherwise inject a huge or NaN
/// gradient into the Nelder-Mead cost function). Analytically `range` (built
/// from `cost_max − cost_min` / `rob_max − rob_min`, each anchor-optimal by
/// construction) is always ≥ 0, but both anchors are approximate Nelder-Mead
/// solves, so numerical noise can land it marginally negative even when both
/// anchors coincide. Guarding on the *signed* value (not just magnitude) below
/// treats that case as degenerate too, so solver noise can never flip the sign
/// of a normalised blend term via division by a small negative divisor.
const TRADEOFF_NORMALISATION_RANGE_EPS: f64 = 1e-12;

/// Builds `weight × (expr − min) / range` as a `Dimensionless` `CompiledExpr`.
///
/// `dimension` is used for the `min`/`range` literals so the division cancels
/// `expr`'s own dimension (Money for the cost axis; the min-slack's dimension —
/// typically Length — for the robustness axis) to `Dimensionless`, letting the
/// two axes be summed directly regardless of their physical units.
///
/// Returns a `Dimensionless` `0.0` literal (ignoring `expr`/`weight`) when
/// `range` is at or below [`TRADEOFF_NORMALISATION_RANGE_EPS`] — including a
/// slightly *negative* range from anchor-solve noise, which would otherwise
/// silently invert this axis of the blend (a plain `.abs() < EPS` magnitude
/// guard would let a small negative range through to the division below).
fn normalised_blend_term(
    expr: CompiledExpr,
    min: f64,
    range: f64,
    dimension: DimensionVector,
    weight: f64,
) -> CompiledExpr {
    let dimensionless = DimensionVector::DIMENSIONLESS;
    if range <= TRADEOFF_NORMALISATION_RANGE_EPS {
        return CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.0,
                dimension: dimensionless,
            },
            Type::Scalar {
                dimension: dimensionless,
            },
        );
    }
    let min_lit = CompiledExpr::literal(
        Value::Scalar {
            si_value: min,
            dimension,
        },
        Type::Scalar { dimension },
    );
    let range_lit = CompiledExpr::literal(
        Value::Scalar {
            si_value: range,
            dimension,
        },
        Type::Scalar { dimension },
    );
    let diff = CompiledExpr::binop(BinOp::Sub, expr, min_lit, Type::Scalar { dimension });
    let normalised = CompiledExpr::binop(
        BinOp::Div,
        diff,
        range_lit,
        Type::Scalar {
            dimension: dimensionless,
        },
    );
    let weight_lit = CompiledExpr::literal(
        Value::Scalar {
            si_value: weight,
            dimension: dimensionless,
        },
        Type::Scalar {
            dimension: dimensionless,
        },
    );
    CompiledExpr::binop(
        BinOp::Mul,
        weight_lit,
        normalised,
        Type::Scalar {
            dimension: dimensionless,
        },
    )
}

/// Runs the `minimize cost_robustness_tradeoff(<money-expr>, λ)` normalised
/// two-anchor blend (task γ #4791, PRD §2.4/§8.1) in place of a plain solve —
/// dispatched from [`DimensionalSolver::solve_with_meta`] whenever
/// `problem.objective`'s `cost_robustness_lambda` marker is `Some(λ)`.
///
/// Two floor-free anchor solves establish the achievable range on both axes:
/// - the **cost anchor** (`Minimize(cost_expr)`) gives `cost_min` and the
///   robustness value AT that point (`rob_min`, i.e. `min_slack` evaluated at
///   the cost-optimal point);
/// - the **robustness anchor** ([`build_centrality_objective`]'s
///   `Maximize(min_slack)`) gives `rob_max` and the cost value at that point
///   (`cost_max`).
///
/// A single dimensionless blend expression
/// `λ·(cost−cost_min)/(cost_max−cost_min) − (1−λ)·(min_slack−rob_min)/(rob_max−rob_min)`
/// is then minimised — also floor-free — as the final solve. At λ=1 this is a
/// positive-affine transform of `cost` alone (identical argmin to the cost
/// anchor); at λ=0 it is a positive-affine transform of `min_slack` alone
/// (identical argmax to the robustness anchor) — both PRD §8.1 invariants by
/// construction, not numerical coincidence.
///
/// All solves share the SAME deterministic `initial` seed (no chaining
/// between anchors) so the whole dispatch stays reproducible.
///
/// Degenerate fallbacks (no panics): a non-`Solved` cost anchor propagates
/// directly; no centrality objective (no inequality slack, or a non-Scalar
/// auto param) or a non-`Solved` robustness anchor falls back to the cost
/// anchor's own (already-`Solved`) result — including its own `unique` flag,
/// not an unconditional `true` — since there is no robustness axis to blend
/// against.
fn solve_cost_robustness_tradeoff(
    problem: &ResolutionProblem,
    initial: &[f64],
    lambda: f64,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> (SolveResult, SolveMeta) {
    // `.expect` below asserts only non-emptiness — never violated, because
    // entity.rs's `MemberDecl::Minimize` arm sets `cost_robustness_lambda` in
    // the SAME branch where it pushes the cost term onto `objective_terms`,
    // so `terms` always holds at least that element whenever the marker is
    // `Some`. It does NOT assert `terms.len() == 1`: if another
    // minimize/maximize declaration shares the objective (e.g. `minimize a`
    // followed by `minimize cost_robustness_tradeoff(c, 0.5)`), entity.rs has
    // no dedicated diagnostic for the collision — `check_objective_conflict`
    // only fires for a Minimize/Maximize sense mismatch, not two Minimize
    // terms — so `.first()` here can silently pick the wrong (non-cost) term.
    // Accepted v1 scope: still degrades rather than panicking, since
    // `normalised_blend_term` below is dimension-agnostic.
    let cost_expr = problem
        .objective
        .as_ref()
        .and_then(|obj| obj.terms.first())
        .map(|term| term.expr.clone())
        .expect(
            "solve_cost_robustness_tradeoff is dispatched only when problem.objective is \
             Some(..) with cost_robustness_lambda set, and entity.rs always pushes the cost \
             term in the same branch that sets the marker — terms is therefore never empty \
             here",
        );
    let cost_dimension = dimension_of(&cost_expr.result_type);

    // ── Anchor 1: pure cost, floor-free ────────────────────────────────────
    let cost_problem = ResolutionProblem {
        objective: Some(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            cost_expr.clone(),
        )),
        ..problem.clone()
    };
    let (cost_result, cost_meta) =
        solve_core_with_sd_tolerance(&cost_problem, initial, NM_SD_TOLERANCE, false, dispatch);
    // `cost_unique` is carried into BOTH degenerate-fallback returns below
    // instead of hardcoding `true` — the cost anchor's own uniqueness
    // determination (real for a strict auto, `false` for `auto(free)`, see
    // `SolveResult::Solved::unique`) is the correct value to report when the
    // final blend solve never runs, not an unconditional claim of uniqueness.
    let (x_cost, cost_unique) = match cost_result {
        SolveResult::Solved { values, unique } => (values, unique),
        other => return (other, cost_meta),
    };

    // ── Anchor 2: pure robustness (Chebyshev centre), floor-free ───────────
    let Some(centrality_obj) =
        build_centrality_objective(&problem.auto_params, &problem.constraints)
    else {
        return (
            SolveResult::Solved {
                values: x_cost,
                unique: cost_unique,
            },
            cost_meta,
        );
    };
    let min_slack_expr = centrality_obj.terms[0].expr.clone();
    let rob_dimension = dimension_of(&min_slack_expr.result_type);

    let rob_problem = ResolutionProblem {
        objective: Some(centrality_obj),
        ..problem.clone()
    };
    let (rob_result, _rob_meta) =
        solve_core_with_sd_tolerance(&rob_problem, initial, NM_SD_TOLERANCE, false, dispatch);
    let x_rob = match rob_result {
        SolveResult::Solved { values, .. } => values,
        _ => {
            return (
                SolveResult::Solved {
                    values: x_cost,
                    unique: cost_unique,
                },
                cost_meta,
            );
        }
    };

    // ── Evaluate both axes at both anchors ──────────────────────────────────
    //
    // Both maps go through [`build_scoring_values`], NOT a hand-rolled
    // `current_values.clone()` + solved-autos overlay. `SolveResult::Solved`
    // carries only the AUTOS (`build_solved_values`), so an unfolded overlay
    // leaves every dependent cell at its stale base value. When the money
    // expression is a READ of a dependent cell — the joint-drive shape — that
    // makes `cost_expr` evaluate to the identical stale number at BOTH anchors,
    // so `cost_max - cost_min` collapses to 0 and `normalised_blend_term`'s
    // [`TRADEOFF_NORMALISATION_RANGE_EPS`] guard silently drops the cost axis
    // from the blend for EVERY λ. The anchor SOLVES already fold (they inherit
    // `dependent_cells` via `..problem.clone()`), so only this scoring step was
    // stale — the anchors moved, but the axes were measured at the wrong place.
    let values_at_cost = build_scoring_values(
        &problem.current_values,
        &x_cost,
        &problem.dependent_cells,
        &problem.functions,
        dispatch,
    );
    let values_at_rob = build_scoring_values(
        &problem.current_values,
        &x_rob,
        &problem.dependent_cells,
        &problem.functions,
        dispatch,
    );

    let ctx_cost = ctx_with(&values_at_cost, &problem.functions, dispatch);
    let ctx_rob = ctx_with(&values_at_rob, &problem.functions, dispatch);
    let axes = (
        reify_expr::eval_expr(&cost_expr, &ctx_cost).as_f64(),
        reify_expr::eval_expr(&min_slack_expr, &ctx_cost).as_f64(),
        reify_expr::eval_expr(&cost_expr, &ctx_rob).as_f64(),
        reify_expr::eval_expr(&min_slack_expr, &ctx_rob).as_f64(),
    );
    let (Some(cost_min), Some(rob_min), Some(cost_max), Some(rob_max)) = axes else {
        return (
            SolveResult::NoProgress {
                reason: "cost_robustness_tradeoff: cost or min-slack expression evaluated to \
                         undefined at an anchor solution"
                    .to_string(),
            },
            SolveMeta::default(),
        );
    };

    // ── Build and solve the normalised blend ────────────────────────────────
    let cost_term = normalised_blend_term(
        cost_expr,
        cost_min,
        cost_max - cost_min,
        cost_dimension,
        lambda,
    );
    let rob_term = normalised_blend_term(
        min_slack_expr,
        rob_min,
        rob_max - rob_min,
        rob_dimension,
        1.0 - lambda,
    );
    let blend = CompiledExpr::binop(
        BinOp::Sub,
        cost_term,
        rob_term,
        Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        },
    );

    let blend_problem = ResolutionProblem {
        objective: Some(ObjectiveSet::single(ObjectiveSense::Minimize, blend)),
        ..problem.clone()
    };
    solve_core_with_sd_tolerance(&blend_problem, initial, NM_SD_TOLERANCE, false, dispatch)
}

/// Returns `true` if `a` and `b` agree within the project's uniqueness
/// tolerance: relative to the larger magnitude (`UNIQUENESS_REL_TOL * scale`,
/// `scale = |a|.max(|b|).max(UNIQUENESS_ABS_TOL)`), OR within the absolute
/// floor `UNIQUENESS_ABS_TOL` — whichever is looser, matching `f64` comparison
/// best practice at both small and large magnitudes.
///
/// Extracted from [`solutions_agree`]'s inline predicate (task #5711) so
/// parameter comparison and objective-score comparison ([`classify_uniqueness`])
/// share ONE tolerance policy. That sharing is load-bearing, not cosmetic: it
/// gives the objective comparison the RELATIVE arm for free, which matters at
/// large magnitudes — e.g. a `1e8`-scale objective, where an absolute-only
/// tolerance would misclassify a genuine tie as a strict improvement.
fn within_uniqueness_tol(a: f64, b: f64) -> bool {
    let diff = (a - b).abs();
    let scale = a.abs().max(b.abs()).max(UNIQUENESS_ABS_TOL);
    !(diff > UNIQUENESS_REL_TOL * scale && diff > UNIQUENESS_ABS_TOL)
}

/// Compare two solution maps across the given auto params.
///
/// Returns `true` if every param value in `solved_values` and
/// `perturbed_values` matches within the project tolerance constants.
///
/// If either map is missing a param, contains a non-numeric value
/// (e.g. `Value::Undef`, `Value::Bool`), or contains a non-finite value
/// (NaN, Infinity), emits a `tracing::warn!` and returns `false` — the
/// caller treats false as non-unique → Infeasible, producing a noisy
/// user-facing error rather than silently masking the bug. Non-finite
/// values must be rejected because NaN comparisons always return false,
/// which would let the tolerance check silently report agreement.
fn solutions_agree(
    auto_params: &[AutoParam],
    solved_values: &HashMap<ValueCellId, Value>,
    perturbed_values: &HashMap<ValueCellId, Value>,
) -> bool {
    for param in auto_params {
        let s1 = match solved_values.get(&param.id).and_then(|v| v.as_f64()) {
            Some(v) if v.is_finite() => v,
            _ => {
                tracing::warn!(
                    param = %param.id,
                    "uniqueness check: original solution has missing, non-numeric, or \
                     non-finite (NaN/Inf) value; cannot verify uniqueness"
                );
                return false;
            }
        };
        let s2 = match perturbed_values.get(&param.id).and_then(|v| v.as_f64()) {
            Some(v) if v.is_finite() => v,
            _ => {
                tracing::warn!(
                    param = %param.id,
                    "uniqueness check: perturbed solution has missing, non-numeric, or \
                     non-finite (NaN/Inf) value; cannot verify uniqueness"
                );
                return false;
            }
        };
        if !within_uniqueness_tol(s1, s2) {
            let diff = (s1 - s2).abs();
            tracing::debug!(
                param = %param.id,
                s1,
                s2,
                diff,
                "uniqueness check failed: solutions differ"
            );
            return false;
        }
    }
    tracing::debug!("uniqueness check passed: perturbed solution matches");
    true
}

/// Verdict from comparing an incumbent solution against a perturbed re-solve,
/// per `docs/reify-implementation-architecture.md` §11.6's two disjunctive
/// well-determinedness tests for strict `auto` resolution: the resolved value
/// must be either uniquely determined by constraints, or uniquely optimal
/// under the applicable objective.
///
/// Wired into [`verify_uniqueness`] as of task #5711 step-5.
///
/// `PartialEq` only (no `Eq`): `IncumbentSuboptimal` carries `f64` evidence,
/// which is not `Eq`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum UniquenessVerdict {
    /// The incumbent and perturbed parameter values agree within tolerance —
    /// the first §11.6 test ("uniquely determined by constraints") is
    /// satisfied directly, so the objective is never consulted.
    Unique,
    /// The parameter values differ, and that difference is NOT explained away
    /// by an objective-optimality finding: either no effective objective
    /// exists (so only the first §11.6 test applies, and it fails), or the
    /// objective ties between the two points (a flat region), or the
    /// perturbed point scores no better than the incumbent. Both §11.6 tests
    /// fail, so this is a genuine non-uniqueness report.
    NonUnique,
    /// The parameter values differ, but the perturbed point scores STRICTLY
    /// better than the incumbent under the applicable objective (beyond
    /// tolerance): the incumbent was never the argmin, so this is an
    /// OPTIMALITY finding — e.g. a drift-fallback or budget-exhausted local
    /// search settling short — not a §11.6 non-uniqueness one. Callers
    /// suppress this verdict (report "cannot prove non-unique") rather than
    /// `ConstraintNonUnique`.
    ///
    /// Carries the two scores that justified the verdict (task #5711
    /// suggestion 8) rather than making callers re-derive them: the ONLY
    /// producer is this function's `Some(scores)` arm below, but nothing
    /// structurally prevented a caller from having to `.expect()` its way
    /// back to the evidence — a future edit adding an `IncumbentSuboptimal`
    /// return on some `None`-scores path would have turned that `.expect()`
    /// into a production panic.
    IncumbentSuboptimal { incumbent: f64, perturbed: f64 },
}

/// Classify solution uniqueness per §11.6's two disjunctive well-determinedness
/// tests. `objective_scores` is invoked LAZILY — at most once, and only when
/// the incumbent and perturbed PARAMETERS differ — and must return
/// `Some((incumbent, perturbed))` on the pure-minimiser scale used by
/// [`eval_objective_set`] (lower is better), or `None` when no effective
/// objective exists or either score is incomparable (`eval_objective_set`
/// already collapses `Undef`/non-finite to `None`).
///
/// The laziness is load-bearing, not an optimisation nicety (task #5711
/// review suggestion 7): it makes "the objective is never consulted when
/// params agree" a STRUCTURAL guarantee rather than a merely documented one
/// — a caller building `objective_scores` from a re-solve's value maps can
/// skip that work entirely on the common well-determined path, instead of
/// computing it only to have this function discard it.
/// `classify_uniqueness_params_agree_with_score_returns_unique` enforces
/// this directly: its closure panics if invoked.
///
/// Deliberately MONOTONE relative to pre-#5711 behaviour: the only new branch
/// is params-differ + perturbed-strictly-better → `IncumbentSuboptimal`. Every
/// other input shape keeps the verdict the old boolean check would have
/// produced (folding `Unique` to `true` and `NonUnique` to `false`).
/// Monotonicity is what bounds this change's blast radius: it can only turn a
/// `false` (non-unique) verdict into `true`, never the reverse, so no
/// currently-passing `ConstraintNonUnique` expectation can flip to `Solved` as
/// a side effect of this classifier alone. In particular, a perturbed point
/// that scores strictly WORSE than the incumbent is deliberately left at
/// `NonUnique` even though that comparison is logically inconclusive on its
/// own (the re-solve may simply have stalled at a worse local point) —
/// abstaining there would silently convert
/// `strict_auto_non_unique_returns_infeasible`'s synthetic-centrality-objective
/// fixture (`solver_integration.rs`) into a false `Solved`.
fn classify_uniqueness(
    auto_params: &[AutoParam],
    solved_values: &HashMap<ValueCellId, Value>,
    perturbed_values: &HashMap<ValueCellId, Value>,
    objective_scores: impl FnOnce() -> Option<(f64, f64)>,
) -> UniquenessVerdict {
    if solutions_agree(auto_params, solved_values, perturbed_values) {
        return UniquenessVerdict::Unique;
    }
    match objective_scores() {
        Some((incumbent, perturbed))
            if !within_uniqueness_tol(incumbent, perturbed) && perturbed < incumbent =>
        {
            UniquenessVerdict::IncumbentSuboptimal { incumbent, perturbed }
        }
        _ => UniquenessVerdict::NonUnique,
    }
}

/// Build the perturbed initial point for uniqueness verification.
///
/// For each auto parameter, computes the perturbed starting value by reflecting
/// to the opposite end of its `bounds[i]` range from the current solution.
/// If a solved value is missing or non-numeric (`as_f64()` returns `None`), the
/// midpoint is used as a fallback and the parameter ID is added to the returned
/// missing list.
///
/// `bounds` is the CALLER's resolved box, one entry per auto param, and must be at
/// least `auto_params.len()` long. Task #5618 changed this from
/// `effective_bounds(param)`: reflection lands at `lo + 0.9·(hi − lo)`, so on the
/// dimensionless default box `(-1e6, 1e6)` a strict auto bracketed to `q ∈ [1, 100]`
/// re-solved from ~±8×10⁵ and could not reconverge — reporting
/// `ConstraintNonUnique` for a problem that has exactly one solution.
///
/// Returns `(perturbed_anchors, missing_param_ids)`.
fn build_perturbation_anchors(
    auto_params: &[reify_ir::AutoParam],
    solved_values: &HashMap<ValueCellId, Value>,
    bounds: &[(f64, f64)],
) -> (Vec<f64>, Vec<String>) {
    let mut missing: Vec<String> = Vec::new();
    let perturbed: Vec<f64> = auto_params
        .iter()
        .enumerate()
        .map(|(i, param)| {
            let (lo, hi) = bounds[i];
            let mid = (lo + hi) / 2.0;
            let solution_val = solved_values
                .get(&param.id)
                .and_then(|v| v.as_f64())
                .unwrap_or_else(|| {
                    missing.push(param.id.to_string());
                    mid
                });
            if solution_val < mid {
                // Solution is in the lower half — start near the high end
                lo + 0.9 * (hi - lo)
            } else {
                // Solution is in the upper half — start near the low end
                lo + 0.1 * (hi - lo)
            }
        })
        .collect();
    (perturbed, missing)
}

/// Score a solved value map against `problem`'s objective, on
/// [`eval_objective_set`]'s pure-minimiser scale (lower is better).
///
/// Keys off `problem.objective` — the USER-authored objective — and NEVER
/// `effective_objective`'s synthesised fallback, per I3/I4: a
/// feasibility-only solve must report `FeasibilityOnly` + `None` even when
/// the solver internally optimised a synthetic centrality objective for
/// exploration. Returns `None` when there is no explicit objective, or when
/// [`eval_objective_set`] cannot produce a comparable score at `values`
/// (a non-numeric or non-finite result).
///
/// Shared by [`rank_single`], the multistart scoring loop in
/// [`ConstraintSolver::solve_ranked`], and `verify_uniqueness`'s
/// objective-optimality check (task #5711): these were three verbatim
/// copies of the same `build_scoring_values` + `eval_objective_set` pair,
/// each restating the "explicit objective only" rule; extracting keeps that
/// rule in exactly one place.
fn score_solution(
    problem: &ResolutionProblem,
    values: &HashMap<ValueCellId, Value>,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> Option<f64> {
    let obj = problem.objective.as_ref()?;
    let full = build_scoring_values(
        &problem.current_values,
        values,
        &problem.dependent_cells,
        &problem.functions,
        dispatch,
    );
    eval_objective_set(obj, &full, &problem.functions, dispatch)
}

/// Verify solution uniqueness by re-solving from a perturbed starting point.
///
/// Creates a perturbed initial point from the #5618 constraint-derived seed
/// box (see the anchor-box comment in the body below), re-solves via
/// [`solve_core`], and classifies the incumbent/perturbed pair via
/// [`classify_uniqueness`].
///
/// Returns `true` if the solution is unique (or a suboptimality finding was
/// suppressed — see `IncumbentSuboptimal` below), `false` if a genuinely
/// different solution was found (the problem is underdetermined).
///
/// # The ruling (task #5711)
///
/// `docs/reify-implementation-architecture.md` §11.6 gives strict
/// `auto` resolution TWO disjunctive well-determinedness tests: the resolved
/// value must be either (1) uniquely determined by constraints, or (2)
/// uniquely optimal under the applicable objective. Before #5711 this
/// function implemented ONLY test (1) — comparing incumbent vs. perturbed
/// PARAMETER values — and applied it unconditionally, including to problems
/// governed by test (2). That single mismatch produced both of #5711's
/// motivating bugs: a latent false-negative (the old anchor, built from
/// unconstrained `effective_bounds`, landed far outside the feasible region
/// for an inequality-bracketed auto, the re-solve failed to converge, and
/// the `_ =>` arm below silently defaulted to "unique"), and a false-positive
/// risk once the anchor could reach the feasible region (a drift-fallback or
/// budget-exhausted incumbent that is merely SUBOPTIMAL — not non-unique —
/// would then misreport `ConstraintNonUnique`).
///
/// # The four-branch rule ([`classify_uniqueness`])
///
/// - params AGREE within tolerance → [`UniquenessVerdict::Unique`] → `true`.
///   Test (1) is satisfied directly; the objective is never consulted.
/// - params DIFFER, no comparable objective score → [`UniquenessVerdict::NonUnique`]
///   → `false`. No applicable objective exists (see explicit-only scoring
///   below), so only test (1) applies, and it failed.
/// - params DIFFER, objective TIES within tolerance, or the perturbed score
///   is NOT strictly better (this includes strictly WORSE) →
///   [`UniquenessVerdict::NonUnique`] → `false`. A tie is a flat region:
///   genuinely not uniquely optimal under test (2) either. A strictly-worse
///   perturbed point is logically inconclusive on its own — the re-solve may
///   simply have stalled at a worse local point — but it is deliberately
///   kept at today's verdict rather than made to abstain, because that is
///   what makes the whole rule MONOTONE relative to pre-#5711 behaviour: it
///   can only turn a `false` (non-unique) verdict into `true`, never the
///   reverse. Monotonicity is what bounds this change's blast radius — no
///   currently-passing `ConstraintNonUnique` expectation anywhere in the
///   workspace can flip to `Solved` as a side effect of the classifier
///   alone.
/// - params DIFFER, perturbed score STRICTLY BETTER beyond tolerance →
///   [`UniquenessVerdict::IncumbentSuboptimal`] → `true` (suppressed), with
///   a `tracing::warn!` naming both scores. The incumbent was never the
///   argmin, so this is an OPTIMALITY finding — not a §11.6 non-uniqueness
///   one.
///
/// Objective scoring below consults ONLY `problem.objective.as_ref()` —
/// deliberately NEVER `.or(build_centrality_objective(..))`, diverging from
/// [`solve_core_with_sd_tolerance`]'s `effective_objective`. Steward ruling
/// (esc-5711-1): the synthetic centrality objective (PRD η) exists only to
/// pick a deterministic representative point out of a feasible continuum; it
/// is not a user-authored semantic contract and must never DISCHARGE a
/// §11.6 well-determinedness obligation the user never authored. Worse, a
/// strictly-better centrality score at a different point is POSITIVE
/// EVIDENCE of non-uniqueness (the feasible region has interior room to
/// move) — feeding it into the suppression branch would invert that signal.
/// Measured self-defeat if this rule is ignored: with the synthetic fallback
/// wired in, `strict_auto_non_unique_returns_infeasible`
/// (`solver_integration.rs`; `x>10mm ∧ y>10mm`, no explicit objective)
/// flips to `Solved{unique:true}` — its incumbent (0.0505,0.0505) scores
/// -0.0405 on the synthetic min-slack objective, but the derived-box
/// perturbed anchor (0.091,0.091) re-solves to itself at -0.081, strictly
/// better — converting the codebase's canonical deliberately-underdetermined
/// fixture into exactly the silent false-negative #5711 exists to
/// eliminate. Do NOT "fix" this by adding `.or(synth)` back.
///
/// # The γ `cost_robustness_tradeoff` path (task #5711 amendment 2)
///
/// When `problem.objective` carries the γ `cost_robustness_tradeoff` marker
/// (task #4791) this function does not perturb at all: it returns
/// [`strict_autos_constraint_bracketed`] directly, before the re-solve below,
/// with [`params_in_underivable_constraints`] supplying the abstention
/// evidence that keeps a derivation blind spot (`Eq`, coefficient, nonlinear
/// or coupled bounds) from masquerading as an unbounded side (esc-5711-3).
///
/// **Why the perturbation machinery is STRUCTURALLY INAPPLICABLE here.**
/// [`solve_cost_robustness_tradeoff`] is SEED-DEPENDENT BY CONSTRUCTION — its
/// own doc records that all three of its solves share the SAME deterministic
/// `initial` seed "so the whole dispatch stays reproducible", which is
/// reproducibility for a FIXED seed, never seed-invariance. Concretely, a
/// floor-free pure-cost minimise's true optimum sits an infinitesimal distance
/// PAST the constraint boundary (the penalty has zero slope at its own root),
/// so [`solve_core_with_sd_tolerance`]'s "optimizer drifted infeasible → fall
/// back to the initially-feasible seed" safety net returns THE SEED ITSELF, and
/// re-seeding therefore MOVES the answer. (Independently corroborated in
/// tracked source: `examples/cost_robustness_tradeoff.ri` documents exactly this
/// drift-fallback-returns-the-seed behaviour.) A perturbation check compares
/// f(seed_A) against f(seed_B) for a seed-dependent f, so every verdict it
/// yields is an artifact of the ANCHOR, not evidence about the model.
///
/// **The rule that replaces it.** §11.6 test (2) asks whether the value is
/// uniquely optimal under the applicable objective; for γ that objective is the
/// BLEND, taken over the constraint-derived feasible interval. So the question
/// is answerable with NO solve at all: if every strict auto's derived interval
/// is bounded on BOTH sides, the blend's argmin is fixed by the user's own
/// constraints plus objective — well-determined, return `true`. If any side is
/// missing, that side is supplied by [`default_bounds_for`], a solver-internal
/// default the user never authored, so the resolved value is
/// DEFAULT-BOUNDS-determined rather than model-determined — genuine
/// non-determinedness, return `false`.
///
/// **A/B evidence.** MEASURED, not asserted. The full per-model A/B table
/// (main vs. branch-before-fix vs. branch-after-fix) is recorded in task
/// #5711's record rather than inlined here; both of its rows are pinned as
/// LIVE TESTS, which is the form that cannot go stale:
/// `gamma_strict_auto_two_sided_bracket_is_solved` for the two-sided bracket
/// (`Infeasible{ConstraintNonUnique}` for ALL THREE λ before this fix,
/// `Solved{unique:true}` on main and after it) and
/// `gamma_strict_auto_one_sided_stays_non_unique` for the one-sided shape
/// (`ConstraintNonUnique` throughout — a pre-existing verdict, not a
/// regression), both in `tests/cost_robustness_tradeoff_blend.rs`.
///
/// λ=1 regressing is what identifies the mechanism: there the blend is a
/// positive-affine transform of cost alone, so "λ<1 pulls the blend off the
/// min-cost point" cannot be the explanation. Also measured and REJECTED: a
/// dispatch-consistent re-solve (running [`solve_cost_robustness_tradeoff`] for
/// the perturbed anchor and comparing on the blend scale) recovers only λ=0 and
/// leaves λ=0.5 and λ=1 `Infeasible` — because it does not address
/// seed-dependence, it merely relabels it.
///
/// **Do NOT simplify this to `return true` for γ.** A blanket abstention was
/// MEASURED to turn the one-sided prd-gate fixture's loud `error: strict auto
/// parameter resolution is not uniquely determined` into a silent
/// `thickness = 10 m` — 10 m being [`default_bounds_for`]'s `Length` ceiling,
/// i.e. a value pinned by a SOLVER-INTERNAL default rather than by the user's
/// model. That is a second, opposite behaviour change in the very commit meant
/// to remove one. `gamma_strict_auto_one_sided_stays_non_unique`
/// (`tests/cost_robustness_tradeoff_blend.rs`) is the standing guard against it;
/// its already-green status is deliberate, not accidental. Same convention as
/// the "Do NOT fix this by adding `.or(synth)`" note above.
///
/// # Per-fixture measurement (task #5711 pre-1)
///
/// Swapping only the anchor box for the derived seed box flips SIX
/// previously-`Solved` fixtures — re-derived per fixture from a real
/// measurement rather than generalised from one probe (the exact error
/// esc-5618-3 warned against), all six carrying an EXPLICIT
/// `problem.objective`, so the explicit-only scoring rule above changes none
/// of the six. The per-fixture incumbent-vs-perturbed SCORE table lives in
/// task #5711's record, not here: raw scores rot against any solver retune
/// while the disposition below does not.
///
/// - FIVE (the `warm_start_*` family) classify `IncumbentSuboptimal` — their
///   incumbent is the drift-fallback or budget-exhausted seed, which the
///   perturbed re-solve beats — and KEEP `free: false`, so the load-bearing
///   warm-start signal is preserved rather than bulk-flipped.
/// - Exactly ONE is a genuine tie:
///   `defined_objective_at_fallback_returns_solved` (this file's `mod
///   tests`), whose `Minimize(if x<=22mm then 1e8 else x)` is the CONSTANT
///   `1e8` across the entire feasible region, so both points score
///   identically → `NonUnique`. It is flipped to `free: true`, with a
///   comment at the fixture naming this mechanism.
///
/// Also confirmed at the same measurement: the flip set is exactly six (all
/// other test binaries in the crate stayed 100% green), and the dedicated
/// uniqueness 2×2 matrix (`strict_auto_unique_solution_returns_unique_true` /
/// `strict_auto_non_unique_returns_infeasible`) is unaffected by
/// the box swap alone.
///
/// # Open interval / infimum-not-attained ruling
///
/// A strict bracket with no attainable optimum — e.g. `5mm < x < 6mm` under
/// `minimize(x)` — has no argmin, so §11.6's "uniquely optimal under the
/// applicable objective" is VACUOUS rather than false: the problem is not
/// underdetermined, it simply has no optimum. `verify_uniqueness` is
/// DEFINED to ABSTAIN — report "cannot prove non-unique" — rather than
/// manufacture a `ConstraintNonUnique` report here; doing otherwise would be
/// exactly the conflation esc-5618-3 identified. The `IncumbentSuboptimal`
/// branch delivers this for free, with no special case: on an open interval
/// every incumbent is beaten by a point nearer the (unattainable) bound, so
/// the suppression branch always fires. Detecting "no optimum exists" and
/// surfacing it as its OWN diagnostic is a separate capability, explicitly out
/// of scope for #5711 and already filed as task #5975 — do not re-file. (The
/// ORDINARY suboptimal-incumbent case, where an optimum does exist and this
/// branch discards a strictly better point without telling anyone, is the
/// distinct gap tracked on task #6901 — see the `IncumbentSuboptimal` arm
/// below.)
fn verify_uniqueness(
    problem: &ResolutionProblem,
    solved_values: &HashMap<ValueCellId, Value>,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> bool {
    // Derive the constraint intervals ONCE for this call (review suggestion 1).
    // Two consumers below need them and they used to be derived separately for
    // each: `derive_param_intervals` walks every constraint and calls
    // `constant_operand_value` -> `reify_expr::eval_expr` on each candidate far
    // operand, which on a dispatch-backed model is real evaluation work. It is a
    // pure function of `problem` + `dispatch`, so hoisting it changes no verdict.
    let intervals = derive_param_intervals(
        &problem.auto_params,
        &problem.constraints,
        &problem.current_values,
        &problem.functions,
        dispatch,
    );

    // Build perturbed initial point: reflect each param to the opposite
    // end of its bounds range from the solution.  #5711 step-5: this is now
    // the #5618 constraint-derived SEED box (the same box `multistart_points`
    // gets from `derived_seed_box`, composed here through that function's
    // shared `seed_box_from_intervals` half; include_strict = true since an
    // anchor is a seed point, not a clamp target) rather than the
    // unconstrained effective_bounds box — see the header note above for the
    // measured mechanism this fixes.
    let bounds = seed_box_from_intervals(&problem.auto_params, &intervals);
    let (perturbed, missing) =
        build_perturbation_anchors(&problem.auto_params, solved_values, &bounds);
    if !missing.is_empty() {
        tracing::warn!(
            "verify_uniqueness: {} solved value(s) missing or non-numeric {:?}; \
             using midpoint as comparison anchor \
             (perturbation start defaults to lower-half side)",
            missing.len(),
            missing
        );
        return false;
    }

    // #5711 amendment 2: the γ `cost_robustness_tradeoff` path answers §11.6
    // WITHOUT a re-solve. `solve_cost_robustness_tradeoff` is SEED-DEPENDENT by
    // construction, so the perturbation machinery below is structurally
    // inapplicable there — see this function's doc for the measured ruling and
    // the A/B evidence table. Positioned deliberately: AFTER the
    // missing/non-numeric guard above, so γ keeps `solutions_agree`'s
    // loud-not-silent contract, and BEFORE the re-solve, so the inapplicable
    // solve never runs.
    //
    // The RAW `intervals` derived above are used here, NOT the composed
    // `bounds`: `seed_box_from_intervals` substitutes a solver-internal default
    // for a missing side and would erase exactly the `None` this predicate keys
    // on.
    if problem
        .objective
        .as_ref()
        .and_then(|obj| obj.cost_robustness_lambda)
        .is_some()
    {
        // esc-5711-3: a param whose missing side is attributable to a
        // constraint the derivation could not READ abstains rather than
        // reporting non-uniqueness — see `params_in_underivable_constraints`.
        //
        // Computed LAZILY (review suggestion 1). The predicate is MONOTONE in
        // its `underivable` argument — that set can only move a param from
        // "not bracketed" to "abstain", never the reverse — so when the raw
        // derivation already brackets every strict auto the evidence set cannot
        // change the verdict, and the per-conjunct re-derivation (which re-runs
        // `derive_from_expr`, and through it `eval_expr`, once per leaf
        // conjunct) is skipped entirely. That is the common case: the γ models
        // this branch exists to keep green.
        let bracketed = strict_autos_constraint_bracketed(
            &problem.auto_params,
            &intervals,
            &HashSet::new(),
        ) || strict_autos_constraint_bracketed(
            &problem.auto_params,
            &intervals,
            &params_in_underivable_constraints(
                &problem.auto_params,
                &problem.constraints,
                &problem.current_values,
                &problem.functions,
                dispatch,
            ),
        );
        // debug!, deliberately NOT warn!: solver_tracing.rs's
        // `normal_solve_emits_zero_warns` expectation and step-4's exact-WARN-count
        // assertion must both stay untouched by this branch.
        tracing::debug!(
            bracketed,
            "uniqueness check: cost_robustness_tradeoff objective — deciding by \
             constraint-bracketing of the strict autos rather than by perturbation \
             (the γ dispatch is seed-dependent, so a re-solve from a different anchor \
             measures the anchor, not the model)"
        );
        return bracketed;
    }

    tracing::debug!(
        n_params = problem.auto_params.len(),
        "verifying uniqueness via perturbation"
    );

    // Re-solve from the perturbed starting point at the tight NM_SD_TOLERANCE.
    // The task #4700 decoupling (UNIQUENESS_SD_TOLERANCE = 1e-15) has been
    // reverted by task #4710: connector-internal autos are now pinned at the
    // eval layer (engine_eval::connector_pin_if_determined) and excluded from
    // the parent solver problem, so no unconstrained strict autos reach the
    // solver from that path.  The tight tolerance correctly flags any genuinely
    // unconstrained strict auto as ConstraintNonUnique (esc-4700-34 root-fixed).
    match solve_core(problem, &perturbed, dispatch).0 {
        SolveResult::Solved {
            values: perturbed_values,
            ..
        } => {
            // #5711: classify against BOTH §11.6 well-determinedness tests,
            // not parameter agreement alone. Objective scoring consults ONLY
            // the EXPLICIT `problem.objective` — never
            // `.or(build_centrality_objective(..))` — deliberately diverging
            // from `solve_core_with_sd_tolerance`'s `effective_objective`.
            // See this function's doc for the ruling (esc-5711-1): the
            // centrality objective is a solver-internal tie-break with no
            // user-authored semantic contract behind it, and a
            // strictly-better centrality score at a different point is
            // evidence FOR non-uniqueness, not a suppression signal. Do NOT
            // "fix" this by adding `.or(synth)`.
            //
            // The closure below is invoked at most once, and only if
            // `classify_uniqueness` finds the params differ — see this
            // function's doc for why that laziness matters (review
            // suggestion 7). It carries NO γ special case: #5711 amendment 2
            // returns for the `cost_robustness_tradeoff` marker before the
            // re-solve above, so no γ problem can reach this point and a
            // second γ policy here would be dead code implying a live one.
            match classify_uniqueness(&problem.auto_params, solved_values, &perturbed_values, || {
                score_solution(problem, solved_values, dispatch)
                    .zip(score_solution(problem, &perturbed_values, dispatch))
            }) {
                UniquenessVerdict::Unique => true,
                UniquenessVerdict::IncumbentSuboptimal {
                    incumbent: incumbent_score,
                    perturbed: perturbed_score,
                } => {
                    // Suppress: an optimality finding, not a §11.6
                    // non-uniqueness one — see
                    // `UniquenessVerdict::IncumbentSuboptimal`'s doc.
                    //
                    // KNOWN GAP, tracked on task #6901 (review suggestion 1).
                    // Returning `true` here makes `finalise_uniqueness` emit
                    // `Solved { unique: true }` carrying the INCUMBENT — a
                    // point we have just obtained POSITIVE EVIDENCE is not the
                    // argmin, having found a strictly better feasible point
                    // and discarded it. This `warn!` is the only surfacing:
                    // it never reaches the `.ri` author, and the resulting
                    // `OptimalityStatus::BestFound { reason }` is
                    // indistinguishable from a clean converged solve. Not a
                    // corner — per the per-fixture measurement above this arm
                    // fires on all five `warm_start_*` fixtures today.
                    //
                    // NOT fixable inside #5711, and the two candidate fixes
                    // are both larger than a warn-string change:
                    //   (a) surface it — `SolveResult::Solved` has no
                    //       diagnostics channel, so this needs a new
                    //       `BestFoundReason` variant or a coded diagnostic,
                    //       i.e. `reify-ir` / `reify-core` carrier types
                    //       outside this task's scope;
                    //   (b) ADOPT the better perturbed point instead of
                    //       discarding it — changes resolved VALUES on those
                    //       five fixtures and breaks the monotonicity property
                    //       documented on `classify_uniqueness`, so it needs
                    //       its own measurement pass.
                    // #6901's constituent β ("honesty floor — stop asserting
                    // unproven uniqueness, typed cause") already owns this
                    // class and already retypes this function's
                    // NON-convergence arm in the carrier; this arm is the
                    // third assertion of the same class and belongs with it.
                    // Do NOT quietly downgrade the verdict here instead —
                    // suppression is what keeps the classifier monotone.
                    tracing::warn!(
                        incumbent_score,
                        perturbed_score,
                        "verify_uniqueness: IncumbentSuboptimal — perturbed re-solve scored \
                         strictly better than the incumbent under the explicit objective; \
                         suppressing (cannot prove non-unique) rather than reporting \
                         ConstraintNonUnique, since the incumbent was never the argmin"
                    );
                    true
                }
                UniquenessVerdict::NonUnique => false,
            }
        }
        _ => {
            // If the perturbed solve fails (Infeasible/NoProgress), we can't
            // prove non-uniqueness — conservatively assume unique.
            tracing::debug!("uniqueness check: perturbed solve did not converge; assuming unique");
            true
        }
    }
}

/// Maps the `iter_limited` flag to a [`reify_ir::BestFoundReason`] for
/// [`reify_ir::OptimalityStatus::BestFound`].
///
/// Both forms are `BestFound` — Nelder-Mead is derivative-free and budget-bounded
/// so it NEVER achieves `ProvenOptimal` (invariant I3). The variant distinguishes
/// whether the iteration budget was exhausted before simplex convergence.
fn best_found_reason(iter_limited: bool) -> reify_ir::BestFoundReason {
    if iter_limited {
        reify_ir::BestFoundReason::IterationLimit
    } else {
        reify_ir::BestFoundReason::ConvergedWithinBudget
    }
}

/// Finalises the uniqueness verdict for a `Solved` result.
///
/// For problems with any strict (non-free) auto param, re-solves from a
/// perturbed starting point ([`verify_uniqueness`]) and either confirms
/// `unique: true` or demotes the whole result to
/// `SolveResult::Infeasible` (`ConstraintNonUnique`) when a different
/// solution is found — a strict auto param MUST be uniquely determined by
/// the constraints, independent of which candidate is being finalised.
/// All-free problems skip the re-solve entirely and report `unique: false`
/// (free auto params accept any feasible solution).
///
/// Shared by [`DimensionalSolver::solve_with_meta`] (applied to the sole
/// candidate — `solve()`'s behaviour is unchanged by this extraction) and
/// the best-of-K [`ConstraintSolver::solve_ranked`] override (applied ONLY
/// to the winning candidate; alternative optima are by definition not *the*
/// unique solution, so non-winning candidates carry `unique: false`
/// directly, without a re-solve).
fn finalise_uniqueness(
    problem: &ResolutionProblem,
    values: HashMap<ValueCellId, Value>,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> SolveResult {
    // Check if any param requires uniqueness verification (strict auto)
    let has_strict = problem.auto_params.iter().any(|p| !p.free);
    if has_strict {
        if verify_uniqueness(problem, &values, dispatch) {
            SolveResult::Solved {
                values,
                unique: true,
            }
        } else {
            // Strict auto params require a unique solution. The
            // perturbation-based check found a different solution,
            // indicating the problem is underdetermined.
            SolveResult::Infeasible {
                diagnostics: vec![
                    reify_core::Diagnostic::error(
                        "strict auto parameter resolution is not uniquely \
                              determined \u{2014} consider using auto(free) \
                              for exploration",
                    )
                    .with_code(DiagnosticCode::ConstraintNonUnique),
                ],
            }
        }
    } else {
        // All params are free — skip uniqueness verification entirely.
        // Free auto params accept any feasible solution, so we report
        // unique=false to let the eval engine emit appropriate warnings.
        SolveResult::Solved {
            values,
            unique: false,
        }
    }
}

/// The historical single-candidate [`ConstraintSolver::solve_ranked`] body
/// (pre-δ #5016): wraps one `(SolveResult, SolveMeta)` pair into a
/// 1-candidate [`reify_ir::RankedSolveResult`].
///
/// Used verbatim by every multistart-ineligible gate branch (dim<=1, no
/// objective, or a `cost_robustness_tradeoff` objective — PRD §5.3) AND as
/// the best-of-K all-infeasible fallback (`solve_ranked`'s multistart branch,
/// re-anchored on the historical seed via `solve_with_meta`), so neither path
/// can drift from the single-candidate contract invariants (F-result I1
/// byte-identical test, B1/B2, BT6 — all dim=1 fixtures).
fn rank_single(
    problem: &ResolutionProblem,
    result: SolveResult,
    meta: SolveMeta,
    dispatch: Option<&dyn reify_ir::ComputeDispatch>,
) -> reify_ir::RankedSolveResult {
    use reify_ir::{OptimalityStatus, RankedCandidate, RankedSolveResult};
    match result {
        SolveResult::Solved { values, unique } => {
            // Compute objective score at the solved value map via
            // `score_solution`, which keys off problem.objective (the USER
            // objective), NOT effective_objective, per I3/I4: a
            // feasibility-only solve reports FeasibilityOnly + None even
            // when the solver internally optimized a synthetic centrality
            // objective.
            let objective_score = score_solution(problem, &values, dispatch);
            // Key optimality off objective_score (not problem.objective.is_some())
            // to preserve I4: BestFound is only emitted when the score is present.
            // In the edge case where eval_objective_set returns None despite
            // problem.objective.is_some() (e.g. objective expression non-numeric
            // at the solved map), fall back to FeasibilityOnly so that
            // objective_score: None is never paired with BestFound.
            let optimality = match &objective_score {
                Some(_) => OptimalityStatus::BestFound {
                    reason: best_found_reason(meta.iter_limited),
                },
                None => OptimalityStatus::FeasibilityOnly,
            };
            RankedSolveResult::Ranked {
                candidates: vec![RankedCandidate {
                    values,
                    objective_score,
                    unique,
                }],
                optimality,
            }
        }
        // Infeasible and NoProgress are structurally identical to the default
        // trait lift — delegate to the shared helper to avoid drift.
        non_solved => non_solved
            .into_ranked_pass_through()
            .expect("Solved arm already handled above"),
    }
}

impl DimensionalSolver {
    /// Run the full solve orchestration and return both the result and its metadata.
    ///
    /// This is the canonical implementation. [`ConstraintSolver::solve`] is a thin
    /// wrapper that discards the [`SolveMeta`]; [`ConstraintSolver::solve_ranked`]
    /// consumes both to populate [`reify_ir::RankedCandidate::objective_score`] and
    /// [`reify_ir::OptimalityStatus`] without re-running the solver (I1).
    fn solve_with_meta(
        &self,
        problem: &ResolutionProblem,
        dispatch: Option<&dyn reify_ir::ComputeDispatch>,
    ) -> (SolveResult, SolveMeta) {
        // Trivial case: no auto parameters to solve for
        if problem.auto_params.is_empty() {
            return (
                SolveResult::Solved {
                    values: HashMap::new(),
                    unique: true,
                },
                SolveMeta::default(),
            );
        }

        let initial = extract_initial_point(problem, dispatch);

        // γ cost_robustness_tradeoff dispatch (task #4791, PRD §2.4/§8.1): a
        // `minimize cost_robustness_tradeoff(<money-expr>, λ)` objective REPLACES
        // the plain solve (and the α robustness floor) with a normalised
        // two-anchor blend. Detected via the `cost_robustness_lambda` marker
        // threaded onto `ObjectiveSet` by the compiler (entity.rs). Every other
        // objective shape falls through to the ordinary `solve_core` path,
        // unchanged.
        let (result, meta) = match problem
            .objective
            .as_ref()
            .and_then(|obj| obj.cost_robustness_lambda)
        {
            Some(lambda) => solve_cost_robustness_tradeoff(problem, &initial, lambda, dispatch),
            None => solve_core(problem, &initial, dispatch),
        };

        let final_result = match result {
            SolveResult::Solved { values, .. } => finalise_uniqueness(problem, values, dispatch),
            other => other, // Infeasible, NoProgress pass through unchanged
        };
        (final_result, meta)
    }
}

impl ConstraintSolver for DimensionalSolver {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        self.solve_with_meta(problem, None).0
    }

    /// Task #4880: `solve` is the `dispatch = None` specialisation of this method.
    /// With `None` the cost loop constructs `EvalContext::new(..)` exactly as it did
    /// pre-#4880 (see [`ctx_with`]), so this is a pure superset — no existing caller
    /// or test changes behaviour.
    fn solve_with_dispatch(
        &self,
        problem: &ResolutionProblem,
        dispatch: Option<&dyn reify_ir::ComputeDispatch>,
    ) -> SolveResult {
        self.solve_with_meta(problem, dispatch).0
    }

    fn solve_ranked(&self, problem: &ResolutionProblem) -> reify_ir::RankedSolveResult {
        self.solve_ranked_impl(problem, None)
    }

    /// Task #4880: `solve_ranked` is the `dispatch = None` specialisation of this
    /// method — see [`DimensionalSolver::solve_ranked_impl`].
    fn solve_ranked_with_dispatch(
        &self,
        problem: &ResolutionProblem,
        dispatch: Option<&dyn reify_ir::ComputeDispatch>,
    ) -> reify_ir::RankedSolveResult {
        self.solve_ranked_impl(problem, dispatch)
    }
}

impl DimensionalSolver {
    /// Shared implementation behind [`ConstraintSolver::solve_ranked`] and
    /// [`ConstraintSolver::solve_ranked_with_dispatch`] (task #4880). Lifted out of
    /// the trait impl so the `@optimized` compute-dispatch hook can be threaded
    /// through the best-of-K multistart loop without duplicating it.
    fn solve_ranked_impl(
        &self,
        problem: &ResolutionProblem,
        dispatch: Option<&dyn reify_ir::ComputeDispatch>,
    ) -> reify_ir::RankedSolveResult {
        use reify_ir::{OptimalityStatus, RankedCandidate, RankedSolveResult};

        // Best-of-K deterministic multistart gate (PRD
        // `docs/prds/v0_6/whole-model-objective-coupling.md` §5.3, §11 Q4,
        // task δ #5016): a merged cluster is always >=2 coupled auto params
        // under a governing objective, so dim>=2 + objective is the in-scope
        // proxy for "merged cluster" (`ResolutionProblem` carries no cluster
        // marker — see design notes). dim<=1, no-objective, and the
        // `cost_robustness_tradeoff` special form (its own single-start
        // two-anchor blend, task γ #4791) keep today's single-candidate path
        // VERBATIM — this is REQUIRED to preserve the dim=1 invariants
        // (F-result I1 byte-identical test, B1/B2, BT6).
        //
        // Deliberately NOT gated on auto-param free/strict shape: multistart's
        // value chiefly targets free-auto exploration (§5.3), but strict-auto
        // clusters are left eligible too rather than adding a second gate
        // predicate for what is a benign edge case — see the winner/`solve()`
        // divergence note below for the resulting, low-risk
        // Solved-vs-Infeasible corner case and why it's accepted as-is.
        let multistart_eligible = match &problem.objective {
            Some(obj) => problem.auto_params.len() >= 2 && obj.cost_robustness_lambda.is_none(),
            None => false,
        };

        if !multistart_eligible {
            let (result, meta) = self.solve_with_meta(problem, dispatch);
            return rank_single(problem, result, meta, dispatch);
        }

        // ---- best-of-K multistart (dim>=2 + objective, §5.3/§11 Q4) ----
        //
        // Run the EXISTING solve_core (Money robustness floor + centrality
        // synth + drift fallback all inherited unchanged, since this loop
        // calls the SAME function `solve_with_meta` uses for the
        // single-start path — task #4789 α's `apply_robustness_floor = true`
        // is therefore inherited per start, not re-implemented here) once per
        // deterministic seed from `multistart_points`; score each Solved
        // candidate against the USER objective (I3/I4), exactly as the
        // single-candidate path above already does.
        //
        // Cost: K = 2*(dim+1) full `solve_core` solves — linear in `dim`,
        // with no cap. §11 Q4 resolved this growth rate deliberately (a
        // seeded per-cluster global solver for larger clusters is out of
        // scope, §10), on the expectation that real merged clusters stay in
        // the single-digit-to-low-tens dimension range.
        let starts = multistart_points(problem, dispatch);
        let mut scored: Vec<(usize, HashMap<ValueCellId, Value>, f64, bool)> = Vec::new();

        for (start_index, start) in starts.iter().enumerate() {
            let (result, meta) = solve_core(problem, start, dispatch);
            if let SolveResult::Solved { values, .. } = result {
                let objective_score = score_solution(problem, &values, dispatch);
                if let Some(score) = objective_score {
                    scored.push((start_index, values, score, meta.iter_limited));
                }
                // A Solved-but-unscored candidate (objective non-numeric at
                // this particular start) is dropped — it cannot be ranked,
                // and the all-starts-unscored case is exactly the
                // `scored.is_empty()` fallback below (I4: never pair
                // BestFound with objective_score: None).
            }
        }

        // No start yielded a feasible, scoreable candidate — map the
        // seed-start's own result (recomputed via the single-candidate path,
        // which uses the same `extract_initial_point` seed as start #0) via
        // the shared pass-through, exactly like every other Infeasible/
        // NoProgress arm in this module.
        if scored.is_empty() {
            let (result, meta) = self.solve_with_meta(problem, dispatch);
            return rank_single(problem, result, meta, dispatch);
        }

        // Rank feasible candidates by strict ascending objective_score, ties
        // broken by ascending start index (start #0, the historical seed,
        // wins exact ties). candidates[0] is the optimum (I2). `partial_cmp`
        // only returns `None` for NaN, which `eval_objective_set` already
        // filters out (`.filter(|v| v.is_finite())`), so every score here is
        // a well-ordered finite f64 — `unwrap_or(Equal)` is a defensive
        // fallback, never actually exercised.
        scored.sort_by(|a, b| {
            a.2.partial_cmp(&b.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });

        let (_winner_start, winner_values, winner_score, winner_iter_limited) = scored.remove(0);

        // Only the winner is uniqueness-finalised — alternative optima are
        // by definition not *the* unique solution, so non-winners carry
        // `unique: false` directly (no re-solve). A non-unique winner
        // (strict auto params only) is demoted to Infeasible by
        // `finalise_uniqueness`, exactly as `solve_with_meta` would — so a
        // non-unique winner can never be silently reported as BestFound.
        //
        // Authoritative verdict, may differ from solve(): the winner is not
        // necessarily start #0 (the seed `solve()` anchors on). For a
        // strict-auto multi-basin problem, `solve()` can land in one basin
        // and pass its own perturbation re-solve (Solved), while the
        // multistart winner is a different, better-scoring basin whose
        // perturbation re-solve lands elsewhere and gets demoted
        // (Infeasible) — so `solve()` and `solve_ranked()` can disagree on
        // Solved-vs-Infeasible for the SAME `problem`. This is intentional:
        // the winner's `finalise_uniqueness` verdict is authoritative for
        // `solve_ranked` (I2 — candidate[0] must be the best-scoring
        // FEASIBLE-AND-UNIQUE point, never a stale verdict borrowed from a
        // different start). Low-risk in practice: strict-auto non-uniqueness
        // is a property of the shared constraint system rather than the
        // objective, and multistart's value chiefly targets free-auto
        // exploration (§5.3). See
        // `solve_ranked_multistart_winner_non_unique_demotes_to_infeasible`
        // for the demotion mechanism itself.
        //
        // NOT deduplicated: `scored` (the non-winning candidates folded in
        // below) carries every feasible start verbatim. For a single-basin
        // objective — the common case — most or all of the K starts
        // converge to the SAME point, so `candidates[1..]` are
        // near-/byte-identical convergences of that ONE optimum, not K
        // distinct alternative designs. Today's only consumers
        // (`SolverRegistry::solve_ranked`, engine_eval.rs) read
        // `candidates[0]` alone, so this is currently harmless; a future
        // consumer that iterates `candidates[1..]` expecting genuinely
        // different solutions must dedupe by resolved-value fingerprint
        // (e.g. within `UNIQUENESS_REL_TOL`) itself first.
        match finalise_uniqueness(problem, winner_values, dispatch) {
            SolveResult::Solved { values, unique } => {
                let mut candidates = Vec::with_capacity(scored.len() + 1);
                candidates.push(RankedCandidate {
                    values,
                    objective_score: Some(winner_score),
                    unique,
                });
                candidates.extend(scored.into_iter().map(|(_, values, score, _)| {
                    RankedCandidate {
                        values,
                        objective_score: Some(score),
                        unique: false,
                    }
                }));
                RankedSolveResult::Ranked {
                    candidates,
                    optimality: OptimalityStatus::BestFound {
                        reason: best_found_reason(winner_iter_limited),
                    },
                }
            }
            non_solved => non_solved
                .into_ranked_pass_through()
                .expect("finalise_uniqueness only ever returns Solved or Infeasible"),
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_ir::{ConstraintSolver, ResolutionProblem, SolveResult, TAG_CONDITIONAL, ValueMap};

    // ---- shared solver test helpers ----

    /// Returns a canonical single-param tuple: (`ValueCellId::new("Part","x")`, one-element
    /// `Vec<AutoParam>` with `Type::length()`, bounds `(0.0, 1.0)`, `free: false`).
    /// Used by `solutions_agree_*` and `build_perturbation_anchors_*` tests that work with one parameter.
    fn test_param() -> (reify_core::ValueCellId, Vec<reify_ir::AutoParam>) {
        use reify_core::{Type, ValueCellId};
        use reify_ir::AutoParam;
        let id = ValueCellId::new("Part", "x");
        let params = vec![AutoParam {
            id: id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 1.0)),
            free: false,
        }];
        (id, params)
    }

    /// The pre-#5618 reflection box: each param's own [`super::effective_bounds`].
    ///
    /// Task #5618 made [`super::build_perturbation_anchors`]' box caller-supplied so
    /// `verify_uniqueness` can reflect inside the CONSTRAINT-DERIVED region. The
    /// `build_perturbation_anchors_*` fixtures below set explicit
    /// `bounds: Some((0.0, 1.0))`, so this reproduces byte-for-byte the box they were
    /// written against and their expected anchors are unchanged.
    fn effective_bounds_box(params: &[reify_ir::AutoParam]) -> Vec<(f64, f64)> {
        params.iter().map(super::effective_bounds).collect()
    }

    /// Returns a `Value::Scalar` with the given `si_value` and `DimensionVector::LENGTH`.
    /// `solutions_agree_*` and `build_perturbation_anchors_*` tests use `Type::length()`, so a
    /// fixed-dimension helper avoids repeating the dimension on every call site.
    fn scalar(v: f64) -> reify_ir::Value {
        use reify_core::DimensionVector;
        use reify_ir::Value;
        Value::Scalar {
            si_value: v,
            dimension: DimensionVector::LENGTH,
        }
    }

    // ---- end shared solver test helpers ----

    // ---- verify_uniqueness test helpers ----

    /// Runs `verify_uniqueness(problem, solved_values)` under a warn-capturing tracing
    /// subscriber and asserts the aggregated WARN contract:
    ///
    /// 1. Exactly one WARN event containing `"midpoint as comparison anchor"` is emitted.
    /// 2. Every substring in `expected_warn_substrings` appears in the joined WARN messages
    ///    (verifies that the relevant `ValueCellId`s were rendered into the message body via
    ///    the `{:?}` placeholder; `WarnCapturingSubscriber`'s `MessageVisitor` only captures
    ///    the `message` field and ignores all structured fields — see
    ///    `crates/reify-test-support/src/tracing_support.rs`).
    ///
    /// Returns the `unique` flag so each call site can assert the verdict with its own
    /// descriptive message, consistent with the named-local style of the sibling tests.
    ///
    /// See the section comment below (above `verify_uniqueness_aggregates_warn_for_multiple_missing_params`)
    /// for the early-return coverage rationale (solve_core and solutions_agree are NOT
    /// invoked on the missing/non-numeric path).
    fn assert_verify_uniqueness_aggregated_warn(
        problem: &ResolutionProblem,
        solved_values: &std::collections::HashMap<reify_core::ValueCellId, reify_ir::Value>,
        expected_warn_substrings: &[&str],
    ) -> bool {
        use reify_test_support::warn_capturing_subscriber;

        use super::verify_uniqueness;

        let (subscriber, capture) = warn_capturing_subscriber();
        let unique = tracing::subscriber::with_default(subscriber, || {
            verify_uniqueness(problem, solved_values, None)
        });

        let msgs = capture.messages();
        let vu_warn_count = msgs
            .iter()
            .filter(|m| m.contains("midpoint as comparison anchor"))
            .count();
        assert_eq!(
            vu_warn_count, 1,
            "expected exactly 1 verify_uniqueness WARN containing 'midpoint as comparison \
             anchor'; got {vu_warn_count}; messages: {msgs:?}"
        );

        let all_msgs = msgs.join("\n");
        for substring in expected_warn_substrings {
            assert!(
                all_msgs.contains(substring),
                "expected WARN messages to contain {substring:?}; messages: {msgs:?}"
            );
        }

        // Pin the rendered count placeholder ({} via missing.len()) so a future cleanup
        // cannot silently drop it from the format-string body without test failure.
        let expected_count_fragment = format!("{} solved value(s)", expected_warn_substrings.len());
        assert!(
            all_msgs.contains(&expected_count_fragment),
            "expected WARN messages to contain rendered count {expected_count_fragment:?} \
             (via the {{}} placeholder in the format-string body); messages: {msgs:?}"
        );

        unique
    }

    // ---- end verify_uniqueness test helpers ----

    #[test]
    fn dimensional_solver_exists_and_implements_trait() {
        use crate::DimensionalSolver;

        // Verify it can be used as a trait object
        let solver = DimensionalSolver;
        let _boxed: Box<dyn ConstraintSolver> = Box::new(solver);
    }

    #[test]
    fn build_trial_values_inserts_auto_params() {
        use super::build_trial_values;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, Value};

        let thickness_id = ValueCellId::new("Bracket", "thickness");
        let width_id = ValueCellId::new("Bracket", "width");

        // Base map has width=80mm
        let mut base = ValueMap::new();
        base.insert(
            width_id.clone(),
            Value::Scalar {
                si_value: 0.080,
                dimension: DimensionVector::LENGTH,
            },
        );

        let params = vec![AutoParam {
            id: thickness_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
            free: false,
        }];

        let trial = build_trial_values(&base, &params, &[0.005], &[], &[], None);

        // Auto param should be inserted with correct dimension
        let thickness = trial.get(&thickness_id).expect("thickness should exist");
        match thickness {
            &Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (si_value - 0.005).abs() < 1e-15,
                    "si_value should be 0.005, got {}",
                    si_value
                );
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }

        // Non-auto value should be preserved
        let width = trial.get(&width_id).expect("width should be preserved");
        match width {
            &Value::Scalar { si_value, .. } => {
                assert!((si_value - 0.080).abs() < 1e-15, "width should be 0.080");
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn build_trial_values_multi_param_regression() {
        use super::build_trial_values;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, Value};

        let thickness_id = ValueCellId::new("Bracket", "thickness");
        let angle_id = ValueCellId::new("Bracket", "angle");
        let width_id = ValueCellId::new("Bracket", "width");

        // Base map has a pre-existing non-auto value (width=80mm)
        let mut base = ValueMap::new();
        base.insert(
            width_id.clone(),
            Value::Scalar {
                si_value: 0.080,
                dimension: DimensionVector::LENGTH,
            },
        );

        let params = vec![
            AutoParam {
                id: thickness_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            },
            AutoParam {
                id: angle_id.clone(),
                param_type: Type::angle(),
                bounds: Some((0.0, std::f64::consts::PI)),
                free: false,
            },
        ];

        let trial = build_trial_values(&base, &params, &[0.005, 1.2], &[], &[], None);

        // First auto param: length with correct dimension
        let thickness = trial.get(&thickness_id).expect("thickness should exist");
        match thickness {
            &Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (si_value - 0.005).abs() < 1e-15,
                    "thickness si_value should be 0.005, got {}",
                    si_value
                );
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar for thickness, got {:?}", other),
        }

        // Second auto param: angle with correct dimension
        let angle = trial.get(&angle_id).expect("angle should exist");
        match angle {
            &Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (si_value - 1.2).abs() < 1e-15,
                    "angle si_value should be 1.2, got {}",
                    si_value
                );
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Scalar for angle, got {:?}", other),
        }

        // Non-auto value should be preserved unchanged
        let width = trial.get(&width_id).expect("width should be preserved");
        match width {
            &Value::Scalar { si_value, .. } => {
                assert!(
                    (si_value - 0.080).abs() < 1e-15,
                    "width should remain 0.080, got {}",
                    si_value
                );
            }
            other => panic!("expected Scalar for width, got {:?}", other),
        }
    }

    // ---- verify_uniqueness integration test ----
    // None-branch data logic is tested in isolation by the build_perturbation_anchors
    // unit tests below. This single end-to-end test verifies that warn emission actually
    // fires through verify_uniqueness when params are missing.

    #[test]
    fn verify_uniqueness_aggregates_warn_for_multiple_missing_params() {
        use std::collections::HashMap;

        use reify_core::{Type, ValueCellId};
        use reify_ir::AutoParam;

        let param_x = ValueCellId::new("Part", "x");
        let param_y = ValueCellId::new("Part", "y");
        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![
                AutoParam {
                    id: param_x.clone(),
                    param_type: Type::length(),
                    bounds: Some((0.0, 1.0)),
                    free: false,
                },
                AutoParam {
                    id: param_y.clone(),
                    param_type: Type::length(),
                    bounds: Some((0.0, 1.0)),
                    free: false,
                },
            ],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        // Empty solved_values: both params are missing → both hit the None branch
        let solved_values: HashMap<ValueCellId, reify_ir::Value> = HashMap::new();

        let unique = assert_verify_uniqueness_aggregated_warn(
            &problem,
            &solved_values,
            &["Part.x", "Part.y"],
        );
        assert!(
            !unique,
            "expected verify_uniqueness to return false when both params are missing"
        );
    }

    /// Proves that `verify_uniqueness` takes the early-return path when a param
    /// is missing from `solved_values` — i.e. it does NOT call `solve_core`.
    ///
    /// Observable contract:
    /// - returns false (no change)
    /// - exactly 1 WARN event (the aggregated missing-param warn)
    /// - exactly 0 DEBUG events from `reify_constraints` target
    ///
    /// The DEBUG-count assertion is the key TDD signal: if the early-return is
    /// absent, at least the `"verifying uniqueness via perturbation"` debug event
    /// at solver.rs:818 fires (DEBUG ≥ 1), plus additional debug events from
    /// inside `solve_core`'s no-constraint / no-objective early-return path
    /// (DEBUG ≥ 2).  Zero DEBUG events proves both were skipped.
    #[test]
    fn verify_uniqueness_skips_solve_core_when_param_missing() {
        use std::collections::HashMap;
        use std::sync::atomic::Ordering;

        use reify_core::{Type, ValueCellId};
        use reify_ir::AutoParam;
        use reify_test_support::CountingSubscriberBuilder;

        use super::verify_uniqueness;

        let param_id = ValueCellId::new("Part", "x");
        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: param_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            }],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        // Empty solved_values: param is missing → early-return path should fire
        let solved_values: HashMap<ValueCellId, reify_ir::Value> = HashMap::new();

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .count_level(tracing::Level::DEBUG)
            .target_prefix("reify_constraints")
            .build();

        let warn_count = std::sync::Arc::clone(&counters[&tracing::Level::WARN]);
        let debug_count = std::sync::Arc::clone(&counters[&tracing::Level::DEBUG]);

        let unique = tracing::subscriber::with_default(subscriber, || {
            verify_uniqueness(&problem, &solved_values, None)
        });

        assert!(
            !unique,
            "verify_uniqueness must return false when param is missing from solved_values"
        );

        let warn_n = warn_count.load(Ordering::Acquire);
        assert_eq!(
            warn_n, 1,
            "expected exactly 1 WARN (the aggregated missing-param early-return warn); \
             got {warn_n}"
        );

        let debug_n = debug_count.load(Ordering::Acquire);
        assert_eq!(
            debug_n, 0,
            "expected 0 DEBUG events (early-return skips both the \
             'verifying uniqueness via perturbation' debug and all solve_core debug events); \
             got {debug_n}"
        );
    }

    /// Proves that `verify_uniqueness` takes the early-return path when a param
    /// value is non-numeric (e.g. `Value::Undef`) — i.e. it does NOT call `solve_core`.
    ///
    /// Observable contract:
    /// - returns false (no change)
    /// - exactly 1 WARN event (the aggregated missing-or-non-numeric early-return warn)
    /// - exactly 0 DEBUG events from `reify_constraints` target
    ///
    /// The DEBUG-count assertion is the key TDD signal: if the early-return is
    /// absent, at least the `"verifying uniqueness via perturbation"` debug event
    /// fires (DEBUG ≥ 1), plus additional debug events from inside `solve_core`'s
    /// no-constraint / no-objective early-return path (DEBUG ≥ 2).  Zero DEBUG
    /// events proves both were skipped.
    #[test]
    fn verify_uniqueness_skips_solve_core_when_param_non_numeric() {
        use std::collections::HashMap;
        use std::sync::atomic::Ordering;

        use reify_core::{Type, ValueCellId};
        use reify_ir::{AutoParam, Value};
        use reify_test_support::CountingSubscriberBuilder;

        use super::verify_uniqueness;

        let param_id = ValueCellId::new("Part", "x");
        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: param_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            }],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        // Value::Undef: as_f64() returns None → early-return path should fire
        let mut solved_values: HashMap<ValueCellId, Value> = HashMap::new();
        solved_values.insert(param_id.clone(), Value::Undef);

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .count_level(tracing::Level::DEBUG)
            .target_prefix("reify_constraints")
            .build();

        let warn_count = std::sync::Arc::clone(&counters[&tracing::Level::WARN]);
        let debug_count = std::sync::Arc::clone(&counters[&tracing::Level::DEBUG]);

        let unique = tracing::subscriber::with_default(subscriber, || {
            verify_uniqueness(&problem, &solved_values, None)
        });

        assert!(
            !unique,
            "verify_uniqueness must return false when param value is non-numeric"
        );

        let warn_n = warn_count.load(Ordering::Acquire);
        assert_eq!(
            warn_n, 1,
            "expected exactly 1 WARN (the aggregated missing-or-non-numeric early-return warn); \
             got {warn_n}"
        );

        let debug_n = debug_count.load(Ordering::Acquire);
        assert_eq!(
            debug_n, 0,
            "expected 0 DEBUG events (early-return skips both the \
             'verifying uniqueness via perturbation' debug and all solve_core debug events); \
             got {debug_n}"
        );
    }

    // ---- build_perturbation_anchors unit tests ----

    #[test]
    fn build_perturbation_anchors_valid_f64() {
        use std::collections::HashMap;

        use super::build_perturbation_anchors;

        let (id, params) = test_param();
        let mut solved_values = HashMap::new();
        solved_values.insert(id, scalar(0.25));

        let (perturbed, missing) =
            build_perturbation_anchors(&params, &solved_values, &effective_bounds_box(&params));

        assert!(
            missing.is_empty(),
            "expected no missing params; got {:?}",
            missing
        );
        // Empty `missing` means verify_uniqueness will not emit a WARN for this input.
        // The explicit tracing-silence integration test was removed when end-to-end
        // tracing coverage was consolidated into unit tests; coverage of the no-warn
        // path is now implicit via this assertion (empty missing => no WARN emitted).
        assert_eq!(perturbed.len(), 1);
        // solution 0.25 < mid 0.5 → lo + 0.9*(hi-lo) = 0.0 + 0.9*1.0 = 0.9
        assert!(
            (perturbed[0] - 0.9).abs() < 1e-10,
            "expected perturbed[0] == 0.9, got {}",
            perturbed[0]
        );
    }

    #[test]
    fn build_perturbation_anchors_missing_param() {
        use std::collections::HashMap;

        use super::build_perturbation_anchors;

        let (_id, params) = test_param();
        // Empty map: param is absent → None branch fires, mid is used as fallback
        let solved_values: HashMap<reify_core::ValueCellId, reify_ir::Value> = HashMap::new();

        let (perturbed, missing) =
            build_perturbation_anchors(&params, &solved_values, &effective_bounds_box(&params));

        assert_eq!(missing, vec!["Part.x"], "expected Part.x in missing list");
        assert_eq!(perturbed.len(), 1);
        // fallback is mid = 0.5, which is NOT < mid → upper-half branch: lo + 0.1*(hi-lo) = 0.1
        assert!(
            (perturbed[0] - 0.1).abs() < 1e-10,
            "expected perturbed[0] == 0.1 (midpoint fallback goes to lower side), got {}",
            perturbed[0]
        );
    }

    #[test]
    fn build_perturbation_anchors_non_numeric_undef() {
        use std::collections::HashMap;

        use super::build_perturbation_anchors;

        let (id, params) = test_param();
        let mut solved_values: HashMap<reify_core::ValueCellId, reify_ir::Value> = HashMap::new();
        // Value::Undef: as_f64() returns None → same None-branch as missing
        solved_values.insert(id, reify_ir::Value::Undef);

        let (perturbed, missing) =
            build_perturbation_anchors(&params, &solved_values, &effective_bounds_box(&params));

        assert_eq!(
            missing,
            vec!["Part.x"],
            "Value::Undef should appear in missing list"
        );
        assert_eq!(perturbed.len(), 1);
        // fallback mid = 0.5 (not < 0.5) → lo + 0.1*(hi-lo) = 0.1
        assert!(
            (perturbed[0] - 0.1).abs() < 1e-10,
            "expected perturbed[0] == 0.1 for Undef fallback, got {}",
            perturbed[0]
        );
    }

    #[test]
    fn build_perturbation_anchors_multiple_missing() {
        use std::collections::HashMap;

        use super::build_perturbation_anchors;
        use reify_core::{Type, ValueCellId};
        use reify_ir::AutoParam;

        let param_x = ValueCellId::new("Part", "x");
        let param_y = ValueCellId::new("Part", "y");
        let params = vec![
            AutoParam {
                id: param_x,
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            },
            AutoParam {
                id: param_y,
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            },
        ];
        // Both params absent → both hit the None branch
        let solved_values: HashMap<reify_core::ValueCellId, reify_ir::Value> = HashMap::new();

        let (perturbed, missing) =
            build_perturbation_anchors(&params, &solved_values, &effective_bounds_box(&params));

        assert_eq!(
            missing.len(),
            2,
            "both params should be missing; got {:?}",
            missing
        );
        assert!(
            missing.contains(&"Part.x".to_string()),
            "Part.x should be missing"
        );
        assert!(
            missing.contains(&"Part.y".to_string()),
            "Part.y should be missing"
        );
        assert_eq!(perturbed.len(), 2);
        // Both fall back to mid = 0.5 → lo + 0.1*(hi-lo) = 0.1 each
        assert!(
            (perturbed[0] - 0.1).abs() < 1e-10,
            "expected perturbed[0] == 0.1, got {}",
            perturbed[0]
        );
        assert!(
            (perturbed[1] - 0.1).abs() < 1e-10,
            "expected perturbed[1] == 0.1, got {}",
            perturbed[1]
        );
    }

    #[test]
    fn build_perturbation_anchors_upper_half_solution() {
        use std::collections::HashMap;

        use super::build_perturbation_anchors;

        let (id, params) = test_param();
        let mut solved_values = HashMap::new();
        // 0.75 >= mid 0.5 → upper half → lo + 0.1*(hi-lo) = 0.1 (perturbation to lower side)
        solved_values.insert(id, scalar(0.75));

        let (perturbed, missing) =
            build_perturbation_anchors(&params, &solved_values, &effective_bounds_box(&params));

        assert!(
            missing.is_empty(),
            "expected no missing params; got {:?}",
            missing
        );
        assert_eq!(perturbed.len(), 1);
        assert!(
            (perturbed[0] - 0.1).abs() < 1e-10,
            "expected perturbed[0] == 0.1 (upper-half solution → lower-end perturbation), got {}",
            perturbed[0]
        );
    }

    /// Task #5618 step-7: the reflection box is supplied BY THE CALLER, so the
    /// uniqueness re-solve anchors inside the constraint-derived box.
    ///
    /// `verify_uniqueness` reflects to `lo + 0.9·(hi − lo)` of the param's box.  On a
    /// dimensionless auto with `bounds: None` that box is `effective_bounds` =
    /// `(-1e6, 1e6)`, so the anchor is ~8×10⁵ — nowhere near a `q ∈ [1, 100]`
    /// bracket, and the re-solve has no chance of reconverging on the same answer.
    /// Passing the derived seed box `[1, 100]` puts the anchor at 90.1 instead.
    #[test]
    fn build_perturbation_anchors_uses_caller_supplied_box() {
        use std::collections::HashMap;

        use super::build_perturbation_anchors;

        let q = reify_core::ValueCellId::new("Derive", "q");
        let params = vec![real_auto_param(q.clone())];
        let mut solved_values = HashMap::new();
        solved_values.insert(
            q,
            reify_ir::Value::Scalar {
                si_value: 1.02,
                dimension: reify_core::DimensionVector::DIMENSIONLESS,
            },
        );

        // The CALLER's box, not `effective_bounds(&params[0])` = (-1e6, 1e6).
        let bounds = vec![(1.0, 100.0)];
        let (perturbed, missing) = build_perturbation_anchors(&params, &solved_values, &bounds);

        assert!(
            missing.is_empty(),
            "expected no missing params; got {missing:?}"
        );
        assert_eq!(perturbed.len(), 1);
        // 1.02 < mid 50.5 → lower half → reflect high: 1.0 + 0.9*99.0 = 90.1.
        assert!(
            (perturbed[0] - 90.1).abs() < 1e-9,
            "expected the anchor at 90.1 (inside the supplied box [1, 100]); a value near \
             8e5 means the reflection is still reading the dimensionless default box. got {}",
            perturbed[0]
        );
    }

    #[test]
    fn build_trial_values_empty_params() {
        use super::build_trial_values;
        use reify_core::{DimensionVector, ValueCellId};
        use reify_ir::Value;

        let width_id = ValueCellId::new("Bracket", "width");

        // Base map has one pre-existing value
        let mut base = ValueMap::new();
        base.insert(
            width_id.clone(),
            Value::Scalar {
                si_value: 0.080,
                dimension: DimensionVector::LENGTH,
            },
        );

        // Empty params slice — should return base unchanged
        let trial = build_trial_values(&base, &[], &[], &[], &[], None);

        // Base value preserved
        let width = trial.get(&width_id).expect("width should be preserved");
        match width {
            &Value::Scalar { si_value, .. } => {
                assert!(
                    (si_value - 0.080).abs() < 1e-15,
                    "width should remain 0.080, got {}",
                    si_value
                );
            }
            other => panic!("expected Scalar for width, got {:?}", other),
        }
    }

    #[test]
    fn compute_violation_satisfied_constraint() {
        use super::compute_total_violation;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{BinOp, CompiledExpr, Value};

        // thickness > 2mm, thickness = 5mm → satisfied, violation = 0
        let thickness_ref =
            CompiledExpr::value_ref(ValueCellId::new("Bracket", "thickness"), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let expr = CompiledExpr::binop(BinOp::Gt, thickness_ref, two_mm, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("Bracket", "thickness"),
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
        );

        let constraints = vec![(ConstraintNodeId::new("Bracket", 0), expr)];
        let violation = compute_total_violation(&constraints, &values, &[], None);
        assert!(
            violation.abs() < 1e-15,
            "satisfied constraint should have zero violation, got {}",
            violation
        );
    }

    #[test]
    fn compute_violation_violated_constraint() {
        use super::compute_total_violation;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{BinOp, CompiledExpr, Value};

        // thickness > 2mm, thickness = 1mm → violated
        let thickness_ref =
            CompiledExpr::value_ref(ValueCellId::new("Bracket", "thickness"), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let expr = CompiledExpr::binop(BinOp::Gt, thickness_ref, two_mm, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("Bracket", "thickness"),
            Value::Scalar {
                si_value: 0.001,
                dimension: DimensionVector::LENGTH,
            },
        );

        let constraints = vec![(ConstraintNodeId::new("Bracket", 0), expr)];
        let violation = compute_total_violation(&constraints, &values, &[], None);
        assert!(
            violation > 0.0,
            "violated constraint should have positive violation"
        );
    }

    #[test]
    fn compute_violation_multiple_constraints() {
        use super::compute_total_violation;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{BinOp, CompiledExpr, Value};

        // constraint 1: thickness > 2mm (satisfied, thickness=5mm)
        let thickness_ref =
            CompiledExpr::value_ref(ValueCellId::new("Bracket", "thickness"), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let expr1 = CompiledExpr::binop(BinOp::Gt, thickness_ref, two_mm, Type::Bool);

        // constraint 2: width > 100mm (violated, width=80mm)
        let width_ref =
            CompiledExpr::value_ref(ValueCellId::new("Bracket", "width"), Type::length());
        let hundred_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.100,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let expr2 = CompiledExpr::binop(BinOp::Gt, width_ref, hundred_mm, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("Bracket", "thickness"),
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
        );
        values.insert(
            ValueCellId::new("Bracket", "width"),
            Value::Scalar {
                si_value: 0.080,
                dimension: DimensionVector::LENGTH,
            },
        );

        let constraints = vec![
            (ConstraintNodeId::new("Bracket", 0), expr1),
            (ConstraintNodeId::new("Bracket", 1), expr2),
        ];
        let violation = compute_total_violation(&constraints, &values, &[], None);
        // Only the violated constraint contributes
        assert!(
            violation > 0.0,
            "should have positive violation from width constraint"
        );
    }

    #[test]
    fn empty_problem_returns_solved() {
        use crate::DimensionalSolver;

        let solver = DimensionalSolver;
        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                assert!(
                    values.is_empty(),
                    "empty problem should return empty values"
                );
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    // ---- solutions_agree tests ----

    #[test]
    fn solutions_agree_matching_values_returns_true() {
        use std::collections::HashMap;

        use super::solutions_agree;
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.5));

        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(0.5000001)); // within tolerance

        assert!(
            solutions_agree(&params, &solved, &perturbed),
            "nearly-identical values should be considered agreeing"
        );
    }

    #[test]
    fn solutions_agree_different_values_returns_false() {
        use std::collections::HashMap;

        use super::solutions_agree;
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.1));

        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(0.9));

        assert!(
            !solutions_agree(&params, &solved, &perturbed),
            "significantly different values should not agree"
        );
    }

    // ---- solutions_agree: None/non-numeric handling tests ----
    //
    // These tests originally exercised a bug where `unwrap_or(0.0)` silently
    // substituted 0.0 for missing or non-numeric values. When both sides were
    // None, diff was 0.0 and the function incorrectly returned true (agreed).
    // After the fix landed, these tests now guard against regression — they
    // must continue to return false.

    #[test]
    fn solutions_agree_both_params_missing_returns_false() {
        use std::collections::HashMap;

        use super::solutions_agree;
        use reify_core::ValueCellId;
        use reify_ir::Value;

        let (_param_id, params) = test_param();

        // Both maps are empty — neither contains the param
        let solved: HashMap<ValueCellId, Value> = HashMap::new();
        let perturbed: HashMap<ValueCellId, Value> = HashMap::new();

        assert!(
            !solutions_agree(&params, &solved, &perturbed),
            "both params missing should be non-agreeing (cannot verify uniqueness)"
        );
    }

    #[test]
    fn solutions_agree_original_param_is_undef_returns_false() {
        use std::collections::HashMap;

        use super::solutions_agree;
        use reify_core::ValueCellId;
        use reify_ir::Value;

        let (param_id, params) = test_param();

        // Original solution has Undef for the param.
        // Perturbed has a value very close to zero — the bug: unwrap_or(0.0) on the Undef
        // produces s1=0.0, and s2≈0.0, so diff≈0 and the function incorrectly returns true.
        let mut solved: HashMap<ValueCellId, Value> = HashMap::new();
        solved.insert(param_id.clone(), Value::Undef);

        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(1e-15)); // near zero — exposes the unwrap_or(0.0) bug

        assert!(
            !solutions_agree(&params, &solved, &perturbed),
            "Undef in original solution should be non-agreeing"
        );
    }

    #[test]
    fn solutions_agree_perturbed_param_is_bool_returns_false() {
        use std::collections::HashMap;

        use super::solutions_agree;
        use reify_core::ValueCellId;
        use reify_ir::Value;

        let (param_id, params) = test_param();

        // Original has a value near zero; perturbed has Bool(true) (non-numeric).
        // The bug: unwrap_or(0.0) on Bool(true) → 0.0, and original ≈ 0.0,
        // so diff ≈ 0.0 and the function incorrectly returns true.
        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(1e-15)); // near zero — exposes the unwrap_or(0.0) bug

        // Perturbed solution has a Bool (non-numeric) for the param
        let mut perturbed: HashMap<ValueCellId, Value> = HashMap::new();
        perturbed.insert(param_id.clone(), Value::Bool(true));

        assert!(
            !solutions_agree(&params, &solved, &perturbed),
            "Bool in perturbed solution should be non-agreeing"
        );
    }

    #[test]
    fn solutions_agree_original_missing_perturbed_near_zero_returns_false() {
        use std::collections::HashMap;

        use super::solutions_agree;
        use reify_core::ValueCellId;
        use reify_ir::Value;

        let (param_id, params) = test_param();

        // Original map doesn't contain the param at all
        let solved: HashMap<ValueCellId, Value> = HashMap::new();

        // Perturbed has a value very close to zero (so the old unwrap_or(0.0) bug
        // would produce diff ≈ 0 and incorrectly report agreement)
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(1e-15));

        assert!(
            !solutions_agree(&params, &solved, &perturbed),
            "missing original param should be non-agreeing even when perturbed is near zero"
        );
    }

    // ---- end solutions_agree None/non-numeric tests ----

    // ---- solutions_agree: edge case tests ----

    #[test]
    fn solutions_agree_nan_value_returns_false() {
        use std::collections::HashMap;

        use super::solutions_agree;
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.5));

        // Perturbed has NaN — as_f64() returns Some(NaN), which slips through
        // the None guard; NaN comparisons in the tolerance check are always
        // false, so the function incorrectly returns true without this fix.
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(f64::NAN));

        assert!(
            !solutions_agree(&params, &solved, &perturbed),
            "NaN in perturbed solution should be non-agreeing"
        );
    }

    #[test]
    fn solutions_agree_infinity_value_returns_false() {
        use std::collections::HashMap;

        use super::solutions_agree;
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.5));

        // Perturbed has Infinity — as_f64() returns Some(Inf), which would
        // slip past a None guard; the is_finite() guard rejects it.
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(f64::INFINITY));

        assert!(
            !solutions_agree(&params, &solved, &perturbed),
            "Infinity in perturbed solution should be non-agreeing"
        );
    }

    #[test]
    fn solutions_agree_multi_param_second_diverges_returns_false() {
        use std::collections::HashMap;

        use super::solutions_agree;
        use reify_core::{Type, ValueCellId};
        use reify_ir::AutoParam;

        // Two params: 'x' agrees within tolerance, 'y' diverges sharply.
        // This verifies the for-loop iterates ALL params and does not
        // short-circuit on the first match.
        // The multi-param vec is constructed inline (no helper) — test_param()
        // returns only the canonical single-param shape.
        let param_x = ValueCellId::new("Part", "x");
        let param_y = ValueCellId::new("Part", "y");
        let params = vec![
            AutoParam {
                id: param_x.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            },
            AutoParam {
                id: param_y.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            },
        ];

        // First param ('x') agrees: 0.5 vs 0.5000001 — well within tolerance.
        // Second param ('y') diverges: 0.1 vs 0.9 — should trigger return false.
        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_x.clone(), scalar(0.5));
        solved.insert(param_y.clone(), scalar(0.1));

        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_x.clone(), scalar(0.5000001));
        perturbed.insert(param_y.clone(), scalar(0.9));

        assert!(
            !solutions_agree(&params, &solved, &perturbed),
            "second param divergence should make solutions_agree return false"
        );
    }

    // ---- classify_uniqueness tests (task #5711) ----
    //
    // classify_uniqueness is the pure verdict classifier behind PRD
    // docs/reify-implementation-architecture.md §11.6's two disjunctive
    // well-determinedness tests: parameter comparison answers "uniquely
    // determined by constraints", objective-score comparison answers
    // "uniquely optimal under the applicable objective". The
    // `objective_scores` closure, when invoked, returns `(incumbent,
    // perturbed)` on the pure-minimiser scale where LOWER is better,
    // matching `eval_objective_set`'s convention.

    #[test]
    fn classify_uniqueness_params_agree_with_score_returns_unique() {
        use std::collections::HashMap;

        use super::{UniquenessVerdict, classify_uniqueness};
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.5));
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(0.5000001)); // within tolerance

        // The objective_scores closure must not even be INVOKED once the
        // params already agree — there's nothing to suppress, and the
        // laziness contract (classify_uniqueness's doc, review suggestion 7)
        // is structural, not just documented: panic if it's called.
        let verdict = classify_uniqueness(&params, &solved, &perturbed, || {
            panic!("objective_scores must not be consulted when params already agree")
        });
        assert_eq!(
            verdict,
            UniquenessVerdict::Unique,
            "params agreeing within tolerance must be Unique regardless of objective_scores"
        );
    }

    #[test]
    fn classify_uniqueness_params_agree_without_score_returns_unique() {
        use std::collections::HashMap;

        use super::{UniquenessVerdict, classify_uniqueness};
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.5));
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(0.5000001));

        let verdict = classify_uniqueness(&params, &solved, &perturbed, || None);
        assert_eq!(
            verdict,
            UniquenessVerdict::Unique,
            "params agreeing within tolerance must be Unique even with no objective score"
        );
    }

    #[test]
    fn classify_uniqueness_params_differ_no_objective_returns_non_unique() {
        use std::collections::HashMap;

        use super::{UniquenessVerdict, classify_uniqueness};
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.1));
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(0.9));

        // No effective objective at all (no explicit, no synthesisable) — the
        // ONLY applicable §11.6 test is "uniquely determined by constraints",
        // and the params differ, so NonUnique.
        let verdict = classify_uniqueness(&params, &solved, &perturbed, || None);
        assert_eq!(verdict, UniquenessVerdict::NonUnique);
    }

    #[test]
    fn classify_uniqueness_params_differ_scores_tie_returns_non_unique() {
        use std::collections::HashMap;

        use super::{UniquenessVerdict, classify_uniqueness};
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.1));
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(0.9));

        // Params differ, but the objective ties (flat region) — genuinely
        // NOT uniquely optimal, so NonUnique (the flat-objective /
        // defined_objective_at_fallback_returns_solved mechanism).
        let verdict = classify_uniqueness(&params, &solved, &perturbed, || Some((5.0, 5.0)));
        assert_eq!(verdict, UniquenessVerdict::NonUnique);
    }

    #[test]
    fn classify_uniqueness_params_differ_perturbed_strictly_lower_returns_incumbent_suboptimal()
     {
        use std::collections::HashMap;

        use super::{UniquenessVerdict, classify_uniqueness};
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.1));
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(0.9));

        // Perturbed strictly BETTER (lower, pure-minimiser scale) beyond
        // tolerance: the incumbent was not the argmin, so this is a
        // suboptimality finding, not a non-uniqueness one — suppress.
        let verdict = classify_uniqueness(&params, &solved, &perturbed, || Some((10.0, 5.0)));
        // Asserts the carried evidence too (review suggestion 8), not just
        // the variant discriminant.
        assert_eq!(
            verdict,
            UniquenessVerdict::IncumbentSuboptimal {
                incumbent: 10.0,
                perturbed: 5.0
            }
        );
    }

    #[test]
    fn classify_uniqueness_params_differ_perturbed_strictly_higher_returns_non_unique() {
        use std::collections::HashMap;

        use super::{UniquenessVerdict, classify_uniqueness};
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.1));
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(0.9));

        // Perturbed strictly WORSE beyond tolerance: logically inconclusive
        // (the re-solve may have simply stalled at a worse point), but
        // DELIBERATELY kept at today's NonUnique verdict — see
        // classify_uniqueness's doc comment for why. Pinned here so a later
        // reader cannot mistake this for an oversight.
        let verdict = classify_uniqueness(&params, &solved, &perturbed, || Some((5.0, 10.0)));
        assert_eq!(verdict, UniquenessVerdict::NonUnique);
    }

    #[test]
    fn classify_uniqueness_large_magnitude_tie_uses_relative_tolerance() {
        use std::collections::HashMap;

        use super::{UniquenessVerdict, classify_uniqueness};
        use reify_core::ValueCellId;

        let (param_id, params) = test_param();

        let mut solved: HashMap<ValueCellId, _> = HashMap::new();
        solved.insert(param_id.clone(), scalar(0.1));
        let mut perturbed: HashMap<ValueCellId, _> = HashMap::new();
        perturbed.insert(param_id.clone(), scalar(0.9));

        // 1e8-scale pair at the `defined_objective_at_fallback_returns_solved`
        // magnitude: incumbent 1e8, perturbed 1e8 - 50.0. An ABSOLUTE-only
        // tolerance (UNIQUENESS_ABS_TOL = 1e-10) would see a diff of 50 as
        // "different" and misclassify this as IncumbentSuboptimal (perturbed
        // is numerically lower). The RELATIVE arm (UNIQUENESS_REL_TOL * scale
        // = 1e-6 * 1e8 = 100) correctly treats a 50-unit difference at this
        // magnitude as a tie, so the verdict must be NonUnique.
        let verdict =
            classify_uniqueness(&params, &solved, &perturbed, || Some((1e8, 1e8 - 50.0)));
        assert_eq!(
            verdict,
            UniquenessVerdict::NonUnique,
            "a 50-unit diff at 1e8 scale is within the RELATIVE tolerance arm and must tie"
        );
    }

    #[test]
    fn classify_uniqueness_params_missing_returns_non_unique() {
        use std::collections::HashMap;

        use super::{UniquenessVerdict, classify_uniqueness};
        use reify_core::ValueCellId;
        use reify_ir::Value;

        let (_param_id, params) = test_param();

        // Both maps empty — the param is missing from both. solutions_agree
        // already treats this as "loud, not silent" (returns false rather
        // than defaulting to 0.0); classify_uniqueness must preserve that —
        // a missing/non-numeric value is never grounds to report Unique.
        let solved: HashMap<ValueCellId, Value> = HashMap::new();
        let perturbed: HashMap<ValueCellId, Value> = HashMap::new();

        let verdict = classify_uniqueness(&params, &solved, &perturbed, || None);
        assert_eq!(verdict, UniquenessVerdict::NonUnique);
    }

    // ---- end classify_uniqueness tests ----

    // ---- strict_autos_constraint_bracketed tests (task #5711, amendment 2) ----
    //
    // `strict_autos_constraint_bracketed` is the pure predicate behind the γ
    // (`cost_robustness_tradeoff`) branch of `verify_uniqueness`. The
    // perturbation machinery is STRUCTURALLY INAPPLICABLE on that path —
    // `solve_cost_robustness_tradeoff` is seed-dependent by construction, so a
    // perturbation check compares f(seed_A) against f(seed_B) for a
    // seed-dependent f — but PRD
    // docs/reify-implementation-architecture.md §11.6 still needs an
    // answer. Test (2) ("uniquely optimal under the applicable objective") is
    // answered WITHOUT any solve: if every strict auto's interval is bounded on
    // BOTH sides by the user's own constraints, the blend's argmin is fixed by
    // the user's model and the value is well-determined; if a side is missing,
    // that side comes from `default_bounds_for` — a solver-internal default the
    // user never authored — so the resolved value is default-bounds-determined,
    // which is genuine non-determinedness.
    //
    // The predicate is PURE: no solve, no I/O, no mutation. These fixtures
    // therefore build `DerivedInterval` values directly rather than routing
    // through `derive_param_intervals`.

    /// A strict (`free: false`) auto param named `Part::<name>` with the
    /// PRODUCTION `bounds: None` shape (solver.rs records that no `.ri` surface
    /// ever sets `AutoParam.bounds`).
    fn bracketed_test_param(name: &str, free: bool) -> reify_ir::AutoParam {
        use reify_core::{Type, ValueCellId};
        use reify_ir::AutoParam;
        AutoParam {
            id: ValueCellId::new("Part", name),
            param_type: Type::length(),
            bounds: None,
            free,
        }
    }

    #[test]
    fn strict_autos_constraint_bracketed_two_sided_returns_true() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        let params = vec![bracketed_test_param("t", false)];
        // `1mm < t < 4mm` — both sides supplied by the user's constraints.
        let mut iv = DerivedInterval::default();
        iv.push_lo(0.001, true);
        iv.push_hi(0.004, true);

        assert!(
            strict_autos_constraint_bracketed(&params, &[iv], &HashSet::new()),
            "a strict auto bracketed on BOTH sides is constraint-determined"
        );
    }

    #[test]
    fn strict_autos_constraint_bracketed_missing_hi_returns_false() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        let params = vec![bracketed_test_param("t", false)];
        // The one-sided `t > 1mm` shape (tests/prd-gate/fixtures/
        // cost_robustness_tradeoff_form.ri): the upper side would come from
        // `default_bounds_for(Length)`, not from the model.
        let mut iv = DerivedInterval::default();
        iv.push_lo(0.001, true);

        assert!(
            !strict_autos_constraint_bracketed(&params, &[iv], &HashSet::new()),
            "a missing upper side means the value is default-bounds-determined, not \
             model-determined"
        );
    }

    #[test]
    fn strict_autos_constraint_bracketed_missing_lo_returns_false() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        let params = vec![bracketed_test_param("t", false)];
        let mut iv = DerivedInterval::default();
        iv.push_hi(0.004, true);

        assert!(
            !strict_autos_constraint_bracketed(&params, &[iv], &HashSet::new()),
            "a missing lower side is symmetric with a missing upper side"
        );
    }

    #[test]
    fn strict_autos_constraint_bracketed_unbounded_returns_false() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        let params = vec![bracketed_test_param("t", false)];

        assert!(
            !strict_autos_constraint_bracketed(
                &params,
                &[DerivedInterval::default()],
                &HashSet::new()
            ),
            "a strict auto with NEITHER side constrained is entirely default-bounds-determined"
        );
    }

    #[test]
    fn strict_autos_constraint_bracketed_free_params_are_exempt() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        // A bracketed STRICT param alongside an entirely unbracketed FREE one.
        let params = vec![
            bracketed_test_param("t", false),
            bracketed_test_param("u", true),
        ];
        let mut bracketed = DerivedInterval::default();
        bracketed.push_lo(0.001, true);
        bracketed.push_hi(0.004, true);

        assert!(
            strict_autos_constraint_bracketed(
                &params,
                &[bracketed, DerivedInterval::default()],
                &HashSet::new()
            ),
            "free params carry no §11.6 obligation (finalise_uniqueness only calls \
             verify_uniqueness when at least one param is strict), so an unbracketed free \
             param must not veto the verdict"
        );
    }

    #[test]
    fn strict_autos_constraint_bracketed_no_strict_params_is_vacuously_true() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        let params = vec![
            bracketed_test_param("t", true),
            bracketed_test_param("u", true),
        ];

        assert!(
            strict_autos_constraint_bracketed(
                &params,
                &[DerivedInterval::default(), DerivedInterval::default()],
                &HashSet::new()
            ),
            "with no strict params the §11.6 obligation is vacuous and the predicate holds"
        );
    }

    #[test]
    fn strict_autos_constraint_bracketed_index_beyond_intervals_returns_false() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        // Two params, ONE interval — a length mismatch is a bug in the caller.
        let params = vec![
            bracketed_test_param("t", false),
            bracketed_test_param("u", false),
        ];
        let mut iv = DerivedInterval::default();
        iv.push_lo(0.001, true);
        iv.push_hi(0.004, true);

        assert!(
            !strict_autos_constraint_bracketed(&params, &[iv], &HashSet::new()),
            "a strict param with no corresponding interval must read as NOT bracketed — \
             preserving solutions_agree's loud-not-silent contract rather than silently \
             defaulting to 'bracketed'"
        );
    }

    /// The PRECEDENCE between the two "no positive bracketing evidence" inputs:
    /// a param that is BOTH beyond the `intervals` slice AND in the abstention
    /// set reads as bracketed, because `underivable.contains(&i)` short-circuits
    /// before the `intervals.get(i)` lookup. Pins the half of
    /// `strict_autos_constraint_bracketed`'s doc that the missing-entry test
    /// above does not reach — the two together are the whole contract.
    #[test]
    fn strict_autos_constraint_bracketed_abstention_outranks_missing_interval() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        // Two params, ONE interval: index 1 has no entry at all.
        let params = vec![
            bracketed_test_param("t", false),
            bracketed_test_param("u", false),
        ];
        let mut iv = DerivedInterval::default();
        iv.push_lo(0.001, true);
        iv.push_hi(0.004, true);

        assert!(
            strict_autos_constraint_bracketed(&params, &[iv], &HashSet::from([1])),
            "abstention must outrank a missing `intervals` entry — the `underivable` \
             check short-circuits before the `intervals.get(i)` lookup"
        );
        assert!(
            !strict_autos_constraint_bracketed(&params, &[iv], &HashSet::new()),
            "without that abstention the SAME missing entry must read as NOT bracketed \
             (the loud-not-silent half)"
        );
    }

    #[test]
    fn strict_autos_constraint_bracketed_strict_bounds_still_count() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        let params = vec![bracketed_test_param("t", false)];
        // BOTH sides strict (`>` / `<`) — mirroring `derived_seed_box`'s
        // `include_strict = true`. The question this predicate answers is "did
        // the USER's constraints supply this side", NOT "is it a legal clamp
        // target", so bound strictness is irrelevant.
        let strict_both = DerivedInterval {
            lo: Some((0.001, true)),
            hi: Some((0.004, true)),
        };
        // …and the non-strict (`>=` / `<=`) pair must agree.
        let non_strict_both = DerivedInterval {
            lo: Some((0.001, false)),
            hi: Some((0.004, false)),
        };

        assert!(
            strict_autos_constraint_bracketed(&params, &[strict_both], &HashSet::new()),
            "a strict (`>`/`<`) bound still SUPPLIES that side"
        );
        assert!(
            strict_autos_constraint_bracketed(&params, &[non_strict_both], &HashSet::new()),
            "a non-strict (`>=`/`<=`) bound must give the same verdict as a strict one"
        );
    }

    // ---- abstention on an UNREADABLE constraint (esc-5711-3) ----
    //
    // `derive_param_intervals` recognises only three syntactic shapes with a
    // constant far operand, so `Eq`, coefficient/nonlinear and coupled bounds
    // all derive to `None`. Those `None`s are derivation BLIND SPOTS, and
    // reading one as "the user did not bound this side" turns a valid, bounded
    // γ model into `error: strict auto parameter resolution is not uniquely
    // determined`. `params_in_underivable_constraints` is the evidence source
    // that keeps the predicate's `false` reserved for params POSITIVELY
    // confirmed unbounded; the integration-level counterparts live in
    // `tests/cost_robustness_tradeoff_blend.rs`.

    /// A readable one-sided bound flags NOTHING: `q >= 1.0` is exactly the
    /// shape `derive_from_side` handles, so the missing upper side really is
    /// `default_bounds_for`'s and must keep reporting non-determinedness.
    #[test]
    fn params_in_underivable_constraints_readable_bound_flags_nothing() {
        use reify_ir::BinOp;

        let q = reify_core::ValueCellId::new("Derive", "q");
        let params = vec![real_auto_param(q.clone())];
        let constraints = as_constraints(vec![cmp_ref_lit(BinOp::Ge, &q, 1.0)]);

        assert!(
            super::params_in_underivable_constraints(
                &params,
                &constraints,
                &ValueMap::new(),
                &[],
                None,
            )
            .is_empty(),
            "a bound the derivation CAN read is positive evidence, not a blind spot"
        );
    }

    /// `constraint q == 5.0` — skipped outright by `derive_from_expr`'s op rule,
    /// yet the canonical DSL way to determine a strict auto
    /// (`examples/auto_binding_sites.ri`). Must be flagged.
    #[test]
    fn params_in_underivable_constraints_flags_eq() {
        use reify_ir::BinOp;

        let q = reify_core::ValueCellId::new("Derive", "q");
        let params = vec![real_auto_param(q.clone())];
        let constraints = as_constraints(vec![cmp_ref_lit(BinOp::Eq, &q, 5.0)]);

        assert_eq!(
            super::params_in_underivable_constraints(
                &params,
                &constraints,
                &ValueMap::new(),
                &[],
                None,
            ),
            std::collections::HashSet::from([0]),
            "`Eq` determines the param but derives no interval — a blind spot, not an \
             unbounded side"
        );
    }

    /// A COUPLED bound (`y < 5 - x`) has a far operand naming another auto, so
    /// `constant_operand_value` rejects it and NEITHER param gets a bound —
    /// both must be flagged.
    #[test]
    fn params_in_underivable_constraints_flags_coupled_pair() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr};

        let x = reify_core::ValueCellId::new("Derive", "x");
        let y = reify_core::ValueCellId::new("Derive", "y");
        let params = vec![real_auto_param(x.clone()), real_auto_param(y.clone())];
        // `y < 5 - x`
        let rhs = CompiledExpr::binop(
            BinOp::Sub,
            real_lit(5.0),
            real_ref(&x),
            Type::dimensionless_scalar(),
        );
        let coupled = CompiledExpr::binop(BinOp::Lt, real_ref(&y), rhs, Type::Bool);
        let constraints = as_constraints(vec![coupled]);

        assert_eq!(
            super::params_in_underivable_constraints(
                &params,
                &constraints,
                &ValueMap::new(),
                &[],
                None,
            ),
            std::collections::HashSet::from([0, 1]),
            "a coupled bound is unreadable for BOTH the near and the far param"
        );
    }

    /// A COEFFICIENT bound (`2*q > 3`) matches none of the three shapes.
    #[test]
    fn params_in_underivable_constraints_flags_coefficient_form() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr};

        let q = reify_core::ValueCellId::new("Derive", "q");
        let params = vec![real_auto_param(q.clone())];
        let scaled = CompiledExpr::binop(
            BinOp::Mul,
            real_lit(2.0),
            real_ref(&q),
            Type::dimensionless_scalar(),
        );
        let constraints = as_constraints(vec![CompiledExpr::binop(
            BinOp::Gt,
            scaled,
            real_lit(3.0),
            Type::Bool,
        )]);

        assert_eq!(
            super::params_in_underivable_constraints(
                &params,
                &constraints,
                &ValueMap::new(),
                &[],
                None,
            ),
            std::collections::HashSet::from([0]),
            "`2*q > 3` bounds q at 1.5 — the derivation just cannot read it"
        );
    }

    /// Conjuncts are scored SEPARATELY: an `And` of two readable bounds flags
    /// nothing, and mixing in an unreadable conjunct flags only what that
    /// conjunct mentions opaquely.
    /// The shared `And` walk both derivation consumers drive (review suggestion
    /// 3). Pinned directly, not only through its two callers: it is now the
    /// ONLY structural recursion in the family, so its contract — nested `And`
    /// flattens left-to-right, anything else is its own single leaf, and no
    /// `And` node is ever handed to the leaf callback — is what keeps
    /// `derive_param_intervals` and `params_in_underivable_constraints` from
    /// disagreeing about what a leaf is.
    #[test]
    fn for_each_leaf_conjunct_flattens_nested_ands_and_yields_non_and_verbatim() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr};

        let q = reify_core::ValueCellId::new("Derive", "q");

        // A non-`And` expression is its own single leaf.
        let leaf = cmp_ref_lit(BinOp::Ge, &q, 1.0);
        let mut seen = 0usize;
        super::for_each_leaf_conjunct(&leaf, &mut |_| seen += 1);
        assert_eq!(seen, 1, "a non-`And` expression is its own single leaf");

        // `((a AND b) AND c)` → three leaves, left to right, no `And` among them.
        let a = cmp_ref_lit(BinOp::Ge, &q, 1.0);
        let b = cmp_ref_lit(BinOp::Le, &q, 4.0);
        let c = cmp_ref_lit(BinOp::Eq, &q, 2.0);
        let nested = CompiledExpr::binop(
            BinOp::And,
            CompiledExpr::binop(BinOp::And, a, b, Type::Bool),
            c,
            Type::Bool,
        );

        let mut ops = Vec::new();
        super::for_each_leaf_conjunct(&nested, &mut |leaf| {
            if let reify_ir::CompiledExprKind::BinOp { op, .. } = &leaf.kind {
                ops.push(*op);
            } else {
                panic!("every leaf here is a BinOp comparison");
            }
        });
        assert_eq!(
            ops,
            vec![BinOp::Ge, BinOp::Le, BinOp::Eq],
            "nested `And`s must flatten left-to-right, and no `And` node may reach the \
             leaf callback — `derive_from_expr` no longer recurses, so an `And` that \
             leaked through would derive nothing at all"
        );
    }

    #[test]
    fn params_in_underivable_constraints_splits_conjunctions() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr};

        let q = reify_core::ValueCellId::new("Derive", "q");
        let params = vec![real_auto_param(q.clone())];
        let both_readable = CompiledExpr::binop(
            BinOp::And,
            cmp_ref_lit(BinOp::Ge, &q, 1.0),
            cmp_ref_lit(BinOp::Le, &q, 4.0),
            Type::Bool,
        );

        assert!(
            super::params_in_underivable_constraints(
                &params,
                &as_constraints(vec![both_readable]),
                &ValueMap::new(),
                &[],
                None,
            )
            .is_empty(),
            "`And` must be split by `for_each_leaf_conjunct` — the same walk \
             `derive_param_intervals` drives — not treated as one opaque leaf"
        );

        let mixed = CompiledExpr::binop(
            BinOp::And,
            cmp_ref_lit(BinOp::Ge, &q, 1.0),
            cmp_ref_lit(BinOp::Eq, &q, 5.0),
            Type::Bool,
        );

        assert_eq!(
            super::params_in_underivable_constraints(
                &params,
                &as_constraints(vec![mixed]),
                &ValueMap::new(),
                &[],
                None,
            ),
            std::collections::HashSet::from([0]),
            "a readable conjunct must not launder an unreadable sibling that mentions the \
             same param"
        );
    }

    /// A NON-ENUMERATED shape: `Or`. The doc's four named shapes (Eq /
    /// coefficient / nonlinear / coupled) are EXAMPLES, not a taxonomy — the
    /// rule is "any conjunct that mentions the param and yields no bound". `Or`
    /// falls into `derive_from_expr`'s same `_ => {}` arm as `Eq` and is NOT
    /// split like `And`, so `q >= 1 OR q <= 4` bounds nothing while mentioning
    /// `q`. Pins the true reach of the abstention rather than leaving it
    /// inferred from the enumeration (review, robustness).
    #[test]
    fn params_in_underivable_constraints_flags_or_disjunction() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr};

        let q = reify_core::ValueCellId::new("Derive", "q");
        let params = vec![real_auto_param(q.clone())];
        let disjunction = CompiledExpr::binop(
            BinOp::Or,
            cmp_ref_lit(BinOp::Ge, &q, 1.0),
            cmp_ref_lit(BinOp::Le, &q, 4.0),
            Type::Bool,
        );

        assert_eq!(
            super::params_in_underivable_constraints(
                &params,
                &as_constraints(vec![disjunction]),
                &ValueMap::new(),
                &[],
                None,
            ),
            std::collections::HashSet::from([0]),
            "`Or` is skipped by the same op rule as `Eq` and must NOT be split like `And`: \
             the disjunction bounds nothing, so it is a blind spot for `q`"
        );
    }

    /// A NON-ENUMERATED shape that is also the realistic one: a DISPATCH-BACKED
    /// predicate, `stress(t) < LIMIT` (this file's own `fea_binding_problem`,
    /// the FEA fixture from #4880). Unreadable on BOTH sides — `derive_from_side`
    /// cannot see a `Call` on the near side, and `constant_operand_value` rejects
    /// a far side naming the auto — so `t` is flagged even though the constraint
    /// is the model's whole point. Asserted WITH a live dispatch attached, since
    /// the derivation threads one through and a reader could otherwise assume the
    /// hook rescues the shape.
    #[test]
    fn params_in_underivable_constraints_flags_dispatch_backed_predicate() {
        use std::sync::atomic::AtomicUsize;

        let (_t_id, problem) = fea_binding_problem();
        let mock = CountingDispatch {
            calls: AtomicUsize::new(0),
            k: 1.0,
        };

        assert_eq!(
            super::params_in_underivable_constraints(
                &problem.auto_params,
                &problem.constraints,
                &problem.current_values,
                &problem.functions,
                Some(&mock),
            ),
            std::collections::HashSet::from([0]),
            "`stress(t) < LIMIT` mentions `t` and bounds neither side, so it abstains — \
             a dispatch hook does not make the Call-shaped near side readable"
        );
    }

    /// The predicate ABSTAINS (reads as bracketed) for a strict param whose
    /// missing side is attributable to an unreadable constraint — and only
    /// then. Same fixture, empty evidence set ⇒ still `false`, which is what
    /// keeps `gamma_strict_auto_one_sided_stays_non_unique` green.
    #[test]
    fn strict_autos_constraint_bracketed_abstains_for_underivable_param() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        let params = vec![bracketed_test_param("t", false)];
        // Lower side readable (`t > 1mm`); upper side opaque.
        let mut iv = DerivedInterval::default();
        iv.push_lo(0.001, true);

        assert!(
            strict_autos_constraint_bracketed(&params, &[iv], &HashSet::from([0])),
            "a missing side traceable to a constraint the derivation could not READ must \
             abstain, not report non-determinedness"
        );
        assert!(
            !strict_autos_constraint_bracketed(&params, &[iv], &HashSet::new()),
            "with no unreadable-constraint evidence the SAME interval must still report \
             default-bounds-determined"
        );
    }

    /// Abstention is per-param and keyed on a MISSING SIDE, not on "no interval
    /// data at all": one abstaining param must not excuse a sibling the
    /// derivation positively confirms is one-sided.
    #[test]
    fn strict_autos_constraint_bracketed_abstention_does_not_leak_across_params() {
        use std::collections::HashSet;

        use super::{DerivedInterval, strict_autos_constraint_bracketed};

        let params = vec![
            bracketed_test_param("t", false),
            bracketed_test_param("u", false),
        ];
        let mut one_sided = DerivedInterval::default();
        one_sided.push_lo(0.001, true);

        assert!(
            !strict_autos_constraint_bracketed(
                &params,
                &[one_sided, one_sided],
                &HashSet::from([0]),
            ),
            "param 1 has no unreadable-constraint evidence, so the verdict must stay false"
        );
    }

    // ---- end strict_autos_constraint_bracketed tests ----

    #[test]
    fn single_param_feasibility() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let solver = DimensionalSolver;
        let thickness_id = ValueCellId::new("Bracket", "thickness");

        // thickness > 2mm
        let thickness_ref = CompiledExpr::value_ref(thickness_id.clone(), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_expr = CompiledExpr::binop(BinOp::Gt, thickness_ref.clone(), two_mm, Type::Bool);

        // thickness < 20mm
        let twenty_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.020,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let lt_expr = CompiledExpr::binop(BinOp::Lt, thickness_ref, twenty_mm, Type::Bool);

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: thickness_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            }],
            constraints: vec![
                (ConstraintNodeId::new("Bracket", 0), gt_expr),
                (ConstraintNodeId::new("Bracket", 1), lt_expr),
            ],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let thickness = values
                    .get(&thickness_id)
                    .expect("thickness should be in solution");
                let si = thickness.as_f64().expect("should be numeric");
                assert!(
                    si > 0.002 && si < 0.020,
                    "thickness should be between 2mm and 20mm, got {} m",
                    si
                );
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    #[test]
    fn infeasible_constraints() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let solver = DimensionalSolver;
        let x_id = ValueCellId::new("Part", "x");

        // x > 10mm
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let ten_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.010,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_expr = CompiledExpr::binop(BinOp::Gt, x_ref.clone(), ten_mm, Type::Bool);

        // x < 5mm — contradicts x > 10mm
        let five_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let lt_expr = CompiledExpr::binop(BinOp::Lt, x_ref, five_mm, Type::Bool);

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            }],
            constraints: vec![
                (ConstraintNodeId::new("Part", 0), gt_expr),
                (ConstraintNodeId::new("Part", 1), lt_expr),
            ],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Infeasible { diagnostics } => {
                assert!(
                    !diagnostics.is_empty(),
                    "infeasible result should have diagnostics"
                );
            }
            other => panic!("expected Infeasible, got {:?}", other),
        }
    }

    #[test]
    fn minimize_objective() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, ObjectiveSense, ObjectiveSet, Value};

        let solver = DimensionalSolver;
        let thickness_id = ValueCellId::new("Bracket", "thickness");

        // thickness >= 2mm (Ge allows equality at boundary, which is where
        // the optimizer converges when minimizing against a constraint)
        let thickness_ref = CompiledExpr::value_ref(thickness_id.clone(), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let ge_expr = CompiledExpr::binop(BinOp::Ge, thickness_ref.clone(), two_mm, Type::Bool);

        // thickness < 20mm
        let twenty_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.020,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let lt_expr = CompiledExpr::binop(BinOp::Lt, thickness_ref.clone(), twenty_mm, Type::Bool);

        // Minimize thickness
        let objective = ObjectiveSet::single(ObjectiveSense::Minimize, thickness_ref);

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: thickness_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            }],
            constraints: vec![
                (ConstraintNodeId::new("Bracket", 0), ge_expr),
                (ConstraintNodeId::new("Bracket", 1), lt_expr),
            ],
            current_values: ValueMap::new(),
            objective: Some(objective),
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let thickness = values
                    .get(&thickness_id)
                    .expect("thickness should be in solution");
                let si = thickness.as_f64().expect("should be numeric");
                // Minimizing thickness subject to >= 2mm should push close to 2mm
                assert!(
                    si > 0.0019 && si < 0.003,
                    "minimized thickness should be close to 2mm, got {} m",
                    si
                );
            }
            SolveResult::Infeasible { .. } => {
                // Nelder-Mead penalty method may converge to a point
                // infinitesimally below the constraint boundary. With L1
                // feasibility check, this is correctly flagged as Infeasible.
                // This is acceptable for optimization-against-boundary.
            }
            other => panic!("expected Solved or Infeasible, got {:?}", other),
        }
    }

    // ---- eval_objective_set I-UNITS coherence backstop (task 5018, step-7 RED / step-8 GREEN) ----

    /// Pins the fold-site `debug_assert!` backstop at the canonical site
    /// (`eval_objective_set`): a `WeightedSum` `ObjectiveSet` whose terms mix
    /// `Money` and `Mass` dimensions is rejected at compile time by
    /// `check_objective_dimension_coherence` (E_OBJECTIVE_MIXED_DIMENSION,
    /// `reify-compiler/src/entity.rs`), so a coherent set is the only kind that
    /// should ever reach this fold. This test simulates a set that reached the
    /// fold ungated (e.g. hand-built, bypassing the compile gate) and pins that
    /// `eval_objective_set` panics via `debug_assert!` rather than silently
    /// folding incommensurable dimensions into a bare f64.
    ///
    /// # Release-build note
    ///
    /// The backstop is a `debug_assert!`, which is compiled out in release
    /// builds, so `eval_objective_set` would silently accept the incoherent
    /// set without panicking. The `#[cfg(debug_assertions)]` gate prevents
    /// this test from incorrectly failing under `#[should_panic]` when run
    /// in release mode (e.g. `cargo test --release`, as exercised by the
    /// merge-queue's `--profile both` verify gate).
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "objective_terms_coherent")]
    fn eval_objective_set_panics_on_incoherent_dimensions() {
        use super::eval_objective_set;
        use reify_core::{DimensionVector, Type};
        use reify_ir::{
            CompiledExpr, ObjectiveCombination, ObjectiveSense, ObjectiveSet, ObjectiveTerm, Value,
        };

        let money_term = ObjectiveTerm::new(
            ObjectiveSense::Minimize,
            CompiledExpr::literal(
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::MONEY,
                },
                Type::Scalar {
                    dimension: DimensionVector::MONEY,
                },
            ),
        );
        let mass_term = ObjectiveTerm::new(
            ObjectiveSense::Minimize,
            CompiledExpr::literal(
                Value::Scalar {
                    si_value: 2.0,
                    dimension: DimensionVector::MASS,
                },
                Type::Scalar {
                    dimension: DimensionVector::MASS,
                },
            ),
        );
        let incoherent = ObjectiveSet {
            terms: vec![money_term, mass_term],
            combination: ObjectiveCombination::WeightedSum,
            cost_robustness_lambda: None,
        };

        let _ = eval_objective_set(&incoherent, &ValueMap::new(), &[], None);
    }

    #[test]
    fn multi_param_solving() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let solver = DimensionalSolver;
        let width_id = ValueCellId::new("Part", "width");
        let height_id = ValueCellId::new("Part", "height");

        let width_ref = CompiledExpr::value_ref(width_id.clone(), Type::length());
        let height_ref = CompiledExpr::value_ref(height_id.clone(), Type::length());

        // width > 50mm
        let fifty_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.050,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_width =
            CompiledExpr::binop(BinOp::Gt, width_ref.clone(), fifty_mm.clone(), Type::Bool);

        // height > 50mm
        let gt_height = CompiledExpr::binop(BinOp::Gt, height_ref.clone(), fifty_mm, Type::Bool);

        // width + height < 200mm
        let sum = CompiledExpr::binop(BinOp::Add, width_ref, height_ref, Type::length());
        let two_hundred_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.200,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let lt_sum = CompiledExpr::binop(BinOp::Lt, sum, two_hundred_mm, Type::Bool);

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![
                AutoParam {
                    id: width_id.clone(),
                    param_type: Type::length(),
                    bounds: Some((0.01, 1.0)),
                    free: true,
                },
                AutoParam {
                    id: height_id.clone(),
                    param_type: Type::length(),
                    bounds: Some((0.01, 1.0)),
                    free: true,
                },
            ],
            constraints: vec![
                (ConstraintNodeId::new("Part", 0), gt_width),
                (ConstraintNodeId::new("Part", 1), gt_height),
                (ConstraintNodeId::new("Part", 2), lt_sum),
            ],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let w = values
                    .get(&width_id)
                    .expect("width should be in solution")
                    .as_f64()
                    .unwrap();
                let h = values
                    .get(&height_id)
                    .expect("height should be in solution")
                    .as_f64()
                    .unwrap();

                assert!(w > 0.05, "width should be > 50mm, got {} m", w);
                assert!(h > 0.05, "height should be > 50mm, got {} m", h);
                assert!(
                    w + h < 0.2,
                    "width + height should be < 200mm, got {} m",
                    w + h
                );
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    #[test]
    fn solution_stays_within_bounds() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let solver = DimensionalSolver;
        let x_id = ValueCellId::new("Part", "x");

        // x > 5mm
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let five_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_expr = CompiledExpr::binop(BinOp::Gt, x_ref, five_mm, Type::Bool);

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.050)), // bounds: 1mm to 50mm
                free: true,
            }],
            constraints: vec![(ConstraintNodeId::new("Part", 0), gt_expr)],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let x = values.get(&x_id).unwrap().as_f64().unwrap();
                assert!(
                    (0.001..=0.050).contains(&x),
                    "solution should be within bounds [1mm, 50mm], got {} m",
                    x
                );
                assert!(x > 0.005, "x should satisfy x > 5mm, got {} m", x);
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    #[test]
    fn no_bounds_length_param() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let solver = DimensionalSolver;
        let x_id = ValueCellId::new("Part", "x");

        // x > 5mm
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let five_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_expr = CompiledExpr::binop(BinOp::Gt, x_ref.clone(), five_mm, Type::Bool);

        // x < 50mm
        let fifty_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.050,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let lt_expr = CompiledExpr::binop(BinOp::Lt, x_ref, fifty_mm, Type::Bool);

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None, // No explicit bounds
                free: true,
            }],
            constraints: vec![
                (ConstraintNodeId::new("Part", 0), gt_expr),
                (ConstraintNodeId::new("Part", 1), lt_expr),
            ],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let x = values.get(&x_id).unwrap().as_f64().unwrap();
                assert!(
                    x > 0.005 && x < 0.050,
                    "should find feasible point, got {} m",
                    x
                );
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    #[test]
    fn comparison_residual_gt_violated_small() {
        use super::comparison_residual;
        use reify_core::{DimensionVector, Type};
        use reify_ir::{BinOp, CompiledExpr, Value};

        // l=1.9999999, r=2.0: violated by 1e-7
        let l_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1.9999999,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let r_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let values = ValueMap::new();
        let res = comparison_residual(BinOp::Gt, &l_expr, &r_expr, &values, &[], None);
        assert!(
            (res - 1e-7).abs() < 1e-12,
            "Gt violated by 1e-7 should have residual ~1e-7, got {:.2e}",
            res
        );
    }

    #[test]
    fn comparison_residual_ge_satisfied() {
        use super::comparison_residual;
        use reify_core::{DimensionVector, Type};
        use reify_ir::{BinOp, CompiledExpr, Value};

        let l_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let r_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let values = ValueMap::new();
        let res = comparison_residual(BinOp::Ge, &l_expr, &r_expr, &values, &[], None);
        assert_eq!(res, 0.0, "Ge with l==r should be satisfied (residual=0)");
    }

    #[test]
    fn comparison_residual_lt_violated() {
        use super::comparison_residual;
        use reify_core::{DimensionVector, Type};
        use reify_ir::{BinOp, CompiledExpr, Value};

        // l=0.010, r=0.005: Lt violated by 0.005
        let l_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.010,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let r_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let values = ValueMap::new();
        let res = comparison_residual(BinOp::Lt, &l_expr, &r_expr, &values, &[], None);
        assert!(
            (res - 0.005).abs() < 1e-15,
            "Lt violated by 0.005 should have residual 0.005, got {}",
            res
        );
    }

    #[test]
    fn comparison_residual_le_satisfied() {
        use super::comparison_residual;
        use reify_core::{DimensionVector, Type};
        use reify_ir::{BinOp, CompiledExpr, Value};

        let l_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.003,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let r_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let values = ValueMap::new();
        let res = comparison_residual(BinOp::Le, &l_expr, &r_expr, &values, &[], None);
        assert_eq!(res, 0.0, "Le with l<r should be satisfied");
    }

    #[test]
    fn comparison_residual_eq_difference() {
        use super::comparison_residual;
        use reify_core::{DimensionVector, Type};
        use reify_ir::{BinOp, CompiledExpr, Value};

        let l_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let r_expr = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1.000001,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let values = ValueMap::new();
        let res = comparison_residual(BinOp::Eq, &l_expr, &r_expr, &values, &[], None);
        assert!(
            (res - 1e-6).abs() < 1e-12,
            "Eq with difference 1e-6 should have residual 1e-6, got {:.2e}",
            res
        );
    }

    #[test]
    fn constraint_residual_single_gt() {
        use super::constraint_residual;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{BinOp, CompiledExpr, Value};

        // thickness > 2mm, thickness=1.9999999m (violated by 1e-7)
        let thickness_ref = CompiledExpr::value_ref(ValueCellId::new("B", "t"), Type::length());
        let two = CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let expr = CompiledExpr::binop(BinOp::Gt, thickness_ref, two, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("B", "t"),
            Value::Scalar {
                si_value: 1.9999999,
                dimension: DimensionVector::LENGTH,
            },
        );

        let res = constraint_residual(&expr, &values, &[], None);
        assert!(
            (res - 1e-7).abs() < 1e-12,
            "single Gt constraint_residual should delegate correctly, got {:.2e}",
            res
        );
    }

    #[test]
    fn constraint_residual_and_returns_max() {
        use super::constraint_residual;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{BinOp, CompiledExpr, Value};

        // And(x > 2.0 [violated by 1e-7], y > 1.0 [violated by 1e-5])
        let x_ref = CompiledExpr::value_ref(ValueCellId::new("P", "x"), Type::length());
        let two = CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_x = CompiledExpr::binop(BinOp::Gt, x_ref, two, Type::Bool);

        let y_ref = CompiledExpr::value_ref(ValueCellId::new("P", "y"), Type::length());
        let one = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_y = CompiledExpr::binop(BinOp::Gt, y_ref, one, Type::Bool);

        let and_expr = CompiledExpr::binop(BinOp::And, gt_x, gt_y, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("P", "x"),
            Value::Scalar {
                si_value: 1.9999999,
                dimension: DimensionVector::LENGTH,
            },
        );
        values.insert(
            ValueCellId::new("P", "y"),
            Value::Scalar {
                si_value: 0.99999,
                dimension: DimensionVector::LENGTH,
            },
        );

        let res = constraint_residual(&and_expr, &values, &[], None);
        // max(1e-7, 1e-5) = 1e-5
        assert!(
            (res - 1e-5).abs() < 1e-10,
            "And should return max of sub-residuals, got {:.2e}",
            res
        );
    }

    #[test]
    fn constraint_residual_or_returns_min() {
        use super::constraint_residual;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{BinOp, CompiledExpr, Value};

        // Or(x > 2.0 [violated by 1e-3], y > 1.0 [satisfied])
        let x_ref = CompiledExpr::value_ref(ValueCellId::new("P", "x"), Type::length());
        let two = CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_x = CompiledExpr::binop(BinOp::Gt, x_ref, two, Type::Bool);

        let y_ref = CompiledExpr::value_ref(ValueCellId::new("P", "y"), Type::length());
        let one = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_y = CompiledExpr::binop(BinOp::Gt, y_ref, one, Type::Bool);

        let or_expr = CompiledExpr::binop(BinOp::Or, gt_x, gt_y, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("P", "x"),
            Value::Scalar {
                si_value: 1.999,
                dimension: DimensionVector::LENGTH,
            },
        );
        values.insert(
            ValueCellId::new("P", "y"),
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        );

        let res = constraint_residual(&or_expr, &values, &[], None);
        assert_eq!(res, 0.0, "Or with one satisfied should return 0.0");
    }

    #[test]
    fn max_constraint_residual_picks_worst() {
        use super::max_constraint_residual;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{BinOp, CompiledExpr, Value};

        // Three constraints: satisfied, violated by 1e-7, violated by 1e-5
        let x_ref = CompiledExpr::value_ref(ValueCellId::new("P", "x"), Type::length());
        let one = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        // x > 1.0, x=2.0 → satisfied
        let c1 = CompiledExpr::binop(BinOp::Gt, x_ref.clone(), one, Type::Bool);

        let two = CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.0000001,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        // x > 2.0000001, x=2.0 → violated by 1e-7
        let c2 = CompiledExpr::binop(BinOp::Gt, x_ref.clone(), two, Type::Bool);

        let three = CompiledExpr::literal(
            Value::Scalar {
                si_value: 2.00001,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        // x > 2.00001, x=2.0 → violated by 1e-5
        let c3 = CompiledExpr::binop(BinOp::Gt, x_ref, three, Type::Bool);

        let constraints = vec![
            (ConstraintNodeId::new("P", 0), c1),
            (ConstraintNodeId::new("P", 1), c2),
            (ConstraintNodeId::new("P", 2), c3),
        ];

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("P", "x"),
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        );

        let res = max_constraint_residual(&constraints, &values, &[], None);
        assert!(
            (res - 1e-5).abs() < 1e-10,
            "should return worst violation ~1e-5, got {:.2e}",
            res
        );
    }

    #[test]
    fn max_constraint_residual_all_satisfied() {
        use super::max_constraint_residual;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{BinOp, CompiledExpr, Value};

        let x_ref = CompiledExpr::value_ref(ValueCellId::new("P", "x"), Type::length());
        let one = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let c1 = CompiledExpr::binop(BinOp::Gt, x_ref, one, Type::Bool);

        let constraints = vec![(ConstraintNodeId::new("P", 0), c1)];

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("P", "x"),
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::LENGTH,
            },
        );

        let res = max_constraint_residual(&constraints, &values, &[], None);
        assert_eq!(res, 0.0, "all satisfied should return 0.0");
    }

    #[test]
    fn max_constraint_residual_empty() {
        use super::max_constraint_residual;

        let constraints = vec![];
        let values = ValueMap::new();
        let res = max_constraint_residual(&constraints, &values, &[], None);
        assert_eq!(res, 0.0, "empty constraints should return 0.0");
    }

    #[test]
    fn constraint_residual_bool_literals() {
        use super::constraint_residual;
        use reify_core::Type;
        use reify_ir::{CompiledExpr, Value};

        let values = ValueMap::new();

        let t = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        assert_eq!(constraint_residual(&t, &values, &[], None,), 0.0);

        let f = CompiledExpr::literal(Value::Bool(false), Type::Bool);
        assert_eq!(constraint_residual(&f, &values, &[], None,), 1.0);

        let u = CompiledExpr::literal(Value::Undef, Type::Bool);
        assert_eq!(constraint_residual(&u, &values, &[], None,), 10.0);
    }

    #[test]
    fn comparison_residual_non_numeric_fallback() {
        use super::comparison_residual;
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr, Value};

        // Non-numeric (Undef) inputs should give fixed penalty 1.0
        let l_expr = CompiledExpr::literal(Value::Undef, Type::Bool);
        let r_expr = CompiledExpr::literal(Value::Undef, Type::Bool);
        let values = ValueMap::new();
        let res = comparison_residual(BinOp::Gt, &l_expr, &r_expr, &values, &[], None);
        assert_eq!(res, 1.0, "Non-numeric inputs should give residual 1.0");
    }

    #[test]
    fn cost_function_penalizes_out_of_bounds() {
        use super::ConstraintCostFunction;
        use argmin::core::CostFunction;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let x_id = ValueCellId::new("Part", "x");
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let zero = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        // Trivially satisfied constraint: x > 0.0
        let constraint = CompiledExpr::binop(BinOp::Gt, x_ref, zero, Type::Bool);

        let auto_params = vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)),
            free: false,
        }];
        let constraints = vec![(ConstraintNodeId::new("Part", 0), constraint)];
        let base_values = ValueMap::new();

        let cost_fn = ConstraintCostFunction {
            auto_params: &auto_params,
            constraints: &constraints,
            base_values: &base_values,
            objective: None,
            functions: &[],
            // Task #5618: the clamp box is now supplied by the caller
            // (`resolve_bounds`) rather than read from `AutoParam.bounds` inline.
            bounds: &[(0.0, 0.010)],
            dependent_cells: &[],
            dispatch: None,
        };

        // In bounds: x=0.005
        let cost_in = cost_fn.cost(&vec![0.005]).unwrap();
        // Out of bounds: x=0.020 (above upper bound 0.010 by 0.010)
        let cost_out = cost_fn.cost(&vec![0.020]).unwrap();

        assert!(
            cost_out > cost_in,
            "out-of-bounds param should have higher cost (in={:.2e}, out={:.2e})",
            cost_in,
            cost_out
        );
    }

    #[test]
    fn cost_function_penalizes_undef_objective() {
        use super::ConstraintCostFunction;
        use argmin::core::CostFunction;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, ObjectiveSense, ObjectiveSet, Value};

        let x_id = ValueCellId::new("Part", "x");
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());

        // Trivially satisfied constraint: x > 0
        let zero_scalar = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let constraint = CompiledExpr::binop(BinOp::Gt, x_ref.clone(), zero_scalar, Type::Bool);

        // Objective: minimize(x / 0) — always Undef
        let zero_int = CompiledExpr::literal(Value::Int(0), Type::Int);
        let div_by_zero =
            CompiledExpr::binop(BinOp::Div, x_ref, zero_int, Type::dimensionless_scalar());
        let objective = Some(ObjectiveSet::single(ObjectiveSense::Minimize, div_by_zero));

        let auto_params = vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)),
            free: false,
        }];
        let constraints = vec![(ConstraintNodeId::new("Part", 0), constraint)];
        let base_values = ValueMap::new();

        let cost_fn = ConstraintCostFunction {
            auto_params: &auto_params,
            constraints: &constraints,
            base_values: &base_values,
            objective: objective.as_ref(),
            functions: &[],
            bounds: &[(0.0, 0.010)],
            dependent_cells: &[],
            dispatch: None,
        };

        // x=0.005 is in bounds and satisfies x > 0, but objective is Undef
        let cost = cost_fn.cost(&vec![0.005]).unwrap();
        assert!(
            cost > 1e10,
            "cost should be very large for Undef objective, got {:.2e}",
            cost
        );
    }

    /// Task η: centrality synthesis fires for an already-feasible scope with
    /// `objective: None` + a one-sided inequality constraint (x > 5 mm).
    /// Maximize(x − 5 mm) drives x toward the upper bound rather than preserving
    /// the initial-feasible point.
    ///
    /// (Renamed from `already_satisfied_returns_solved_immediately` — after task η
    /// the early-return fast-path is gated on `effective_objective.is_none()`, so an
    /// already-feasible scope with a synthetic objective now runs the optimiser and
    /// moves the parameter, contradicting the old name and its implied behaviour.)
    #[test]
    fn centrality_moves_already_feasible_param_toward_bound() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let solver = DimensionalSolver;
        let x_id = ValueCellId::new("Part", "x");

        // x > 5mm
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let five_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_expr = CompiledExpr::binop(BinOp::Gt, x_ref, five_mm, Type::Bool);

        // Current value already satisfies: x = 10mm
        let mut current = ValueMap::new();
        current.insert(
            x_id.clone(),
            Value::Scalar {
                si_value: 0.010,
                dimension: DimensionVector::LENGTH,
            },
        );

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            }],
            constraints: vec![(ConstraintNodeId::new("Part", 0), gt_expr)],
            current_values: current,
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let x = values.get(&x_id).unwrap().as_f64().unwrap();
                // Task η: centrality synthesis fires (single inequality x>5mm).
                // Maximize(x−5mm) pushes x toward the upper bound 100mm.
                // Must remain strictly feasible (x > 5mm).
                assert!(
                    x > 0.005,
                    "centrality synthesis result must satisfy x > 5mm, got {} m",
                    x
                );
                // Optimizer should have moved x above the initial 10mm toward the bound.
                assert!(
                    x > 0.010,
                    "centrality synthesis should move x above initial 10mm, got {} m",
                    x
                );
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    #[test]
    fn simplex_has_n_plus_1_vertices() {
        use super::build_simplex;

        // Task #5618: `build_simplex` takes the caller's resolved box
        // (`resolve_bounds`) rather than `&[AutoParam]`, so these fixtures pass the
        // per-dimension bounds directly.

        // 1-dimensional: simplex should have 2 vertices
        let simplex = build_simplex(&[0.5], &[(0.0, 1.0)]);
        assert_eq!(simplex.len(), 2, "1D simplex must have N+1=2 vertices");

        // 2-dimensional: simplex should have 3 vertices
        let simplex = build_simplex(&[0.5, 0.5], &[(0.0, 1.0), (0.0, 1.0)]);
        assert_eq!(simplex.len(), 3, "2D simplex must have N+1=3 vertices");

        // 3-dimensional: simplex should have 4 vertices
        let simplex = build_simplex(&[0.5, 0.5, 0.5], &[(0.0, 1.0), (0.0, 1.0), (0.0, 1.0)]);
        assert_eq!(simplex.len(), 4, "3D simplex must have N+1=4 vertices");
    }

    // ---- multistart_points unit tests (task δ #5016, step-1 RED / step-2 GREEN) ----
    //
    // `multistart_points` is the pure deterministic seed generator behind
    // `DimensionalSolver::solve_ranked`'s best-of-K multistart (PRD §5.3, §11 Q4).
    // These tests pin its K-count, seed-first ordering, bounds containment,
    // determinism, and corner/midpoint anchor shape before any call site wires it
    // into `solve_ranked` (step-3/4).

    /// Builds a 2-param length `ResolutionProblem` with distinct per-axis bounds and a
    /// `current_values` seed that sits off both axes' bounds-midpoints, so the seed
    /// point (start #0) is distinguishable from the all-midpoint point and every axis
    /// corner anchor. No constraints/objective — `multistart_points` reads only
    /// `auto_params` and `current_values` (via `extract_initial_point`).
    fn two_param_multistart_problem() -> ResolutionProblem {
        use reify_core::Type;
        use reify_ir::AutoParam;
        use reify_test_support::{mm, vcid};

        let x_id = vcid("Part", "x");
        let y_id = vcid("Part", "y");

        let mut current = ValueMap::new();
        current.insert(x_id.clone(), mm(10.0)); // 0.010 m — off the [5mm,100mm] midpoint (52.5mm)
        current.insert(y_id.clone(), mm(40.0)); // 0.040 m — off the [2mm,50mm] midpoint (26mm)

        ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![
                AutoParam {
                    id: x_id,
                    param_type: Type::length(),
                    bounds: Some((0.005, 0.100)),
                    free: false,
                },
                AutoParam {
                    id: y_id,
                    param_type: Type::length(),
                    bounds: Some((0.002, 0.050)),
                    free: false,
                },
            ],
            constraints: vec![],
            current_values: current,
            objective: None,
            functions: vec![].into(),
        }
    }

    #[test]
    fn multistart_points_count_is_2_times_dim_plus_1() {
        use super::multistart_points;

        let problem = two_param_multistart_problem();
        let points = multistart_points(&problem, None);
        // dim = 2 → K = 2*(2+1) = 6
        assert_eq!(
            points.len(),
            6,
            "expected K=2*(dim+1)=6 starts for a 2-param problem, got {}",
            points.len()
        );
        for p in &points {
            assert_eq!(
                p.len(),
                2,
                "each start vector must have one coordinate per auto param"
            );
        }
    }

    #[test]
    fn multistart_points_start_0_is_extract_initial_point_seed() {
        use super::{extract_initial_point, multistart_points};

        let problem = two_param_multistart_problem();
        let points = multistart_points(&problem, None);
        let seed = extract_initial_point(&problem, None);
        assert_eq!(
            points[0], seed,
            "start #0 must be the historical extract_initial_point seed \
             (dominance: best-of-K must never be worse than today's single start)"
        );
    }

    #[test]
    fn multistart_points_all_starts_within_effective_bounds() {
        use super::{effective_bounds, multistart_points};

        let problem = two_param_multistart_problem();
        let points = multistart_points(&problem, None);
        for (start_idx, point) in points.iter().enumerate() {
            for (axis, (&coord, param)) in point.iter().zip(problem.auto_params.iter()).enumerate()
            {
                let (lo, hi) = effective_bounds(param);
                assert!(
                    coord >= lo && coord <= hi,
                    "start {start_idx} axis {axis}: coordinate {coord} outside effective bounds [{lo}, {hi}]"
                );
            }
        }
    }

    #[test]
    fn multistart_points_is_deterministic_across_calls() {
        use super::multistart_points;

        let problem = two_param_multistart_problem();
        let first = multistart_points(&problem, None);
        let second = multistart_points(&problem, None);
        assert_eq!(
            first, second,
            "multistart_points is a pure function of `problem` (no RNG/clock/seed, BT5) — \
             two calls on the same problem must return identical vectors"
        );
    }

    #[test]
    fn multistart_points_includes_midpoint_and_per_axis_corner_anchors() {
        use super::{effective_bounds, multistart_points};

        let problem = two_param_multistart_problem();
        let points = multistart_points(&problem, None);

        let (lo_x, hi_x) = effective_bounds(&problem.auto_params[0]);
        let (lo_y, hi_y) = effective_bounds(&problem.auto_params[1]);
        let mid_x = (lo_x + hi_x) / 2.0;
        let mid_y = (lo_y + hi_y) / 2.0;

        // All-midpoint point.
        assert!(
            points.iter().any(|p| p[0] == mid_x && p[1] == mid_y),
            "expected an all-midpoint start [{mid_x}, {mid_y}]; got {points:?}"
        );
        // Axis 0 (x) low/high anchors, y held at its midpoint.
        assert!(
            points.iter().any(|p| p[0] == lo_x && p[1] == mid_y),
            "expected an x-low/y-mid corner anchor [{lo_x}, {mid_y}]; got {points:?}"
        );
        assert!(
            points.iter().any(|p| p[0] == hi_x && p[1] == mid_y),
            "expected an x-high/y-mid corner anchor [{hi_x}, {mid_y}]; got {points:?}"
        );
        // Axis 1 (y) low/high anchors, x held at its midpoint.
        assert!(
            points.iter().any(|p| p[0] == mid_x && p[1] == lo_y),
            "expected a y-low/x-mid corner anchor [{mid_x}, {lo_y}]; got {points:?}"
        );
        assert!(
            points.iter().any(|p| p[0] == mid_x && p[1] == hi_y),
            "expected a y-high/x-mid corner anchor [{mid_x}, {hi_y}]; got {points:?}"
        );
    }

    /// Task #5618 step-7: the per-axis corner anchors must sample the
    /// CONSTRAINT-DERIVED box, not `default_bounds_for`.
    ///
    /// `two_param_multistart_problem` above sets explicit `bounds: Some(..)` with an
    /// EMPTY constraint list, so the derived path is inert there by construction and
    /// the four tests above are unaffected. This fixture is the production shape:
    /// `bounds: None` (so `effective_bounds` degrades to the dimensionless
    /// `(-1e6, 1e6)`) plus an inequality pair per axis. Without the derived seed box
    /// starts #1..5 are the all-`0` midpoint and corners at ±10⁶ — every one of them
    /// outside `[1, 100] × [2, 200]`, so best-of-K degenerates to best-of-one and the
    /// whole ranked result rests on start #0 alone.
    #[test]
    fn multistart_points_corners_sample_the_constraint_derived_box() {
        use reify_ir::BinOp;

        use super::multistart_points;

        let q0 = reify_core::ValueCellId::new("Derive", "q0");
        let q1 = reify_core::ValueCellId::new("Derive", "q1");
        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![real_auto_param(q0.clone()), real_auto_param(q1.clone())],
            constraints: as_constraints(vec![
                cmp_ref_lit(BinOp::Ge, &q0, 1.0),
                cmp_ref_lit(BinOp::Le, &q0, 100.0),
                cmp_ref_lit(BinOp::Ge, &q1, 2.0),
                cmp_ref_lit(BinOp::Le, &q1, 200.0),
            ]),
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        let points = multistart_points(&problem, None);
        assert_eq!(points.len(), 6, "K = 2*(dim+1) is unchanged by task #5618");

        // Derived seed boxes: [1, 100] and [2, 200]; midpoints 50.5 and 101.0.
        let (mid_0, mid_1) = (50.5, 101.0);
        for (label, expect) in [
            ("all-midpoint", [mid_0, mid_1]),
            ("q0-low", [1.0, mid_1]),
            ("q0-high", [100.0, mid_1]),
            ("q1-low", [mid_0, 2.0]),
            ("q1-high", [mid_0, 200.0]),
        ] {
            assert!(
                points
                    .iter()
                    .any(|p| (p[0] - expect[0]).abs() < 1e-9 && (p[1] - expect[1]).abs() < 1e-9),
                "expected the {label} anchor {expect:?} from the CONSTRAINT-DERIVED box; \
                 corners at ±1e6 mean multistart is still reading `default_bounds_for`. \
                 got {points:?}"
            );
        }
    }

    // ---- end multistart_points unit tests ----

    /// Verify that the optimizer converges near the lower bound when minimizing.
    /// With auto param bounds [5mm, 100mm] and a trivially-satisfied constraint
    /// (x > 1mm), minimizing x should drive it toward the 5mm lower bound,
    /// confirming convergence quality (result between 4mm and 8mm).
    ///
    /// Also serves as a positive-path regression guard for "feasibility check
    /// returns Solved when an objective is present."
    #[test]
    fn optimization_converges_near_lower_bound() {
        use crate::DimensionalSolver;
        use reify_core::Type;
        use reify_ir::{AutoParam, ObjectiveSense, ObjectiveSet};
        use reify_test_support::{cnid, gt, literal, mm, value_ref, vcid};

        let solver = DimensionalSolver;
        let x_id = vcid("Part", "x");

        // x > 1mm — trivially satisfied when x starts at 10mm
        let x_ref = value_ref("Part", "x");
        let one_mm = literal(mm(1.0));
        let gt_expr = gt(x_ref.clone(), one_mm);

        // Minimize x — with auto param bounds [5mm, 100mm], the minimum
        // is at 5mm which is still above the 1mm constraint.
        let objective = ObjectiveSet::single(ObjectiveSense::Minimize, x_ref);

        let mut current = ValueMap::new();
        current.insert(x_id.clone(), mm(10.0)); // 10mm — already feasible

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.005, 0.100)), // 5mm–100mm
                free: false,
            }],
            constraints: vec![(cnid("Part", 0), gt_expr)],
            current_values: current,
            objective: Some(objective),
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let si = values.get(&x_id).unwrap().as_f64().unwrap();
                assert!(
                    si > 0.004 && si < 0.008,
                    "optimizer should drive x toward 5mm lower bound, got {} m \
                     (expected 4mm < x < 8mm — lower bound catches zero/negative, \
                     upper bound confirms convergence near 5mm)",
                    si
                );
            }
            other => panic!(
                "minimizing x with feasible initial point should return Solved, got {:?}",
                other
            ),
        }
    }

    /// Running the solver through TerminationReason extraction must not panic
    /// or regress the result. A trivially feasible 1-param problem (x > 5mm AND
    /// x < 50mm with bounds [1mm, 100mm]) must return Solved with x in the
    /// feasible range, verifying both the solver result variant and constraint
    /// satisfaction.
    #[test]
    fn termination_reason_extracted_without_panic() {
        use crate::DimensionalSolver;
        use reify_core::Type;
        use reify_ir::{AutoParam, Value};
        use reify_test_support::{cnid, gt, literal, lt, mm, value_ref, vcid};

        let solver = DimensionalSolver;
        let x_id = vcid("Part", "x");

        // Simple feasibility: x > 5mm AND x < 50mm
        let x_ref = value_ref("Part", "x");
        let five_mm = literal(mm(5.0));
        let fifty_mm = literal(mm(50.0));
        let gt_expr = gt(x_ref.clone(), five_mm);
        let lt_expr = lt(x_ref, fifty_mm);

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            }],
            constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        let SolveResult::Solved { values, .. } = result else {
            panic!(
                "trivially feasible 1-param problem must return Solved, got {:?}",
                result
            );
        };

        // Verify constraint satisfaction: solved x must be within (5mm, 50mm).
        let x_val = values.get(&x_id).expect("solved values must contain x");
        if let Value::Scalar { si_value, .. } = x_val {
            assert!(
                *si_value > 0.005 && *si_value < 0.050,
                "solved x SI value {} must be in (0.005, 0.050)",
                si_value
            );
        } else {
            panic!("expected Scalar value for x, got {:?}", x_val);
        }
    }

    #[test]
    fn build_solved_values_builds_correct_hashmap() {
        use super::build_solved_values;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, Value};

        let length_id = ValueCellId::new("Part", "length");
        let angle_id = ValueCellId::new("Part", "angle");

        let params = vec![
            AutoParam {
                id: length_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 1.0)),
                free: false,
            },
            AutoParam {
                id: angle_id.clone(),
                param_type: Type::angle(),
                bounds: Some((0.0, std::f64::consts::TAU)),
                free: false,
            },
        ];

        let x = [0.025, std::f64::consts::FRAC_PI_2]; // 25mm, ~90°

        let result = build_solved_values(&params, &x);

        assert_eq!(result.len(), 2, "should contain exactly 2 entries");

        // Check length entry
        match result.get(&length_id) {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) => {
                assert!(
                    (si_value - 0.025).abs() < 1e-15,
                    "length si_value should be 0.025, got {}",
                    si_value
                );
                assert_eq!(
                    *dimension,
                    DimensionVector::LENGTH,
                    "length dimension should be LENGTH"
                );
            }
            other => panic!("expected Scalar for length, got {:?}", other),
        }

        // Check angle entry
        match result.get(&angle_id) {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) => {
                assert!(
                    (si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-15,
                    "angle si_value should be FRAC_PI_2, got {}",
                    si_value
                );
                assert_eq!(
                    *dimension,
                    DimensionVector::ANGLE,
                    "angle dimension should be ANGLE"
                );
            }
            other => panic!("expected Scalar for angle, got {:?}", other),
        }
    }

    #[test]
    fn build_solved_values_empty_params_returns_empty_map() {
        use super::build_solved_values;

        let result = build_solved_values(&[], &[]);
        assert!(result.is_empty(), "empty params should produce empty map");
    }

    #[test]
    fn build_solved_values_dimensionless_type() {
        use super::build_solved_values;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, Value};

        let id = ValueCellId::new("Part", "ratio");
        let params = vec![AutoParam {
            id: id.clone(),
            param_type: Type::dimensionless_scalar(),
            bounds: None,
            free: false,
        }];
        let x = [3.125];

        let result = build_solved_values(&params, &x);
        assert_eq!(result.len(), 1);

        match result.get(&id) {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) => {
                assert!(
                    (si_value - 3.125).abs() < 1e-15,
                    "si_value should be 3.125, got {}",
                    si_value
                );
                assert_eq!(
                    *dimension,
                    DimensionVector::DIMENSIONLESS,
                    "Type::dimensionless_scalar() should map to DIMENSIONLESS"
                );
            }
            other => panic!("expected Scalar for ratio, got {:?}", other),
        }
    }

    #[test]
    #[should_panic(expected = "params and x must have the same length")]
    fn build_solved_values_panics_on_length_mismatch() {
        use super::build_solved_values;
        use reify_core::{Type, ValueCellId};
        use reify_ir::AutoParam;

        let params = vec![AutoParam {
            id: ValueCellId::new("Part", "length"),
            param_type: Type::length(),
            bounds: Some((0.001, 1.0)),
            free: false,
        }];
        // x has 2 elements but params has 1 — should panic
        let x = [0.025, 0.050];

        let _ = build_solved_values(&params, &x);
    }

    /// A feasible initial point with an always-undefined objective (x/0)
    /// must return NoProgress, never Solved. Because the objective is Undef
    /// everywhere, the optimizer stays near the initial (feasible) point and
    /// the post-solve validation (not the fallback path) catches the undefined
    /// objective. The reason string should mention "solution point".
    #[test]
    fn undefined_objective_at_feasible_initial_returns_no_progress() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, ObjectiveSense, ObjectiveSet, Value};

        let solver = DimensionalSolver;
        let x_id = ValueCellId::new("Part", "x");

        // x > 5mm — satisfied when x starts at 10mm
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let five_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_expr = CompiledExpr::binop(BinOp::Gt, x_ref.clone(), five_mm, Type::Bool);

        // Objective: minimize(x / 0) — always Undef
        let zero_int = CompiledExpr::literal(Value::Int(0), Type::Int);
        let div_by_zero =
            CompiledExpr::binop(BinOp::Div, x_ref, zero_int, Type::dimensionless_scalar());
        let objective = ObjectiveSet::single(ObjectiveSense::Minimize, div_by_zero);

        // Current value x = 10mm (already satisfies x > 5mm)
        let mut current = ValueMap::new();
        current.insert(
            x_id.clone(),
            Value::Scalar {
                si_value: 0.010,
                dimension: DimensionVector::LENGTH,
            },
        );

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            }],
            constraints: vec![(ConstraintNodeId::new("Part", 0), gt_expr)],
            current_values: current,
            objective: Some(objective),
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::NoProgress { reason } => {
                assert!(
                    reason.contains("solution point"),
                    "expected post-solve path ('solution point'), got: {}",
                    reason
                );
            }
            other => panic!(
                "feasible initial + undefined objective should return NoProgress, got {:?}",
                other
            ),
        }
    }

    /// Trigger the *fallback* path for undefined-objective validation:
    /// the optimizer drifts infeasible while chasing an objective that is
    /// Undef in the feasible region but defined (small) in the infeasible
    /// region. When the solver falls back to the initial feasible point,
    /// it discovers the objective is undefined there and returns NoProgress
    /// with a reason mentioning "fallback point".
    ///
    /// Key design: uses TWO thresholds — the constraint boundary (x <= 0.020)
    /// and a wider Undef boundary (x <= 0.022) in the Conditional. This prevents
    /// the optimizer from finding a boundary sweet spot where both constraint
    /// and objective are simultaneously satisfied. The simplex perturbation
    /// (+10% of range ≈ 0.0099) pushes the second vertex to ~0.0249 (past the
    /// Undef boundary), giving the optimizer a low-cost infeasible vertex to
    /// chase.
    #[test]
    fn undefined_objective_at_fallback_triggers_no_progress() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, ObjectiveSense, ObjectiveSet, Value};
        use reify_test_support::conditional_expr;

        let solver = DimensionalSolver;
        let x_id = ValueCellId::new("Part", "x");

        // Constraint: x <= 0.020 (feasible when x ≤ 20mm)
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let constraint_threshold = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.020,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let le_expr =
            CompiledExpr::binop(BinOp::Le, x_ref.clone(), constraint_threshold, Type::Bool);

        // Objective: minimize(if x <= 0.022 then x/0 else x)
        //
        // The Undef boundary (0.022) is wider than the constraint boundary (0.020),
        // preventing the optimizer from finding a feasible point with a defined objective.
        //
        // x ≤ 0.022: objective = x/0 = Undef → UNDEF_OBJECTIVE_PENALTY (~f64::MAX/2)
        //   (covers entire feasible region x ≤ 0.020 plus a buffer zone 0.020..0.022)
        // x > 0.022: objective = x → small finite value (well into infeasible region)
        //
        // Initial simplex: vertex 0 at x=0.015 (feasible, Undef, cost ≈ f64::MAX/2),
        //   vertex 1 at x=0.015+0.0099≈0.0249 (infeasible, finite, cost ≈ 4900).
        // The enormous cost differential lures the optimizer past x=0.022 into the
        // infeasible region. The solver detects infeasibility (residual >> 1e-12),
        // falls back to the initial feasible point, then discovers the objective
        // is Undef there → NoProgress("fallback point").
        let undef_threshold = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.022,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let condition = CompiledExpr::binop(BinOp::Le, x_ref.clone(), undef_threshold, Type::Bool);
        let zero_int = CompiledExpr::literal(Value::Int(0), Type::Int);
        let then_branch = CompiledExpr::binop(
            BinOp::Div,
            x_ref.clone(),
            zero_int,
            Type::dimensionless_scalar(),
        );
        let else_branch = x_ref;

        let objective_expr = conditional_expr(condition, then_branch, else_branch);
        let objective = ObjectiveSet::single(ObjectiveSense::Minimize, objective_expr);

        // Current value x = 0.015 (15mm, feasible since 0.015 <= 0.020)
        // With bounds (0.001, 0.1), the simplex perturbation is +0.0099,
        // pushing the second vertex to ~0.0249 (past both thresholds).
        let mut current = ValueMap::new();
        current.insert(
            x_id.clone(),
            Value::Scalar {
                si_value: 0.015,
                dimension: DimensionVector::LENGTH,
            },
        );

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            }],
            constraints: vec![(ConstraintNodeId::new("Part", 0), le_expr)],
            current_values: current,
            objective: Some(objective),
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::NoProgress { reason } => {
                assert!(
                    reason.contains("fallback point"),
                    "expected fallback path ('fallback point'), got: {}",
                    reason
                );
            }
            other => panic!(
                "feasible initial + region-dependent Undef objective should return NoProgress, got {:?}",
                other
            ),
        }
    }

    /// Happy path of the fallback mechanism: the optimizer drifts infeasible
    /// while chasing an attractive objective in the infeasible region, the solver
    /// falls back to the initial feasible point, the objective IS defined there,
    /// and the solver returns Solved with the exact initial values.
    ///
    /// This completes the trio with `undefined_objective_at_feasible_initial_returns_no_progress`
    /// and `undefined_objective_at_fallback_triggers_no_progress`, covering all three
    /// branches of the fallback validation logic (solver.rs lines 637-659).
    #[test]
    fn defined_objective_at_fallback_returns_solved() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, ObjectiveSense, ObjectiveSet, Value};
        use reify_test_support::conditional_expr;

        let solver = DimensionalSolver;
        let x_id = ValueCellId::new("Part", "x");

        // Constraint: x <= 0.020 (feasible when x ≤ 20mm)
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let constraint_threshold = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.020,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let le_expr =
            CompiledExpr::binop(BinOp::Le, x_ref.clone(), constraint_threshold, Type::Bool);

        // Objective: minimize(if x <= 0.022 then 1e8 else x)
        //
        // The large constant (1e8) in the feasible region creates cost >> infeasible
        // cost (~5000), luring the optimizer past x=0.022 into the infeasible region.
        //
        // x ≤ 0.022: objective = 1e8 → large defined finite value (covers entire
        //   feasible region plus a buffer zone 0.020..0.022)
        // x > 0.022: objective = x → small attractive value (well into infeasible region)
        //
        // Initial simplex: vertex 0 at x=0.015 (feasible, cost=1e8),
        //   vertex 1 at x=0.015+0.0099≈0.0249 (infeasible, cost≈4900).
        // The enormous cost differential lures the optimizer past x=0.022 into the
        // infeasible region. The solver detects infeasibility (residual >> 1e-12),
        // falls back to the initial feasible point x=0.015, validates the objective
        // (eval_objective_set returns Some(1e8) → passes), and returns Solved with the
        // initial values.
        let cond_threshold = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.022,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let condition = CompiledExpr::binop(BinOp::Le, x_ref.clone(), cond_threshold, Type::Bool);
        let then_branch = CompiledExpr::literal(Value::Real(1e8), Type::dimensionless_scalar());
        let else_branch = x_ref;

        let objective_expr = conditional_expr(condition, then_branch, else_branch);
        let objective = ObjectiveSet::single(ObjectiveSense::Minimize, objective_expr);

        // Current value x = 0.015 (15mm, feasible since 0.015 <= 0.020)
        // With bounds (0.001, 0.1), the simplex perturbation is +0.0099,
        // pushing the second vertex to ~0.0249 (past both thresholds).
        let mut current = ValueMap::new();
        current.insert(
            x_id.clone(),
            Value::Scalar {
                si_value: 0.015,
                dimension: DimensionVector::LENGTH,
            },
        );

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                // not testing uniqueness — MEASURED (#5711 pre-1): the
                // objective is the constant 1e8 across the whole feasible
                // region [0.001, 0.020] (plus the 0.020..0.022 buffer), so
                // the derived-box perturbed re-solve ties the incumbent's
                // objective score exactly — a genuine §11.6 flat-region
                // non-uniqueness, not the drift-fallback mechanism this
                // fixture exists to cover. free: true keeps the fallback
                // path under test alive without asserting a uniqueness
                // verdict this problem cannot support. See verify_uniqueness's
                // doc comment (§ Per-fixture measurement) for the full ruling.
                free: true,
            }],
            constraints: vec![(ConstraintNodeId::new("Part", 0), le_expr)],
            current_values: current,
            objective: Some(objective),
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let si = values.get(&x_id).unwrap().as_f64().unwrap();
                assert!(
                    (si - 0.015).abs() < 1e-10,
                    "fallback path should return initial x = 0.015 m, got {} m",
                    si
                );
            }
            other => panic!(
                "feasible initial + region-dependent defined objective should return Solved \
                 (fallback happy path), got {:?}",
                other
            ),
        }
    }

    // ── centrality default objective tests (task 4013, PRD η) ──────────────

    /// [B6 GREEN] x >= 2mm, x <= 8mm, objective: None → solver must return
    /// x ≈ 5mm (the Chebyshev centre of [2mm, 8mm]).
    ///
    /// Before step-2 this test was RED (solver returned first-feasible boundary).
    /// After step-2 the synthetic centrality objective drives Nelder-Mead to the
    /// midpoint x = 5mm within the 1e-4 m tolerance required by PRD §11.
    #[test]
    fn centrality_default_centers_two_sided_bound() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{
            AutoParam, BinOp, CompiledExpr, ConstraintSolver, ResolutionProblem, SolveResult,
            Value, ValueMap,
        };

        let solver = DimensionalSolver;

        let x_id = ValueCellId::new("CentredBar", "x");
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());

        // x >= 2mm
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let ge_expr = CompiledExpr::binop(BinOp::Ge, x_ref.clone(), two_mm, Type::Bool);

        // x <= 8mm
        let eight_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.008,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let le_expr = CompiledExpr::binop(BinOp::Le, x_ref, eight_mm, Type::Bool);

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None, // use default bounds (1µm–10m)
                free: false,
            }],
            constraints: vec![
                (ConstraintNodeId::new("CentredBar", 0), ge_expr),
                (ConstraintNodeId::new("CentredBar", 1), le_expr),
            ],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let si = values.get(&x_id).unwrap().as_f64().unwrap();
                // Chebyshev centre of [2mm, 8mm] is the midpoint 5mm (0.005 m).
                // Tolerance: |x − 5mm| < 1e-4 m (0.1mm) per PRD §11.
                assert!(
                    (si - 0.005).abs() < 1e-4,
                    "centrality should place x ≈ 5mm (0.005 m), got {:.6} m",
                    si
                );
                // Must be strictly interior — NOT on the boundary.
                assert!(
                    si > 0.002 && si < 0.008,
                    "x must be strictly interior to [2mm, 8mm], got {:.6} m",
                    si
                );
            }
            other => panic!("expected Solved (centrality), got {:?}", other),
        }
    }

    /// [step-3 RED, step-4 GREEN] Discrete (Int) auto param with inequality constraints:
    /// `build_centrality_objective` must return `None` (continuous-only guard, PRD B7).
    ///
    /// Before step-4 adds the Type::Scalar check, the function has no discrete-type
    /// guard and returns Some(centrality) for any param type → this assertion fails (RED).
    /// After step-4 inserts the Type::Scalar guard, Int params short-circuit to None (GREEN).
    #[test]
    fn centrality_objective_none_for_discrete_param() {
        use reify_core::{ConstraintNodeId, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let x_id = ValueCellId::new("DiscreteScope", "x");

        // Integer-valued reference and literal
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::Int);
        let five_lit = CompiledExpr::literal(Value::Int(5), Type::Int);
        // Inequality constraint: x >= 5
        let ge_expr = CompiledExpr::binop(BinOp::Ge, x_ref.clone(), five_lit.clone(), Type::Bool);
        let ten_lit = CompiledExpr::literal(Value::Int(10), Type::Int);
        // x <= 10
        let le_expr = CompiledExpr::binop(BinOp::Le, x_ref, ten_lit, Type::Bool);

        let auto_params = vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::Int, // discrete — not Scalar
            bounds: Some((-1e6, 1e6)),
            free: true,
        }];
        let constraints = vec![
            (ConstraintNodeId::new("DiscreteScope", 0), ge_expr),
            (ConstraintNodeId::new("DiscreteScope", 1), le_expr),
        ];

        let result = super::build_centrality_objective(&auto_params, &constraints);
        assert!(
            result.is_none(),
            "build_centrality_objective must return None for discrete (Int) auto params \
             (continuous-only guard, B7); got Some(_)"
        );
    }

    /// [step-3 GREEN immediately] Scalar auto param with equality-only constraints:
    /// `build_centrality_objective` must return `None` (no inequality slacks → first-feasible).
    ///
    /// `collect_slack_terms` skips BinOp::Eq entirely, so slacks is empty → None is
    /// returned already by the step-2 implementation (no inequality slacks guard).
    /// This test documents and locks in that existing correct behaviour.
    #[test]
    fn centrality_objective_none_without_inequalities() {
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let x_id = ValueCellId::new("EqScope", "x");

        // Scalar reference
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        // x == 5mm (equality only — no signed-slack decomposition)
        let five_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let eq_expr = CompiledExpr::binop(BinOp::Eq, x_ref, five_mm, Type::Bool);

        let auto_params = vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: None,
            free: false,
        }];
        let constraints = vec![(ConstraintNodeId::new("EqScope", 0), eq_expr)];

        let result = super::build_centrality_objective(&auto_params, &constraints);
        assert!(
            result.is_none(),
            "build_centrality_objective must return None when only equality constraints exist \
             (no signed-slack decomposition); got Some(_)"
        );
    }

    /// [task-4700 RED → step-2 GREEN] DimensionalSolver must return Solved when
    /// the auto param x must MOVE from an off-target seed to reach the constraint.
    ///
    /// Setup: `param x: Length = auto; constraint x == 10mm`.
    /// current_values seeds x = 20mm (0.02 m) — the MOVED case.
    ///
    /// With the pre-fix sd_tolerance=1e-15 the Nelder-Mead cost (sum of squared
    /// violations, i.e. d²) converges to a floor where the LINEAR residual
    /// (|d|) is ~1e-8, which is > FEASIBILITY_THRESHOLD=1e-12. The solver
    /// returns Infeasible, so this test is RED before step-2.
    ///
    /// After step-2 tightens sd_tolerance to NM_SD_TOLERANCE (≤ FEASIBILITY_THRESHOLD²),
    /// the linear residual reaches ~1e-16, well below 1e-12, and the test is GREEN.
    #[test]
    fn dimensional_solver_resolves_moved_eq_auto() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{
            AutoParam, BinOp, CompiledExpr, ConstraintSolver, ResolutionProblem, SolveResult,
            Value, ValueMap,
        };

        let solver = DimensionalSolver;

        let x_id = ValueCellId::new("MovedAuto", "x");
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());

        // constraint: x == 10mm (0.01 m in SI)
        let ten_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.01,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let eq_expr = CompiledExpr::binop(BinOp::Eq, x_ref, ten_mm, Type::Bool);

        // Seed x = 20mm (MOVED — off-target, requires Nelder-Mead to search)
        let mut current_values = ValueMap::new();
        current_values.insert(
            x_id.clone(),
            Value::Scalar {
                si_value: 0.02,
                dimension: DimensionVector::LENGTH,
            },
        );

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None, // default bounds (1µm–10m)
                free: false,
            }],
            constraints: vec![(ConstraintNodeId::new("MovedAuto", 0), eq_expr)],
            current_values,
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let si = values.get(&x_id).unwrap().as_f64().unwrap();
                assert!(
                    (si - 0.01).abs() <= 1e-11,
                    "moved-auto eq constraint: x must converge to 0.01 m (10mm) \
                     within 1e-11 m; got {si:.3e} m (error {:.3e} m)",
                    (si - 0.01).abs()
                );
            }
            SolveResult::Infeasible { .. } => {
                panic!(
                    "dimensional_solver_resolves_moved_eq_auto: expected Solved but got \
                     Infeasible. This indicates the NM sd_tolerance floor prevents the \
                     linear residual from reaching FEASIBILITY_THRESHOLD=1e-12. Fix: \
                     tighten sd_tolerance to NM_SD_TOLERANCE ≤ FEASIBILITY_THRESHOLD² \
                     (see step-2)."
                );
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    /// Solver correctness under tight uniqueness tolerance (task #4710):
    /// a problem with one constrained strict auto (`x == 10mm`, seed 20mm) and one
    /// unconstrained strict auto (`y`) must return `SolveResult::Infeasible` with
    /// `DiagnosticCode::ConstraintNonUnique`.
    ///
    /// ## Landed contract
    ///
    /// The uniqueness re-solve in `verify_uniqueness` routes through `NM_SD_TOLERANCE`
    /// (the tight main-solve tolerance).  The perturbed re-solve converges `x` to its
    /// constrained value; `y` (unconstrained) lands at a different point;
    /// `solutions_agree` returns `false`; `ConstraintNonUnique` is raised.
    ///
    /// This is the **correct** solver behaviour.  The eval layer (task #4710,
    /// `engine_eval::connector_pin_if_determined`) is responsible for ensuring that
    /// connector-instance autos are never injected as unconstrained strict autos into
    /// the parent resolution problem — so the `AllFourSites` example never reaches
    /// the solver with `__connector_0.gain` unconstrained.
    #[test]
    fn unconstrained_strict_auto_flagged_non_unique_under_tight_tolerance() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DiagnosticCode, DimensionVector, Type, ValueCellId};
        use reify_ir::{
            AutoParam, BinOp, CompiledExpr, ConstraintSolver, ResolutionProblem, SolveResult,
            Value, ValueMap,
        };

        let solver = DimensionalSolver;

        let x_id = ValueCellId::new("UniquenessDecouple", "x");
        let y_id = ValueCellId::new("UniquenessDecouple", "y");

        // constraint: x == 10mm (0.01 m in SI); y has no determining constraint.
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let ten_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.01,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let eq_expr = CompiledExpr::binop(BinOp::Eq, x_ref, ten_mm, Type::Bool);

        // Seed x = 20mm (MOVED — off-target, requires NM_SD_TOLERANCE=1e-30 to converge).
        // Seed y = 5mm (arbitrary; no constraint anchors it).
        let mut current_values = ValueMap::new();
        current_values.insert(
            x_id.clone(),
            Value::Scalar {
                si_value: 0.02,
                dimension: DimensionVector::LENGTH,
            },
        );
        current_values.insert(
            y_id.clone(),
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
        );

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![
                AutoParam {
                    id: x_id.clone(),
                    param_type: Type::length(),
                    bounds: None,
                    free: false, // strict; determined by x == 10mm constraint
                },
                AutoParam {
                    id: y_id.clone(),
                    param_type: Type::length(),
                    bounds: None,
                    free: false, // strict but NO determining constraint in this problem
                },
            ],
            constraints: vec![(ConstraintNodeId::new("UniquenessDecouple", 0), eq_expr)],
            current_values,
            objective: None,
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Infeasible { ref diagnostics } => {
                let has_non_unique = diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique));
                assert!(
                    has_non_unique,
                    "expected ConstraintNonUnique for a problem with one constrained strict \
                     auto (x==10mm) and one unconstrained strict auto (y); \
                     got Infeasible but not ConstraintNonUnique. diagnostics: {diagnostics:?}"
                );
            }
            SolveResult::Solved { .. } => {
                panic!(
                    "expected Infeasible/ConstraintNonUnique for a bare \
                     {{x==10mm strict, y unconstrained strict}} problem, but got Solved. \
                     The uniqueness re-solve must route through the tight NM_SD_TOLERANCE \
                     so that an unconstrained strict auto is correctly flagged non-unique. \
                     See task #4710 and esc-4700-34."
                );
            }
            other => panic!("expected Infeasible/ConstraintNonUnique, got {:?}", other),
        }
    }

    /// §11.6 "flat region → not uniquely optimal" (task #5711): a strict auto
    /// bracketed by a plain inequality pair (`x > 5mm AND x < 6mm`), with
    /// `bounds: None` (the production shape — the "Constraint-derived parameter
    /// bounds" section header above records that no `.ri` surface ever sets
    /// `AutoParam.bounds`), and an objective that is
    /// FLAT within the bracket (`minimize(if x <= 6.2mm then 1e8 else x)` —
    /// the exact cost-cliff shape of `defined_objective_at_fallback_returns_solved`,
    /// scaled to this bracket) must report `ConstraintNonUnique`:
    /// post-perturbation the params disagree (the anchor lands elsewhere in
    /// the bracket) and the objective ties (both points fall in the flat
    /// `1e8` region), so BOTH §11.6 well-determinedness tests fail.
    ///
    /// MEASURED (task #5711, not guessed): a naively "flat everywhere" literal
    /// objective does NOT reproduce today's bug for this shape — a
    /// featureless cost surface lets Nelder-Mead reconverge cleanly from even
    /// an astronomically distant anchor (nothing pulls it back toward a
    /// boundary), so the pre-existing parameter-comparison mechanism already
    /// (and correctly, if coincidentally) reports `ConstraintNonUnique`
    /// without step-5. The cost-CLIFF shape here is what reproduces the real
    /// bug: it drives the SAME "optimizer drifts infeasible, falls back to
    /// the initial point" mechanism as `defined_objective_at_fallback_returns_solved`
    /// or `warm_start_falls_back_to_initial_when_optimizer_drifts_infeasible`,
    /// which is precisely the shape `verify_uniqueness`'s perturbation
    /// re-solve cannot recover from today.
    ///
    /// RED today (MEASURED): `solver.solve(&problem)` returns
    /// `Solved { values: {x: 0.0055}, unique: true }` — the main solve drifts
    /// infeasible chasing the cost cliff and falls back to the initial
    /// 5.5mm; `verify_uniqueness`'s perturbation anchor is still built from
    /// `effective_bounds`, which degrades to `default_bounds_for` —
    /// `(1e-6, 10.0)` for `LENGTH`, since `bounds: None` here — landing at
    /// `0.9 * 10.0 \u{2248} 9.0` (9 metres), wildly outside the 1mm bracket.
    /// `solve_core`'s re-solve from there does NOT converge (confirmed via a
    /// DEBUG-capturing subscriber: the exact message
    /// `"uniqueness check: perturbed solve did not converge; assuming
    /// unique"` fires), so the inert `_ =>` arm in `verify_uniqueness`
    /// conservatively reports `true` — silently returning
    /// `Solved { unique: true }` for a problem that is genuinely NOT
    /// uniquely determined (any `x` in `(5mm, 6mm)` scores the same flat
    /// `1e8` objective).
    ///
    /// AFTER step-5: the anchor is built from the derived box `(0.005,
    /// 0.006)`. Incumbent 0.0055 is not below its midpoint, so the anchor is
    /// `lo + 0.1*(hi-lo) = 0.0051` — feasible AND still below the `6.2mm`
    /// buffer threshold, so the re-solve lands cleanly at `0.0051` (MEASURED:
    /// `Solved`, not lured past the cliff). Both `0.0055` and `0.0051` score
    /// the SAME flat `1e8` (the cliff is at `6.2mm`, well above both), so the
    /// params differ but the objective ties → `NonUnique`.
    #[test]
    fn flat_objective_over_inequality_bracket_reports_non_unique() {
        use crate::DimensionalSolver;
        use reify_core::{ConstraintNodeId, DiagnosticCode, DimensionVector, Type, ValueCellId, hash::ContentHash};
        use reify_ir::{
            AutoParam, BinOp, CompiledExpr, CompiledExprKind, ConstraintSolver, ObjectiveSense,
            ObjectiveSet, ResolutionProblem, SolveResult, Value, ValueMap,
        };

        let solver = DimensionalSolver;
        let x_id = ValueCellId::new("Part", "x");

        // x > 5mm AND x < 6mm — two constraint nodes (house convention for
        // conjunction; see e.g. `strict_auto_non_unique_returns_infeasible` in
        // `solver_integration.rs`).
        let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::length());
        let five_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let six_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.006,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let gt_expr = CompiledExpr::binop(BinOp::Gt, x_ref.clone(), five_mm, Type::Bool);
        let lt_expr = CompiledExpr::binop(BinOp::Lt, x_ref.clone(), six_mm, Type::Bool);

        // Objective: minimize(if x <= 6.2mm then 1e8 else x) — the same
        // cost-cliff shape as `defined_objective_at_fallback_returns_solved`,
        // scaled to this bracket. Flat (1e8) across the whole feasible region
        // plus a small buffer; attractive (the raw x value, dramatically
        // smaller than 1e8) just past the buffer — luring the optimizer past
        // the x<6mm boundary while chasing it.
        let buffer_threshold = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.0062,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let condition = CompiledExpr::binop(BinOp::Le, x_ref.clone(), buffer_threshold, Type::Bool);
        let then_branch = CompiledExpr::literal(Value::Real(1e8), Type::dimensionless_scalar());
        let else_branch = x_ref;
        let cond_hash = ContentHash::of(&[TAG_CONDITIONAL])
            .combine(condition.content_hash)
            .combine(then_branch.content_hash)
            .combine(else_branch.content_hash);
        let objective_expr = CompiledExpr {
            kind: CompiledExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: cond_hash,
        };
        let objective = ObjectiveSet::single(ObjectiveSense::Minimize, objective_expr);

        let mut current_values = ValueMap::new();
        current_values.insert(
            x_id.clone(),
            Value::Scalar {
                si_value: 0.0055,
                dimension: DimensionVector::LENGTH,
            },
        );

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            }],
            constraints: vec![
                (ConstraintNodeId::new("Part", 0), gt_expr),
                (ConstraintNodeId::new("Part", 1), lt_expr),
            ],
            current_values,
            objective: Some(objective),
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Infeasible { ref diagnostics } => {
                assert!(
                    diagnostics
                        .iter()
                        .any(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique)),
                    "expected ConstraintNonUnique for a bracket whose objective is flat \
                     within the feasible region (\u{a7}11.6: params differ post-perturbation \
                     and the objective ties, so neither well-determinedness test holds); got \
                     diagnostics: {diagnostics:?}"
                );
            }
            other => panic!(
                "expected Infeasible/ConstraintNonUnique for x \u{2208} (5mm, 6mm) \
                 [bounds: None] with a flat-within-bracket objective, got {:?}. If Solved, \
                 verify_uniqueness's perturbation anchor is still landing outside the \
                 feasible region under `effective_bounds` (pre-#5711 step-5) rather than the \
                 #5618 constraint-derived box.",
                other
            ),
        }
    }

    /// Pins the suppression rule (task #5711) — a genuinely SUBOPTIMAL
    /// incumbent must never be reported as non-unique — and, as a corollary,
    /// the open-interval / infimum-not-attained ruling: `x > 5mm AND x < 6mm`
    /// is an OPEN interval under `minimize(x)`, so the infimum (5mm) is never
    /// attained, there is no argmin, and "uniquely optimal" (\u{a7}11.6) is
    /// vacuous rather than false. `verify_uniqueness` must ABSTAIN (report
    /// "cannot prove non-unique") rather than manufacture a
    /// `ConstraintNonUnique` for a problem that simply has no optimum — and
    /// the `IncumbentSuboptimal` branch is what makes it abstain, for free,
    /// with no special-casing of open intervals.
    ///
    /// MUST be a unit test, not an integration test: the distinction is
    /// invisible through the public `SolveResult` — this fixture already
    /// reports `Solved` (i.e. `verify_uniqueness` already returns `true`)
    /// BOTH before and after step-5, via two entirely different mechanisms.
    /// Precedent for reaching into the private fn directly:
    /// `verify_uniqueness_skips_solve_core_when_param_missing`.
    ///
    /// Rebuilds the
    /// `warm_start_falls_back_to_initial_when_optimizer_drifts_infeasible`
    /// shape (`solver_integration.rs`) locally: `Part.x`,
    /// `Type::length()`, `bounds: Some((0.0, 0.1))`, `free: false`;
    /// `x > 5mm AND x < 6mm`; `minimize(x)`; `current_values: {x: 5.5mm}`.
    /// `solved = {x: 0.0055}` is that fixture's real, measured `Solved`
    /// result (the drift fallback returning the exact seed) — taken as a
    /// given incumbent here rather than re-derived, since the mechanism
    /// under test is `verify_uniqueness`'s re-solve, not the main solve.
    ///
    /// RED today (MEASURED, task #5711 pre-step-5 — not guessed): calling
    /// `super::verify_uniqueness` on this exact problem/incumbent under a
    /// WARN-capturing subscriber shows it reaches its `true` verdict via the
    /// inert `_ =>` arm — the `effective_bounds` anchor (`0.09`, i.e. 90mm)
    /// is so far outside the 1mm bracket that `solve_core`'s re-solve from
    /// there lands at `Infeasible` (measured max residual `5.00e-7` — just
    /// outside `FEASIBILITY_THRESHOLD`), so ZERO WARN events fire;
    /// `verify_uniqueness` only ever emits a DEBUG event on this path
    /// ("perturbed solve did not converge; assuming unique"). This is the
    /// part that makes the test RED: `capture.count()` is `0` today.
    ///
    /// AFTER step-5 wires the derived box, the SAME incumbent instead
    /// re-solves via a FEASIBLE anchor (MEASURED: derived box `(0.005,
    /// 0.006)`, anchor `0.0051`, re-solve lands `Solved` at exactly `0.0051`
    /// — strictly below the incumbent's `0.0055` under `minimize(x)`),
    /// `classify_uniqueness` returns `IncumbentSuboptimal`, and step-5 wires
    /// `verify_uniqueness` to emit exactly one WARN naming the suppression —
    /// so `capture.count() == 1` afterward. The (b) block below independently
    /// re-derives that same anchor/re-solve/verdict chain from the raw
    /// building blocks (`build_perturbation_anchors`, `solve_core`,
    /// `classify_uniqueness`), pinning the MECHANISM itself, not just its
    /// observable side effect.
    #[test]
    fn incumbent_suboptimal_is_suppressed_not_reported_non_unique() {
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{
            AutoParam, ObjectiveSense, ObjectiveSet, ResolutionProblem, SolveResult, Value,
            ValueMap,
        };
        use reify_test_support::{cnid, gt, literal, lt, mm, value_ref, vcid, warn_capturing_subscriber};

        use super::{
            UniquenessVerdict, build_perturbation_anchors, build_scoring_values,
            classify_uniqueness, derive_param_intervals, eval_objective_set, resolve_bounds,
            solve_core, verify_uniqueness,
        };

        let x_id = vcid("Part", "x");
        let x_ref = value_ref("Part", "x");
        let gt_expr = gt(x_ref.clone(), literal(mm(5.0)));
        let lt_expr = lt(x_ref.clone(), literal(mm(6.0)));
        let objective = ObjectiveSet::single(ObjectiveSense::Minimize, x_ref);

        let mut current_values = ValueMap::new();
        current_values.insert(x_id.clone(), mm(5.5));

        let problem = ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.1)),
                free: false,
            }],
            constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
            current_values,
            objective: Some(objective),
            functions: vec![].into(),
        };

        // This fixture's real measured Solved result (the drift fallback
        // returning the exact seed) — see
        // `warm_start_falls_back_to_initial_when_optimizer_drifts_infeasible` in
        // `solver_integration.rs`.
        let mut solved: std::collections::HashMap<ValueCellId, Value> =
            std::collections::HashMap::new();
        solved.insert(
            x_id.clone(),
            Value::Scalar {
                si_value: 0.0055,
                dimension: DimensionVector::LENGTH,
            },
        );

        // ---- (a)+(c): the REAL verify_uniqueness call, under a
        // WARN-capturing subscriber. (a) the verdict is `true` both before
        // and after step-5 — NOT what distinguishes this test (the public
        // SolveResult reads Solved either way). (c) the WARN count IS what
        // distinguishes it: 0 today (inert `_ =>` arm, DEBUG-only), 1 after
        // step-5 (explicit IncumbentSuboptimal suppression warning).
        let (subscriber, capture) = warn_capturing_subscriber();
        let unique = tracing::subscriber::with_default(subscriber, || {
            verify_uniqueness(&problem, &solved, None)
        });
        assert!(
            unique,
            "verify_uniqueness must return true for this incumbent both before and after \
             step-5 (the public verdict is unchanged — only the INTERNAL mechanism differs)"
        );
        capture.assert_count_and_any_message_contains(1, "IncumbentSuboptimal");

        // ---- (b): pin the mechanism directly — recompute the #5618
        // constraint-derived box and this fixture's perturbation anchor, and
        // confirm the ACTUAL re-solve from that anchor is what step-5's
        // wired classifier will see as `IncumbentSuboptimal`.
        let derived_box = resolve_bounds(
            &problem.auto_params,
            &derive_param_intervals(
                &problem.auto_params,
                &problem.constraints,
                &problem.current_values,
                &problem.functions,
                None,
            ),
            true,
        );
        let (anchor, missing) =
            build_perturbation_anchors(&problem.auto_params, &solved, &derived_box);
        assert!(missing.is_empty(), "x must not be missing from `solved`");
        assert!(
            anchor[0] > 0.005 && anchor[0] < 0.006,
            "the derived-box anchor must land INSIDE the (5mm, 6mm) bracket, proving the \
             mechanism is available once step-5 wires it in; got {anchor:?}"
        );

        let (perturbed_result, _meta) = solve_core(&problem, &anchor, None);
        let SolveResult::Solved {
            values: perturbed_values,
            ..
        } = perturbed_result
        else {
            panic!(
                "re-solving from the derived-box anchor must converge (Solved); got {:?}",
                perturbed_result
            );
        };

        let incumbent_scoring = build_scoring_values(
            &problem.current_values,
            &solved,
            &problem.dependent_cells,
            &problem.functions,
            None,
        );
        let perturbed_scoring = build_scoring_values(
            &problem.current_values,
            &perturbed_values,
            &problem.dependent_cells,
            &problem.functions,
            None,
        );
        let obj = problem
            .objective
            .as_ref()
            .expect("objective is Some in this fixture");
        let incumbent_score = eval_objective_set(obj, &incumbent_scoring, &problem.functions, None)
            .expect("numeric");
        let perturbed_score = eval_objective_set(obj, &perturbed_scoring, &problem.functions, None)
            .expect("numeric");
        assert!(
            perturbed_score < incumbent_score,
            "the re-solve must find a STRICTLY better point under minimize(x) \
             ({perturbed_score} should be < {incumbent_score}) — this is the premise that \
             makes the incumbent suboptimal rather than the constraints underdetermined"
        );

        let verdict = classify_uniqueness(&problem.auto_params, &solved, &perturbed_values, || {
            Some((incumbent_score, perturbed_score))
        });
        assert_eq!(
            verdict,
            UniquenessVerdict::IncumbentSuboptimal {
                incumbent: incumbent_score,
                perturbed: perturbed_score,
            },
            "params differ and the perturbed point scores strictly better — this MUST \
             classify as IncumbentSuboptimal, not NonUnique: the incumbent is not the \
             argmin, so this is an optimality finding, not evidence the constraints are \
             underdetermined"
        );
    }

    // ---- best_found_reason unit tests ----

    /// Deterministic unit test for best_found_reason (B1 sub-test).
    /// Tests the iteration-limit-vs-converged reason mapping without forcing
    /// the solver to hit MaxIters from a fixture.
    #[test]
    fn best_found_reason_iteration_limit_vs_converged() {
        use super::best_found_reason;
        use reify_ir::BestFoundReason;
        assert_eq!(
            best_found_reason(true),
            BestFoundReason::IterationLimit,
            "best_found_reason(true) must be BestFoundReason::IterationLimit"
        );
        assert_eq!(
            best_found_reason(false),
            BestFoundReason::ConvergedWithinBudget,
            "best_found_reason(false) must be BestFoundReason::ConvergedWithinBudget"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Constraint-derived bounds (task #5618) — `derive_param_intervals` /
    // `resolve_bounds` unit tests.
    //
    // `AutoParam.bounds` is always `None` in production (all three construction
    // sites in reify-eval hardcode it), so `effective_bounds` always degrades to
    // `default_bounds_for` — `(-1e6, 1e6)` for a dimensionless Real. These two
    // helpers recover a usable box from the inequality constraints instead.
    // ─────────────────────────────────────────────────────────────────────────

    /// Dimensionless (`Real`) auto param with `bounds: None` — the production
    /// shape, where `effective_bounds` degrades to `(-1e6, 1e6)`.
    fn real_auto_param(id: reify_core::ValueCellId) -> reify_ir::AutoParam {
        use reify_core::Type;
        reify_ir::AutoParam {
            id,
            param_type: Type::dimensionless_scalar(),
            bounds: None,
            free: true,
        }
    }

    /// Dimensionless literal expression.
    fn real_lit(v: f64) -> reify_ir::CompiledExpr {
        use reify_core::{DimensionVector, Type};
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Scalar {
                si_value: v,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Type::dimensionless_scalar(),
        )
    }

    /// Dimensionless `ValueRef` expression.
    fn real_ref(id: &reify_core::ValueCellId) -> reify_ir::CompiledExpr {
        use reify_core::Type;
        reify_ir::CompiledExpr::value_ref(id.clone(), Type::dimensionless_scalar())
    }

    /// `<id> OP <v>` as a dimensionless comparison.
    fn cmp_ref_lit(
        op: reify_ir::BinOp,
        id: &reify_core::ValueCellId,
        v: f64,
    ) -> reify_ir::CompiledExpr {
        use reify_core::Type;
        reify_ir::CompiledExpr::binop(op, real_ref(id), real_lit(v), Type::Bool)
    }

    /// `<a> - <b> OP <v>` — the slack shape emitted by `synthesise_floor_constraints`.
    fn cmp_sub_lit(
        op: reify_ir::BinOp,
        a: reify_ir::CompiledExpr,
        b: reify_ir::CompiledExpr,
        v: f64,
    ) -> reify_ir::CompiledExpr {
        use reify_core::Type;
        let slack =
            reify_ir::CompiledExpr::binop(reify_ir::BinOp::Sub, a, b, Type::dimensionless_scalar());
        reify_ir::CompiledExpr::binop(op, slack, real_lit(v), Type::Bool)
    }

    /// Wraps expressions into the `(ConstraintNodeId, CompiledExpr)` shape.
    fn as_constraints(
        exprs: Vec<reify_ir::CompiledExpr>,
    ) -> Vec<(reify_core::ConstraintNodeId, reify_ir::CompiledExpr)> {
        exprs
            .into_iter()
            .enumerate()
            .map(|(i, e)| (reify_core::ConstraintNodeId::new("Derive", i as u32), e))
            .collect()
    }

    /// Convenience: derive intervals for a single dimensionless auto param with
    /// an empty `current_values` map and no user functions.
    fn derive_one(
        id: &reify_core::ValueCellId,
        exprs: Vec<reify_ir::CompiledExpr>,
    ) -> super::DerivedInterval {
        let params = vec![real_auto_param(id.clone())];
        let constraints = as_constraints(exprs);
        let values = ValueMap::new();
        super::derive_param_intervals(&params, &constraints, &values, &[], None)
            .into_iter()
            .next()
            .expect("one interval per auto param")
    }

    /// (a) A plain `q >= 1.0` / `q <= 100.0` pair derives the raw constraint box,
    /// both sides non-strict.
    #[test]
    fn derive_intervals_two_sided_non_strict() {
        use reify_ir::BinOp;
        let q = reify_core::ValueCellId::new("Derive", "q");
        let iv = derive_one(
            &q,
            vec![
                cmp_ref_lit(BinOp::Ge, &q, 1.0),
                cmp_ref_lit(BinOp::Le, &q, 100.0),
            ],
        );
        assert_eq!(
            iv.lo,
            Some((1.0, false)),
            "`q >= 1.0` must derive a non-strict lower bound of 1.0"
        );
        assert_eq!(
            iv.hi,
            Some((100.0, false)),
            "`q <= 100.0` must derive a non-strict upper bound of 100.0"
        );
    }

    /// (a′) The reversed operand order (`1.0 <= q`, `100.0 >= q`) must derive the
    /// same box — the near operand can be on either side.
    #[test]
    fn derive_intervals_reversed_operand_order() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr};
        let q = reify_core::ValueCellId::new("Derive", "q");
        let le = CompiledExpr::binop(BinOp::Le, real_lit(1.0), real_ref(&q), Type::Bool);
        let ge = CompiledExpr::binop(BinOp::Ge, real_lit(100.0), real_ref(&q), Type::Bool);
        let iv = derive_one(&q, vec![le, ge]);
        assert_eq!(
            iv.lo,
            Some((1.0, false)),
            "`1.0 <= q` must derive a non-strict lower bound of 1.0"
        );
        assert_eq!(
            iv.hi,
            Some((100.0, false)),
            "`100.0 >= q` must derive a non-strict upper bound of 100.0"
        );
    }

    /// (b) The `Sub` slack shapes emitted by `synthesise_floor_constraints`:
    /// `q - 1.0 >= 0.02` → lo = 1.02;  `100.0 - q >= 2.0` → hi = 98.0.
    /// Deriving these is what recovers the FLOORED box (the clamp target).
    #[test]
    fn derive_intervals_floor_slack_shapes() {
        use reify_ir::BinOp;
        let q = reify_core::ValueCellId::new("Derive", "q");

        let lower = derive_one(
            &q,
            vec![cmp_sub_lit(BinOp::Ge, real_ref(&q), real_lit(1.0), 0.02)],
        );
        let lo = lower
            .lo
            .expect("`q - 1.0 >= 0.02` must derive a lower bound");
        assert!(
            (lo.0 - 1.02).abs() < 1e-12,
            "`q - 1.0 >= 0.02` must derive lo = 1.02, got {}",
            lo.0
        );
        assert!(!lo.1, "a `Ge`-sourced bound must be non-strict");
        assert_eq!(lower.hi, None, "no upper bound in this constraint set");

        let upper = derive_one(
            &q,
            vec![cmp_sub_lit(BinOp::Ge, real_lit(100.0), real_ref(&q), 2.0)],
        );
        let hi = upper
            .hi
            .expect("`100.0 - q >= 2.0` must derive an upper bound");
        assert!(
            (hi.0 - 98.0).abs() < 1e-12,
            "`100.0 - q >= 2.0` must derive hi = 98.0, got {}",
            hi.0
        );
        assert!(!hi.1, "a `Ge`-sourced bound must be non-strict");
        assert_eq!(upper.lo, None, "no lower bound in this constraint set");
    }

    /// (c) A strict `q > 1.0` derives lo = 1.0 flagged strict; `resolve_bounds`
    /// DROPS it under `include_strict = false` (a clamp target must never be a
    /// value at which the strict comparison is violated) and KEEPS it under
    /// `include_strict = true` (a seed has no such obligation).
    #[test]
    fn resolve_bounds_strict_excluded_from_clamp_kept_for_seed() {
        use reify_ir::BinOp;
        let q = reify_core::ValueCellId::new("Derive", "q");
        let params = vec![real_auto_param(q.clone())];
        let constraints = as_constraints(vec![cmp_ref_lit(BinOp::Gt, &q, 1.0)]);
        let values = ValueMap::new();
        let intervals = super::derive_param_intervals(&params, &constraints, &values, &[], None);
        assert_eq!(
            intervals[0].lo,
            Some((1.0, true)),
            "`q > 1.0` must derive lo = 1.0 flagged STRICT"
        );

        let (default_lo, default_hi) = super::default_bounds_for(&params[0].param_type);

        let clamp = super::resolve_bounds(&params, &intervals, false);
        assert_eq!(
            clamp[0],
            (default_lo, default_hi),
            "include_strict = false must DROP the Gt-sourced bound and keep the default low side"
        );

        let seed = super::resolve_bounds(&params, &intervals, true);
        assert_eq!(
            seed[0],
            (1.0, default_hi),
            "include_strict = true must KEEP the Gt-sourced bound as a seed bound"
        );
    }

    /// (d) Tightest wins when two constraints bound the same side.
    #[test]
    fn derive_intervals_tightest_bound_wins() {
        use reify_ir::BinOp;
        let q = reify_core::ValueCellId::new("Derive", "q");
        let iv = derive_one(
            &q,
            vec![
                cmp_ref_lit(BinOp::Ge, &q, 1.0),
                cmp_ref_lit(BinOp::Ge, &q, 7.5), // tighter low
                cmp_ref_lit(BinOp::Le, &q, 100.0),
                cmp_ref_lit(BinOp::Le, &q, 42.0), // tighter high
            ],
        );
        assert_eq!(
            iv.lo,
            Some((7.5, false)),
            "the tightest (largest) lower bound must win"
        );
        assert_eq!(
            iv.hi,
            Some((42.0, false)),
            "the tightest (smallest) upper bound must win"
        );
    }

    /// (e) SKIP rules — a far operand that references another auto param, a
    /// multi-auto shape, an `Eq`/`Ne` op, and a non-finite/Undef far operand all
    /// yield no bound. A bound that cannot be evaluated must never become a clamp.
    #[test]
    fn derive_intervals_skips_unusable_shapes() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr, Value};

        let q = reify_core::ValueCellId::new("Derive", "q");
        let p = reify_core::ValueCellId::new("Derive", "p");
        let params = vec![real_auto_param(q.clone()), real_auto_param(p.clone())];
        let values = ValueMap::new();

        // `q >= p` — far operand names another auto param.
        let q_ge_p = CompiledExpr::binop(BinOp::Ge, real_ref(&q), real_ref(&p), Type::Bool);
        // `q + p >= 10.0` — multi-auto near operand, not a recognised shape.
        let sum = CompiledExpr::binop(
            BinOp::Add,
            real_ref(&q),
            real_ref(&p),
            Type::dimensionless_scalar(),
        );
        let sum_ge = CompiledExpr::binop(BinOp::Ge, sum, real_lit(10.0), Type::Bool);
        // `q == 3.0` — Eq is not an inequality.
        let q_eq = cmp_ref_lit(BinOp::Eq, &q, 3.0);
        // `q != 4.0` — Ne is not an inequality.
        let q_ne = cmp_ref_lit(BinOp::Ne, &q, 4.0);
        // `q >= undef` — far operand does not evaluate to a finite f64.
        let undef = CompiledExpr::literal(Value::Undef, Type::dimensionless_scalar());
        let q_ge_undef = CompiledExpr::binop(BinOp::Ge, real_ref(&q), undef, Type::Bool);
        // `q <= inf` — far operand is non-finite.
        let q_le_inf = cmp_ref_lit(BinOp::Le, &q, f64::INFINITY);

        let constraints = as_constraints(vec![q_ge_p, sum_ge, q_eq, q_ne, q_ge_undef, q_le_inf]);
        let intervals = super::derive_param_intervals(&params, &constraints, &values, &[], None);

        assert_eq!(
            intervals[0],
            super::DerivedInterval::default(),
            "none of the skipped shapes may contribute a bound for q"
        );
        assert_eq!(
            intervals[1],
            super::DerivedInterval::default(),
            "none of the skipped shapes may contribute a bound for p"
        );
    }

    /// (f) `And` recursion collects from both branches, exactly like
    /// `collect_slack_terms` / `collect_floor_terms`.
    #[test]
    fn derive_intervals_recurses_into_and() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr};
        let q = reify_core::ValueCellId::new("Derive", "q");
        let conj = CompiledExpr::binop(
            BinOp::And,
            cmp_ref_lit(BinOp::Ge, &q, 1.0),
            cmp_ref_lit(BinOp::Le, &q, 100.0),
            Type::Bool,
        );
        let iv = derive_one(&q, vec![conj]);
        assert_eq!(
            iv.lo,
            Some((1.0, false)),
            "And recursion must collect the lower bound from the left branch"
        );
        assert_eq!(
            iv.hi,
            Some((100.0, false)),
            "And recursion must collect the upper bound from the right branch"
        );
    }

    /// (g) `resolve_bounds` composition: a derived side replaces the default
    /// side, an absent side keeps the default.
    #[test]
    fn resolve_bounds_composes_per_side_with_defaults() {
        use reify_ir::BinOp;
        let q = reify_core::ValueCellId::new("Derive", "q");
        let params = vec![real_auto_param(q.clone())];
        let (default_lo, default_hi) = super::default_bounds_for(&params[0].param_type);
        let values = ValueMap::new();

        // Lower only.
        let lower_only = as_constraints(vec![cmp_ref_lit(BinOp::Ge, &q, 1.0)]);
        let iv = super::derive_param_intervals(&params, &lower_only, &values, &[], None);
        assert_eq!(
            super::resolve_bounds(&params, &iv, false)[0],
            (1.0, default_hi),
            "a derived low side replaces the default low; the absent high side keeps the default"
        );

        // Upper only.
        let upper_only = as_constraints(vec![cmp_ref_lit(BinOp::Le, &q, 100.0)]);
        let iv = super::derive_param_intervals(&params, &upper_only, &values, &[], None);
        assert_eq!(
            super::resolve_bounds(&params, &iv, false)[0],
            (default_lo, 100.0),
            "a derived high side replaces the default high; the absent low side keeps the default"
        );

        // Neither.
        let iv = super::derive_param_intervals(&params, &[], &values, &[], None);
        assert_eq!(
            super::resolve_bounds(&params, &iv, false)[0],
            (default_lo, default_hi),
            "no derived side → the default box verbatim"
        );
    }

    /// (g′) An empty / inverted composed box falls back to the default bounds
    /// WHOLESALE. This is the guard that keeps a genuinely floor-empty problem
    /// (e.g. `x > 10mm ∧ x < 10.3mm`, whose floored pair inverts to
    /// lo 0.0102 > hi 0.0101) reporting `Infeasible` exactly as it does today,
    /// rather than clamping into a degenerate box.
    #[test]
    fn resolve_bounds_empty_box_falls_back_wholesale() {
        use reify_ir::BinOp;
        let q = reify_core::ValueCellId::new("Derive", "q");
        let params = vec![real_auto_param(q.clone())];
        let (default_lo, default_hi) = super::default_bounds_for(&params[0].param_type);
        let values = ValueMap::new();

        // Inverted: q >= 50.0 AND q <= 10.0.
        let inverted = as_constraints(vec![
            cmp_ref_lit(BinOp::Ge, &q, 50.0),
            cmp_ref_lit(BinOp::Le, &q, 10.0),
        ]);
        let iv = super::derive_param_intervals(&params, &inverted, &values, &[], None);
        assert_eq!(
            super::resolve_bounds(&params, &iv, false)[0],
            (default_lo, default_hi),
            "an inverted composed box must fall back to the default bounds WHOLESALE"
        );

        // Degenerate (lo == hi): q >= 5.0 AND q <= 5.0.
        let degenerate = as_constraints(vec![
            cmp_ref_lit(BinOp::Ge, &q, 5.0),
            cmp_ref_lit(BinOp::Le, &q, 5.0),
        ]);
        let iv = super::derive_param_intervals(&params, &degenerate, &values, &[], None);
        assert_eq!(
            super::resolve_bounds(&params, &iv, false)[0],
            (default_lo, default_hi),
            "a zero-width composed box (!(lo < hi)) must fall back to the default bounds"
        );
    }

    // ---- extract_initial_point derived seeding (task #5618, step-3/step-4) ----

    /// Length auto param with `bounds: None` — the production shape.
    fn length_auto(id: reify_core::ValueCellId) -> reify_ir::AutoParam {
        use reify_core::Type;
        reify_ir::AutoParam {
            id,
            param_type: Type::length(),
            bounds: None,
            free: true,
        }
    }

    /// `<id> OP <v>` as a Length comparison (`v` in SI metres).
    fn length_cmp(
        op: reify_ir::BinOp,
        id: &reify_core::ValueCellId,
        v: f64,
    ) -> reify_ir::CompiledExpr {
        use reify_core::{DimensionVector, Type};
        reify_ir::CompiledExpr::binop(
            op,
            reify_ir::CompiledExpr::value_ref(id.clone(), Type::length()),
            reify_ir::CompiledExpr::literal(
                reify_ir::Value::Scalar {
                    si_value: v,
                    dimension: DimensionVector::LENGTH,
                },
                Type::length(),
            ),
            Type::Bool,
        )
    }

    /// Objective-free `ResolutionProblem` for `extract_initial_point` seeding tests.
    fn seed_problem(
        auto_params: Vec<reify_ir::AutoParam>,
        exprs: Vec<reify_ir::CompiledExpr>,
        current_values: ValueMap,
    ) -> ResolutionProblem {
        ResolutionProblem {
            dependent_cells: Vec::new(),
            auto_params,
            constraints: as_constraints(exprs),
            current_values,
            objective: None,
            functions: vec![].into(),
        }
    }

    /// (a) Two derived sides → the seed is the derived box's MIDPOINT, not the
    /// fixed `0.01` fallback. This is the defect's proximate cause: `q >= 1 ∧
    /// q <= 100` seeded at 0.01, outside the synthesised floor's `[1.02, …]`
    /// window, so Nelder-Mead approached the region from the wrong side.
    #[test]
    fn extract_initial_point_seeds_derived_midpoint() {
        use reify_ir::BinOp;
        let q = reify_core::ValueCellId::new("Seed", "q");
        let problem = seed_problem(
            vec![real_auto_param(q.clone())],
            vec![
                cmp_ref_lit(BinOp::Ge, &q, 1.0),
                cmp_ref_lit(BinOp::Le, &q, 100.0),
            ],
            ValueMap::new(),
        );
        let seed = super::extract_initial_point(&problem, None);
        assert!(
            (seed[0] - 50.5).abs() < 1e-12,
            "expected the midpoint of the derived box [1, 100] = 50.5, got {} \
             (0.01 means the derived box was ignored)",
            seed[0]
        );
    }

    /// (b) Exactly one derived side → nudge INWARD from that bound by
    /// `max(SEED_NUDGE_REL·|v|, SEED_NUDGE_ABS)`, never the midpoint of the
    /// half-open box. For `thickness > 1mm` the derived box is
    /// `[0.001, default_hi = 10.0]`, whose midpoint would be an absurd 5 m.
    #[test]
    fn extract_initial_point_one_sided_seed_nudges_inward() {
        use reify_ir::BinOp;
        let t = reify_core::ValueCellId::new("Seed", "thickness");
        let problem = seed_problem(
            vec![length_auto(t.clone())],
            vec![length_cmp(BinOp::Gt, &t, 0.001)],
            ValueMap::new(),
        );
        let seed = super::extract_initial_point(&problem, None);
        let expected = 0.001 + (super::SEED_NUDGE_REL * 0.001).max(super::SEED_NUDGE_ABS);
        assert!(
            (seed[0] - expected).abs() < 1e-15,
            "expected a one-sided seed nudged just inside the 1mm bound ({expected} m), got {} m",
            seed[0]
        );
        assert!(
            seed[0] < 0.002,
            "the one-sided seed must NOT be the midpoint of [0.001, 10.0] (= 5 m); got {} m",
            seed[0]
        );
    }

    /// (c) An existing `current_values` entry still wins over any derived box —
    /// the precedence head of the chain is unchanged.
    #[test]
    fn extract_initial_point_current_value_still_wins() {
        use reify_core::DimensionVector;
        use reify_ir::{BinOp, Value};
        let q = reify_core::ValueCellId::new("Seed", "q");
        let mut current = ValueMap::new();
        current.insert(
            q.clone(),
            Value::Scalar {
                si_value: 7.25,
                dimension: DimensionVector::DIMENSIONLESS,
            },
        );
        let problem = seed_problem(
            vec![real_auto_param(q.clone())],
            vec![
                cmp_ref_lit(BinOp::Ge, &q, 1.0),
                cmp_ref_lit(BinOp::Le, &q, 100.0),
            ],
            current,
        );
        let seed = super::extract_initial_point(&problem, None);
        assert_eq!(
            seed[0], 7.25,
            "an existing current value must still win over the derived box"
        );
    }

    /// (d) An explicit `AutoParam.bounds` midpoint still wins over the derived
    /// box — the derived arm is inserted BELOW it in the fall-through chain.
    #[test]
    fn extract_initial_point_explicit_bounds_still_win() {
        use reify_ir::BinOp;
        let q = reify_core::ValueCellId::new("Seed", "q");
        let mut param = real_auto_param(q.clone());
        param.bounds = Some((10.0, 20.0));
        let problem = seed_problem(
            vec![param],
            vec![
                cmp_ref_lit(BinOp::Ge, &q, 1.0),
                cmp_ref_lit(BinOp::Le, &q, 100.0),
            ],
            ValueMap::new(),
        );
        let seed = super::extract_initial_point(&problem, None);
        assert_eq!(
            seed[0], 15.0,
            "an explicit bounds midpoint must still win over the derived box"
        );
    }

    /// (e) No usable constraint → the fixed `0.01` fallback, verbatim as today.
    /// Covers an empty constraint list, an `Eq`-only list, and a multi-auto shape
    /// that no rule recognises.
    #[test]
    fn extract_initial_point_no_usable_constraint_keeps_fixed_default() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr};

        let q = reify_core::ValueCellId::new("Seed", "q");
        let p = reify_core::ValueCellId::new("Seed", "p");

        // Empty constraint list.
        let empty = seed_problem(vec![real_auto_param(q.clone())], vec![], ValueMap::new());
        assert_eq!(
            super::extract_initial_point(&empty, None)[0],
            0.01,
            "an unconstrained auto must still seed at exactly 0.01"
        );

        // Eq only — not an inequality, contributes no bound.
        let eq_only = seed_problem(
            vec![real_auto_param(q.clone())],
            vec![cmp_ref_lit(BinOp::Eq, &q, 3.0)],
            ValueMap::new(),
        );
        assert_eq!(
            super::extract_initial_point(&eq_only, None)[0],
            0.01,
            "an Eq-only constraint set must still seed at exactly 0.01"
        );

        // Multi-auto shape (`q + p >= 10`) — no recognised linear-in-one-auto form.
        let sum = CompiledExpr::binop(
            BinOp::Add,
            real_ref(&q),
            real_ref(&p),
            Type::dimensionless_scalar(),
        );
        let multi = seed_problem(
            vec![real_auto_param(q.clone()), real_auto_param(p.clone())],
            vec![CompiledExpr::binop(
                BinOp::Ge,
                sum,
                real_lit(10.0),
                Type::Bool,
            )],
            ValueMap::new(),
        );
        let seed = super::extract_initial_point(&multi, None);
        assert_eq!(
            seed,
            vec![0.01, 0.01],
            "a multi-auto shape must contribute no bound; both autos seed at 0.01"
        );
    }

    /// (f) The one-sided nudge is clamped inside the composed box, so it can never
    /// cross the opposing default bound.
    #[test]
    fn extract_initial_point_one_sided_nudge_clamped_into_box() {
        use reify_ir::BinOp;

        // High side: `x >= 9.5 m` with default Length hi = 10.0 m.
        // nudge = 0.1 × 9.5 = 0.95 → 10.45 m, past the box top.
        let x = reify_core::ValueCellId::new("Seed", "x");
        let high = seed_problem(
            vec![length_auto(x.clone())],
            vec![length_cmp(BinOp::Ge, &x, 9.5)],
            ValueMap::new(),
        );
        let (box_lo, box_hi) = (9.5_f64, 10.0_f64);
        let seed = super::extract_initial_point(&high, None)[0];
        assert!(
            (box_lo..=box_hi).contains(&seed),
            "the nudged seed must be clamped inside [{box_lo}, {box_hi}], got {seed}"
        );

        // Low side: `q <= -950000` with default dimensionless lo = -1e6.
        // nudge = 0.1 × 950000 = 95000 → -1045000, past the box bottom.
        let q = reify_core::ValueCellId::new("Seed", "q");
        let low = seed_problem(
            vec![real_auto_param(q.clone())],
            vec![cmp_ref_lit(BinOp::Le, &q, -950_000.0)],
            ValueMap::new(),
        );
        let seed = super::extract_initial_point(&low, None)[0];
        assert!(
            (-1e6..=-950_000.0).contains(&seed),
            "the nudged seed must be clamped inside [-1e6, -950000], got {seed}"
        );
    }

    // ---- per-trial dependent-cell recompute (task #5189 β, PRD §6.2 / §7) ----

    /// The minimal whole-model joint-drive shape, hand-built.
    ///
    /// Mirrors `examples/whole_model_joint_drive.ri`: a `Costed` child whose
    /// derived cell `line_cost = unit_cost * quantity_produced` is a DEPENDENT
    /// cell — a non-auto value that is a function of the cluster's auto — and a
    /// parent objective that reads `line_cost`, NEVER the auto directly.
    ///
    /// That indirection is the whole point of the seam: a solver that does not
    /// recompute dependent cells per trial evaluates the objective against
    /// `line_cost`'s STALE base value, so the objective is CONSTANT in the auto
    /// and Nelder-Mead has no gradient to follow.
    ///
    /// Returns `(auto_params, constraints, base_values, dependent_cells, objective)`.
    /// `base_values` deliberately seeds `line_cost` with a stale number that
    /// matches NO trial point, so any test reading the stale value gets an
    /// unmistakable wrong answer rather than a coincidentally-right one.
    #[allow(clippy::type_complexity)]
    fn joint_drive_dependent_cell_fixture() -> (
        Vec<reify_ir::AutoParam>,
        Vec<(reify_core::ConstraintNodeId, reify_ir::CompiledExpr)>,
        ValueMap,
        Vec<(reify_core::ValueCellId, reify_ir::CompiledExpr)>,
        reify_ir::ObjectiveSet,
    ) {
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, ObjectiveSense, ObjectiveSet, Value};

        let money = Type::Scalar {
            dimension: DimensionVector::MONEY,
        };
        let q_id = ValueCellId::new("Rivet", "quantity_produced");
        let unit_cost_id = ValueCellId::new("Rivet", "unit_cost");
        let line_cost_id = ValueCellId::new("Rivet", "line_cost");

        let mut base = ValueMap::new();
        base.insert(
            unit_cost_id.clone(),
            Value::Scalar {
                si_value: 0.5,
                dimension: DimensionVector::MONEY,
            },
        );
        // STALE: matches no trial point used by any test below.
        base.insert(
            line_cost_id.clone(),
            Value::Scalar {
                si_value: 999.0,
                dimension: DimensionVector::MONEY,
            },
        );

        let auto_params = vec![AutoParam {
            id: q_id.clone(),
            param_type: Type::dimensionless_scalar(),
            bounds: Some((1.0, 100.0)),
            free: true,
        }];

        // Trivially satisfied at every trial point used below (q >= 1.0), so
        // the cost function's violation term stays 0 and the objective term is
        // the only thing that can vary.
        let one = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Type::dimensionless_scalar(),
        );
        let constraints = vec![(
            ConstraintNodeId::new("Rivet", 0),
            CompiledExpr::binop(
                BinOp::Ge,
                CompiledExpr::value_ref(q_id.clone(), Type::dimensionless_scalar()),
                one,
                Type::Bool,
            ),
        )];

        // line_cost = unit_cost * quantity_produced — the stdlib `Costed` Let.
        let dependent_cells = vec![(
            line_cost_id.clone(),
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(unit_cost_id, money.clone()),
                CompiledExpr::value_ref(q_id, Type::dimensionless_scalar()),
                money.clone(),
            ),
        )];

        let objective = ObjectiveSet::single(
            ObjectiveSense::Minimize,
            CompiledExpr::value_ref(line_cost_id, money),
        );

        (auto_params, constraints, base, dependent_cells, objective)
    }

    /// BT-1 (PRD §7) — per-trial recompute makes the objective NON-CONSTANT.
    ///
    /// The objective reads only `line_cost`, which is not an auto. Without the
    /// per-trial fold, `line_cost` keeps its stale base value at every trial
    /// point, so the objective is a constant function of the auto and the
    /// solver cannot minimise it. Both halves are pinned: the helper itself and
    /// the hot Nelder-Mead path through `ConstraintCostFunction::cost`.
    #[test]
    fn dependent_cells_make_the_objective_vary_with_the_auto() {
        use super::{
            ConstraintCostFunction, UNDEF_OBJECTIVE_PENALTY, build_trial_values, eval_objective_set,
        };
        use argmin::core::CostFunction;

        let (auto_params, constraints, base, dependent_cells, objective) =
            joint_drive_dependent_cell_fixture();

        // ---- half 1: the helper folds, so the objective moves ----
        let lo = build_trial_values(&base, &auto_params, &[2.0], &dependent_cells, &[], None);
        let hi = build_trial_values(&base, &auto_params, &[8.0], &dependent_cells, &[], None);

        let obj_lo = eval_objective_set(&objective, &lo, &[], None)
            .expect("objective must be numeric at q=2 once line_cost is folded");
        let obj_hi = eval_objective_set(&objective, &hi, &[], None)
            .expect("objective must be numeric at q=8 once line_cost is folded");

        // 0.5 USD * q — the closed form of the stdlib `Costed` line_cost Let.
        assert!(
            (obj_lo - 1.0).abs() < 1e-12,
            "objective at q=2 must be unit_cost*q = 0.5*2 = 1.0; got {obj_lo}. \
             999.0 here means `line_cost` was read STALE from the base map \
             instead of being recomputed for the trial point."
        );
        assert!(
            (obj_hi - 4.0).abs() < 1e-12,
            "objective at q=8 must be unit_cost*q = 0.5*8 = 4.0; got {obj_hi}"
        );
        assert!(
            obj_lo < obj_hi,
            "the objective must be STRICTLY increasing in the auto \
             (0.5*2 < 0.5*8); got {obj_lo} vs {obj_hi} — a constant objective \
             is exactly the pre-fold failure this test exists to catch"
        );
        for v in [obj_lo, obj_hi] {
            assert!(
                v < UNDEF_OBJECTIVE_PENALTY,
                "objective must be a real number, not the Undef sentinel; got {v}"
            );
        }

        // ---- half 2: the hot Nelder-Mead path folds too ----
        // Pinning only the helper would leave `cost()` free to keep calling an
        // unfolded variant, which is precisely the divergence β must prevent.
        let cost_fn = ConstraintCostFunction {
            auto_params: &auto_params,
            constraints: &constraints,
            base_values: &base,
            objective: Some(&objective),
            functions: &[],
            // Task #5618: the clamp box is supplied by the caller. This is what
            // `resolve_bounds` yields for the fixture's lone `q >= 1.0` composed
            // against the dimensionless default box — so q=2 and q=8 are both
            // strictly inside it and contribute no bound penalty, keeping the
            // objective the only varying term this test is measuring.
            bounds: &[(1.0, 1e6)],
            dependent_cells: &dependent_cells,
            dispatch: None,
        };
        let cost_lo = cost_fn.cost(&vec![2.0]).expect("cost at q=2");
        let cost_hi = cost_fn.cost(&vec![8.0]).expect("cost at q=8");

        assert!(
            cost_lo < cost_hi,
            "ConstraintCostFunction::cost must be strictly increasing in the \
             auto for this fixture (both trial points are feasible and in \
             bounds, so the objective term is the only varying term); \
             got {cost_lo} vs {cost_hi}"
        );
        assert!(
            (cost_hi - cost_lo - 3.0).abs() < 1e-9,
            "the cost gap must be exactly the objective gap (4.0 - 1.0 = 3.0) \
             since violation and bound penalties are both zero here; \
             got {}",
            cost_hi - cost_lo
        );
        assert!(
            cost_lo < UNDEF_OBJECTIVE_PENALTY,
            "cost must not be the Undef sentinel; got {cost_lo}"
        );
    }

    /// Assert two `ValueMap`s hold exactly the same keys bound to exactly the
    /// same values — the teeth behind BT-2's "byte-identical" claim (a
    /// per-key spot check would not catch an EXTRA key the fold inserted).
    #[track_caller]
    fn assert_same_value_map(actual: &ValueMap, expected: &ValueMap, ctx: &str) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{ctx}: key COUNT differs — actual {:?} vs expected {:?}",
            actual.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            expected.iter().map(|(k, _)| k).collect::<Vec<_>>(),
        );
        for (id, want) in expected.iter() {
            let got = actual
                .get(id)
                .unwrap_or_else(|| panic!("{ctx}: expected key {id:?} is missing"));
            assert_eq!(got, want, "{ctx}: value at {id:?} differs");
        }
    }

    /// BT-2 (PRD §6.2 second INVARIANT) — an empty `dependent_cells` leaves
    /// `build_trial_values` byte-identical to its pre-β 3-arg behaviour.
    ///
    /// This is the regression fence for every non-clustered solve, which is
    /// almost all of them: they must take exactly the path they took before.
    /// The three legacy `build_trial_values_*` tests above already witness the
    /// empty case per-key; this one adds whole-map equality, so an extra key
    /// leaking in from a future fold cannot slip through.
    #[test]
    fn empty_dependent_cells_leaves_build_trial_values_byte_identical() {
        use super::build_trial_values;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, Value};

        let thickness_id = ValueCellId::new("Bracket", "thickness");
        let angle_id = ValueCellId::new("Bracket", "angle");
        let width_id = ValueCellId::new("Bracket", "width");

        let mut base = ValueMap::new();
        base.insert(
            width_id.clone(),
            Value::Scalar {
                si_value: 0.080,
                dimension: DimensionVector::LENGTH,
            },
        );

        let params = vec![
            AutoParam {
                id: thickness_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            },
            AutoParam {
                id: angle_id.clone(),
                param_type: Type::angle(),
                bounds: Some((0.0, std::f64::consts::PI)),
                free: false,
            },
        ];

        // Hand-built expectation: base ++ the trial autos, nothing else.
        let mut expected = base.clone();
        expected.insert(
            thickness_id,
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
        );
        expected.insert(
            angle_id,
            Value::Scalar {
                si_value: 1.2,
                dimension: DimensionVector::ANGLE,
            },
        );

        assert_same_value_map(
            &build_trial_values(&base, &params, &[0.005, 1.2], &[], &[], None),
            &expected,
            "multi-param, empty dependent_cells",
        );

        // Empty params AND empty dependent_cells: the base map, untouched.
        assert_same_value_map(
            &build_trial_values(&base, &[], &[], &[], &[], None),
            &base,
            "empty params, empty dependent_cells",
        );
    }

    /// BT-2 (PRD §7) — an empty `dependent_cells` preserves the LEGACY cost
    /// surface through `ConstraintCostFunction::cost`, not just the helper.
    ///
    /// Uses the same joint-drive fixture as BT-1, so the two tests read as a
    /// matched pair and the difference is unmistakable: WITH the fold the
    /// objective moves (1.0 → 4.0); WITHOUT dependent cells it is pinned at
    /// `line_cost`'s stale base value at every trial point. That constant is
    /// precisely the pre-β behaviour every non-clustered solve must keep.
    #[test]
    fn empty_dependent_cells_preserves_the_legacy_cost_surface() {
        use super::ConstraintCostFunction;
        use argmin::core::CostFunction;

        let (auto_params, constraints, base, _dependent_cells, objective) =
            joint_drive_dependent_cell_fixture();

        let cost_fn = ConstraintCostFunction {
            auto_params: &auto_params,
            constraints: &constraints,
            base_values: &base,
            objective: Some(&objective),
            functions: &[],
            // Task #5618: same clamp box as the sibling test above — see there
            // for why (1.0, 1e6) is the production-faithful value for this
            // fixture, and why it must not clamp q=2 or q=8.
            bounds: &[(1.0, 1e6)],
            dependent_cells: &[],
            dispatch: None,
        };

        let cost_lo = cost_fn.cost(&vec![2.0]).expect("cost at q=2");
        let cost_hi = cost_fn.cost(&vec![8.0]).expect("cost at q=8");

        assert!(
            (cost_lo - 999.0).abs() < 1e-9,
            "with NO dependent cells the objective must read `line_cost`'s \
             stale base value (999.0) verbatim — the pre-β surface; got {cost_lo}"
        );
        assert!(
            (cost_lo - cost_hi).abs() < 1e-12,
            "with NO dependent cells the cost must be CONSTANT in the auto \
             (that constancy IS the legacy behaviour, and is exactly what the \
              fold exists to remove when dependent cells ARE present); \
             got {cost_lo} vs {cost_hi}"
        );
    }

    /// BT-2 never-overwrite-auto INVARIANT (PRD §6.2 first INVARIANT).
    ///
    /// Hands the fold a hostile/malformed `dependent_cells` list whose entry id
    /// COLLIDES with an auto param — a list reify-eval's `build_dependent_cells`
    /// would never emit, since stage (a) drops autos by construction. The trial
    /// auto scalar must survive: silently clobbering it would corrupt the point
    /// Nelder-Mead thinks it is evaluating, and the corruption would be
    /// invisible (the solver would report a solved auto it never actually
    /// tested).
    ///
    /// This guards against upstream membership DRIFT, not against today's
    /// contract — which is exactly why it must be enforced rather than assumed.
    ///
    /// The guard is profile-split, and this one test pins BOTH halves rather
    /// than taking either on trust — the `should_panic` attribute is itself
    /// `cfg_attr`-gated on `debug_assertions`, so the same body asserts a
    /// different contract per profile:
    ///
    /// * debug (`cargo test`) — the `debug_assert!` fires, and the expected
    ///   panic substring pins that the alarm NAMES the offending cell. A
    ///   membership regression in reify-eval must not reach production quietly.
    /// * release — there is no alarm, so the entry is skipped, the body runs to
    ///   completion, and its assertions pin that the trial scalar survived.
    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "collides with an auto param")
    )]
    fn fold_must_never_overwrite_an_auto_param() {
        use super::build_trial_values;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let q_id = ValueCellId::new("Rivet", "quantity_produced");
        let unit_cost_id = ValueCellId::new("Rivet", "unit_cost");

        let mut base = ValueMap::new();
        base.insert(
            unit_cost_id.clone(),
            Value::Scalar {
                si_value: 0.5,
                dimension: DimensionVector::MONEY,
            },
        );

        let auto_params = vec![AutoParam {
            id: q_id.clone(),
            param_type: Type::dimensionless_scalar(),
            bounds: Some((1.0, 100.0)),
            free: true,
        }];

        // HOSTILE: a dependent cell keyed on the AUTO's own id. Evaluating it
        // would yield 0.5 * 0.5 = 0.25, which is neither trial point — so a
        // clobber is unmistakable.
        let money = Type::Scalar {
            dimension: DimensionVector::MONEY,
        };
        let hostile = vec![(
            q_id.clone(),
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(unit_cost_id.clone(), money.clone()),
                CompiledExpr::value_ref(unit_cost_id, money),
                Type::dimensionless_scalar(),
            ),
        )];

        for trial in [2.0_f64, 8.0_f64] {
            let values = build_trial_values(&base, &auto_params, &[trial], &hostile, &[], None);
            match values.get(&q_id) {
                Some(&Value::Scalar { si_value, .. }) => assert!(
                    (si_value - trial).abs() < 1e-12,
                    "the fold must NEVER overwrite an auto param's trial \
                     scalar: expected {trial}, got {si_value}. A dependent-cell \
                     id colliding with an auto id means upstream membership \
                     drifted; the trial point must still win."
                ),
                other => panic!("expected a Scalar at the auto id, got {other:?}"),
            }
        }
    }

    /// BT-7(b) (PRD §5 decision 5) — the solver-side half of the `@optimized`
    /// exclusion: a cell that reify-eval's membership rule left OUT of
    /// `dependent_cells` must keep its base value across every trial point.
    ///
    /// An `@optimized` cell's value comes from the compute-dispatch registry, and
    /// the contract is a MEMBERSHIP rule: reify-eval decides, once, which cells the
    /// solver may re-fold, and an `@optimized` cell is not one of them — its
    /// dispatched result is authoritative for the whole solve and must survive every
    /// trial point untouched. (Task #4880 note: `build_trial_values` no longer
    /// *inherently* lacks a registry — it now folds through [`ctx_with`], which can
    /// carry a `reify_ir::ComputeDispatch` hook. That does not weaken this test: the
    /// invariant asserted here is exclusion from `dependent_cells`, not the absence
    /// of a dispatcher.) The membership rule
    /// (asserted end-to-end by reify-eval's
    /// `dependent_cells_excludes_optimized_userfunctioncall_cell`) keeps it out
    /// of the list; THIS test pins the consequence at the fold: absent means
    /// frozen, with no bypass path back in.
    #[test]
    fn fold_leaves_cells_absent_from_dependent_cells_untouched() {
        use super::build_trial_values;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, Value};

        let q_id = ValueCellId::new("Rivet", "quantity_produced");
        let unit_cost_id = ValueCellId::new("Rivet", "unit_cost");
        let line_cost_id = ValueCellId::new("Rivet", "line_cost");
        // Stands in for an @optimized cell: excluded from `dependent_cells`,
        // its value having come from the compute-dispatch registry.
        let dispatched_id = ValueCellId::new("Rivet", "opt_cost");

        let money = Type::Scalar {
            dimension: DimensionVector::MONEY,
        };

        let mut base = ValueMap::new();
        base.insert(
            unit_cost_id.clone(),
            Value::Scalar {
                si_value: 0.5,
                dimension: DimensionVector::MONEY,
            },
        );
        base.insert(
            dispatched_id.clone(),
            Value::Scalar {
                si_value: 42.0,
                dimension: DimensionVector::MONEY,
            },
        );

        let auto_params = vec![AutoParam {
            id: q_id.clone(),
            param_type: Type::dimensionless_scalar(),
            bounds: Some((1.0, 100.0)),
            free: true,
        }];

        // Only the plain coupled cell is present — mirroring what membership
        // emits once the @optimized cell has been dropped.
        let dependent_cells = vec![(
            line_cost_id.clone(),
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(unit_cost_id, money.clone()),
                CompiledExpr::value_ref(q_id, Type::dimensionless_scalar()),
                money,
            ),
        )];

        for trial in [2.0_f64, 8.0_f64] {
            let values =
                build_trial_values(&base, &auto_params, &[trial], &dependent_cells, &[], None);

            match values.get(&dispatched_id) {
                Some(&Value::Scalar { si_value, .. }) => assert!(
                    (si_value - 42.0).abs() < 1e-12,
                    "a cell absent from `dependent_cells` must keep its base \
                     (compute-dispatched) value at trial q={trial}: expected \
                     42.0, got {si_value}"
                ),
                other => panic!("expected the dispatched Scalar, got {other:?}"),
            }

            // Sanity: the cell that IS listed did move, so the test is not
            // passing merely because the fold never ran.
            match values.get(&line_cost_id) {
                Some(&Value::Scalar { si_value, .. }) => assert!(
                    (si_value - 0.5 * trial).abs() < 1e-12,
                    "the LISTED cell must be recomputed at q={trial}: expected \
                     {}, got {si_value}",
                    0.5 * trial
                ),
                other => panic!("expected a Scalar at line_cost, got {other:?}"),
            }
        }
    }

    // ---- ComputeDispatch hook tests (step-5 RED / step-6 GREEN, task #4880) ----
    //
    // Hand-builds a single-param FEA-shaped problem: `stress(t) < LIMIT` where `stress`
    // is an `@optimized("test::stress")` stub whose body reduces to `Undef` (mirroring
    // what `solve_elastic_static` does with no dispatcher attached), plus `minimize t`.
    // A `CountingDispatch` mock resolves `"test::stress"` to `K / t`
    // (monotone-decreasing in t), so the constraint binds at a unique interior
    // t* = K / LIMIT when — and only when — the hook is actually threaded into the
    // cost loop.

    /// A [`reify_ir::ComputeDispatch`] that resolves exactly `"test::stress"` to
    /// `K / t` (reading trial `t` from `args[0]`), counting how many times it was
    /// asked to resolve that target. Defers (`None`) for every other target.
    struct CountingDispatch {
        calls: std::sync::atomic::AtomicUsize,
        k: f64,
    }

    impl reify_ir::ComputeDispatch for CountingDispatch {
        fn dispatch(&self, target: &str, args: &[reify_ir::Value]) -> Option<reify_ir::Value> {
            if target != "test::stress" {
                return None;
            }
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let t = args.first()?.as_f64()?;
            Some(reify_ir::Value::Scalar {
                si_value: self.k / t,
                dimension: reify_core::DimensionVector::DIMENSIONLESS,
            })
        }
    }

    /// Builds the shared `stress(t) < LIMIT`, `minimize t` fixture. `K` / `LIMIT` are
    /// chosen so the binding point `t* = K / LIMIT = 0.25` sits strictly inside the
    /// declared bounds `(0.001, 1.0)` (and away from their `0.5005` midpoint, so a
    /// pass that actually reaches the optimum is distinguishable from one that
    /// merely reports the unmoved initial guess).
    fn fea_binding_problem() -> (reify_core::ValueCellId, ResolutionProblem) {
        use reify_core::{ConstraintNodeId, Type, ValueCellId, hash::ContentHash};
        use reify_ir::{
            AutoParam, BinOp, CompiledExpr, CompiledFnBody, CompiledFunction, ObjectiveSense,
            ObjectiveSet, Value,
        };

        let params = vec![("t".to_string(), Type::length())];
        let stress_fn = CompiledFunction {
            name: "stress".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
            return_type: Type::dimensionless_scalar(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::literal(Value::Undef, Type::dimensionless_scalar()),
            },
            content_hash: ContentHash::of(b"step5_fea_binding_stress_stub"),
            annotations: vec![],
            optimized_target: Some("test::stress".to_string()),
            type_params: vec![],
        };

        let t_id = ValueCellId::new("Bracket", "t");
        let t_ref = CompiledExpr::value_ref(t_id.clone(), Type::length());
        let stress_call = CompiledExpr::user_function_call(
            "stress".to_string(),
            vec![t_ref.clone()],
            Type::dimensionless_scalar(),
        );
        let limit_lit = CompiledExpr::literal(
            Value::Scalar {
                si_value: 4.0, // LIMIT; with K = 1.0 below, t* = K / LIMIT = 0.25
                dimension: reify_core::DimensionVector::DIMENSIONLESS,
            },
            Type::dimensionless_scalar(),
        );
        let lt_expr = CompiledExpr::binop(BinOp::Lt, stress_call, limit_lit, Type::Bool);
        let objective = ObjectiveSet::single(ObjectiveSense::Minimize, t_ref);

        let problem = ResolutionProblem {
            auto_params: vec![AutoParam {
                id: t_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 1.0)),
                free: false,
            }],
            constraints: vec![(ConstraintNodeId::new("Bracket", 0), lt_expr)],
            current_values: ValueMap::new(),
            objective: Some(objective),
            functions: vec![stress_fn].into(),
            dependent_cells: Vec::new(),
        };
        (t_id, problem)
    }

    #[test]
    fn dispatch_hook_steers_convergence_to_fea_binding_point() {
        use crate::DimensionalSolver;
        use reify_ir::RankedSolveResult;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let (t_id, problem) = fea_binding_problem();
        let (lo, hi) = (0.001, 1.0);
        let solver = DimensionalSolver;

        // (a) WITH the dispatch hook: stress(t) resolves to a real, thickness-varying
        // value inside the cost loop, so `stress(t) < LIMIT` is a real constraint that
        // binds at the interior optimum t* = K / LIMIT.
        let mock = CountingDispatch {
            calls: AtomicUsize::new(0),
            k: 1.0,
        };
        match solver.solve_with_dispatch(&problem, Some(&mock)) {
            SolveResult::Solved { values, .. } => {
                let t = values
                    .get(&t_id)
                    .expect("t should be in the solution")
                    .as_f64()
                    .expect("t should be numeric");
                assert!(
                    t > lo && t < hi,
                    "solve_with_dispatch should converge to a t strictly interior to \
                     bounds ({lo}, {hi}); got {t}"
                );
            }
            other => panic!(
                "expected Solved once the dispatch hook is wired into the cost loop; got {other:?}"
            ),
        }
        assert!(
            mock.calls.load(Ordering::SeqCst) > 0,
            "expected the dispatch hook to have been invoked from inside the cost loop"
        );

        let mock_ranked = CountingDispatch {
            calls: AtomicUsize::new(0),
            k: 1.0,
        };
        match solver.solve_ranked_with_dispatch(&problem, Some(&mock_ranked)) {
            RankedSolveResult::Ranked { candidates, .. } => {
                let t = candidates
                    .first()
                    .expect("non-empty candidates (invariant I2)")
                    .values
                    .get(&t_id)
                    .expect("t should be in the solution")
                    .as_f64()
                    .expect("t should be numeric");
                assert!(
                    t > lo && t < hi,
                    "solve_ranked_with_dispatch should converge to a t strictly interior to \
                     bounds ({lo}, {hi}); got {t}"
                );
            }
            other => panic!(
                "expected Ranked once the dispatch hook is wired into the cost loop; got {other:?}"
            ),
        }
        assert!(
            mock_ranked.calls.load(Ordering::SeqCst) > 0,
            "expected the dispatch hook to have been invoked from inside the cost loop \
             (ranked path)"
        );

        // (b) WITHOUT the dispatch hook (plain solve/solve_ranked, no dispatch parameter
        // to even attempt wiring the mock through): `stress(t)` falls through to
        // body-eval -> Undef for every t, so `stress(t) < LIMIT` never decomposes
        // numerically and the constraint is unsatisfiable for any t — back-compat with
        // pre-#4880 behaviour.
        match solver.solve(&problem) {
            SolveResult::Infeasible { .. } => {}
            other => panic!(
                "expected Infeasible for the plain (no-dispatch) solve -- stress(t) is Undef \
                 for every t so the FEA constraint can never be numerically satisfied \
                 without the hook; got {other:?}"
            ),
        }
        match solver.solve_ranked(&problem) {
            RankedSolveResult::Infeasible { .. } => {}
            other => panic!(
                "expected Infeasible for the plain (no-dispatch) solve_ranked; got {other:?}"
            ),
        }
    }
}
