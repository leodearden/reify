//! Engine-level integration test for `body_mass_props` (RBD-β, task 3829;
//! PRD `docs/prds/v0_3/rigid-body-dynamics.md` §2.1/§5.4).
//!
//! Pins the full engine dispatch path end-to-end:
//!   parse → `compile_with_stdlib` → `Engine::build` →
//!   `engine_build.rs::post_process_body_mass_props` →
//!   `reify_eval::dynamics_ops::try_eval_body_mass_props`.
//!
//! Observable signal (kernel-INDEPENDENT, so a `MockGeometryKernel` suffices):
//! a body with NO resolvable `Material.density` passed to `body_mass_props`
//! must (a) emit exactly one `W_DynamicsDefaultDensity` warning (the density
//! ladder falls through to the 1000 kg/m³ water default) and (b) leave the
//! `mp` cell as a `MassProperties` `StructureInstance`. The geometric fields
//! (`mass`/`com`/`inertia`) are the deferred `Value::Undef` sentinel because
//! the density-aware KGQ kernel query (`moment_of_inertia(Solid, Density)`,
//! task 3620) is NOT wired by this batch — so the warning + structure shape are
//! the only signals here, and both are kernel-independent.
//!
//! The MassProperties PSD inertia-validation hook (engine_eval.rs, task 3822)
//! classifies an `inertia == Value::Undef` field as `Skip` (no false
//! positives), so the assembled deferred instance is neither clobbered to a
//! bare `Undef` nor flagged `E_DynamicsInertiaNotPSD` — leaving exactly the one
//! `W_DynamicsDefaultDensity` warning asserted below.
//!
//! Step-11 RED: before step-12 wires `post_process_body_mass_props` into
//! `engine_build.rs`, the `mp` cell stays at the `Value::Undef` left by the
//! pure `eval_expr` path (a builtin `FunctionCall` has no pure-eval rule) and
//! no warning is emitted — so both assertions fail. Step-12 makes it GREEN.

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, errors_only, parse_and_compile_with_stdlib};

/// A body with no `Material` (hence no `Material.density`) is passed to
/// `body_mass_props`, so the fn-level density ladder falls through to the
/// 1000 kg/m³ water default and emits `W_DynamicsDefaultDensity`. The `mp`
/// cell must resolve to a `MassProperties` `StructureInstance` (geometric
/// fields deferred to `Undef`).
#[test]
fn body_mass_props_without_material_density_warns_and_assembles_mass_properties() {
    let source = "structure def MassPropsBox {\n    \
        let body = box(50mm, 30mm, 10mm)\n    \
        let mp = body_mass_props(body)\n}";

    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "MassPropsBox should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Kernel-independent: body_mass_props does not consult the kernel in this
    // batch (geometric fields stay Undef), so a plain mock kernel is enough.
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let result = engine.build(&compiled, ExportFormat::Step);

    // (1) Exactly one DynamicsDefaultDensity warning (default-water fallback).
    let default_density: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DynamicsDefaultDensity))
        .collect();
    assert_eq!(
        default_density.len(),
        1,
        "exactly one W_DynamicsDefaultDensity warning expected when body_mass_props \
         falls back to the water default; got {} (all diagnostics: {:#?})",
        default_density.len(),
        result.diagnostics,
    );
    assert_eq!(
        default_density[0].severity,
        Severity::Warning,
        "the default-density diagnostic must be a Warning (computation still proceeds)"
    );

    // (2) The `mp` cell evaluates to a MassProperties StructureInstance. The
    // geometric fields may be Undef (deferred KGQ kernel seam, task 3620); the
    // PSD hook's Undef-inertia Skip rule keeps the instance intact.
    let cell = ValueCellId::new("MassPropsBox", "mp");
    match result.values.get(&cell) {
        Some(Value::StructureInstance(data)) => {
            assert_eq!(
                data.type_name, "MassProperties",
                "MassPropsBox.mp must be a MassProperties StructureInstance, got type_name {:?}",
                data.type_name
            );
        }
        other => panic!(
            "MassPropsBox.mp must be a MassProperties StructureInstance (geometric fields \
             may be deferred Undef), got {other:?}"
        ),
    }
}
