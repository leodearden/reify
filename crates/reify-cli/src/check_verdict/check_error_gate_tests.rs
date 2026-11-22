//! Unit behaviour for INV-SF-2's two pure exit-gate helpers, built from
//! synthetic [`reify_core::Diagnostic`] values — no OCCT, no CLI exec, so
//! these run in a stub-mode build.

use super::{check_gating_error, has_error_diagnostic};
use reify_core::{Diagnostic, DiagnosticCode, Severity};

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

/// The code-less geometry Error `geometry_ops.rs::unresolved_arg_message` words
/// for an argument left `Undef`.
fn undef_argument_error() -> Diagnostic {
    Diagnostic::error(
        "failed to compile geometry operation: argument 'depth' for box is unresolved (Undef)",
    )
}

/// The code-less sibling emission that SHARES the `@optimized target `
/// prefix and must keep gating.
const TRAMPOLINE_CANCELLED: &str =
    "@optimized target \"solver::elastic_static\": compute trampoline was cancelled";

/// Warning-severity DFM findings: the family that most looks like it should gate.
const W_DFM_OVERHANG: &str = "W_DFM_OVERHANG: 62° exceeds 45° limit";
const W_DFM_DRAFT: &str = "W_DFM_DRAFT: 0.5° below the 1° minimum";

/// Error-severity DFM findings, once gated by a deleted `E_DFM_` message filter.
const E_DFM_MESSAGES: [&str; 3] = [
    "E_DFM_OVERHANG: face dips past the overhang limit",
    "E_DFM_UNDERCUT: re-entrant wall — part cannot release",
    "E_DFM_DRAFT: 0.2° below the 1° minimum",
];

const VIOLATION: &str =
    "constraint BoltFlange#constraint[1] violated: clearance 0.4mm below minimum 0.5mm";

fn violation_error() -> Diagnostic {
    Diagnostic::error(VIOLATION).with_code(DiagnosticCode::ConstraintViolated)
}

/// The code-less rollup Error for declared geometry that produced nothing.
fn rollup_error() -> Diagnostic {
    Diagnostic::error("all geometry operations failed; no geometry output produced")
}

/// `GdtIllegalModifier`'s one emission shape
/// (`engine_constraints::illegal_modifier_error`), unconditionally an Error.
fn gdt_error() -> Diagnostic {
    Diagnostic::error(
        "`flatness` is an RFS-only characteristic; the `M` material-condition \
         modifier is illegal on it",
    )
    .with_code(DiagnosticCode::GdtIllegalModifier)
}

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

fn relate_family_error(code: DiagnosticCode) -> Diagnostic {
    Diagnostic::error(format!("{code:?}")).with_code(code)
}

/// Every diagnostic shape the gate tests know, as whole sets: each single
/// Error, the Warning- and Info-only sets, and the mixed orderings where the
/// first Error must gate and no co-resident Error may be masked.
fn gate_fixture_sets() -> Vec<Vec<Diagnostic>> {
    let mut sets = vec![
        vec![],
        vec![
            Diagnostic::warning(W_DFM_OVERHANG),
            Diagnostic::warning(W_DFM_DRAFT),
            trampoline_warning(),
        ],
        vec![Diagnostic::info("note: nothing to see")],
        vec![trampoline_error()],
        vec![undef_argument_error()],
        vec![Diagnostic::error(TRAMPOLINE_CANCELLED)],
        vec![gdt_error()],
        vec![violation_error()],
        vec![rollup_error()],
        vec![
            Diagnostic::warning(W_DFM_OVERHANG),
            Diagnostic::info("note: nothing to see"),
            Diagnostic::error("first error"),
            Diagnostic::error("second error"),
        ],
        vec![undef_argument_error(), Diagnostic::error(E_DFM_MESSAGES[0])],
        vec![trampoline_warning(), trampoline_error()],
    ];
    sets.extend(E_DFM_MESSAGES.map(|msg| vec![Diagnostic::error(msg)]));
    sets.extend(
        RELATE_FAMILY_ERROR_CODES
            .iter()
            .map(|code| vec![relate_family_error(*code)]),
    );
    sets
}

// -------------------------------------------------------------------
// has_error_diagnostic — the shared severity predicate
// -------------------------------------------------------------------

/// `has_error_diagnostic` is the ONE definition of "this set carries an
/// Error", shared with `cmd_eval` and `cmd_build`, which have gated on
/// `Severity::Error` since #4458 — including on the trampoline Error, which
/// `build_is_success`' own doc names as its motivating example.
#[test]
fn has_error_diagnostic_is_pure_severity() {
    assert!(!has_error_diagnostic(&[]), "empty set carries no Error");
    assert!(
        !has_error_diagnostic(&[Diagnostic::warning(W_DFM_OVERHANG)]),
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
        "nothing excuses the coded Error, so it gates check"
    );
    assert!(
        has_error_diagnostic(&[trampoline_error()]),
        "eval/build keep gating on the coded Error"
    );
}

// -------------------------------------------------------------------
// check_gating_error — every Severity::Error gates
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
            Diagnostic::warning(W_DFM_OVERHANG),
            Diagnostic::warning(W_DFM_DRAFT),
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
    assert!(
        check_gating_error(&[gdt_error()]).is_some(),
        "GdtIllegalModifier still exits non-zero, now via the general gate"
    );

    for msg in E_DFM_MESSAGES {
        assert!(
            check_gating_error(&[Diagnostic::error(msg)]).is_some(),
            "DFM Error {msg:?} still exits non-zero, now via the general gate"
        );
    }
}

/// Families whose allowlist entry has been retired now gate like any other
/// Error: whatever still excused them is fixed upstream of the gate.
#[test]
fn retired_allowlist_families_now_gate() {
    assert!(
        check_gating_error(&[violation_error()]).is_some(),
        "a violation Error that survives the merge is real: stale ones are \
         removed upstream by drop_falsified_constraint_diagnostics"
    );
    assert!(
        check_gating_error(&[undef_argument_error()]).is_some(),
        "a code-less Undef-argument Error gates: check solves, so what remains \
         is a genuine Error that `reify eval` also exits 1 on"
    );
    assert!(
        check_gating_error(&[rollup_error()]).is_some(),
        "the code-less rollup Error gates: declared geometry that produced \
         nothing is never a healthy path"
    );
}

/// Behavioural, so it holds against any shape of excuse.  `ArgTypeMismatch` is
/// guarded whole because an excuse keyed on a code cannot be scoped to relate
/// call sites.  Code-less relate Errors (the conflict family) can only be
/// swallowed by a message-text excuse and are pinned
/// end to end by `crates/reify-cli/tests/harness_cli_check/cli_check_relate_family_exit.rs`
/// `check_exits_nonzero_on_engine_phase_relate_conflict`.
#[test]
fn relate_family_errors_are_never_excused() {
    for code in RELATE_FAMILY_ERROR_CODES {
        assert!(
            check_gating_error(&[relate_family_error(*code)]).is_some(),
            "a relate-family Error must always move `reify check`'s exit \
             (INV-SF-2); {code:?} is excused — excusing it at the gate is \
             the regression this test forbids"
        );
    }
}

/// The guard against re-adding an excuse (Leo's rulings of 2026-08-26 and
/// 2026-11-18): no Error is ever excused at `reify check`'s gate.  Over every
/// fixture set and every singleton, the gate agrees with eval/build's
/// severity predicate and returns the FIRST Error by position, so any
/// row-shaped excuse reds here.
#[test]
fn check_gating_error_agrees_with_has_error_diagnostic() {
    let sets = gate_fixture_sets();
    let singletons: Vec<Vec<Diagnostic>> = sets.iter().flatten().map(|d| vec![d.clone()]).collect();
    for set in sets.iter().chain(&singletons) {
        let messages: Vec<&str> = set.iter().map(|d| d.message.as_str()).collect();
        let gating = check_gating_error(set);
        assert_eq!(
            gating.is_some(),
            has_error_diagnostic(set),
            "check's gate and eval/build's severity predicate disagree on {messages:?}"
        );
        let first_error = set.iter().find(|d| d.severity == Severity::Error);
        let gates_first_error = match (gating, first_error) {
            (Some(gating), Some(first)) => std::ptr::eq(gating, first),
            (None, None) => true,
            _ => false,
        };
        assert!(
            gates_first_error,
            "check must gate on the first Error of {messages:?}; got {:?}",
            gating.map(|d| &d.message)
        );
    }
}
