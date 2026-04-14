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
