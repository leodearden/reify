//! Solver registry for multi-domain constraint dispatch.
//!
//! Combines classification + decomposition to dispatch sub-problems
//! to domain-specific solvers.

use crate::decompose::decompose_into_components;
use reify_core::{ConstraintNodeId, Type, ValueCellId};
use reify_ir::{AutoParam, BinOp, CompiledExpr, CompiledFunction, ConstraintDomain, ConstraintSolver, ObjectiveCombination, ObjectiveSense, ObjectiveSet, ObjectiveTerm, ResolutionProblem, SolveResult, UnOp, Value, ValueMap};
use std::collections::HashMap;

// ε-band constants (task ε — PRD §12.1).
// Half-width δ = max(REL · |obj*|, ABS) so a near-zero obj* yields a non-degenerate band.
const LEX_EPSILON_BAND_REL: f64 = 1e-3;
const LEX_EPSILON_BAND_ABS: f64 = 1e-9;

/// A registry that dispatches constraint sub-problems to domain-specific solvers.
///
/// Implements the `ConstraintSolver` trait, making it a drop-in replacement
/// for `DimensionalSolver` in the Engine. The registry:
/// 1. Classifies each constraint's domain
/// 2. Decomposes the problem into independent connected components
/// 3. Dispatches each component to the appropriate domain solver
/// 4. Merges results from all components
pub struct SolverRegistry {
    /// Solver for dimensional constraints (length, angle, etc.).
    dimensional: Box<dyn ConstraintSolver>,
    /// Solver for geometric constraints (optional, falls back to dimensional).
    geometric: Option<Box<dyn ConstraintSolver>>,
    /// Solver for logical constraints (optional, falls back to dimensional).
    logical: Option<Box<dyn ConstraintSolver>>,
    /// Explicit fallback solver for cross-domain constraints (if provided).
    fallback: Option<Box<dyn ConstraintSolver>>,
}

impl SolverRegistry {
    /// Create a new solver registry with a single solver used as both
    /// the dimensional solver and the fallback for all domains.
    pub fn new(solver: Box<dyn ConstraintSolver>) -> Self {
        Self {
            dimensional: solver,
            geometric: None,
            logical: None,
            fallback: None,
        }
    }

    /// Production solver set: Dimensional + geometric SolveSpace.
    ///
    /// This is the **single source of truth** for the constraint solver set
    /// installed by the CLI and GUI engines.  Both binaries call this factory
    /// rather than constructing their own registry, which prevents CLI/GUI
    /// solver-set drift.
    ///
    /// Slot assignments:
    /// - Dimensional: `DimensionalSolver` (Nelder-Mead; handles length/angle/scalar)
    /// - Geometric: `SolveSpaceSolver` (SolveSpace; handles `std::distance`,
    ///   `std::angle_between`, `std::parallel`, `std::tangent`, `std::geo::*`)
    /// - Logical: `None` — falls back to `DimensionalSolver`
    /// - CrossDomain fallback: `None` — falls back to `DimensionalSolver`
    pub fn production() -> Self {
        Self::with_solvers(
            Box::new(crate::DimensionalSolver),
            Some(Box::new(crate::SolveSpaceSolver)),
            None,
            None,
        )
    }

    /// Create a new solver registry with explicit solvers for each domain.
    pub fn with_solvers(
        dimensional: Box<dyn ConstraintSolver>,
        geometric: Option<Box<dyn ConstraintSolver>>,
        logical: Option<Box<dyn ConstraintSolver>>,
        fallback: Option<Box<dyn ConstraintSolver>>,
    ) -> Self {
        Self {
            dimensional,
            geometric,
            logical,
            fallback,
        }
    }

    /// Select the solver for a given domain.
    fn solver_for(&self, domain: ConstraintDomain) -> &dyn ConstraintSolver {
        match domain {
            ConstraintDomain::Dimensional => &*self.dimensional,
            ConstraintDomain::Geometric => self.geometric.as_deref().unwrap_or(&*self.dimensional),
            ConstraintDomain::Logical => self.logical.as_deref().unwrap_or(&*self.dimensional),
            ConstraintDomain::CrossDomain => self.fallback.as_deref().unwrap_or(&*self.dimensional),
        }
    }
}

impl ConstraintSolver for SolverRegistry {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        // Early exit: no auto params → already solved
        if problem.auto_params.is_empty() {
            return SolveResult::Solved {
                values: HashMap::new(),
                unique: true,
            };
        }

        // Collect value-refs from ALL objective terms for objective-aware decomposition.
        // Single-term ObjectiveSet reduces to the prior single-expr ref set bit-identically.
        let obj_refs: Option<std::collections::HashSet<ValueCellId>> =
            problem.objective.as_ref().map(|obj: &ObjectiveSet| {
                let mut refs = std::collections::HashSet::new();
                for term in &obj.terms {
                    crate::decompose::collect_value_refs_pub(&term.expr, &mut refs);
                }
                refs
            });

        // Decompose into connected components, merging any components
        // whose auto params are co-referenced by the objective expression(s)
        let components =
            decompose_into_components(&problem.auto_params, &problem.constraints, obj_refs.as_ref());

        // If no components (all constraints reference non-auto params),
        // the auto params are unconstrained. Return current values or defaults.
        if components.is_empty() {
            return SolveResult::Solved {
                values: HashMap::new(),
                unique: true,
            };
        }

        // Build a lookup for auto params by ID
        let param_lookup: HashMap<&ValueCellId, &AutoParam> =
            problem.auto_params.iter().map(|ap| (&ap.id, ap)).collect();

        // Determine which component gets the objective (if any).
        // Because decompose_into_components unions all objective-referenced
        // params, they are guaranteed to be in a single component. The
        // first-match iteration always finds the correct one.
        let objective_component = obj_refs.as_ref().map(|refs| {
            for (ci, comp) in components.iter().enumerate() {
                if refs.iter().any(|r| comp.auto_params.contains(r)) {
                    return ci;
                }
            }
            // Objective references no auto params in any component →
            // give it to the first component
            0
        });

        let mut merged_values: HashMap<ValueCellId, Value> = HashMap::new();
        let mut all_unique = true;

        for (ci, component) in components.iter().enumerate() {
            // Build sub-ResolutionProblem for this component
            let sub_auto_params: Vec<AutoParam> = component
                .auto_params
                .iter()
                .filter_map(|id| param_lookup.get(id).map(|ap| (*ap).clone()))
                .collect();

            // Filter current_values to only this component's params
            let mut sub_values = ValueMap::new();
            for (k, v) in problem.current_values.iter() {
                sub_values.insert(k.clone(), v.clone());
            }

            // Attach objective only to the designated component
            let sub_objective = if objective_component == Some(ci) {
                problem.objective.clone()
            } else {
                None
            };

            let sub_problem = ResolutionProblem {
                auto_params: sub_auto_params,
                constraints: component.constraints.clone(),
                current_values: sub_values,
                objective: sub_objective,
                functions: problem.functions.clone(),
            };

            // Select solver based on component domain
            let solver = self.solver_for(component.domain);

            // Branch: Lexicographic objectives require staged solving so that each
            // priority rank is presented to the domain solver as a WeightedSum (the
            // domain solver's debug_assert rejects Lexicographic directly).
            let result = match &sub_problem.objective {
                Some(obj) if obj.combination == ObjectiveCombination::Lexicographic => {
                    solve_lexicographic(solver, &sub_problem)
                }
                _ => solver.solve(&sub_problem),
            };

            match result {
                SolveResult::Solved { values, unique } => {
                    merged_values.extend(values);
                    all_unique &= unique;
                }
                SolveResult::Infeasible { diagnostics } => {
                    return SolveResult::Infeasible { diagnostics };
                }
                SolveResult::NoProgress { reason } => {
                    return SolveResult::NoProgress { reason };
                }
            }
        }

        SolveResult::Solved {
            values: merged_values,
            unique: all_unique,
        }
    }
}

// ============================================================================
// Lexicographic staged solve helper (task ε)
// ============================================================================

/// Solve a `ResolutionProblem` whose objective is `ObjectiveCombination::Lexicographic`
/// by sequencing sub-solves in descending priority order.
///
/// For each distinct priority rank (highest first), a fresh `ResolutionProblem` is
/// built whose objective is the rank's terms presented as `WeightedSum` (the domain
/// solver's `eval_objective_set` carries a `debug_assert` rejecting Lexicographic
/// directly).  All auto-params are forced `free = true` for intermediate stages so
/// the perturbation-based uniqueness check does not spuriously fail on intentionally
/// underdetermined faces.
///
/// A degenerate single-rank Lexicographic (all terms share the same priority) is
/// delegated to the underlying `solver.solve` with a WeightedSum-rebuilt objective,
/// preserving the solver's own uniqueness verdict.
///
/// Returns the final stage's `SolveResult`.  Intermediate stages force `unique = false`
/// (the ε-band leaves real slack on earlier-rank faces, so those points are not
/// uniqueness-verified).  The final stage's own `unique` verdict is preserved — given
/// the accumulated ε-band constraints, the final rank may itself be uniquely determined.
/// Infeasible / NoProgress from any stage propagates immediately.
fn solve_lexicographic(solver: &dyn ConstraintSolver, base: &ResolutionProblem) -> SolveResult {
    let obj = base.objective.as_ref().expect("solve_lexicographic: objective must be Some");

    // --- Group terms into ranks by distinct priority, sorted DESCENDING ---
    let priority_order: Vec<u32> = {
        let mut priorities: Vec<u32> = obj.terms.iter().map(|t| t.priority).collect();
        priorities.sort_unstable();
        priorities.dedup();
        priorities.reverse(); // highest first
        priorities
    };

    // Degenerate case: all terms share one priority — delegate as WeightedSum.
    if priority_order.len() == 1 {
        let ws_objective = ObjectiveSet {
            terms: obj.terms.clone(),
            combination: ObjectiveCombination::WeightedSum,
        };
        let ws_problem = ResolutionProblem {
            objective: Some(ws_objective),
            ..base.clone()
        };
        return solver.solve(&ws_problem);
    }

    // Multi-rank staged loop.
    let num_ranks = priority_order.len();
    let mut current_values = base.current_values.clone();
    let mut accumulated_constraints = base.constraints.clone();
    let mut last_result: Option<SolveResult> = None;

    for (stage_idx, priority) in priority_order.iter().enumerate() {
        // Collect terms for this rank.
        let rank_terms: Vec<ObjectiveTerm> = obj
            .terms
            .iter()
            .filter(|t| t.priority == *priority)
            .cloned()
            .collect();

        // Build stage objective as WeightedSum of this rank's terms.
        let stage_objective = ObjectiveSet {
            terms: rank_terms.clone(), // clone kept for band computation below
            combination: ObjectiveCombination::WeightedSum,
        };

        // Force all auto-params to free=true for intermediate stages so that the
        // perturbation-based uniqueness check does not spuriously fail on faces
        // that later ranks will resolve.  The final stage also uses free=true
        // because the ε-band on earlier ranks leaves real slack (unique:false).
        let free_auto_params: Vec<AutoParam> = base
            .auto_params
            .iter()
            .map(|ap| AutoParam { free: true, ..ap.clone() })
            .collect();

        let stage_problem = ResolutionProblem {
            auto_params: free_auto_params,
            constraints: accumulated_constraints.clone(),
            current_values: current_values.clone(),
            objective: Some(stage_objective),
            functions: base.functions.clone(),
        };

        let stage_result = solver.solve(&stage_problem);

        match stage_result {
            SolveResult::Solved { values, unique: stage_unique } => {
                // Warm-start the next stage from this stage's solution.
                for (k, v) in &values {
                    current_values.insert(k.clone(), v.clone());
                }

                let is_final = stage_idx == num_ranks - 1;

                // Intermediate stages are always non-unique: the ε-band leaves real
                // slack on earlier-rank faces, so those points are not
                // uniqueness-verified.  The final stage's own verdict is preserved —
                // given the accumulated ε-band constraints it may be fully determined.
                let result_unique = is_final && stage_unique;
                last_result = Some(SolveResult::Solved { values, unique: result_unique });

                if is_final {
                    break;
                }

                // Freeze this rank's realized optimum as an ε-band for the next stage.
                // If any term is non-finite, skip the band and warn — the lexicographic
                // ordering is NOT enforced for this rank, so later ranks may freely
                // sacrifice it.
                match eval_rank_cost(&rank_terms, &current_values, &base.functions) {
                    Some(obj_star) => {
                        accumulated_constraints
                            .extend(build_band_constraints(&rank_terms, obj_star, stage_idx));
                    }
                    None => {
                        tracing::warn!(
                            stage = stage_idx,
                            "solve_lexicographic: stage {} rank produced non-finite obj*; \
                             ε-band skipped — lexicographic ordering not enforced for this rank",
                            stage_idx,
                        );
                    }
                }
            }
            infeasible_or_no_progress => {
                return infeasible_or_no_progress;
            }
        }
    }

    last_result.expect("solve_lexicographic: priority_order is non-empty so at least one stage ran")
}

// ============================================================================
// ε-band private helpers
// ============================================================================

/// Compute the realized cost obj* for a rank at the current solution.
///
/// Mirrors `eval_objective_set` I3 fold (solver.rs:~436):
///   Minimize → acc += w·v
///   Maximize → acc -= w·v
/// Returns `None` if any term evaluates to a non-finite value.
fn eval_rank_cost(
    rank_terms: &[ObjectiveTerm],
    values: &ValueMap,
    functions: &[CompiledFunction],
) -> Option<f64> {
    let mut acc = 0.0_f64;
    for term in rank_terms {
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

/// Build the signed cost expression for a single objective term.
///
/// Sign convention (same as `eval_rank_cost`):
///   w=1, Minimize → expr (contributes positively to the minimization cost)
///   w=1, Maximize → UnOp::Neg(expr)
///   w≠1, Minimize → Real(w) * expr
///   w≠1, Maximize → Real(-w) * expr
///
/// The `result_type` of the returned expression mirrors the term's expr type
/// for unit-weight paths (B5/primary path); non-unit-weight paths use the
/// term's type (comparison is done via `as_f64()` so dimension is irrelevant).
fn signed_term_expr(term: &ObjectiveTerm) -> CompiledExpr {
    let e = term.expr.clone();
    let e_type = e.result_type.clone();
    let is_unit = (term.weight - 1.0).abs() < f64::EPSILON;
    match term.sense {
        ObjectiveSense::Minimize if is_unit => e,
        ObjectiveSense::Maximize if is_unit => CompiledExpr::unop(UnOp::Neg, e, e_type),
        ObjectiveSense::Minimize => {
            let w_lit = CompiledExpr::literal(Value::Real(term.weight), Type::dimensionless_scalar());
            CompiledExpr::binop(BinOp::Mul, w_lit, e, e_type)
        }
        ObjectiveSense::Maximize => {
            let w_lit = CompiledExpr::literal(Value::Real(-term.weight), Type::dimensionless_scalar());
            CompiledExpr::binop(BinOp::Mul, w_lit, e, e_type)
        }
    }
}

/// Fold a rank's signed term expressions into one combined cost expression.
///
/// Single-term ranks (the primary B5 path) return the term's signed expression
/// directly.  Multi-term tie ranks fold via `BinOp::Add` — this is valid only
/// for dimensionally-compatible terms (documented limitation, PRD §scope).
fn signed_cost_expr(rank_terms: &[ObjectiveTerm]) -> CompiledExpr {
    debug_assert!(!rank_terms.is_empty(), "rank_terms must be non-empty");
    rank_terms
        .iter()
        .map(signed_term_expr)
        .reduce(|a, b| {
            let ty = a.result_type.clone();
            CompiledExpr::binop(BinOp::Add, a, b, ty)
        })
        .expect("rank_terms is non-empty")
}

/// Build the two ε-band constraints that freeze a rank's realized optimum.
///
/// Produces:
///   `cost_expr ≤ Value::Real(obj* + δ)`  — Le, upper-bound (entity index 2·s)
///   `cost_expr ≥ Value::Real(obj* − δ)`  — Ge, lower-bound (entity index 2·s+1)
///
/// where `δ = max(LEX_EPSILON_BAND_REL · |obj*|, LEX_EPSILON_BAND_ABS)`.
///
/// Both constraints carry synthetic `ConstraintNodeId{ entity: "__lex_freeze__", .. }`.
/// The comparison is dimension-agnostic — the solver evaluates both sides via `as_f64()`.
fn build_band_constraints(
    rank_terms: &[ObjectiveTerm],
    obj_star: f64,
    stage_idx: usize,
) -> Vec<(ConstraintNodeId, CompiledExpr)> {
    let delta = f64::max(LEX_EPSILON_BAND_REL * obj_star.abs(), LEX_EPSILON_BAND_ABS);
    let cost = signed_cost_expr(rank_terms);

    let upper = CompiledExpr::literal(Value::Real(obj_star + delta), Type::dimensionless_scalar());
    let lower = CompiledExpr::literal(Value::Real(obj_star - delta), Type::dimensionless_scalar());

    let le_expr = CompiledExpr::binop(BinOp::Le, cost.clone(), upper, Type::Bool);
    let ge_expr = CompiledExpr::binop(BinOp::Ge, cost, lower, Type::Bool);

    let base_idx = stage_idx as u32 * 2;
    vec![
        (ConstraintNodeId::new("__lex_freeze__", base_idx), le_expr),
        (ConstraintNodeId::new("__lex_freeze__", base_idx + 1), ge_expr),
    ]
}
