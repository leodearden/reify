//! Ranked solve result carrier types (PRD `docs/prds/v0_6/ranked-solve-result.md` §3.1).
//!
//! This module is a SIBLING to [`crate::constraint`]'s [`crate::SolveResult`] —
//! `SolveResult` and [`crate::constraint::ConstraintSolver`] are FROZEN (invariant I1);
//! `RankedSolveResult` and its companions live here to keep `constraint.rs` focused.
//!
//! # Deliverables
//! - [`OptimalityStatus`] — task α (this task, #4801)
//! - [`RankedCandidate`] — task α (this task, #4801)
//! - [`RankedSolveResult`] — task α (this task, #4801)
//!
//! The `solve_ranked` trait method is task β; engine wiring + the
//! `W_SOLVER_OPTIMALITY_UNPROVEN` diagnostic is task γ.

/// Structured reason for [`OptimalityStatus::BestFound`].
///
/// Replaces the former free-form `String` (PRD OQ#4 deferral resolved in task #4871):
/// the engine consumer now branches on the reason, so a type-safe enum is warranted.
/// `describe()` returns the **exact** strings that were previously inlined, so the
/// user-facing diagnostic message is byte-identical before and after the migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BestFoundReason {
    /// The derivative-free solver exhausted its iteration budget before the simplex
    /// converged.  This is the gate condition for `W_SOLVER_OPTIMALITY_UNPROVEN`.
    IterationLimit,
    /// The solver converged within the iteration budget (no optimality proof, but not
    /// iteration-limited).  Does NOT trigger `W_SOLVER_OPTIMALITY_UNPROVEN`.
    ConvergedWithinBudget,
    /// The solver does not report an optimality status (default lift for solvers that
    /// only implement `ConstraintSolver::solve`).  Does NOT trigger the warning.
    Unreported,
}

impl BestFoundReason {
    /// Returns the human-readable reason string (identical to the former inlined strings).
    pub fn describe(&self) -> &'static str {
        match self {
            BestFoundReason::IterationLimit => {
                "iteration limit reached; derivative-free solver cannot prove global optimality"
            }
            BestFoundReason::ConvergedWithinBudget => {
                "converged within iteration budget; derivative-free solver cannot prove global optimality"
            }
            BestFoundReason::Unreported => "solver does not report optimality",
        }
    }
}

/// Describes the quality of the ranked solution set returned by a solver.
///
/// # Invariant I3 (producer-side contract)
///
/// - `ProvenOptimal` MAY only be set by a producer that has **proven global
///   optimality** (e.g. an exact MILP solver with a duality certificate).
///   Derivative-free / budget-truncated solvers MUST use `BestFound`.
/// - `FeasibilityOnly` MUST be used iff no objective governs the solve (i.e.
///   the [`crate::ResolutionProblem::objective`] is `None`).
/// - Violating I3 triggers the `W_SOLVER_OPTIMALITY_UNPROVEN` diagnostic wired
///   in task γ.
#[derive(Debug, Clone)]
pub enum OptimalityStatus {
    /// A proof of global optimality was obtained (e.g. branch-and-bound gap = 0).
    ProvenOptimal,
    /// The best result found within the given budget, without a proof of optimality.
    ///
    /// `reason` is a structured enum (see [`BestFoundReason`]) describing the stopping
    /// criterion.  Use `reason.describe()` to get the human-readable string.
    BestFound { reason: BestFoundReason },
    /// No objective governed this solve; the ranking contains a single feasible
    /// point with no ordering claim.
    FeasibilityOnly,
}

/// A single candidate in a [`RankedSolveResult::Ranked`] list.
///
/// # Field contracts (producer-side — enforced by task β/γ)
///
/// - `values`: resolved auto-param values; same shape as
///   [`crate::SolveResult`]`::Solved.values`.
/// - `objective_score`: ranking scalar — **LOWER is better**. Producers
///   normalise maximisation problems to minimisation before populating this field
///   (invariant I2). `None` only for feasibility-only candidates (invariant I4).
/// - `unique`: carries [`crate::SolveResult`]`::Solved.unique` semantics —
///   `true` iff the solver certifies no other solution with the same objective
///   value exists.
#[derive(Debug, Clone)]
pub struct RankedCandidate {
    /// Resolved values for each auto-parameter.
    pub values: std::collections::HashMap<reify_core::identity::ValueCellId, crate::value::Value>,
    /// Objective score for ranking; lower is better. `None` for feasibility-only candidates.
    pub objective_score: Option<f64>,
    /// Whether the solver certifies this candidate is unique.
    pub unique: bool,
}

/// The result of a ranked solve; sibling to [`crate::SolveResult`] (I1: SolveResult unchanged).
///
/// # Invariant I2 (producer-side — enforced by task β/γ)
///
/// For the `Ranked` variant:
/// - `candidates` is **non-empty**.
/// - `candidates` are ordered **best-first by ascending `objective_score`**;
///   index 0 is the selected optimum.
/// - Feasibility-only rankings are size-1 with no ordering claim.
#[derive(Debug, Clone)]
pub enum RankedSolveResult {
    /// One or more ranked candidates were found.
    ///
    /// See invariant I2 above for ordering and non-empty contracts.
    Ranked {
        /// Ranked candidates, best-first (index 0 = optimum).
        candidates: Vec<RankedCandidate>,
        /// Quality of the solution set.
        optimality: OptimalityStatus,
    },
    /// The constraint system has no feasible solution.
    Infeasible {
        /// Diagnostics explaining which constraints are unsatisfiable.
        diagnostics: Vec<reify_core::diagnostics::Diagnostic>,
    },
    /// The solver made no progress (e.g. could not find an initial feasible point).
    NoProgress {
        /// Brief reason (e.g. `"iteration limit, no feasible point"`).
        reason: String,
    },
}
