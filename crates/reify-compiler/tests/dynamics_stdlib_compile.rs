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
use reify_ir::{BinOp, CompiledExprKind, Value};

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

    // Verify the constraint is specifically `mass >= 0kg` and not just any
    // unrelated constraint or one with a dimension-incompatible bare `0` RHS
    // (esc-3115: a bare 0 is Type::Real, which is dimension-incompatible with
    // the Mass-typed LHS and would compile differently).
    let cc = &template.constraints[0];
    let (op, left, right) = match &cc.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => (op, left.as_ref(), right.as_ref()),
        other => panic!(
            "expected `constraint mass >= 0kg` to compile to a BinOp expression, \
             got {:?}",
            other
        ),
    };

    assert_eq!(
        *op,
        BinOp::Ge,
        "mass constraint should use >= (BinOp::Ge), got {:?}",
        op
    );

    match &left.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.member, "mass",
                "constraint LHS should reference `mass`, got `{}`",
                id.member
            );
        }
        other => panic!(
            "expected constraint LHS to be a ValueRef to `mass`, got {:?}",
            other
        ),
    }

    match &right.kind {
        CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert!(
                si_value.abs() < 1e-12,
                "constraint RHS should be 0kg (si_value 0.0), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::MASS,
                "constraint RHS should be Mass-dimensioned (esc-3115: bare `0` would \
                 be Type::Real and dimension-incompatible with the Mass LHS), \
                 got {:?}",
                dimension
            );
        }
        other => panic!(
            "expected constraint RHS to be a Mass-dimensioned Scalar literal `0kg`, \
             got {:?}",
            other
        ),
    }
}

// ─── JointForceValue trait ────────────────────────────────────────────────────

#[test]
fn joint_force_value_is_empty_marker_trait() {
    let module = load_stdlib_module();
    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "JointForceValue")
        .expect("expected JointForceValue trait in std/dynamics");
    assert!(
        trait_def.required_members.is_empty() && trait_def.defaults.is_empty(),
        "JointForceValue trait should be an empty marker (body intentionally \
         empty; payload-carrying dispatch uses the SIR-α nominal type-tag), \
         got requirements: {:?}, defaults: {:?}",
        trait_def.required_members.iter().map(|r| &r.name).collect::<Vec<_>>(),
        trait_def.defaults.iter().map(|d| &d.name).collect::<Vec<_>>(),
    );
}

// ─── JointForceValue variants ─────────────────────────────────────────────────

#[test]
fn scalar_force_has_one_real_param_and_refines_joint_force_value() {
    let template = find_structure("ScalarForce");
    assert_eq!(
        template.trait_bounds,
        vec!["JointForceValue"],
        "ScalarForce should conform to JointForceValue"
    );
    let params = param_cells(template);
    assert_eq!(params.len(), 1, "ScalarForce should have exactly 1 param (magnitude)");
    let mag = params[0];
    assert_eq!(mag.id.member, "magnitude");
    assert_eq!(mag.cell_type, Type::Real, "ScalarForce.magnitude should be Type::Real");
}

#[test]
fn scalar_torque_has_one_real_param_and_refines_joint_force_value() {
    let template = find_structure("ScalarTorque");
    assert_eq!(
        template.trait_bounds,
        vec!["JointForceValue"],
        "ScalarTorque should conform to JointForceValue"
    );
    let params = param_cells(template);
    assert_eq!(params.len(), 1, "ScalarTorque should have exactly 1 param (magnitude)");
    let mag = params[0];
    assert_eq!(mag.id.member, "magnitude");
    assert_eq!(mag.cell_type, Type::Real, "ScalarTorque.magnitude should be Type::Real");
}

#[test]
fn cyl_force_has_list_real_param_and_refines_joint_force_value() {
    let template = find_structure("CylForce");
    assert_eq!(
        template.trait_bounds,
        vec!["JointForceValue"],
        "CylForce should conform to JointForceValue"
    );
    let params = param_cells(template);
    assert_eq!(params.len(), 1, "CylForce should have exactly 1 param (components)");
    let comp = params[0];
    assert_eq!(comp.id.member, "components");
    assert_eq!(
        comp.cell_type,
        Type::List(Box::new(Type::Real)),
        "CylForce.components should be Type::List(Real)"
    );
}

#[test]
fn planar_force_has_list_real_param_and_refines_joint_force_value() {
    let template = find_structure("PlanarForce");
    assert_eq!(
        template.trait_bounds,
        vec!["JointForceValue"],
        "PlanarForce should conform to JointForceValue"
    );
    let params = param_cells(template);
    assert_eq!(params.len(), 1, "PlanarForce should have exactly 1 param (components)");
    assert_eq!(params[0].id.member, "components");
    assert_eq!(
        params[0].cell_type,
        Type::List(Box::new(Type::Real)),
        "PlanarForce.components should be Type::List(Real)"
    );
}

#[test]
fn sphere_force_has_list_real_param_and_refines_joint_force_value() {
    let template = find_structure("SphereForce");
    assert_eq!(
        template.trait_bounds,
        vec!["JointForceValue"],
        "SphereForce should conform to JointForceValue"
    );
    let params = param_cells(template);
    assert_eq!(params.len(), 1, "SphereForce should have exactly 1 param (components)");
    assert_eq!(params[0].id.member, "components");
    assert_eq!(
        params[0].cell_type,
        Type::List(Box::new(Type::Real)),
        "SphereForce.components should be Type::List(Real)"
    );
}

#[test]
fn zero_force_has_no_params_and_refines_joint_force_value() {
    let template = find_structure("ZeroForce");
    assert_eq!(
        template.trait_bounds,
        vec!["JointForceValue"],
        "ZeroForce should conform to JointForceValue"
    );
    let params = param_cells(template);
    assert_eq!(
        params.len(),
        0,
        "ZeroForce should have zero params (zero-DOF marker), got: {:?}",
        params.iter().map(|p| &p.id.member).collect::<Vec<_>>()
    );
}

#[test]
fn joint_force_has_joint_id_and_value_params() {
    let template = find_structure("JointForce");
    let params = param_cells(template);
    assert_eq!(
        params.len(),
        2,
        "JointForce should have exactly 2 params (joint_id, value), got: {:?}",
        params.iter().map(|p| &p.id.member).collect::<Vec<_>>()
    );
    let joint_id = params.iter().find(|p| p.id.member == "joint_id")
        .expect("JointForce missing param 'joint_id'");
    assert_eq!(
        joint_id.cell_type,
        Type::Real,
        "JointForce.joint_id should be Type::Real (BodyId placeholder)"
    );
    let value = params.iter().find(|p| p.id.member == "value")
        .expect("JointForce missing param 'value'");
    assert_eq!(
        value.cell_type,
        Type::TraitObject("JointForceValue".to_string()),
        "JointForce.value should be Type::TraitObject(\"JointForceValue\")"
    );
}

// ─── TrajectorySample shape ───────────────────────────────────────────────────

#[test]
fn trajectory_sample_has_four_params_with_correct_types() {
    let template = find_structure("TrajectorySample");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["t", "values", "vels", "accels"],
        "TrajectorySample should have exactly (t, values, vels, accels) in that order"
    );

    // `t : Time` — Time dimension scalar
    let time_ty = Type::Scalar {
        dimension: DimensionVector::TIME,
    };
    // `values/vels/accels : List<JointValue>` — JointValue resolves to Real
    let list_real_ty = Type::List(Box::new(Type::Real));

    let expected: &[(&str, Type)] = &[
        ("t", time_ty),
        ("values", list_real_ty.clone()),
        ("vels", list_real_ty.clone()),
        ("accels", list_real_ty),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| panic!("TrajectorySample missing param '{}'", member));
        assert_eq!(
            cell.cell_type, *expected_ty,
            "TrajectorySample.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}

// ─── MotionTrajectory shape ───────────────────────────────────────────────────

#[test]
fn motion_trajectory_has_mechanism_and_samples_params() {
    let template = find_structure("MotionTrajectory");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["mechanism", "samples"],
        "MotionTrajectory should have exactly (mechanism, samples) in that order"
    );

    let mechanism = params.iter().find(|p| p.id.member == "mechanism")
        .expect("MotionTrajectory missing param 'mechanism'");
    assert_eq!(
        mechanism.cell_type,
        Type::Real,
        "MotionTrajectory.mechanism should be Type::Real (Mechanism placeholder)"
    );

    let samples = params.iter().find(|p| p.id.member == "samples")
        .expect("MotionTrajectory missing param 'samples'");
    assert_eq!(
        samples.cell_type,
        Type::List(Box::new(Type::StructureRef("TrajectorySample".to_string()))),
        "MotionTrajectory.samples should be Type::List(StructureRef(\"TrajectorySample\"))"
    );
}
