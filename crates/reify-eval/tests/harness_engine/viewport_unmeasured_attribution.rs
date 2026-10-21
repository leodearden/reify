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

const DFM_RULE_DECLS: &str = r#"
import std.process

structure def FDM : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 5USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 0deg
}

structure def OverhangRule : DFMRule {
    param rule_name  : String      = "overhang-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = FDM()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

const NO_DFM_RULE_SOURCE: &str = r#"
structure def Widget {
    param width : Length = 10mm
    param geometry : Solid = box(width, 20mm, 30mm)
}
"#;

/// A Subtracting process carries no angle field, so only the thickness
/// measurement path recognizes this rule.
const THICKNESS_ONLY_SOURCE: &str = r#"
import std.process

structure def Milling : Subtracting {
    param duration          : Time   = 30min
    param cost              : Money  = 10USD
    param tool_access       : Solid  = box(200mm, 200mm, 200mm)
    param min_feature_size  : Length = 2mm
    param achievable_finish : Length = 0.01mm
}

structure def ThicknessRule : DFMRule {
    param rule_name  : String      = "thickness-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = Milling()
    param subject    : Solid       = translate(box(14mm, 14mm, 1mm), 0mm, 0mm, 0.5mm)
}
"#;

/// The rule reachable from both discovery sources: the `OverhangRule`
/// template and the instance at `Part.rule`.
fn both_discovery_sources_source() -> String {
    format!("{DFM_RULE_DECLS}\nstructure def Part {{\n    let rule = OverhangRule()\n}}\n")
}

fn gdt_and_dfm_source() -> String {
    format!("{DFM_RULE_DECLS}\n{GDT_SOURCE}")
}

fn unbound_dfm_subject_source() -> String {
    DFM_RULE_DECLS.replace(
        "param subject    : Solid       = box(50mm, 30mm, 20mm)",
        "param subject    : Solid",
    )
}

fn viewport_attributions(source: &str) -> Vec<String> {
    let tess = viewport(&parse_and_compile_with_stdlib(source));
    attributions(&tess.diagnostics)
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[test]
fn viewport_states_why_a_dfm_rule_is_unmeasured() {
    let tess = viewport(&parse_and_compile_with_stdlib(DFM_RULE_DECLS));
    let subject = tess
        .values
        .get(&ValueCellId::new("OverhangRule", "subject"));
    assert!(
        matches!(subject, Some(Value::GeometryHandle { .. })),
        "premise: the viewport mints the rule's subject: {subject:?}"
    );

    let found = attributions(&tess.diagnostics);
    assert_eq!(found.len(), 1, "{:#?}", tess.diagnostics);
    let message = found[0];
    assert!(
        message.starts_with("DFM rule `overhang-check` indeterminate: "),
        "{message}"
    );
    for token in ["does not measure DFM rules", "reify check"] {
        assert!(message.contains(token), "lacks {token:?}: {message}");
    }
}

/// Counted, not found-first: a second discovery source must not double it.
#[test]
fn one_dfm_rule_reached_from_both_discovery_sources_is_attributed_once() {
    let found = viewport_attributions(&both_discovery_sources_source());
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].contains("overhang-check"), "{}", found[0]);
}

#[test]
fn thickness_only_dfm_rule_is_attributed() {
    let found = viewport_attributions(THICKNESS_ONLY_SOURCE);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].contains("thickness-check"), "{}", found[0]);
}

#[test]
fn gdt_and_dfm_in_one_module_each_get_their_own_attribution() {
    let compiled = parse_and_compile_with_stdlib(&gdt_and_dfm_source());
    let tess = viewport(&compiled);
    let entry = entry_for(&tess, conforms(&compiled));
    let callout = entry.label.clone().unwrap_or_else(|| entry.id.to_string());

    let found = attributions(&tess.diagnostics);
    assert_eq!(found.len(), 2, "{found:#?}");
    let gdt: Vec<_> = found
        .iter()
        .filter(|m| m.contains("geometric conformance") && m.contains(&callout))
        .collect();
    let dfm: Vec<_> = found
        .iter()
        .filter(|m| m.contains("does not measure DFM rules") && m.contains("overhang-check"))
        .collect();
    assert_eq!((gdt.len(), dfm.len()), (1, 1), "{found:#?}");
}

/// Guard: a geometry-bearing module that declares no DFM rule gains nothing.
#[test]
fn module_without_dfm_rules_gains_nothing() {
    let found = viewport_attributions(NO_DFM_RULE_SOURCE);
    assert!(found.is_empty(), "{found:#?}");
}

/// Guard (may be green on arrival): a rule with no geometry subject is
/// measured on no surface, so the viewport claims nothing for it.
#[test]
fn viewport_does_not_blame_itself_for_an_unbound_dfm_subject() {
    let found = viewport_attributions(&unbound_dfm_subject_source());
    assert!(
        !found
            .iter()
            .any(|m| m.contains("does not measure DFM rules")),
        "{found:#?}"
    );
}

/// Guard: `reify check` output is unchanged for a DFM rule.
#[test]
fn check_surface_gains_no_dfm_viewport_attribution() {
    let compiled = parse_and_compile_with_stdlib(DFM_RULE_DECLS);
    let result = engine().check(&compiled);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("does not measure DFM rules")),
        "{:#?}",
        result.diagnostics
    );
}
