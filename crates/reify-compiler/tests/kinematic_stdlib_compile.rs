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

fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
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

// ─── Conforming joints — exhaustive data-driven partition ─────────────────────
//
// The conforming set is exactly these five; any future structure that silently
// gains or loses the DrivingJoint bound will break this test.

#[test]
fn conforming_joints_have_driving_joint_bound() {
    for name in &["Prismatic", "Revolute", "Cylindrical", "Planar", "Spherical"] {
        let template = find_structure(name);
        assert_eq!(
            template.trait_bounds,
            vec!["DrivingJoint"],
            "{} should conform to DrivingJoint",
            name
        );
    }
}

// ─── Field-shape assertions ───────────────────────────────────────────────────
//
// Catch regressions that delete a field or change its type to another
// still-resolvable type (e.g. Vec3→Int, dropping one of Planar's two axes).
// Vec3 and JointValue are `Real` aliases (trajectory.ri:96/76); they resolve
// to Type::Real here.

#[test]
fn axis_joints_have_one_vec3_axis_param() {
    for name in &["Prismatic", "Revolute", "Cylindrical"] {
        let template = find_structure(name);
        let params = param_cells(template);
        assert_eq!(
            params.len(),
            1,
            "{} should have exactly 1 param (axis), got: {:?}",
            name,
            params.iter().map(|p| &p.id.member).collect::<Vec<_>>()
        );
        assert_eq!(
            params[0].id.member, "axis",
            "{}.axis param missing or misnamed",
            name
        );
        assert_eq!(
            params[0].cell_type,
            Type::Real,
            "{}.axis should be Type::Real (Vec3 = Real alias, trajectory.ri:96)",
            name
        );
    }
}

#[test]
fn planar_has_two_vec3_axis_params() {
    let template = find_structure("Planar");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["axis_x", "axis_y"],
        "Planar should have exactly (axis_x, axis_y) in that order"
    );
    for p in &params {
        assert_eq!(
            p.cell_type,
            Type::Real,
            "Planar.{} should be Type::Real (Vec3 = Real alias, trajectory.ri:96)",
            p.id.member
        );
    }
}

#[test]
fn spherical_has_no_params() {
    let template = find_structure("Spherical");
    let params = param_cells(template);
    assert_eq!(
        params.len(),
        0,
        "Spherical should have no params (axis-isotropic — full SO(3)), \
         got: {:?}",
        params.iter().map(|p| &p.id.member).collect::<Vec<_>>()
    );
}

#[test]
fn mechanism_has_three_placeholder_params() {
    let template = find_structure("Mechanism");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["bodies", "joint_parents", "loop_closures"],
        "Mechanism should have exactly (bodies, joint_parents, loop_closures) in that order"
    );

    let bodies = params.iter().find(|p| p.id.member == "bodies").unwrap();
    assert_eq!(
        bodies.cell_type,
        Type::List(Box::new(Type::Real)),
        "Mechanism.bodies should be Type::List(Real) (List<BodyId> placeholder)"
    );

    let jp = params.iter().find(|p| p.id.member == "joint_parents").unwrap();
    assert_eq!(
        jp.cell_type,
        Type::Map(Box::new(Type::String), Box::new(Type::Real)),
        "Mechanism.joint_parents should be Type::Map(String, Real) \
         (Map<BodyId,JointParent> placeholder)"
    );

    let lc = params.iter().find(|p| p.id.member == "loop_closures").unwrap();
    assert_eq!(
        lc.cell_type,
        Type::List(Box::new(Type::Real)),
        "Mechanism.loop_closures should be Type::List(Real) \
         (List<LoopClosureRecord> placeholder)"
    );
}

#[test]
fn snapshot_has_correct_params() {
    let template = find_structure("Snapshot");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["free_values", "is_singular"],
        "Snapshot should have exactly (free_values, is_singular) in that order"
    );

    let fv = params.iter().find(|p| p.id.member == "free_values").unwrap();
    assert_eq!(
        fv.cell_type,
        Type::List(Box::new(Type::Real)),
        "Snapshot.free_values should be Type::List(Real) \
         (JointValue = Real alias, trajectory.ri:76)"
    );

    let is_sing = params.iter().find(|p| p.id.member == "is_singular").unwrap();
    assert_eq!(
        is_sing.cell_type,
        Type::Bool,
        "Snapshot.is_singular should be Type::Bool"
    );
}

#[test]
fn sweep_dim_has_correct_params() {
    let template = find_structure("SweepDim");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["joint", "range", "steps"],
        "SweepDim should have exactly (joint, range, steps) in that order"
    );

    let joint = params.iter().find(|p| p.id.member == "joint").unwrap();
    assert_eq!(
        joint.cell_type,
        Type::Real,
        "SweepDim.joint should be Type::Real (DrivingJoint placeholder)"
    );

    let range = params.iter().find(|p| p.id.member == "range").unwrap();
    assert_eq!(
        range.cell_type,
        Type::Real,
        "SweepDim.range should be Type::Real (Range<T> not yet a surface type)"
    );

    let steps = params.iter().find(|p| p.id.member == "steps").unwrap();
    assert_eq!(
        steps.cell_type,
        Type::Int,
        "SweepDim.steps should be Type::Int"
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
