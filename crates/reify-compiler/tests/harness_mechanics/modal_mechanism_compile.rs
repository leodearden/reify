//! Step-1 (RED) / Step-2 (GREEN) compile-typing test for
//! `mechanism_modal_analysis(mechanism, options) -> ModalResult` (task 4271).
//!
//! Observable signal: a `.ri` call to `mechanism_modal_analysis(mech, ModalOptions())`
//! must compile without errors and type as `StructureRef("ModalResult")`.
//!
//! Mirrors the Probe type-check pattern in
//! `crates/reify-compiler/tests/dynamics_stdlib_compile.rs`
//! (`point_mass_and_mass_properties_ctors_type_as_mass_properties_struct_ref`,
//! ~line 440) — embeds a `structure def Probe` whose `let` cells are
//! inspected for their resolved `cell_type`.
//!
//! RED until step-2 adds `modal_mechanism_fns.ri` + stdlib_loader registration.

use reify_core::*;
use reify_test_support::compile_source_with_stdlib;

/// `mechanism_modal_analysis(mech, ModalOptions())` must compile without errors
/// and the result cell must type as `Type::StructureRef("ModalResult")`.
///
/// Uses `mechanism()` (a JOINT_TYPED_FN_NAMES builtin → `StructureRef("Mechanism")`)
/// and passes it to `mechanism_modal_analysis` with a default `ModalOptions()`.
/// Mirrors the `inverse_dynamics(mechanism, trajectory)` Mechanism-param precedent in
/// dynamics.ri:257 — the function's `mechanism : Mechanism` parameter accepts the
/// `StructureRef("Mechanism")` value produced by `mechanism()`.
///
/// RED: fails with a compile error until step-2 registers
/// `std.modal.mechanism.fns` (containing the `@optimized("modal::mechanism_modal")`
/// function declaration) in stdlib_loader.rs.
#[test]
fn mechanism_modal_analysis_call_types_as_modal_result() {
    let source = r#"
structure def Probe {
    let mech   = mechanism()
    let result = mechanism_modal_analysis(mech, ModalOptions())
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "mechanism_modal_analysis Probe should compile without errors; got: {:?}",
        errors
    );

    let probe = compiled
        .templates
        .iter()
        .find(|t| t.name == "Probe")
        .expect("Probe template should be present in compiled module");

    let result_cell = probe
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "result")
        .expect("Probe.result cell should exist");

    assert_eq!(
        result_cell.cell_type,
        Type::StructureRef("ModalResult".to_string()),
        "mechanism_modal_analysis(mech, ModalOptions()) should type as \
         StructureRef(\"ModalResult\"), not {:?}",
        result_cell.cell_type
    );
}
