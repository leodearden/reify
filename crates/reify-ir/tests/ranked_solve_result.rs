//! Integration tests for RankedSolveResult carrier types (PRD ranked-solve-result §3.1).
//!
//! Written TDD: each test/impl step pair adds a new type, going RED→GREEN.
//! Step 1 (RED): OptimalityStatus variants, reason round-trip, Debug, Clone.
//! Step 3 (RED): RankedCandidate construction, field access, Debug, Clone.
//! Step 5 (RED): RankedSolveResult construction, destructure, Debug, Clone.

use reify_ir::OptimalityStatus;

// ── OptimalityStatus ─────────────────────────────────────────────────────────

#[test]
fn optimality_status_variants_construct() {
    let _proven = OptimalityStatus::ProvenOptimal;
    let _best = OptimalityStatus::BestFound { reason: "iteration limit reached".into() };
    let _feasibility = OptimalityStatus::FeasibilityOnly;
}

#[test]
fn optimality_status_best_found_reason_round_trips() {
    let status = OptimalityStatus::BestFound { reason: "iteration limit reached".into() };
    match status {
        OptimalityStatus::BestFound { reason } => {
            assert_eq!(reason, "iteration limit reached");
        }
        _ => panic!("expected BestFound"),
    }
}

#[test]
fn optimality_status_debug_smoke() {
    assert!(format!("{:?}", OptimalityStatus::ProvenOptimal).contains("ProvenOptimal"));
    assert!(
        format!("{:?}", OptimalityStatus::BestFound { reason: "x".into() }).contains("BestFound")
    );
    assert!(format!("{:?}", OptimalityStatus::FeasibilityOnly).contains("FeasibilityOnly"));
}

#[test]
fn optimality_status_clone_smoke() {
    let status = OptimalityStatus::BestFound { reason: "iter limit".into() };
    let cloned = status.clone();
    assert_eq!(format!("{:?}", status), format!("{:?}", cloned));
}
