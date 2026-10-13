//! Renders constraint verdicts and undef causes to the CLI's stdout/stderr writers.
//! This is NOT `reify report` (`cmd_report`, the BOM roll-up), which is unrelated.

use reify_ir::{Satisfaction, UndefCause};

use crate::check_verdict::{ConstraintOutcome, check_fails};

/// Format a single [`reify_ir::UndefCause`] as a terse, human-readable string.
///
/// Called by `cmd_eval` to render the complete cause set for each undef output
/// cell as a comma-joined `because:` clause in the note line (Q5 / PRD §4.4).
///
/// # Variant renderings
///
/// | Variant | Rendered as |
/// |---|---|
/// | `Unbound { param }` | `"<entity>.<member> unbound"` |
/// | `AwaitingSolve { param }` | `"<entity>.<member> awaiting solve"` |
/// | `SolveFailed { detail }` | `"solve failed: <detail>"` |
/// | `OpContractFailed { code, .. }` | `"op contract failed (<code:?>)"` |
/// | `UserUndef { .. }` | `"explicit undef"` |
///
/// The `OpContractFailed` branch is wired for task γ forward-compatibility:
/// γ constructs this variant; δ just formats it so the CLI auto-enriches once
/// γ lands without any re-edit here.
pub(crate) fn format_undef_cause(cause: &UndefCause) -> String {
    match cause {
        UndefCause::Unbound { param, .. } => format!("{param} unbound"),
        UndefCause::AwaitingSolve { param } => format!("{param} awaiting solve"),
        UndefCause::SolveFailed { detail } => format!("solve failed: {detail}"),
        UndefCause::OpContractFailed { code, .. } => format!("op contract failed ({code:?})"),
        UndefCause::UserUndef { .. } => "explicit undef".to_string(),
    }
}

/// Return the display label for a constraint entry: the `label` field when
/// present, or the [`ConstraintNodeId`] Display representation as a fallback.
///
/// Shared by [`report_constraint_results`] and [`report_indeterminate_detail`]
/// so both use the same label-or-id formatting without duplication.
fn constraint_display_label(entry: &reify_eval::ConstraintCheckEntry) -> String {
    match entry.label.as_deref() {
        Some(l) => l.to_string(),
        None => format!("{}", entry.id),
    }
}

/// The line naming an `Indeterminate` entry: its display label, followed by
/// `: {reason}` when its producer recorded one. Nothing stands in for an
/// absent reason. Shared by both reports so the two renderings cannot drift.
fn indeterminate_subject_line(entry: &reify_eval::ConstraintCheckEntry) -> String {
    let label = constraint_display_label(entry);
    match &entry.indeterminate_reason {
        Some(reason) => format!("{label}: {reason}"),
        None => label,
    }
}

/// Write the strict-failure detail block for indeterminate constraints.
///
/// Emits a header naming the count of `Indeterminate` entries, then one
/// indented [`indeterminate_subject_line`] per `Indeterminate` entry, so each
/// shows the reason recorded for it rather than a guessed one. `Satisfied` and
/// `Violated` entries are silently skipped.
///
/// `n` is the already-computed indeterminate count from
/// [`ConstraintOutcome::SomeIndeterminate`]; it is used directly in the header
/// to avoid recomputing the same count independently of [`report_constraint_results`].
fn report_indeterminate_detail(
    n: usize,
    results: &[reify_eval::ConstraintCheckEntry],
    out: &mut impl std::io::Write,
) {
    let _ = writeln!(out, "Strict check failed: {n} constraint(s) INDETERMINATE:");
    for entry in results
        .iter()
        .filter(|e| e.satisfaction == reify_ir::Satisfaction::Indeterminate)
    {
        let _ = writeln!(out, "  {}", indeterminate_subject_line(entry));
    }
}

/// Report constraint check results to the given writer.
///
/// Returns a [`ConstraintOutcome`] indicating the overall result.
/// Each entry is printed as `  {STATUS} {label}` where label falls back to the
/// constraint id's Display representation when `entry.label` is `None`; an
/// `Indeterminate` entry is printed as `  INDETERMINATE {line}` with its
/// [`indeterminate_subject_line`], which shows the recorded reason.
///
/// **Indeterminate constraints are intentionally treated as non-violating.**
/// `Indeterminate` means the constraint could not be decided on this run — for
/// example an `auto` parameter not yet resolved by the solver. Treating these as
/// violations would block evaluations that are otherwise valid and break the
/// incremental evaluation engine. Only explicit `Violated` results cause
/// a `SomeViolated` outcome.
fn report_constraint_results(
    results: &[reify_eval::ConstraintCheckEntry],
    out: &mut impl std::io::Write,
) -> ConstraintOutcome {
    let mut violated = false;
    let mut indeterminate_count: usize = 0;
    for entry in results {
        let (status, subject) = match entry.satisfaction {
            Satisfaction::Satisfied => ("OK", constraint_display_label(entry)),
            Satisfaction::Violated => {
                violated = true;
                ("VIOLATED", constraint_display_label(entry))
            }
            // Indeterminate does not count as violated — undef inputs
            // (auto params, partial evaluation) are not violations.
            // Undef propagates as quiet-NaN semantics.
            Satisfaction::Indeterminate => {
                indeterminate_count += 1;
                ("INDETERMINATE", indeterminate_subject_line(entry))
            }
        };
        let _ = writeln!(out, "  {} {}", status, subject);
    }
    if violated {
        ConstraintOutcome::SomeViolated
    } else if indeterminate_count > 0 {
        ConstraintOutcome::SomeIndeterminate(indeterminate_count)
    } else {
        ConstraintOutcome::AllSatisfied
    }
}

/// Write the terminal summary for `reify check` and return the appropriate
/// [`ExitCode`](std::process::ExitCode).
///
/// Replaces the two byte-identical terminal `match outcome` blocks in
/// `cmd_check` (no-purpose path and `--purpose` path) with a single
/// implementation so the strict upgrade logic lives in one place.
///
/// * [`ConstraintOutcome::AllSatisfied`] → `"All constraints satisfied."` + SUCCESS (to `out`)
/// * [`ConstraintOutcome::SomeViolated`] → `"Some constraints violated."` + FAILURE (to `out`)
/// * [`ConstraintOutcome::SomeIndeterminate(n)`]:
///   * `strict=false` → legacy `"No constraints violated ({n} indeterminate)."` + SUCCESS (to `out`)
///   * `strict=true`  → [`report_indeterminate_detail`] output + FAILURE (to `err`)
///
/// Success-path summaries go to `out` (stdout). The strict-failure narrative goes
/// to `err` (stderr) — conventional for error diagnostics and avoids polluting the
/// stdout stream on the failure path. The exit code remains the machine-parseable
/// contract in both cases.
///
/// **Intentional duplication in strict mode:** when `strict=true` and constraints are
/// INDETERMINATE, callers first invoke [`report_eval_output`] (which writes
/// `INDETERMINATE <label>` status lines to stdout via [`report_constraint_results`])
/// and then call this function, which emits [`report_indeterminate_detail`] to `err`.
/// A machine consumer scanning both streams will therefore see each indeterminate
/// constraint listed in two different formats. This is conventional (per-item status
/// lines on stdout + a failure summary on stderr) and intentional — not a bug.
///
/// Without `--strict` every existing literal string and exit code is preserved
/// byte-for-byte (C2 — backward-compatible behavior).
pub(crate) fn finish_check(
    outcome: &ConstraintOutcome,
    results: &[reify_eval::ConstraintCheckEntry],
    strict: bool,
    out: &mut impl std::io::Write,
    err: &mut impl std::io::Write,
) -> std::process::ExitCode {
    match outcome {
        ConstraintOutcome::AllSatisfied => {
            let _ = writeln!(out, "All constraints satisfied.");
        }
        ConstraintOutcome::SomeViolated => {
            let _ = writeln!(out, "Some constraints violated.");
        }
        ConstraintOutcome::SomeIndeterminate(n) => {
            if strict {
                report_indeterminate_detail(*n, results, err);
            } else {
                let _ = writeln!(out, "No constraints violated ({n} indeterminate).");
            }
        }
    }
    if check_fails(outcome, strict) {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

/// Report constraint results and eval diagnostics in a consistent order.
///
/// Writes constraint status lines to `out` (via [`report_constraint_results`]),
/// then writes each diagnostic to `err`. This ensures both `cmd_check` and
/// `cmd_build` produce output in the same order: constraints first, diagnostics
/// second.
pub(crate) fn report_eval_output(
    constraint_results: &[reify_eval::ConstraintCheckEntry],
    diagnostics: &[reify_core::Diagnostic],
    out: &mut impl std::io::Write,
    err: &mut impl std::io::Write,
) -> ConstraintOutcome {
    let outcome = report_constraint_results(constraint_results, out);
    for diag in diagnostics {
        let _ = writeln!(err, "{}: {}", diag.severity, diag.message);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::ConstraintNodeId;
    use reify_eval::ConstraintCheckEntry;
    use reify_ir::{IndeterminateReason, Satisfaction, TransientReason};

    /// Helper: capture `report_constraint_results` output into an in-memory
    /// buffer and return the outcome plus the formatted output as a `String`.
    fn run_report(entries: &[ConstraintCheckEntry]) -> (ConstraintOutcome, String) {
        let mut buf = Vec::new();
        let result = report_constraint_results(entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        (result, output)
    }

    #[test]
    fn empty_entries_returns_all_satisfied_with_no_output() {
        let (result, output) = run_report(&[]);

        assert_eq!(
            result,
            ConstraintOutcome::AllSatisfied,
            "empty entries should return AllSatisfied (vacuous truth)"
        );
        assert!(
            output.is_empty(),
            "empty entries should produce no output, got: {:?}",
            output
        );
    }

    fn make_entry(
        entity: &str,
        index: u32,
        label: Option<&str>,
        satisfaction: Satisfaction,
    ) -> ConstraintCheckEntry {
        ConstraintCheckEntry {
            id: ConstraintNodeId::new(entity, index),
            label: label.map(|s| s.to_string()),
            satisfaction,
            indeterminate_reason: None,
        }
    }

    fn make_indeterminate_entry(
        entity: &str,
        index: u32,
        label: Option<&str>,
        reason: TransientReason,
    ) -> ConstraintCheckEntry {
        ConstraintCheckEntry {
            indeterminate_reason: Some(IndeterminateReason::Transient(reason)),
            ..make_entry(entity, index, label, Satisfaction::Indeterminate)
        }
    }

    fn operator_undefined_without_kinds() -> TransientReason {
        TransientReason::OperatorUndefinedForKinds { kinds: vec![] }
    }

    #[test]
    fn indeterminate_line_renders_the_recorded_reason() {
        let entries = vec![
            make_entry("Bracket", 0, Some("stress_limit"), Satisfaction::Satisfied),
            make_indeterminate_entry(
                "Beam",
                0,
                Some("load"),
                operator_undefined_without_kinds(),
            ),
        ];
        let (_, output) = run_report(&entries);

        assert_eq!(
            output.lines().collect::<Vec<_>>(),
            vec![
                "  OK stress_limit",
                "  INDETERMINATE load: operator undefined for these operand kinds",
            ]
        );
    }

    #[test]
    fn all_satisfied_returns_true_and_formats_ok() {
        let entries = vec![
            make_entry("Bracket", 0, Some("stress_limit"), Satisfaction::Satisfied),
            make_entry("Bracket", 1, Some("size_bound"), Satisfaction::Satisfied),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::AllSatisfied,
            "should return AllSatisfied when all satisfied"
        );
        assert!(output.contains("  OK stress_limit"));
        assert!(output.contains("  OK size_bound"));
        assert!(!output.contains("VIOLATED"));
    }

    #[test]
    fn violated_returns_false_and_formats_violated() {
        let entries = vec![
            make_entry("Part", 0, Some("max_force"), Satisfaction::Satisfied),
            make_entry("Part", 1, Some("clearance"), Satisfaction::Violated),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeViolated,
            "should return SomeViolated when any violated"
        );
        assert!(output.contains("  OK max_force"));
        assert!(output.contains("VIOLATED clearance"));
    }

    #[test]
    fn indeterminate_formats_correctly_and_counts_as_satisfied() {
        let entries = vec![make_entry(
            "Beam",
            0,
            Some("load"),
            Satisfaction::Indeterminate,
        )];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeIndeterminate(1),
            "indeterminate should return SomeIndeterminate with count"
        );
        assert!(output.contains("INDETERMINATE load"));
    }

    #[test]
    fn violated_with_indeterminate_returns_some_violated() {
        let entries = vec![
            make_entry("Bracket", 0, Some("thickness"), Satisfaction::Violated),
            make_entry("Bracket", 1, Some("tolerance"), Satisfaction::Indeterminate),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeViolated,
            "should return SomeViolated when violated + indeterminate coexist"
        );
        assert!(
            output.contains("VIOLATED thickness"),
            "output should contain 'VIOLATED thickness', got: {}",
            output
        );
        assert!(
            output.contains("INDETERMINATE tolerance"),
            "output should contain 'INDETERMINATE tolerance', got: {}",
            output
        );
        assert!(
            !output.contains("  OK "),
            "output should NOT contain '  OK ' when no constraints are satisfied, got: {}",
            output
        );
    }

    #[test]
    fn three_way_satisfied_violated_indeterminate_returns_some_violated() {
        let entries = vec![
            make_entry("Assembly", 0, Some("weight_limit"), Satisfaction::Satisfied),
            make_entry("Assembly", 1, Some("clearance"), Satisfaction::Violated),
            make_entry("Assembly", 2, Some("thermal"), Satisfaction::Indeterminate),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeViolated,
            "violated takes priority over indeterminate: should return SomeViolated"
        );
        assert!(
            output.contains("  OK weight_limit"),
            "output should contain '  OK weight_limit', got: {}",
            output
        );
        assert!(
            output.contains("VIOLATED clearance"),
            "output should contain 'VIOLATED clearance', got: {}",
            output
        );
        assert!(
            output.contains("INDETERMINATE thermal"),
            "output should contain 'INDETERMINATE thermal', got: {}",
            output
        );
    }

    #[test]
    fn uses_id_display_as_fallback_when_label_is_none() {
        let entries = vec![make_entry("Gear", 2, None, Satisfaction::Satisfied)];
        let (_result, output) = run_report(&entries);

        // ConstraintNodeId Display: "Gear#constraint[2]"
        assert!(
            output.contains("  OK Gear#constraint[2]"),
            "should use id Display as fallback, got: {}",
            output
        );
    }

    #[test]
    fn uses_label_when_present() {
        let entries = vec![make_entry(
            "Axle",
            0,
            Some("torque_limit"),
            Satisfaction::Violated,
        )];
        let (_result, output) = run_report(&entries);

        assert!(
            output.contains("VIOLATED torque_limit"),
            "should use label, got: {}",
            output
        );
        assert!(
            !output.contains("Axle#constraint"),
            "should NOT contain id fallback when label is present"
        );
    }

    #[test]
    fn report_eval_output_writes_constraints_to_out_and_diagnostics_to_err() {
        let constraints = vec![
            make_entry("Bracket", 0, Some("stress_limit"), Satisfaction::Satisfied),
            make_entry("Bracket", 1, Some("size_bound"), Satisfaction::Violated),
        ];
        let diagnostics = vec![
            reify_core::Diagnostic::warning("some msg"),
            reify_core::Diagnostic::error("bad thing"),
        ];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let outcome = report_eval_output(&constraints, &diagnostics, &mut out, &mut err);

        let out_str = String::from_utf8(out).unwrap();
        let err_str = String::from_utf8(err).unwrap();

        // (a) out buffer contains constraint status lines
        assert!(
            out_str.contains("  OK stress_limit"),
            "out should contain constraint OK line, got: {}",
            out_str
        );
        assert!(
            out_str.contains("VIOLATED size_bound"),
            "out should contain constraint VIOLATED line, got: {}",
            out_str
        );

        // (b) err buffer contains diagnostic lines
        assert!(
            err_str.contains("warning: some msg"),
            "err should contain warning diagnostic, got: {}",
            err_str
        );
        assert!(
            err_str.contains("error: bad thing"),
            "err should contain error diagnostic, got: {}",
            err_str
        );

        // (c) correct outcome
        assert_eq!(outcome, ConstraintOutcome::SomeViolated);
    }

    // ── step-3: RED unit tests for report_indeterminate_detail ───────────────

    #[test]
    fn report_indeterminate_detail_lists_only_indeterminate_entries() {
        // Mix: satisfied, indeterminate (with label), violated, indeterminate
        // (no label — must fall back to id Display "Foo#constraint[3]").
        let entries = vec![
            make_entry("Bracket", 0, Some("c_ok"), Satisfaction::Satisfied),
            make_indeterminate_entry(
                "Bracket",
                1,
                Some("c_bad"),
                TransientReason::UndefInputs {
                    cells: vec![reify_core::ValueCellId::new("Bracket", "tolerance")],
                },
            ),
            make_entry("Bracket", 2, Some("c_v"), Satisfaction::Violated),
            make_indeterminate_entry("Foo", 3, None, operator_undefined_without_kinds()),
        ];
        let mut buf = Vec::new();
        report_indeterminate_detail(2, &entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

        // Each listed constraint names the reason its producer recorded, and
        // the header guesses none.
        assert_eq!(
            output.lines().collect::<Vec<_>>(),
            vec![
                "Strict check failed: 2 constraint(s) INDETERMINATE:",
                "  c_bad: undefined inputs: Bracket.tolerance",
                "  Foo#constraint[3]: operator undefined for these operand kinds",
            ]
        );
    }

    /// An Indeterminate with no recorded reason is listed bare: nothing is
    /// fabricated in its place.
    #[test]
    fn report_indeterminate_detail_without_a_reason_lists_the_bare_label() {
        let entries = vec![make_entry("Part", 0, Some("load"), Satisfaction::Indeterminate)];
        let mut buf = Vec::new();
        report_indeterminate_detail(1, &entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

        assert_eq!(
            output.lines().collect::<Vec<_>>(),
            vec!["Strict check failed: 1 constraint(s) INDETERMINATE:", "  load"]
        );
    }

    #[test]
    fn report_indeterminate_detail_single_entry_count_one() {
        let entries = vec![make_entry(
            "Part",
            0,
            Some("load"),
            Satisfaction::Indeterminate,
        )];
        let mut buf = Vec::new();
        report_indeterminate_detail(1, &entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

        // Count is 1 and the labelled constraint is listed.
        assert!(
            output.contains("1"),
            "header should name the indeterminate count (1), got: {output}"
        );
        assert!(
            output.contains("load"),
            "output should list 'load', got: {output}"
        );
    }

    // ── end step-3 ────────────────────────────────────────────────────────────

    // ── step-5: RED unit tests for finish_check writer output ────────────────

    #[test]
    fn finish_check_non_strict_indeterminate_emits_unchanged_summary() {
        // (a) !strict + SomeIndeterminate(1) → byte-identical "No constraints
        // violated (1 indeterminate).\n" regression guard.
        let entries = vec![make_entry(
            "Bracket",
            1,
            Some("tolerance"),
            Satisfaction::Indeterminate,
        )];
        let outcome = ConstraintOutcome::SomeIndeterminate(1);
        let mut buf = Vec::new();
        let mut err_buf = Vec::new();
        finish_check(&outcome, &entries, false, &mut buf, &mut err_buf);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(
            output, "No constraints violated (1 indeterminate).\n",
            "non-strict SomeIndeterminate(1) must produce the exact legacy summary line"
        );
    }

    #[test]
    fn finish_check_strict_indeterminate_emits_detail_not_legacy_line() {
        // (b) strict + SomeIndeterminate → the strict-failure block goes to `err`
        // (stderr); `out` (stdout) must remain empty. The failure narrative must
        // contain "Strict check failed" and name the indeterminate constraint; the
        // legacy summary "No constraints violated" must NOT appear in either stream.
        let entries = vec![make_entry(
            "Bracket",
            1,
            Some("tolerance"),
            Satisfaction::Indeterminate,
        )];
        let outcome = ConstraintOutcome::SomeIndeterminate(1);
        let mut buf = Vec::new();
        let mut err_buf = Vec::new();
        finish_check(&outcome, &entries, true, &mut buf, &mut err_buf);
        let out_str = String::from_utf8(buf).unwrap();
        let err_str = String::from_utf8(err_buf).unwrap();
        assert!(
            err_str.contains("Strict check failed"),
            "strict SomeIndeterminate: 'Strict check failed' must appear on stderr, got err: {err_str}"
        );
        assert!(
            err_str.contains("tolerance"),
            "strict SomeIndeterminate: constraint name 'tolerance' must appear on stderr, got err: {err_str}"
        );
        assert!(
            !out_str.contains("No constraints violated"),
            "strict SomeIndeterminate: 'No constraints violated' must NOT appear on stdout, got: {out_str}"
        );
        assert!(
            !out_str.contains("Strict check failed"),
            "strict SomeIndeterminate: 'Strict check failed' must NOT appear on stdout, got: {out_str}"
        );
    }

    #[test]
    fn finish_check_all_satisfied_either_strict() {
        // (c) AllSatisfied (either strict value) → "All constraints satisfied.\n".
        let entries: Vec<reify_eval::ConstraintCheckEntry> = vec![];
        let outcome = ConstraintOutcome::AllSatisfied;
        for strict in [false, true] {
            let mut buf = Vec::new();
            let mut err_buf = Vec::new();
            finish_check(&outcome, &entries, strict, &mut buf, &mut err_buf);
            let output = String::from_utf8(buf).unwrap();
            assert_eq!(
                output, "All constraints satisfied.\n",
                "AllSatisfied (strict={strict}) must produce 'All constraints satisfied.'"
            );
        }
    }

    #[test]
    fn finish_check_some_violated_either_strict() {
        // (d) SomeViolated (either strict value) → "Some constraints violated.\n".
        let entries: Vec<reify_eval::ConstraintCheckEntry> = vec![];
        let outcome = ConstraintOutcome::SomeViolated;
        for strict in [false, true] {
            let mut buf = Vec::new();
            let mut err_buf = Vec::new();
            finish_check(&outcome, &entries, strict, &mut buf, &mut err_buf);
            let output = String::from_utf8(buf).unwrap();
            assert_eq!(
                output, "Some constraints violated.\n",
                "SomeViolated (strict={strict}) must produce 'Some constraints violated.'"
            );
        }
    }

    // ── end step-5 ────────────────────────────────────────────────────────────

    #[test]
    fn report_eval_output_returns_correct_outcome_variants() {
        let no_diags: Vec<reify_core::Diagnostic> = vec![];

        // AllSatisfied: all constraints OK
        {
            let entries = vec![
                make_entry("A", 0, Some("c1"), Satisfaction::Satisfied),
                make_entry("A", 1, Some("c2"), Satisfaction::Satisfied),
            ];
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = report_eval_output(&entries, &no_diags, &mut out, &mut err);
            assert_eq!(outcome, ConstraintOutcome::AllSatisfied);
        }

        // SomeViolated: at least one violated
        {
            let entries = vec![
                make_entry("B", 0, Some("c1"), Satisfaction::Satisfied),
                make_entry("B", 1, Some("c2"), Satisfaction::Violated),
            ];
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = report_eval_output(&entries, &no_diags, &mut out, &mut err);
            assert_eq!(outcome, ConstraintOutcome::SomeViolated);
        }

        // SomeIndeterminate: indeterminate but no violated
        {
            let entries = vec![
                make_entry("C", 0, Some("c1"), Satisfaction::Satisfied),
                make_entry("C", 1, Some("c2"), Satisfaction::Indeterminate),
                make_entry("C", 2, Some("c3"), Satisfaction::Indeterminate),
            ];
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = report_eval_output(&entries, &no_diags, &mut out, &mut err);
            assert_eq!(outcome, ConstraintOutcome::SomeIndeterminate(2));
        }
    }
}

// ── format_undef_cause unit tests (task 4327 / undef-self-describing δ) ──────
//
// Tests Q5: terse text rendering of every UndefCause variant.
// The function under test (`format_undef_cause`) does not yet exist —
// this module is RED until step-2 (GREEN) implements it.
#[cfg(test)]
mod format_undef_cause_tests {
    use super::format_undef_cause;
    use reify_core::{DiagnosticCode, SourceSpan, ValueCellId};
    use reify_ir::UndefCause;

    fn cell(entity: &str, member: &str) -> ValueCellId {
        ValueCellId::new(entity, member)
    }

    fn span(start: u32, end: u32) -> SourceSpan {
        SourceSpan::new(start, end)
    }

    /// Unbound param: renders as "<entity>.<member> unbound".
    #[test]
    fn unbound_renders_cell_name_and_unbound() {
        let cause = UndefCause::Unbound {
            param: cell("S", "outer_diameter"),
            span: span(0, 14),
        };
        assert_eq!(format_undef_cause(&cause), "S.outer_diameter unbound");
    }

    /// AwaitingSolve: renders as "<entity>.<member> awaiting solve".
    #[test]
    fn awaiting_solve_renders_cell_name_and_awaiting_solve() {
        let cause = UndefCause::AwaitingSolve {
            param: cell("S", "k"),
        };
        assert_eq!(format_undef_cause(&cause), "S.k awaiting solve");
    }

    /// SolveFailed: renders as "solve failed: <detail>".
    #[test]
    fn solve_failed_renders_detail() {
        let cause = UndefCause::SolveFailed {
            detail: "infeasible".to_string(),
        };
        assert_eq!(format_undef_cause(&cause), "solve failed: infeasible");
    }

    /// UserUndef: renders as "explicit undef".
    #[test]
    fn user_undef_renders_explicit_undef() {
        let cause = UndefCause::UserUndef { span: span(5, 5) };
        assert_eq!(format_undef_cause(&cause), "explicit undef");
    }

    /// OpContractFailed (γ forward-compat): non-empty and contains "contract".
    #[test]
    fn op_contract_failed_is_nonempty_and_contains_contract() {
        let cause = UndefCause::OpContractFailed {
            code: DiagnosticCode::ConstraintViolated,
            span: span(0, 5),
        };
        let rendered = format_undef_cause(&cause);
        assert!(!rendered.is_empty(), "rendered string must not be empty");
        assert!(
            rendered.contains("contract"),
            "rendered string must contain \"contract\", got: {rendered:?}"
        );
    }
}
