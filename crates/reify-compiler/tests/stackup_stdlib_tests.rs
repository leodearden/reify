//! Tests for stdlib/stackup.ri — tolerance stack-up authoring types
//! (Distribution, StackupMethod, Contributor, StackupResult).
//!
//! TDD structure mirrors tolerancing_tests.rs: each step adds one
//! structural assertion against the compiled std/stackup module, plus a
//! capstone acceptance test via the production parse_with_stdlib path.
//!
//! Field/variant names are the §4.1/§4.2 seam with the T1 builtins in
//! reify-stdlib/src/stackup.rs (task 3996 DONE @ a416709).

use reify_compiler::*;
use reify_core::*;

// ─── helper ───────────────────────────────────────────────────────────────────

/// Return the `std/stackup` CompiledModule from the production stdlib loader.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/stackup")
        .expect("stdlib should contain std/stackup module")
}

// ─── step-1: Distribution enum ───────────────────────────────────────────────

/// Step 1: std/stackup compiles without errors and declares the Distribution
/// enum with exactly three variants: Normal, Uniform, Triangular.
/// These variant names are the §4.1 seam with the T1 Value::Enum shapes in
/// reify-stdlib/src/stackup.rs.
#[test]
fn stackup_module_compiles_clean_and_declares_distribution() {
    let module = load_stdlib_module();

    // Zero error-severity diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in stackup.ri: {:?}",
        errors
    );

    // Distribution enum is present with exactly 3 variants.
    let dist = module
        .enum_defs
        .iter()
        .find(|e| e.name == "Distribution")
        .expect("expected 'Distribution' enum in std/stackup");

    assert_eq!(
        dist.variants.len(),
        3,
        "Distribution should have 3 variants, got: {:?}",
        dist.variants
    );
    for variant in &["Normal", "Uniform", "Triangular"] {
        assert!(
            dist.variants.contains(&variant.to_string()),
            "Distribution missing variant '{}', variants: {:?}",
            variant,
            dist.variants
        );
    }
}
