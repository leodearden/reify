use super::merge_build_diagnostics;
use reify_core::{Diagnostic, DiagnosticCode, Severity};

/// Renders a diagnostic down to the three fields the merge compares, so
/// assertions read as data rather than as a hand-written `PartialEq`
/// (`reify_core::Diagnostic` derives only `Debug, Clone`).
fn key(d: &Diagnostic) -> (Severity, Option<DiagnosticCode>, String) {
    (d.severity, d.code, d.message.clone())
}

fn keys(ds: &[Diagnostic]) -> Vec<(Severity, Option<DiagnosticCode>, String)> {
    ds.iter().map(key).collect()
}

/// (1) check()'s list is copied VERBATIM, in order, at the front, and (2) a
/// build-only entry is appended after it.
///
/// This is PRD D2's pair of headline invariants in one assertion: the
/// authoritative list is never reordered, filtered or deduped against
/// itself (`check` remains the source of record), and a diagnostic that
/// only `build()` produces still reaches the user.
#[test]
fn merge_copies_check_verbatim_and_appends_build_only() {
    let check = vec![
        Diagnostic::warning("first check warning"),
        Diagnostic::error("second check error"),
    ];
    let build = vec![Diagnostic::error(
        "failed to compile geometry operation: missing or non-Length argument 'ox' for mirror",
    )];

    let merged = merge_build_diagnostics(&check, &build);

    assert_eq!(
        keys(&merged),
        [keys(&check), keys(&build)].concat(),
        "merged must be check's list verbatim and in order, then build-only entries"
    );
}

/// (3) A build entry structurally equal to a check entry by ALL THREE of
/// `(severity, code, message)` is NOT appended.
///
/// This is the "nothing check() prints today is printed twice" invariant:
/// both `check()` and `build()` run the same eval front-end, so their
/// diagnostic lists overlap heavily and a naive concatenation would double
/// every shared entry on `check`'s stderr.
#[test]
fn merge_drops_build_entry_structurally_equal_to_a_check_entry() {
    let shared =
        Diagnostic::error("undefined value").with_code(DiagnosticCode::ConstraintIndeterminate);
    let check = vec![shared.clone()];
    let build = vec![shared.clone()];

    let merged = merge_build_diagnostics(&check, &build);

    assert_eq!(
        keys(&merged),
        keys(&check),
        "a build entry equal on all of (severity, code, message) must not be appended"
    );
}

/// (4) Same message, different `severity` → DISTINCT, so it IS appended.
///
/// Severity is part of the key precisely because `check` degrades some
/// conditions to warnings that `build` reports as hard errors; collapsing
/// them would silently drop the more severe report.
#[test]
fn merge_treats_differing_severity_as_distinct() {
    let check = vec![Diagnostic::warning("same message")];
    let build = vec![Diagnostic::error("same message")];

    let merged = merge_build_diagnostics(&check, &build);

    assert_eq!(
        keys(&merged),
        [keys(&check), keys(&build)].concat(),
        "same message at a different severity is a distinct diagnostic and must be appended"
    );
}

/// (5) Same severity + message, different `code` — including `None` vs
/// `Some(_)` — → DISTINCT, so it IS appended.
///
/// `code` is the machine-readable identity downstream consumers match on
/// (γ/#5403 gates on it), so two entries that differ only there are not
/// interchangeable.
#[test]
fn merge_treats_differing_code_as_distinct() {
    // Some(a) vs Some(b)
    let check = vec![
        Diagnostic::error("coded message").with_code(DiagnosticCode::ConstraintIndeterminate)
    ];
    let build =
        vec![Diagnostic::error("coded message").with_code(DiagnosticCode::GdtIllegalModifier)];
    let merged = merge_build_diagnostics(&check, &build);
    assert_eq!(
        keys(&merged),
        [keys(&check), keys(&build)].concat(),
        "same severity+message under a different code must be appended"
    );

    // None vs Some(_) — the asymmetric case
    let check_uncoded = vec![Diagnostic::error("coded message")];
    let merged = merge_build_diagnostics(&check_uncoded, &build);
    assert_eq!(
        keys(&merged),
        [keys(&check_uncoded), keys(&build)].concat(),
        "an uncoded check entry must not absorb a coded build entry with the same message"
    );

    // …and the mirror direction: Some(_) check entry vs None build entry.
    let build_uncoded = vec![Diagnostic::error("coded message")];
    let merged = merge_build_diagnostics(&check, &build_uncoded);
    assert_eq!(
        keys(&merged),
        [keys(&check), keys(&build_uncoded)].concat(),
        "a coded check entry must not absorb an uncoded build entry with the same message"
    );
}

/// (6) Two identical entries WITHIN `build_diags` collapse to exactly ONE
/// appended copy — membership is tested against the ACCUMULATING merged
/// list, not only against check()'s original one.
///
/// Empirically measured on the `mirror(...)` bare-origin fixture task 5748
/// adds, RE-MEASURED at task 5746 (units-length ε) after it gated
/// `plane_yz(0)` at the producer: `reify eval` and `reify build` both emit
/// `failed to compile geometry operation: mirror: expected a Plane value,
/// got undef` TWICE for a single call site — the duplication is unchanged by
/// that move, only the text of the duplicated line is. Deduping against
/// check()'s list alone would print it twice on `check`'s stderr; PRD D2 only
/// requires "at least once", and collapsing matches `check`'s existing output
/// discipline.
///
/// The literal below is a SYNTHETIC stand-in — this test exercises the merge
/// key, not the fixture — so it is left spelling the pre-5746 message rather
/// than chasing the measured one.
#[test]
fn merge_collapses_duplicates_internal_to_build() {
    let dup = Diagnostic::error(
        "failed to compile geometry operation: missing or non-Length argument 'ox' for mirror",
    );
    let check = vec![Diagnostic::warning("unrelated check warning")];
    let build = vec![dup.clone(), dup.clone()];

    let merged = merge_build_diagnostics(&check, &build);

    assert_eq!(
        keys(&merged),
        [keys(&check), keys(&[dup])].concat(),
        "a diagnostic build() emits twice for one call site must appear exactly once"
    );
}

/// (7) An empty `build_diags` returns exactly `check_diags`.
///
/// This is the C2 byte-identity guarantee for every pre-5748 input: the
/// lightweight (non-kernel) arm has no build result, so the merge must be a
/// total no-op there.
#[test]
fn merge_with_empty_build_returns_check_unchanged() {
    let check = vec![
        Diagnostic::warning("w").with_code(DiagnosticCode::ConstraintIndeterminate),
        Diagnostic::error("e"),
    ];

    let merged = merge_build_diagnostics(&check, &[]);

    assert_eq!(
        keys(&merged),
        keys(&check),
        "an empty build list must leave check's diagnostics exactly as they were"
    );
}
