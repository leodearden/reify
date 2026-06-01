//! End-to-end boolean operations tests: source → STEP export.
//!
//! Tests the full pipeline for boolean ops:
//!   parse → compile → Engine (with OcctKernelHandle) → build → valid STEP output.
//!
//! All tests are guarded by `reify_kernel_occt::OCCT_AVAILABLE` and are skipped
//! if the OCCT library is not present.

use reify_core::ModulePath;
use reify_ir::ExportFormat;

/// Run a boolean-ops source string through the full pipeline and return the STEP output.
/// Returns None if OCCT is not available.
fn run_boolean_e2e(source: &str) -> Option<String> {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return None;
    }

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test_bool"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Should have at least one realization (the boolean let)
    assert_eq!(compiled.templates.len(), 1);
    assert!(
        !compiled.templates[0].realizations.is_empty(),
        "expected at least one realization"
    );

    // Build with real OCCT kernel
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Geometry output should be present
    let output = result
        .geometry_output
        .expect("build should produce geometry output");
    assert!(!output.is_empty(), "STEP output should be non-empty");

    let step_str = String::from_utf8(output).expect("STEP should be valid UTF-8");
    assert!(
        step_str.contains("ISO-10303-21"),
        "STEP output should contain ISO-10303-21 header, got: {}...",
        &step_str[..step_str.len().min(200)]
    );
    Some(step_str)
}

/// Step-12: union(box, box) → valid STEP export.
#[test]
fn boolean_union_box_box_e2e() {
    let source = r#"structure S {
    let r = union(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm))
}"#;
    run_boolean_e2e(source);
}

/// Step-14: difference(box, cylinder) → valid STEP export (box with hole).
#[test]
fn boolean_difference_box_cylinder_e2e() {
    let source = r#"structure S {
    let r = difference(box(20mm, 20mm, 20mm), cylinder(5mm, 20mm))
}"#;
    run_boolean_e2e(source);
}

/// Step-15: intersection(box, box) → valid STEP export.
#[test]
fn boolean_intersection_box_box_e2e() {
    let source = r#"structure S {
    let r = intersection(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm))
}"#;
    run_boolean_e2e(source);
}

/// Step-16: union_all with 3 boxes → valid STEP export.
#[test]
fn boolean_union_all_three_boxes_e2e() {
    let source = r#"structure S {
    let r = union_all(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm))
}"#;
    run_boolean_e2e(source);
}

/// Step-19: multi-realization with nested booleans in both realizations.
///
/// r1 = difference(box(20mm,20mm,20mm), cylinder(5mm, 20mm)) — box with cylindrical hole
/// r2 = union(box(10mm,10mm,10mm), sphere(8mm))              — box merged with sphere
///
/// Both realizations use nested boolean ops. Without the realization-local slice
/// fix (step-18), r2's Step(0), Step(1), Step(2) would resolve into r1's handles
/// rather than r2's own primitives. This exercises the fix with compound ops
/// where step indices would be even more wrong: r2 emits Step(0)=box, Step(1)=sphere,
/// Step(2)=Boolean{Union, Step(0), Step(1)}, but globally after r1's 3 ops those
/// step indices point to r1's box, cylinder, and difference result respectively.
#[test]
fn boolean_multi_realization_nested_e2e() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let r1 = difference(box(20mm, 20mm, 20mm), cylinder(5mm, 20mm))
    let r2 = union(box(10mm, 10mm, 10mm), sphere(8mm))
}"#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test_bool_nested_multi"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Should have 1 template with 2 realizations
    assert_eq!(compiled.templates.len(), 1, "expected 1 template");
    assert_eq!(
        compiled.templates[0].realizations.len(),
        2,
        "expected 2 realizations (r1 and r2)"
    );
    // r1 has 3 ops: box, cylinder, Boolean{Difference}
    assert_eq!(
        compiled.templates[0].realizations[0].operations.len(),
        3,
        "r1 should have 3 ops (box, cylinder, difference)"
    );
    // r2 has 3 ops: box, sphere, Boolean{Union}
    assert_eq!(
        compiled.templates[0].realizations[1].operations.len(),
        3,
        "r2 should have 3 ops (box, sphere, union)"
    );

    // Tessellate with real OCCT kernel
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    let result = engine.tessellate_realizations(&compiled);

    // No geometry errors
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(geom_errors.is_empty(), "geometry errors: {:?}", geom_errors);

    // Both realizations must produce tessellated meshes
    assert_eq!(
        result.meshes.len(),
        2,
        "expected 2 meshes (one per realization), got {}",
        result.meshes.len()
    );
    assert!(
        !result.meshes[0].mesh.vertices.is_empty(),
        "r1 (difference) mesh should have vertices"
    );
    assert!(
        !result.meshes[1].mesh.vertices.is_empty(),
        "r2 (union) mesh should have vertices"
    );
}

/// Step-17: two geometry let bindings — r1 = simple box, r2 = boolean union.
///
/// This test exposes the multi-realization step index bug: when the eval engine
/// passes the full global step_handles vector to compile_geometry_op, r2's
/// Boolean{Union, Step(0), Step(1)} resolves Step(0) to r1's handle instead of
/// r2's first box handle. The fix (step-18) passes a realization-local slice.
///
/// For tessellate_realizations: both realizations must tessellate successfully
/// and produce non-empty meshes without geometry errors.
#[test]
fn boolean_multi_realization_step_index_e2e() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let r1 = box(10mm, 10mm, 10mm)
    let r2 = union(box(20mm, 20mm, 20mm), box(30mm, 30mm, 30mm))
}"#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test_bool_multi"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Should have 1 template with 2 realizations (r1 and r2)
    assert_eq!(compiled.templates.len(), 1, "expected 1 template");
    assert_eq!(
        compiled.templates[0].realizations.len(),
        2,
        "expected 2 realizations (r1 and r2)"
    );

    // Tessellate with real OCCT kernel — tests the realization-local step index path
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    let result = engine.tessellate_realizations(&compiled);

    // No geometry errors
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(geom_errors.is_empty(), "geometry errors: {:?}", geom_errors);

    // Both realizations must produce tessellated meshes
    assert_eq!(
        result.meshes.len(),
        2,
        "expected 2 meshes (one per realization), got {}",
        result.meshes.len()
    );
    assert!(
        !result.meshes[0].mesh.vertices.is_empty(),
        "r1 (box) mesh should have vertices"
    );
    assert!(
        !result.meshes[1].mesh.vertices.is_empty(),
        "r2 (union) mesh should have vertices"
    );
}
