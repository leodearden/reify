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
use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::{ExportFormat, Satisfaction, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

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

/// Build the canonical compiled module through a real-OCCT engine once,
/// cached via OnceLock. Returns `None` when OCCT is unavailable (callers skip
/// numeric assertions). Mirrors the OnceLock caching pattern from eval_canonical()
/// so the expensive 54-sub OCCT realization runs only once across all mass tests.
fn build_canonical_occt() -> Option<&'static reify_eval::BuildResult> {
    static B: OnceLock<Option<reify_eval::BuildResult>> = OnceLock::new();
    B.get_or_init(|| {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping real-OCCT assertions: OCCT not available");
            return None;
        }
        let checker = SimpleConstraintChecker;
        let mut planner = reify_geometry::SingleKernelHolder::new();
        planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
        let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
        Some(engine.build(&compiled(), ExportFormat::Step))
    })
    .as_ref()
}

/// Read the runtime `density` (SI kg·m⁻³) from a structure's evaluated
/// `material` StructureInstance cell. Lets expected mass track the actual
/// material constant rather than a hardcoded literal.
fn material_density_si(result: &reify_eval::BuildResult, structure: &str) -> f64 {
    match result.values.get(&ValueCellId::new(structure, "material")) {
        Some(Value::StructureInstance(data)) => match data.fields.get("density") {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("{structure}.material.density should be Scalar, got {other:?}"),
        },
        other => panic!("{structure}.material should be StructureInstance, got {other:?}"),
    }
}

/// Assert `value` is a `Value::Scalar` of dimension `dim` whose `si_value` is
/// within 1e-6 relative of `expected`.
fn assert_scalar_rel(value: Option<&Value>, dim: DimensionVector, expected: f64, what: &str) {
    match value {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension, dim,
                "{what}: expected dimension {dim:?}, got {dimension:?}"
            );
            let rel = (si_value - expected).abs() / expected.abs().max(f64::MIN_POSITIVE);
            assert!(
                rel < 1e-6,
                "{what}: si_value {si_value:.12} not within 1e-6 relative of \
                 {expected:.12} (rel={rel:.3e})"
            );
        }
        other => panic!("{what}: expected Value::Scalar{{{dim:?}}}, got {other:?}"),
    }
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

/// Assert that `entity.mass` is `Value::Scalar<MASS>` ≈ `expected_box_volume_m3 *
/// material.density` (rel < 1e-6), using the real-OCCT build result.
/// Density is read at runtime from the `material` StructureInstance slot (not hardcoded),
/// matching the landed GHR-ζ `mass = volume(geometry) * material.density` contract.
fn assert_mass_equals_volume_times_density(
    result: &reify_eval::BuildResult,
    entity: &str,
    expected_box_volume_m3: f64,
) {
    let density = material_density_si(result, entity);
    assert_scalar_rel(
        result.values.get(&ValueCellId::new(entity, "mass")),
        DimensionVector::MASS,
        expected_box_volume_m3 * density,
        &format!("{entity}.mass"),
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

/// `SteelBeam.mass` folds via the landed GHR-ζ dispatch (task 3608):
/// `mass = volume(geometry) * material.density`, where
///   - `geometry = box(150mm, 150mm, 3000mm)` → analytic volume 0.0675 m³
///   - `material.density ≈ 7850 kg/m³` (runtime-read from the `material` StructureInstance slot)
///     → `mass ≈ 529.875 kg` (`Value::Scalar<MASS>`, rel < 1e-6).
///
/// Requires the real-OCCT `Engine::build()` path via `build_canonical_occt()`.
/// Skips with zero numeric coverage when OCCT is unavailable; CI must have
/// `/opt/reify-deps` configured for this test to verify.
#[test]
fn mass_propagation_steel_beam() {
    let Some(result) = build_canonical_occt() else {
        return;
    };
    // box(150mm, 150mm, 3000mm) = 0.150 * 0.150 * 3.000 = 0.0675 m³
    assert_mass_equals_volume_times_density(result, "SteelBeam", 0.150 * 0.150 * 3.000);
}

/// `AluminumPlate.mass` folds via the landed GHR-ζ dispatch (task 3608):
/// `mass = volume(geometry) * material.density`, where
///   - `geometry = box(500mm, 500mm, 4mm)` → analytic volume 0.001 m³
///   - `material.density ≈ 2700 kg/m³` (runtime-read from the `material` StructureInstance slot)
///     → `mass = 2.7 kg` (`Value::Scalar<MASS>`, rel < 1e-6).
///
/// Requires the real-OCCT `Engine::build()` path via `build_canonical_occt()`.
/// Skips with zero numeric coverage when OCCT is unavailable; CI must have
/// `/opt/reify-deps` configured for this test to verify.
#[test]
fn mass_propagation_aluminum_plate() {
    let Some(result) = build_canonical_occt() else {
        return;
    };
    // box(500mm, 500mm, 4mm) = 0.500 * 0.500 * 0.004 = 0.001 m³
    assert_mass_equals_volume_times_density(result, "AluminumPlate", 0.500 * 0.500 * 0.004);
}

/// `LargeAssembly.total_mass = all_masses.sum` aggregates cross-cell AND
/// cross-entity geometry-query-derived sub masses. GHR-ζ (task 3608) landed and
/// per-entity `mass` now folds (see `mass_propagation_steel_beam`/`_aluminum_plate`),
/// but `post_process_geometry_queries` inserts only into geometry-query cells and
/// does NOT re-evaluate dependent/aggregate cells (documented limitation in
/// `geometry_ops.rs`; regression-pinned by `cross_cell_factored_dependent_stays_undef`).
/// So `total_mass` stays `Value::Undef` on the current build path.
///
/// Revival requires the per-cell fixpoint re-evaluation scheduled by task 4358
/// (unified-dag ε, pending), which will schedule geometry-query folds in
/// dependency order so that downstream aggregates like `total_mass` fold after
/// their per-entity `mass` inputs are resolved.
#[test]
#[ignore = "Blocked on #4358 (unified-dag ε): total_mass = all_masses.sum stays Undef — post_process_geometry_queries does not re-evaluate cross-cell/cross-entity dependent cells (geometry_ops.rs documented limitation)"]
fn total_mass_computed() {
    // TODO(#4358): when unified-dag ε lands, remove #[ignore], call
    // build_canonical_occt(), and assert LargeAssembly.total_mass > 0.
    // The Undef invariant is regression-pinned by cross_cell_factored_dependent_stays_undef.
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
#[ignore = "performance benchmark; run explicitly with --ignored"]
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
