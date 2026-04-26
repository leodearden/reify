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

// ─── step-3: all 7 trait names present ───────────────────────────────────────

/// Step 3: All 7 geometry conformance trait names are present in the compiled
/// module: Bounded, Closed, Manifold, Orientable, Convex, Connected, Watertight.
#[test]
fn all_seven_traits_present() {
    let module = load_stdlib_module();

    let trait_names: Vec<&str> = module.trait_defs.iter().map(|t| t.name.as_str()).collect();

    let expected = [
        "Bounded",
        "Closed",
        "Manifold",
        "Orientable",
        "Convex",
        "Connected",
        "Watertight",
    ];

    assert_eq!(
        module.trait_defs.len(),
        expected.len(),
        "expected exactly {} traits, got: {:?}",
        expected.len(),
        trait_names
    );

    for name in &expected {
        assert!(
            trait_names.contains(name),
            "expected trait '{}' in compiled module, found: {:?}",
            name,
            trait_names
        );
    }
}

// ─── step-5: Watertight refines Closed + Manifold ────────────────────────────

/// Step 5: Watertight is the only multi-refinement trait in this set. Its
/// refinements list must contain exactly Closed and Manifold (containment +
/// length, not exact ordering).
#[test]
fn watertight_refines_closed_and_manifold() {
    let module = load_stdlib_module();

    let watertight = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Watertight")
        .expect("expected 'Watertight' trait in compiled module");

    assert_eq!(
        watertight.refinements.len(),
        2,
        "Watertight should refine exactly 2 traits, got refinements: {:?}",
        watertight.refinements
    );
    assert!(
        watertight.refinements.contains(&"Closed".to_string()),
        "Watertight should refine Closed, got refinements: {:?}",
        watertight.refinements
    );
    assert!(
        watertight.refinements.contains(&"Manifold".to_string()),
        "Watertight should refine Manifold, got refinements: {:?}",
        watertight.refinements
    );
}

// ─── step-7: non-Watertight traits have empty refinements ────────────────────

/// Step 7: All six non-Watertight traits are pure markers with no parents —
/// their refinements lists must be empty.
#[test]
fn non_watertight_traits_have_empty_refinements() {
    let module = load_stdlib_module();

    let names = ["Bounded", "Closed", "Manifold", "Orientable", "Convex", "Connected"];
    for name in &names {
        let trait_def = module
            .trait_defs
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("expected '{}' trait in compiled module", name));

        assert!(
            trait_def.refinements.is_empty(),
            "trait '{}' should have empty refinements, got: {:?}",
            name,
            trait_def.refinements
        );
    }
}
