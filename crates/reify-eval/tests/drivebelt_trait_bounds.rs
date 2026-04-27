//! Integration tests for `examples/drivebelt_trait_bounds.ri`.
//!
//! Exercises the composed-bound integration (ElasticallyDeformable + ImpactResistant +
//! Damping) and the §6.3-§6.6 gap-fill sample structures (CeramicLiner, Copper,
//! BorosilicateGlass, TitaniumImplant) through the full pipeline:
//!   parse_and_compile_with_stdlib → make_simple_engine → eval → check → verify.
//!
//! Mirrors the eval-integration pattern from `m9_trait_conformance.rs` and
//! `stress_trait_hierarchy.rs`.  The example is also auto-discovered by
//! `examples_smoke.rs` (no edits needed there).
//!
//! Tests will FAIL until step-10 lands `examples/drivebelt_trait_bounds.ri`.

use reify_test_support::{assert_no_eval_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Satisfaction, Severity, Value, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/drivebelt_trait_bounds.ri"
);

// ── Helper ────────────────────────────────────────────────────────────────────

/// Read the example file, parse it, compile with stdlib, assert zero error
/// diagnostics, run eval, assert no eval errors.
/// Returns (compiled, eval_result) for per-test assertions.
fn compile_and_eval() -> (reify_compiler::CompiledModule, reify_eval::EvalResult) {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("examples/drivebelt_trait_bounds.ri should exist: {}", e));

    let compiled = parse_and_compile_with_stdlib(&source);

    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "drivebelt_trait_bounds.ri should compile with zero errors, got: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);
    assert_no_eval_errors(&eval_result);

    (compiled, eval_result)
}

/// Look up a Real or Int value by (entity, field) and return it as f64.
/// Panics with a clear message if not found or wrong type.
#[track_caller]
fn get_real(eval: &reify_eval::EvalResult, entity: &str, field: &str) -> f64 {
    let id = ValueCellId::new(entity, field);
    let val = eval
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("{}.{} not found in eval result", entity, field));
    match val {
        Value::Real(v) => *v,
        Value::Int(i) => *i as f64,
        other => panic!("{}.{} should be Real/Int, got {:?}", entity, field, other),
    }
}

// ── (a) smoke: parses, compiles, ≥5 templates ────────────────────────────────

/// The example must compile with zero error diagnostics and produce at least 5
/// templates: DriveBelt, CeramicLiner, Copper, BorosilicateGlass, TitaniumImplant.
#[test]
fn drivebelt_example_compiles_and_produces_five_templates() {
    let (compiled, _eval) = compile_and_eval();

    assert!(
        compiled.templates.len() >= 5,
        "expected >=5 templates in drivebelt_trait_bounds.ri, got: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    let template_names: Vec<&str> = compiled.templates.iter().map(|t| t.name.as_str()).collect();
    for expected in &["DriveBelt", "CeramicLiner", "Copper", "BorosilicateGlass", "TitaniumImplant"] {
        assert!(
            template_names.contains(expected),
            "expected template '{}' in drivebelt_trait_bounds.ri, got: {:?}",
            expected,
            template_names
        );
    }
}

// ── (b) DriveBelt trait_bounds and value cells ────────────────────────────────

/// DriveBelt must have trait_bounds containing ElasticallyDeformable, ImpactResistant,
/// and Damping, and value cells for all eight inherited members.
#[test]
fn drivebelt_trait_bounds_and_value_cells() {
    let (compiled, _eval) = compile_and_eval();

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "DriveBelt")
        .expect("DriveBelt template should exist");

    // trait_bounds
    for expected_trait in &["ElasticallyDeformable", "ImpactResistant", "Damping"] {
        assert!(
            template
                .trait_bounds
                .contains(&expected_trait.to_string()),
            "DriveBelt should have trait bound '{}', got: {:?}",
            expected_trait,
            template.trait_bounds
        );
    }

    // value cells: eight inherited members across the chain
    let expected_members = [
        "stiffness",        // from Flexible via ElasticallyDeformable
        "max_deflection",   // from Flexible via ElasticallyDeformable
        "max_elastic_strain", // from ElasticallyDeformable
        "density",          // from MaterialSpec via ImpactResistant / Damping
        "name",             // from MaterialSpec via ImpactResistant / Damping
        "impact_energy",    // from ImpactResistant
        "damping_ratio",    // from Damping
        "loss_factor",      // from Damping
    ];
    let cell_members: Vec<&str> = template
        .value_cells
        .iter()
        .map(|vc| vc.id.member.as_str())
        .collect();
    for expected_member in &expected_members {
        assert!(
            cell_members.contains(expected_member),
            "DriveBelt should have value cell '{}', got: {:?}",
            expected_member,
            cell_members
        );
    }
}

// ── (c) eval populates representative values ──────────────────────────────────

/// DriveBelt.density should equal the declared elastomer-belt default (1100.0).
#[test]
fn drivebelt_density_is_elastomer_value() {
    let (_compiled, eval) = compile_and_eval();
    let density = get_real(&eval, "DriveBelt", "density");
    assert!(
        (density - 1100.0).abs() < 1e-9,
        "DriveBelt.density should be 1100.0 (elastomer), got {}",
        density
    );
}

/// CeramicLiner.max_service_temperature = 2050.0 (above the Refractory >= 1500 guard).
#[test]
fn ceramic_liner_max_service_temperature() {
    let (_compiled, eval) = compile_and_eval();
    let temp = get_real(&eval, "CeramicLiner", "max_service_temperature");
    assert!(
        (temp - 2050.0).abs() < 1e-9,
        "CeramicLiner.max_service_temperature should be 2050.0, got {}",
        temp
    );
}

/// Copper.resistivity = 1.7e-8 (below the Conductive < 1e-4 guard).
#[test]
fn copper_resistivity_is_conductive_value() {
    let (_compiled, eval) = compile_and_eval();
    let r = get_real(&eval, "Copper", "resistivity");
    assert!(
        (r - 1.7e-8).abs() < 1e-14,
        "Copper.resistivity should be 1.7e-8, got {}",
        r
    );
}

/// BorosilicateGlass.refractive_index = 1.52.
#[test]
fn borosilicate_glass_refractive_index() {
    let (_compiled, eval) = compile_and_eval();
    let n = get_real(&eval, "BorosilicateGlass", "refractive_index");
    assert!(
        (n - 1.52).abs() < 1e-9,
        "BorosilicateGlass.refractive_index should be 1.52, got {}",
        n
    );
}

/// TitaniumImplant.corrosion_class is the enum variant CorrosionClass.C5.
#[test]
fn titanium_implant_corrosion_class_is_c5() {
    let (_compiled, eval) = compile_and_eval();
    let id = ValueCellId::new("TitaniumImplant", "corrosion_class");
    let val = eval
        .values
        .get(&id)
        .expect("TitaniumImplant.corrosion_class not found in eval result");
    match val {
        Value::Enum { type_name, variant } => {
            assert_eq!(
                type_name, "CorrosionClass",
                "corrosion_class type_name should be CorrosionClass, got {}",
                type_name
            );
            assert_eq!(
                variant, "C5",
                "corrosion_class variant should be C5, got {}",
                variant
            );
        }
        other => panic!(
            "TitaniumImplant.corrosion_class should be Value::Enum, got {:?}",
            other
        ),
    }
}

// ── (d) all constraints satisfied for every entity ───────────────────────────

/// engine.check(&compiled) must produce non-empty constraint_results for each of
/// the five entities, and every constraint must be Satisfaction::Satisfied.
///
/// Key constraints exercised:
///   - Flexible: stiffness > 0, max_deflection > 0 (DriveBelt via ED)
///   - ElasticallyDeformable: max_elastic_strain > 0 (DriveBelt)
///   - Refractory: max_service_temperature >= 1500.0 (CeramicLiner.max_service_temperature = 2050.0)
///   - Conductive: resistivity < 1e-4 (Copper.resistivity = 1.7e-8)
#[test]
fn all_constraints_satisfied_for_all_entities() {
    let (compiled, _eval) = compile_and_eval();
    let mut engine = make_simple_engine();
    // Re-eval to rebuild engine state for check
    engine.eval(&compiled);
    let check = engine.check(&compiled);

    let entities = ["DriveBelt", "CeramicLiner", "Copper", "BorosilicateGlass", "TitaniumImplant"];

    for entity in &entities {
        let entity_constraints: Vec<_> = check
            .constraint_results
            .iter()
            .filter(|r| r.id.entity == *entity)
            .collect();

        assert!(
            !entity_constraints.is_empty(),
            "expected at least one constraint for entity '{}', check produced none",
            entity
        );

        for entry in &entity_constraints {
            assert_eq!(
                entry.satisfaction,
                Satisfaction::Satisfied,
                "constraint {} for entity '{}' should be Satisfied",
                entry.id,
                entity
            );
        }
    }
}
