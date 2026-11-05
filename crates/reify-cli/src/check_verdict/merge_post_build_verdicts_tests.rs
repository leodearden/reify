//! Prefix-collision coverage for the INWARD leg — the sibling of
//! `drop_falsified_indeterminate_diagnostics_tests` (task 5748, esc-5748-4).
//!
//! `merge_post_build_verdicts`'s retain used a bare
//! `d.message.contains(label_or_id)` while the outward leg was already anchored.
//! Both now share [`is_falsified_indeterminacy`] (over [`indeterminacy_subject`]
//! and [`indeterminacy_subject_in`]); these tests pin that the inward leg really
//! is anchored, so the two matchers cannot drift apart again.

use super::merge_post_build_verdicts;
use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode, Severity, ValueCellId};
use reify_eval::{BuildResult, CheckResult, ConstraintCheckEntry};
use reify_ir::{IndeterminateReason, Satisfaction, TransientReason};

fn key(d: &Diagnostic) -> (Severity, Option<DiagnosticCode>, String) {
    (d.severity, d.code, d.message.clone())
}

fn keys(ds: &[Diagnostic]) -> Vec<(Severity, Option<DiagnosticCode>, String)> {
    ds.iter().map(key).collect()
}

fn entry(
    entity: &str,
    index: u32,
    label: Option<&str>,
    satisfaction: Satisfaction,
) -> ConstraintCheckEntry {
    ConstraintCheckEntry {
        id: ConstraintNodeId::new(entity, index),
        label: label.map(str::to_string),
        satisfaction,
        indeterminate_reason: None,
    }
}

/// The exact shape `reify_constraints`' checker emits.
fn indeterminate_warning(needle: &str, undefined: &str) -> Diagnostic {
    Diagnostic::warning(format!(
        "constraint {} indeterminate: undefined inputs: {}",
        needle, undefined
    ))
    .with_code(DiagnosticCode::ConstraintIndeterminate)
}

fn check_result(
    constraint_results: Vec<ConstraintCheckEntry>,
    diagnostics: Vec<Diagnostic>,
) -> CheckResult {
    CheckResult {
        values: Default::default(),
        constraint_results,
        diagnostics,
        resolved_params: Default::default(),
        structured_detail: Vec::new(),
    }
}

fn build_result(constraint_results: Vec<ConstraintCheckEntry>) -> BuildResult {
    BuildResult {
        values: Default::default(),
        constraint_results,
        geometry_output: None,
        diagnostics: Vec::new(),
        resolved_params: Default::default(),
    }
}

fn undefined_input_reason() -> IndeterminateReason {
    IndeterminateReason::Transient(TransientReason::UndefInputs {
        cells: vec![ValueCellId::new("BoltFlange", "moi_principal")],
    })
}

/// An adopted definite verdict takes no stale reason with it.
#[test]
fn upgrade_clears_the_recorded_indeterminate_reason() {
    let mut indeterminate = entry("BoltFlange", 1, None, Satisfaction::Indeterminate);
    indeterminate.indeterminate_reason = Some(undefined_input_reason());
    let mut result = check_result(vec![indeterminate], Vec::new());
    let build = build_result(vec![entry("BoltFlange", 1, None, Satisfaction::Satisfied)]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(result.constraint_results[0].satisfaction, Satisfaction::Satisfied);
    assert_eq!(result.constraint_results[0].indeterminate_reason, None);
}

/// An Indeterminate build verdict overwrites nothing, reason included.
#[test]
fn indeterminate_build_verdict_keeps_the_recorded_reason() {
    let mut indeterminate = entry("BoltFlange", 1, None, Satisfaction::Indeterminate);
    indeterminate.indeterminate_reason = Some(undefined_input_reason());
    let mut result = check_result(vec![indeterminate], Vec::new());
    let build = build_result(vec![entry("BoltFlange", 1, None, Satisfaction::Indeterminate)]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(result.constraint_results[0].satisfaction, Satisfaction::Indeterminate);
    assert_eq!(
        result.constraint_results[0].indeterminate_reason,
        Some(undefined_input_reason())
    );
}

/// Baseline: the upgraded constraint's own now-false warning IS dropped.
/// The anchoring fix must not weaken the behaviour the helper exists for.
#[test]
fn drops_the_upgraded_constraints_own_warning() {
    let mut result = check_result(
        vec![entry("BoltFlange", 1, None, Satisfaction::Indeterminate)],
        vec![indeterminate_warning(
            "BoltFlange#constraint[1]",
            "BoltFlange.moi_principal",
        )],
    );
    let build = build_result(vec![entry("BoltFlange", 1, None, Satisfaction::Satisfied)]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "the build verdict must be adopted onto the indeterminate entry"
    );
    assert!(
        result.diagnostics.is_empty(),
        "the upgraded constraint's own indeterminacy warning is now false \
         and must be dropped, got {:?}",
        keys(&result.diagnostics)
    );
}

/// UPGRADE-ONLY, the load-bearing half of design decision #7 (the
/// esc-5748-1 amendment).  A definite `check()` verdict must NEVER be
/// regressed by build()'s pre-`tessellate_realizations` copy — that copy is
/// genuinely stale on the RepresentationWithin/GD&T axis, and consulting it
/// at all is only safe because this guard exists.
///
/// The stakes are an exit code, not just a printed line: `check` Satisfied
/// overwritten by build Violated makes `report_constraint_results` return a
/// violated outcome and `finish_check` return FAILURE, so a file that
/// legitimately exits 0 would start exiting 1.
#[test]
fn never_regresses_a_definite_check_verdict() {
    let mut result = check_result(
        vec![entry("Sphere", 0, None, Satisfaction::Satisfied)],
        Vec::new(),
    );
    // build()'s copy is computed before tessellation, so its Violated here
    // is exactly the stale verdict the upgrade-only rule must ignore.
    let build = build_result(vec![entry("Sphere", 0, None, Satisfaction::Violated)]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "check() is the source of record: build() may fill holes it left, \
         never overwrite a verdict it already reached"
    );
}

/// The upgrade target is DEFINITE, not "Satisfied".  Pins that a build
/// `Violated` is adopted too — an implementation that only ever upgraded to
/// `Satisfied` would pass every other test in this module while silently
/// swallowing real violations (and holding the exit code at 0).
#[test]
fn adopts_a_definite_violated_verdict() {
    let mut result = check_result(
        vec![entry("Foo", 0, None, Satisfaction::Indeterminate)],
        vec![indeterminate_warning("Foo#constraint[0]", "Foo.centroid")],
    );
    let build = build_result(vec![entry("Foo", 0, None, Satisfaction::Violated)]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "a definite Violated resolves the indeterminacy just as a Satisfied does"
    );
    assert!(
        result.diagnostics.is_empty(),
        "the indeterminacy warning is false either way, got {:?}",
        keys(&result.diagnostics)
    );
}

/// An `Indeterminate` build verdict never overwrites, and the still-true
/// warning is KEPT — the entry remains unexplained-for-a-reason.
#[test]
fn indeterminate_build_verdict_never_overwrites() {
    let warning = indeterminate_warning("Foo#constraint[0]", "Foo.x");
    let mut result = check_result(
        vec![entry("Foo", 0, None, Satisfaction::Indeterminate)],
        vec![warning.clone()],
    );
    let build = build_result(vec![entry("Foo", 0, None, Satisfaction::Indeterminate)]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Indeterminate
    );
    assert_eq!(
        keys(&result.diagnostics),
        keys(&[warning]),
        "nothing was upgraded, so the explanation must survive"
    );
}

/// Matched by `id`, NEVER by position.  The two result vectors are built by
/// independent calls, so their orders need not agree; the single integration
/// test cannot see a positional bug because its fixture happens to align.
///
/// The build list is deliberately REVERSED here, and the two verdicts are
/// distinguishable, so a positional implementation produces the exact
/// inverse assignment rather than an incidentally-correct one.
#[test]
fn entries_are_matched_by_id_not_position() {
    let mut result = check_result(
        vec![
            entry("Foo", 0, None, Satisfaction::Indeterminate),
            entry("Bar", 0, None, Satisfaction::Indeterminate),
        ],
        Vec::new(),
    );
    let build = build_result(vec![
        entry("Bar", 0, None, Satisfaction::Satisfied),
        entry("Foo", 0, None, Satisfaction::Violated),
    ]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "Foo#constraint[0] must take BAR-position build entry's id-match \
         (Violated), not the positionally-aligned Satisfied"
    );
    assert_eq!(
        result.constraint_results[1].satisfaction,
        Satisfaction::Satisfied,
        "Bar#constraint[0] must take its id-match (Satisfied), not Violated"
    );
}

/// An id present ONLY in build()'s list is not inserted.  `check()` decides
/// which constraints exist; build() can only speak to ones it already named.
#[test]
fn an_id_only_in_the_build_list_is_not_inserted() {
    let mut result = check_result(
        vec![entry("Foo", 0, None, Satisfaction::Indeterminate)],
        Vec::new(),
    );
    let build = build_result(vec![
        entry("Foo", 0, None, Satisfaction::Satisfied),
        entry("Ghost", 7, None, Satisfaction::Violated),
    ]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(
        result.constraint_results.len(),
        1,
        "build() must not add constraint rows check() never reported, got {:?}",
        result
            .constraint_results
            .iter()
            .map(|e| e.id.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied
    );
}

/// Failure mode A — RAW-ID PREFIX COLLISION.
///
/// `Foo#constraint[1]` upgrades; `Foo#constraint[10]` stays indeterminate.
/// A bare `contains("Foo#constraint[1]")` matches `[10]`'s message and
/// silently deletes it, leaving stdout printing `INDETERMINATE
/// Foo#constraint[10]` with NO stderr explanation. Anchoring makes that
/// impossible: `constraint Foo#constraint[1] indeterminate` is not a
/// substring of `constraint Foo#constraint[10] indeterminate`.
#[test]
fn anchored_matcher_survives_an_id_prefix_collision() {
    let surviving = indeterminate_warning("Foo#constraint[10]", "Foo.x");
    let mut result = check_result(
        vec![
            entry("Foo", 1, None, Satisfaction::Indeterminate),
            entry("Foo", 10, None, Satisfaction::Indeterminate),
        ],
        vec![
            indeterminate_warning("Foo#constraint[1]", "Foo.centroid"),
            surviving.clone(),
        ],
    );
    // Only [1] resolves under build(); [10] is genuinely still unknown.
    let build = build_result(vec![
        entry("Foo", 1, None, Satisfaction::Satisfied),
        entry("Foo", 10, None, Satisfaction::Indeterminate),
    ]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(
        keys(&result.diagnostics),
        keys(&[surviving]),
        "upgrading Foo#constraint[1] must drop ONLY its own warning — \
         Foo#constraint[10] is still indeterminate and its explanation is \
         the only thing telling the user why"
    );
}

/// Failure mode B — SHORT-LABEL COLLISION (the likelier one).
///
/// The needle prefers the label, so an upgraded constraint labeled `w`
/// would, unanchored, match essentially EVERY other message (`… undefined
/// inputs: Bracket.width` contains `w`), wiping the whole explanation set.
#[test]
fn anchored_matcher_survives_a_short_label_collision() {
    let surviving = indeterminate_warning("depth", "Bracket.width");
    let mut result = check_result(
        vec![
            entry("Bracket", 0, Some("w"), Satisfaction::Indeterminate),
            entry("Bracket", 1, Some("depth"), Satisfaction::Indeterminate),
        ],
        vec![
            indeterminate_warning("w", "Bracket.centroid"),
            surviving.clone(),
        ],
    );
    let build = build_result(vec![
        entry("Bracket", 0, Some("w"), Satisfaction::Satisfied),
        entry("Bracket", 1, Some("depth"), Satisfaction::Indeterminate),
    ]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(
        keys(&result.diagnostics),
        keys(&[surviving]),
        "a one-character label must not wipe every other constraint's \
         indeterminacy explanation — the matcher must be anchored"
    );
}

/// A non-`ConstraintIndeterminate` diagnostic naming the upgraded
/// constraint survives: the verdict falsifies the INDETERMINACY claim and
/// nothing else. Mirrors the outward leg's same-named contract.
#[test]
fn leaves_other_codes_about_the_same_constraint_alone() {
    let other = Diagnostic::error(
        "failed to compile geometry operation: missing or non-Length \
         argument 'ox' for mirror (constraint Foo#constraint[0])",
    );
    let mut result = check_result(
        vec![entry("Foo", 0, None, Satisfaction::Indeterminate)],
        vec![other.clone()],
    );
    let build = build_result(vec![entry("Foo", 0, None, Satisfaction::Satisfied)]);

    merge_post_build_verdicts(&mut result, Some(&build));

    assert_eq!(
        keys(&result.diagnostics),
        keys(&[other]),
        "only ConstraintIndeterminate entries are in scope for the retain"
    );
}

/// `None` build result → total no-op (the lightweight arm). Pins the
/// byte-identical-for-pre-5748-inputs guarantee.
#[test]
fn none_build_result_is_a_total_no_op() {
    let warning = indeterminate_warning("Foo#constraint[0]", "Foo.x");
    let mut result = check_result(
        vec![entry("Foo", 0, None, Satisfaction::Indeterminate)],
        vec![warning.clone()],
    );

    merge_post_build_verdicts(&mut result, None);

    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Indeterminate
    );
    assert_eq!(keys(&result.diagnostics), keys(&[warning]));
}
