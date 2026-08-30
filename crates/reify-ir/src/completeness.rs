//! Solution-set completeness carrier
//! (PRD `docs/prds/v0_6/solution-set-completeness.md` §3.1).
//!
//! This module is a SIBLING to [`crate::ranked`], following the pattern that PRD
//! established for exactly this reason: [`crate::SolveResult`] and
//! [`crate::constraint::ConstraintSolver::solve`] are FROZEN (ranked-solve-result
//! invariant I1), so a new axis of information lands in its own module and is
//! threaded additively rather than by widening the frozen types.
//!
//! # The axis
//!
//! [`Completeness`] is **orthogonal** to [`crate::ranked::OptimalityStatus`]
//! (design decision D6): `OptimalityStatus` says *how good the best one is*,
//! `Completeness` says *how many there are and whether that count was proven*.
//! A solver can hold a tight optimality certificate for a point it found while
//! having established nothing at all about how many other solutions exist — that
//! pair of independent claims is why the two enums do not collapse into one.
//!
//! # Precedent being generalised
//!
//! `reify_constraints::cpsat`'s `SolveAllResult::Enumerated { solutions, complete }`
//! already carries this distinction on the discrete side, and its `NotEnumerable`
//! variant already exists to stop "found none" rendering as "none exist". This
//! module lifts that same doctrine to the continuous side and to the shared
//! `reify-ir` seam so both sides speak one vocabulary (PRD §0.3).
//!
//! # Status at this leaf (α, #6706)
//!
//! Carrier types only. No producer computes a real verdict yet: every existing
//! producer reports [`Completeness::not_attempted`], which is behaviour-preserving
//! (boundary test BT13). The enumerator that actually establishes `Exhaustive` /
//! `Refuted` arrives at later leaves of the same PRD.

use reify_core::identity::{ConstraintNodeId, ValueCellId};

/// How much of the solution set the producer actually established.
///
/// Orthogonal to [`crate::ranked::OptimalityStatus`] (D6): that says how good the
/// best one is, this says how many there are and whether that count was proven.
///
/// # Invariants (PRD §3.2)
///
/// The invariants this type exists to make checkable are C1–C4; each has a
/// method rather than only a paragraph, so a producer cannot satisfy the prose
/// while violating the rule:
///
/// - **C1** — [`Completeness::derived_unique`] is the single site from which
///   `unique` may be computed.
/// - **C2** — [`Completeness::permits_proven_optimal`].
/// - **C3** — `Refuted` and `Partial` are structurally distinct variants and
///   render distinctly via [`Completeness::describe`].
/// - **C4** — [`SolutionSet::proven_count`] is the only sanctioned route to a total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completeness {
    /// Every solution inside the searched domain is present in the set.
    ///
    /// This is the ONLY verdict from which `unique` (C1, see
    /// [`Completeness::derived_unique`]) or
    /// [`crate::ranked::OptimalityStatus::ProvenOptimal`] (C2, see
    /// [`Completeness::permits_proven_optimal`]) may be derived. Every other
    /// verdict leaves the size of the solution set unknown, and a claim about an
    /// unknown-size set is the false-completeness claim this axis exists to prevent.
    ///
    /// Note the scope: *inside the searched domain*. `Exhaustive` is not a claim
    /// about the whole of parameter space, only about the domain the producer
    /// actually searched.
    Exhaustive,
    /// More solutions may exist; how many is unknown. **Never a count claim** (C4).
    ///
    /// A `Partial` verdict may report the solutions it found — that is an honest
    /// statement about the search — but it may never report how many exist, and no
    /// user-facing string derived from it may imply a total. This is mechanised by
    /// [`SolutionSet::proven_count`] returning `None` for every `Partial`, so a
    /// consumer that wants a total has to handle the "not established" case.
    ///
    /// `reason` names the runtime condition that stopped the producer, so the
    /// decline is attributable rather than silent (INV-SF-3).
    Partial {
        /// Why the producer declined to establish the set. See [`PartialReason`].
        reason: PartialReason,
    },
    /// Proven that **NO** solution exists in the searched domain, and which
    /// constraint drove the domain empty.
    ///
    /// # C3 — a refutation is a proof; an empty `Partial` is not
    ///
    /// `{ solutions: [], Refuted }` means no solution exists. `{ solutions: [],
    /// Partial { .. } }` means the search found none and proved nothing. These must
    /// never render as the same diagnostic: one is a statement about the model, the
    /// other is a statement about the search. That collapse is precisely what
    /// `reify_constraints::cpsat`'s `SolveAllResult::NotEnumerable` variant exists
    /// to prevent on the discrete side — this variant generalises it.
    ///
    /// Discriminated numerically by [`SolutionSet::proven_count`]: `Some(0)` for
    /// `Refuted` versus `None` for `Partial`.
    Refuted {
        /// The constraint whose narrowing emptied the searched domain.
        narrowing: ConstraintNodeId,
    },
}

/// Why a producer declined to establish the full solution set
/// (the payload of [`Completeness::Partial`], PRD §3.1).
///
/// Every variant except [`PartialReason::NotAttempted`] names a runtime condition
/// that **can clear** — raise the budget, bound the domain, land an interval form —
/// which is what makes a `Partial` verdict attributable rather than a shrug
/// (INV-SF-3, INV-SF-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartialReason {
    /// The subdivision hit its node budget with boxes still unresolved.
    ///
    /// Clears by raising the budget. Note the budget is counted in nodes, never in
    /// seconds (C7), so this verdict does not vary with machine load.
    BoxBudgetExhausted,
    /// The component's dimension exceeds the enumeration envelope.
    ///
    /// A diagnosed decline, never a silent skip: the solve still proceeds from the
    /// underived box, but the set is not claimed.
    DimensionAboveEnvelope {
        /// The component's dimension (number of autos), which exceeded the cap.
        dims: usize,
    },
    /// An auto's domain is unbounded, so no finite box could be searched.
    DomainUnbounded {
        /// The auto-parameter whose domain has no finite bound.
        param: ValueCellId,
    },
    /// A constraint has no sound interval form; the box cannot be trusted.
    ///
    /// Soundness is one-directional (C8): interval arithmetic over-approximates, so
    /// a constraint without a sound interval form makes every box verdict
    /// underivable, not merely imprecise.
    NotIntervalRepresentable {
        /// The constraint that has no sound interval form.
        constraint: ConstraintNodeId,
    },
    /// A discrete leaf's continuous sub-problem was not enumerated (see PRD §3.5).
    ///
    /// This is the honest verdict for a budget-bounded inner argmin under a
    /// discrete outer enumeration: the outer enumeration may be complete while the
    /// inner set is not, and the composition law's meet says the whole is `Partial`.
    InnerSolveUnproven,
    /// Only the legacy perturbation probe ran — one re-solve from one reflected
    /// anchor. Establishes nothing about the set.
    ProbeOnly,
    /// No completeness reasoning was attempted by this driver.
    ///
    /// **Migration state only** — see [`Completeness::not_attempted`], which is the
    /// single named home for this verdict and carries the full INV-SF-4/INV-SF-5
    /// resolution.
    NotAttempted,
}
