//! End-to-end boolean operations tests: source → STEP export.
//!
//! Tests the full pipeline for boolean ops:
//!   parse → compile → Engine (with OcctKernelHandle) → build → valid STEP output.
//!
//! All tests are guarded by `reify_kernel_occt::OCCT_AVAILABLE` and are skipped
//! if the OCCT library is not present.

use reify_types::{ExportFormat, ModulePath};

/// Run a boolean-ops source string through the full pipeline and return the STEP output.
/// Returns None if OCCT is not available.
fn run_boolean_e2e(source: &str) -> Option<String> {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return None;
    }

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test_bool"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
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
    let mut planner = reify_geometry::DispatchPlanner::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Geometry output should be present
    let output = result.geometry_output.expect("build should produce geometry output");
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
