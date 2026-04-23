//! Tests for the canonical first-class `Material` struct defined in
//! `stdlib/materials_mechanical.ri` — task 1876.
//!
//! The task promotes `Material` from a trait to a concrete structure carrying
//! fields `name : String`, `density : Real`, and `youngs_modulus : Real`. The
//! original trait-level contract has been renamed to `MaterialSpec` so that
//! the name `Material` is free to bind the new struct. These tests exercise
//! the struct's surface: presence in the stdlib, type-resolution behaviour
//! for `param x : Material` (must pick the struct over any trait fallback),
//! struct-call defaults at param sites, the end-to-end BoltFlange case, and
//! a regression that the renamed `MaterialSpec` trait still works as a
//! trait-object param type (preserving the task-1874 pathway).

use reify_compiler::{EntityKind, stdlib_loader};
use reify_test_support::compile_source_with_stdlib;
use reify_types::{CompiledExprKind, Severity, Type};

// ─── step-3: canonical Material struct is present in the stdlib ─────────────

/// The canonical `Material` struct must appear as a Structure template in the
/// stdlib with exactly three params — `name : String`, `density : Real`, and
/// `youngs_modulus : Real` — and none of the params may declare a default.
/// Callers are expected to supply values at construction.
#[test]
fn material_struct_present_in_stdlib() {
    let modules = stdlib_loader::load_stdlib();

    // Search every stdlib module for a template named "Material" that is a
    // Structure (not an Occurrence). The canonical home for this template is
    // `std/materials/mechanical`, but the assertion is expressed at the
    // whole-stdlib level so a future reorg doesn't break the test unnecessarily.
    let material = modules
        .iter()
        .flat_map(|m| m.templates.iter())
        .find(|t| t.name == "Material" && t.entity_kind == EntityKind::Structure)
        .expect(
            "expected a `structure def Material` template in the stdlib \
             (task 1876 promotes Material from a trait to a canonical struct)",
        );

    // Collect param cells (ignore lets and autos — step-3 expects three params).
    let param_cells: Vec<_> = material
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, reify_compiler::ValueCellKind::Param))
        .collect();

    assert_eq!(
        param_cells.len(),
        3,
        "Material struct should have exactly 3 params, got {}: {:?}",
        param_cells.len(),
        param_cells
            .iter()
            .map(|c| c.id.member.as_str())
            .collect::<Vec<_>>()
    );

    // Check each expected (name, type) pair is present.
    let expected: &[(&str, reify_types::Type)] = &[
        ("name", reify_types::Type::String),
        ("density", reify_types::Type::Real),
        ("youngs_modulus", reify_types::Type::Real),
    ];
    for (expected_name, expected_type) in expected {
        let cell = param_cells
            .iter()
            .find(|c| c.id.member == *expected_name)
            .unwrap_or_else(|| {
                panic!(
                    "Material struct missing expected param `{}`; present params: {:?}",
                    expected_name,
                    param_cells
                        .iter()
                        .map(|c| c.id.member.as_str())
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            &cell.cell_type, expected_type,
            "Material.{} should have type {:?}, got {:?}",
            expected_name, expected_type, cell.cell_type
        );
    }

    // None of the three params should carry a default — callers must supply
    // values at construction (design decision D2 in the task plan).
    for cell in &param_cells {
        assert!(
            cell.default_expr.is_none(),
            "Material.{} should have no default, got default_expr: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }
}

// ─── step-5: `param material : Material` resolves to StructureRef ───────────

/// `param material : Material` in a user structure must resolve to
/// `Type::StructureRef("Material")`, NOT `Type::TraitObject("Material")`. After
/// task 1876 the name `Material` is bound to the canonical struct (trait
/// fallback now lives under `MaterialSpec`), so type resolution of the bare
/// name `Material` must pick the struct. Compilation should succeed cleanly.
#[test]
fn param_material_resolves_to_struct_ref() {
    let source = r#"
        structure def Part { param material : Material }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors compiling `Part` with `param material : Material`, got: {:?}",
        errors
    );

    let part = module
        .templates
        .iter()
        .find(|t| t.name == "Part")
        .expect("Part template should be compiled");

    let material_cell = part
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "material")
        .expect("Part.material should exist");

    assert_eq!(
        material_cell.cell_type,
        Type::StructureRef("Material".to_string()),
        "Part.material should resolve to Type::StructureRef(\"Material\") now that Material \
         is a canonical struct (not the old trait); got {:?}",
        material_cell.cell_type
    );
}

// ─── step-7: struct-call is a valid default for a struct-typed param ────────

/// `param material : Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)`
/// must compile cleanly: the param type is `Type::StructureRef("Material")`,
/// and the default expression is recorded as a call to `Material` carrying the
/// three supplied arguments. This is the core "`: Material = Material(...)` is
/// meaningful" assertion for task 1876 — default-expression type-checking must
/// accept a struct-constructor call whose return type matches the declared
/// param type.
#[test]
fn material_struct_call_is_valid_param_default() {
    let source = r#"
        structure def Part {
            param material : Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors compiling `Part` with a Material(...) default, got: {:?}",
        errors
    );

    let part = module
        .templates
        .iter()
        .find(|t| t.name == "Part")
        .expect("Part template should be compiled");

    let material_cell = part
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "material")
        .expect("Part.material should exist");

    assert_eq!(
        material_cell.cell_type,
        Type::StructureRef("Material".to_string()),
        "Part.material should have type Type::StructureRef(\"Material\"); got {:?}",
        material_cell.cell_type
    );

    let default_expr = material_cell.default_expr.as_ref().expect(
        "Part.material should have a recorded default_expr (the `Material(...)` call) — \
         default-expression compilation must not drop struct-constructor calls",
    );

    // Struct-constructor calls lower to `CompiledExprKind::FunctionCall` with
    // a ResolvedFunction whose `name` is the struct's simple name and whose
    // `qualified_name` starts with the module prefix (e.g. `std::Material`).
    // Named-arg reordering is handled by the compiler; here we only care that
    // the callee is `Material` and that all three supplied values survived.
    match &default_expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            assert_eq!(
                function.name, "Material",
                "default_expr should be a call to `Material`, got function.name={:?}",
                function.name
            );
            assert_eq!(
                args.len(),
                3,
                "Material(...) should lower to a call with 3 args, got {}: {:?}",
                args.len(),
                args
            );
        }
        other => panic!(
            "expected Part.material.default_expr to be a FunctionCall for `Material(...)`, \
             got {:?}",
            other
        ),
    }
}

// ─── step-9: end-to-end BoltFlange compiles with a Material(...) default ────

/// Mirror `examples/m5_geometry_flange.ri` exactly, except replace the
/// previously-defaultless `param material : Material` declaration with a
/// concrete struct-call default. This is the user-visible payoff promised by
/// task 1876: "`param material : Material = Material(...)` is meaningful" —
/// end-to-end compilation must succeed against the full stdlib (so trait
/// refinements like `Rigid : Physical : MaterialSpec` are exercised), the
/// `material` member must resolve to `Type::StructureRef("Material")`, and the
/// default expression must be recorded as a `Material(...)` call. This guards
/// the entire pipeline (resolution + default typing + stdlib cascade) against
/// regressions before step-10 updates the example file itself.
#[test]
fn boltflange_compiles_with_material_default() {
    // Source intentionally mirrors the example layout one-for-one so that, if
    // the example evolves, a diff against this string makes the divergence
    // visible. The only change versus the on-disk example is line 22.
    let source = r#"
        structure def BoltFlange : Rigid {
            param outer_radius : Length = 60mm
            param height : Length = 12mm
            param hole_count : Int = 8
            param bolt_circle_radius : Length = 45mm
            param hole_radius : Length = 4mm

            // MaterialSpec trait requirements (density + name, inherited via Physical)
            param density : Real = 7850
            param name : String = "steel"

            // Physical trait requirements
            param volume : Real = 0.0001
            param centroid_x : Real = 0.0
            param centroid_y : Real = 0.0
            param centroid_z : Real = 0.0

            // Rigid trait requirements
            param moment_of_inertia : Real = 0.000001

            // Material reference (canonical struct default — task 1876 payoff)
            param material : Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)

            constraint outer_radius > bolt_circle_radius
            constraint hole_count > 0

            let body = cylinder(outer_radius, height)
            let hole = translate(cylinder(hole_radius, height), bolt_circle_radius, 0mm, 0mm)
            let holes = circular_pattern(hole, 0mm, 0mm, 0mm, 0, 0, 1, hole_count, 360deg)
            param geometry : Solid = difference(body, holes)
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected BoltFlange (with Material(...) default) to compile cleanly, got errors: {:?}",
        errors
    );

    let bolt_flange = module
        .templates
        .iter()
        .find(|t| t.name == "BoltFlange")
        .expect("BoltFlange template should be compiled");

    let material_cell = bolt_flange
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "material")
        .expect("BoltFlange.material should exist");

    assert_eq!(
        material_cell.cell_type,
        Type::StructureRef("Material".to_string()),
        "BoltFlange.material should resolve to Type::StructureRef(\"Material\"); got {:?}",
        material_cell.cell_type
    );

    let default_expr = material_cell.default_expr.as_ref().expect(
        "BoltFlange.material should carry the recorded `Material(...)` default — \
         the canonical struct default is the user-visible payoff for task 1876",
    );
    match &default_expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            assert_eq!(
                function.name, "Material",
                "BoltFlange.material default should be a `Material(...)` call, got {:?}",
                function.name
            );
            assert_eq!(
                args.len(),
                3,
                "BoltFlange.material default should carry 3 named args (name, density, \
                 youngs_modulus); got {}: {:?}",
                args.len(),
                args
            );
        }
        other => panic!(
            "expected BoltFlange.material.default_expr to be a FunctionCall for `Material(...)`, \
             got {:?}",
            other
        ),
    }
}
