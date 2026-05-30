//! Tests for `crates/reify-compiler/stdlib/dynamics.ri` —
//! `std.dynamics` module: MassProperties, JointForceValue trait family,
//! TrajectorySample, MotionTrajectory.
//!
//! Observable signal for PRD RBD-α (task 3822): the structures, trait, and
//! sub-family compile through the production stdlib path and their param cells
//! carry the expected types.
//!
//! Mirrors the `constitutive_stdlib_compile.rs` helper trio and discipline.

use reify_compiler::*;
use reify_core::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/dynamics")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/dynamics module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` in std/dynamics, got: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
}

// ─── module loads cleanly ────────────────────────────────────────────────────

#[test]
fn std_dynamics_module_loads_with_no_errors() {
    let module = load_stdlib_module();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in dynamics.ri: {:?}",
        errors
    );
}

// ─── MassProperties shape ─────────────────────────────────────────────────────

#[test]
fn mass_properties_has_four_params_with_correct_types() {
    let template = find_structure("MassProperties");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["mass", "com", "inertia", "origin"],
        "MassProperties should have exactly (mass, com, inertia, origin) in that order"
    );

    let mass_ty = Type::Scalar {
        dimension: DimensionVector::MASS,
    };
    let length_scalar = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let com_ty = Type::Point {
        n: 3,
        quantity: Box::new(length_scalar),
    };
    let inertia_ty = Type::Matrix {
        m: 3,
        n: 3,
        quantity: Box::new(Type::Real),
    };
    let origin_ty = Type::Real;

    let expected: &[(&str, Type)] = &[
        ("mass", mass_ty),
        ("com", com_ty),
        ("inertia", inertia_ty),
        ("origin", origin_ty),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| panic!("MassProperties missing param '{}'", member));
        assert_eq!(
            cell.cell_type, *expected_ty,
            "MassProperties.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}

#[test]
fn mass_properties_has_mass_non_negativity_constraint() {
    let template = find_structure("MassProperties");
    assert!(
        !template.constraints.is_empty(),
        "MassProperties should carry the `constraint mass >= 0kg` bound, \
         but template.constraints is empty"
    );
}
