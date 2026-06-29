//! Integration tests for RankedSolveResult carrier types (PRD ranked-solve-result §3.1).
//!
//! Written TDD: each test/impl step pair adds a new type, going RED→GREEN.
//! Step 1 (RED): OptimalityStatus variants, reason round-trip, Debug, Clone.
//! Step 3 (RED): RankedCandidate construction, field access, Debug, Clone.
//! Step 5 (RED): RankedSolveResult construction, destructure, Debug, Clone.

use reify_ir::OptimalityStatus;
use reify_ir::{BestFoundReason, RankedCandidate, RankedSolveResult, Value};
use reify_core::diagnostics::Diagnostic;
use reify_core::identity::ValueCellId;
use std::collections::HashMap;

// ── BestFoundReason enum (S2, task #4871) ────────────────────────────────────

/// [S2] BestFoundReason enum: variants construct, are PartialEq, and describe()
/// returns the exact current reason strings.
///
/// RED until step-3 introduces the enum in ranked.rs and re-exports it from lib.rs.
#[test]
fn best_found_reason_variants_describe() {
    // All three variants must be constructible and are Copy + PartialEq.
    assert_eq!(BestFoundReason::IterationLimit, BestFoundReason::IterationLimit);
    assert_eq!(BestFoundReason::ConvergedWithinBudget, BestFoundReason::ConvergedWithinBudget);
    assert_eq!(BestFoundReason::Unreported, BestFoundReason::Unreported);

    // Each variant maps to a distinct describe() string.
    let il_desc = BestFoundReason::IterationLimit.describe();
    assert!(
        il_desc.contains("iteration limit"),
        "IterationLimit.describe() must contain \"iteration limit\", got: {il_desc:?}"
    );

    let cb_desc = BestFoundReason::ConvergedWithinBudget.describe();
    assert!(
        cb_desc.contains("iteration budget"),
        "ConvergedWithinBudget.describe() must contain \"iteration budget\", got: {cb_desc:?}"
    );
    assert!(
        !cb_desc.contains("iteration limit"),
        "ConvergedWithinBudget.describe() must NOT contain \"iteration limit\", got: {cb_desc:?}"
    );

    let ur_desc = BestFoundReason::Unreported.describe();
    assert!(
        ur_desc.contains("does not report"),
        "Unreported.describe() must contain \"does not report\", got: {ur_desc:?}"
    );
    assert!(
        !ur_desc.contains("iteration limit"),
        "Unreported.describe() must NOT contain \"iteration limit\", got: {ur_desc:?}"
    );

    // OptimalityStatus::BestFound accepts BestFoundReason in the reason field.
    let status = OptimalityStatus::BestFound { reason: BestFoundReason::IterationLimit };
    assert!(matches!(status, OptimalityStatus::BestFound { reason: BestFoundReason::IterationLimit }));
}

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

// ── RankedSolveResult ────────────────────────────────────────────────────────

fn make_candidate() -> RankedCandidate {
    let mut values = HashMap::new();
    values.insert(ValueCellId::new("Part", "x"), Value::length(0.05));
    RankedCandidate { values, objective_score: Some(0.5), unique: false }
}

#[test]
fn ranked_solve_result_ranked_variant() {
    let result = RankedSolveResult::Ranked {
        candidates: vec![make_candidate()],
        optimality: OptimalityStatus::BestFound { reason: "iteration limit reached".into() },
    };

    match result {
        RankedSolveResult::Ranked { candidates, optimality } => {
            assert_eq!(candidates.len(), 1);
            assert!(matches!(optimality, OptimalityStatus::BestFound { .. }));
        }
        _ => panic!("expected Ranked"),
    }
}

#[test]
fn ranked_solve_result_infeasible_variant() {
    let result = RankedSolveResult::Infeasible {
        diagnostics: vec![Diagnostic::warning("no feasible point")],
    };

    match result {
        RankedSolveResult::Infeasible { diagnostics } => {
            assert_eq!(diagnostics.len(), 1);
        }
        _ => panic!("expected Infeasible"),
    }
}

#[test]
fn ranked_solve_result_no_progress_variant() {
    let result = RankedSolveResult::NoProgress {
        reason: "iteration limit, no feasible point".into(),
    };

    match result {
        RankedSolveResult::NoProgress { reason } => {
            assert_eq!(reason, "iteration limit, no feasible point");
        }
        _ => panic!("expected NoProgress"),
    }
}

#[test]
fn ranked_solve_result_debug_and_clone_smoke() {
    let result = RankedSolveResult::Ranked {
        candidates: vec![make_candidate()],
        optimality: OptimalityStatus::ProvenOptimal,
    };
    let cloned = result.clone();
    let d1 = format!("{:?}", result);
    let d2 = format!("{:?}", cloned);
    assert!(d1.contains("Ranked"));
    assert_eq!(d1, d2);
}
