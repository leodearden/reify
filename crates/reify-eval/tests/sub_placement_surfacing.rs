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

/// Compose the descendant `entity_path` `<prefix>#realization[<index>]` for the
/// realization named `name` in `template`, using the realization's own index
/// (so the suffix matches the surfacing scheme regardless of declaration order).
fn composed_path(template: &reify_compiler::TopologyTemplate, prefix: &str, name: &str) -> String {
    let index = template
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("realization for `{name}` not found"))
        .id
        .index;
    format!("{prefix}#realization[{index}]")
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

/// step-3 (Mock): a 2-level CONTAINMENT-ONLY assembly (identity pose, no `at`)
/// surfaces the contained child body exactly ONCE, under the COMPOSED
/// `entity_path` `<root>.<sub>#realization[i]` (PRD §11.2
/// `parent.sub#realization[i]`, e.g. `Assembly.c#realization[0]`), and
/// SUPPRESSES the standalone `Child#realization[0]` surface (no
/// double-surfacing).
///
/// Pins the exact descendant path scheme. Fails until step-4 adds the root-set
/// + containment tree-walk + standalone-suppression (today every template is
/// surfaced flatly, so `Child#realization[0]` appears standalone and the
/// composed path is absent).
#[test]
fn containment_child_surfaces_once_at_composed_path() {
    let source = r#"structure Child {
    let body = box(20mm, 20mm, 20mm)
}
structure Assembly {
    sub c : Child
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

    let paths: Vec<&str> = result
        .meshes
        .iter()
        .map(|s| s.entity_path.as_str())
        .collect();

    // (b) no standalone duplicate of the contained child.
    assert!(
        !paths.contains(&"Child#realization[0]"),
        "standalone `Child#realization[0]` must be suppressed (no double-surfacing); got {:?}",
        paths
    );

    // (a) the child body surfaces under the composed `Assembly.c#realization[0]`.
    assert!(
        paths.contains(&"Assembly.c#realization[0]"),
        "child body must surface under composed path `Assembly.c#realization[0]`; got {:?}",
        paths
    );

    // (c) total surface count == number of surfaced descendants (exactly 1).
    assert_eq!(
        result.meshes.len(),
        1,
        "exactly one surfaced descendant expected; got {:?}",
        paths
    );
}

/// step-5 case 1 (Mock): an `aux sub` hides its ENTIRE contained subtree — the
/// descendant surface under the aux sub has `default_visible == false`,
/// inherited from the sub even though the child's own `let body` is NOT aux.
/// The body is still realized, tessellated, and shipped (hidden, not skipped).
///
/// Fails until step-6 threads `aux` inheritance through the containment walk
/// (step-4 honors only per-realization aux, so today the non-aux child body
/// surfaces visible under an aux sub).
#[test]
fn aux_sub_hides_entire_contained_subtree() {
    let source = r#"structure Child {
    let body = box(20mm, 20mm, 20mm)
}
structure Assembly {
    aux sub c : Child
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

    let surface = result
        .meshes
        .iter()
        .find(|s| s.entity_path == "Assembly.c#realization[0]")
        .expect("aux-sub child body must still be surfaced (hidden, not skipped)");
    assert!(
        !surface.mesh.vertices.is_empty(),
        "aux-sub descendant mesh payload must still be shipped"
    );
    assert!(
        !surface.default_visible,
        "descendant under an `aux sub` must surface with default_visible == false (aux inherited)"
    );
}

/// step-5 case 2 (Mock): inside a surfaced (contained, NON-aux) child, a
/// per-realization `aux let` body is hidden while a sibling non-aux `let` is
/// visible — composed-path surfacing honors per-realization aux independently
/// of the containing sub. Regression-lock: already satisfied by step-4's
/// per-realization aux; pinned here so step-6's ancestor-OR does not over-hide
/// a non-aux sibling.
#[test]
fn contained_child_aux_let_hidden_sibling_visible() {
    let source = r#"structure Child {
    let body = box(20mm, 20mm, 20mm)
    aux let blank = cylinder(8mm, 40mm)
}
structure Assembly {
    sub c : Child
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

    let child = compiled
        .templates
        .iter()
        .find(|t| t.name == "Child")
        .expect("Child template not found");
    let body_path = composed_path(child, "Assembly.c", "body");
    let blank_path = composed_path(child, "Assembly.c", "blank");

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
        .unwrap_or_else(|| panic!("non-aux `body` surface `{body_path}` must be present"));
    let blank = result
        .meshes
        .iter()
        .find(|s| s.entity_path == blank_path)
        .unwrap_or_else(|| panic!("aux `blank` surface `{blank_path}` must still be shipped"));

    assert!(
        body.default_visible,
        "non-aux `let body` in a non-aux sub must surface with default_visible == true"
    );
    assert!(
        !blank.default_visible,
        "aux `let blank` must surface with default_visible == false even under a non-aux sub"
    );
}
