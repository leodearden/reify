//! Sub-placement T5 surfacing tests (task 3903).
//!
//! Covers auto-surfacing of containment descendants at composed world poses and
//! the per-surface `default_visible` flag derived from `aux`.
//!
//! - **Structural** tests (entity_path composition, `default_visible`, surface
//!   counts, no-double-surfacing) use `MockGeometryKernel`: its `tessellate`
//!   returns a fixed dummy triangle for every handle, so it pins surface
//!   *shape* but cannot validate placement.
//! - **Placement / golden-AABB** tests (added in later steps) use the real OCCT
//!   kernel, gated on `reify_kernel_occt::OCCT_AVAILABLE`.
//!
//! All sources are compiled via `compile_source_with_stdlib` so that
//! `SubComponentDecl.pose` / `SubComponentDecl.is_aux` and `ValueCellDecl.is_aux`
//! are populated by the real T1/T2 lowering path (`TopologyTemplateBuilder` does
//! not model `at` / `aux`).

use reify_core::Severity;
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib};

/// Build a Mock-kernel engine for structural surfacing assertions.
fn mock_engine() -> reify_eval::Engine {
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Resolve the `entity_path` a realization named `name` surfaces under on the
/// flat (root) path: `<entity>#realization[<index>]`.
fn root_realization_path(template: &reify_compiler::TopologyTemplate, name: &str) -> String {
    template
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("realization for `{name}` not found"))
        .id
        .to_string()
}

/// step-1 (Mock): on the flat (no-composition) path, an `aux let` body is still
/// realized, tessellated, and surfaced (mesh payload shipped) but with
/// `default_visible == false`, while a plain `let` body surfaces with
/// `default_visible == true`.
///
/// Pins the realization→`ValueCellDecl.is_aux` mapping. Fails until step-2 wires
/// `default_visible` from `is_aux` (pre-1 left it hard-coded `true`).
#[test]
fn aux_let_body_surfaces_hidden_plain_let_visible() {
    let source = r#"structure Single {
    let body = box(20mm, 20mm, 20mm)
    aux let blank = cylinder(8mm, 40mm)
}"#;
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Single")
        .expect("Single template not found");
    let body_path = root_realization_path(template, "body");
    let blank_path = root_realization_path(template, "blank");

    let mut engine = mock_engine();
    let result = engine.tessellate_realizations(&compiled);
    let tess_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        tess_errors.is_empty(),
        "unexpected tessellation errors: {:?}",
        tess_errors
    );

    let body = result
        .meshes
        .iter()
        .find(|s| s.entity_path == body_path)
        .expect("plain `body` surface must be present");
    let blank = result
        .meshes
        .iter()
        .find(|s| s.entity_path == blank_path)
        .expect("aux `blank` surface must still be shipped (hidden, not skipped)");

    // The aux body is still realized + tessellated + shipped: mesh payload present.
    assert!(
        !blank.mesh.vertices.is_empty(),
        "aux `blank` mesh payload must be shipped"
    );

    // default_visible is derived from ValueCellDecl.is_aux.
    assert!(
        body.default_visible,
        "plain `let body` must surface with default_visible == true"
    );
    assert!(
        !blank.default_visible,
        "aux `let blank` must surface with default_visible == false (hidden)"
    );
}
