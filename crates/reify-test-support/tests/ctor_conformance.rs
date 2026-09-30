//! Behavioural pins for the canonical ctor-conformance test helpers
//! (`reify_test_support::ctor_conformance`), hoisted by task #6323.
//!
//! Before the hoist these helpers existed as four hand-kept copies across
//! separate integration-test binaries, and the failure mode was SILENT: a new
//! member of `DiagnosticCode`'s conformance family added to one copy left the
//! others quietly not matching rather than failing to build. Nothing pinned
//! the admission set itself, so this binary is the pin the copies never had.
//!
//! Deliberately asserts on BEHAVIOUR only — never on docstring prose.

use reify_core::diagnostics::DiagnosticCode;
use reify_test_support::ctor_conformance::{
    CTOR_CONFORMANCE_CODES, ctor_diagnostic_names_arg, is_ctor_conformance_code,
};

/// EXACT membership of the admission set, pinned whole so an ADDITION and a
/// DELETION are each red here — the one place either is visible. A per-member
/// containment loop over the set would be tautological against the
/// `contains`-based predicate, and a per-member loop over a restated list sees
/// additions not at all.
///
/// Membership is the union of the α type codes (tasks 5302 / 4584 / 4598 /
/// 4622 / 4444) and ε's two structural codes (task 5303), which belong
/// together because both families are emitted at the same
/// `CTOR_FIELD_CONFORMANCE_SEVERITY` knob and δ flips them as one.
#[test]
fn the_admission_set_is_exactly_the_seven_ctor_conformance_codes() {
    assert_eq!(
        CTOR_CONFORMANCE_CODES,
        &[
            DiagnosticCode::ArgTypeMismatch,
            DiagnosticCode::SelectorKindMismatch,
            DiagnosticCode::TypeNotConformingToTrait,
            DiagnosticCode::TypeNotConformingToStructureRef,
            DiagnosticCode::TypeNotConformingToVector,
            DiagnosticCode::CtorUnknownField,
            DiagnosticCode::CtorArity,
        ],
        "the ctor-conformance admission set changed. A DROPPED code silently \
         un-filters every count-based pin that narrows through it; an EMPTY \
         set makes `is_ctor_conformance_code` vacuously false everywhere and \
         every such pin vacuously green. An ADDED code is fine on purpose — \
         update this list deliberately, having checked the narrowing pins that \
         read it"
    );
}

/// The predicate honours every member of the set it is derived from.
///
/// Inert against today's one-line `CTOR_CONFORMANCE_CODES.contains(&c)` — that
/// is the point: this arm pins the INTERFACE, so a re-implementation that
/// spells the set out again (a hand-written `matches!`, which is exactly what
/// the four deleted copies were) goes red instead of drifting silently.
#[test]
fn the_predicate_admits_every_member_of_the_set() {
    for code in CTOR_CONFORMANCE_CODES {
        assert!(
            is_ctor_conformance_code(Some(*code)),
            "`{code:?}` is in CTOR_CONFORMANCE_CODES but the predicate rejects it"
        );
    }
}

/// A codeless diagnostic is not admitted. Live shape on this surface: the
/// duplicate-named-arg diagnostic in the same binder is built with a bare
/// `Diagnostic::error` and carries no code at all.
#[test]
fn a_codeless_diagnostic_is_not_admitted() {
    assert!(!is_ctor_conformance_code(None));
}

/// An unrelated code is not admitted — the set is a narrowing filter, not a
/// pass-through.
#[test]
fn an_unrelated_code_is_not_admitted() {
    assert!(!is_ctor_conformance_code(Some(
        DiagnosticCode::GeometryUnbounded
    )));
}

/// `ctor_diagnostic_names_arg` matches the QUOTED param label.
#[test]
fn names_the_quoted_param_label() {
    let message = "E_ARG_TYPE_MISMATCH: argument 'beta' has type 'Real' but \
                   param 'beta' requires type 'Time'";
    assert!(ctor_diagnostic_names_arg(message, "beta"));
}

/// …and does not match a DIFFERENT param's diagnostic.
#[test]
fn does_not_name_a_different_param() {
    let message = "E_ARG_TYPE_MISMATCH: argument 'beta' has type 'Real' but \
                   param 'beta' requires type 'Time'";
    assert!(!ctor_diagnostic_names_arg(message, "alpha"));
}

/// THE LOAD-BEARING ARM. A message containing the param name UNQUOTED must NOT
/// match. This is the property all four former copies' docstrings argued for
/// and none of them pinned: a module compiled through
/// `compile_source_with_stdlib` carries the diagnostics of the probe source AND
/// of the whole stdlib prelude, so a bare `contains(param)` would also catch
/// any prelude message that merely contains the word — including the substring
/// hiding inside an ordinary English word ("o(beta)in").
#[test]
fn an_unquoted_occurrence_of_the_param_name_does_not_match() {
    assert!(
        !ctor_diagnostic_names_arg("failed to obtain the beta value", "beta"),
        "the matcher must key on the QUOTED label, never a bare substring — \
         note `obtain` itself contains `beta`"
    );
    assert!(
        !ctor_diagnostic_names_arg("argument beta has type 'Real'", "beta"),
        "an unquoted `argument beta` must not match the quoted-label matcher"
    );
}
