//! Solver registry for multi-domain constraint dispatch.
//!
//! Combines classification + decomposition to dispatch sub-problems
//! to domain-specific solvers.

use crate::decompose::decompose_into_components;
use reify_core::{ConstraintNodeId, Type, ValueCellId};
use reify_ir::{AutoParam, BinOp, CompiledExpr, CompiledFunction, ConstraintDomain, ConstraintSolver, ObjectiveCombination, ObjectiveSense, ObjectiveSet, ObjectiveTerm, OptimalityStatus, RankedCandidate, RankedSolveResult, ResolutionProblem, SolveResult, UnOp, Value, ValueMap};
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

impl SolverRegistry {
    /// Shared decomposition/dispatch core for [`ConstraintSolver::solve`] and
    /// [`ConstraintSolver::solve_ranked`].
    ///
    /// `want_optimality = false` reproduces the historical `solve()` path
    /// byte-for-byte (invariant I1 freeze): every component is dispatched via
    /// `solver.solve()`.  `want_optimality = true` routes the *objective-bearing*
    /// component through `solver.solve_ranked()` so the domain solver's real
    /// [`OptimalityStatus`] (and objective score) is recovered and returned to the
    /// caller — this is what lets `reify eval` surface `W_SOLVER_OPTIMALITY_UNPROVEN`
    /// (task #4804 γ) instead of the generic default-lift reason.
    ///
    /// # δ best-of-K propagation (task #5016)
    ///
    /// The merged resolved values are NOT guaranteed identical on both paths
    /// in general: when the objective component is multistart-eligible (see
    /// `DimensionalSolver::solve_ranked`'s dim>=2 gate), `solve_ranked` can
    /// find a STRICTLY BETTER point than the single-seed `solve()` path
    /// (best-of-K dominance, not identity — see `SolverRegistry::solve_ranked`'s
    /// doc comment). `solve()` itself (`want_optimality = false`) is
    /// completely unaffected: the guard on the ranked-dispatch arm below is
    /// `want_optimality && is_objective_component`, so with
    /// `want_optimality = false` every component, including the would-be
    /// objective one, always takes the plain `solver.solve()` arm exactly as
    /// before (I1 for `solve()` itself is preserved byte-for-byte).
    ///
    /// The 4th return slot carries the objective component's FULL
    /// [`RankedCandidate`] vector (already cross-merged with every other
    /// component's shared values — see the cross-merge step at the end of
    /// this function), captured only when that component was actually
    /// dispatched via the `solver.solve_ranked` arm below (i.e. always `None`
    /// when `want_optimality = false`, or when the objective is absent,
    /// lexicographic, or the solve produced no feasible component at all).
    /// The 1st slot's `SolveResult` always reflects the WINNER (best
    /// candidate) merged with every other component — i.e. slot 1 and
    /// `objective_candidates[0]` (slot 4, post cross-merge) describe the same
    /// solution whenever slot 4 is `Some`.
    fn solve_inner(
        &self,
        problem: &ResolutionProblem,
        want_optimality: bool,
    ) -> (
        SolveResult,
        Option<OptimalityStatus>,
        Option<f64>,
        Option<Vec<RankedCandidate>>,
    ) {
        // Optimality/score recovered from the objective component (None on the
        // `want_optimality = false` path or when the objective component is solved
        // via a route that does not surface optimality, e.g. lexicographic staging).
        let mut captured_optimality: Option<OptimalityStatus> = None;
        let mut captured_score: Option<f64> = None;
        // δ (task #5016): the objective component's full best-of-K candidate
        // vector — see the "δ best-of-K propagation" doc section above.
        let mut captured_candidates: Option<Vec<RankedCandidate>> = None;

        // Early exit: no auto params → already solved
        if problem.auto_params.is_empty() {
            return (
                SolveResult::Solved {
                    values: HashMap::new(),
                    unique: true,
                },
                None,
                None,
                None,
            );
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
            return (
                SolveResult::Solved {
                    values: HashMap::new(),
                    unique: true,
                },
                None,
                None,
                None,
            );
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
        // δ (task #5016): values/unique accumulated over every component
        // EXCEPT the one whose candidates got captured (i.e. the shared,
        // non-objective-multistart portion of the merged result) — unioned
        // into EVERY cross-merged candidate below, not just the winner.
        let mut other_values: HashMap<ValueCellId, Value> = HashMap::new();
        let mut other_unique = true;

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
                dependent_cells: Vec::new(),
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
            //
            // γ (task #4804): on the `want_optimality` path, route the
            // objective-bearing component (non-lexicographic) through
            // `solver.solve_ranked` so the domain solver's real `OptimalityStatus`
            // and objective score propagate up.  All other dispatch — every
            // component on the `solve()` path, every non-objective component, and
            // the lexicographic staged path — keeps `solver.solve()` exactly as
            // before (so `solve()` stays byte-for-byte unchanged, I1).
            //
            // δ (task #5016): this branch now RETAINS the full candidate vector
            // (instead of `swap_remove(0)`-and-discard) in `component_candidates`,
            // so `solve_ranked` can cross-merge the whole best-of-K set instead of
            // collapsing to 1 (registry.rs pre-δ). The winner (candidates[0]) is
            // still what feeds `merged_values`/`captured_score` below, so `solve()`
            // and the single-candidate fallback are unaffected.
            let is_objective_component = objective_component == Some(ci);
            let mut component_candidates: Option<Vec<RankedCandidate>> = None;
            let result = match &sub_problem.objective {
                Some(obj) if obj.combination == ObjectiveCombination::Lexicographic => {
                    solve_lexicographic(solver, &sub_problem)
                }
                Some(_) if want_optimality && is_objective_component => {
                    match solver.solve_ranked(&sub_problem) {
                        RankedSolveResult::Ranked {
                            candidates,
                            optimality,
                        } => {
                            // I2: candidates is non-empty; index 0 is the optimum.
                            // assert! (always-on, all build profiles) enforces this contract
                            // so that a solver violating I2 produces a clear diagnostic in
                            // debug AND release builds, rather than the opaque vec-index
                            // panic (task #4871 S3; promoted from debug_assert! per amend).
                            assert!(
                                !candidates.is_empty(),
                                "RankedSolveResult::Ranked must carry >=1 candidate (I2) (registry seam)"
                            );
                            captured_optimality = Some(optimality);
                            captured_score = candidates[0].objective_score;
                            let winner = SolveResult::Solved {
                                values: candidates[0].values.clone(),
                                unique: candidates[0].unique,
                            };
                            // Stash the FULL vector (δ) for the cross-merge step
                            // below; `winner` above already captured candidate 0's
                            // values/unique for the pre-δ merged-result shape.
                            component_candidates = Some(candidates);
                            winner
                        }
                        RankedSolveResult::Infeasible { diagnostics } => {
                            SolveResult::Infeasible { diagnostics }
                        }
                        RankedSolveResult::NoProgress { reason } => {
                            SolveResult::NoProgress { reason }
                        }
                    }
                }
                _ => solver.solve(&sub_problem),
            };

            match result {
                SolveResult::Solved { values, unique } => {
                    all_unique &= unique;
                    match component_candidates {
                        Some(candidates) => {
                            // The objective component: its winner's values are
                            // already folded into `merged_values`; the full
                            // candidate set is deferred to the cross-merge step
                            // below (needs `other_values`/`other_unique` from
                            // every OTHER component first).
                            merged_values.extend(values);
                            captured_candidates = Some(candidates);
                        }
                        None => {
                            // Every other component: folds into both the merged
                            // result AND the shared "other" accumulator that gets
                            // unioned into each cross-merged candidate.
                            merged_values.extend(values.clone());
                            other_values.extend(values);
                            other_unique &= unique;
                        }
                    }
                }
                SolveResult::Infeasible { diagnostics } => {
                    return (SolveResult::Infeasible { diagnostics }, None, None, None);
                }
                SolveResult::NoProgress { reason } => {
                    return (SolveResult::NoProgress { reason }, None, None, None);
                }
            }
        }

        // δ (task #5016): cross-merge the objective component's captured
        // best-of-K set (if any) with the shared non-objective values, so
        // EVERY ranked candidate — not just the winner — carries the full
        // merged-cluster value map. `objective_candidates[0]` (post
        // cross-merge) is by construction exactly `(merged_values, all_unique)`
        // below: `other_values ∪ candidates[0].values == merged_values` and
        // `other_unique && candidates[0].unique == all_unique`, since
        // `merged_values`/`all_unique` above were folded from the SAME
        // `other_values`/`other_unique` plus the SAME winner.
        // Perf: when there is no independent non-objective component,
        // `other_values` is empty and `other_values.clone().extend(c.values)`
        // is exactly `c.values` — skip the clone-and-rehash for every one of
        // the K captured candidates in that (common, single-component
        // merged-cluster) case instead of paying an unconditional
        // allocation+rehash K times over. When `other_values` IS non-empty (a
        // real independent component to cross-merge, e.g. the (c) test
        // fixture), behaviour is unchanged.
        let objective_candidates = captured_candidates.map(|candidates| {
            candidates
                .into_iter()
                .map(|c| {
                    let values = if other_values.is_empty() {
                        c.values
                    } else {
                        let mut values = other_values.clone();
                        values.extend(c.values);
                        values
                    };
                    RankedCandidate {
                        values,
                        objective_score: c.objective_score,
                        unique: c.unique && other_unique,
                    }
                })
                .collect::<Vec<_>>()
        });

        (
            SolveResult::Solved {
                values: merged_values,
                unique: all_unique,
            },
            captured_optimality,
            captured_score,
            objective_candidates,
        )
    }
}

impl ConstraintSolver for SolverRegistry {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        // I1: delegate to the shared core with optimality recovery OFF, which
        // reproduces the historical dispatch path byte-for-byte.
        self.solve_inner(problem, false).0
    }

    /// δ (task #5016) contract: `solve_ranked` is a best-of-K propagation, NOT
    /// a pure single-point projection of `solve()`. When the objective
    /// component is multistart-eligible (`DimensionalSolver::solve_ranked`'s
    /// dim>=2 gate), the FULL candidate set it produces is cross-merged with
    /// the other components' (shared) values into K ranked merged candidates
    /// here — `candidates[0]` is always the best (I2), and for a single-basin
    /// problem (every start converges to the same point) it is operationally
    /// identical to `solve()`; for a multi-basin problem it can be STRICTLY
    /// BETTER (dominance: `candidates[0]`'s score is never worse than
    /// `solve()`'s, but not guaranteed byte-identical — see
    /// `solve_ranked_registry_candidate0_dominates_solve` /
    /// `solve_ranked_multistart_dominates_single_start_solve`). Falls back to
    /// the pre-δ single-candidate lift whenever `solve_inner` captured no
    /// objective-component candidate set (no objective, lexicographic
    /// staging, dim<=1, or a degenerate/all-infeasible solve) — every dim=1
    /// fixture (F-result I1 byte-identical test, B1/B2, BT6) stays on this
    /// unchanged fallback path.
    ///
    /// `candidates[1..]` are NOT deduplicated: they are
    /// `DimensionalSolver::solve_ranked`'s non-winning starts cross-merged
    /// verbatim, so for a single-basin objective (most merged clusters) they
    /// may be near-/byte-identical repeats of the SAME resolved point rather
    /// than distinct alternative designs — best-of-K runner-ups, not a
    /// guaranteed-distinct alternative set. Callers that need genuinely
    /// distinct alternatives must dedupe by resolved-value fingerprint
    /// themselves.
    fn solve_ranked(&self, problem: &ResolutionProblem) -> RankedSolveResult {
        let (result, optimality, objective_score, objective_candidates) =
            self.solve_inner(problem, true);
        match result {
            SolveResult::Solved { values, unique } => {
                // Prefer the optimality recovered from the objective component.
                // Fall back to the conservative default lift when the objective
                // component did not surface one (e.g. lexicographic staging, or a
                // degenerate problem with no solved component): an objective
                // present but unreported maps to a generic `BestFound` whose reason
                // does NOT contain "iteration limit" (so it never spuriously fires
                // `W_SOLVER_OPTIMALITY_UNPROVEN`); no objective maps to
                // `FeasibilityOnly` (invariant I3).
                let optimality = optimality.unwrap_or_else(|| {
                    if problem.objective.is_some() {
                        OptimalityStatus::BestFound {
                            reason: reify_ir::BestFoundReason::Unreported,
                        }
                    } else {
                        OptimalityStatus::FeasibilityOnly
                    }
                });
                // δ: propagate the full cross-merged K-candidate set when
                // `solve_inner` captured one; otherwise fall back to the pre-δ
                // single-candidate lift. `values`/`objective_score`/`unique`
                // here already equal `objective_candidates[0]` whenever the
                // latter is `Some` (see `solve_inner`'s cross-merge comment),
                // so the two arms agree on candidates[0] in every case.
                let candidates = objective_candidates.unwrap_or_else(|| {
                    vec![RankedCandidate {
                        values,
                        objective_score,
                        unique,
                    }]
                });
                RankedSolveResult::Ranked {
                    candidates,
                    optimality,
                }
            }
            // Infeasible / NoProgress map structurally, identical to the default lift.
            non_solved => non_solved
                .into_ranked_pass_through()
                .expect("Solved arm handled above"),
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
            cost_robustness_lambda: None,
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
            cost_robustness_lambda: None,
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
            dependent_cells: Vec::new(),
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
    // I-UNITS backstop (PRD D2/I-UNITS, task α #5018): this does NOT re-diagnose —
    // the compile-time gate (E_OBJECTIVE_MIXED_DIMENSION, `check_objective_dimension_coherence`
    // in reify-compiler/src/entity.rs) is the sole user-facing diagnostic and already
    // rejects every authored incoherent multi-term objective before it can reach a
    // solve. This assert only guards the upstream-guaranteed invariant against a
    // future ungated ObjectiveSet (e.g. hand-built or solve-time-synthesized).
    debug_assert!(
        reify_ir::objective_terms_coherent(rank_terms).is_ok(),
        "eval_rank_cost: I-UNITS violated (task α #5018) — objective_terms_coherent() \
         reported Err for a set that reached the fold; the compile-time gate \
         (E_OBJECTIVE_MIXED_DIMENSION, reify-compiler/src/entity.rs) should have \
         rejected this ObjectiveSet before it ever reached eval_rank_cost"
    );
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
