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
use reify_test_support::compile_source_with_stdlib;

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

/// MassProperties has exactly (mass, com, inertia, origin) with the expected
/// types. `origin` is retargeted from the old `Real` placeholder to the real
/// `Frame3` struct (declared in std.ports, shipped constructible by task 4254;
/// std.ports was moved before std.dynamics in the loader by task 4547 pre-1 so
/// Frame3 resolves in the growing prelude). A nominal struct param compiles to
/// `Type::StructureRef` — cf. `analysis_stress_fn_compile.rs`'s `StressInvariants`.
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
        quantity: Box::new(Type::Scalar {
            dimension: DimensionVector::MOMENT_OF_INERTIA,
        }),
    };
    let origin_ty = Type::StructureRef("Frame3".to_string());

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
    // (esc-3115: a bare 0 is Type::dimensionless_scalar(), which is dimension-incompatible with
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
                 be Type::dimensionless_scalar() and dimension-incompatible with the Mass LHS), \
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

// Closed-set pin only: per-structure trait-bound conformance is already
// asserted individually by the sibling tests below (e.g.
// `scalar_force_has_one_real_param_and_refines_joint_force_value`); this
// test's sole new signal is that the refinement set is closed at exactly
// six members. The scan is deliberately scoped to std/dynamics only (via
// `load_stdlib_module()`) — a refiner declared in another stdlib module
// would not be caught here.
#[test]
fn joint_force_value_refinement_set_is_exactly_six_structures() {
    let module = load_stdlib_module();
    let mut refiners: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| {
            t.entity_kind == EntityKind::Structure
                && t.trait_bounds
                    .iter()
                    .any(|b| b.as_str() == "JointForceValue")
        })
        .map(|t| t.name.as_str())
        .collect();
    refiners.sort();

    assert_eq!(
        refiners,
        vec![
            "CylForce",
            "PlanarForce",
            "ScalarForce",
            "ScalarTorque",
            "SphereForce",
            "ZeroForce",
        ],
        "JointForceValue's refinement set is the marker-trait encoding of the \
         PRD's tagged union (see the TAGGED-UNION NOTE above `trait \
         JointForceValue`) and must stay exactly these six structure_defs; \
         adding, removing, or migrating a member to a data-carrying enum must \
         be a deliberate, reviewed change, not silent drift — got {:?}",
        refiners
    );
}

// ─── JointForceValue variants ─────────────────────────────────────────────────

#[test]
fn scalar_force_has_one_force_param_and_refines_joint_force_value() {
    let template = find_structure("ScalarForce");
    assert_eq!(
        template.trait_bounds,
        vec!["JointForceValue"],
        "ScalarForce should conform to JointForceValue"
    );
    let params = param_cells(template);
    assert_eq!(
        params.len(),
        1,
        "ScalarForce should have exactly 1 param (magnitude)"
    );
    let mag = params[0];
    assert_eq!(mag.id.member, "magnitude");
    assert_eq!(
        mag.cell_type,
        Type::Scalar {
            dimension: DimensionVector::FORCE,
        },
        "ScalarForce.magnitude carries the joint force as a dimensioned quantity \
         (task 6098): it should be Type::Scalar{{ dimension: FORCE }}, not the \
         former dimensionless Real"
    );
}

#[test]
fn scalar_torque_has_one_torque_param_and_refines_joint_force_value() {
    let template = find_structure("ScalarTorque");
    assert_eq!(
        template.trait_bounds,
        vec!["JointForceValue"],
        "ScalarTorque should conform to JointForceValue"
    );
    let params = param_cells(template);
    assert_eq!(
        params.len(),
        1,
        "ScalarTorque should have exactly 1 param (magnitude)"
    );
    let mag = params[0];
    assert_eq!(mag.id.member, "magnitude");
    // TORQUE carries an Angle⁻¹ slot (N·m/rad) and is deliberately distinct
    // from ENERGY (= Force·Length) — see reify-core dimension.rs. Assert the
    // registry vector directly; never compose FORCE·LENGTH here.
    assert_eq!(
        mag.cell_type,
        Type::Scalar {
            dimension: DimensionVector::TORQUE,
        },
        "ScalarTorque.magnitude carries the joint torque as a dimensioned \
         quantity (task 6098): it should be Type::Scalar{{ dimension: TORQUE }}, \
         not the former dimensionless Real"
    );
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
    assert_eq!(
        params.len(),
        1,
        "CylForce should have exactly 1 param (components)"
    );
    let comp = params[0];
    assert_eq!(comp.id.member, "components");
    assert_eq!(
        comp.cell_type,
        Type::List(Box::new(Type::dimensionless_scalar())),
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
    assert_eq!(
        params.len(),
        1,
        "PlanarForce should have exactly 1 param (components)"
    );
    assert_eq!(params[0].id.member, "components");
    assert_eq!(
        params[0].cell_type,
        Type::List(Box::new(Type::dimensionless_scalar())),
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
    assert_eq!(
        params.len(),
        1,
        "SphereForce should have exactly 1 param (components)"
    );
    assert_eq!(params[0].id.member, "components");
    assert_eq!(
        params[0].cell_type,
        Type::List(Box::new(Type::dimensionless_scalar())),
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
    let joint_id = params
        .iter()
        .find(|p| p.id.member == "joint_id")
        .expect("JointForce missing param 'joint_id'");
    assert_eq!(
        joint_id.cell_type,
        Type::dimensionless_scalar(),
        "JointForce.joint_id should be Type::dimensionless_scalar() (BodyId placeholder)"
    );
    let value = params
        .iter()
        .find(|p| p.id.member == "value")
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
    let list_real_ty = Type::List(Box::new(Type::dimensionless_scalar()));

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

    let mechanism = params
        .iter()
        .find(|p| p.id.member == "mechanism")
        .expect("MotionTrajectory missing param 'mechanism'");
    assert_eq!(
        mechanism.cell_type,
        Type::dimensionless_scalar(),
        "MotionTrajectory.mechanism should be Type::dimensionless_scalar() (Mechanism placeholder)"
    );

    let samples = params
        .iter()
        .find(|p| p.id.member == "samples")
        .expect("MotionTrajectory missing param 'samples'");
    assert_eq!(
        samples.cell_type,
        Type::List(Box::new(Type::StructureRef("TrajectorySample".to_string()))),
        "MotionTrajectory.samples should be Type::List(StructureRef(\"TrajectorySample\"))"
    );
}

// ─── Task 4278 — dynamics-constructor compile-typing (step-9 RED) ────────────

/// Task 4278 step-9 (RED). `point_mass(mass)` and
/// `mass_properties(mass, com, inertia)` are dynamics-constructor builtins
/// (task 4278, DYNAMICS_CONSTRUCTOR_NAMES family). A `.ri` let cell assigned
/// from either call must type as `Type::StructureRef("MassProperties")`, NOT
/// the first-arg fallback — `Scalar<Mass>` for `point_mass(2.5kg)` — which
/// would trip `value_type_kind_matches` at eval time. RED until step-10 adds
/// `DYNAMICS_CONSTRUCTOR_NAMES`, `is_dynamics_constructor`, and the
/// `is_dynamics_constructor` arm in the `NoUserFunctions` ladder of
/// `expr.rs::infer_type`. Mirrors
/// `body_mass_props_resolves_to_function_call_returning_mass_properties`
/// (expr.rs unit test) for the ctor-family names.
#[test]
fn point_mass_and_mass_properties_ctors_type_as_mass_properties_struct_ref() {
    // ── point_mass(mass) → StructureRef("MassProperties") ──────────────────
    {
        let source = r#"
structure def Probe {
    let pm = point_mass(2.5kg)
}
"#;
        let compiled = compile_source_with_stdlib(source);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "point_mass Probe should compile without errors; got: {:?}",
            errors
        );

        let probe = compiled
            .templates
            .iter()
            .find(|t| t.name == "Probe")
            .expect("Probe template should be present in compiled module");

        let pm = probe
            .value_cells
            .iter()
            .find(|vc| vc.id.member == "pm")
            .expect("Probe.pm cell should exist");

        assert_eq!(
            pm.cell_type,
            Type::StructureRef("MassProperties".to_string()),
            "point_mass(2.5kg) cell should type as StructureRef(\"MassProperties\"), \
             NOT the first-arg fallback Scalar<Mass>; got {:?}",
            pm.cell_type
        );
    }

    // ── mass_properties(mass, com, inertia) → StructureRef("MassProperties") ─
    {
        let source = r#"
structure def Probe {
    let mp = mass_properties(2.5kg, [0m, 0m, 0m], [[0, 0, 0], [0, 0, 0], [0, 0, 0]])
}
"#;
        let compiled = compile_source_with_stdlib(source);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "mass_properties Probe should compile without errors; got: {:?}",
            errors
        );

        let probe = compiled
            .templates
            .iter()
            .find(|t| t.name == "Probe")
            .expect("Probe template should be present in compiled module");

        let mp = probe
            .value_cells
            .iter()
            .find(|vc| vc.id.member == "mp")
            .expect("Probe.mp cell should exist");

        assert_eq!(
            mp.cell_type,
            Type::StructureRef("MassProperties".to_string()),
            "mass_properties(...) cell should type as StructureRef(\"MassProperties\"), \
             NOT the first-arg fallback; got {:?}",
            mp.cell_type
        );
    }
}

// ─── Task 6098 — dimensioned ScalarForce/ScalarTorque magnitude ──────────────
//
// The user-observable signal of retyping `ScalarForce.magnitude : Real` to
// `: Force` (and `ScalarTorque.magnitude` to `: Torque`) is what the
// *expression* dimension checker says when the field is READ:
//
//   before: `f.magnitude + 1.0N`  → "dimension mismatch in addition: Real vs
//                                    Scalar[m·kg·s^-2]"; `+ 1.0` was clean.
//   after:  `f.magnitude + 1.0N`  → clean; `+ 1.0` is the mismatch.
//
// These tests assert on the READ, never on the construction: what makes the
// read `Force`-typed is the declared param type, not the argument. The
// fixtures nonetheless construct with dimensioned literals
// (`ScalarForce(magnitude: 2.5N)`) so the exemplar source is eval-sound as
// well as compile-clean — a bare `2.5` in that slot is accepted silently by
// the compiler but leaves a `Value::Real` in a dimensioned field, which
// evaluates to `undef` with an OpContractViolation the moment the very
// expression these tests certify is actually run. Constructor argument
// strictness (i.e. rejecting that bare `2.5` at compile time) is owned by
// docs/prds/v0_6/dimensioned-construction-strictness.md and is deliberately
// out of scope here; that unchecked-ctor-arg limitation is recorded as prose,
// never encoded into a fixture.

/// Collect the `Severity::Error` diagnostic messages from a stdlib-aware
/// compile of `source`. Shared by the four task-6098 field-read signal tests.
fn stdlib_compile_error_messages(source: &str) -> Vec<String> {
    compile_source_with_stdlib(source)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn scalar_force_magnitude_reads_as_a_force_dimensioned_quantity() {
    // Newton-dimensioned arithmetic on the read field is well-typed.
    let source = r#"
structure def Probe {
    let f = ScalarForce(magnitude: 2.5N)
    let x = f.magnitude + 1.0N
}
"#;
    let errors = stdlib_compile_error_messages(source);
    assert!(
        errors.is_empty(),
        "reading ScalarForce.magnitude into Newton-dimensioned arithmetic must \
         type-check once magnitude is declared `: Force` (task 6098); got: {:?}",
        errors
    );
}

#[test]
fn scalar_force_magnitude_read_rejects_bare_real_arithmetic() {
    // The other direction: a bare dimensionless Real no longer unifies.
    let source = r#"
structure def Probe {
    let f = ScalarForce(magnitude: 2.5N)
    let x = f.magnitude + 1.0
}
"#;
    let errors = stdlib_compile_error_messages(source);
    assert!(
        errors
            .iter()
            .any(|m| m.contains("dimension mismatch in addition")),
        "adding a bare Real to a Force-typed ScalarForce.magnitude must be a \
         dimension mismatch (task 6098); got: {:?}",
        errors
    );
}

#[test]
fn scalar_torque_magnitude_reads_as_a_torque_dimensioned_quantity() {
    // `Nm` (units.ri) is the Torque literal — N·m/rad, distinct from Energy.
    let source = r#"
structure def Probe {
    let t = ScalarTorque(magnitude: 1.5Nm)
    let x = t.magnitude + 1.0Nm
}
"#;
    let errors = stdlib_compile_error_messages(source);
    assert!(
        errors.is_empty(),
        "reading ScalarTorque.magnitude into N·m-dimensioned arithmetic must \
         type-check once magnitude is declared `: Torque` (task 6098); got: {:?}",
        errors
    );
}

#[test]
fn scalar_torque_magnitude_read_rejects_bare_real_arithmetic() {
    let source = r#"
structure def Probe {
    let t = ScalarTorque(magnitude: 1.5Nm)
    let x = t.magnitude + 1.0
}
"#;
    let errors = stdlib_compile_error_messages(source);
    assert!(
        errors
            .iter()
            .any(|m| m.contains("dimension mismatch in addition")),
        "adding a bare Real to a Torque-typed ScalarTorque.magnitude must be a \
         dimension mismatch (task 6098); got: {:?}",
        errors
    );
}

#[test]
fn scalar_force_and_scalar_torque_magnitudes_do_not_unify_across_families() {
    // The headline benefit claimed by stdlib/dynamics.ri's ScalarTorque note:
    // the two magnitudes are now non-unifiable in *arithmetic*, not just
    // distinguishable by nominal type-tag. Exercised at the expression level
    // (the four tests above only cover dimensioned-vs-bare-Real in each
    // direction; the structural `cell_type` assertions cover it only
    // indirectly).
    //
    // The Torque-vs-Energy case is the same claim's sharp edge: `Torque`
    // carries an Angle⁻¹ slot (N·m/rad), so it must NOT unify with `J`
    // (Energy = N·m) either, even though the two read the same on paper.
    for (label, source) in [
        (
            "Force magnitude + a Torque literal",
            r#"
structure def Probe {
    let f = ScalarForce(magnitude: 2.5N)
    let x = f.magnitude + 1.0Nm
}
"#,
        ),
        (
            "Torque magnitude + a Force literal",
            r#"
structure def Probe {
    let t = ScalarTorque(magnitude: 1.5Nm)
    let x = t.magnitude + 1.0N
}
"#,
        ),
        (
            "Torque magnitude + an Energy literal",
            r#"
structure def Probe {
    let t = ScalarTorque(magnitude: 1.5Nm)
    let x = t.magnitude + 1.0J
}
"#,
        ),
    ] {
        let errors = stdlib_compile_error_messages(source);
        assert!(
            errors
                .iter()
                .any(|m| m.contains("dimension mismatch in addition")),
            "{label} must be a dimension mismatch (task 6098); got: {:?}",
            errors
        );
    }
}
