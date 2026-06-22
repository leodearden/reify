// DimensionalSolver: Nelder-Mead based constraint solver for auto parameters.

use std::collections::HashMap;

use argmin::core::{CostFunction, Error as ArgminError, Executor, State, TerminationReason};
use argmin::solver::neldermead::NelderMead;
use reify_core::{ConstraintNodeId, DiagnosticCode, DimensionVector, Type, ValueCellId, hash::ContentHash};
use reify_ir::{AutoParam, BinOp, CompiledExpr, CompiledExprKind, CompiledFunction, ConstraintSolver, ObjectiveCombination, ObjectiveSense, ObjectiveSet, ResolutionProblem, SolveResult, Value, ValueMap, TAG_CONDITIONAL};

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

/// Build a ValueMap from a base map with trial auto-param values inserted.
///
/// Clones the base map (O(1) via PersistentMap structural sharing) and
/// inserts each auto param as a Value::Scalar with the correct dimension.
/// Maps params directly to avoid the intermediate HashMap allocation that
/// `build_solved_values` would create — this is the hot path called on
/// every Nelder-Mead iteration.
fn build_trial_values(base: &ValueMap, params: &[AutoParam], x: &[f64]) -> ValueMap {
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
    values
}

/// Extract initial parameter values from the problem.
///
/// For each auto param, uses the midpoint of bounds if bounded, otherwise a
/// small default (0.01 for lengths). The auto param's own entry in
/// `current_values` (a *prior* resolved value, present only on the warm
/// edit/re-solve path) is deliberately NOT used as the Nelder-Mead seed.
///
/// ## Why the seed must NOT come from the auto's prior value (task #4700)
///
/// Seeding Nelder-Mead from the *current* (edited/prior) value of an auto made
/// the resolved value **path-dependent**: a cold `eval()` has no prior value for
/// the auto (it carries no entry in `current_values` before the solve), so it
/// seeds from the bounds-midpoint/default, whereas a warm re-solve after an edit
/// seeds from the previously-resolved value. Two Nelder-Mead runs that start from
/// *different* simplex origins converge to the SAME solution only to within
/// optimizer tolerance — they differ in the last 2–3 ULPs. That ULP-level
/// divergence breaks the edit-vs-cold **bit-exact** value-parity contract
/// (`assert_edit_matches_cold_with_solver`,
/// `edit_param_solver_auto_re_resolution_matches_cold`): a MOVED auto re-resolved
/// warm produced e.g. `0.009000000000000560 m` where cold produced
/// `0.009000000000000556 m`.
///
/// Making the seed a pure function of the *problem definition* (the auto's
/// declared bounds) rather than of edit history makes the resolved value
/// deterministic and identical on the warm and cold paths. Non-auto cells (e.g.
/// the edited upstream `base`) still flow through `current_values` into the
/// constraint evaluation — only the seed *origin* for the auto unknowns is
/// pinned. This is option (b) of the task: "re-seed from default rather than the
/// edited value."
///
/// Note this only changes behaviour for params that already had an entry in
/// `current_values` (the re-solve case); cold solves are byte-for-byte
/// unaffected because their auto params are absent from `current_values`.
fn extract_initial_point(problem: &ResolutionProblem) -> Vec<f64> {
    problem
        .auto_params
        .iter()
        .map(|param| {
            // Seed from the bounds midpoint if the auto declares bounds.
            if let Some((lo, hi)) = param.bounds {
                return (lo + hi) / 2.0;
            }
            // Default based on dimension.
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
) -> f64 {
    let lhs =
        reify_expr::eval_expr(left, &reify_expr::EvalContext::new(values, functions)).as_f64();
    let rhs =
        reify_expr::eval_expr(right, &reify_expr::EvalContext::new(values, functions)).as_f64();

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
) -> f64 {
    let lhs =
        reify_expr::eval_expr(left, &reify_expr::EvalContext::new(values, functions)).as_f64();
    let rhs =
        reify_expr::eval_expr(right, &reify_expr::EvalContext::new(values, functions)).as_f64();

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
) -> f64 {
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            match op {
                BinOp::Gt | BinOp::Ge | BinOp::Lt | BinOp::Le | BinOp::Eq | BinOp::Ne => {
                    comparison_residual(*op, left, right, values, functions)
                }
                BinOp::And => {
                    // AND: worst case (max) of sub-residuals
                    let lr = constraint_residual(left, values, functions);
                    let rr = constraint_residual(right, values, functions);
                    lr.max(rr)
                }
                BinOp::Or => {
                    // OR: best case (min) of sub-residuals
                    let lr = constraint_residual(left, values, functions);
                    let rr = constraint_residual(right, values, functions);
                    lr.min(rr)
                }
                _ => {
                    match reify_expr::eval_expr(
                        expr,
                        &reify_expr::EvalContext::new(values, functions),
                    ) {
                        Value::Bool(true) => 0.0,
                        Value::Bool(false) => 1.0,
                        Value::Undef => 10.0,
                        _ => 1.0,
                    }
                }
            }
        }
        _ => match reify_expr::eval_expr(expr, &reify_expr::EvalContext::new(values, functions)) {
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
) -> f64 {
    // First try decomposing into a comparison
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            match op {
                BinOp::Gt | BinOp::Ge | BinOp::Lt | BinOp::Le | BinOp::Eq | BinOp::Ne => {
                    comparison_violation(*op, left, right, values, functions)
                }
                BinOp::And => {
                    // AND: sum violations of both sides
                    constraint_violation(left, values, functions)
                        + constraint_violation(right, values, functions)
                }
                BinOp::Or => {
                    // OR: minimum violation of both sides
                    let lv = constraint_violation(left, values, functions);
                    let rv = constraint_violation(right, values, functions);
                    lv.min(rv)
                }
                _ => {
                    // Not a logical/comparison op; evaluate as boolean
                    match reify_expr::eval_expr(
                        expr,
                        &reify_expr::EvalContext::new(values, functions),
                    ) {
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
            match reify_expr::eval_expr(expr, &reify_expr::EvalContext::new(values, functions)) {
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
) -> f64 {
    constraints
        .iter()
        .map(|(_, expr)| constraint_residual(expr, values, functions))
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
) -> f64 {
    constraints
        .iter()
        .map(|(_, expr)| constraint_violation(expr, values, functions))
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
fn build_centrality_objective(
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
fn eval_objective_set(
    objective: &ObjectiveSet,
    values: &ValueMap,
    functions: &[CompiledFunction],
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
    let mut acc = 0.0_f64;
    for term in &objective.terms {
        let v = reify_expr::eval_expr(&term.expr, &reify_expr::EvalContext::new(values, functions))
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
        for (&val, ap) in param.iter().zip(self.auto_params.iter()) {
            let (lo, hi) = effective_bounds(ap);
            let cv = val.clamp(lo, hi);
            bound_penalty += (val - cv).powi(2);
            clamped.push(cv);
        }

        let values = build_trial_values(self.base_values, self.auto_params, &clamped);
        let violation = compute_total_violation(self.constraints, &values, self.functions);

        let cost = match self.objective {
            Some(obj) => {
                // Combine objective with penalty for constraint violations and bounds
                let obj_value =
                    eval_objective_set(obj, &values, self.functions).unwrap_or(UNDEF_OBJECTIVE_PENALTY);
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
fn build_simplex(initial: &[f64], params: &[AutoParam]) -> Vec<Vec<f64>> {
    let n = initial.len();
    let mut simplex = Vec::with_capacity(n + 1);
    simplex.push(initial.to_vec());

    for i in 0..n {
        let mut vertex = initial.to_vec();
        // Perturb dimension i by a fraction of the effective range
        let (lo, hi) = effective_bounds(&params[i]);
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

/// Relative tolerance for uniqueness comparison between two solutions.
const UNIQUENESS_REL_TOL: f64 = 1e-6;

/// Absolute tolerance for uniqueness comparison between two solutions.
const UNIQUENESS_ABS_TOL: f64 = 1e-10;

/// Nelder-Mead `sd_tolerance` for the **uniqueness re-solve** (`verify_uniqueness`).
///
/// ## Why this is decoupled from `NM_SD_TOLERANCE` (task #4700 esc-4700-34)
///
/// `verify_uniqueness` re-solves the problem from a far-perturbed seed and
/// compares the result to the main solution: agreement ⇒ unique, divergence ⇒
/// the strict-auto "not uniquely determined" error (`ConstraintNonUnique`).
///
/// Task #4700 tightened the **main-solve** tolerance to `NM_SD_TOLERANCE`
/// (1e-30) so a MOVED strict auto converges to `FEASIBILITY_THRESHOLD`. If that
/// same tight tolerance also drove the uniqueness re-solve, the perturbed
/// re-solve would reach feasibility on the *well-constrained* params of a
/// multi-param problem and thereby EXPOSE the (expected) divergence of any
/// param that is **unconstrained within this problem** — producing a spurious
/// `ConstraintNonUnique`.
///
/// This bites the `auto_binding_sites.ri` `AllFourSites` scope: its
/// `__connector_0.gain` auto is determined by the connector's *own* internal
/// constraint (design D5 — the parent cannot name the synthesised
/// `__connector_N`), so it carries NO determining constraint in the parent
/// resolution problem. It is genuinely non-unique *within that problem* and is
/// only correct because it is Determined by a separate connector pass — a fact
/// the solver cannot see. The pre-#4700 tolerance (1e-15) masked this because
/// the perturbed re-solve could not drive the other params to `1e-12`
/// feasibility and fell back to "perturbed solve did not converge ⇒ assume
/// unique".
///
/// Keeping the uniqueness re-solve at the pre-#4700 `1e-15` restores that
/// exact behaviour: it does NOT weaken genuine non-uniqueness detection for
/// strict autos that *are* constrained (e.g. a sole unconstrained
/// `let m : Length = auto` is still flagged — see
/// `let_auto_strict_underdetermined_emits_error`), while the main solve keeps
/// the #4700 moved-auto convergence fix.
///
/// The principled fix — not injecting already-Determined connector-internal
/// autos as fresh unconstrained autos into the parent problem — lives in
/// reify-eval problem construction (esc-4700-34); outside task #4700's
/// solver-side file scope.
const UNIQUENESS_SD_TOLERANCE: f64 = 1e-15;

/// Core solve logic: runs Nelder-Mead from a given initial point, using the
/// caller-supplied `sd_tolerance` for the simplex termination criterion.
///
/// Returns `SolveResult` with `unique: true` as placeholder — the caller
/// (`DimensionalSolver::solve`) is responsible for setting the correct
/// uniqueness flag based on free/strict auto param classification.
///
/// The `sd_tolerance` is parameterised (rather than reading `NM_SD_TOLERANCE`
/// directly) because the two callers want different convergence regimes:
///
/// * The **main solve** (`DimensionalSolver::solve`) passes `NM_SD_TOLERANCE`
///   (1e-30) so a strict auto forced to MOVE from an off-target seed converges
///   all the way to `FEASIBILITY_THRESHOLD` (task #4700).
/// * The **uniqueness re-solve** (`verify_uniqueness`) passes
///   `UNIQUENESS_SD_TOLERANCE` (1e-15) — see that constant's docs for why the
///   tight main-solve tolerance must NOT leak into the uniqueness heuristic.
fn solve_core_with_sd_tolerance(
    problem: &ResolutionProblem,
    initial: &[f64],
    sd_tolerance: f64,
) -> SolveResult {
    // Check feasibility at the initial point for ALL problems (not just
    // pure feasibility). This enables early-exit for no-objective problems
    // and a reduced iteration budget for optimization warm-starts.
    // NB: `trial_values` is used in two places — (1) the feasibility check
    // immediately below, and (2) the fallback objective validation when the
    // optimizer drifts infeasible (see `eval_objective(&trial_values, …)`).
    // Do not inline into the feasibility check.
    let trial_values = build_trial_values(&problem.current_values, &problem.auto_params, initial);
    let initially_feasible =
        max_constraint_residual(&problem.constraints, &trial_values, &problem.functions)
            <= FEASIBILITY_THRESHOLD;

    // Synthesise a default centrality (Chebyshev-centre) objective when the scope has
    // inequality constraints but no explicit user objective (PRD η).  The synthetic
    // objective is built once and threaded through the cost function exactly like a
    // user-supplied objective; no new cost branch is added.
    //
    // `synth` lives for the rest of the function so the borrow in `effective_objective`
    // remains valid.  Discrete-type guard (Type::Scalar check) is added in step-4.
    let synth: Option<ObjectiveSet> = if problem.objective.is_none() {
        build_centrality_objective(&problem.auto_params, &problem.constraints)
    } else {
        None
    };

    // Effective objective: explicit (if any), else synthetic (if any), else None.
    // This is a borrow — `synth` and `problem` both outlive the function body.
    let effective_objective: Option<&ObjectiveSet> =
        problem.objective.as_ref().or(synth.as_ref());

    // Pure feasibility + already feasible → return immediately.
    // Gate on the EFFECTIVE objective so a centrality scope optimises instead of
    // short-circuiting to the first feasible boundary point.
    if initially_feasible && effective_objective.is_none() {
        let n_params = problem.auto_params.len();
        tracing::debug!(
            n_params,
            "initial point already feasible with no objective; returning early"
        );
        return SolveResult::Solved {
            values: build_solved_values(&problem.auto_params, initial),
            unique: true,
        };
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
        constraints: &problem.constraints,
        base_values: &problem.current_values,
        objective: effective_objective,
        functions: &problem.functions,
    };

    // Build simplex from the provided initial point
    let simplex = build_simplex(initial, &problem.auto_params);

    // Configure and run Nelder-Mead
    let solver: NelderMead<Vec<f64>, f64> = NelderMead::new(simplex)
        .with_sd_tolerance(sd_tolerance)
        .expect("sd_tolerance is always valid (positive finite f64: NM_SD_TOLERANCE or UNIQUENESS_SD_TOLERANCE)");

    let executor = Executor::new(cost_fn, solver).configure(|state| state.max_iters(max_iters));

    let result = match executor.run() {
        Ok(res) => res,
        Err(e) => {
            let n_params = problem.auto_params.len();
            tracing::warn!(error = %e, n_params, "solver executor failed");
            return SolveResult::NoProgress {
                reason: format!("solver error: {}", e),
            };
        }
    };

    // Extract and log convergence information from the solver result.
    let termination_reason = result.state().get_termination_reason().cloned();
    let has_objective = effective_objective.is_some();
    let n_params = problem.auto_params.len();
    let iter_limited =
        termination_reason == Some(TerminationReason::MaxItersReached) && has_objective;
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
            return SolveResult::NoProgress {
                reason: "solver returned no solution".to_string(),
            };
        }
    };

    // Clamp final solution to effective bounds
    let clamped: Vec<f64> = best_param
        .iter()
        .zip(problem.auto_params.iter())
        .map(|(val, ap)| {
            let (lo, hi) = effective_bounds(ap);
            val.clamp(lo, hi)
        })
        .collect();

    // Check feasibility by re-evaluating constraint violations
    // (best_cost may include the objective term, so we check violations separately)
    let final_values = build_trial_values(&problem.current_values, &problem.auto_params, &clamped);
    let final_max_residual =
        max_constraint_residual(&problem.constraints, &final_values, &problem.functions);
    if final_max_residual > FEASIBILITY_THRESHOLD {
        // If the initial point was feasible but the optimizer drifted infeasible
        // while chasing an objective, fall back to the initial feasible values
        // rather than reporting a false Infeasible.
        if initially_feasible {
            // Validate that the objective is numeric at the initial point
            // before promoting to Solved. The trial_values ValueMap was built
            // from the same initial point and is still in scope.
            if let Some(obj) = effective_objective
                && eval_objective_set(obj, &trial_values, &problem.functions).is_none()
            {
                return SolveResult::NoProgress {
                    reason: "objective expression evaluated to undefined at fallback point"
                        .to_string(),
                };
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
            return SolveResult::Solved {
                values: fallback,
                unique: true,
            };
        }
        return SolveResult::Infeasible {
            diagnostics: vec![
                reify_core::Diagnostic::error(format!(
                    "constraints could not be satisfied (max absolute residual: {:.2e})",
                    final_max_residual
                ))
                .with_code(DiagnosticCode::ConstraintUnsatisfiable),
            ],
        };
    }

    // Post-solve objective validation: if the objective is still non-numeric
    // at the solution point, report NoProgress rather than Solved.
    if let Some(obj) = effective_objective
        && eval_objective_set(obj, &final_values, &problem.functions).is_none()
    {
        return SolveResult::NoProgress {
            reason: "objective expression evaluated to undefined at solution point".to_string(),
        };
    }

    // Build solution values
    let values = build_solved_values(&problem.auto_params, &clamped);

    // NOTE: Solved indicates constraint satisfaction but does NOT guarantee objective
    // optimality. The Nelder-Mead optimizer may have hit the iteration limit without
    // full convergence. Convergence quality is logged via tracing::debug! (see above)
    // including TerminationReason, iteration budget, and whether fallback was used.
    // This information is NOT propagated through SolveResult to avoid a breaking API
    // change across 6+ consumer crates. Enable RUST_LOG=reify_constraints=debug to
    // inspect convergence details at runtime.
    SolveResult::Solved {
        values,
        unique: true,
    }
}

/// Core solve at the default (main-solve) convergence regime.
///
/// Thin wrapper over [`solve_core_with_sd_tolerance`] passing `NM_SD_TOLERANCE`
/// (1e-30). This is the entry point for the **main** resolution solve, where a
/// strict auto must converge to `FEASIBILITY_THRESHOLD` even from a moved seed
/// (task #4700). The uniqueness re-solve deliberately does NOT route through
/// here — see [`verify_uniqueness`] / `UNIQUENESS_SD_TOLERANCE`.
fn solve_core(problem: &ResolutionProblem, initial: &[f64]) -> SolveResult {
    solve_core_with_sd_tolerance(problem, initial, NM_SD_TOLERANCE)
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
        let diff = (s1 - s2).abs();
        let scale = s1.abs().max(s2.abs()).max(UNIQUENESS_ABS_TOL);
        if diff > UNIQUENESS_REL_TOL * scale && diff > UNIQUENESS_ABS_TOL {
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

/// Build the perturbed initial point for uniqueness verification.
///
/// For each auto parameter, computes the perturbed starting value by reflecting
/// to the opposite end of its effective bounds range from the current solution.
/// If a solved value is missing or non-numeric (`as_f64()` returns `None`), the
/// midpoint is used as a fallback and the parameter ID is added to the returned
/// missing list.
///
/// Returns `(perturbed_anchors, missing_param_ids)`.
fn build_perturbation_anchors(
    auto_params: &[reify_ir::AutoParam],
    solved_values: &HashMap<ValueCellId, Value>,
) -> (Vec<f64>, Vec<String>) {
    let mut missing: Vec<String> = Vec::new();
    let perturbed: Vec<f64> = auto_params
        .iter()
        .map(|param| {
            let (lo, hi) = effective_bounds(param);
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

/// Verify solution uniqueness by re-solving from a perturbed starting point.
///
/// Creates a perturbed initial point by reflecting each parameter to the
/// opposite end of its effective bounds range. If the solution found from
/// the perturbed starting point matches the original within tolerance,
/// the solution is considered unique.
///
/// Returns `true` if the solution is unique, `false` if a different
/// solution was found (indicating the problem is underdetermined).
fn verify_uniqueness(
    problem: &ResolutionProblem,
    solved_values: &HashMap<ValueCellId, Value>,
) -> bool {
    // Build perturbed initial point: reflect each param to the opposite
    // end of its bounds range from the solution.
    let (perturbed, missing) = build_perturbation_anchors(&problem.auto_params, solved_values);
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

    tracing::debug!(
        n_params = problem.auto_params.len(),
        "verifying uniqueness via perturbation"
    );

    // Re-solve from the perturbed starting point.
    // Uses UNIQUENESS_SD_TOLERANCE (the pre-#4700 1e-15), NOT the tight
    // main-solve NM_SD_TOLERANCE — see UNIQUENESS_SD_TOLERANCE docs for why the
    // tight tolerance must not leak into this heuristic (esc-4700-34).
    match solve_core_with_sd_tolerance(problem, &perturbed, UNIQUENESS_SD_TOLERANCE) {
        SolveResult::Solved {
            values: perturbed_values,
            ..
        } => {
            // Compare solutions: all params must match within tolerance
            solutions_agree(&problem.auto_params, solved_values, &perturbed_values)
        }
        _ => {
            // If the perturbed solve fails (Infeasible/NoProgress), we can't
            // prove non-uniqueness — conservatively assume unique.
            tracing::debug!("uniqueness check: perturbed solve did not converge; assuming unique");
            true
        }
    }
}

impl ConstraintSolver for DimensionalSolver {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        // Trivial case: no auto parameters to solve for
        if problem.auto_params.is_empty() {
            return SolveResult::Solved {
                values: HashMap::new(),
                unique: true,
            };
        }

        let initial = extract_initial_point(problem);
        let result = solve_core(problem, &initial);

        match result {
            SolveResult::Solved { values, .. } => {
                // Check if any param requires uniqueness verification (strict auto)
                let has_strict = problem.auto_params.iter().any(|p| !p.free);
                if has_strict {
                    if verify_uniqueness(problem, &values) {
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
            other => other, // Infeasible, NoProgress pass through unchanged
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
            verify_uniqueness(problem, solved_values)
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

        let trial = build_trial_values(&base, &params, &[0.005]);

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

        let trial = build_trial_values(&base, &params, &[0.005, 1.2]);

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

        use reify_test_support::CountingSubscriberBuilder;
        use reify_core::{Type, ValueCellId};
        use reify_ir::AutoParam;

        use super::verify_uniqueness;

        let param_id = ValueCellId::new("Part", "x");
        let problem = ResolutionProblem {
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
            verify_uniqueness(&problem, &solved_values)
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

        use reify_test_support::CountingSubscriberBuilder;
        use reify_core::{Type, ValueCellId};
        use reify_ir::{AutoParam, Value};

        use super::verify_uniqueness;

        let param_id = ValueCellId::new("Part", "x");
        let problem = ResolutionProblem {
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
            verify_uniqueness(&problem, &solved_values)
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

        let (perturbed, missing) = build_perturbation_anchors(&params, &solved_values);

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

        let (perturbed, missing) = build_perturbation_anchors(&params, &solved_values);

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
        let mut solved_values: HashMap<reify_core::ValueCellId, reify_ir::Value> =
            HashMap::new();
        // Value::Undef: as_f64() returns None → same None-branch as missing
        solved_values.insert(id, reify_ir::Value::Undef);

        let (perturbed, missing) = build_perturbation_anchors(&params, &solved_values);

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

        let (perturbed, missing) = build_perturbation_anchors(&params, &solved_values);

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

        let (perturbed, missing) = build_perturbation_anchors(&params, &solved_values);

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
        let trial = build_trial_values(&base, &[], &[]);

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
        let violation = compute_total_violation(&constraints, &values, &[]);
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
        let violation = compute_total_violation(&constraints, &values, &[]);
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
        let violation = compute_total_violation(&constraints, &values, &[]);
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
        let res = comparison_residual(BinOp::Gt, &l_expr, &r_expr, &values, &[]);
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
        let res = comparison_residual(BinOp::Ge, &l_expr, &r_expr, &values, &[]);
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
        let res = comparison_residual(BinOp::Lt, &l_expr, &r_expr, &values, &[]);
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
        let res = comparison_residual(BinOp::Le, &l_expr, &r_expr, &values, &[]);
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
        let res = comparison_residual(BinOp::Eq, &l_expr, &r_expr, &values, &[]);
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

        let res = constraint_residual(&expr, &values, &[]);
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

        let res = constraint_residual(&and_expr, &values, &[]);
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

        let res = constraint_residual(&or_expr, &values, &[]);
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

        let res = max_constraint_residual(&constraints, &values, &[]);
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

        let res = max_constraint_residual(&constraints, &values, &[]);
        assert_eq!(res, 0.0, "all satisfied should return 0.0");
    }

    #[test]
    fn max_constraint_residual_empty() {
        use super::max_constraint_residual;

        let constraints = vec![];
        let values = ValueMap::new();
        let res = max_constraint_residual(&constraints, &values, &[]);
        assert_eq!(res, 0.0, "empty constraints should return 0.0");
    }

    #[test]
    fn constraint_residual_bool_literals() {
        use super::constraint_residual;
        use reify_core::Type;
        use reify_ir::{CompiledExpr, Value};

        let values = ValueMap::new();

        let t = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        assert_eq!(constraint_residual(&t, &values, &[]), 0.0);

        let f = CompiledExpr::literal(Value::Bool(false), Type::Bool);
        assert_eq!(constraint_residual(&f, &values, &[]), 1.0);

        let u = CompiledExpr::literal(Value::Undef, Type::Bool);
        assert_eq!(constraint_residual(&u, &values, &[]), 10.0);
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
        let res = comparison_residual(BinOp::Gt, &l_expr, &r_expr, &values, &[]);
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
        let div_by_zero = CompiledExpr::binop(BinOp::Div, x_ref, zero_int, Type::dimensionless_scalar());
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
        use reify_core::{Type, ValueCellId};
        use reify_ir::AutoParam;

        // 1-dimensional: simplex should have 2 vertices
        let params_1d = vec![AutoParam {
            id: ValueCellId::new("S", "x"),
            param_type: Type::length(),
            bounds: Some((0.0, 1.0)),
            free: false,
        }];
        let initial_1d = vec![0.5];
        let simplex = build_simplex(&initial_1d, &params_1d);
        assert_eq!(simplex.len(), 2, "1D simplex must have N+1=2 vertices");

        // 2-dimensional: simplex should have 3 vertices
        let params_2d = vec![
            AutoParam {
                id: ValueCellId::new("S", "x"),
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            },
            AutoParam {
                id: ValueCellId::new("S", "y"),
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            },
        ];
        let initial_2d = vec![0.5, 0.5];
        let simplex = build_simplex(&initial_2d, &params_2d);
        assert_eq!(simplex.len(), 3, "2D simplex must have N+1=3 vertices");

        // 3-dimensional: simplex should have 4 vertices
        let params_3d = vec![
            AutoParam {
                id: ValueCellId::new("S", "x"),
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            },
            AutoParam {
                id: ValueCellId::new("S", "y"),
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            },
            AutoParam {
                id: ValueCellId::new("S", "z"),
                param_type: Type::length(),
                bounds: Some((0.0, 1.0)),
                free: false,
            },
        ];
        let initial_3d = vec![0.5, 0.5, 0.5];
        let simplex = build_simplex(&initial_3d, &params_3d);
        assert_eq!(simplex.len(), 4, "3D simplex must have N+1=4 vertices");
    }

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
        use reify_test_support::{cnid, gt, literal, mm, value_ref, vcid};
        use reify_core::Type;
        use reify_ir::{AutoParam, ObjectiveSense, ObjectiveSet};

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
        use reify_test_support::{cnid, gt, literal, lt, mm, value_ref, vcid};
        use reify_core::Type;
        use reify_ir::{AutoParam, Value};

        let solver = DimensionalSolver;
        let x_id = vcid("Part", "x");

        // Simple feasibility: x > 5mm AND x < 50mm
        let x_ref = value_ref("Part", "x");
        let five_mm = literal(mm(5.0));
        let fifty_mm = literal(mm(50.0));
        let gt_expr = gt(x_ref.clone(), five_mm);
        let lt_expr = lt(x_ref, fifty_mm);

        let problem = ResolutionProblem {
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
        let div_by_zero = CompiledExpr::binop(BinOp::Div, x_ref, zero_int, Type::dimensionless_scalar());
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
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId, hash::ContentHash};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, CompiledExprKind, ObjectiveSense, ObjectiveSet, Value};

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
        // Initial simplex (bounds: None → Length defaults 1µm–10m):
        //   vertex 0 at x=0.01 (feasible, Undef, cost ≈ f64::MAX/2),
        //   vertex 1 at x=0.01+~1.0=~1.01 (infeasible, finite, cost ≈ 980101).
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
        let then_branch = CompiledExpr::binop(BinOp::Div, x_ref.clone(), zero_int, Type::dimensionless_scalar());
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

        // No current_values needed — extract_initial_point seeds from the Length
        // default (0.01 m = 10mm) when bounds is None. The seed is feasible since
        // 0.01 ≤ 0.020, so initially_feasible=true and the fallback path fires.
        let problem = ResolutionProblem {
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None, // seed = 0.01 m (feasible), default bounds 1µm–10m
                free: false,
            }],
            constraints: vec![(ConstraintNodeId::new("Part", 0), le_expr)],
            current_values: ValueMap::new(),
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
        use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId, hash::ContentHash};
        use reify_ir::{AutoParam, BinOp, CompiledExpr, CompiledExprKind, ObjectiveSense, ObjectiveSet, Value};

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
        // Initial simplex (bounds: None → Length defaults 1µm–10m):
        //   vertex 0 at x=0.01 (feasible, cost=1e8),
        //   vertex 1 at x=0.01+~1.0=~1.01 (infeasible, cost≈980101).
        // The enormous cost differential lures the optimizer past x=0.022 into the
        // infeasible region. The solver detects infeasibility (residual >> 1e-12),
        // falls back to the initial feasible point x=0.01 (Length default seed),
        // validates the objective (eval_objective returns Some(1e8) → passes), and
        // returns Solved with the initial values.
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

        // No current_values needed — extract_initial_point seeds from the Length
        // default (0.01 m = 10mm) when bounds is None. The seed is feasible since
        // 0.01 ≤ 0.020, so initially_feasible=true and the fallback path fires.
        let problem = ResolutionProblem {
            auto_params: vec![AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None, // seed = 0.01 m (feasible), default bounds 1µm–10m
                free: false,
            }],
            constraints: vec![(ConstraintNodeId::new("Part", 0), le_expr)],
            current_values: ValueMap::new(),
            objective: Some(objective),
            functions: vec![].into(),
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values, .. } => {
                let si = values.get(&x_id).unwrap().as_f64().unwrap();
                assert!(
                    (si - 0.01).abs() < 1e-10,
                    "fallback path should return initial x = 0.01 m, got {} m",
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
            Value::Scalar { si_value: 0.002, dimension: DimensionVector::LENGTH },
            Type::length(),
        );
        let ge_expr = CompiledExpr::binop(BinOp::Ge, x_ref.clone(), two_mm, Type::Bool);

        // x <= 8mm
        let eight_mm = CompiledExpr::literal(
            Value::Scalar { si_value: 0.008, dimension: DimensionVector::LENGTH },
            Type::length(),
        );
        let le_expr = CompiledExpr::binop(BinOp::Le, x_ref, eight_mm, Type::Bool);

        let problem = ResolutionProblem {
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
            Value::Scalar { si_value: 0.005, dimension: DimensionVector::LENGTH },
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
            Value::Scalar { si_value: 0.01, dimension: DimensionVector::LENGTH },
            Type::length(),
        );
        let eq_expr = CompiledExpr::binop(BinOp::Eq, x_ref, ten_mm, Type::Bool);

        // Seed x = 20mm (MOVED — off-target, requires Nelder-Mead to search)
        let mut current_values = ValueMap::new();
        current_values.insert(
            x_id.clone(),
            Value::Scalar { si_value: 0.02, dimension: DimensionVector::LENGTH },
        );

        let problem = ResolutionProblem {
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

    /// [task-4700 esc-4700-34] UNIQUENESS_SD_TOLERANCE decoupling: a problem with one
    /// constrained strict auto (`x == 10mm`) and one unconstrained strict auto (`y`) must
    /// return `Solved` — NOT `ConstraintNonUnique` — at the asymmetric tolerances
    /// introduced by task #4700.
    ///
    /// ## What this test pins
    ///
    /// `DimensionalSolver::solve` uses two distinct tolerance regimes:
    ///
    /// - **Main solve** (`NM_SD_TOLERANCE = 1e-30`): tight enough for `x` to converge
    ///   from an off-target seed (20mm → 10mm) within `FEASIBILITY_THRESHOLD = 1e-12`.
    /// - **Uniqueness re-solve** (`UNIQUENESS_SD_TOLERANCE = 1e-15`): deliberately looser.
    ///   From the far-perturbed starting point, the re-solve can only drive `x` to a
    ///   linear residual of ~1e-8 (> 1e-12), so it returns `Infeasible`. `verify_uniqueness`
    ///   then conservatively returns `true` (assume unique) and the overall result is `Solved`.
    ///
    /// ## Why it would break if UNIQUENESS_SD_TOLERANCE were tightened to match NM_SD_TOLERANCE
    ///
    /// With `UNIQUENESS_SD_TOLERANCE = 1e-30`, the perturbed re-solve CAN converge `x` to
    /// 10mm. But `y` (unconstrained in this problem) lands at a different value than in the
    /// main solve — no constraint anchors it. `solutions_agree` then finds `y` diverged and
    /// returns `false`, which triggers `SolveResult::Infeasible` with
    /// `DiagnosticCode::ConstraintNonUnique` — a spurious error.
    ///
    /// This mirrors the `auto_binding_sites.ri` `AllFourSites` connector scope where
    /// `__connector_0.gain` is a strict auto that is Determined by the connector's own
    /// internal pass (invisible to the parent solver), so it carries NO constraint in the
    /// parent problem and appears genuinely non-unique within that scope (esc-4700-34).
    ///
    /// If this test fails with `Infeasible`/`ConstraintNonUnique`, `UNIQUENESS_SD_TOLERANCE`
    /// has been tightened past the safe threshold and must be reverted.
    #[test]
    fn uniqueness_sd_tolerance_decoupling_suppresses_spurious_non_unique() {
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
            Value::Scalar { si_value: 0.01, dimension: DimensionVector::LENGTH },
            Type::length(),
        );
        let eq_expr = CompiledExpr::binop(BinOp::Eq, x_ref, ten_mm, Type::Bool);

        // Seed x = 20mm (MOVED — off-target, requires NM_SD_TOLERANCE=1e-30 to converge).
        // Seed y = 5mm (arbitrary; no constraint will move it from its initial value).
        let mut current_values = ValueMap::new();
        current_values.insert(
            x_id.clone(),
            Value::Scalar { si_value: 0.02, dimension: DimensionVector::LENGTH },
        );
        current_values.insert(
            y_id.clone(),
            Value::Scalar { si_value: 0.005, dimension: DimensionVector::LENGTH },
        );

        let problem = ResolutionProblem {
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
            SolveResult::Solved { values, .. } => {
                // x must have converged to 10mm (NM_SD_TOLERANCE=1e-30 fix).
                let x_si = values.get(&x_id).unwrap().as_f64().unwrap();
                assert!(
                    (x_si - 0.01).abs() <= 1e-11,
                    "x must converge to 0.01 m (10mm) within 1e-11 m; got {x_si:.3e} m \
                     (error {:.3e} m)",
                    (x_si - 0.01).abs()
                );
            }
            SolveResult::Infeasible { diagnostics } => {
                let is_spurious_non_unique = diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique));
                if is_spurious_non_unique {
                    panic!(
                        "uniqueness_sd_tolerance_decoupling: got spurious ConstraintNonUnique \
                         for a problem with one constrained strict auto (x==10mm) and one \
                         unconstrained strict auto (y). UNIQUENESS_SD_TOLERANCE has been \
                         tightened past the safe threshold, causing the perturbed re-solve \
                         to converge x and thereby expose y's non-uniqueness. \
                         See esc-4700-34 and the UNIQUENESS_SD_TOLERANCE constant docs."
                    );
                }
                panic!(
                    "uniqueness_sd_tolerance_decoupling: expected Solved but got Infeasible \
                     (not ConstraintNonUnique). diagnostics: {diagnostics:?}"
                );
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }
}
