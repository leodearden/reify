//! Contract tests for the `Vec3 = Vector3<Length>` type alias
//! (crates/reify-compiler/stdlib/trajectory.ri, task #4575).
//!
//! These tests encode the TIGHTENED alias surface:
//!   - joint axis params (Revolute/Prismatic/Cylindrical/Planar) → Vector3<Length>
//!   - EndEffectorTrack.vibration_offset → List<List<Vector3<Length>>>
//!   - std/trajectory, std/kinematic, std/fea/multi_case load with zero errors
//!
//! All alias assertions fail against the OLD `pub type Vec3 = Real` (which resolves
//! to `Type::dimensionless_scalar()`). They go green once step-2 sets
//! `pub type Vec3 = Vector3<Length>` in trajectory.ri.
//!
//! TWO-WAY BOUNDARY TEST RESTORED (task #4622, S5):
//!   - POSITIVE: `AxisHolder(axis: vec3(0.0, 0.0, 1.0))` → ZERO errors
//!     (dimensionless vec3 arg accepted for Vector3<Length> param via loose-quantity rule)
//!   - REJECTION: `AxisHolder(axis: 1.0)` → exactly ONE TypeNotConformingToVector error
//!     (bare scalar rejected for Vector3<Length> param)
//!
//! Uses a self-contained `AxisHolder` structure (no stdlib import coupling),
//! exercised via a PascalCase StructureInstanceCtor that check_expr_struct_ctor_args
//! walks — mirrors the solid_param_tests.rs:574 rejection-test pattern.
//!
//! Reuses the stdlib-compile helper pattern from kinematic_stdlib_compile.rs.

use reify_compiler::*;
use reify_core::*;

// ─── snippet compile helper (task-4622 two-way boundary test) ─────────────────

/// Compile a `.ri` source snippet, asserting no parse errors.
/// Mirrors `solid_param_tests.rs::parse_and_compile`.
fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_vec3_type"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in vec3 boundary test snippet: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

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

// ─── Two-way boundary tests: AxisHolder with Vector3<Length> axis param ───────
//
// These tests use a self-contained `AxisHolder` structure so the boundary is
// tested without coupling to the stdlib. They exercise the StructureInstanceCtor
// path (`let h = AxisHolder(axis: <arg>)`) which is walked by
// `check_expr_struct_ctor_args` — the same path the 4584 rejection-test uses
// for StructureRef params.
//
// POSITIVE (RED-until-S6 for the gate wiring, but GREEN for the arg check):
//   `vec3(0.0, 0.0, 1.0)` is dimensionless → zero errors (loose-quantity rule).
//
// REJECTION (RED-until-S6):
//   `1.0` is a bare Real → exactly one TypeNotConformingToVector error.
//   RED because the `should_check` gate in entities_phase.rs does not yet route
//   Type::Vector params; after S6 the gate adds `|| matches!(&vc.cell_type, Type::Vector { .. })`
//   and the rejection fires correctly.

/// POSITIVE leg of the two-way boundary: a `vec3` arg is accepted for a
/// `Vector3<Length>` param (loose-quantity: dimensionless vec3 is valid).
///
/// Must produce ZERO Severity::Error diagnostics.
#[test]
fn axis_holder_accepts_vec3_arg() {
    let source = r#"structure def AxisHolder {
    param axis : Vector3<Length>
}
structure def PosRig {
    let h = AxisHolder(axis: vec3(0.0, 0.0, 1.0))
}"#;
    let compiled = parse_and_compile(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "vec3(0,0,1) arg for Vector3<Length> param must compile with ZERO errors; \
         got: {:#?}",
        errors
    );
}

/// REJECTION leg of the two-way boundary: a bare scalar `1.0` is rejected for
/// a `Vector3<Length>` param → exactly one `TypeNotConformingToVector` error.
///
/// RED until S6 wires Type::Vector params through the `should_check` gate in
/// `entities_phase.rs::check_expr_struct_ctor_args`.
#[test]
fn axis_holder_rejects_scalar_arg() {
    let source = r#"structure def AxisHolder {
    param axis : Vector3<Length>
}
structure def NegRig {
    let h = AxisHolder(axis: 1.0)
}"#;
    let compiled = parse_and_compile(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "scalar arg for Vector3<Length> param must produce exactly 1 Error; \
         got {}: {:#?}",
        errors.len(),
        errors
    );
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::TypeNotConformingToVector),
        "expected TypeNotConformingToVector, got {:?}",
        errors[0].code,
    );
}
