//! Task #6667: the GUI viewport surface (`Engine::tessellate_snapshot`, the
//! producer of `GuiState.tessellation_diagnostics`) attributes the measurements
//! it never takes, through ζ's `ConstraintIndeterminate` Info channel.

use reify_compiler::{CompiledConstraint, CompiledModule};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{Diagnostic, DiagnosticCode, Severity, ValueCellId};
use reify_eval::{ConstraintCheckEntry, Engine, SolverPosture, TessellateResult};
use reify_ir::{IndeterminateReason, Satisfaction, TransientReason, Value};
use reify_test_support::{MockGeometryKernel, parse_and_compile_with_stdlib};

const GDT_SOURCE: &str = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    param act : Geometry = box(1mm, 1mm, 1mm)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
}
"#;

const UNBOUND_TOLERANCE_SOURCE: &str = r#"
structure def Probe {
    param tol : Flatness
    param act : Geometry = box(1mm, 1mm, 1mm)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
}
"#;

const UNBOUND_ACTUAL_SOURCE: &str = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    param act : Geometry
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
}
"#;

const SCALAR_SOURCE: &str = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm)
}
"#;

fn engine() -> Engine {
    Engine::new(
        SolverPosture::SolverFree,
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    )
}

/// Evaluate `compiled`, then take the viewport's snapshot of it.
fn viewport(compiled: &CompiledModule) -> TessellateResult {
    let mut engine = engine();
    engine.eval(compiled);
    engine
        .tessellate_snapshot(compiled)
        .expect("tessellate_snapshot must return Some after eval()")
}

/// The Probe's `Conforms` (the constraint binding `tolerance`).
fn conforms(compiled: &CompiledModule) -> &CompiledConstraint {
    compiled
        .templates
        .iter()
        .find(|t| t.name == "Probe")
        .expect("Probe template")
        .constraints
        .iter()
        .find(|c| c.arg_bindings.iter().any(|(n, _)| n == "tolerance"))
        .expect("Conforms binding `tolerance`")
}

fn entry_for<'a>(tess: &'a TessellateResult, c: &CompiledConstraint) -> &'a ConstraintCheckEntry {
    tess.constraint_results
        .iter()
        .find(|e| e.id == c.id)
        .unwrap_or_else(|| panic!("no entry for {}: {:#?}", c.id, tess.constraint_results))
}

/// ζ's channel: Info severity carrying `ConstraintIndeterminate`.
fn attributions(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Info && d.code == Some(DiagnosticCode::ConstraintIndeterminate)
        })
        .map(|d| d.message.as_str())
        .collect()
}

/// The entry's recorded `MeasurementUnavailable` reason, if that is its cause.
fn measurement_unavailable(entry: &ConstraintCheckEntry) -> Option<&IndeterminateReason> {
    entry.indeterminate_reason.as_ref().filter(|r| {
        matches!(
            r,
            IndeterminateReason::Transient(TransientReason::MeasurementUnavailable { .. })
        )
    })
}

/// A geometric Conforms is recorded Indeterminate with ζ's measurement cause,
/// and its Info is rendered from that recorded reason.
#[test]
fn viewport_records_and_states_why_a_geometric_conforms_is_unmeasured() {
    let compiled = parse_and_compile_with_stdlib(GDT_SOURCE);
    let callout = conforms(&compiled);
    let tess = viewport(&compiled);

    let entry = entry_for(&tess, callout);
    assert_eq!(entry.satisfaction, Satisfaction::Indeterminate, "{entry:?}");
    let reason = measurement_unavailable(entry)
        .unwrap_or_else(|| panic!("expected a MeasurementUnavailable reason: {entry:?}"));
    let detail = reason.to_string();
    for token in ["does not measure", "geometric conformance", "reify check"] {
        assert!(detail.contains(token), "reason lacks {token:?}: {detail}");
    }

    let subject = entry.label.clone().unwrap_or_else(|| entry.id.to_string());
    let expected = format!("constraint {subject} indeterminate: {reason}");
    let found = attributions(&tess.diagnostics);
    assert_eq!(found, vec![expected.as_str()], "{:#?}", tess.diagnostics);
}

/// Guard: a Conforms with no `actual` keeps its scalar verdict and gains nothing.
#[test]
fn viewport_leaves_a_scalar_conforms_untouched() {
    let compiled = parse_and_compile_with_stdlib(SCALAR_SOURCE);
    let callout = conforms(&compiled);
    let tess = viewport(&compiled);

    let entry = entry_for(&tess, callout);
    assert_ne!(entry.satisfaction, Satisfaction::Indeterminate, "{entry:?}");
    assert!(
        attributions(&tess.diagnostics).is_empty(),
        "{:#?}",
        tess.diagnostics
    );
}

/// Guard: `reify check` (and every other `Engine::check` surface) is unchanged.
#[test]
fn check_surface_gains_no_viewport_attribution() {
    let compiled = parse_and_compile_with_stdlib(GDT_SOURCE);
    let result = engine().check(&compiled);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("does not measure geometric conformance")),
        "{:#?}",
        result.diagnostics
    );
}

/// ζ's decline: an undefined operand is the model's defect, so the surface is
/// not blamed and no measurement cause is recorded.
fn assert_viewport_declines(tess: &TessellateResult, entry: &ConstraintCheckEntry) {
    assert!(
        !tess
            .diagnostics
            .iter()
            .any(|d| d.message.contains("does not measure geometric conformance")),
        "{:#?}",
        tess.diagnostics
    );
    assert!(measurement_unavailable(entry).is_none(), "{entry:?}");
}

/// The checker's own `undefined inputs` cause stands.
#[test]
fn viewport_does_not_blame_itself_for_an_unbound_tolerance() {
    let compiled = parse_and_compile_with_stdlib(UNBOUND_TOLERANCE_SOURCE);
    let tess = viewport(&compiled);
    let entry = entry_for(&tess, conforms(&compiled));
    assert_eq!(entry.satisfaction, Satisfaction::Indeterminate, "{entry:?}");
    assert_viewport_declines(&tess, entry);
}

/// The Conforms predicate never reads `actual`, so the checker's verdict is
/// whatever it is; the viewport only must not claim the cause.
#[test]
fn viewport_does_not_blame_itself_for_an_unbound_actual() {
    let compiled = parse_and_compile_with_stdlib(UNBOUND_ACTUAL_SOURCE);
    let tess = viewport(&compiled);
    let act = ValueCellId::new("Probe", "act");
    assert!(
        tess.values.get(&act).is_none_or(Value::is_undef),
        "premise: `act` is undefined on the viewport: {:?}",
        tess.values.get(&act)
    );
    assert_viewport_declines(&tess, entry_for(&tess, conforms(&compiled)));
}
