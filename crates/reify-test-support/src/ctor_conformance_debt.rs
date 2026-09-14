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
        for (param, unit) in [("velocity_limit", "m·s^-1"), ("acceleration_limit", "m·s^-2")] {
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
            param_name_from_ctor_diagnostic("E_CTOR_ARITY: Widget() expects at most 1 argument, got 2"),
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
