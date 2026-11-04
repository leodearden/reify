//! Unit behaviour for INV-SF-2's two pure exit-gate helpers, built from
//! synthetic [`reify_core::Diagnostic`] values — no OCCT, no CLI exec, so
//! these run in a stub-mode build.

use super::{check_gating_error, has_error_diagnostic};
use reify_core::{Diagnostic, DiagnosticCode};

/// The SOFT missing-trampoline message, verbatim as
/// `engine_compute.rs::soft_no_trampoline_diagnostic` builds it.
const TRAMPOLINE: &str =
    "@optimized target \"solver::elastic_static\": no registered compute trampoline \
     (falling back to body-inlining)";

/// The missing-trampoline diagnostic on a NON-empty compute registry: every
/// CLI driver's case, since `reify check`, `reify eval` and `reify build` all
/// register the production compute bundle.
fn trampoline_error() -> Diagnostic {
    Diagnostic::error(TRAMPOLINE).with_code(DiagnosticCode::NoRegisteredComputeTrampoline)
}

/// The same diagnostic on an EMPTY compute registry — the LSP's posture.
fn trampoline_warning() -> Diagnostic {
    Diagnostic::warning(TRAMPOLINE).with_code(DiagnosticCode::NoRegisteredComputeTrampoline)
}

/// A code-less geometry Error that the `is unresolved (Undef)` entry excuses.
fn undef_argument_error() -> Diagnostic {
    Diagnostic::error(
        "failed to compile geometry operation: argument 'depth' for box is unresolved (Undef)",
    )
}

/// The code-less sibling emission that SHARES the `@optimized target `
/// prefix and must keep gating.
const TRAMPOLINE_CANCELLED: &str =
    "@optimized target \"solver::elastic_static\": compute trampoline was cancelled";

// -------------------------------------------------------------------
// has_error_diagnostic — the shared, allowlist-BLIND severity predicate
// -------------------------------------------------------------------

/// `has_error_diagnostic` is the ONE definition of "this set carries an
/// Error", shared with `cmd_eval` and `cmd_build`.  It must know nothing
/// about `CHECK_ERROR_EXIT_ALLOWLIST`: the allowlist is a `check`-only
/// migration ratchet, and eval/build have gated on `Severity::Error` since
/// #4458 — including on the trampoline Error, which `build_is_success`'
/// own doc names as its motivating example.
#[test]
fn has_error_diagnostic_is_pure_severity_and_allowlist_blind() {
    assert!(!has_error_diagnostic(&[]), "empty set carries no Error");
    assert!(
        !has_error_diagnostic(&[Diagnostic::warning("W_DFM_OVERHANG: 62° exceeds 45° limit")]),
        "a Warning is not an Error"
    );
    assert!(
        has_error_diagnostic(&[Diagnostic::error("failed to compile geometry operation")]),
        "any Severity::Error makes this true"
    );
    assert!(
        has_error_diagnostic(&[trampoline_error()]),
        "the shared predicate must report the trampoline Error, or `reify \
         build` / `reify eval` would silently stop gating on it"
    );
}

/// Both real shapes of the missing-trampoline diagnostic, carrying the
/// same `NoRegisteredComputeTrampoline` code: a Warning on an empty compute
/// registry, an Error on a non-empty one.
#[test]
fn missing_trampoline_warning_never_gates_and_its_error_gates() {
    assert!(
        check_gating_error(&[trampoline_warning()]).is_none(),
        "an empty-registry Warning never gates"
    );
    assert!(
        check_gating_error(&[trampoline_error()]).is_some(),
        "no allowlist entry excuses the coded Error, so it gates check"
    );
    assert!(
        has_error_diagnostic(&[trampoline_error()]),
        "eval/build keep gating on the coded Error"
    );
}

// -------------------------------------------------------------------
// check_gating_error — severity AND no allowlist match
// -------------------------------------------------------------------

#[test]
fn nothing_gates_on_an_empty_set() {
    assert!(check_gating_error(&[]).is_none());
}

/// C1 (graceful degradation): a Warning never invents a failure, whatever
/// it says.  `W_DFM_*` is the family that most looks like it should.
#[test]
fn warnings_never_gate() {
    assert!(
        check_gating_error(&[
            Diagnostic::warning("W_DFM_OVERHANG: 62° exceeds 45° limit"),
            Diagnostic::warning("W_DFM_DRAFT: 0.5° below the 1° minimum"),
        ])
        .is_none(),
        "Warning-severity DFM findings are non-fatal by design"
    );
}

/// EXECUTABLE PROOF of PRD §3 Leg B item 3 — "behavior stays byte-identical
/// for those classes".
///
/// Both bolt-ons this task removes escalated a strict SUBSET of what the
/// general `Severity::Error` gate catches:
///
/// - `GdtIllegalModifier` has exactly one emission site
///   (`engine_constraints::illegal_modifier_error`) and it is
///   unconditionally `Diagnostic::error`, so every diagnostic the deleted
///   code-scoped escalation could see is Error-severity;
/// - the deleted `E_DFM_` message-filter predicate matched `severity ==
///   Error && message contains "E_DFM_"`, which is the general predicate
///   AND a message filter.
///
/// Deleting them therefore cannot lose a gate, only widen one.
#[test]
fn the_deleted_bolt_ons_are_subsumed() {
    let gdt = Diagnostic::error(
        "`flatness` is an RFS-only characteristic; the `M` material-condition \
         modifier is illegal on it",
    )
    .with_code(DiagnosticCode::GdtIllegalModifier);
    assert!(
        check_gating_error(std::slice::from_ref(&gdt)).is_some(),
        "GdtIllegalModifier still exits non-zero, now via the general gate"
    );

    for msg in [
        "E_DFM_OVERHANG: face dips past the overhang limit",
        "E_DFM_UNDERCUT: re-entrant wall — part cannot release",
        "E_DFM_DRAFT: 0.2° below the 1° minimum",
    ] {
        assert!(
            check_gating_error(&[Diagnostic::error(msg)]).is_some(),
            "DFM Error {msg:?} still exits non-zero, now via the general gate"
        );
    }
}

/// Each seeded allowlist entry, exercised against a message taken verbatim
/// from the MEASURED corpus sweep that seeded it.
#[test]
fn seeded_allowlist_entries_excuse_their_families() {
    assert!(
        check_gating_error(&[undef_argument_error()]).is_none(),
        "the `is unresolved (Undef)` entry (#5404) excuses the code-less \
         Undef-argument geometry Error"
    );
    assert!(
        check_gating_error(&[Diagnostic::error(
            "all geometry operations failed; no geometry output produced"
        )])
        .is_none(),
        "the `all geometry operations failed` entry (#5404): check writes no \
         geometry, so this is not a fact about the design"
    );
    assert!(
        check_gating_error(&[Diagnostic::error(
            "constraint BoltFlange#constraint[1] violated: clearance 0.4mm \
             below minimum 0.5mm"
        )
        .with_code(DiagnosticCode::ConstraintViolated)])
        .is_none(),
        "the `Code(ConstraintViolated)` entry (#5404): a stale build-side \
         ConstraintViolated can survive \
         the merge while check's authoritative verdict is Satisfied; gating \
         on it would contradict check's own stdout"
    );
}

/// No matcher may be over-broad: `TRAMPOLINE_CANCELLED` shares the
/// `@optimized target "solver::elastic_static": ` prefix with the
/// missing-trampoline message and is a genuine failure that must keep gating.
#[test]
fn allowlist_does_not_swallow_the_prefix_sibling() {
    assert!(
        check_gating_error(&[Diagnostic::error(TRAMPOLINE_CANCELLED)]).is_some(),
        "a cancelled trampoline is a real failure and must gate"
    );
}

/// A `MessageContains` entry excuses CODE-LESS diagnostics only, and no
/// entry excuses the missing-trampoline text.
///
/// This is the executable form of the 2026-08-26 ruling that codes minted
/// after the gate can never be excused: once an emission is coded — #6608's
/// replacement of the code-less "per-instance re-realization compile error
/// … is unresolved (Undef)" family in particular — no legacy substring
/// entry may silently keep excusing it.  The code attached below is
/// arbitrary; it is one no `Code(..)` entry names.
#[test]
fn message_entries_excuse_only_code_less_diagnostics() {
    for msg in [
        "failed to compile geometry operation: argument 'depth' for box is \
         unresolved (Undef)",
        "all geometry operations failed; no geometry output produced",
    ] {
        assert!(
            check_gating_error(&[
                Diagnostic::error(msg).with_code(DiagnosticCode::ArgTypeMismatch)
            ])
            .is_some(),
            "a CODED Error must not be excused by a legacy substring: {msg:?}"
        );
    }

    assert!(
        check_gating_error(&[Diagnostic::error("no registered compute trampoline")]).is_some(),
        "no allowlist entry excuses the code-less missing-trampoline text"
    );
}

/// The load-bearing composition: an allowlisted Error must neither invent a
/// gate nor mask a co-resident one.
#[test]
fn an_excused_error_neither_gates_nor_masks() {
    assert!(
        check_gating_error(&[
            undef_argument_error(),
            Diagnostic::warning("W_DFM_OVERHANG: 62° exceeds 45° limit"),
        ])
        .is_none(),
        "an excused Error beside a Warning must not gate"
    );
    assert!(
        check_gating_error(&[
            trampoline_warning(),
            Diagnostic::warning("W_DFM_OVERHANG: 62° exceeds 45° limit"),
        ])
        .is_none(),
        "an empty-registry trampoline Warning (the LSP's posture) beside a \
         DFM Warning must not gate"
    );

    let mixed = [
        undef_argument_error(),
        Diagnostic::error("E_DFM_OVERHANG: face dips past the overhang limit"),
    ];
    let gating = check_gating_error(&mixed)
        .expect("a co-resident non-allowlisted Error must still gate");
    assert!(
        gating.message.contains("E_DFM_OVERHANG"),
        "the gate must return the diagnostic that actually gates, not the \
         excused one it scanned past; got {:?}",
        gating.message
    );
}

/// Behavioural, not a table-text match, so it holds against any matcher
/// shape.  `ArgTypeMismatch` is guarded whole because a `Code` matcher cannot
/// be scoped to relate call sites.  Code-less relate Errors (the conflict
/// family) can only be swallowed by a `MessageContains` entry and are pinned
/// end to end by `crates/reify-cli/tests/harness_cli/cli_check_relate_family_exit.rs`
/// `check_exits_nonzero_on_engine_phase_relate_conflict`.
#[test]
fn relate_family_errors_are_never_excused() {
    const RELATE_FAMILY_ERROR_CODES: &[DiagnosticCode] = &[
        DiagnosticCode::DatumProjectionUnavailable,
        DiagnosticCode::DatumProjectionAmbiguous,
        DiagnosticCode::TangentOperandsUnsupported,
        DiagnosticCode::ArgTypeMismatch,
        DiagnosticCode::RelateExpectsRelation,
        DiagnosticCode::RelateStaticViolated,
        DiagnosticCode::AssemblyGlobalFloat,
        // TODO(#5436): add β's pose-cycle code (E_POSE_CYCLE) here once it is minted.
    ];

    for code in RELATE_FAMILY_ERROR_CODES {
        let d = Diagnostic::error(format!("{code:?}")).with_code(*code);
        assert!(
            check_gating_error(std::slice::from_ref(&d)).is_some(),
            "a relate-family Error must always move `reify check`'s exit \
             (INV-SF-2); {code:?} is excused — adding it to \
             CHECK_ERROR_EXIT_ALLOWLIST is the regression this test forbids"
        );
    }
}
