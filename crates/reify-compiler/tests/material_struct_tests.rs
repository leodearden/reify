//! Tests for the canonical first-class `Material` struct defined in
//! `stdlib/materials_mechanical.ri` — task 1876.
//!
//! The task promotes `Material` from a trait to a concrete structure carrying
//! fields `name : String`, `density : Density`, and `youngs_modulus : Pressure`. The
//! original trait-level contract has been renamed to `MaterialSpec` so that
//! the name `Material` is free to bind the new struct. These tests exercise
//! the struct's surface: presence in the stdlib, type-resolution behaviour
//! for `param x : Material` (must pick the struct over any trait fallback),
//! struct-call defaults at param sites, the end-to-end BoltFlange case, and
//! a regression that the renamed `MaterialSpec` trait still works as a
//! trait-object param type (preserving the task-1874 pathway).

use reify_compiler::{EntityKind, stdlib_loader};
use reify_test_support::compile_source_with_stdlib;
use reify_core::{DimensionVector, Severity, Type};
use reify_ir::CompiledExprKind;

// ─── step-3: canonical Material struct is present in the stdlib ─────────────

/// The canonical `Material` struct must appear as a Structure template in the
/// stdlib with exactly three params — `name : String`, `density : Density`, and
/// `youngs_modulus : Pressure` — and none of the params may declare a default.
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
    // density → Density (DimensionVector::MASS_DENSITY), youngs_modulus → Pressure
    // (DimensionVector::PRESSURE), per task #3111 tightening.
    let expected: &[(&str, Type)] = &[
        ("name", Type::String),
        (
            "density",
            Type::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
            },
        ),
        (
            "youngs_modulus",
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        ),
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

    // SIR-α (task 3540, design-decision-2): a `structure def` constructor call
    // lowers to `CompiledExprKind::StructureInstanceCtor` (NOT a stdlib
    // `FunctionCall`) — the ctor path takes precedence over `eval_builtin`.
    // The original task-1876 intent is preserved: the struct-call default must
    // not be dropped, the callee is `Material`, and all three supplied values
    // survive (here as `ordered_args`, since they cover all three params).
    match &default_expr.kind {
        CompiledExprKind::StructureInstanceCtor {
            type_name,
            ordered_args,
            ..
        } => {
            assert_eq!(
                type_name, "Material",
                "default_expr should construct `Material`, got type_name={:?}",
                type_name
            );
            assert_eq!(
                ordered_args.len(),
                3,
                "Material(...) should lower to a ctor with 3 bound args, got {}: {:?}",
                ordered_args.len(),
                ordered_args
            );
        }
        other => panic!(
            "expected Part.material.default_expr to be a StructureInstanceCtor for \
             `Material(...)`, got {:?}",
            other
        ),
    }
}

// ─── step-9: end-to-end BoltFlange compiles with a Material(...) default ────

/// Mirror of `examples/m5_geometry_flange.ri`, used by both
/// `boltflange_compiles_with_material_default` and the self-enforcing
/// mirror check `boltflange_mirror_source_matches_example_file`.
///
/// **Mirroring contract** (reviewer #6 follow-up): this string MUST stay
/// in lock-step with `examples/m5_geometry_flange.ri`. The mirror-check
/// test below reads the on-disk example at test time and compares the
/// structural-body lines, so divergence between the embedded source and
/// the example is caught at `cargo test` time rather than via human
/// diffing.
const BOLTFLANGE_MIRROR_SOURCE: &str = r#"
        structure def BoltFlange : Rigid {
            param outer_radius : Length = 60mm
            param height : Length = 12mm
            param hole_count : Int = 8
            param bolt_circle_radius : Length = 45mm
            param hole_radius : Length = 4mm

            // Rigid trait requirement (Rigid's own param; Physical's geometry +
            // material slots are below). Disc OD 120 mm, h 12 mm, mass ≈ 0.86 kg → I_z ≈ 0.002 kg·m²
            param moment_of_inertia : MomentOfInertia = 0.002 * 1kg * 1m * 1m

            // Canonical Material struct default — the task-1876 payoff this
            // test pins (recorded StructureInstanceCtor with 3 bound args).
            param material : Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)

            constraint outer_radius > bolt_circle_radius
            constraint hole_count > 0

            let body = cylinder(outer_radius, height)
            let hole = translate(cylinder(hole_radius, height), bolt_circle_radius, 0mm, 0mm)
            let holes = circular_pattern(hole, 0mm, 0mm, 0mm, 0, 0, 1, hole_count, 360deg)
            param geometry : Solid = difference(body, holes)
        }
    "#;

/// **Self-enforcing mirror contract** (reviewer #6 follow-up). Reads
/// `examples/m5_geometry_flange.ri` at test time and asserts that every
/// distinctive param/let/constraint line of `BOLTFLANGE_MIRROR_SOURCE` is
/// present in the example file. Without this check, the doc-comment on
/// `boltflange_compiles_with_material_default` claimed the embedded source
/// "mirrors the example one-for-one" but enforcement was manual — a
/// divergence in the example would silently outdate the test fixture.
///
/// Comparison strategy: trim leading whitespace on each line, drop empty
/// lines and pure comment lines, and require each non-trivial line of the
/// embedded source to appear as a substring of the on-disk file. Tolerates
/// whitespace / comment / surrounding-text differences while still
/// catching any structural divergence (param renames, value changes,
/// trait-bound changes, etc.).
#[test]
fn boltflange_mirror_source_matches_example_file() {
    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/m5_geometry_flange.ri"
    );
    let example_src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/m5_geometry_flange.ri — check CARGO_MANIFEST_DIR resolution",
    );

    // Each distinctive line of the embedded source must appear (after
    // leading-whitespace trim) somewhere in the on-disk example.
    let mut missing: Vec<String> = Vec::new();
    for line in BOLTFLANGE_MIRROR_SOURCE.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Skip pure-comment lines and the bracketing braces — those don't
        // carry structural-body content.
        if trimmed.starts_with("//") || trimmed == "{" || trimmed == "}" {
            continue;
        }
        if !example_src.contains(trimmed) {
            missing.push(trimmed.to_string());
        }
    }

    assert!(
        missing.is_empty(),
        "BOLTFLANGE_MIRROR_SOURCE has diverged from examples/m5_geometry_flange.ri \
         — the following lines from the embedded mirror are not present in the on-disk \
         example: {:#?}. Either update the example to match the mirror, or update the \
         mirror (and the boltflange_compiles_with_material_default test) to match the \
         example. Mirror contract is documented at \
         `boltflange_compiles_with_material_default`.",
        missing
    );
}

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
    // Source intentionally mirrors `examples/m5_geometry_flange.ri`
    // one-for-one. Post-GHR-α (task 3603 / PRD §8 Phase 1) the example is
    // spec-shape `Rigid : Physical` — geometry + material struct slots; the
    // legacy flat `density/name/volume/centroid_x/y/z` params are gone
    // (`material : Material` now carries density via the struct,
    // `geometry : Solid` feeds the trait's `volume(geometry)` let).
    //
    // The mirroring contract is SELF-ENFORCING (reviewer #6 follow-up): the
    // `boltflange_mirror_source_matches_example_file` assertion below reads
    // the on-disk example at test time and verifies every distinctive
    // param/let line of `BOLTFLANGE_MIRROR_SOURCE` is present in the
    // example file. If the example evolves, this test fails — humans no
    // longer need to diff manually.
    let source = BOLTFLANGE_MIRROR_SOURCE;
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
    // SIR-α (task 3540, design-decision-2): the `Material(...)` struct default
    // lowers to a `StructureInstanceCtor`, not a stdlib `FunctionCall`. The
    // task-1876 payoff (canonical struct default is recorded, carrying all
    // three supplied values) is preserved against the new lowering shape.
    match &default_expr.kind {
        CompiledExprKind::StructureInstanceCtor {
            type_name,
            ordered_args,
            ..
        } => {
            assert_eq!(
                type_name, "Material",
                "BoltFlange.material default should construct `Material`, got {:?}",
                type_name
            );
            assert_eq!(
                ordered_args.len(),
                3,
                "BoltFlange.material default should carry 3 bound args (name, density, \
                 youngs_modulus); got {}: {:?}",
                ordered_args.len(),
                ordered_args
            );
        }
        other => panic!(
            "expected BoltFlange.material.default_expr to be a StructureInstanceCtor for \
             `Material(...)`, got {:?}",
            other
        ),
    }
}

// ─── step-11: MaterialSpec trait still usable as a trait-object param ────────

/// Regression guard for task 1874's trait-typed-param feature: now that the
/// original `Material` trait has been renamed to `MaterialSpec`, declaring a
/// `param m : MaterialSpec` must continue to resolve to `Type::TraitObject(
/// "MaterialSpec")` (NOT `StructureRef` — there is no struct named
/// `MaterialSpec`), and a struct that conforms to `MaterialSpec` must remain
/// a valid default value via the call-syntax form `SomeSteel()`. This locks in
/// that promoting `Material` to a struct (task 1876) did not regress the
/// trait-object pathway beneath the renamed trait.
#[test]
fn material_spec_trait_still_usable_as_trait_object() {
    let source = r#"
        structure def SomeSteel : MaterialSpec {
            param name : String = "steel"
            param density : Real = 7850.0
        }
        trait HasMat { param m : MaterialSpec }
        structure def Widget : HasMat {
            param m : MaterialSpec = SomeSteel()
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
        "expected no errors compiling Widget : HasMat with `param m : MaterialSpec = SomeSteel()`; \
         the renamed MaterialSpec trait must continue to function as a trait-object param type \
         (task 1874 pathway) — got: {:?}",
        errors
    );

    let widget = module
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template should be compiled");

    let m_cell = widget
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("Widget.m should exist");

    assert_eq!(
        m_cell.cell_type,
        Type::TraitObject("MaterialSpec".to_string()),
        "Widget.m should resolve to Type::TraitObject(\"MaterialSpec\") (NOT StructureRef — \
         MaterialSpec is a trait, not a struct); got {:?}",
        m_cell.cell_type
    );

    // The default expression `SomeSteel()` must lower to a
    // `StructureInstanceCtor` (SIR-α design-decision-2) whose constructed type
    // is `SomeSteel` — confirming the struct-constructor path survives as a
    // valid default for a trait-typed param (task-1874 pathway) under the new
    // SIR-α lowering.
    let default_expr = m_cell.default_expr.as_ref().expect(
        "Widget.m should carry the recorded `SomeSteel()` default — \
         struct-constructor call defaults must work for trait-typed params (task 1874)",
    );
    match &default_expr.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
            assert_eq!(
                type_name, "SomeSteel",
                "Widget.m default should construct `SomeSteel`, got {:?}",
                type_name
            );
        }
        other => panic!(
            "expected Widget.m.default_expr to be a StructureInstanceCtor for `SomeSteel()`, \
             got {:?}",
            other
        ),
    }
}
