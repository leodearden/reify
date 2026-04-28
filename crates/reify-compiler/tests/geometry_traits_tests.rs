//! Tests for stdlib/geometry_traits.ri — geometry conformance marker traits.
//!
//! Behavioral coverage only. The "all expected trait names present + correct
//! count" check lives in `stdlib_loader_tests.rs::geometry_traits_present`,
//! driven by `EXPECTED_GEOMETRY_TRAITS` as the single source of truth — so
//! this file does not duplicate it. "No error diagnostics" is covered for
//! every stdlib module by `all_stdlib_modules_have_no_errors` in the same
//! loader test file. Per-trait structural-emptiness checks (empty refinements,
//! `required_members`, `defaults`) are intentionally omitted: a future change
//! that turned one of these into a real trait with members would be caught by
//! the prelude integration tests below in semantically meaningful ways.

use reify_compiler::*;
use reify_test_support::{compile_source_with_stdlib, errors_only};
use reify_types::Type;

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

// ─── Watertight refines Closed + Manifold ────────────────────────────────────

/// Watertight is the only multi-refinement trait in this set. Its refinements
/// list must contain exactly Closed and Manifold (containment + length, not
/// exact ordering — the parser is free to emit refinements in any order).
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

/// Compile a user `.ri` source declaring `structure def {struct_name} : {trait_name}`
/// against the production stdlib prelude, assert no error diagnostics, and
/// assert the trait bound landed on the generated template. Mirrors
/// stdlib_loader_tests.rs's compile_with_prelude_makes_traits_visible pattern.
fn assert_trait_resolves_from_prelude(trait_name: &str, struct_name: &str) {
    let source = format!(
        "structure def {struct_name} : {trait_name} {{\n    param x : Real = 1.0\n}}\n"
    );
    let compiled = compile_source_with_stdlib(&source);

    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "{struct_name} : {trait_name} should compile without errors via the prelude, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");
    assert!(
        template.trait_bounds.contains(&trait_name.to_string()),
        "{struct_name} should have '{trait_name}' trait bound, got: {:?}",
        template.trait_bounds
    );
}

// ─── marker trait resolves from prelude in user source ───────────────────────

/// A user `.ri` source can reference a geometry marker trait by bare name
/// and have it resolve via the prelude.
#[test]
fn marker_trait_resolves_from_prelude_in_user_source() {
    assert_trait_resolves_from_prelude("Bounded", "Box");
}

// ─── Watertight resolves from prelude with multi-refinement ──────────────────

/// End-to-end multi-refinement check. Watertight refines Closed + Manifold
/// (both declared in the same stdlib file) — the only behaviorally novel
/// case in this task; all six others are zero-refinement markers.
#[test]
fn watertight_resolves_from_prelude_with_multi_refinement() {
    assert_trait_resolves_from_prelude("Watertight", "Shell");
}

// ─── Conformance query helpers (task 2320 step-3) ────────────────────────────
//
// `is_watertight(g) -> Bool`, `is_manifold(g) -> Bool`, `is_orientable(g) ->
// Bool` are dispatched by name in the compiler (`is_geometry_query_helper` in
// `units.rs`) and force the let-binding's compiled cell type to `Type::Bool`
// in `expr.rs`'s `OverloadResolution::NoUserFunctions` arm. Without that
// branch, the cell would be typed `Type::Geometry` from the first-arg
// fallback and trip `assert_value_cell_types_representable`.

/// Compile a structure that names a single conformance helper as a let
/// binding's RHS, assert no compile errors, and return the compiled module
/// for further inspection.
fn assert_helper_let_compiles(helper: &str, cell_name: &str) -> CompiledModule {
    let source = format!(
        r#"
structure def Bracket {{
    let body = box(10mm, 10mm, 10mm)
    let {cell_name} = {helper}(body)
}}
"#
    );
    let compiled = compile_source_with_stdlib(&source);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "structure with `{helper}(body)` should compile cleanly, got errors: {:#?}",
        errors
    );
    compiled
}

/// Find the value cell named `cell_name` in the compiled `Bracket` template
/// and assert its `cell_type` equals `Type::Bool`. Returns the cell type so
/// the caller can produce a richer assertion error if the type is wrong.
fn assert_helper_cell_typed_bool(compiled: &CompiledModule, cell_name: &str) {
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("compiled module should contain `Bracket` template");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == cell_name)
        .unwrap_or_else(|| {
            panic!(
                "template `Bracket` should contain a `{cell_name}` value cell, got: {:?}",
                template
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        cell.cell_type,
        Type::Bool,
        "cell `{cell_name}` must be typed Bool (not the first-arg Geometry \
         fallback), got: {:?}",
        cell.cell_type
    );
}

#[test]
fn is_watertight_let_binding_compiles_with_bool_type() {
    let compiled = assert_helper_let_compiles("is_watertight", "watertight");
    assert_helper_cell_typed_bool(&compiled, "watertight");
}

#[test]
fn is_manifold_let_binding_compiles_with_bool_type() {
    let compiled = assert_helper_let_compiles("is_manifold", "manifold");
    assert_helper_cell_typed_bool(&compiled, "manifold");
}

#[test]
fn is_orientable_let_binding_compiles_with_bool_type() {
    let compiled = assert_helper_let_compiles("is_orientable", "orientable");
    assert_helper_cell_typed_bool(&compiled, "orientable");
}
