//! Tests for `crates/reify-compiler/stdlib/kinematic.ri` —
//! `std.kinematic` module: DrivingJoint marker trait, joint-kind structures
//! (Prismatic / Revolute / Cylindrical / Planar / Spherical), non-conforming
//! joints (Coupling / Fixed), and top-level types (Mechanism / Snapshot /
//! BodyId / SweepDim).
//!
//! Observable signal for PRD KCC-ζ (task 3845): the structures, trait, and
//! conformance declarations compile through the production stdlib path and
//! `TopologyTemplate.trait_bounds` carries the expected values.
//!
//! Joints stay `Value::Map` per PRD §7.1 (esc-3845-91); these are nominal
//! type-tags, not runtime carriers. units.rs / sweep.rs per-name hooks are
//! KEPT per esc-3845-91.
//!
//! Mirrors the `dynamics_stdlib_compile.rs` helper trio and discipline.

use reify_compiler::*;
use reify_core::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/kinematic")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/kinematic module; available paths: {:?}",
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
                "expected `structure def {}` in std/kinematic, got: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

// ─── module loads cleanly ────────────────────────────────────────────────────

#[test]
fn kinematic_module_loads_with_no_errors() {
    let module = load_stdlib_module();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in kinematic.ri: {:?}",
        errors
    );
}

// ─── DrivingJoint marker trait ────────────────────────────────────────────────

#[test]
fn driving_joint_is_empty_marker_trait() {
    let module = load_stdlib_module();
    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "DrivingJoint")
        .expect("expected DrivingJoint trait in std/kinematic");
    assert!(
        trait_def.required_members.is_empty() && trait_def.defaults.is_empty(),
        "DrivingJoint trait should be an empty marker (body intentionally \
         empty; joints stay Value::Map per PRD §7.1 — esc-3845-91), \
         got requirements: {:?}, defaults: {:?}",
        trait_def.required_members.iter().map(|r| &r.name).collect::<Vec<_>>(),
        trait_def.defaults.iter().map(|d| &d.name).collect::<Vec<_>>(),
    );
}

// ─── Prismatic conforms to DrivingJoint ──────────────────────────────────────

#[test]
fn prismatic_conforms_to_driving_joint() {
    let template = find_structure("Prismatic");
    assert_eq!(
        template.trait_bounds,
        vec!["DrivingJoint"],
        "Prismatic should conform to DrivingJoint"
    );
}

// ─── Remaining four conforming joint kinds ────────────────────────────────────

#[test]
fn revolute_conforms_to_driving_joint() {
    let template = find_structure("Revolute");
    assert_eq!(
        template.trait_bounds,
        vec!["DrivingJoint"],
        "Revolute should conform to DrivingJoint"
    );
}

#[test]
fn cylindrical_conforms_to_driving_joint() {
    let template = find_structure("Cylindrical");
    assert_eq!(
        template.trait_bounds,
        vec!["DrivingJoint"],
        "Cylindrical should conform to DrivingJoint"
    );
}

#[test]
fn planar_conforms_to_driving_joint() {
    let template = find_structure("Planar");
    assert_eq!(
        template.trait_bounds,
        vec!["DrivingJoint"],
        "Planar should conform to DrivingJoint"
    );
}

#[test]
fn spherical_conforms_to_driving_joint() {
    let template = find_structure("Spherical");
    assert_eq!(
        template.trait_bounds,
        vec!["DrivingJoint"],
        "Spherical should conform to DrivingJoint"
    );
}

// ─── Coupling and Fixed are NOT DrivingJoint ──────────────────────────────────

#[test]
fn coupling_and_fixed_are_declared_without_driving_joint() {
    let coupling = find_structure("Coupling");
    assert!(
        coupling.trait_bounds.is_empty(),
        "Coupling should NOT conform to DrivingJoint (derived motion — no \
         independent motion variable); got trait_bounds: {:?}",
        coupling.trait_bounds
    );

    let fixed = find_structure("Fixed");
    assert!(
        fixed.trait_bounds.is_empty(),
        "Fixed should NOT conform to DrivingJoint (0-DOF sub-assembly \
         grouping — no motion variable at all); got trait_bounds: {:?}",
        fixed.trait_bounds
    );
}

// ─── Top-level types exist and do not conform ─────────────────────────────────

#[test]
fn top_level_kinematic_types_exist_and_do_not_conform() {
    for name in &["Mechanism", "Snapshot", "BodyId", "SweepDim"] {
        let template = find_structure(name);
        assert!(
            template.trait_bounds.is_empty(),
            "{} should NOT conform to DrivingJoint (top-level container \
             type, not a joint kind); got trait_bounds: {:?}",
            name,
            template.trait_bounds
        );
    }
}
