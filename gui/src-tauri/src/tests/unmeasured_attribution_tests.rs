//! Task #6667: the GUI viewport attributes the GD&T and DFM measurements it
//! never takes, on the panel's own stream (`GuiState.tessellation_diagnostics`),
//! through ζ's `ConstraintIndeterminate` Info channel.

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_ir::Satisfaction;

use crate::engine::{EngineSession, satisfaction_token};
use crate::types::{ConstraintData, GuiState};

const GDT_SOURCE: &str = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    param act : Geometry = box(1mm, 1mm, 1mm)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
}
"#;

const DFM_SOURCE: &str = r#"
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

/// The terminal remedy reachable on this binary: `reify check` when OCCT is
/// linked, the kernel itself otherwise.
#[cfg(feature = "gui")]
fn acceptable_remedy_tokens() -> &'static [&'static str] {
    if crate::kernel_status::current_kernel_status().available {
        &["reify check"]
    } else {
        &["OCCT"]
    }
}

/// Without the `gui` feature there is no `OCCT_AVAILABLE` to discriminate on.
#[cfg(not(feature = "gui"))]
fn acceptable_remedy_tokens() -> &'static [&'static str] {
    &["reify check", "OCCT"]
}

fn assert_names_a_terminal_remedy(message: &str) {
    let accepted = acceptable_remedy_tokens();
    assert!(
        accepted.iter().any(|t| message.contains(t)),
        "expected one of {accepted:?} in: {message}"
    );
}

fn load(source: &str, module_name: &str) -> GuiState {
    let mut session = EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker));
    session
        .load_from_source(source, module_name)
        .unwrap_or_else(|e| panic!("{module_name} must load: {e:?}"))
}

/// The panel's Info diagnostics.
fn infos(state: &GuiState) -> Vec<&str> {
    state
        .tessellation_diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info.as_wire_str())
        .map(|d| d.message.as_str())
        .collect()
}

/// Info diagnostics naming `c` by label, else node id.
fn infos_claiming<'a>(state: &'a GuiState, c: &ConstraintData) -> Vec<&'a str> {
    let subject = c.label.as_deref().unwrap_or(&c.node_id);
    infos(state)
        .into_iter()
        .filter(|m| m.split_whitespace().any(|word| word == subject))
        .collect()
}

fn probe_callout(state: &GuiState) -> &ConstraintData {
    state
        .constraints
        .iter()
        .find(|c| c.node_id.contains("Probe#constraint["))
        .unwrap_or_else(|| panic!("no Probe callout in {:#?}", state.constraints))
}

/// The viewport Info stream says why a geometric Conforms is unmeasured. The
/// panel status is `Engine::check`'s, so only the Info is under test here.
#[test]
fn gui_viewport_attributes_an_unmeasured_geometric_conforms() {
    let state = load(GDT_SOURCE, "gdt_viewport");
    let callout = probe_callout(&state);

    let claiming = infos_claiming(&state, callout);
    assert_eq!(claiming.len(), 1, "{:#?}", state.tessellation_diagnostics);
    let message = claiming[0];
    for token in ["does not measure", "geometric conformance"] {
        assert!(message.contains(token), "lacks {token:?}: {message}");
    }
    assert_names_a_terminal_remedy(message);
}

/// A declared DFM rule is no longer silent on the panel: one Info says why.
#[test]
fn gui_viewport_attributes_an_unmeasured_dfm_rule() {
    let state = load(DFM_SOURCE, "dfm_viewport");
    let naming: Vec<&str> = infos(&state)
        .into_iter()
        .filter(|m| m.contains("overhang-check"))
        .collect();
    assert_eq!(naming.len(), 1, "{:#?}", state.tessellation_diagnostics);
    let message = naming[0];
    for token in ["does not measure", "DFM"] {
        assert!(message.contains(token), "lacks {token:?}: {message}");
    }
    assert_names_a_terminal_remedy(message);
}

/// The panel's verdict (`Engine::check`'s) and the viewport's Info must not
/// contradict each other: a recheck or reload that runs no build has no live
/// handle to measure with, so the panel stays Indeterminate beside the Info.
#[test]
fn gui_panel_verdict_and_viewport_info_agree_across_a_recheck_and_a_reload() {
    let mut session = EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker));
    let loaded = session
        .load_from_source(GDT_SOURCE, "gdt_viewport")
        .expect("gdt_viewport must load");
    session.recheck_for_test();
    let rechecked = session.build_gui_state().expect("rechecked state");
    let reloaded = session
        .update_source("gdt_viewport.ri", GDT_SOURCE)
        .expect("reload of the same source");

    for (surface, state) in [
        ("load", &loaded),
        ("recheck", &rechecked),
        ("reload", &reloaded),
    ] {
        let callout = probe_callout(state);
        assert_eq!(
            callout.status,
            satisfaction_token(Satisfaction::Indeterminate),
            "{surface}: {callout:?}"
        );
        assert_eq!(
            infos_claiming(state, callout).len(),
            1,
            "{surface}: {:#?}",
            state.tessellation_diagnostics
        );
    }
}
