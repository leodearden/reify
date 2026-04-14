//! M8.3 stdlib integration tests.
//!
//! Exercises four stdlib modules — materials, ports, tolerancing, units —
//! through the full parse → compile_with_stdlib → eval pipeline using
//! .ri fixture files in examples/.
//!
//! Unlike m8_stdlib_integration.rs (M8.1: math/linalg), this file uses
//! `parse_and_compile_with_stdlib` + `SimpleConstraintChecker` because the
//! fixtures depend on stdlib traits (Material, Physical, Elastic, Strong),
//! enums (MaterialCondition, SurfaceParameter), structures (Position,
//! DimensionalTolerance), and unit aliases (1nm, 1Mm, 1in, 1psi).
//! Mirrors the m10_combined.rs / m11_field_calculus.rs eval pattern.

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_test_support::parse_and_compile_with_stdlib;
use reify_types::{DimensionVector, ModulePath, Severity, Value, ValueCellId};

// ── File paths (resolved at compile time from this crate's root) ─────────────

const PATH_MATERIALS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m8_materials.ri"
);
const PATH_PORTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m8_ports.ri"
);
const PATH_TOLERANCING: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m8_tolerancing.ri"
);
const PATH_UNITS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m8_units.ri"
);

// ── Expected SI constants for imperial units (from imperial_units_tests.rs) ──

const LBF_SI: f64 = 4.4482216152605;
const PSI_SI: f64 = 6894.757293168361;
const GAL_SI: f64 = 0.003785411784;
const LB_SI: f64 = 0.45359237;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read a .ri fixture file, parse, compile with stdlib (asserting no
/// Severity::Error diagnostics at each stage), eval with
/// `SimpleConstraintChecker`, and assert no eval errors.
/// Returns the full `EvalResult` for per-test assertions.
fn eval_ri_file(path: &str, module_name: &str) -> reify_eval::EvalResult {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} should exist: {}", path, e));

    let parsed = reify_syntax::parse(&source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {}: {:?}",
        path,
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors in {}: {:?}",
        path,
        compile_errors
    );

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in {}: {:?}",
        path,
        eval_errors
    );

    result
}

/// Read a .ri fixture file, parse, and compile with stdlib.
/// Panics if there are any Severity::Error diagnostics.
/// Returns the CompiledModule for compile-level assertions.
fn compiled_ri(path: &str, module_name: &str) -> CompiledModule {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
    parse_and_compile_with_stdlib(&source)
}

// ── Section 1: m8_materials.ri ───────────────────────────────────────────────

/// Smoke test: m8_materials.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn m8_materials_smoke() {
    let result = eval_ri_file(PATH_MATERIALS, "m8_materials");
    assert!(
        !result.values.is_empty(),
        "m8_materials.ri eval should produce non-empty values"
    );
}

// ── step-3: materials_bracket_mass_computed ───────────────────────────────────

/// Asserts AluminumBracket.mass = volume * density = 0.0004 * 2700 = 1.08 (Value::Real).
/// Both volume and density are Real (dimensionless) per the stdlib convention,
/// so mass is also Value::Real(1.08), not a Scalar.
#[test]
fn materials_bracket_mass_computed() {
    let result = eval_ri_file(PATH_MATERIALS, "m8_materials");

    // mass = volume * density (let computed inside the Physical trait)
    let mass_id = ValueCellId::new("AluminumBracket", "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("AluminumBracket.mass not found in eval result"));

    match mass_val {
        Value::Real(v) => {
            assert!(
                (v - 1.08).abs() < 1e-9,
                "AluminumBracket.mass should be ≈1.08 (= 0.0004 * 2700), got {}",
                v
            );
        }
        other => panic!(
            "AluminumBracket.mass should be Value::Real (Real*Real=Real), got {:?}",
            other
        ),
    }

    // density param (Reify stores whole-number float literals as Int in the evaluator)
    let density_id = ValueCellId::new("AluminumBracket", "density");
    let density_val = result
        .values
        .get(&density_id)
        .unwrap_or_else(|| panic!("AluminumBracket.density not found"));
    let density_f64 = match density_val {
        Value::Real(v) => *v,
        Value::Int(i) => *i as f64,
        other => panic!("AluminumBracket.density should be Real or Int, got {:?}", other),
    };
    assert!(
        (density_f64 - 2700.0).abs() < 1e-9,
        "AluminumBracket.density should be 2700.0, got {}",
        density_f64
    );
}

// ── step-5: materials_trait_conformance_checks ───────────────────────────────

/// Asserts the compiled AluminumBracket template has trait_bounds including
/// Physical, Elastic, and Strong (from the `: Physical + Elastic + Strong`
/// declaration), and at least 2 constraints injected by trait refinement
/// (Physical.volume > 0, Strong.uts >= yield_strength).
#[test]
fn materials_trait_conformance_checks() {
    use reify_compiler::CompiledModule;
    let compiled: CompiledModule = compiled_ri(PATH_MATERIALS, "m8_materials");

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "AluminumBracket")
        .expect("AluminumBracket template should exist in compiled module");

    // trait_bounds from the `: Physical + Elastic + Strong` header
    assert!(
        template.trait_bounds.contains(&"Physical".to_string()),
        "AluminumBracket should have 'Physical' trait bound, got: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"Elastic".to_string()),
        "AluminumBracket should have 'Elastic' trait bound, got: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"Strong".to_string()),
        "AluminumBracket should have 'Strong' trait bound, got: {:?}",
        template.trait_bounds
    );

    // At least 2 constraints injected by trait refinement:
    //   Physical: constraint volume > 0
    //   Strong:   constraint uts >= yield_strength
    assert!(
        template.constraints.len() >= 2,
        "AluminumBracket should have >= 2 constraints (from Physical + Strong trait refinements), got: {}",
        template.constraints.len()
    );
}

// ── Section 2: m8_ports.ri ───────────────────────────────────────────────────

/// Smoke test: m8_ports.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn m8_ports_smoke() {
    let result = eval_ri_file(PATH_PORTS, "m8_ports");
    assert!(
        !result.values.is_empty(),
        "m8_ports.ri eval should produce non-empty values"
    );
}

// ── Section 3: m8_tolerancing.ri ─────────────────────────────────────────────

/// Smoke test: m8_tolerancing.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn m8_tolerancing_smoke() {
    let result = eval_ri_file(PATH_TOLERANCING, "m8_tolerancing");
    assert!(
        !result.values.is_empty(),
        "m8_tolerancing.ri eval should produce non-empty values"
    );
}

// ── Section 4: m8_units.ri ───────────────────────────────────────────────────

/// Smoke test: m8_units.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn m8_units_smoke() {
    let result = eval_ri_file(PATH_UNITS, "m8_units");
    assert!(
        !result.values.is_empty(),
        "m8_units.ri eval should produce non-empty values"
    );
}

