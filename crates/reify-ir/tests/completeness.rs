//! Integration tests for the `Completeness` / `SolutionSet` carrier
//! (PRD `docs/prds/v0_6/solution-set-completeness.md` §3.1, invariants C1–C4).
//!
//! Written TDD: each test/impl step pair adds a new capability, going RED→GREEN.
//! Step 1 (RED): `Completeness` / `PartialReason` variant shape + payload round-trip.
//!
//! # Rail (plan pre-1)
//!
//! This task must NOT touch the optimality axis. No test in this file may pin a
//! [`reify_ir::BestFoundReason`] variant by name — assertions are on the
//! completeness axis only.

use reify_core::identity::{ConstraintNodeId, ValueCellId};
use reify_ir::{Completeness, PartialReason, RankedCandidate, SolutionSet, Value};
use std::collections::HashMap;

// ── Completeness variant shape (§3.1) ────────────────────────────────────────

#[test]
fn completeness_variants_construct() {
    let _exhaustive = Completeness::Exhaustive;
    let _partial = Completeness::Partial {
        reason: PartialReason::BoxBudgetExhausted,
    };
    let _refuted = Completeness::Refuted {
        narrowing: ConstraintNodeId::new("Bracket", 0),
    };
}

#[test]
fn completeness_refuted_payload_round_trips() {
    let narrowing = ConstraintNodeId::new("Bracket", 3);
    let refuted = Completeness::Refuted {
        narrowing: narrowing.clone(),
    };

    match refuted {
        Completeness::Refuted { narrowing: got } => assert_eq!(got, narrowing),
        other => panic!("expected Refuted, got {other:?}"),
    }
}

#[test]
fn completeness_partial_payload_round_trips() {
    let partial = Completeness::Partial {
        reason: PartialReason::ProbeOnly,
    };

    match partial {
        Completeness::Partial { reason } => assert_eq!(reason, PartialReason::ProbeOnly),
        other => panic!("expected Partial, got {other:?}"),
    }
}

// ── PartialReason variant shape (§3.1) ───────────────────────────────────────

/// Every `PartialReason` in §3.1 constructs. Kept as one exhaustive list so a
/// variant added later without a test is visible in the diff.
#[test]
fn partial_reason_variants_construct() {
    let _budget = PartialReason::BoxBudgetExhausted;
    let _envelope = PartialReason::DimensionAboveEnvelope { dims: 4 };
    let _unbounded = PartialReason::DomainUnbounded {
        param: ValueCellId::new("Part", "x"),
    };
    let _interval = PartialReason::NotIntervalRepresentable {
        constraint: ConstraintNodeId::new("Part", 1),
    };
    let _inner = PartialReason::InnerSolveUnproven;
    let _probe = PartialReason::ProbeOnly;
    let _not_attempted = PartialReason::NotAttempted;
}

#[test]
fn partial_reason_payloads_round_trip() {
    match (PartialReason::DimensionAboveEnvelope { dims: 7 }) {
        PartialReason::DimensionAboveEnvelope { dims } => assert_eq!(dims, 7),
        other => panic!("expected DimensionAboveEnvelope, got {other:?}"),
    }

    let param = ValueCellId::new("Frame", "width");
    match (PartialReason::DomainUnbounded {
        param: param.clone(),
    }) {
        PartialReason::DomainUnbounded { param: got } => assert_eq!(got, param),
        other => panic!("expected DomainUnbounded, got {other:?}"),
    }

    let constraint = ConstraintNodeId::new("Frame", 2);
    match (PartialReason::NotIntervalRepresentable {
        constraint: constraint.clone(),
    }) {
        PartialReason::NotIntervalRepresentable { constraint: got } => {
            assert_eq!(got, constraint)
        }
        other => panic!("expected NotIntervalRepresentable, got {other:?}"),
    }
}

// ── Derives: Debug / Clone / PartialEq / Eq ──────────────────────────────────

#[test]
fn completeness_debug_is_non_empty_and_clone_round_trips() {
    let values = [
        Completeness::Exhaustive,
        Completeness::Partial {
            reason: PartialReason::DimensionAboveEnvelope { dims: 3 },
        },
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0),
        },
    ];

    for value in &values {
        let rendered = format!("{value:?}");
        assert!(
            !rendered.is_empty(),
            "Debug must be non-empty for {value:?}"
        );

        let cloned = value.clone();
        assert_eq!(&cloned, value, "Clone must round-trip equal");
        assert_eq!(
            format!("{cloned:?}"),
            rendered,
            "Clone must Debug identically"
        );
    }
}

#[test]
fn partial_reason_debug_is_non_empty_and_clone_round_trips() {
    for reason in all_partial_reasons() {
        let rendered = format!("{reason:?}");
        assert!(
            !rendered.is_empty(),
            "Debug must be non-empty for {reason:?}"
        );
        assert_eq!(reason.clone(), reason, "Clone must round-trip equal");
    }
}

#[test]
fn completeness_distinct_variants_compare_unequal() {
    let exhaustive = Completeness::Exhaustive;
    let partial = Completeness::Partial {
        reason: PartialReason::NotAttempted,
    };
    let refuted = Completeness::Refuted {
        narrowing: ConstraintNodeId::new("Bracket", 0),
    };

    assert_ne!(exhaustive, partial);
    assert_ne!(exhaustive, refuted);
    assert_ne!(partial, refuted);
    assert_eq!(exhaustive, Completeness::Exhaustive);
}

#[test]
fn completeness_refuted_discriminates_on_narrowing_id() {
    let a = Completeness::Refuted {
        narrowing: ConstraintNodeId::new("Bracket", 0),
    };
    let b = Completeness::Refuted {
        narrowing: ConstraintNodeId::new("Bracket", 1),
    };
    let c = Completeness::Refuted {
        narrowing: ConstraintNodeId::new("Flange", 0),
    };

    assert_ne!(a, b, "different constraint index must compare unequal");
    assert_ne!(a, c, "different constraint entity must compare unequal");
    assert_eq!(
        a,
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0)
        }
    );
}

#[test]
fn partial_reason_distinct_variants_compare_unequal() {
    let reasons = all_partial_reasons();
    for (i, left) in reasons.iter().enumerate() {
        for (j, right) in reasons.iter().enumerate() {
            if i == j {
                assert_eq!(left, right);
            } else {
                assert_ne!(left, right, "{left:?} and {right:?} must compare unequal");
            }
        }
    }

    // Payload-carrying reasons discriminate on their payload too.
    assert_ne!(
        PartialReason::DimensionAboveEnvelope { dims: 3 },
        PartialReason::DimensionAboveEnvelope { dims: 4 }
    );
    assert_ne!(
        PartialReason::DomainUnbounded {
            param: ValueCellId::new("Part", "x")
        },
        PartialReason::DomainUnbounded {
            param: ValueCellId::new("Part", "y")
        }
    );
    assert_ne!(
        PartialReason::NotIntervalRepresentable {
            constraint: ConstraintNodeId::new("Part", 0)
        },
        PartialReason::NotIntervalRepresentable {
            constraint: ConstraintNodeId::new("Part", 1)
        }
    );
}

/// `Eq` (not just `PartialEq`) is required: the composition law at ι #6715 → #6903 keys
/// on completeness verdicts, so they must be usable as map keys / in `assert_eq!`
/// without a partial-equivalence caveat.
#[test]
fn completeness_is_eq() {
    fn assert_eq_bound<T: Eq>(_: &T) {}
    assert_eq_bound(&Completeness::Exhaustive);
    assert_eq_bound(&PartialReason::NotAttempted);
}

// ── Shared fixtures ──────────────────────────────────────────────────────────

/// The full §3.1 `PartialReason` set, with distinct payloads so equality tests
/// exercise the payload discriminators too.
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

fn set_of(n: usize, completeness: Completeness) -> SolutionSet {
    SolutionSet {
        solutions: (0..n).map(|i| make_candidate(i as f64 / 100.0)).collect(),
        completeness,
    }
}

/// (a) `SolutionSet::unique()` must agree with
/// `Completeness::derived_unique(solutions.len())` across the same grid as the
/// C1 truth table — it delegates, it does not re-derive.
#[test]
fn solution_set_unique_agrees_with_derived_unique() {
    let mut completenesses = vec![
        Completeness::Exhaustive,
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0),
        },
    ];
    completenesses.extend(
        all_partial_reasons()
            .into_iter()
            .map(|reason| Completeness::Partial { reason }),
    );

    for completeness in &completenesses {
        for n in [0usize, 1, 2, 5] {
            let set = set_of(n, completeness.clone());
            assert_eq!(
                set.unique(),
                completeness.derived_unique(n),
                "SolutionSet::unique must delegate to derived_unique for {completeness:?} with {n} solutions"
            );
        }
    }

    // The three named cells, spelled out.
    assert!(set_of(1, Completeness::Exhaustive).unique());
    assert!(
        !set_of(
            1,
            Completeness::Partial {
                reason: PartialReason::ProbeOnly
            }
        )
        .unique(),
        "one candidate + ProbeOnly is not a uniqueness claim"
    );
    assert!(
        !set_of(2, Completeness::Exhaustive).unique(),
        "two proven solutions is the opposite of unique"
    );
}

/// (b) C4 — a `Partial` verdict may never report a total.
#[test]
fn proven_count_is_none_for_every_partial_reason() {
    // Exhaustive: the count IS proven, and it is the set's own length.
    for n in [0usize, 1, 2, 5] {
        assert_eq!(set_of(n, Completeness::Exhaustive).proven_count(), Some(n));
    }

    // Refuted: proven zero.
    let refuted = set_of(
        0,
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0),
        },
    );
    assert_eq!(refuted.proven_count(), Some(0));

    // Partial: no total, for every reason and regardless of what was found.
    for reason in all_partial_reasons() {
        for n in [0usize, 1, 2] {
            let set = set_of(
                n,
                Completeness::Partial {
                    reason: reason.clone(),
                },
            );
            assert_eq!(
                set.proven_count(),
                None,
                "Partial{{{reason:?}}} holding {n} candidates must not report a total"
            );
            // C4 permits reporting what was found — that stays available.
            assert_eq!(set.solutions.len(), n);
        }
    }
}

/// (c) C3 — an empty `Refuted` and an empty `Partial` must be distinguishable.
/// One is a proof about the model; the other is a report about the search.
#[test]
fn empty_refuted_and_empty_partial_are_distinguishable() {
    let refuted_c = Completeness::Refuted {
        narrowing: ConstraintNodeId::new("Bracket", 0),
    };
    let partial_c = Completeness::Partial {
        reason: PartialReason::BoxBudgetExhausted,
    };

    let refuted = set_of(0, refuted_c.clone());
    let partial = set_of(0, partial_c.clone());

    // Both found nothing...
    assert_eq!(refuted.solutions.len(), 0);
    assert_eq!(partial.solutions.len(), 0);

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
    assert_ne!(refuted.proven_count(), partial.proven_count());
    assert_ne!(refuted_c, partial_c);

    // Neither is a uniqueness claim.
    assert!(!refuted.unique());
    assert!(!partial.unique());
}

/// (d) Debug / Clone smoke. `SolutionSet` is not `PartialEq` because
/// `RankedCandidate` is not, so the round-trip is asserted through `Debug`.
#[test]
fn solution_set_debug_and_clone_smoke() {
    let set = set_of(2, Completeness::Exhaustive);
    let cloned = set.clone();

    let d1 = format!("{set:?}");
    let d2 = format!("{cloned:?}");
    assert!(d1.contains("SolutionSet"));
    assert_eq!(d1, d2);
    assert_eq!(cloned.proven_count(), set.proven_count());
    assert_eq!(cloned.unique(), set.unique());
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

// ── SolutionSet well-formedness: a `Refuted` set holding candidates is loud ──

/// (e) The `Refuted` well-formedness rule stated on `SolutionSet::completeness`
/// is enforced, not merely advisory: `proven_count()` on a malformed `Refuted`
/// set that still holds candidates trips a `debug_assert!` instead of silently
/// reporting `Some(0)` beside a non-empty `solutions`.
///
/// `#[cfg(debug_assertions)]`-gated because `debug_assert!` compiles out in
/// release, where the answer stays `Some(0)` read from the verdict (deliberately
/// — see `proven_count`'s docs: returning `solutions.len()` there would re-render
/// a refutation as a count).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a well-formed Refuted SolutionSet carries no solutions")]
fn proven_count_on_malformed_refuted_trips_debug_assert() {
    let malformed = set_of(
        2,
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0),
        },
    );
    let _ = malformed.proven_count();
}

/// The well-formed `Refuted` set — the one every producer must build — is
/// unaffected by that guard: empty `solutions`, `proven_count() == Some(0)`.
/// Pinned separately so the guard cannot be "fixed" by making the good case panic.
#[test]
fn proven_count_on_well_formed_refuted_is_some_zero() {
    let refuted = set_of(
        0,
        Completeness::Refuted {
            narrowing: ConstraintNodeId::new("Bracket", 0),
        },
    );
    assert!(refuted.solutions.is_empty());
    assert_eq!(refuted.proven_count(), Some(0));
}
