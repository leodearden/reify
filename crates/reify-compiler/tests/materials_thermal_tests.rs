//! Tests for stdlib/materials_thermal.ri — §6.3 thermal material traits.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path,
//! that `ThermallyCharacterized` and `Refractory` are correctly represented in
//! the compiled module, and that trait conformance and constraint injection
//! work as expected.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production (not a standalone `.ri` file re-read).

use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_types::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/materials/thermal` CompiledModule from the production
/// stdlib loader. Exercises the exact same code path as production: embedded
/// source, sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/materials/thermal")
        .expect("stdlib should contain std/materials/thermal module")
}

// ─── (a) module loads with zero error diagnostics and non-empty trait_defs ───

/// The std/materials/thermal module must load with zero error-severity
/// diagnostics and contain at least one trait definition.
#[test]
fn thermal_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in materials_thermal.ri: {:?}",
        errors
    );

    assert!(
        !module.trait_defs.is_empty(),
        "expected at least one trait def in std/materials/thermal, got zero"
    );
}

// ─── (b) ThermallyCharacterized has MaterialSpec refinement and 6 Real members ─

/// ThermallyCharacterized must refine MaterialSpec and declare six required
/// members, all typed as Real: thermal_conductivity, specific_heat,
/// thermal_expansion, melting_point, max_service_temperature, glass_transition.
#[test]
fn thermally_characterized_refines_material_spec_with_six_real_members() {
    let module = load_stdlib_module();

    let tc = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ThermallyCharacterized")
        .expect("expected 'ThermallyCharacterized' trait in std/materials/thermal");

    // Must refine MaterialSpec (the canonical base material trait).
    assert!(
        tc.refinements.contains(&"MaterialSpec".to_string()),
        "ThermallyCharacterized must refine MaterialSpec, got refinements: {:?}",
        tc.refinements
    );

    // Exactly six own required members.
    assert_eq!(
        tc.required_members.len(),
        6,
        "ThermallyCharacterized should have exactly 6 required members, got: {:?}",
        tc.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let expected_members = [
        "thermal_conductivity",
        "specific_heat",
        "thermal_expansion",
        "melting_point",
        "max_service_temperature",
        "glass_transition",
    ];

    for expected in &expected_members {
        let req = tc
            .required_members
            .iter()
            .find(|r| r.name == *expected)
            .unwrap_or_else(|| {
                panic!(
                    "ThermallyCharacterized missing required member '{}', got: {:?}",
                    expected,
                    tc.required_members
                        .iter()
                        .map(|r| &r.name)
                        .collect::<Vec<_>>()
                )
            });
        match &req.kind {
            RequirementKind::Param(ty) => assert_eq!(
                *ty,
                Type::Real,
                "ThermallyCharacterized member '{}' should be Real, got {:?}",
                expected,
                ty
            ),
            other => panic!(
                "ThermallyCharacterized member '{}' should be Param, got {:?}",
                expected, other
            ),
        }
    }
}

// ─── (c) Refractory refines ThermallyCharacterized with a constraint default ──

/// Refractory must refine ThermallyCharacterized and carry at least one
/// DefaultKind::Constraint (the `max_service_temperature >= 1500.0` guard).
#[test]
fn refractory_refines_thermally_characterized_with_constraint() {
    let module = load_stdlib_module();

    let refractory = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Refractory")
        .expect("expected 'Refractory' trait in std/materials/thermal");

    assert!(
        refractory
            .refinements
            .contains(&"ThermallyCharacterized".to_string()),
        "Refractory must refine ThermallyCharacterized, got refinements: {:?}",
        refractory.refinements
    );

    let constraint_defaults: Vec<_> = refractory
        .defaults
        .iter()
        .filter(|d| matches!(d.kind, DefaultKind::Constraint(_)))
        .collect();
    assert!(
        !constraint_defaults.is_empty(),
        "Refractory must have at least one DefaultKind::Constraint (max_service_temperature >= 1500.0)"
    );
}

// ─── (d) CeramicLiner : Refractory conformance test ──────────────────────────

/// A structure conforming to Refractory must compile cleanly via the full
/// stdlib pipeline, carry Refractory as a trait bound, and have value cells
/// for the complete inherited member chain:
///   MaterialSpec: density, name
///   ThermallyCharacterized (own): thermal_conductivity, specific_heat,
///     thermal_expansion, melting_point, max_service_temperature, glass_transition
#[test]
fn ceramic_liner_conforms_to_refractory_with_full_member_chain() {
    // max_service_temperature = 2050.0 clears the >= 1500.0 Refractory constraint.
    let source = r#"
structure def CeramicLiner : Refractory {
    param density : Real = 3900.0
    param name : String = "alumina"
    param thermal_conductivity : Real = 30.0
    param specific_heat : Real = 880.0
    param thermal_expansion : Real = 8.1e-6
    param melting_point : Real = 2345.0
    param max_service_temperature : Real = 2050.0
    param glass_transition : Real = 0.0
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
        "CeramicLiner : Refractory should compile cleanly, got errors: {:?}",
        errors
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "CeramicLiner")
        .expect("expected 'CeramicLiner' template in compiled module");

    assert!(
        template.trait_bounds.contains(&"Refractory".to_string()),
        "CeramicLiner must have 'Refractory' trait bound, got: {:?}",
        template.trait_bounds
    );

    // Verify all expected value cells are present (inherited + own members).
    let expected_cells = [
        "density",
        "name",
        "thermal_conductivity",
        "specific_heat",
        "thermal_expansion",
        "melting_point",
        "max_service_temperature",
        "glass_transition",
    ];
    for cell_name in &expected_cells {
        assert!(
            template.value_cells.iter().any(|vc| vc.id.member == *cell_name),
            "CeramicLiner template missing value cell '{}', cells: {:?}",
            cell_name,
            template
                .value_cells
                .iter()
                .map(|vc| &vc.id.member)
                .collect::<Vec<_>>()
        );
    }
}
