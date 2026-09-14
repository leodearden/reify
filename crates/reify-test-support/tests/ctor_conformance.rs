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

/// Every admitted code is admitted. Iterates the SET rather than restating the
/// list, so the predicate and the set it is derived from cannot drift apart —
/// an ADDITION to `CTOR_CONFORMANCE_CODES` that the predicate does not honour
/// goes red here instead of silently widening nothing.
#[test]
fn every_member_of_the_code_set_is_admitted() {
    assert!(
        !CTOR_CONFORMANCE_CODES.is_empty(),
        "the admission set must not be empty — an empty set would make \
         `is_ctor_conformance_code` vacuously false everywhere and every \
         count-based pin that filters through it vacuously green"
    );
    for code in CTOR_CONFORMANCE_CODES {
        assert!(
            is_ctor_conformance_code(Some(*code)),
            "`{code:?}` is in CTOR_CONFORMANCE_CODES but the predicate rejects it"
        );
    }
}

/// The seven variants named explicitly, so a DELETION from the set goes red
/// rather than silently shrinking the admission surface. The iterating test
/// above cannot see a deletion — it would still pass over the smaller set.
///
/// Membership here is the union of the α type codes (tasks 5302 / 4584 / 4598
/// / 4622 / 4444) and ε's two structural codes (task 5303), which belong
/// together because both families are emitted at the same
/// `CTOR_FIELD_CONFORMANCE_SEVERITY` knob and δ flips them as one.
#[test]
fn the_seven_admitted_codes_are_all_present() {
    for code in [
        DiagnosticCode::ArgTypeMismatch,
        DiagnosticCode::SelectorKindMismatch,
        DiagnosticCode::TypeNotConformingToTrait,
        DiagnosticCode::TypeNotConformingToStructureRef,
        DiagnosticCode::TypeNotConformingToVector,
        DiagnosticCode::CtorUnknownField,
        DiagnosticCode::CtorArity,
    ] {
        assert!(
            CTOR_CONFORMANCE_CODES.contains(&code),
            "`{code:?}` must stay in the ctor-conformance admission set; \
             dropping it silently un-filters every count-based pin that \
             narrows through it"
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
