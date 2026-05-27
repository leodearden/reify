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
use reify_core::Type;

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
    let source =
        format!("structure def {struct_name} : {trait_name} {{\n    param x : Real = 1.0\n}}\n");
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

// ─── Topology selector helpers (task 2324 step-10) ───────────────────────────
//
// `closest_point(point, geometry) -> Point3<Length>`,
// `is_on(point, geometry) -> Bool`, and
// `angle_between_surfaces(a, b) -> Angle` are dispatched by name in the
// compiler (`is_geometry_topology_selector` in `units.rs`) and force the
// let-binding's compiled cell type to the registry-mandated Type via the new
// arm in `expr.rs`. Without that branch, the cell would be typed
// `Type::Geometry` (closest_point's geometry arg is the second arg, but
// `Type::Point3<Length>` for `is_on` would still be wrong from the first-arg
// fallback) and trip `assert_value_cell_types_representable`.

/// Compile a structure that names a topology-selector helper as a let binding
/// RHS, with a let-bound `point3(0mm, 0mm, 0mm)` and `box(10mm, 10mm, 10mm)`
/// in scope. Asserts no compile errors and returns the compiled module.
fn assert_topology_selector_let_compiles(call_rhs: &str, cell_name: &str) -> CompiledModule {
    let source = format!(
        r#"
structure def Bracket {{
    let body = box(10mm, 10mm, 10mm)
    let p = point3(0mm, 0mm, 0mm)
    let {cell_name} = {call_rhs}
}}
"#
    );
    let compiled = compile_source_with_stdlib(&source);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "structure with `{call_rhs}` should compile cleanly, got errors: {:#?}",
        errors
    );
    compiled
}

/// Find the value cell named `cell_name` in the compiled `Bracket` template
/// and assert its `cell_type` equals `expected`.
fn assert_helper_cell_typed(compiled: &CompiledModule, cell_name: &str, expected: &Type) {
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
        &cell.cell_type, expected,
        "cell `{cell_name}` must be typed {:?} (registry-mandated, not first-arg fallback), got: {:?}",
        expected, cell.cell_type
    );
}

#[test]
fn closest_point_let_binding_compiles_with_point3_length_type() {
    let compiled = assert_topology_selector_let_compiles("closest_point(p, body)", "cp");
    assert_helper_cell_typed(&compiled, "cp", &Type::point3(Type::length()));
}

#[test]
fn is_on_let_binding_compiles_with_bool_type() {
    let compiled = assert_topology_selector_let_compiles("is_on(p, body)", "is_on_body");
    assert_helper_cell_typed(&compiled, "is_on_body", &Type::Bool);
}

#[test]
fn angle_between_surfaces_let_binding_compiles_with_angle_type() {
    // angle_between_surfaces' first/second args resolve to whatever expression
    // is provided; the compile-time type wiring keys only on the function
    // name, so passing `body, body` (two geometries) is sufficient to exercise
    // the result_type arm in `expr.rs`. v0.1 has no surface-extraction syntax;
    // semantic / runtime-arg validation lives in
    // `geometry_ops::try_eval_topology_selector` and falls through to
    // `Value::Undef` when args don't resolve to face handles.
    let compiled =
        assert_topology_selector_let_compiles("angle_between_surfaces(body, body)", "ang");
    assert_helper_cell_typed(&compiled, "ang", &Type::angle());
}

// ─── Task 2699 — table-driven coverage for all 11 topology-selector cells ─────
//
// Single source of truth: one `Bracket` template with all 11 let-bindings
// compiled via one `compile_source_with_stdlib` call. The table below pins
// (cell_name, expected cell_type) per name; iterating with
// `assert_helper_cell_typed` covers the same regression-lock as the 11 former
// per-name `*_let_binding_compiles_with_*_type` tests, at ~1/11 the stdlib-
// compile cost. Call shapes mirror PRD §3.9 (and the
// `examples/topology_selectors/all_topology_selectors_wiring.ri` fixture).
//
// Diagnostic-locality tradeoff: this test compiles all 11 rows in a single
// Bracket template (one stdlib-compile, ~1/11 the cost of 11 separate tests).
// The cost is that if any single registry row regresses, only the first compile
// error surfaces and the per-cell `assert_helper_cell_typed` loop never runs.
// If a maintainer hits that case, the recovery is straightforward: split the
// failing row into its own `compile_source_with_stdlib(...)` call (mirroring
// the existing helpers `assert_topology_selector_let_compiles` /
// `assert_helper_let_compiles`) to isolate. Adding a 12th task-2699 name
// remains a one-line table edit.

#[test]
fn task_2699_topology_selector_cells_typed_per_registry() {
    // Each row: (cell_name, RHS expression, expected cell type).
    // RHS expressions are inlined into the Bracket source below; the
    // cell-name / expected-type columns drive the post-compile assertion loop.
    let cases: &[(&str, &str, Type)] = &[
        (
            "all_edges",
            "edges(body)",
            Type::List(Box::new(Type::Geometry)),
        ),
        (
            "all_faces",
            "faces(body)",
            Type::List(Box::new(Type::Geometry)),
        ),
        (
            "short_edges",
            "edges_by_length(body, 0mm..50mm)",
            Type::List(Box::new(Type::Geometry)),
        ),
        (
            "small_faces",
            "faces_by_area(body, 0mm * 1mm .. 1m * 1m)",
            Type::List(Box::new(Type::Geometry)),
        ),
        (
            "top_faces",
            "faces_by_normal(body, vec3(0.0, 0.0, 1.0), 1deg)",
            Type::List(Box::new(Type::Geometry)),
        ),
        (
            "vert_edges",
            "edges_parallel_to(body, vec3(1.0, 0.0, 0.0), 1deg)",
            Type::List(Box::new(Type::Geometry)),
        ),
        (
            "bot_edges",
            "edges_at_height(body, 0mm, 0.01mm)",
            Type::List(Box::new(Type::Geometry)),
        ),
        (
            "neighbors",
            "adjacent_faces(body, body)",
            Type::List(Box::new(Type::Geometry)),
        ),
        (
            "shared",
            "shared_edges(body, body)",
            Type::List(Box::new(Type::Geometry)),
        ),
        (
            "centroid",
            "center_of_mass(body, 7850.0)",
            Type::point3(Type::length()),
        ),
        (
            "inertia_tensor",
            "moment_of_inertia(body, 7850.0)",
            Type::tensor(
                2,
                3,
                Type::Scalar {
                    dimension: reify_core::DimensionVector::MOMENT_OF_INERTIA,
                },
            ),
        ),
    ];

    let mut source =
        String::from("structure def Bracket {\n    let body = box(50mm, 30mm, 10mm)\n");
    for (cell, rhs, _) in cases {
        source.push_str(&format!("    let {cell} = {rhs}\n"));
    }
    source.push_str("}\n");

    let compiled = compile_source_with_stdlib(&source);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "Bracket with all 11 task-2699 topology-selector let-bindings must compile cleanly.\n\nSource:\n{source}\n\nErrors: {:#?}",
        errors
    );

    for (cell, _rhs, expected) in cases {
        assert_helper_cell_typed(&compiled, cell, expected);
    }
}
