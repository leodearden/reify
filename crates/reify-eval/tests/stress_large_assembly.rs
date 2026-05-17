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
use reify_types::{ModulePath, Satisfaction, Severity, ValueCellId};

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

/// Parse, compile with stdlib, and cache the result.
fn compiled() -> CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| {
        let src = source();
        parse_and_compile_with_stdlib(&src)
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

/// Check the canonical source with SimpleConstraintChecker, cache and return a static reference.
/// Uses the same OnceLock pattern as eval_canonical() for API consistency.
fn check_canonical() -> &'static reify_eval::CheckResult {
    static K: OnceLock<reify_eval::CheckResult> = OnceLock::new();
    K.get_or_init(|| {
        let mut engine = make_simple_engine();
        engine.check(&compiled())
    })
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

/// check_canonical() must return the same static reference on every call.
/// This verifies the OnceLock caching is in place: two calls produce pointer-equal results.
#[test]
fn check_canonical_is_cached() {
    let a: &'static reify_eval::CheckResult = check_canonical();
    let b: &'static reify_eval::CheckResult = check_canonical();
    assert!(
        std::ptr::eq(a, b),
        "check_canonical() must return the same static reference on every call"
    );
}

/// Assert that `entity.mass == entity.volume * entity.density` within 1e-9.
fn assert_mass_equals_volume_times_density(result: &reify_eval::EvalResult, entity: &str) {
    let mass_id = ValueCellId::new(entity, "mass");
    let vol_id = ValueCellId::new(entity, "volume");
    let den_id = ValueCellId::new(entity, "density");

    let mass = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("{}.mass not found in eval result", entity));
    let volume = result
        .values
        .get(&vol_id)
        .unwrap_or_else(|| panic!("{}.volume not found in eval result", entity));
    let density = result
        .values
        .get(&den_id)
        .unwrap_or_else(|| panic!("{}.density not found in eval result", entity));

    let mass_val = mass
        .as_f64()
        .unwrap_or_else(|| panic!("{}.mass should be numeric, got {:?}", entity, mass));
    let vol_val = volume
        .as_f64()
        .unwrap_or_else(|| panic!("{}.volume should be numeric, got {:?}", entity, volume));
    let den_val = density
        .as_f64()
        .unwrap_or_else(|| panic!("{}.density should be numeric, got {:?}", entity, density));

    let expected = vol_val * den_val;
    assert!(
        (mass_val - expected).abs() < 1e-9,
        "{}.mass should be volume*density ≈ {}, got {}",
        entity,
        expected,
        mass_val
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
///
/// Post-GHR-α (task 3603 / PRD §8 Phase 1): spec-shape `Physical` no longer
/// exposes flat `volume` / `density` params — it now stores a `geometry :
/// Solid` slot plus a `material : Material` struct slot, and computes `mass =
/// volume(geometry) * material.density` via the new stdlib geometry-query
/// `volume(Solid) → Scalar<Volume>`. At Phase 1 this is typecheck-only; runtime
/// kernel dispatch for `volume(geometry)` arrives in Phase 6 (GHR-ζ), so
/// `mass` evaluates to `Value::Undef`. This numeric-read assertion (and the
/// helper's `.volume` / `.density` lookups) revives once geometry-derived
/// computation lands.
#[test]
#[ignore = "Phase 6 will revive — GHR-ζ (geometry-handle-runtime PRD): mass = volume(geometry) * material.density needs kernel dispatch"]
fn mass_propagation_steel_beam() {
    assert_mass_equals_volume_times_density(eval_canonical(), "SteelBeam");
}

/// AluminumPlate.mass = volume * density (tolerance 1e-9).
///
/// Post-GHR-α (task 3603 / PRD §8 Phase 1): see `mass_propagation_steel_beam`
/// for the full rationale — same flat-scalar→spec-shape transition; revived
/// once kernel dispatch lands.
#[test]
#[ignore = "Phase 6 will revive — GHR-ζ (geometry-handle-runtime PRD): mass = volume(geometry) * material.density needs kernel dispatch"]
fn mass_propagation_aluminum_plate() {
    assert_mass_equals_volume_times_density(eval_canonical(), "AluminumPlate");
}

/// LargeAssembly.total_mass exists and is > 0.
///
/// Post-GHR-α (task 3603 / PRD §8 Phase 1): `total_mass` aggregates each
/// sub-component's spec-shape `mass = volume(geometry) * material.density`,
/// which evaluates to `Value::Undef` until Phase 6 (GHR-ζ) wires kernel
/// dispatch — so the aggregate is also Undef. Revived once kernel dispatch
/// lands.
#[test]
#[ignore = "Phase 6 will revive — GHR-ζ (geometry-handle-runtime PRD): total_mass aggregates per-sub mass = volume(geometry) * material.density (needs kernel dispatch)"]
fn total_mass_computed() {
    let result = eval_canonical();
    let id = ValueCellId::new("LargeAssembly", "total_mass");
    let total = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("LargeAssembly.total_mass not found in eval result"));
    let val = total.as_f64().unwrap_or_else(|| {
        panic!(
            "LargeAssembly.total_mass should be numeric, got {:?}",
            total
        )
    });
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

/// Full pipeline (read + parse + compile_with_stdlib + eval) should complete in < 15 seconds.
// Performance benchmark — run explicitly with `cargo test -- --ignored`.
#[ignore]
#[test]
fn eval_full_pipeline_benchmark() {
    let start = Instant::now();

    // Intentionally NOT using the cached source()/compiled() helpers here.
    // This test measures the full uncached pipeline cost: file I/O → parse →
    // compile_with_stdlib → eval. The threshold is generous (15s) to avoid
    // CI flakiness from CPU contention.
    let source = std::fs::read_to_string(FIXTURE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", FIXTURE_PATH, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single("large_assembly"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
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
        elapsed < Duration::from_secs(15),
        "full pipeline should complete in < 15s, took {:?}",
        elapsed
    );
}
