//! Solver registry for multi-domain constraint dispatch.
//!
//! Combines classification + decomposition to dispatch sub-problems
//! to domain-specific solvers.

use crate::decompose::decompose_into_components;
use reify_core::ValueCellId;
use reify_ir::{AutoParam, ConstraintDomain, ConstraintSolver, ObjectiveCombination, ObjectiveSet, ObjectiveTerm, ResolutionProblem, SolveResult, Value, ValueMap};
use std::collections::HashMap;

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
/// Returns the final stage's `SolveResult`, overriding `unique` to `false` for any
/// multi-rank solve (the ε-band leaves real slack on earlier ranks).
/// Infeasible / NoProgress from any stage propagates immediately.
fn solve_lexicographic(solver: &dyn ConstraintSolver, base: &ResolutionProblem) -> SolveResult {
    let obj = base.objective.as_ref().expect("solve_lexicographic: objective must be Some");

    // --- Group terms into ranks by distinct priority, sorted DESCENDING ---
    let mut priority_order: Vec<u32> = {
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
            terms: rank_terms,
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
            SolveResult::Solved { values, .. } => {
                // Warm-start the next stage from this stage's solution.
                for (k, v) in &values {
                    current_values.insert(k.clone(), v.clone());
                }

                let is_final = stage_idx == num_ranks - 1;
                last_result = Some(SolveResult::Solved { values, unique: false });

                if is_final {
                    break;
                }
                // (band constraints will be added here in step-4)
            }
            infeasible_or_no_progress => {
                return infeasible_or_no_progress;
            }
        }
    }

    last_result.expect("solve_lexicographic: priority_order is non-empty so at least one stage ran")
}
