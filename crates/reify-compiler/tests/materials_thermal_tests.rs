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

mod common;

use common::assert_trait_constraint_binop;
use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_core::*;

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

// ─── (b) ThermallyCharacterized: 3 required + 3 optional members ─────────────

/// ThermallyCharacterized must refine MaterialSpec and declare exactly three
/// required members (the dimensioned scalar types tightened by task #3115)
/// plus three optional params with `= undef` defaults (the temperature-point
/// params that become optional in task #4241 γ; their tightening to Temperature
/// belongs to sibling task #3112).
///
/// Required:
///   thermal_conductivity     → Type::Scalar { dimension: THERMAL_CONDUCTIVITY }
///   specific_heat            → Type::Scalar { dimension: SPECIFIC_HEAT }
///   thermal_expansion        → Type::Scalar { dimension: THERMAL_EXPANSION }
///
/// Optional (DefaultKind::Param in tc.defaults):
///   melting_point            → Type::Scalar { dimension: TEMPERATURE } (= undef; tightened by task #3112)
///   max_service_temperature  → Type::Scalar { dimension: TEMPERATURE } (= undef; tightened by task #3112)
///   glass_transition         → Type::Scalar { dimension: TEMPERATURE } (= undef; tightened by task #3112)
#[test]
fn thermally_characterized_has_three_required_and_three_optional_members() {
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

    // Exactly three own required members (the dimensioned scalar types).
    assert_eq!(
        tc.required_members.len(),
        3,
        "ThermallyCharacterized should have exactly 3 required members, got: {:?}",
        tc.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let expected_required: [(&str, Type); 3] = [
        (
            "thermal_conductivity",
            Type::Scalar {
                dimension: DimensionVector::THERMAL_CONDUCTIVITY,
            },
        ),
        (
            "specific_heat",
            Type::Scalar {
                dimension: DimensionVector::SPECIFIC_HEAT,
            },
        ),
        (
            "thermal_expansion",
            Type::Scalar {
                dimension: DimensionVector::THERMAL_EXPANSION,
            },
        ),
    ];

    for (expected_name, expected_ty) in &expected_required {
        let req = tc
            .required_members
            .iter()
            .find(|r| r.name == *expected_name)
            .unwrap_or_else(|| {
                panic!(
                    "ThermallyCharacterized missing required member '{}', got: {:?}",
                    expected_name,
                    tc.required_members
                        .iter()
                        .map(|r| &r.name)
                        .collect::<Vec<_>>()
                )
            });
        match &req.kind {
            RequirementKind::Param(ty) => assert_eq!(
                ty, expected_ty,
                "ThermallyCharacterized required member '{}' expected {:?}, got {:?}",
                expected_name, expected_ty, ty
            ),
            other => panic!(
                "ThermallyCharacterized required member '{}' should be Param, got {:?}",
                expected_name, other
            ),
        }
    }

    // Three optional params must appear in tc.defaults as DefaultKind::Param with
    // cell_type = Type::Scalar { dimension: TEMPERATURE } — tightened from Real by
    // task #3112.
    let expected_optional_type = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };
    let optional_params = ["melting_point", "max_service_temperature", "glass_transition"];
    for param_name in &optional_params {
        let default = tc
            .defaults
            .iter()
            .find(|d| d.name.as_deref() == Some(param_name))
            .unwrap_or_else(|| {
                panic!(
                    "ThermallyCharacterized missing optional default for '{}', defaults: {:?}",
                    param_name,
                    tc.defaults
                        .iter()
                        .map(|d| &d.name)
                        .collect::<Vec<_>>()
                )
            });
        match &default.kind {
            DefaultKind::Param { cell_type, .. } => assert_eq!(
                cell_type, &expected_optional_type,
                "ThermallyCharacterized optional param '{}' expected {:?}, got {:?}",
                param_name, expected_optional_type, cell_type
            ),
            other => panic!(
                "ThermallyCharacterized optional param '{}' should be DefaultKind::Param, got {:?}",
                param_name, other
            ),
        }
    }
}

// ─── (c) Refractory refines ThermallyCharacterized with a constraint default ──

/// Refractory must refine ThermallyCharacterized and carry a constraint
/// `max_service_temperature >= 1500.0` — verified at the BinOp expression level
/// so that a regression flipping the op (e.g. `>` instead of `>=`) or changing
/// the bound (e.g. `0.0`) is caught here.
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

    // BinOp-level check: op=">=", LHS=max_service_temperature, RHS≈1500.0
    assert_trait_constraint_binop(
        refractory,
        "Refractory",
        "max_service_temperature",
        ">=",
        1500.0,
        1e-6,
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
    // max_service_temperature = 2050.0K clears the >= 1500.0K Refractory constraint.
    let source = r#"
structure def CeramicLiner : Refractory {
    param density : Real = 3900.0
    param name : String = "alumina"
    param thermal_conductivity : ThermalConductivity = 30.0 * 1W / (1m * 1K)
    param specific_heat : SpecificHeat = 880.0 * 1J / (1kg * 1K)
    param thermal_expansion : ThermalExpansion = 0.0000081 / 1K
    param melting_point : Temperature = 2345.0K
    param max_service_temperature : Temperature = 2050.0K
    param glass_transition : Temperature = 0.0K
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
            template
                .value_cells
                .iter()
                .any(|vc| vc.id.member == *cell_name),
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
