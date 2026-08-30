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
use reify_ir::{Completeness, PartialReason};

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
        assert!(!rendered.is_empty(), "Debug must be non-empty for {value:?}");

        let cloned = value.clone();
        assert_eq!(&cloned, value, "Clone must round-trip equal");
        assert_eq!(format!("{cloned:?}"), rendered, "Clone must Debug identically");
    }
}

#[test]
fn partial_reason_debug_is_non_empty_and_clone_round_trips() {
    for reason in all_partial_reasons() {
        let rendered = format!("{reason:?}");
        assert!(!rendered.is_empty(), "Debug must be non-empty for {reason:?}");
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

/// `Eq` (not just `PartialEq`) is required: the composition law at ι #6715 keys
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
        "Partial{ProbeOnly} + 1: found one, established nothing"
    );
    assert!(
        !Completeness::Partial {
            reason: PartialReason::NotAttempted
        }
        .derived_unique(1),
        "Partial{NotAttempted} + 1: no completeness reasoning was attempted"
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
            !Completeness::Partial { reason: reason.clone() }.is_exhaustive(),
            "Partial{{{reason:?}}} must not be exhaustive"
        );
    }
}
