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

impl Completeness {
    /// Whether this verdict is [`Completeness::Exhaustive`].
    ///
    /// Exposed as a named predicate rather than leaving callers to `matches!` so
    /// the two rules that key on it — C1 ([`Self::derived_unique`]) and C2
    /// ([`Self::permits_proven_optimal`]) — read as the invariants they are.
    pub const fn is_exhaustive(&self) -> bool {
        matches!(self, Completeness::Exhaustive)
    }

    /// **Invariant C1 — `unique` is derived, never asserted.**
    ///
    /// `unique == (completeness == Exhaustive && solutions.len() == 1)`.
    ///
    /// This function is **THE single site** from which any producer may compute
    /// `unique`. No producer may write `unique: true` from any other reasoning —
    /// not from a hardcoded literal, not from local Jacobian rank, not from a
    /// one-shot perturbation probe that failed to find a second point. Each of
    /// those is a claim about the *whole* solution set derived from *local*
    /// evidence, which is the false-completeness shape this axis exists to prevent.
    ///
    /// The sweep that enforces this across in-tree producers is **#6903**; until it
    /// lands, producers that have not opted in report
    /// [`Completeness::not_attempted`] and this function correctly returns `false`
    /// for them.
    ///
    /// # `solution_count` means DISTINCT solutions
    ///
    /// The count is the number of **distinct** solutions — basins, in the sense of
    /// invariant C5 (basin identity is the containing verified box, design decision
    /// D3). It is **NOT**
    /// [`crate::ranked::RankedSolveResult::Ranked`]`::candidates.len()`, which is
    /// not deduplicated: a K-start multistart can converge K times into one basin
    /// and produce K near-identical candidates. Feeding that length in here would
    /// turn one solution into a claim of K, and — worse, in the other direction —
    /// would make a genuinely unique solution look non-unique.
    ///
    /// Deduplication arrives with the box-based basin identity at ζ (#6711). That
    /// is precisely why no `derived_unique` is offered on
    /// [`crate::ranked::RankedSolveResult`] itself: there is no honest count to
    /// pass it there yet.
    pub const fn derived_unique(&self, solution_count: usize) -> bool {
        self.is_exhaustive() && solution_count == 1
    }

    /// **Invariant C2 — `ProvenOptimal` requires `Exhaustive`.**
    ///
    /// Whether a producer holding this verdict may set
    /// [`crate::ranked::OptimalityStatus::ProvenOptimal`]. Only
    /// [`Completeness::Exhaustive`] does.
    ///
    /// C2 is a **conjunction, not an assertion**: an optimality certificate and a
    /// completeness verdict are independent claims (D6), and `ProvenOptimal` asserts
    /// *both* — that this point is optimal *and* that nothing outside the searched
    /// set could beat it. A producer with an independent certificate (an exact MILP
    /// duality gap) may still claim it, but it must then also justify `Exhaustive`
    /// rather than treating the certificate as a substitute for the enumeration.
    pub const fn permits_proven_optimal(&self) -> bool {
        self.is_exhaustive()
    }

    /// The verdict for a producer that does no completeness reasoning at all:
    /// `Partial { NotAttempted }`.
    ///
    /// # This is a MIGRATION STATE ONLY (INV-SF-4 / INV-SF-5)
    ///
    /// Every other [`PartialReason`] names a runtime condition that **can clear** —
    /// raise the budget, bound the domain, land an interval form. `NotAttempted`
    /// names none: nothing at runtime clears it, so it is *permanently*
    /// unattributable, which is exactly the structural-indeterminacy shape
    /// INV-SF-4 `indeterminate-attributable-transient` forbids.
    ///
    /// The resolution recorded in PRD §8.1 is that it is a migration state and
    /// nothing more. It is retired from every in-tree producer by the conformance
    /// sweep, after which it survives **solely** for the defaulted
    /// [`crate::constraint::ConstraintSolver::solve_ranked`] lift — where it is not
    /// a placeholder at all but the honest verdict "this solver was never asked".
    ///
    /// There is deliberately **no** `Default` impl for [`Completeness`]: a
    /// `..Default::default()` shorthand would let this sentinel be acquired
    /// silently, and the sweep that retires it depends on every site that reports
    /// it being spelled out and greppable.
    ///
    /// Being a sentinel default, INV-SF-5 `placeholders-owned-and-loud` requires it
    /// to carry an owner for as long as any in-tree producer still reports it; that
    /// owner is the cite below.
    // TODO(#6903): retire Partial{NotAttempted} from every in-tree producer.
    // Every call site of `Completeness::not_attempted()` outside the default
    // `solve_ranked` lift is one of that sweep's targets — grep the constructor
    // to enumerate them. #6903 also carries the assertion that no in-tree
    // ConstraintSolver impl reports this verdict.
    pub const fn not_attempted() -> Completeness {
        Completeness::Partial {
            reason: PartialReason::NotAttempted,
        }
    }

    /// Human-readable rendering of this verdict.
    ///
    /// **C3 at the rendering layer:** a [`Completeness::Refuted`] verdict must never
    /// render like a [`Completeness::Partial`] one. One is a proof about the model,
    /// the other is a report about the search, and collapsing them is exactly the
    /// wrong this axis exists to correct.
    ///
    /// Returns `String` rather than the `&'static str` that
    /// [`crate::ranked::BestFoundReason::describe`] returns — see
    /// [`PartialReason::describe`] for why that divergence is deliberate.
    pub fn describe(&self) -> String {
        match self {
            Completeness::Exhaustive => {
                "every solution in the searched domain was enumerated".to_string()
            }
            Completeness::Partial { reason } => {
                format!("solution set not established: {}", reason.describe())
            }
            Completeness::Refuted { narrowing } => format!(
                "proven infeasible: no solution exists in the searched domain \
                 (narrowed to empty by constraint {narrowing})"
            ),
        }
    }
}

impl PartialReason {
    /// Human-readable rendering of why the set was not established.
    ///
    /// # Why `String`, not `&'static str`
    ///
    /// [`crate::ranked::BestFoundReason::describe`] returns `&'static str`, and this
    /// method deliberately diverges rather than drifting: three reasons carry
    /// payloads (`dims`, `param`, `constraint`) that MUST be interpolated for the
    /// decline to be attributable (INV-SF-3). A `&'static str` here would render two
    /// different declines as one indistinguishable string, which is the silent-skip
    /// shape that invariant forbids.
    pub fn describe(&self) -> String {
        match self {
            PartialReason::BoxBudgetExhausted => {
                "subdivision budget exhausted with boxes still unresolved".to_string()
            }
            PartialReason::DimensionAboveEnvelope { dims } => format!(
                "component dimension {dims} exceeds the enumeration envelope"
            ),
            PartialReason::DomainUnbounded { param } => {
                format!("domain of {param} is unbounded, so no finite box could be searched")
            }
            PartialReason::NotIntervalRepresentable { constraint } => format!(
                "constraint {constraint} has no sound interval form, so no box verdict is trustworthy"
            ),
            PartialReason::InnerSolveUnproven => {
                "the continuous sub-problem under this discrete choice was not enumerated"
                    .to_string()
            }
            PartialReason::ProbeOnly => {
                "only the legacy perturbation probe ran; nothing was established about the set"
                    .to_string()
            }
            PartialReason::NotAttempted => {
                "this solver does not reason about solution-set completeness".to_string()
            }
        }
    }
}

/// A solution set paired with the verdict on how much of it was established
/// (PRD §3.1).
///
/// This is the carrier that makes invariants C1 and C4 mechanical rather than
/// advisory: the only route to a *uniqueness* claim is [`Self::unique`], and the
/// only route to a *total* is [`Self::proven_count`], which returns `None`
/// precisely when the total was not proven.
///
/// Not `PartialEq`, because [`crate::ranked::RankedCandidate`] is not (it holds
/// `f64` scores); compare the [`Self::completeness`] field directly when a test
/// needs verdict equality.
#[derive(Debug, Clone)]
pub struct SolutionSet {
    /// The solutions the producer actually found.
    ///
    /// This is "what I found", not "what exists" — reading a total off this
    /// length is the C4 violation. Use [`Self::proven_count`] for a total.
    pub solutions: Vec<crate::ranked::RankedCandidate>,
    /// How much of the solution set the producer established.
    ///
    /// A well-formed [`Completeness::Refuted`] set carries an **empty**
    /// `solutions`: a proof that no solution exists in the searched domain is
    /// contradicted by holding one. [`Self::proven_count`] deliberately reports
    /// `Some(0)` for `Refuted` from the *verdict*, so a producer that violates
    /// this is visible in the mismatch against `solutions.len()` rather than
    /// silently masked.
    pub completeness: Completeness,
}

impl SolutionSet {
    /// **Invariant C1** — whether this set is a uniqueness claim.
    ///
    /// Delegates to [`Completeness::derived_unique`] with this set's own length.
    /// It deliberately does **not** re-derive the rule: C1 has exactly one
    /// implementation site, and a second copy here is how the two would drift.
    pub fn unique(&self) -> bool {
        self.completeness.derived_unique(self.solutions.len())
    }

    /// **Invariant C4 — no unproven count.** The number of solutions that
    /// **exist**, or `None` when that was never established.
    ///
    /// - [`Completeness::Exhaustive`] → `Some(solutions.len())`: the search covered
    ///   the domain, so what was found is what exists.
    /// - [`Completeness::Refuted`] → `Some(0)`: proven empty.
    /// - [`Completeness::Partial`] → `None`: more may exist and how many is unknown.
    ///
    /// This is the **only sanctioned route to a solution TOTAL**. Returning `None`
    /// for `Partial` is what makes C4 mechanical instead of advisory: a consumer
    /// that wants to print "there are N solutions" is forced to handle the
    /// not-established case, so no user-facing string can imply a total the
    /// producer never proved.
    ///
    /// `solutions.len()` remains available as "how many were **found**", which C4
    /// explicitly permits — a `Partial` verdict may report what the search
    /// produced. The two are different questions and this method answers only the
    /// second.
    ///
    /// Note the `Refuted` arm reads the verdict, not the vector: see the
    /// [`Self::completeness`] field docs for why a well-formed `Refuted` carries an
    /// empty `solutions` and why that requirement is stated rather than enforced by
    /// silently returning `solutions.len()`.
    pub fn proven_count(&self) -> Option<usize> {
        match self.completeness {
            Completeness::Exhaustive => Some(self.solutions.len()),
            Completeness::Refuted { .. } => Some(0),
            Completeness::Partial { .. } => None,
        }
    }
}
