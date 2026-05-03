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

/// Look up a structure template by name within the `std/materials/fea` module.
///
/// All four starter materials (`Steel_AISI_1045`, `Aluminium_6061_T6`,
/// `Titanium_Ti6Al4V`, `ABS_Plastic`) plus `MaterialPropertyProvenance`
/// are top-level structures, so we go through `module.templates` and filter on
/// `EntityKind::Structure` to keep the assertion stable against future
/// non-structure additions to the same module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/materials/fea, got templates: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

/// Collect the param-kind value cells (ignoring `let` and auto cells) from a
/// template, returning them in the file order they were declared.
fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
}

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

// ─── step-3: MaterialPropertyProvenance structure ────────────────────────────

/// `MaterialPropertyProvenance` is the citation record carried alongside each
/// property of a concrete material. It must exist as a top-level structure in
/// the compiled `std/materials/fea` module with exactly three required `param`
/// slots — `source`, `reference`, `notes` — each typed `String`.
///
/// The three-slot shape is the foundation of the per-property-provenance
/// design (see Plan §"Architecture chosen"): each material gets four parallel
/// `..._provenance : MaterialPropertyProvenance` fields, one per property,
/// rather than a single Map keyed by property name. This test locks in the
/// citation record's shape before any material structure refers to it.
#[test]
fn material_property_provenance_struct_has_three_string_fields() {
    let template = find_structure("MaterialPropertyProvenance");

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        params.len(),
        3,
        "MaterialPropertyProvenance should have exactly 3 param cells, got: {:?}",
        names
    );

    for expected in &["source", "reference", "notes"] {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *expected)
            .unwrap_or_else(|| {
                panic!(
                    "MaterialPropertyProvenance missing '{}' param; got: {:?}",
                    expected, names
                )
            });
        assert_eq!(
            cell.cell_type,
            Type::String,
            "MaterialPropertyProvenance.{} should be String, got {:?}",
            expected,
            cell.cell_type
        );
    }
}
