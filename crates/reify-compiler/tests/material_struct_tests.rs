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
