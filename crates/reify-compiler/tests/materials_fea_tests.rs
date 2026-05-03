//! Tests for stdlib/materials_fea.ri — FEA-bound elastic-material trait + four
//! starter material instances (Steel_AISI_1045, Aluminium_6061_T6,
//! Titanium_Ti6Al4V, ABS_Plastic).
//!
//! Tests validate that the .ri file is loaded by the production stdlib path,
//! that `MaterialPropertyProvenance`, `ElasticMaterial`, and the four concrete
//! material structures are correctly represented in the compiled module, and
//! that trait conformance, constraint injection, and end-to-end value
//! evaluation through dimensioned defaults all work as expected.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production (not a standalone `.ri` file re-read). This mirrors the pattern
//! in `materials_thermal_tests.rs` and `materials_electrical_tests.rs`.

use reify_compiler::*;
use reify_types::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/materials/fea` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/materials/fea")
        .expect("stdlib should contain std/materials/fea module")
}

// ─── step-1: module loads with zero error diagnostics ────────────────────────

/// The std/materials/fea module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_materials_fea_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in materials_fea.ri: {:?}",
        errors
    );
}
