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

// ─── step-3: single-feasible — Selected, no diagnostic ───────────────────

/// When `FeasibilityResult::Feasible` carries exactly one accepted candidate,
/// `select_candidate` returns [`SelectionResult::Selected(name)`] and emits
/// no diagnostic. There is nothing to disambiguate when only one candidate
/// is feasible.
#[test]
fn select_returns_selected_for_single_feasible_candidate_with_no_diagnostic() {
    let feasibility = FeasibilityResult::Feasible {
        accepted: vec!["ORingSeal".to_string()],
        rejected: vec![],
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
        SelectionResult::Selected("ORingSeal".to_string()),
        "single feasible candidate must be Selected directly"
    );
    assert!(
        diagnostics.is_empty(),
        "single feasible candidate must emit no diagnostic, got: {:?}",
        diagnostics
    );
}

// ─── step-5: ≥2 feasible under strict — Ambiguous + Error ────────────────

/// When two or more candidates are feasible and `free=false`,
/// `select_candidate` returns [`SelectionResult::Ambiguous(...)`] carrying
/// every feasible FQN, and pushes one error diagnostic with the
/// `AutoTypeParamAmbiguous` code, the use-site-span label, and the
/// machine-readable candidates list.
#[test]
fn select_returns_ambiguous_for_two_strict_feasible_candidates() {
    let use_site_span = SourceSpan::new(100, 110);
    let feasibility = FeasibilityResult::Feasible {
        accepted: vec!["GraphiteSeal".to_string(), "ORingSeal".to_string()],
        rejected: vec![],
    };

    let mut diagnostics = Vec::new();
    let result = select_candidate(
        feasibility,
        &["Seal".to_string()],
        false,
        use_site_span,
        &mut diagnostics,
    );

    assert_eq!(
        result,
        SelectionResult::Ambiguous(vec![
            "GraphiteSeal".to_string(),
            "ORingSeal".to_string()
        ]),
        "≥2 feasible under strict must return Ambiguous with all feasible FQNs"
    );
    assert_eq!(diagnostics.len(), 1, "exactly one ambiguous diagnostic");
    let d = &diagnostics[0];
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.code, Some(DiagnosticCode::AutoTypeParamAmbiguous));
    assert_eq!(
        d.candidates,
        vec!["GraphiteSeal".to_string(), "ORingSeal".to_string()],
        "diagnostic.candidates must list every feasible FQN in input order"
    );
    assert!(
        !d.labels.is_empty(),
        "diagnostic must carry at least one label at the use-site span"
    );
    assert_eq!(d.labels[0].span, use_site_span, "label span = use-site span");
}

// ─── step-7: ≥2 feasible under free — Selected(lex_first) + Warning ──────

/// When two or more candidates are feasible and `free=true`,
/// `select_candidate` returns [`SelectionResult::Selected(lex_first)`] — the
/// lexicographically-first candidate (which is `accepted[0]` because Phase
/// B preserves Phase A's alphabetical input order) — and pushes one
/// **Warning** diagnostic with the `AutoTypeParamNonUnique` code. The
/// warning severity is the load-bearing assertion that distinguishes this
/// path from the strict-ambiguous error path.
#[test]
fn select_returns_lex_first_for_two_free_feasible_candidates_and_emits_warning() {
    let use_site_span = SourceSpan::new(100, 110);
    let feasibility = FeasibilityResult::Feasible {
        accepted: vec!["GraphiteSeal".to_string(), "ORingSeal".to_string()],
        rejected: vec![],
    };

    let mut diagnostics = Vec::new();
    let result = select_candidate(
        feasibility,
        &["Seal".to_string()],
        true,
        use_site_span,
        &mut diagnostics,
    );

    assert_eq!(
        result,
        SelectionResult::Selected("GraphiteSeal".to_string()),
        "≥2 feasible under auto(free) must Selected(lex_first)"
    );
    assert_eq!(diagnostics.len(), 1, "exactly one non-unique diagnostic");
    let d = &diagnostics[0];
    assert_eq!(
        d.severity,
        Severity::Warning,
        "auto(free) non-unique resolution must emit Warning, not Error"
    );
    assert_eq!(d.code, Some(DiagnosticCode::AutoTypeParamNonUnique));
    assert_eq!(
        d.candidates,
        vec!["GraphiteSeal".to_string(), "ORingSeal".to_string()],
        "diagnostic.candidates must list every feasible FQN in input order"
    );
    assert!(
        !d.labels.is_empty(),
        "diagnostic must carry at least one label at the use-site span"
    );
    assert_eq!(d.labels[0].span, use_site_span, "label span = use-site span");
}
