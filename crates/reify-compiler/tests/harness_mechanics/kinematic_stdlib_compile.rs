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
                module
                    .trait_defs
                    .iter()
                    .map(|t| &t.name)
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
        trait_def
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>(),
        trait_def
            .defaults
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>(),
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
        trait_def
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>(),
        trait_def
            .defaults
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>(),
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
    // Single-DOF joints (Prismatic, Revolute) also conform to HasMotion (task #4605 ε):
    // their MotionValue associated type makes Coupling<P> projection-reducible.
    for name in &["Prismatic", "Revolute"] {
        let template = find_structure(name);
        assert!(
            template.trait_bounds.contains(&"DrivingJoint".to_owned()),
            "{name} should conform to DrivingJoint; got: {:?}",
            template.trait_bounds
        );
        assert!(
            template.trait_bounds.contains(&"HasMotion".to_owned()),
            "{name} should conform to HasMotion (single-DOF, task #4605 ε); got: {:?}",
            template.trait_bounds
        );
    }
    // Multi-DOF joints (Cylindrical, Planar, Spherical) conform to DrivingJoint
    // but NOT HasMotion (multi-DOF, out of scope for single-dimension MotionValue).
    for name in &["Cylindrical", "Planar", "Spherical"] {
        let template = find_structure(name);
        assert!(
            template.trait_bounds.contains(&"DrivingJoint".to_owned()),
            "{name} should conform to DrivingJoint; got: {:?}",
            template.trait_bounds
        );
        assert!(
            !template.trait_bounds.contains(&"HasMotion".to_owned()),
            "{name} should NOT conform to HasMotion (multi-DOF, out of scope); \
             got: {:?}",
            template.trait_bounds
        );
    }
}

// ─── Field-shape assertions ───────────────────────────────────────────────────
//
// Catch regressions that delete a field or change its type to another
// still-resolvable type (e.g. Vec3→Int, dropping one of Planar's two axes).
// Vec3<Q> is `Vector3<Q>` (task #4794); axis fields take Q = Dimensionless
// (direction, task 5848). JointValue is still
// a `Real` alias (trajectory.ri:76) and resolves to Type::dimensionless_scalar().

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
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS
        }),
        "Cylindrical.axis should be Type::vec3(Dimensionless) (direction field, task 5848)"
    );
}

// ─── task 3849 step-5: flexure field shape tests ──────────────────────────────

/// Revolute now has four params: axis (Vec3<Dimensionless>, task 5848), spring_rate
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

    // axis: Vec3<Dimensionless> — a direction, not a length (task 5848).
    // Full five-slot coverage lives in joint_direction_params_are_dimensionless_vec3.
    assert_eq!(
        params[0].cell_type,
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS
        }),
        "Revolute.axis should be Type::vec3(Dimensionless) (direction field, task 5848)"
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

/// Prismatic now has four params: axis (Vec3<Dimensionless>, task 5848), spring_rate
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

    // axis: Vec3<Dimensionless> — a direction, not a length (task 5848).
    // Full five-slot coverage lives in joint_direction_params_are_dimensionless_vec3.
    assert_eq!(
        params[0].cell_type,
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS
        }),
        "Prismatic.axis should be Type::vec3(Dimensionless) (direction field, task 5848)"
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
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::DIMENSIONLESS
            }),
            "Planar.{} should be Type::vec3(Dimensionless) (direction field, task 5848)",
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

/// `Mechanism` carries three params: `bodies` (still a `List<Real>` placeholder),
/// `joint_parents` (tightened to `Map<BodyId,JointParent>` by task 4579/M),
/// and `loop_closures` (tightened to `List<LoopClosure>` by task 4579/M).
///
/// `bodies : List<Real>` (TODO(body-type)) is INTENTIONALLY UNCHANGED — // ptodo:allow doc reference to a placeholder marker - not tracked debt
/// owned by the kinematic-completion/BodyId promotion line.
///
/// Resolution guards at the top verify that `BodyId` and `JointParent` are
/// actually declared structures — a string-equality StructureRef assertion
/// alone would pass even if the referenced name did not exist.
#[test]
fn mechanism_has_three_params_with_tightened_collection_types() {
    // Resolution guards: panic early (with a clear message) if the StructureRef
    // target names are not actually declared in std/kinematic.
    // - BodyId: Map key; no standalone find_structure test elsewhere in this file.
    // - JointParent: Map value; also covered by joint_parent_struct_has_correct_param_shape,
    //   but co-locating the guard makes the dependency explicit.
    find_structure("BodyId");
    find_structure("JointParent");

    let template = find_structure("Mechanism");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["bodies", "joint_parents", "loop_closures"],
        "Mechanism should have exactly (bodies, joint_parents, loop_closures) in that order"
    );

    // bodies: still a List<Real> placeholder (body-type marker — DO NOT change).
    let bodies = params.iter().find(|p| p.id.member == "bodies").unwrap();
    assert_eq!(
        bodies.cell_type,
        Type::List(Box::new(Type::dimensionless_scalar())),
        "Mechanism.bodies should be Type::List(Real) (List<BodyId> placeholder, \
         TODO(body-type) owned by kinematic-completion line)" // ptodo:allow doc reference to a placeholder marker - not tracked debt
    );

    // joint_parents: tightened to Map<BodyId, JointParent> by task 4579 (M).
    let jp = params
        .iter()
        .find(|p| p.id.member == "joint_parents")
        .unwrap();
    assert_eq!(
        jp.cell_type,
        Type::Map(
            Box::new(Type::StructureRef("BodyId".to_string())),
            Box::new(Type::StructureRef("JointParent".to_string())),
        ),
        "Mechanism.joint_parents should be Type::Map(StructureRef(\"BodyId\"), \
         StructureRef(\"JointParent\")); got: {:?}",
        jp.cell_type
    );

    // loop_closures: tightened to List<LoopClosure> by task 4579 (M).
    let lc = params
        .iter()
        .find(|p| p.id.member == "loop_closures")
        .unwrap();
    assert_eq!(
        lc.cell_type,
        Type::List(Box::new(Type::StructureRef("LoopClosure".to_string()))),
        "Mechanism.loop_closures should be Type::List(StructureRef(\"LoopClosure\")); \
         got: {:?}",
        lc.cell_type
    );
}

// ─── task 4579 (family M): LoopClosure nominal record type ───────────────────

/// `LoopClosure` is a closed-chain edge excluded from the spanning tree
/// (element of `Mechanism.loop_closures : List<LoopClosure>`).
///
/// Shape: exactly 4 params in canonical order (parent, child, joint,
/// residual_dim); no trait bound; no defaults; no structure-level constraints.
/// Introduced by task 4579 (family M).
///
/// RED until step-6: LoopClosure is not yet declared in kinematic.ri.
#[test]
fn loop_closure_struct_has_correct_param_shape() {
    let template = find_structure("LoopClosure");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) Plain record — no trait bound.
    assert!(
        template.trait_bounds.is_empty(),
        "LoopClosure should have no trait bounds (plain closed-chain edge record); \
         got: {:?}",
        template.trait_bounds
    );

    // (b) Exactly 4 params in canonical order.
    assert_eq!(
        names,
        vec!["parent", "child", "joint", "residual_dim"],
        "LoopClosure should have exactly (parent, child, joint, residual_dim) \
         in that order; got: {:?}",
        names
    );

    let parent = params.iter().find(|p| p.id.member == "parent").unwrap();
    assert_eq!(
        parent.cell_type,
        Type::StructureRef("BodyId".to_string()),
        "LoopClosure.parent should be Type::StructureRef(\"BodyId\"); got: {:?}",
        parent.cell_type
    );

    let child = params.iter().find(|p| p.id.member == "child").unwrap();
    assert_eq!(
        child.cell_type,
        Type::StructureRef("BodyId".to_string()),
        "LoopClosure.child should be Type::StructureRef(\"BodyId\"); got: {:?}",
        child.cell_type
    );

    let joint = params.iter().find(|p| p.id.member == "joint").unwrap();
    assert_eq!(
        joint.cell_type,
        Type::TraitObject("Joint".to_string()),
        "LoopClosure.joint should be Type::TraitObject(\"Joint\"); got: {:?}",
        joint.cell_type
    );

    let residual_dim = params
        .iter()
        .find(|p| p.id.member == "residual_dim")
        .unwrap();
    assert_eq!(
        residual_dim.cell_type,
        Type::Int,
        "LoopClosure.residual_dim should be Type::Int; got: {:?}",
        residual_dim.cell_type
    );

    // (c) No defaults.
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "LoopClosure.{} should have no default_expr; got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) No structure-level constraints.
    assert!(
        template.constraints.is_empty(),
        "LoopClosure should declare no structure-level constraints; got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── task 4579 (family M): JointParent nominal record type ───────────────────

/// `JointParent` is the value side of `Mechanism.joint_parents : Map<BodyId,JointParent>`.
/// It records a body's spanning-tree parent edge: which body is the parent
/// (`parent : BodyId`) and the joint connecting child→parent (`joint : Joint`).
///
/// Shape: exactly 2 params in canonical order (parent, joint); no trait bound
/// (plain value record, not a joint kind); no defaults (all caller-supplied);
/// no structure-level constraints. Introduced by task 4579 (family M).
///
/// RED until step-4: JointParent is not yet declared in kinematic.ri.
#[test]
fn joint_parent_struct_has_correct_param_shape() {
    let template = find_structure("JointParent");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) Plain record — no trait bound.
    assert!(
        template.trait_bounds.is_empty(),
        "JointParent should have no trait bounds (plain spanning-tree record, \
         not a Joint kind); got: {:?}",
        template.trait_bounds
    );

    // (b) Exactly 2 params in canonical order.
    assert_eq!(
        names,
        vec!["parent", "joint"],
        "JointParent should have exactly (parent, joint) in that order; got: {:?}",
        names
    );

    let parent = params.iter().find(|p| p.id.member == "parent").unwrap();
    assert_eq!(
        parent.cell_type,
        Type::StructureRef("BodyId".to_string()),
        "JointParent.parent should be Type::StructureRef(\"BodyId\"); got: {:?}",
        parent.cell_type
    );

    let joint = params.iter().find(|p| p.id.member == "joint").unwrap();
    assert_eq!(
        joint.cell_type,
        Type::TraitObject("Joint".to_string()),
        "JointParent.joint should be Type::TraitObject(\"Joint\"); got: {:?}",
        joint.cell_type
    );

    // (c) No defaults (all caller-supplied at mechanism construction).
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "JointParent.{} should have no default_expr; got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) No structure-level constraints.
    assert!(
        template.constraints.is_empty(),
        "JointParent should declare no structure-level constraints; got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
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

    let fv = params
        .iter()
        .find(|p| p.id.member == "free_values")
        .unwrap();
    assert_eq!(
        fv.cell_type,
        Type::List(Box::new(Type::dimensionless_scalar())),
        "Snapshot.free_values should be Type::List(Real) \
         (JointValue = Real alias, trajectory.ri:76)"
    );

    let is_sing = params
        .iter()
        .find(|p| p.id.member == "is_singular")
        .unwrap();
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

    // range: Range<JointValue> (JointValue = Real = dimensionless; tightened by task 4576).
    // RED until step-4 changes kinematic.ri param type to Range<JointValue>.
    let range = params.iter().find(|p| p.id.member == "range").unwrap();
    assert_eq!(
        range.cell_type,
        Type::Range(Box::new(Type::dimensionless_scalar())),
        "SweepDim.range should be Type::Range(dimensionless) (Range<JointValue>, task 4576)"
    );
    // Boundary guards: a Range literal is accepted; a bare scalar is rejected.
    assert!(
        type_compatible(&range.cell_type, &Type::range(Type::dimensionless_scalar())),
        "SweepDim.range must accept a Range<JointValue> value (type_compatible == true)",
    );
    assert!(
        !type_compatible(&range.cell_type, &Type::dimensionless_scalar()),
        "SweepDim.range must reject a bare Real/JointValue scalar (type_compatible == false)",
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
    // Coupling now also conforms to HasMotion (generic Coupling<P>, task #4605 ε).
    assert!(
        coupling.trait_bounds.contains(&"Joint".to_owned()),
        "Coupling should conform to Joint (root joint marker); \
         got trait_bounds: {:?}",
        coupling.trait_bounds
    );
    assert!(
        coupling.trait_bounds.contains(&"HasMotion".to_owned()),
        "Coupling should conform to HasMotion (generic Coupling<P>, task #4605 ε); \
         got trait_bounds: {:?}",
        coupling.trait_bounds
    );
    assert!(
        !coupling.trait_bounds.contains(&"DrivingJoint".to_owned()),
        "Coupling should NOT conform to DrivingJoint (derived motion — no \
         independent motion variable); got trait_bounds: {:?}",
        coupling.trait_bounds
    );

    let fixed = find_structure("Fixed");
    assert_eq!(
        fixed.trait_bounds,
        vec!["Joint".to_owned()],
        "Fixed should conform to Joint (root joint marker) but NOT \
         DrivingJoint (0-DOF sub-assembly grouping — no motion variable at all) \
         and NOT HasMotion (no motion dimension); got trait_bounds: {:?}",
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

// ─── JacobianColumn structure (task 6102) ─────────────────────────────────────

/// `joint_jacobian` returns its own nominal structure, `JacobianColumn` — NOT
/// `Twist`. A Jacobian column is the partial derivative of pose with respect to
/// a joint coordinate (dpose/dq); a twist is a spatial velocity. The two were
/// shape-punned onto one nominal tag; task 6102 splits them.
///
/// Unlike `Twist`, `JacobianColumn` DECLARES its two members, because compile-time
/// member access is gated on a structure's declared params: against an empty
/// marker, `joint_jacobian(rev).angular` fails with
/// `structure 'Twist' has no member 'angular'`.
#[test]
fn jacobian_column_declares_angular_and_linear_params() {
    let template = find_structure("JacobianColumn");
    let params = param_cells(template);

    let mut names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["angular", "linear"],
        "JacobianColumn should declare exactly (angular, linear) — the two keys the \
         runtime `Value::Map` actually carries on every joint arm"
    );

    // Both components are carried unit-tagless today: measured, every arm
    // (prismatic / revolute / screw / gear / rack_and_pinion / cylindrical)
    // emits a bare untagged `vec(...)`. The TRUE dpose/dq dimensions are
    // heterogeneous across joint kinds (revolute: linear is m/rad; prismatic:
    // angular is rad/m), so no monomorphic field type can be true for every
    // arm — see the declaration's doc comment in kinematic.ri.
    for param in &params {
        assert_eq!(
            param.cell_type,
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::DIMENSIONLESS
            }),
            "JacobianColumn.{} should be Type::vec3(Dimensionless) — the components \
             are carried unit-tagless because dpose/dq dimensions differ per joint kind",
            param.id.member
        );
    }

    assert!(
        template.trait_bounds.is_empty(),
        "JacobianColumn should NOT conform to Joint or DrivingJoint \
         (it is a dpose/dq partial-derivative column marker, not a joint kind \
         and not a spatial velocity); got trait_bounds: {:?}",
        template.trait_bounds
    );

    // Regression: `Twist` is NOT renamed, deleted, or repurposed by task 6102.
    // It stays the `transform_log` / `transform_exp` type, and is the subject of
    // the separate narrowings in tasks 6080 (angular : Vector3<Angle>) and
    // 6126 (linear : Vector3<Length>) — narrowings that are exactly what would
    // make the old `joint_jacobian -> Twist` claim false.
    let twist = find_structure("Twist");
    assert_eq!(
        twist.name, "Twist",
        "Twist must survive alongside JacobianColumn (transform_log / transform_exp type)"
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

// ─── task #4605 ε: HasMotion / MotionValue / generic Coupling ─────────────────
//
// RED until step-3 edits kinematic.ri.

/// `trait HasMotion { type MotionValue }` is declared as a required associated
/// type — no default.  The trait body has exactly one `RequirementKind::AssocType(None)`
/// member named "MotionValue" and no defaults.
#[test]
fn has_motion_trait_declares_required_assoc_type_motion_value() {
    let trait_def = find_trait("HasMotion");

    // Exactly one required member: AssocType("MotionValue").
    assert_eq!(
        trait_def.required_members.len(),
        1,
        "HasMotion must declare exactly 1 required member; got: {:?}",
        trait_def
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let req = &trait_def.required_members[0];
    assert_eq!(
        req.name, "MotionValue",
        "the required member must be named 'MotionValue'; got: {:?}",
        req.name
    );
    assert!(
        matches!(req.kind, RequirementKind::AssocType(None)),
        "the requirement kind must be AssocType(None) (required, no default); \
         got: {:?}",
        req.kind
    );

    // No defaults (HasMotion declares no default binding for MotionValue).
    assert!(
        trait_def.defaults.is_empty(),
        "HasMotion must have no defaults; got: {:?}",
        trait_def
            .defaults
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
}

/// Prismatic conforms to both DrivingJoint and HasMotion and carries an
/// `assoc_types` entry for `MotionValue` resolved to `Type::length()`.
#[test]
fn prismatic_conforms_to_has_motion_with_length_motion_value() {
    let template = find_structure("Prismatic");

    // trait_bounds contains both DrivingJoint and HasMotion.
    assert!(
        template.trait_bounds.contains(&"DrivingJoint".to_owned()),
        "Prismatic must have DrivingJoint in trait_bounds; got: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"HasMotion".to_owned()),
        "Prismatic must have HasMotion in trait_bounds; got: {:?}",
        template.trait_bounds
    );

    // assoc_types carries MotionValue resolved to Type::length().
    let entry = template
        .assoc_types
        .iter()
        .find(|a| a.type_name == "MotionValue")
        .unwrap_or_else(|| {
            panic!(
                "Prismatic must carry an assoc_types entry for MotionValue; \
                 assoc_types = {:?}",
                template.assoc_types
            )
        });
    assert_eq!(
        entry.resolved,
        Type::length(),
        "Prismatic::MotionValue must resolve to Type::length(); got: {:?}",
        entry.resolved
    );
}

/// Revolute conforms to both DrivingJoint and HasMotion and carries an
/// `assoc_types` entry for `MotionValue` resolved to `Type::angle()`.
#[test]
fn revolute_conforms_to_has_motion_with_angle_motion_value() {
    let template = find_structure("Revolute");

    // trait_bounds contains both DrivingJoint and HasMotion.
    assert!(
        template.trait_bounds.contains(&"DrivingJoint".to_owned()),
        "Revolute must have DrivingJoint in trait_bounds; got: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"HasMotion".to_owned()),
        "Revolute must have HasMotion in trait_bounds; got: {:?}",
        template.trait_bounds
    );

    // assoc_types carries MotionValue resolved to Type::angle().
    let entry = template
        .assoc_types
        .iter()
        .find(|a| a.type_name == "MotionValue")
        .unwrap_or_else(|| {
            panic!(
                "Revolute must carry an assoc_types entry for MotionValue; \
                 assoc_types = {:?}",
                template.assoc_types
            )
        });
    assert_eq!(
        entry.resolved,
        Type::angle(),
        "Revolute::MotionValue must resolve to Type::angle(); got: {:?}",
        entry.resolved
    );
}

/// Coupling is generic: `type_params` has exactly one entry named "P" whose
/// bounds include both "DrivingJoint" and "HasMotion".
/// Its `trait_bounds == ["Joint", "HasMotion"]`.
/// Its `assoc_types` entry for "MotionValue" has `resolved ==
/// Type::projection(Type::TypeParam("P".into()), "MotionValue")` (symbolic).
#[test]
fn coupling_is_generic_with_driving_joint_and_has_motion_bound() {
    let template = find_structure("Coupling");

    // Exactly one type parameter named "P".
    assert_eq!(
        template.type_params.len(),
        1,
        "Coupling must have exactly 1 type parameter; got: {:?}",
        template
            .type_params
            .iter()
            .map(|tp| &tp.name)
            .collect::<Vec<_>>()
    );
    let p_param = &template.type_params[0];
    assert_eq!(
        p_param.name, "P",
        "the type parameter must be named 'P'; got: {:?}",
        p_param.name
    );

    // P's bounds include both DrivingJoint and HasMotion.
    let bound_names: Vec<&str> = p_param
        .bounds
        .iter()
        .map(|b| b.trait_ref.name.as_str())
        .collect();
    assert!(
        bound_names.contains(&"DrivingJoint"),
        "P's bounds must include DrivingJoint; got: {:?}",
        bound_names
    );
    assert!(
        bound_names.contains(&"HasMotion"),
        "P's bounds must include HasMotion; got: {:?}",
        bound_names
    );

    // Coupling's trait_bounds == ["Joint", "HasMotion"].
    assert!(
        template.trait_bounds.contains(&"Joint".to_owned()),
        "Coupling must conform to Joint; got trait_bounds: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"HasMotion".to_owned()),
        "Coupling must conform to HasMotion; got trait_bounds: {:?}",
        template.trait_bounds
    );
    assert!(
        !template.trait_bounds.contains(&"DrivingJoint".to_owned()),
        "Coupling must NOT conform to DrivingJoint; got trait_bounds: {:?}",
        template.trait_bounds
    );

    // assoc_types carries MotionValue resolved to the symbolic Projection.
    let entry = template
        .assoc_types
        .iter()
        .find(|a| a.type_name == "MotionValue")
        .unwrap_or_else(|| {
            panic!(
                "Coupling must carry an assoc_types entry for MotionValue; \
                 assoc_types = {:?}",
                template.assoc_types
            )
        });
    assert_eq!(
        entry.resolved,
        Type::projection(Type::TypeParam("P".into()), "MotionValue"),
        "Coupling::MotionValue must store the symbolic \
         Projection{{TypeParam(P), MotionValue}} (unreduced, build-side); \
         got: {:?}",
        entry.resolved
    );
}

// ─── task 5848: joint direction params are DIMENSIONLESS ──────────────────────

/// The five joint direction params — `Prismatic.axis`, `Revolute.axis`,
/// `Cylindrical.axis`, `Planar.axis_x`, `Planar.axis_y` — resolve to a
/// DIMENSIONLESS 3-vector, not `Vec3<Length>`.
///
/// Asserts on the RESOLVED `Type`, never on the source spelling, so it is
/// immune to whether the field is written `Vec3<Dimensionless>` or
/// `Vector3<Dimensionless>` (both resolve to the same internal type).
///
/// This is a correction of a live decl/runtime divergence, not a cosmetic
/// change: `reify-stdlib/src/helpers.rs validate_dimensionless_unit_axis_vec3`
/// REJECTS a non-dimensionless axis outright, and gates the joint constructors
/// in joints.rs — so `revolute(vec3(0mm,0mm,1mm), …)` already evaluated to
/// `Value::Undef` while kinematic.ri declared `Vec3<Length>`.
#[test]
fn joint_direction_params_are_dimensionless_vec3() {
    let dimensionless_vec3 = Type::vec3(Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    });

    for (structure_name, field) in [
        ("Prismatic", "axis"),
        ("Revolute", "axis"),
        ("Cylindrical", "axis"),
        ("Planar", "axis_x"),
        ("Planar", "axis_y"),
    ] {
        let template = find_structure(structure_name);
        let params = param_cells(template);
        let p = params
            .iter()
            .find(|p| p.id.member == field)
            .unwrap_or_else(|| panic!("{structure_name}.{field} param must exist"));
        assert_eq!(
            p.cell_type, dimensionless_vec3,
            "{structure_name}.{field} denotes a DIRECTION, so its quantity slot must be \
             dimensionless (task 5848); got: {:?}",
            p.cell_type
        )
    }
}

/// FENCE for `joint_direction_params_are_dimensionless_vec3`: the retype must
/// touch ONLY the direction slots. The compliant-joint fields carry genuine
/// physical dimensions and must keep them — proving dimensionlessness was not
/// smeared across the whole structure.
#[test]
fn compliant_joint_fields_keep_their_dimensions() {
    let prismatic = param_cells(find_structure("Prismatic"));
    let neutral = prismatic
        .iter()
        .find(|p| p.id.member == "neutral")
        .expect("Prismatic.neutral param must exist");
    assert_eq!(
        neutral.cell_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::LENGTH
        })),
        "Prismatic.neutral is a rest POSITION, so it stays Option<Length>; got: {:?}",
        neutral.cell_type
    );

    let revolute = param_cells(find_structure("Revolute"));
    let spring_rate = revolute
        .iter()
        .find(|p| p.id.member == "spring_rate")
        .expect("Revolute.spring_rate param must exist");
    assert_eq!(
        spring_rate.cell_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::ROTATIONAL_STIFFNESS
        })),
        "Revolute.spring_rate stays Option<RotationalStiffness>; got: {:?}",
        spring_rate.cell_type
    )
}
