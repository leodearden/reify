//! Integration tests for RankedSolveResult carrier types (PRD ranked-solve-result §3.1).
//!
//! Written TDD: each test/impl step pair adds a new type, going RED→GREEN.
//! Step 1 (RED): OptimalityStatus variants, reason round-trip, Debug, Clone.
//! Step 3 (RED): RankedCandidate construction, field access, Debug, Clone.
//! Step 5 (RED): RankedSolveResult construction, destructure, Debug, Clone.

use reify_ir::OptimalityStatus;
use reify_ir::{RankedCandidate, Value};
use reify_core::identity::ValueCellId;
use std::collections::HashMap;

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

// ── RankedCandidate ──────────────────────────────────────────────────────────

#[test]
fn ranked_candidate_with_objective_score() {
    let mut values: HashMap<ValueCellId, Value> = HashMap::new();
    values.insert(ValueCellId::new("Part", "x"), Value::length(0.05));

    let candidate = RankedCandidate {
        values,
        objective_score: Some(1.0),
        unique: true,
    };

    assert!(candidate.values.contains_key(&ValueCellId::new("Part", "x")));
    assert_eq!(candidate.objective_score, Some(1.0));
    assert!(candidate.unique);
}

#[test]
fn ranked_candidate_feasibility_only() {
    let candidate = RankedCandidate {
        values: HashMap::new(),
        objective_score: None,
        unique: false,
    };

    assert!(candidate.objective_score.is_none());
    assert!(!candidate.unique);
}

#[test]
fn ranked_candidate_debug_and_clone_smoke() {
    let mut values: HashMap<ValueCellId, Value> = HashMap::new();
    values.insert(ValueCellId::new("Part", "y"), Value::length(0.1));

    let candidate = RankedCandidate { values, objective_score: Some(2.0), unique: false };
    let cloned = candidate.clone();
    let d1 = format!("{:?}", candidate);
    let d2 = format!("{:?}", cloned);
    assert!(d1.contains("RankedCandidate"));
    assert_eq!(d1, d2);
}
