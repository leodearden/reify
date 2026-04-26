//! Tests for stdlib/geometry_traits.ri — geometry conformance marker traits.
//!
//! Tests validate that the .ri file parses and compiles cleanly, that each
//! pure marker trait is correctly represented in the compiled module, and
//! that the traits resolve from the prelude in user `.ri` sources.

use reify_compiler::*;
use reify_types::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/geometry/traits` CompiledModule from the production
/// stdlib loader. Exercises the exact same code path as production: embedded
/// source, sequential compilation with growing prelude, OnceLock caching.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/geometry/traits")
        .expect("stdlib should contain std/geometry/traits module")
}

// ─── step-1: file exists, parses, compiles without errors ────────────────────

/// Step 1: geometry_traits.ri file exists, parses cleanly, compiles
/// without error-severity diagnostics, and has at least one trait def.
#[test]
fn stdlib_file_parses_and_compiles_without_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in geometry_traits.ri: {:?}",
        errors
    );

    assert!(
        !module.trait_defs.is_empty(),
        "expected at least one trait def, got zero"
    );
}
