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
