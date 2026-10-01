//! Task 5418 (DIC δ) — the recorded `IndeterminateReason` reaches the reported
//! outcome.
//!
//! PRD `docs/prds/v0_6/declared-intent-consumption-accounting.md` §4.3, R1:
//! every Indeterminate outcome carries the reason its producer recorded, so a
//! report can render it instead of a guess. These tests drive the kernel-less
//! `Engine::check` path — the one `reify check` takes — and read the reason off
//! each `ConstraintCheckEntry`.
//!
//! The sources are byte-mirrors of the committed fixtures so the tests track
//! the same user-observable signal the PRD measured.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{Diagnostic, DiagnosticCode, Severity, ValueCellId};
use reify_eval::{CheckResult, ConstraintCheckEntry, Engine};
use reify_ir::{IndeterminateReason, Satisfaction, TransientReason};
use reify_test_support::compile_source_with_stdlib;

/// Byte-mirror of `docs/prds/v0_6/fixtures/dic_inert_connect.ri`.
const DIC_INERT_CONNECT: &str = r#"module dic_inert_connect

// INV-SF-4 probe: ad-hoc @-selector connect generates a frame_align constraint
// that is INDETERMINATE in every possible run (structurally inert). Baseline
// 2026-07-24: non-strict check reports "No constraints violated (1
// indeterminate)." exit 0; strict detail MISATTRIBUTES the reason as "inputs
// undefined" while the recorded reason is "operator undefined for these
// operand kinds".

trait T {
    param d : Length
}

structure def DicInertRig {
    let shape = cylinder(10mm, 20mm)
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a @ face("top") -> b @ face("bottom")
}
"#;

/// `crates/reify-cli/tests/fixtures/bracket_indeterminate.ri` with a module
/// declaration: the `auto` tolerance is never resolved, so the constraint that
/// reads it is Indeterminate for want of an input.
const BRACKET_INDETERMINATE: &str = r#"module dic_bracket_indeterminate

structure Bracket {
    param width: Length = 80mm
    param height: Length = 100mm
    param thickness: Length = 5mm
    param tolerance: Length = auto

    constraint thickness > 2mm
    constraint tolerance > 0.1mm

    let body = box(width, height, thickness)
}
"#;

fn check_kernel_less(source: &str) -> CheckResult {
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "fixture must be compile-clean; got {compile_errors:?}"
    );
    Engine::new(Box::new(SimpleConstraintChecker), None).check(&compiled)
}

fn display_name(entry: &ConstraintCheckEntry) -> String {
    entry.label.clone().unwrap_or_else(|| entry.id.to_string())
}

fn entry_named<'a>(result: &'a CheckResult, name: &str) -> &'a ConstraintCheckEntry {
    result
        .constraint_results
        .iter()
        .find(|entry| display_name(entry) == name)
        .unwrap_or_else(|| {
            let names: Vec<String> = result.constraint_results.iter().map(display_name).collect();
            panic!("no constraint entry named {name}; entries: {names:?}")
        })
}

fn transient(reason: TransientReason) -> Option<IndeterminateReason> {
    Some(IndeterminateReason::Transient(reason))
}

#[test]
fn inert_connect_records_the_operator_undefined_reason() {
    let result = check_kernel_less(DIC_INERT_CONNECT);

    let frame_align = entry_named(&result, "frame_align_a_b");
    assert_eq!(frame_align.satisfaction, Satisfaction::Indeterminate);
    assert_eq!(
        frame_align.indeterminate_reason,
        transient(TransientReason::OperatorUndefinedForKinds { kinds: vec![] })
    );

    let connect_compat = entry_named(&result, "connect_compat_a_b");
    assert_eq!(connect_compat.satisfaction, Satisfaction::Satisfied);
    assert_eq!(connect_compat.indeterminate_reason, None);
}

#[test]
fn unresolved_auto_records_the_undefined_input_cell() {
    let result = check_kernel_less(BRACKET_INDETERMINATE);

    let tolerance = entry_named(&result, "Bracket#constraint[1]");
    assert_eq!(tolerance.satisfaction, Satisfaction::Indeterminate);
    assert_eq!(
        tolerance.indeterminate_reason,
        transient(TransientReason::UndefInputs {
            cells: vec![ValueCellId::new("Bracket", "tolerance")],
        })
    );
}

/// The checker's warning and the carrier are one source rendered twice: the
/// emitted diagnostic is exactly the recorded reason behind the
/// constraint's reported name.
#[test]
fn indeterminate_diagnostic_renders_the_recorded_reason() {
    for source in [DIC_INERT_CONNECT, BRACKET_INDETERMINATE] {
        let result = check_kernel_less(source);
        let indeterminate: Vec<&ConstraintCheckEntry> = result
            .constraint_results
            .iter()
            .filter(|entry| entry.satisfaction == Satisfaction::Indeterminate)
            .collect();
        assert!(!indeterminate.is_empty(), "anti-vacuity: no Indeterminate entry");

        for entry in indeterminate {
            let reason = entry
                .indeterminate_reason
                .as_ref()
                .unwrap_or_else(|| panic!("{} carries no reason", display_name(entry)));
            let expected = format!("constraint {} indeterminate: {}", display_name(entry), reason);
            let rendered: Vec<&str> = result
                .diagnostics
                .iter()
                .filter(|d| d.code == Some(DiagnosticCode::ConstraintIndeterminate))
                .map(|d| d.message.as_str())
                .collect();
            assert!(
                rendered.contains(&expected.as_str()),
                "expected diagnostic {expected:?}; ConstraintIndeterminate diagnostics: {rendered:?}"
            );
        }
    }
}
