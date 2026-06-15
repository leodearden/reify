//! Positive contract tests for the `Vec3 = Vector3<Length>` type alias
//! (crates/reify-compiler/stdlib/trajectory.ri, task #4575).
//!
//! These tests encode the TIGHTENED alias surface:
//!   - joint axis params (Revolute/Prismatic/Cylindrical/Planar) → Vector3<Length>
//!   - EndEffectorTrack.vibration_offset → List<List<Vector3<Length>>>
//!   - std/trajectory, std/kinematic, std/fea/multi_case load with zero errors
//!
//! All assertions fail against the OLD `pub type Vec3 = Real` (which resolves
//! to `Type::dimensionless_scalar()`). They go green once step-2 sets
//! `pub type Vec3 = Vector3<Length>` in trajectory.ri.
//!
//! POSITIVE-ONLY: no scalar-rejection assertion (the two-way boundary test was
//! dropped / rehomed to #4584 per the ratified rescope, esc-4575-54 Option A).
//!
//! Reuses the stdlib-compile helper pattern from kinematic_stdlib_compile.rs.

use reify_compiler::*;
use reify_core::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn load_kinematic() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/kinematic")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/kinematic; available: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

fn load_trajectory() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/trajectory")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/trajectory; available: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

fn find_structure_in<'a>(
    module: &'a CompiledModule,
    name: &str,
) -> &'a TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` in module {}; got: {:?}",
                name,
                module.path,
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

/// The expected Vector3<Length> type (Vec3after tightening).
fn vec3_length() -> Type {
    Type::vec3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    })
}

// ─── std/kinematic: axis param cell-type assertions ───────────────────────────

/// Revolute.axis param cell type must be Vector3<Length> after alias tightening.
/// FAILS against Vec3 = Real (resolves to dimensionless_scalar).
#[test]
fn revolute_axis_is_vec3_length() {
    let module = load_kinematic();
    let template = find_structure_in(module, "Revolute");
    let params = param_cells(template);
    let axis = params
        .iter()
        .find(|p| p.id.member == "axis")
        .expect("Revolute.axis param must exist");
    assert_eq!(
        axis.cell_type,
        vec3_length(),
        "Revolute.axis should be Vector3<Length> (Vec3 tightened by task #4575); \
         got: {:?}",
        axis.cell_type
    );
    // Positive compatibility: a Vector3<Length> argument must be accepted.
    assert!(
        type_compatible(&axis.cell_type, &vec3_length()),
        "type_compatible(Revolute.axis, Vector3<Length>) should be true after tightening"
    );
}

/// Prismatic.axis param cell type must be Vector3<Length>.
/// FAILS against Vec3 = Real.
#[test]
fn prismatic_axis_is_vec3_length() {
    let module = load_kinematic();
    let template = find_structure_in(module, "Prismatic");
    let params = param_cells(template);
    let axis = params
        .iter()
        .find(|p| p.id.member == "axis")
        .expect("Prismatic.axis param must exist");
    assert_eq!(
        axis.cell_type,
        vec3_length(),
        "Prismatic.axis should be Vector3<Length> (Vec3 tightened by task #4575); \
         got: {:?}",
        axis.cell_type
    );
    assert!(
        type_compatible(&axis.cell_type, &vec3_length()),
        "type_compatible(Prismatic.axis, Vector3<Length>) should be true after tightening"
    );
}

/// Cylindrical.axis param cell type must be Vector3<Length>.
/// FAILS against Vec3 = Real.
#[test]
fn cylindrical_axis_is_vec3_length() {
    let module = load_kinematic();
    let template = find_structure_in(module, "Cylindrical");
    let params = param_cells(template);
    let axis = params
        .iter()
        .find(|p| p.id.member == "axis")
        .expect("Cylindrical.axis param must exist");
    assert_eq!(
        axis.cell_type,
        vec3_length(),
        "Cylindrical.axis should be Vector3<Length> (Vec3 tightened by task #4575); \
         got: {:?}",
        axis.cell_type
    );
    assert!(
        type_compatible(&axis.cell_type, &vec3_length()),
        "type_compatible(Cylindrical.axis, Vector3<Length>) should be true after tightening"
    );
}

/// Planar.axis_x and Planar.axis_y param cell types must be Vector3<Length>.
/// FAILS against Vec3 = Real.
#[test]
fn planar_axis_x_and_axis_y_are_vec3_length() {
    let module = load_kinematic();
    let template = find_structure_in(module, "Planar");
    let params = param_cells(template);
    for expected_name in ["axis_x", "axis_y"] {
        let p = params
            .iter()
            .find(|p| p.id.member == expected_name)
            .unwrap_or_else(|| panic!("Planar.{} param must exist", expected_name));
        assert_eq!(
            p.cell_type,
            vec3_length(),
            "Planar.{} should be Vector3<Length> (Vec3 tightened by task #4575); \
             got: {:?}",
            expected_name,
            p.cell_type
        );
        assert!(
            type_compatible(&p.cell_type, &vec3_length()),
            "type_compatible(Planar.{}, Vector3<Length>) should be true after tightening",
            expected_name
        );
    }
}

// ─── std/trajectory: EndEffectorTrack.vibration_offset ───────────────────────

/// EndEffectorTrack.vibration_offset cell type must be List<List<Vector3<Length>>>
/// after the Vec3 alias is tightened.
/// FAILS against Vec3 = Real (resolves to List<List<dimensionless_scalar>>).
#[test]
fn end_effector_track_vibration_offset_is_list_list_vec3_length() {
    let module = load_trajectory();
    let template = find_structure_in(module, "EndEffectorTrack");
    let params = param_cells(template);
    let cell = params
        .iter()
        .find(|p| p.id.member == "vibration_offset")
        .expect("EndEffectorTrack.vibration_offset param must exist");

    let expected = Type::List(Box::new(Type::List(Box::new(vec3_length()))));
    assert_eq!(
        cell.cell_type,
        expected,
        "EndEffectorTrack.vibration_offset should be List<List<Vector3<Length>>> \
         (Vec3 tightened by task #4575); got: {:?}",
        cell.cell_type
    );
}

// ─── Load-clean assertions: zero Error diagnostics ───────────────────────────

/// std/trajectory loads with zero Severity::Error diagnostics.
#[test]
fn trajectory_loads_with_no_errors() {
    let module = load_trajectory();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected Error diagnostics in std/trajectory: {:?}",
        errors
    );
}

/// std/kinematic loads with zero Severity::Error diagnostics.
#[test]
fn kinematic_loads_with_no_errors() {
    let module = load_kinematic();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected Error diagnostics in std/kinematic: {:?}",
        errors
    );
}

/// std/fea/multi_case loads with zero Severity::Error diagnostics.
#[test]
fn fea_multi_case_loads_with_no_errors() {
    let module = stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/fea/multi_case")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/fea/multi_case; available: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        });
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected Error diagnostics in std/fea/multi_case: {:?}",
        errors
    );
}
