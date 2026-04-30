//! Phase C tests for `auto` type-parameter resolution selection logic.
//!
//! Targets `crates/reify-compiler/src/auto_type_param.rs`'s public
//! `select_candidate` function and its three-arm result enum
//! [`SelectionResult`]. The PRD that drives this work is
//! `docs/prds/auto-type-param-resolution.md` §"Phase C" and language
//! spec §3.9.
//!
//! Phase C is a pure dispatcher over Phase B's [`FeasibilityResult`]:
//! - 0 feasible candidates → `E_AUTO_TYPE_PARAM_NO_CANDIDATE` + `NoCandidate`
//! - 1 feasible candidate → `Selected(name)` (no diagnostic, free-flag-independent)
//! - ≥2 feasible & strict (`free=false`) → `E_AUTO_TYPE_PARAM_AMBIGUOUS` + `Ambiguous(...)`
//! - ≥2 feasible & free (`free=true`) → `W_AUTO_TYPE_PARAM_NON_UNIQUE` + `Selected(lex_first)`
//!
//! # Test approach
//!
//! Phase C consumes Phase B's `FeasibilityResult` directly. Tests construct
//! `FeasibilityResult` values by hand rather than running them through Phase B
//! (`MockConstraintChecker` / `filter_feasible_candidates`). This keeps each
//! test focused on the dispatch arm under examination and decouples Phase C
//! tests from any Phase B / constraint-checker behavior.

use reify_compiler::auto_type_param::*;
use reify_types::{ConstraintNodeId, DiagnosticCode, Severity, SourceSpan};

// ─── step-1: NoCandidate arm — Empty feasibility → error + NoCandidate ────

/// When `FeasibilityResult::Empty` is supplied, `select_candidate` returns
/// [`SelectionResult::NoCandidate`] and pushes one error diagnostic carrying
/// the `AutoTypeParamNoCandidate` code.
#[test]
fn select_returns_no_candidate_and_emits_error_when_feasibility_is_empty() {
    let cnid = ConstraintNodeId::new("Seal", 0);
    let feasibility = FeasibilityResult::Empty {
        rejected: vec![RejectedCandidate {
            name: "ORingSeal".to_string(),
            violated_constraints: vec![cnid],
        }],
    };

    let mut diagnostics = Vec::new();
    let result = select_candidate(
        feasibility,
        &["Seal".to_string()],
        false,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert_eq!(
        result,
        SelectionResult::NoCandidate,
        "Empty feasibility must return NoCandidate"
    );
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one no-candidate diagnostic expected, got {}",
        diagnostics.len()
    );
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(
        diagnostics[0].code,
        Some(DiagnosticCode::AutoTypeParamNoCandidate)
    );
}
