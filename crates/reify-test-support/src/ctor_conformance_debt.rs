//! Per-SITE waivers for ctor-conformance diagnostics that a shipped example
//! still emits, and the predicate that applies them to a `Diagnostic`.
//!
//! # Why this lives in `reify-test-support` rather than next to one of its readers
//!
//! Task δ (#5306) flipped `CTOR_FIELD_CONFORMANCE_SEVERITY` to `Severity::Error`,
//! which put the two un-migrated `examples/trajectory/printer_print_envelope.ri`
//! call sites in front of THREE zero-Error gates living in TWO different crates:
//!
//! * `crates/reify-compiler/tests/harness_compilation_surface/examples_smoke.rs`
//! * `crates/reify-compiler/tests/harness_physical_modeling/printer_print_envelope_example_tests.rs`
//! * `crates/reify-eval/tests/printer_print_envelope_e2e.rs` (release profile only)
//!
//! A `#[path]` sibling module can reach across test files inside ONE test binary;
//! it cannot reach across crates. `reify-test-support` is a dev-dependency of all
//! three, so it is the one hop that lets the waiver rule stay stated exactly once.
//!
//! # These sites are WAIVED, not fixed
//!
//! esc-5305-3 (Leo): #5847 owns dimensioning `trajectory/printer_print_envelope.ri:154`
//! / `:155`, and the sites cannot be dimensioned in isolation without collapsing the
//! TOTS solve. δ therefore keeps them COVERED by this waiver rather than migrating
//! them or taking a dependency edge on #5847.

use reify_core::diagnostics::DiagnosticCode;
use reify_core::{Diagnostic, Severity};

use crate::ctor_conformance::CTOR_DIAGNOSTIC_ARG_PREFIX;

/// Per-SITE waivers for ctor-conformance diagnostics that a shipped example
/// still emits because its call site has not been migrated yet, and cannot be
/// migrated by the task that promoted the family.
///
/// Each entry is `(relative_path, param_name, owning_task)`:
/// * `relative_path` is the same forward-slash `relative_to_examples_dir` key
///   form `examples_smoke::SKIP_SET` uses (`"trajectory/printer_print_envelope.ri"`, never the
///   repo-relative `"examples/trajectory/..."` spelling);
/// * `param_name` is the offending ctor param, parsed back out of the
///   diagnostic by [`param_name_from_ctor_diagnostic`];
/// * `owning_task` is the live task that owns retiring the entry, in the
///   canonical `#NNNN` cite form required by the repo's citation convention.
///
/// # This is NOT `examples_smoke::SKIP_SET`, and must never be merged into it
///
/// `examples_smoke::SKIP_SET` is for files that cannot reach a clean compile AT ALL — the file
/// is dropped from the walk entirely, so it gets no coverage of any kind.
/// `printer_print_envelope.ri` compiles cleanly; it merely carries two
/// un-migrated call sites. It stays fully walked, and every OTHER diagnostic it
/// emits still fails the gate.
///
/// # The waiver is per-SITE, never per-file
///
/// Matching is on the `(file, param)` PAIR. A future diagnostic in the same file
/// at a different param is unwaived and fails the gate, as does a diagnostic at
/// one of these params that carries a different, non-`argument '<name>'`
/// wording.
///
/// # Retirement
///
/// Task #5847 owns deleting BOTH entries in the same diff that dimensions
/// `trajectory/printer_print_envelope.ri:154` / `:155` (esc-5627-5 option A).
/// The sites cannot be dimensioned in isolation without collapsing the TOTS
/// solve, which is the whole reason the debt exists rather than the migration
/// simply having been done. Leaving the entries behind after that lands is
/// caught by `ctor_conformance_migration_debt_entries_are_all_live`.
pub const CTOR_CONFORMANCE_MIGRATION_DEBT: &[(&str, &str, &str)] = &[
    (
        "trajectory/printer_print_envelope.ri",
        "velocity_limit",
        "#5847",
    ),
    (
        "trajectory/printer_print_envelope.ri",
        "acceleration_limit",
        "#5847",
    ),
];

/// Recover the offending param name from a ctor-conformance diagnostic message.
///
/// A `Diagnostic` carries no structured param field, so the only handle the
/// per-site waiver has is the wording: the text between the first pair of single
/// quotes following the `argument '` prefix. Returns `None` for any message that
/// does not have that shape (the non-`ArgTypeMismatch` ctor-conformance codes),
/// which makes such a diagnostic unwaivable rather than silently waived.
///
/// This is a real coupling to diagnostic prose, and it is deliberately guarded
/// rather than merely commented: if the wording ever drifts so extraction stops
/// matching, `examples_smoke::ctor_conformance_migration_debt_entries_are_all_live` goes red
/// naming the entry that stopped matching. The prefix it keys on is the shared
/// [`crate::ctor_conformance::CTOR_DIAGNOSTIC_ARG_PREFIX`] (#6323), whose doc
/// comment is where that coupling is recorded once.
///
/// Single copy, not a duplication of that module: this EXTRACTS the param name,
/// where the shared `ctor_diagnostic_names_arg` only TESTS for a given one.
pub fn param_name_from_ctor_diagnostic(message: &str) -> Option<String> {
    let start = message.find(CTOR_DIAGNOSTIC_ARG_PREFIX)? + CTOR_DIAGNOSTIC_ARG_PREFIX.len();
    let rest = &message[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_owned())
}

/// Whether `entry` (a [`CTOR_CONFORMANCE_MIGRATION_DEBT`] row) waives the site
/// `(file, param)`.
///
/// Both halves of the key must match: the file AND the param. An entry whose
/// param does not match — including because extraction returned `None` — waives
/// nothing.
///
/// Takes the two key halves rather than a `CtorConformanceViolation` so the
/// sibling `ctor_conformance_corpus_survey` module can apply the SAME rule to a
/// `SurveySite`, which carries the same pair under different field names. The
/// rule stays defined exactly once.
pub fn debt_entry_matches(entry: &(&str, &str, &str), file: &str, param: Option<&str>) -> bool {
    entry.0 == file && param == Some(entry.1)
}

/// Whether `d`, observed while compiling the example at `rel_key`, is one of the
/// [`CTOR_CONFORMANCE_MIGRATION_DEBT`] sites δ (#5306) deliberately left
/// un-migrated.
///
/// This is the predicate the three zero-Error gates named in the module doc
/// route their `Severity::Error` filter through. It is deliberately NARROWER
/// than the severity-agnostic `(file, param)` rule [`debt_entry_matches`]
/// states, on two axes:
///
/// * **Severity.** Only an `Error` can be waived. The waiver exists solely to
///   hold back δ's promotion; a Warning at the same site is still fully visible
///   to `examples_smoke`'s code-filtered corpus gate, which is severity-agnostic
///   by design.
/// * **Code.** Only [`DiagnosticCode::ArgTypeMismatch`] — the one variant BOTH
///   printer rows actually emit, per
///   `docs/prds/struct-ctor-field-type-conformance.survey.md`. Keying on the
///   single concrete variant rather than on the seven-member ctor-conformance
///   code set keeps the waiver from becoming a file-level escape hatch for an
///   unrelated Error, and needs nothing from #6323 (which owns hoisting that
///   set).
///
/// Param extraction fails CLOSED: a message with no `argument '<name>'` shape
/// yields `None`, which matches no entry.
pub fn is_migration_debt_diagnostic(rel_key: &str, d: &Diagnostic) -> bool {
    d.severity == Severity::Error
        && d.code == Some(DiagnosticCode::ArgTypeMismatch)
        && CTOR_CONFORMANCE_MIGRATION_DEBT.iter().any(|entry| {
            debt_entry_matches(
                entry,
                rel_key,
                param_name_from_ctor_diagnostic(&d.message).as_deref(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::diagnostics::DiagnosticCode;
    use reify_core::{Diagnostic, Severity};

    /// The `rel_key` both debt rows are filed under — the forward-slash
    /// `relative_to_examples_dir` spelling, never the repo-relative
    /// `examples/trajectory/...` one.
    const PRINTER: &str = "trajectory/printer_print_envelope.ri";

    /// A post-δ `emit_arg_type_mismatch` diagnostic for `param`, in the exact
    /// shape `crates/reify-compiler/src/conformance/mod.rs` produces for a bare
    /// `Real` argument at a dimensioned `Scalar` slot (D4-6 migration hint and
    /// all).
    fn ctor_mismatch(param: &str, unit: &str) -> Diagnostic {
        Diagnostic::error(format!(
            "argument '{param}' has type 'Real' but param '{param}' requires type \
             'Scalar[{unit}]'; pass a dimensioned literal (e.g. `1{unit}`) or a \
             dimensioned expression"
        ))
        .with_code(DiagnosticCode::ArgTypeMismatch)
    }

    /// (f) The predicate tests below can only mean something against a NON-EMPTY
    /// table: an emptied `CTOR_CONFORMANCE_MIGRATION_DEBT` would make every
    /// `is_migration_debt_diagnostic` call return `false` and turn cases (b)–(e)
    /// vacuously green while silently disarming case (a).
    #[test]
    fn migration_debt_table_is_non_empty() {
        assert!(
            !CTOR_CONFORMANCE_MIGRATION_DEBT.is_empty(),
            "CTOR_CONFORMANCE_MIGRATION_DEBT is empty — the waiver predicate's \
             negative cases would all pass vacuously. If #5847 landed and both \
             printer rows were retired, DELETE this module and its three readers \
             rather than leaving an empty table behind."
        );
    }

    /// (a) Both live rows waive their own site.
    #[test]
    fn both_printer_rows_waive_their_own_diagnostic() {
        for (param, unit) in [
            ("velocity_limit", "m·s^-1"),
            ("acceleration_limit", "m·s^-2"),
        ] {
            let d = ctor_mismatch(param, unit);
            assert!(
                is_migration_debt_diagnostic(PRINTER, &d),
                "the #5847-owned '{param}' site in {PRINTER} must be waived, got: {d:?}"
            );
        }
    }

    /// (b) The FILE half of the key is load-bearing: the identical message
    /// emitted from some other example is unwaived.
    #[test]
    fn the_same_message_in_another_file_is_not_waived() {
        let d = ctor_mismatch("velocity_limit", "m·s^-1");
        assert!(
            !is_migration_debt_diagnostic("trajectory/some_other_example.ri", &d),
            "the waiver is per-SITE: a matching param name in an unlisted file must \
             NOT be waived, got: {d:?}"
        );
    }

    /// (c) The PARAM half of the key is load-bearing: a different param in the
    /// waived file is unwaived, so the file keeps full coverage of every OTHER
    /// diagnostic it can emit.
    #[test]
    fn a_different_param_in_the_waived_file_is_not_waived() {
        let d = ctor_mismatch("jerk_limit", "m·s^-3");
        assert!(
            !is_migration_debt_diagnostic(PRINTER, &d),
            "the waiver is per-SITE, never per-file: an unlisted param in a listed \
             file must NOT be waived, got: {d:?}"
        );
    }

    /// (d) The waiver is keyed on the ctor-conformance CODE too, so it can never
    /// degrade into a blanket file-level escape hatch for unrelated Errors that
    /// happen to mention one of the waived param names.
    #[test]
    fn a_non_arg_type_mismatch_error_in_the_waived_file_is_not_waived() {
        let uncoded = Diagnostic::error(
            "argument 'velocity_limit' has type 'Real' but param 'velocity_limit' \
             requires type 'Scalar[m·s^-1]'",
        );
        assert_eq!(uncoded.code, None);
        assert!(
            !is_migration_debt_diagnostic(PRINTER, &uncoded),
            "a code-less Error must NOT be waived, got: {uncoded:?}"
        );

        let other_code = Diagnostic::error("unresolved type: Velocity")
            .with_code(DiagnosticCode::UnresolvedType);
        assert!(
            !is_migration_debt_diagnostic(PRINTER, &other_code),
            "an Error carrying a non-ArgTypeMismatch code must NOT be waived, got: \
             {other_code:?}"
        );
    }

    /// (e) Param extraction fails CLOSED: a message that does not carry the
    /// `argument '<name>'` shape yields `None`, which matches no entry.
    ///
    /// That is the same conservative default the helper had before it moved here
    /// — an unwaivable diagnostic is visible, a silently-waived one is not.
    #[test]
    fn a_message_without_the_argument_prefix_is_not_waived() {
        assert_eq!(
            param_name_from_ctor_diagnostic(
                "E_CTOR_ARITY: Widget() expects at most 1 argument, got 2"
            ),
            None,
            "extraction must fail closed on a message with no `argument '<name>'` shape"
        );

        let d = Diagnostic::error("velocity_limit is not conforming")
            .with_code(DiagnosticCode::ArgTypeMismatch);
        assert!(
            !is_migration_debt_diagnostic(PRINTER, &d),
            "a diagnostic whose param cannot be extracted must NOT be waived, got: {d:?}"
        );
    }

    /// Severity is part of the predicate: the waiver exists only to hold back δ's
    /// Error-severity promotion, so it must not swallow a Warning the corpus gates
    /// still want to see.
    #[test]
    fn a_warning_at_a_waived_site_is_not_waived() {
        let mut d = ctor_mismatch("velocity_limit", "m·s^-1");
        d.severity = Severity::Warning;
        assert!(
            !is_migration_debt_diagnostic(PRINTER, &d),
            "the waiver is scoped to Error severity — δ's promotion is the only thing \
             it holds back. Got: {d:?}"
        );
    }
}
