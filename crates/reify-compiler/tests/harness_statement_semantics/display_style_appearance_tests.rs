//! Tests for the DisplayStyle color/finish extension in stdlib/io.ri
//! (task #4772, step-1 RED / step-2 GREEN).
//!
//! Asserts that `structure def DisplayStyle` in `std/io` declares params
//! named "color" AND "finish" (alongside the retained "opacity"/"wireframe").
//! Introspects the compiled template directly rather than asserting a
//! constructor-arg error — unknown named args are lenient (__arg fallback,
//! expr.rs:2316/2375), so a behavioral rejection test would be vacuously GREEN
//! on base. Template introspection is a guaranteed RED (color/finish absent on
//! base) and goes GREEN once io.ri is extended.
//!
//! load_stdlib() panics on any stdlib compile error, so this also guards the
//! clean-compile invariant once the extension lands (same guarantee as
//! all_stdlib_modules_have_no_errors in stdlib_loader_tests.rs).
//!
//! Precedent: io_traits_tests.rs::provenance_structure_present_with_correct_fields
//! (same io_module() helper, same value_cells introspection pattern).

use reify_compiler::stdlib_loader;

// ─── helper ────────────────────────────────────────────────────────────────────

/// Return the `std/io` CompiledModule from the production stdlib loader.
fn io_module() -> &'static reify_compiler::CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| format!("{}", m.path) == "std/io")
        .expect("std.io module should be present in the stdlib")
}

// ─── step-1 RED / step-2 GREEN ─────────────────────────────────────────────────

/// DisplayStyle must declare params "color", "finish", "opacity", and
/// "wireframe". RED on base (only opacity/wireframe); GREEN after step-2
/// extends io.ri with `param color : Color` and `param finish : Finish`.
///
/// Does NOT assert param types — the plan only requires the param names to be
/// present, and type assertions would need to resolve cross-module StructureRef
/// ("Color") vs TraitObject vs Scalar paths. Name presence is a robust,
/// sufficient gate for the task deliverable.
#[test]
fn display_style_has_color_and_finish_params() {
    let module = io_module();

    let display_style = module
        .templates
        .iter()
        .find(|t| t.name == "DisplayStyle")
        .expect("std.io should contain a DisplayStyle structure template");

    // Helper: assert a value cell with the given member name is present.
    let assert_param = |member: &str| {
        assert!(
            display_style
                .value_cells
                .iter()
                .any(|vc| vc.id.member == member),
            "DisplayStyle should have a '{}' param; found: {:?}",
            member,
            display_style
                .value_cells
                .iter()
                .map(|vc| &vc.id.member)
                .collect::<Vec<_>>()
        );
    };

    // Retained params (must still be present after extension).
    assert_param("opacity");
    assert_param("wireframe");

    // New params (RED on base — these will be absent until step-2).
    assert_param("color");
    assert_param("finish");
}
