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

// ─── step-11: Ambiguous message — lex-first explicit-substitution hint ──

/// The AMBIGUOUS message must surface the lex-first feasible candidate as
/// a suggested explicit substitution, not just list it among the
/// `candidates` structured field. This pins the human-readable surface so
/// a regression that emits an empty/generic message would fail.
#[test]
fn ambiguous_diagnostic_message_includes_lex_first_explicit_substitution_suggestion() {
    let feasibility = FeasibilityResult::Feasible {
        accepted: vec!["GraphiteSeal".to_string(), "ORingSeal".to_string()],
        rejected: vec![],
    };
    let mut diagnostics = Vec::new();
    let _ = select_candidate(
        feasibility,
        &["Seal".to_string()],
        false,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    let d = &diagnostics[0];
    assert!(
        d.message.contains("GraphiteSeal"),
        "AMBIGUOUS message must contain the lex-first candidate 'GraphiteSeal'; got: {}",
        d.message
    );
    assert!(
        d.message.contains("explicit")
            || d.message.contains("instead")
            || d.message.contains("suggested"),
        "AMBIGUOUS message must convey that 'GraphiteSeal' is a suggested explicit substitution; got: {}",
        d.message
    );
    // Bind the lex-first FQN to the suggestion clause as a single contract.
    // "GraphiteSeal" alone appears in the candidates list "GraphiteSeal,
    // ORingSeal" already, so a regression that swapped the suggestion to
    // 'ORingSeal' (the wrong lex-first) would still pass the substring
    // checks above. Pinning the bound substring catches that.
    assert!(
        d.message.contains("like 'GraphiteSeal' instead"),
        "AMBIGUOUS message must bind lex-first FQN to the suggestion clause (`like '<lex_first>' instead`); got: {}",
        d.message
    );
}

// ─── step-13: NonUnique message — names chosen lex-first candidate ──────

/// The NON_UNIQUE warning must surface the lex-first candidate that was
/// selected, not just list it among the structured `candidates` field.
/// This pins the human-readable surface so a regression that emits an
/// empty/generic warning would fail.
#[test]
fn non_unique_diagnostic_message_names_chosen_lex_first_candidate() {
    let feasibility = FeasibilityResult::Feasible {
        accepted: vec!["GraphiteSeal".to_string(), "ORingSeal".to_string()],
        rejected: vec![],
    };
    let mut diagnostics = Vec::new();
    let _ = select_candidate(
        feasibility,
        &["Seal".to_string()],
        true,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    let d = &diagnostics[0];
    assert!(
        d.message.contains("GraphiteSeal"),
        "NON_UNIQUE message must contain the chosen lex-first candidate 'GraphiteSeal'; got: {}",
        d.message
    );
    assert!(
        d.message.contains("auto(free)")
            || d.message.contains("non-unique")
            || d.message.contains("selected"),
        "NON_UNIQUE message must convey choice-under-non-uniqueness; got: {}",
        d.message
    );
    // Bind the chosen lex-first FQN to the "selected lexicographically-first"
    // clause as a single contract. "GraphiteSeal" alone already appears in
    // the candidates list "GraphiteSeal, ORingSeal", so a regression that
    // swapped the chosen candidate to 'ORingSeal' would still pass the
    // looser substring checks above. Pinning the bound substring catches
    // that.
    assert!(
        d.message.contains("selected lexicographically-first 'GraphiteSeal'"),
        "NON_UNIQUE message must bind chosen FQN to the `selected lexicographically-first '<lex_first>'` clause; got: {}",
        d.message
    );
}

// ─── step-15: composite-bound rendering parity across all three arms ─────

/// All three Phase C diagnostics must render composite bounds with
/// `bounds.join(" + ")`, mirroring Phase A's overflow diagnostic
/// (`enumerate_candidates`). This test pins the rendering for each of the
/// NO_CANDIDATE, AMBIGUOUS, and NON_UNIQUE arms by feeding
/// `bounds = ["Seal", "Cooled"]` and asserting `"Seal + Cooled"` appears
/// in the message. Prevents a regression that hard-codes `bounds[0]`.
#[test]
fn composite_bound_diagnostics_join_bounds_with_plus_separator() {
    let bounds = vec!["Seal".to_string(), "Cooled".to_string()];

    // (a) Empty arm — NO_CANDIDATE.
    {
        let cnid = ConstraintNodeId::new("Bearing", 0);
        let feasibility = FeasibilityResult::Empty {
            rejected: vec![RejectedCandidate {
                name: "GraphiteSeal".to_string(),
                violated_constraints: vec![cnid],
            }],
        };
        let mut diagnostics = Vec::new();
        let _ = select_candidate(
            feasibility,
            &bounds,
            false,
            SourceSpan::empty(0),
            &mut diagnostics,
        );
        assert!(
            diagnostics[0].message.contains("Seal + Cooled"),
            "NO_CANDIDATE message must render composite bounds with ' + ' separator; got: {}",
            diagnostics[0].message
        );
    }

    // (b) Strict ≥2 — AMBIGUOUS.
    {
        let feasibility = FeasibilityResult::Feasible {
            accepted: vec!["GraphiteSeal".to_string(), "ORingSeal".to_string()],
            rejected: vec![],
        };
        let mut diagnostics = Vec::new();
        let _ = select_candidate(
            feasibility,
            &bounds,
            false,
            SourceSpan::empty(0),
            &mut diagnostics,
        );
        assert!(
            diagnostics[0].message.contains("Seal + Cooled"),
            "AMBIGUOUS message must render composite bounds with ' + ' separator; got: {}",
            diagnostics[0].message
        );
    }

    // (c) Free ≥2 — NON_UNIQUE.
    {
        let feasibility = FeasibilityResult::Feasible {
            accepted: vec!["GraphiteSeal".to_string(), "ORingSeal".to_string()],
            rejected: vec![],
        };
        let mut diagnostics = Vec::new();
        let _ = select_candidate(
            feasibility,
            &bounds,
            true,
            SourceSpan::empty(0),
            &mut diagnostics,
        );
        assert!(
            diagnostics[0].message.contains("Seal + Cooled"),
            "NON_UNIQUE message must render composite bounds with ' + ' separator; got: {}",
            diagnostics[0].message
        );
    }
}

// ─── step-9: NoCandidate diagnostic shape (full contract) ─────────────────

/// Pins the NO_CANDIDATE diagnostic's full contract: rejected FQNs land in
/// the structured `candidates` field (in input order, alphabetical because
/// Phase B preserves Phase A's input order), the label sits at the
/// supplied `use_site_span`, and the message text mentions the bound name.
#[test]
fn no_candidate_diagnostic_carries_rejected_fqns_in_candidates_field_and_label_at_use_site() {
    let cnid_a = ConstraintNodeId::new("Bearing", 0);
    let cnid_b = ConstraintNodeId::new("Bearing", 1);
    let use_site_span = SourceSpan::new(100, 110);
    let feasibility = FeasibilityResult::Empty {
        rejected: vec![
            RejectedCandidate {
                name: "GraphiteSeal".to_string(),
                violated_constraints: vec![cnid_a],
            },
            RejectedCandidate {
                name: "ORingSeal".to_string(),
                violated_constraints: vec![cnid_b],
            },
        ],
    };

    let mut diagnostics = Vec::new();
    let result = select_candidate(
        feasibility,
        &["Seal".to_string()],
        false,
        use_site_span,
        &mut diagnostics,
    );

    assert_eq!(result, SelectionResult::NoCandidate);
    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert_eq!(
        d.candidates,
        vec!["GraphiteSeal".to_string(), "ORingSeal".to_string()],
        "NO_CANDIDATE diagnostic.candidates must contain rejected FQNs in input order"
    );
    assert!(
        !d.labels.is_empty(),
        "NO_CANDIDATE diagnostic must carry a label at the use-site span"
    );
    assert_eq!(
        d.labels[0].span, use_site_span,
        "NO_CANDIDATE label span must equal supplied use_site_span"
    );
    assert!(
        d.message.contains("Seal"),
        "NO_CANDIDATE message must reference the bound name 'Seal'; got: {}",
        d.message
    );
    // Per-rejection prose contract: the message must list each rejected
    // candidate FQN paired with "rejected by constraint" prose so a
    // regression that drops `rejection_summary` from the format string
    // would fail rather than silently emit a truncated message that still
    // mentions the bound.
    assert!(
        d.message.contains("GraphiteSeal") && d.message.contains("ORingSeal"),
        "NO_CANDIDATE message must name every rejected candidate FQN; got: {}",
        d.message
    );
    assert!(
        d.message.contains("rejected by constraint"),
        "NO_CANDIDATE message must include the per-rejection 'rejected by constraint' prose; got: {}",
        d.message
    );
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

// ─── step-17: single-feasible is independent of `free` flag ──────────────

/// Pins the design decision that a single-feasible candidate is selected
/// directly without consulting the `free` flag and without emitting any
/// diagnostic. PRD §"Phase C" specifies "1 feasible → use it." with no
/// condition on strictness; emitting `W_AUTO_TYPE_PARAM_NON_UNIQUE` under
/// `auto(free)` for a 1-candidate input would be both noise and a contract
/// violation. This test exercises BOTH `free=false` and `free=true` with
/// the same single-feasible input to prevent a future refactor from
/// accidentally surfacing a warning on the 1-candidate path.
#[test]
fn single_feasible_candidate_returns_selected_regardless_of_free_flag() {
    // Strict (`free=false`).
    {
        let feasibility = FeasibilityResult::Feasible {
            accepted: vec!["X".to_string()],
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
            SelectionResult::Selected("X".to_string()),
            "single feasible under free=false must return Selected(name)"
        );
        assert!(
            diagnostics.is_empty(),
            "single feasible under free=false must emit no diagnostic, got: {:?}",
            diagnostics
        );
    }

    // Free (`free=true`).
    {
        let feasibility = FeasibilityResult::Feasible {
            accepted: vec!["X".to_string()],
            rejected: vec![],
        };
        let mut diagnostics = Vec::new();
        let result = select_candidate(
            feasibility,
            &["Seal".to_string()],
            true,
            SourceSpan::empty(0),
            &mut diagnostics,
        );
        assert_eq!(
            result,
            SelectionResult::Selected("X".to_string()),
            "single feasible under free=true must return Selected(name)"
        );
        assert!(
            diagnostics.is_empty(),
            "single feasible under free=true must emit no diagnostic (NON_UNIQUE warning would be a contract violation), got: {:?}",
            diagnostics
        );
    }
}

// ─── step-19: Empty.rejected must be non-empty (debug_assert!) ───────────

/// Passing `FeasibilityResult::Empty { rejected: vec![] }` to
/// `select_candidate` is a caller bug per the invariant established in
/// Phase A. Phase A's empty-pool path emits `E_AUTO_TYPE_PARAM_POOL_OVERFLOW`
/// before any feasibility check, so a normal flow can never reach
/// `select_candidate` with `rejected = vec![]`; every `FeasibilityResult::Empty`
/// produced in normal flow carries ≥1 `RejectedCandidate`. The `debug_assert!`
/// exists to catch hand-constructed misuse (e.g., wiring a bare `Empty { rejected:
/// vec![] }` directly to Phase C) and prevent the malformed diagnostic message
/// that would otherwise result — one ending in `for bound 'Seal': ` (trailing
/// colon-space, empty rejection_summary).
///
/// The `#[cfg(debug_assertions)]` gate skips this test in release builds where
/// `debug_assert!` is a no-op, avoiding spurious test failures in optimized
/// profiles. In debug builds (the default for `cargo test`), the assert fires
/// and the `#[should_panic(expected = ...)]` attribute pins the exact message
/// substring so that any future weakening or removal of the assert fails loudly.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "FeasibilityResult::Empty must carry at least one rejected candidate")]
fn select_panics_on_empty_rejected_in_feasibility_empty() {
    let feasibility = FeasibilityResult::Empty { rejected: vec![] };
    let _ = select_candidate(
        feasibility,
        &["Seal".to_string()],
        false,
        SourceSpan::empty(0),
        &mut Vec::new(),
    );
}
