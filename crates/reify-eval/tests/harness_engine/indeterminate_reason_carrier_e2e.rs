//! Task 5418 (DIC δ) — the recorded `IndeterminateReason` reaches the reported
//! outcome.
//!
//! PRD `docs/prds/v0_6/declared-intent-consumption-accounting.md` §4.3, R1:
//! every Indeterminate outcome carries the reason its producer recorded, so a
//! report can render it instead of a guess. These tests drive the kernel-less
//! `Engine::check` path — the one `reify check` takes — and read the reason off
//! each `ConstraintCheckEntry`.
//!
//! The sources are the committed `reify check` fixtures themselves, loaded
//! from the CLI harness, so this test and the CLI test exercise one program.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{Diagnostic, DiagnosticCode, Severity, ValueCellId};
use reify_eval::{BuildScheduler, CheckResult, ConstraintCheckEntry, Engine};
use reify_ir::{
    ExportFormat, GeometryHandleId, IndeterminateReason, Satisfaction, TransientReason, Value,
};
use reify_test_support::{MockGeometryKernel, compile_source_with_stdlib};

/// The `reify check` fixture the CLI harness drives: an ad-hoc @-selector
/// connect whose frame_align constraint is Indeterminate.
const DIC_INERT_CONNECT: &str =
    include_str!("../../../reify-cli/tests/fixtures/dic_inert_connect.ri");

/// The `reify check` fixture the CLI harness drives: the `auto` tolerance is
/// never resolved, so the constraint that reads it is Indeterminate for want
/// of an input.
const BRACKET_INDETERMINATE: &str =
    include_str!("../../../reify-cli/tests/fixtures/bracket_indeterminate.ri");

/// A RepresentationWithin on a surface that never measured: its Indeterminate
/// is decided by the engine, not the checker. Stdlib-free (`mm` is built in).
const REPRESENTATION_WITHIN_UNMEASURED: &str = r#"
structure MyGeom {
    param x : Real = 1.0
}

structure Checker {
    param subject : MyGeom = MyGeom()
    constraint RepresentationWithin(subject, 1mm)
}
"#;

/// A constraint that only a realized geometry can decide: Indeterminate on the
/// kernel-less check surface, definite once `build()` has realized `part`.
const VOLUME_GATED: &str = r#"
structure Widget {
    param part : Solid = box(10mm, 10mm, 10mm)
    constraint volume(part) <= 2000mm^3
}
"#;

fn check_kernel_less(source: &str) -> CheckResult {
    check_compiled(compile_source_with_stdlib(source))
}

fn check_compiled(compiled: reify_compiler::CompiledModule) -> CheckResult {
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

/// R1 sweep: every Indeterminate a first-party producer decides — the checker
/// (both reason classes) and the RepresentationWithin peel — carries a reason.
#[test]
fn every_first_party_indeterminate_records_a_reason() {
    let results = [
        ("dic_inert_connect", check_kernel_less(DIC_INERT_CONNECT)),
        ("bracket_indeterminate", check_kernel_less(BRACKET_INDETERMINATE)),
        (
            "representation_within_unmeasured",
            check_compiled(reify_test_support::parse_and_compile(
                REPRESENTATION_WITHIN_UNMEASURED,
            )),
        ),
    ];

    for (source, result) in &results {
        let indeterminate: Vec<&ConstraintCheckEntry> = result
            .constraint_results
            .iter()
            .filter(|entry| entry.satisfaction == Satisfaction::Indeterminate)
            .collect();
        assert!(!indeterminate.is_empty(), "anti-vacuity: {source} has no Indeterminate entry");
        for entry in indeterminate {
            assert!(
                entry.indeterminate_reason.is_some(),
                "{source}: {} is Indeterminate with no recorded reason",
                display_name(entry)
            );
        }
    }
}

/// `Engine::build` re-checks every Indeterminate entry once geometry has
/// realized; an entry it upgrades must drop the reason the first check
/// recorded, or the report would explain a verdict that no longer holds.
#[test]
fn build_recheck_upgrade_clears_the_recorded_reason() {
    let checked = check_kernel_less(VOLUME_GATED);
    let [before] = checked.constraint_results.as_slice() else {
        panic!(
            "expected one constraint entry; got {:?}",
            checked.constraint_results
        );
    };
    assert_eq!(before.satisfaction, Satisfaction::Indeterminate);
    assert!(
        before.indeterminate_reason.is_some(),
        "anti-vacuity: the first check must record a reason for the re-check to clear"
    );

    // 1000 mm³ for whichever handle `part` realizes to.
    let kernel = (1..=4u64).fold(MockGeometryKernel::new(), |kernel, handle| {
        kernel.with_volume_result(GeometryHandleId(handle), Value::Real(1e-6))
    });
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(Box::new(kernel)));
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    let built = engine.build(
        &compile_source_with_stdlib(VOLUME_GATED),
        ExportFormat::Step,
    );

    let after = built
        .constraint_results
        .iter()
        .find(|entry| entry.id == before.id)
        .unwrap_or_else(|| {
            panic!(
                "build lost {}; got {:?}",
                before.id, built.constraint_results
            )
        });
    assert_eq!(after.satisfaction, Satisfaction::Satisfied);
    assert_eq!(after.indeterminate_reason, None);
}
