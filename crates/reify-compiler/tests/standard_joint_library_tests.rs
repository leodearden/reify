//! Tests for the standard joint library — geometric-joints γ (task 4397).
//!
//! The standard joint set (revolute / prismatic / cylindrical / planar /
//! spherical / ball) is defined as `joint … with` declarations over the landed
//! relation vocabulary. Each joint's body residual must exactly match its
//! declared DOF by COUNT and KIND (the β self-checking law). These tests:
//!
//! (a) `standard_joint_library_compiles_clean` — reads `stdlib/joints.ri` and
//!     asserts zero Error-severity diagnostics and zero `JointDofMismatch`
//!     (RED until step-2 creates the file).
//!
//! (b) Per-joint inline tests — compile exactly one `joint … with` definition
//!     per standard joint and assert zero `JointDofMismatch`. These characterise
//!     the landed ΔDOF kind-split tables (relation_signatures.rs) and lock the
//!     joint bodies against regression. GREEN from the moment the β self-check
//!     machinery is wired (pre-landed).
//!
//! DOF derivation — nominal rigid-body freedom = (3 rot, 3 trans):
//!   revolute:    concentric(Axis,Axis)(2,2) + on(Point,Plane)(0,1) → Σ=(2,3) → residual(1,0) ✓
//!   prismatic:   concentric(Axis,Axis)(2,2) + perpendicular(Axis,Axis)(1,0) → Σ=(3,2) → residual(0,1) ✓
//!   cylindrical: concentric(Axis,Axis)(2,2) → Σ=(2,2) → residual(1,1) ✓
//!   planar:      flush(Plane,Plane)(2,1) → Σ=(2,1) → residual(1,2) ✓
//!   spherical:   coincident(Point,Point)(0,3) → Σ=(0,3) → residual(3,0) ✓
//!   ball:        coincident(Point,Point)(0,3) → Σ=(0,3) → residual(3,0) ✓

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

/// The error-severity `JointDofMismatch` diagnostics emitted while compiling
/// `module` — the β joint-DOF self-check signal (mirrors β's `joint_dof_errors`
/// helper in `joint_dof_self_check_tests.rs`).
fn joint_dof_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::JointDofMismatch) && d.severity == Severity::Error
        })
        .collect()
}

// ── (a) Library-compiles-clean ────────────────────────────────────────────────

/// The standard joint library `stdlib/joints.ri` compiles with zero
/// Error-severity diagnostics and zero `JointDofMismatch` — all 6 standard
/// joints are self-check-clean.
///
/// RED: `stdlib/joints.ri` does not exist yet → the file read fails with
/// `std::io::Error`. Step-2 (impl) creates the file and makes this green.
#[test]
fn standard_joint_library_compiles_clean() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/stdlib/joints.ri");
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read stdlib/joints.ri at `{path}`: {e}"));
    let module = compile_source_with_stdlib(&source);

    let errors: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "stdlib/joints.ri must compile with zero Error-severity diagnostics, got: {errors:#?}",
    );
    assert!(
        joint_dof_errors(&module).is_empty(),
        "stdlib/joints.ri must emit zero E_JOINT_DOF_MISMATCH (all 6 joints must be \
         self-check-clean): {:#?}",
        joint_dof_errors(&module)
    );
}

// ── (b) Per-joint inline self-check-clean tests ──────────────────────────────
//
// Each test compiles exactly one `joint … with` definition inline (without the
// full library file) and asserts zero `JointDofMismatch` diagnostics. This
// pins:
//   - the relation ΔDOF kind-split tables (relation_delta_dof_kinds in
//     relation_signatures.rs) against drift;
//   - the canonical joint body for each standard kind against regression.
//
// These tests are GREEN from the moment the β self-check machinery is wired
// (pre-landed, task 4396). Creating joints.ri (step-2) does not affect them.

/// revolute: concentric(a,b)=(2rot,2trans) + on(p,stop)=(0rot,1trans)
/// → Σ=(2,3) → residual(1rot,0trans) = angle:Angle ✓
#[test]
fn revolute_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint revolute(a: Axis, b: Axis, p: Point3<Length>, stop: Plane) \
         with angle: Angle in 0deg..120deg = { concentric(a, b)  on(p, stop) }",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "revolute: residual(1rot,0trans) must match declared `angle: Angle` = (1,0) \
         → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// prismatic: concentric(a,b)=(2rot,2trans) + perpendicular(key_a,key_b)=(1rot,0trans)
/// → Σ=(3,2) → residual(0rot,1trans) = travel:Length ✓
/// (perpendicular lifts Axis→Direction via .dir; ΔDOF=(1,0) unconditional)
#[test]
fn prismatic_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint prismatic(a: Axis, b: Axis, key_a: Axis, key_b: Axis) \
         with travel: Length in 0mm..50mm = { concentric(a, b)  perpendicular(key_a, key_b) }",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "prismatic: residual(0rot,1trans) must match declared `travel: Length` = (0,1) \
         → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// cylindrical: concentric(a,b)=(2rot,2trans) → Σ=(2,2) → residual(1rot,1trans)
/// = { angle:Angle, travel:Length } = (1,1) ✓
#[test]
fn cylindrical_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint cylindrical(a: Axis, b: Axis) \
         with { angle: Angle in 0deg..360deg, travel: Length in 0mm..50mm } = concentric(a, b)",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "cylindrical: residual(1rot,1trans) must match declared `{{ angle:Angle, travel:Length }}` \
         = (1,1) → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// planar: flush(face_a,face_b)=(2rot,1trans) → Σ=(2,1) → residual(1rot,2trans)
/// = { x:Length, y:Length, spin:Angle } = (1,2) ✓
#[test]
fn planar_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint planar(face_a: Plane, face_b: Plane) \
         with { x: Length, y: Length, spin: Angle } = flush(face_a, face_b)",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "planar: residual(1rot,2trans) must match declared `{{ x:Length, y:Length, spin:Angle }}` \
         = (1,2) → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// spherical: coincident(c,d) where c,d:Point3<Length> → (0rot,3trans)
/// → Σ=(0,3) → residual(3rot,0trans) = orientation:Orientation ✓
#[test]
fn spherical_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint spherical(c: Point3<Length>, d: Point3<Length>) \
         with orientation: Orientation = coincident(c, d)",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "spherical: residual(3rot,0trans) must match declared `orientation: Orientation` = (3,0) \
         → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// ball: coincident(c,d) where c,d:Point3<Length> → (0rot,3trans)
/// → Σ=(0,3) → residual(3rot,0trans) = orientation:Orientation ✓
/// (design §7 canonical name; kinematic synonym of spherical — both defined to
/// preserve both vocabularies)
#[test]
fn ball_joint_definition_is_self_check_clean() {
    let module = compile_source_with_stdlib(
        "joint ball(c: Point3<Length>, d: Point3<Length>) \
         with orientation: Orientation = coincident(c, d)",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "ball: residual(3rot,0trans) must match declared `orientation: Orientation` = (3,0) \
         → zero E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

// ── (c) stdlib registration check ────────────────────────────────────────────

/// `std.joints` must be registered as a prelude stdlib module — `load_stdlib()`
/// returns a compiled module whose `path` display is `std/joints`.
///
/// RED: `std.joints` is not registered in `stdlib_sources()` yet (step-3 test).
/// Step-4 (impl) adds the `include_str!` entry and makes this green.
#[test]
fn std_joints_registered_in_stdlib_prelude() {
    let modules = reify_compiler::stdlib_loader::load_stdlib();
    let found = modules
        .iter()
        .any(|m| format!("{}", m.path) == "std/joints");
    assert!(
        found,
        "std.joints must be registered in the stdlib prelude (stdlib_loader.rs::stdlib_sources);\n\
         currently loaded module paths: {:?}",
        modules
            .iter()
            .map(|m| format!("{}", m.path))
            .collect::<Vec<_>>()
    );
}
