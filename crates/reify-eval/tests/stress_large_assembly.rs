//! Stress tests for large parametric assembly via large_assembly.ri fixture.
//!
//! Covers:
//!   - smoke test: fixture parses, compiles with stdlib, evaluates without errors
//!   - expected templates: >=9 templates (8 Physical types + 1 Assembly + port traits)
//!   - assembly has >=50 sub-components
//!   - mass propagation: SteelBeam.mass = volume * density (1e-9 tolerance)
//!   - mass propagation: AluminumPlate.mass = volume * density
//!   - total_mass computed: LargeAssembly.total_mass > 0
//!   - all constraints satisfied (no Violated)
//!   - purpose activation: simulation_ready on SteelBeam injects constraints
//!   - eval performance: full pipeline (read+parse+compile+eval) < 5 seconds

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use reify_compiler::CompiledModule;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{ModulePath, Satisfaction, Severity, Value, ValueCellId};

/// Absolute path to the fixture file.
const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/large_assembly.ri"
);

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Read and cache the fixture source. Panics if the file doesn't exist.
fn source() -> String {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(FIXTURE_PATH)
            .unwrap_or_else(|e| panic!("{} should exist: {}", FIXTURE_PATH, e))
    })
    .clone()
}

/// Parse, compile with stdlib, and cache the result. Asserts no compile errors.
fn compiled() -> CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| {
        let src = source();
        let compiled = parse_and_compile_with_stdlib(&src);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "compile errors in large_assembly.ri: {:?}",
            errors
        );
        compiled
    })
    .clone()
}

/// Eval the canonical source with SimpleConstraintChecker, cache and return a static reference.
/// Uses the cached compiled module to avoid redundant compilation.
/// The OnceLock ensures the expensive eval runs only once across all tests.
fn eval_canonical() -> &'static reify_eval::EvalResult {
    static E: OnceLock<reify_eval::EvalResult> = OnceLock::new();
    E.get_or_init(|| {
        let mut engine = make_simple_engine();
        let result = engine.eval(&compiled());
        let eval_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            eval_errors.is_empty(),
            "eval errors in large_assembly.ri: {:?}",
            eval_errors
        );
        result
    })
}

/// Check the canonical source with SimpleConstraintChecker, return CheckResult.
fn check_canonical() -> reify_eval::CheckResult {
    let mut engine = make_simple_engine();
    engine.check(&compiled())
}

// ── caching test ──────────────────────────────────────────────────────────────

/// eval_canonical() must return the same static reference on every call.
/// This verifies the OnceLock caching is in place: two calls produce pointer-equal results.
#[test]
fn eval_canonical_is_cached() {
    let a: &'static reify_eval::EvalResult = eval_canonical();
    let b: &'static reify_eval::EvalResult = eval_canonical();
    assert!(
        std::ptr::eq(a, b),
        "eval_canonical() must return the same static reference on every call"
    );
}

// ── step-5: smoke test ────────────────────────────────────────────────────────

/// Load large_assembly.ri with stdlib, compile, eval — no errors, non-empty values.
#[test]
fn smoke_compiles_and_evals() {
    let result = eval_canonical();
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for large_assembly.ri"
    );
}

// ── step-7: structural assertions ─────────────────────────────────────────────

/// >=9 templates: 8 Physical structure types + 1 Assembly + port traits.
#[test]
fn has_expected_templates() {
    let c = compiled();
    assert!(
        c.templates.len() >= 9,
        "expected >=9 templates (8 Physical types + Assembly + port traits), got {}: {:?}",
        c.templates.len(),
        c.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
    // All 8 Physical types and the assembly must be present
    let expected = [
        "SteelBeam",
        "AluminumPlate",
        "StiffenerBracket",
        "BoltConnector",
        "MotorUnit",
        "GearboxUnit",
        "CoverPanel",
        "SensorPod",
        "LargeAssembly",
    ];
    let actual: Vec<&str> = c.templates.iter().map(|t| t.name.as_str()).collect();
    for name in &expected {
        assert!(
            actual.contains(name),
            "expected template '{}' in compiled output, got {:?}",
            name,
            actual
        );
    }
}

/// LargeAssembly template has >=50 sub-components.
#[test]
fn assembly_has_50_plus_subs() {
    let c = compiled();
    let assembly = c
        .templates
        .iter()
        .find(|t| t.name == "LargeAssembly")
        .expect("LargeAssembly template should be present");
    assert!(
        assembly.sub_components.len() >= 50,
        "LargeAssembly should have >=50 sub-components, got {}",
        assembly.sub_components.len()
    );
}

/// SteelBeam.mass = volume * density (tolerance 1e-9).
#[test]
fn mass_propagation_steel_beam() {
    let result = eval_canonical();
    let mass_id = ValueCellId::new("SteelBeam", "mass");
    let vol_id = ValueCellId::new("SteelBeam", "volume");
    let den_id = ValueCellId::new("SteelBeam", "density");

    let mass = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("SteelBeam.mass not found in eval result"));
    let volume = result
        .values
        .get(&vol_id)
        .unwrap_or_else(|| panic!("SteelBeam.volume not found in eval result"));
    let density = result
        .values
        .get(&den_id)
        .unwrap_or_else(|| panic!("SteelBeam.density not found in eval result"));

    let mass_val = match mass {
        Value::Real(v) => *v,
        Value::Int(v) => *v as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("SteelBeam.mass should be Real or Scalar, got {:?}", other),
    };
    let vol_val = match volume {
        Value::Real(v) => *v,
        Value::Int(v) => *v as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("SteelBeam.volume should be Real or Scalar, got {:?}", other),
    };
    let den_val = match density {
        Value::Real(v) => *v,
        Value::Int(v) => *v as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("SteelBeam.density should be Real or Scalar, got {:?}", other),
    };

    let expected = vol_val * den_val;
    assert!(
        (mass_val - expected).abs() < 1e-9,
        "SteelBeam.mass should be volume*density ≈ {}, got {}",
        expected,
        mass_val
    );
}

/// AluminumPlate.mass = volume * density (tolerance 1e-9).
#[test]
fn mass_propagation_aluminum_plate() {
    let result = eval_canonical();
    let mass_id = ValueCellId::new("AluminumPlate", "mass");
    let vol_id = ValueCellId::new("AluminumPlate", "volume");
    let den_id = ValueCellId::new("AluminumPlate", "density");

    let mass = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("AluminumPlate.mass not found in eval result"));
    let volume = result
        .values
        .get(&vol_id)
        .unwrap_or_else(|| panic!("AluminumPlate.volume not found in eval result"));
    let density = result
        .values
        .get(&den_id)
        .unwrap_or_else(|| panic!("AluminumPlate.density not found in eval result"));

    let mass_val = match mass {
        Value::Real(v) => *v,
        Value::Int(v) => *v as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("AluminumPlate.mass should be Real or Scalar, got {:?}", other),
    };
    let vol_val = match volume {
        Value::Real(v) => *v,
        Value::Int(v) => *v as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("AluminumPlate.volume should be Real or Scalar, got {:?}", other),
    };
    let den_val = match density {
        Value::Real(v) => *v,
        Value::Int(v) => *v as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("AluminumPlate.density should be Real or Scalar, got {:?}", other),
    };

    let expected = vol_val * den_val;
    assert!(
        (mass_val - expected).abs() < 1e-9,
        "AluminumPlate.mass should be volume*density ≈ {}, got {}",
        expected,
        mass_val
    );
}

/// LargeAssembly.total_mass exists and is > 0.
#[test]
fn total_mass_computed() {
    let result = eval_canonical();
    let id = ValueCellId::new("LargeAssembly", "total_mass");
    let total = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("LargeAssembly.total_mass not found in eval result"));
    let val = match total {
        Value::Real(v) => *v,
        Value::Int(v) => *v as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("LargeAssembly.total_mass should be Real or Scalar, got {:?}", other),
    };
    assert!(
        val > 0.0,
        "LargeAssembly.total_mass should be > 0, got {}",
        val
    );
}

// ── step-9: constraint, purpose, and performance tests ────────────────────────

/// All constraints in the module should be Satisfied (not Violated).
#[test]
fn all_constraints_satisfied() {
    let check = check_canonical();
    for entry in &check.constraint_results {
        assert_ne!(
            entry.satisfaction,
            Satisfaction::Violated,
            "constraint {} should not be Violated, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

/// Activating simulation_ready on SteelBeam should inject constraints into the engine.
#[test]
fn purpose_activation_simulation_ready() {
    let c = compiled();
    let mut engine = make_simple_engine();
    engine.eval(&c);

    let before = engine
        .snapshot()
        .expect("snapshot should exist after eval")
        .graph
        .constraints
        .len();

    engine.activate_purpose("simulation_ready", "SteelBeam");

    let after = engine
        .snapshot()
        .expect("snapshot should exist after purpose activation")
        .graph
        .constraints
        .len();

    assert!(
        after > before,
        "activating simulation_ready on SteelBeam should increase constraint count ({} -> {})",
        before,
        after
    );
    assert!(
        engine.is_purpose_active("simulation_ready"),
        "simulation_ready should be active after activation"
    );
}

/// Full pipeline (read + parse + compile_with_stdlib + eval) should complete in < 5 seconds.
#[test]
fn eval_under_5_seconds() {
    let start = Instant::now();

    let source = std::fs::read_to_string(FIXTURE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", FIXTURE_PATH, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single("large_assembly"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "full pipeline should complete in < 5s, took {:?}",
        elapsed
    );
}
