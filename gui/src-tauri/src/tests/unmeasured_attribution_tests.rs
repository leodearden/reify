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

/// A geometric Conforms reads Indeterminate and the panel says why.
#[test]
fn gui_viewport_attributes_an_unmeasured_geometric_conforms() {
    let state = load(GDT_SOURCE, "gdt_viewport");
    let callout = state
        .constraints
        .iter()
        .find(|c| c.node_id.contains("Probe#constraint["))
        .unwrap_or_else(|| panic!("no Probe callout in {:#?}", state.constraints));
    assert_eq!(
        callout.status,
        satisfaction_token(Satisfaction::Indeterminate),
        "{callout:?}"
    );

    let claiming = infos_claiming(&state, callout);
    assert_eq!(claiming.len(), 1, "{:#?}", state.tessellation_diagnostics);
    let message = claiming[0];
    for token in ["does not measure", "geometric conformance"] {
        assert!(message.contains(token), "lacks {token:?}: {message}");
    }
    assert_names_a_terminal_remedy(message);
}
