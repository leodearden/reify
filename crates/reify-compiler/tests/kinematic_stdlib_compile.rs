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
use reify_ir::CompiledExprKind;

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

fn find_trait(name: &str) -> &'static reify_compiler::CompiledTrait {
    let module = load_stdlib_module();
    module
        .trait_defs
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| {
            panic!(
                "expected `trait {}` in std/kinematic, got: {:?}",
                name,
                module.trait_defs.iter().map(|t| &t.name).collect::<Vec<_>>()
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

// ─── Joint root marker trait (task 4310) ─────────────────────────────────────

/// `trait Joint {}` is declared as an empty marker — root of the joint hierarchy.
/// RED until step-2: kinematic.ri does not yet declare `trait Joint`.
#[test]
fn joint_is_empty_marker_trait() {
    let trait_def = find_trait("Joint");
    assert!(
        trait_def.required_members.is_empty() && trait_def.defaults.is_empty(),
        "Joint trait should be an empty marker (root joint hierarchy tag; no \
         members required), got requirements: {:?}, defaults: {:?}",
        trait_def.required_members.iter().map(|r| &r.name).collect::<Vec<_>>(),
        trait_def.defaults.iter().map(|d| &d.name).collect::<Vec<_>>(),
    );
}

/// `DrivingJoint` is declared as `: Joint` (refines Joint).
/// Coupling/Fixed conform to Joint but NOT DrivingJoint; the refinement makes
/// `satisfies_trait_bound(bounds, "Joint")` true for ALL joint kinds.
/// RED until step-2: DrivingJoint currently has no refinements.
#[test]
fn driving_joint_refines_joint() {
    let trait_def = find_trait("DrivingJoint");
    assert!(
        trait_def.refinements.contains(&"Joint".to_owned()),
        "DrivingJoint trait should refine Joint (declared as \
         `trait DrivingJoint : Joint {{}}`), got refinements: {:?}",
        trait_def.refinements
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
// to Type::dimensionless_scalar() here.

#[test]
fn cylindrical_has_one_vec3_axis_param() {
    // Narrowed by task 3849: Prismatic and Revolute now have 4 params (axis +
    // spring_rate + damping + neutral); only Cylindrical still has exactly 1.
    let template = find_structure("Cylindrical");
    let params = param_cells(template);
    assert_eq!(
        params.len(),
        1,
        "Cylindrical should have exactly 1 param (axis), got: {:?}",
        params.iter().map(|p| &p.id.member).collect::<Vec<_>>()
    );
    assert_eq!(
        params[0].id.member, "axis",
        "Cylindrical.axis param missing or misnamed"
    );
    assert_eq!(
        params[0].cell_type,
        Type::dimensionless_scalar(),
        "Cylindrical.axis should be Type::dimensionless_scalar() (Vec3 = Real alias, trajectory.ri:96)"
    );
}

// ─── task 3849 step-5: flexure field shape tests ──────────────────────────────

/// Revolute now has four params: axis (Vec3=Real), spring_rate
/// (Option<RotationalStiffness>), damping (Option<RotationalDamping>),
/// neutral (Option<Angle>). The three new params default to `none`.
#[test]
fn revolute_has_four_params_with_correct_types() {
    let template = find_structure("Revolute");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["axis", "spring_rate", "damping", "neutral"],
        "Revolute should have exactly (axis, spring_rate, damping, neutral) in that order"
    );

    // axis: Vec3 = Real alias
    assert_eq!(
        params[0].cell_type,
        Type::dimensionless_scalar(),
        "Revolute.axis should be Type::dimensionless_scalar() (Vec3 = Real alias)"
    );

    // spring_rate: Option<RotationalStiffness>
    assert_eq!(
        params[1].cell_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::ROTATIONAL_STIFFNESS
        })),
        "Revolute.spring_rate should be Option<RotationalStiffness>"
    );

    // damping: Option<RotationalDamping>
    assert_eq!(
        params[2].cell_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::ROTATIONAL_DAMPING
        })),
        "Revolute.damping should be Option<RotationalDamping>"
    );

    // neutral: Option<Angle>
    assert_eq!(
        params[3].cell_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::ANGLE
        })),
        "Revolute.neutral should be Option<Angle>"
    );

    // All three new params default to `= none` (CompiledExprKind::OptionNone).
    for (field_name, idx) in [("spring_rate", 1usize), ("damping", 2), ("neutral", 3)] {
        let default = params[idx]
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("Revolute.{field_name} missing default_expr"));
        assert!(
            matches!(default.kind, CompiledExprKind::OptionNone),
            "Revolute.{field_name} default should be OptionNone, got {:?}",
            default.kind
        );
    }
}

/// Prismatic now has four params: axis (Vec3=Real), spring_rate
/// (Option<TranslationalStiffness>), damping (Option<TranslationalDamping>),
/// neutral (Option<Length>). The three new params default to `none`.
#[test]
fn prismatic_has_four_params_with_correct_types() {
    let template = find_structure("Prismatic");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["axis", "spring_rate", "damping", "neutral"],
        "Prismatic should have exactly (axis, spring_rate, damping, neutral) in that order"
    );

    // axis: Vec3 = Real alias
    assert_eq!(
        params[0].cell_type,
        Type::dimensionless_scalar(),
        "Prismatic.axis should be Type::dimensionless_scalar() (Vec3 = Real alias)"
    );

    // spring_rate: Option<TranslationalStiffness>
    assert_eq!(
        params[1].cell_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::TRANSLATIONAL_STIFFNESS
        })),
        "Prismatic.spring_rate should be Option<TranslationalStiffness>"
    );

    // damping: Option<TranslationalDamping>
    assert_eq!(
        params[2].cell_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::TRANSLATIONAL_DAMPING
        })),
        "Prismatic.damping should be Option<TranslationalDamping>"
    );

    // neutral: Option<Length>
    assert_eq!(
        params[3].cell_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::LENGTH
        })),
        "Prismatic.neutral should be Option<Length>"
    );

    // All three new params default to `= none` (CompiledExprKind::OptionNone).
    for (field_name, idx) in [("spring_rate", 1usize), ("damping", 2), ("neutral", 3)] {
        let default = params[idx]
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("Prismatic.{field_name} missing default_expr"));
        assert!(
            matches!(default.kind, CompiledExprKind::OptionNone),
            "Prismatic.{field_name} default should be OptionNone, got {:?}",
            default.kind
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
            Type::dimensionless_scalar(),
            "Planar.{} should be Type::dimensionless_scalar() (Vec3 = Real alias, trajectory.ri:96)",
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
        Type::List(Box::new(Type::dimensionless_scalar())),
        "Mechanism.bodies should be Type::List(Real) (List<BodyId> placeholder)"
    );

    let jp = params.iter().find(|p| p.id.member == "joint_parents").unwrap();
    assert_eq!(
        jp.cell_type,
        Type::Map(Box::new(Type::String), Box::new(Type::dimensionless_scalar())),
        "Mechanism.joint_parents should be Type::Map(String, Real) \
         (Map<BodyId,JointParent> placeholder)"
    );

    let lc = params.iter().find(|p| p.id.member == "loop_closures").unwrap();
    assert_eq!(
        lc.cell_type,
        Type::List(Box::new(Type::dimensionless_scalar())),
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
        Type::List(Box::new(Type::dimensionless_scalar())),
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
        Type::dimensionless_scalar(),
        "SweepDim.joint should be Type::dimensionless_scalar() (DrivingJoint placeholder)"
    );

    let range = params.iter().find(|p| p.id.member == "range").unwrap();
    assert_eq!(
        range.cell_type,
        Type::dimensionless_scalar(),
        "SweepDim.range should be Type::dimensionless_scalar() (Range<T> not yet a surface type)"
    );

    let steps = params.iter().find(|p| p.id.member == "steps").unwrap();
    assert_eq!(
        steps.cell_type,
        Type::Int,
        "SweepDim.steps should be Type::Int"
    );
}

// ─── Coupling and Fixed: conform to Joint but NOT DrivingJoint ───────────────
//
// Updated by task 4310 (γ): Coupling and Fixed now carry `: Joint` so they
// appear in the joint hierarchy. They do NOT carry `: DrivingJoint` (no
// independent motion variable). After kinematic.ri step-2, trait_bounds == ["Joint"].
//
// RED until step-2: Coupling and Fixed currently have empty trait_bounds.

#[test]
fn coupling_and_fixed_are_declared_without_driving_joint() {
    let coupling = find_structure("Coupling");
    assert_eq!(
        coupling.trait_bounds,
        vec!["Joint".to_owned()],
        "Coupling should conform to Joint (root joint marker) but NOT \
         DrivingJoint (derived motion — no independent motion variable); \
         got trait_bounds: {:?}",
        coupling.trait_bounds
    );

    let fixed = find_structure("Fixed");
    assert_eq!(
        fixed.trait_bounds,
        vec!["Joint".to_owned()],
        "Fixed should conform to Joint (root joint marker) but NOT \
         DrivingJoint (0-DOF sub-assembly grouping — no motion variable at all); \
         got trait_bounds: {:?}",
        fixed.trait_bounds
    );
}

// ─── JointBinding and Twist marker structures (task 4310 γ) ──────────────────
//
// JointBinding — element type of snapshot()'s `bindings` argument (D8).
//   Declared now to make the type expressible; the actual `List<JointBinding>`
//   param typing of snapshot()'s bindings arg lands with β's signature family.
//
// Twist — spatial-velocity / joint-Jacobian column element.
//
// Both are empty marker structures (no params). They do NOT conform to
// Joint or DrivingJoint — they are not joint kinds.
//
// RED until step-4: decls not yet added to kinematic.ri.

#[test]
fn joint_binding_is_empty_marker_structure() {
    let template = find_structure("JointBinding");
    assert!(
        param_cells(template).is_empty(),
        "JointBinding should be an empty marker structure (no params); \
         got: {:?}",
        param_cells(template)
            .iter()
            .map(|p| &p.id.member)
            .collect::<Vec<_>>()
    );
    assert!(
        template.trait_bounds.is_empty(),
        "JointBinding should NOT conform to Joint or DrivingJoint \
         (it is a binding-record marker, not a joint kind); \
         got trait_bounds: {:?}",
        template.trait_bounds
    );
}

#[test]
fn twist_is_empty_marker_structure() {
    let template = find_structure("Twist");
    assert!(
        param_cells(template).is_empty(),
        "Twist should be an empty marker structure (no params); got: {:?}",
        param_cells(template)
            .iter()
            .map(|p| &p.id.member)
            .collect::<Vec<_>>()
    );
    assert!(
        template.trait_bounds.is_empty(),
        "Twist should NOT conform to Joint or DrivingJoint \
         (it is a spatial-velocity marker, not a joint kind); \
         got trait_bounds: {:?}",
        template.trait_bounds
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
