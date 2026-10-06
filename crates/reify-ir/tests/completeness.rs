//! Integration tests for the `Completeness` / `SolutionSet` carrier
//! (PRD `docs/prds/v0_6/solution-set-completeness.md` §3.1, invariants C1–C4).
//!
//! Assertions stay on the completeness axis: no test in this file pins a
//! [`reify_ir::BestFoundReason`] variant by name.

use reify_core::identity::{ConstraintNodeId, ValueCellId};
use reify_ir::{Completeness, PartialReason, RankedCandidate, SolutionSet, Value};
use std::collections::HashMap;

/// `Eq` (not just `PartialEq`): the composition law at ι #6715 → #6903 keys on
/// completeness verdicts, so they must be usable as map keys.
const _: () = {
    const fn assert_eq_bound<T: Eq>() {}
    assert_eq_bound::<Completeness>();
    assert_eq_bound::<PartialReason>();
};

// ── Shared fixtures ──────────────────────────────────────────────────────────

/// The full §3.1 `PartialReason` set, one value per variant.
fn all_partial_reasons() -> Vec<PartialReason> {
    vec![
        PartialReason::BoxBudgetExhausted,
        PartialReason::DimensionAboveEnvelope { dims: 3 },
        PartialReason::DomainUnbounded {
            param: ValueCellId::new("Part", "x"),
        },
        PartialReason::NotIntervalRepresentable {
            constraint: ConstraintNodeId::new("Part", 0),
        },
        PartialReason::InnerSolveUnproven,
        PartialReason::ProbeOnly,
        PartialReason::NotAttempted,
    ]
}

// ── C1: `unique` is derived, never asserted (§3.2) ───────────────────────────

/// C1 truth table. `derived_unique` must be true for **exactly one** cell of the
/// grid {Exhaustive, Partial(each of the 7 reasons), Refuted} × {0, 1, 2, 5}:
/// `Exhaustive` with a count of 1.
///
/// Enumerating the whole grid (rather than spot-checking) is the point: C1 says
/// no producer may write `unique: true` from any other reasoning, so every other
/// cell being false is the actual contract.
#[test]
fn derived_unique_is_true_for_exactly_one_grid_cell() {
    let counts = [0usize, 1, 2, 5];

    for &count in &counts {
        assert_eq!(
            Completeness::Exhaustive.derived_unique(count),
            count == 1,
            "Exhaustive with count {count} must be unique iff count == 1"
        );

        assert!(
            !Completeness::Refuted {
                narrowing: ConstraintNodeId::new("Bracket", 0)
            }
            .derived_unique(count),
            "Refuted with count {count} must never be unique"
        );

        for reason in all_partial_reasons() {
            assert!(
                !Completeness::Partial {
                    reason: reason.clone()
                }
                .derived_unique(count),
                "Partial{{{reason:?}}} with count {count} must never be unique"
            );
        }
    }
}

/// The four honesty cases the PRD §0.1 bool cannot express, called out by name so
/// a regression names the wrong it reintroduces.
#[test]
fn derived_unique_rejects_the_four_dishonest_claims() {
    // Proved two solutions exist — that is the opposite of unique.
    assert!(
        !Completeness::Exhaustive.derived_unique(2),
        "Exhaustive + 2: proved two solutions, so not unique"
    );

    // Found one, proved nothing about how many exist. Today's false `unique: true`.
    assert!(
        !Completeness::Partial {
            reason: PartialReason::ProbeOnly
        }
        .derived_unique(1),
        "Partial{{ProbeOnly}} + 1: found one, established nothing"
    );
    assert!(
        !Completeness::Partial {
            reason: PartialReason::NotAttempted
        }
        .derived_unique(1),
        "Partial{{NotAttempted}} + 1: no completeness reasoning was attempted"
    );

    // Refutation is not uniqueness — it proves none exist, not that one does.
    assert!(
        !Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0)
        }
        .derived_unique(0),
        "Refuted + 0: proved none exist; refutation is not uniqueness"
    );

    // The one true cell, for contrast.
    assert!(Completeness::Exhaustive.derived_unique(1));
}

#[test]
fn is_exhaustive_is_true_only_for_exhaustive() {
    assert!(Completeness::Exhaustive.is_exhaustive());
    assert!(
        !Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0)
        }
        .is_exhaustive()
    );
    for reason in all_partial_reasons() {
        assert!(
            !Completeness::Partial {
                reason: reason.clone()
            }
            .is_exhaustive(),
            "Partial{{{reason:?}}} must not be exhaustive"
        );
    }
}

// ── SolutionSet (§3.1) and C4 — no unproven count ────────────────────────────

/// Local candidate fixture, modelled on `tests/ranked_solve_result.rs`'s
/// `make_candidate`. The optimality axis is deliberately not involved.
fn make_candidate(x: f64) -> RankedCandidate {
    let mut values = HashMap::new();
    values.insert(ValueCellId::new("Part", "x"), Value::length(x));
    RankedCandidate {
        values,
        objective_score: Some(x),
        unique: false,
    }
}

fn candidates(n: usize) -> Vec<RankedCandidate> {
    (0..n).map(|i| make_candidate(i as f64 / 100.0)).collect()
}

fn refuted_set() -> SolutionSet {
    SolutionSet::refuted(ConstraintNodeId::new("Bracket", 0))
}

const COUNTS: [usize; 4] = [0, 1, 2, 5];

/// Every set the constructors can build over [`COUNTS`]: `Exhaustive` and each
/// `Partial` reason at each count, plus the one `Refuted` shape.
fn every_constructible_set() -> Vec<SolutionSet> {
    let mut sets = vec![refuted_set()];
    for n in COUNTS {
        sets.push(SolutionSet::exhaustive(candidates(n)));
        sets.extend(
            all_partial_reasons()
                .into_iter()
                .map(|reason| SolutionSet::partial(candidates(n), reason)),
        );
    }
    sets
}

/// Each constructor attaches the verdict it names and keeps what it was given;
/// `refuted` builds the empty set, the only well-formed `Refuted` shape.
#[test]
fn constructors_attach_the_verdict_they_name() {
    let exhaustive = SolutionSet::exhaustive(candidates(2));
    assert_eq!(exhaustive.completeness(), &Completeness::Exhaustive);
    assert_eq!(exhaustive.solutions().len(), 2);

    for reason in all_partial_reasons() {
        let partial = SolutionSet::partial(candidates(3), reason.clone());
        assert_eq!(
            partial.completeness(),
            &Completeness::Partial {
                reason: reason.clone()
            }
        );
        assert_eq!(partial.solutions().len(), 3, "Partial{{{reason:?}}}");
    }

    let narrowing = ConstraintNodeId::new("Flange", 4);
    let refuted = SolutionSet::refuted(narrowing.clone());
    assert_eq!(refuted.completeness(), &Completeness::Refuted { narrowing });
    assert!(refuted.solutions().is_empty());
}

/// `SolutionSet::unique()` must agree with
/// `Completeness::derived_unique(solutions.len())` for every set the
/// constructors can build — it delegates, it does not re-derive.
#[test]
fn solution_set_unique_agrees_with_derived_unique() {
    for set in every_constructible_set() {
        assert_eq!(
            set.unique(),
            set.completeness().derived_unique(set.solutions().len()),
            "SolutionSet::unique must delegate to derived_unique for {:?} with {} solutions",
            set.completeness(),
            set.solutions().len()
        );
    }

    // The three named cells, spelled out.
    assert!(SolutionSet::exhaustive(candidates(1)).unique());
    assert!(
        !SolutionSet::partial(candidates(1), PartialReason::ProbeOnly).unique(),
        "one candidate + ProbeOnly is not a uniqueness claim"
    );
    assert!(
        !SolutionSet::exhaustive(candidates(2)).unique(),
        "two proven solutions is the opposite of unique"
    );
}

/// C4 — a `Partial` verdict may never report a total.
#[test]
fn proven_count_is_none_for_every_partial_reason() {
    // Exhaustive: the count IS proven, and it is the set's own length.
    for n in COUNTS {
        assert_eq!(
            SolutionSet::exhaustive(candidates(n)).proven_count(),
            Some(n)
        );
    }

    // Refuted: proven zero.
    assert_eq!(refuted_set().proven_count(), Some(0));

    // Partial: no total, for every reason and regardless of what was found.
    for reason in all_partial_reasons() {
        for n in COUNTS {
            let set = SolutionSet::partial(candidates(n), reason.clone());
            assert_eq!(
                set.proven_count(),
                None,
                "Partial{{{reason:?}}} holding {n} candidates must not report a total"
            );
            // C4 permits reporting what was found — that stays available.
            assert_eq!(set.solutions().len(), n);
        }
    }
}

/// C3 — an empty `Refuted` and an empty `Partial` must be distinguishable.
/// One is a proof about the model; the other is a report about the search.
#[test]
fn empty_refuted_and_empty_partial_are_distinguishable() {
    let refuted = refuted_set();
    let partial = SolutionSet::partial(Vec::new(), PartialReason::BoxBudgetExhausted);

    // Both found nothing...
    assert!(refuted.solutions().is_empty());
    assert!(partial.solutions().is_empty());

    // ...but only one of them proved anything.
    assert_eq!(
        refuted.proven_count(),
        Some(0),
        "Refuted proves the set is empty"
    );
    assert_eq!(
        partial.proven_count(),
        None,
        "Partial proves nothing about the total"
    );
    assert_ne!(refuted.completeness(), partial.completeness());

    // Neither is a uniqueness claim.
    assert!(!refuted.unique());
    assert!(!partial.unique());
}

// ── C2: ProvenOptimal requires Exhaustive (§3.2) ─────────────────────────────

/// C2 over the full 9-value variant set, so a later variant addition cannot
/// quietly widen the set of verdicts that permit a `ProvenOptimal` claim.
#[test]
fn permits_proven_optimal_only_for_exhaustive() {
    assert!(Completeness::Exhaustive.permits_proven_optimal());

    assert!(
        !Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0)
        }
        .permits_proven_optimal(),
        "a refutation has no optimum to prove"
    );

    for reason in all_partial_reasons() {
        assert!(
            !Completeness::Partial {
                reason: reason.clone()
            }
            .permits_proven_optimal(),
            "Partial{{{reason:?}}} must not permit ProvenOptimal"
        );
    }
}

// ── C3 rendering: a refutation can never read as a partial result ────────────

/// Follows the `best_found_reason_variants_describe` precedent in
/// `tests/ranked_solve_result.rs`: assert non-emptiness and pairwise distinctness
/// ONLY, never exact wording — pinning wording relocates the rewording-fragility
/// that `describe()` exists to remove.
#[test]
fn partial_reason_describe_is_non_empty_and_pairwise_distinct() {
    let reasons = all_partial_reasons();
    let described: Vec<String> = reasons.iter().map(|r| r.describe()).collect();

    for (reason, text) in reasons.iter().zip(&described) {
        assert!(!text.is_empty(), "{reason:?}.describe() must be non-empty");
    }

    for (i, left) in described.iter().enumerate() {
        for (j, right) in described.iter().enumerate() {
            if i != j {
                assert_ne!(
                    left, right,
                    "{:?} and {:?} must describe() differently",
                    reasons[i], reasons[j]
                );
            }
        }
    }
}

/// C3 at the rendering layer: `Refuted` must never render as `Exhaustive` or as
/// any `Partial`. A proof and a report about the search are different claims and
/// a diagnostic built on `describe()` must not be able to confuse them.
#[test]
fn refuted_never_describes_like_exhaustive_or_partial() {
    let refuted = Completeness::Refuted {
        narrowing: ConstraintNodeId::new("Bracket", 0),
    }
    .describe();
    let exhaustive = Completeness::Exhaustive.describe();

    assert!(!refuted.is_empty());
    assert!(!exhaustive.is_empty());
    assert_ne!(
        refuted, exhaustive,
        "a refutation is not an exhaustive enumeration"
    );

    for reason in all_partial_reasons() {
        let partial = Completeness::Partial {
            reason: reason.clone(),
        }
        .describe();
        assert!(
            !partial.is_empty(),
            "Partial{{{reason:?}}}.describe() must be non-empty"
        );
        assert_ne!(
            refuted, partial,
            "Refuted must not describe like Partial{{{reason:?}}} — that collapse is C3"
        );
        assert_ne!(
            exhaustive, partial,
            "Exhaustive must not describe like Partial{{{reason:?}}}"
        );
    }
}

/// INV-SF-3: a decline must be attributable, so the payload-carrying reasons
/// interpolate their payload into `describe()` — otherwise two different
/// declines render as one indistinguishable string and the diagnostic cannot
/// name what actually stopped the producer.
#[test]
fn payload_carrying_reasons_interpolate_their_payload() {
    assert_ne!(
        PartialReason::DimensionAboveEnvelope { dims: 7 }.describe(),
        PartialReason::DimensionAboveEnvelope { dims: 8 }.describe(),
        "dims must be attributable in the rendered decline"
    );

    assert_ne!(
        PartialReason::DomainUnbounded {
            param: ValueCellId::new("Part", "x")
        }
        .describe(),
        PartialReason::DomainUnbounded {
            param: ValueCellId::new("Part", "y")
        }
        .describe(),
        "the unbounded param must be attributable"
    );

    assert_ne!(
        PartialReason::NotIntervalRepresentable {
            constraint: ConstraintNodeId::new("Part", 0)
        }
        .describe(),
        PartialReason::NotIntervalRepresentable {
            constraint: ConstraintNodeId::new("Part", 1)
        }
        .describe(),
        "the non-representable constraint must be attributable"
    );

    // The same must hold once wrapped in Completeness::Partial, since that is
    // what a producer actually returns.
    assert_ne!(
        Completeness::Partial {
            reason: PartialReason::DimensionAboveEnvelope { dims: 7 }
        }
        .describe(),
        Completeness::Partial {
            reason: PartialReason::DimensionAboveEnvelope { dims: 8 }
        }
        .describe()
    );
}

/// `Refuted` names the narrowing constraint, so a refutation diagnostic can point
/// at the constraint that emptied the domain rather than at the model in general.
#[test]
fn refuted_describe_names_the_narrowing_constraint() {
    assert_ne!(
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0)
        }
        .describe(),
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 1)
        }
        .describe(),
    );
    assert_ne!(
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0)
        }
        .describe(),
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Flange", 0)
        }
        .describe(),
    );
}
