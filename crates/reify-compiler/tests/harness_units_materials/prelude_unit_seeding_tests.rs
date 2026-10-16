//! The prelude unit-seeding seam, `reify_compiler::seed_prelude_units`, driven
//! directly rather than through `compile_with_prelude`.
//!
//! `phase_units` calls this same fn, so the end-to-end collision tests in
//! `unit_registry_tests.rs` and these direct tests pin one implementation. The
//! direct form adds what the end-to-end form cannot observe: the returned
//! diagnostics as a value, and the seeded registry itself.

use reify_compiler::{CompiledModule, UnitRegistry, seed_prelude_units};
use reify_core::Severity;
use reify_test_support::{compile_source_named, errors_only};

fn compiled_prelude(source: &str, name: &str) -> CompiledModule {
    let module = compile_source_named(source, name);
    assert!(
        errors_only(&module).is_empty(),
        "{name} compilation errors: {:?}",
        errors_only(&module)
    );
    module
}

#[test]
fn seeding_is_pub_only_and_last_wins_with_one_collision_warning() {
    let mod_a = compiled_prelude(
        "pub unit foo : Length = 1.0\nunit hidden : Length = 3.0",
        "mod_a",
    );
    let mod_b = compiled_prelude("pub unit foo : Length = 2.0", "mod_b");

    let mut registry = UnitRegistry::new();
    let diags = seed_prelude_units(&mut registry, &[&mod_a, &mod_b]);

    assert_eq!(diags.len(), 1, "exactly one collision warning: {diags:?}");
    let warning = &diags[0];
    assert_eq!(warning.severity, Severity::Warning, "{warning:?}");
    for needle in ["'foo'", "mod_a", "mod_b", "last-wins"] {
        assert!(
            warning.message.contains(needle),
            "collision warning must mention {needle}: {:?}",
            warning.message
        );
    }

    let foo = registry
        .lookup("foo")
        .expect("foo must be seeded from the prelude");
    assert_eq!(foo.factor, 2.0, "the later module must win");
    assert_eq!(foo.source_module.as_deref(), Some("mod_b"));

    assert!(
        registry.lookup("hidden").is_none(),
        "a non-pub prelude unit must not be seeded"
    );
}

#[test]
fn seeding_a_single_module_reports_nothing() {
    let only = compiled_prelude("pub unit foo : Length = 1.0", "mod_a");

    let mut registry = UnitRegistry::new();
    let diags = seed_prelude_units(&mut registry, &[&only]);

    assert!(diags.is_empty(), "no collision, no diagnostics: {diags:?}");
    assert!(registry.lookup("foo").is_some(), "foo must be seeded");
}
