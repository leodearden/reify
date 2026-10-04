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
/// For the three variants #4871 migrated, `describe()` returns the **exact** strings
/// that were previously inlined, so the user-facing diagnostic message is byte-identical
/// before and after the migration.  Variants added later (see [`Self::EnumerationBudget`])
/// carry wording of their own and make no such claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BestFoundReason {
    /// The derivative-free solver exhausted its iteration budget before the simplex
    /// converged.
    IterationLimit,
    /// An exact solver stopped at a node/enumeration cap with part of the discrete
    /// search space never visited, so the best candidate it saw may not be the global
    /// optimum (PRD `docs/prds/v0_6/discrete-cost-minimisation.md` §4.2).
    ///
    /// Distinct from [`Self::IterationLimit`] because the account differs, not just the
    /// wording: nothing here is derivative-free, and nothing iterated to a limit.
    EnumerationBudget,
    /// The solver converged within the iteration budget (no optimality proof, but not
    /// iteration-limited).
    ConvergedWithinBudget,
    /// The solver does not report an optimality status (default lift for solvers that
    /// only implement `ConstraintSolver::solve`).
    Unreported,
}

impl BestFoundReason {
    /// Whether the solver halted because it ran out of a search budget, leaving part of
    /// the search undone.  This is the whole gate for `W_SOLVER_OPTIMALITY_UNPROVEN`:
    /// a solve that converged, or that never said why it stopped, is not flagged.
    pub fn stopped_at_budget(&self) -> bool {
        match self {
            BestFoundReason::IterationLimit | BestFoundReason::EnumerationBudget => true,
            BestFoundReason::ConvergedWithinBudget | BestFoundReason::Unreported => false,
        }
    }

    /// Returns the human-readable reason string.  For the three variants migrated by
    /// #4871 these are identical to the strings formerly inlined at the diagnostic site.
    pub fn describe(&self) -> &'static str {
        match self {
            BestFoundReason::IterationLimit => {
                "iteration limit reached; derivative-free solver cannot prove global optimality"
            }
            BestFoundReason::EnumerationBudget => {
                "enumeration budget reached; the discrete search space was not exhausted, so global optimality is unproven"
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
    ///
    /// # Invariant C2 — this requires [`crate::Completeness::Exhaustive`]
    ///
    /// See [`crate::Completeness::permits_proven_optimal`]. `ProvenOptimal` asserts
    /// two independent things: that this candidate is optimal, **and** that nothing
    /// outside the set could beat it. The second is a completeness claim, and only
    /// `Exhaustive` supplies it.
    ///
    /// **An optimality certificate is not a substitute for that.** A first-order
    /// stationarity certificate — a vanishing projected-gradient norm — is *local*:
    /// it says the gradient vanishes at this point and says nothing whatsoever
    /// about other basins. A stationary point plus an unenumerated domain is
    /// exactly the false-completeness claim the completeness axis exists to
    /// prevent, so a stationarity certificate justifies at most
    /// [`OptimalityStatus::BestFound`], never this variant. Promoting on
    /// stationarity alone is the specific wrong refactor this note pre-empts.
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
///
/// The `Infeasible` and `NoProgress` arms carry no `completeness` field: neither
/// is a solution SET, so there is nothing for the axis to describe. `Infeasible`
/// is already the strongest emptiness claim the old vocabulary could make; the
/// verdict that says *proven* empty and names the narrowing constraint is
/// [`crate::Completeness::Refuted`].
///
/// # OPEN SEAM — no arm of this enum can carry `Refuted` yet (α #6706)
///
/// A well-formed [`crate::Completeness::Refuted`] set carries an **empty**
/// `solutions` (see [`crate::SolutionSet::completeness`]), while I2 requires
/// `Ranked.candidates` to be NON-empty — enforced by always-on `assert!` at both
/// consumption seams (reify-eval's `engine_eval.rs`, reify-constraints'
/// `registry.rs`). So `Refuted` is representable in [`crate::SolutionSet`] but in
/// no arm of this enum today: `Ranked { candidates: [], .. }` would violate I2,
/// and `Infeasible`/`NoProgress` have no field to put it in.
///
/// That gap is deliberate at α, which ships the carrier and wires no producer.
/// The leaf that first PRODUCES a refutation — ε #6710 → #6900, refutation by
/// subdivision — owns the choice between the two resolutions, and must make it
/// explicitly rather than smuggling a dummy candidate past I2 (which would be
/// exactly the false-completeness claim this axis exists to prevent):
///
/// - declare `Ranked { candidates: [], completeness: Refuted { .. } }` the
///   sanctioned I2 exemption and relax both asserts to admit precisely that
///   shape, **or**
/// - widen `Infeasible` to `{ diagnostics, completeness }` and route the
///   refutation there, leaving I2 and both asserts untouched.
///
/// ε's charter emits the refutation BEFORE any solver iteration, which the second
/// option fits without touching I2 — but the decision is ε's, made with its
/// fixture in hand, not α's to pre-empt.
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
        /// How much of the solution set was actually established
        /// (solution-set-completeness PRD §3.1, task α #6706).
        ///
        /// Additive and **orthogonal** to `optimality` (D6): `optimality` says how
        /// good the best candidate is, `completeness` says how many solutions there
        /// are and whether that count was proven. A solver can hold a tight
        /// optimality certificate for a point while having established nothing
        /// about how many other solutions exist.
        ///
        /// Producers that do not reason about the set report
        /// [`crate::Completeness::not_attempted`], which is behaviour-preserving
        /// (BT13). Note that `candidates.len()` is **not** a solution count — the
        /// list is not deduplicated until ζ #6711 → #6902 — so `completeness` must
        /// never be
        /// combined with it to derive `unique`; see
        /// [`crate::Completeness::derived_unique`].
        completeness: crate::completeness::Completeness,
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
