//! Tests for stdlib_loader — embedded .ri stdlib loading, compilation, and caching.

use reify_compiler::stdlib_loader;
use reify_types::Severity;

// ─── step-1: basic loading ──────────────────────────────────────────────

/// load_stdlib() returns a non-empty slice of compiled modules.
#[test]
fn load_stdlib_returns_non_empty_slice() {
    let modules = stdlib_loader::load_stdlib();
    assert!(
        !modules.is_empty(),
        "load_stdlib() should return at least one compiled module"
    );
}

/// All stdlib modules compile without error-severity diagnostics.
#[test]
fn all_stdlib_modules_have_no_errors() {
    let modules = stdlib_loader::load_stdlib();
    for module in modules {
        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "stdlib module '{}' has error diagnostics: {:?}",
            module.path, errors
        );
    }
}

/// materials_mechanical.ri traits are present in the stdlib (Material, Elastic,
/// Strong, Hard, FatigueRated, FractureTough, Ductile, ImpactResistant, Damping).
#[test]
fn materials_mechanical_traits_present() {
    let modules = stdlib_loader::load_stdlib();

    // Collect all trait names across all stdlib modules
    let all_traits: Vec<&str> = modules
        .iter()
        .flat_map(|m| m.trait_defs.iter().map(|t| t.name.as_str()))
        .collect();

    let expected = [
        "Material",
        "Elastic",
        "Strong",
        "Hard",
        "FatigueRated",
        "FractureTough",
        "Ductile",
        "ImpactResistant",
        "Damping",
    ];

    for name in &expected {
        assert!(
            all_traits.contains(name),
            "expected trait '{}' in stdlib, found: {:?}",
            name, all_traits
        );
    }
}

/// Second call to load_stdlib() returns the same pointer (OnceLock cached).
#[test]
fn load_stdlib_is_cached() {
    let first = stdlib_loader::load_stdlib();
    let second = stdlib_loader::load_stdlib();
    assert!(
        std::ptr::eq(first, second),
        "load_stdlib() should return the same slice reference on repeated calls"
    );
}
