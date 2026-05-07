//! End-to-end engine-level integration tests for task 2874 — exercises the
//! production-wired tolerance subsystem: dispatcher emission of import-promise
//! + zero-promise diagnostics on `build()`, `RealizationCache` population /
//! short-circuit keyed on demanded tolerance, and `per_stage_tolerance_for_plan`
//! consumption from the realization loop.
//!
//! Imports use the established test fixture surface
//! (`reify_test_support::{make_engine, step_input_template, step_output_template,
//! my_design_template, manufacturing_purpose}` + `CompiledModuleBuilder`).
//! Per-step tests are added by the subsequent TDD steps.

#[allow(unused_imports)]
use reify_test_support::builders::CompiledModuleBuilder;
#[allow(unused_imports)]
use reify_test_support::{
    make_engine, manufacturing_purpose, my_design_template, step_input_template,
    step_output_template,
};
#[allow(unused_imports)]
use reify_types::{DiagnosticCode, ExportFormat, ModulePath, Severity};

/// Step-1 (failing initially; passes once step-2's
/// `emit_imported_tolerance_promise_diagnostics_for_module` helper is wired
/// into the production `build()` path).
///
/// The fixture is the canonical "promise loose, demand tight" pairing: a
/// `STEPInput` template carries a 50µm imported-geometry tolerance promise,
/// the `STEPOutput` template's body constraint is `RepresentationWithin(…, 1µm)`,
/// and a manufacturing purpose at 1µm is activated against `MyDesign`. Per the
/// `Engine::check_imported_tolerance_promise` truth table (engine_tolerance.rs:
/// 36-67), `min(1µm, 1µm) = 1µm` is strictly tighter than the 50µm promise, so
/// the runtime must surface a single `Severity::Warning` carrying
/// `DiagnosticCode::ImportedTolerancePromiseInsufficient` whose message names
/// the input template (`"STEPInput"`) so authors can locate the import site.
///
/// Today (pre step-2) the production `build()` path never invokes
/// `Engine::check_imported_tolerance_promise`, so this assertion FAILS — no
/// matching diagnostic is present in `BuildResult.diagnostics`. After step-2
/// adds the dispatcher helper and wires it from `build` /
/// `build_snapshot` / `tessellate_realizations`, the assertion passes.
#[test]
fn build_emits_imported_tolerance_promise_insufficient_warning_when_demand_strictly_tighter_than_promise()
 {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_build_emits_imported_tolerance_promise_warning".to_string(),
    ]))
    .template(step_input_template(50e-6))
    .template(step_output_template(1e-6))
    .template(my_design_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let mut engine = make_engine();
    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let build = engine.build(&module, ExportFormat::Step);

    let matched: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::ImportedTolerancePromiseInsufficient)
        })
        .collect();

    assert_eq!(
        matched.len(),
        1,
        "expected exactly one ImportedTolerancePromiseInsufficient warning in \
         BuildResult.diagnostics; got {} matching diagnostics. Full diagnostic \
         set: {:?}",
        matched.len(),
        build.diagnostics,
    );
    assert!(
        matched[0].message.contains("STEPInput"),
        "warning message must name the input template so authors can locate \
         the import site (got: {:?})",
        matched[0].message,
    );
}
