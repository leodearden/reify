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
    BinOp, ExportFormat, GeometryHandleId, IndeterminateReason, RefusedOperation, Satisfaction,
    TransientReason, Value,
};
use reify_test_support::{MockGeometryKernel, compile_source_with_stdlib};

/// The `reify check` fixture the CLI harness drives: an ad-hoc @-selector
/// connect whose frame_align constraint is Indeterminate.
const DIC_INERT_CONNECT: &str =
    include_str!("../../../reify-cli/tests/fixtures/dic_inert_connect.ri");

/// The `reify check` fixture the CLI harness drives: the tolerance divides by
/// a zero param and is Undef, so the constraint that reads it is Indeterminate
/// for want of an input.
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

/// Two pure operations that refuse defined plain-data operands: a Real
/// division by zero and the builtin `sqrt` of a negative Real.
const PURE_REFUSALS: &str = r#"
structure def Root {
    param x : Real = 0.0
    param y : Real = -1.0
    constraint 1.0 / x > 0.0
    constraint sqrt(y) > 0.0
}
"#;

/// A constraint over an inline geometry query, which no kernel-less check can
/// measure.
const INLINE_VOLUME_QUERY: &str = r#"
structure def Part {
    param w : Length = 10mm
    constraint volume(box(w, w, w)) > 1mm^3
}
"#;

/// A well-formed distance between two inline geometry constructors: no stdlib
/// family claims `box` or `sphere`, so their kernel-less Undef is not a refusal.
const INLINE_GEOMETRY_DISTANCE: &str = r#"
structure def Part {
    param w : Length = 10mm
    constraint distance(box(w, w, w), sphere(w)) >= 0mm
}
"#;

/// The same Real division by zero as `PURE_REFUSALS`, behind a user function:
/// the boundary a refusal is not traced across.
const USER_FUNCTION_REFUSAL: &str = r#"
fn inv(v: Real) -> Real { 1.0 / v }

structure def Root {
    param x : Real = 0.0
    constraint inv(x) > 0.0
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
    Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(SimpleConstraintChecker), None).check(&compiled)
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
fn undef_input_records_the_undefined_input_cell() {
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
    let mut engine = Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(SimpleConstraintChecker), Some(Box::new(kernel)));
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

/// A pure operation refusing defined plain-data operands fails on every run,
/// so the checker records the refusing operation and reports it as an error.
#[test]
fn pure_refusals_are_recorded_and_reported_as_errors() {
    let result = check_kernel_less(PURE_REFUSALS);

    for (name, operation, operands) in [
        (
            "Root#constraint[0]",
            RefusedOperation::Binary(BinOp::Div),
            vec!["Real", "Real"],
        ),
        (
            "Root#constraint[1]",
            RefusedOperation::Builtin("sqrt".to_string()),
            vec!["Real"],
        ),
    ] {
        let entry = entry_named(&result, name);
        assert_eq!(entry.satisfaction, Satisfaction::Indeterminate, "{name}");
        let reason = TransientReason::OperationRefused {
            operation,
            operands: operands.into_iter().map(String::from).collect(),
        };
        assert_eq!(
            entry.indeterminate_reason,
            transient(reason.clone()),
            "{name}"
        );

        let expected = format!("constraint {name} indeterminate: {reason}");
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|d| {
                d.code == Some(DiagnosticCode::ConstraintIndeterminate) && d.message == expected
            })
            .unwrap_or_else(|| {
                panic!(
                    "expected diagnostic {expected:?}; got {:#?}",
                    result.diagnostics
                )
            });
        assert_eq!(diagnostic.severity, Severity::Error, "{name}");
    }
}

/// An Undef not proven to be a pure refusal stays a tolerated warning. Only
/// that is asserted, not today's verdict or wording, so a later fix that
/// decides one of these constraints stays green.
#[test]
fn unproven_undefs_stay_tolerated() {
    for (source_name, source) in [
        ("dic_inert_connect", DIC_INERT_CONNECT),
        ("volume_gated", VOLUME_GATED),
        ("inline_volume_query", INLINE_VOLUME_QUERY),
        ("inline_geometry_distance", INLINE_GEOMETRY_DISTANCE),
        ("user_function_refusal", USER_FUNCTION_REFUSAL),
    ] {
        let result = check_kernel_less(source);
        assert!(
            !result.constraint_results.is_empty(),
            "anti-vacuity: {source_name} has no constraint entry"
        );

        let loud: Vec<&Diagnostic> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code == Some(DiagnosticCode::ConstraintIndeterminate)
                    && d.severity == Severity::Error
            })
            .collect();
        assert!(
            loud.is_empty(),
            "{source_name}: unproven Undef reported as an error: {loud:#?}"
        );

        for entry in &result.constraint_results {
            assert!(
                !entry
                    .indeterminate_reason
                    .as_ref()
                    .is_some_and(IndeterminateReason::is_unexpected),
                "{source_name}: {} recorded an unexpected reason: {:?}",
                display_name(entry),
                entry.indeterminate_reason
            );
        }
    }
}
