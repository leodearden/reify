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
use reify_ir::ExportFormat;
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

/// Compute the axis-aligned bounding-box of a mesh as `(min, max)` over the flat
/// vertex buffer (copied from `geometry_conditional_e2e.rs:23`). Panics if the
/// mesh has no vertices.
fn mesh_aabb(mesh: &reify_ir::Mesh) -> ([f32; 3], [f32; 3]) {
    assert!(
        !mesh.vertices.is_empty(),
        "mesh_aabb: vertex buffer is empty"
    );
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for chunk in mesh.vertices.chunks_exact(3) {
        for i in 0..3 {
            if chunk[i] < min[i] {
                min[i] = chunk[i];
            }
            if chunk[i] > max[i] {
                max[i] = chunk[i];
            }
        }
    }
    (min, max)
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
/// Pins the exact descendant path scheme. Fails until step-4 adds the root-set,
/// the containment tree-walk, and standalone-suppression (today every template
/// is surfaced flatly, so `Child#realization[0]` appears standalone and the
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

/// step-9 (real OCCT, `OCCT_AVAILABLE`-gated): a contained child placed via
/// `sub c : Child at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))` must
/// surface with REAL transformed geometry — its mesh AABB equals the source box
/// AABB shifted +30 mm on X — while an identity-pose control child surfaces at
/// unchanged coords.
///
/// `box(20mm, 20mm, 20mm)` is centered at the origin (OCCT `make_box`
/// convention, verified in `reify-kernel-occt` lib.rs:8244), so the source AABB
/// is `[-0.01, 0.01]³` m. Golden:
/// - control (identity): min `[-0.01,-0.01,-0.01]`, max `[0.01,0.01,0.01]`;
/// - placed (`+30mm` X): X-range shifts to `[0.02, 0.04]`, Y/Z unchanged.
///
/// Both children are contained subs (no manual lift transforms), so each
/// surfaces once under its composed `<parent>.c#realization[0]` path. Fails
/// until step-10 applies the composed transform to geometry: step-4 surfaces
/// descendants un-placed, so today the placed child lands at the control coords
/// and the `+30mm` X assertions fail.
#[test]
fn placed_child_surfaces_at_composed_world_aabb() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure Child {
    let body = box(20mm, 20mm, 20mm)
}
structure ControlAsm {
    sub c : Child
}
structure PlacedAsm {
    sub c : Child at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
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
    let control_path = composed_path(child, "ControlAsm.c", "body");
    let placed_path = composed_path(child, "PlacedAsm.c", "body");

    // Build engine with the real OCCT kernel (Mock cannot validate placement).
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    let result = engine.tessellate_realizations(&compiled);
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(geom_errors.is_empty(), "geometry errors: {:?}", geom_errors);

    let control = result
        .meshes
        .iter()
        .find(|s| s.entity_path == control_path)
        .unwrap_or_else(|| panic!("control surface `{control_path}` must be present"));
    let placed = result
        .meshes
        .iter()
        .find(|s| s.entity_path == placed_path)
        .unwrap_or_else(|| panic!("placed surface `{placed_path}` must be present"));

    let (cmin, cmax) = mesh_aabb(&control.mesh);
    let (pmin, pmax) = mesh_aabb(&placed.mesh);

    // Tessellation of a box places vertices exactly at its 8 corners (no
    // curvature deflection), so a tight tolerance is reliable.
    let tol = 1e-5_f32;

    // Control: identity-pose child surfaces at unchanged coords (centered cube).
    let expect_cmin = [-0.01_f32, -0.01, -0.01];
    let expect_cmax = [0.01_f32, 0.01, 0.01];
    for axis in 0..3 {
        assert!(
            (cmin[axis] - expect_cmin[axis]).abs() < tol,
            "control min[{axis}] expected {}, got {}",
            expect_cmin[axis],
            cmin[axis]
        );
        assert!(
            (cmax[axis] - expect_cmax[axis]).abs() < tol,
            "control max[{axis}] expected {}, got {}",
            expect_cmax[axis],
            cmax[axis]
        );
    }

    // Placed: source box AABB translated +30 mm on X (Y/Z unchanged).
    let expect_pmin = [0.02_f32, -0.01, -0.01];
    let expect_pmax = [0.04_f32, 0.01, 0.01];
    for axis in 0..3 {
        assert!(
            (pmin[axis] - expect_pmin[axis]).abs() < tol,
            "placed min[{axis}] expected {}, got {} (composed placement not applied?)",
            expect_pmin[axis],
            pmin[axis]
        );
        assert!(
            (pmax[axis] - expect_pmax[axis]).abs() < tol,
            "placed max[{axis}] expected {}, got {} (composed placement not applied?)",
            expect_pmax[axis],
            pmax[axis]
        );
    }
}

/// step-11 (real OCCT, `OCCT_AVAILABLE`-gated) — full T5 acceptance signal.
///
/// A 2-level assembly with NO manual lift transforms exercises BOTH new T5
/// behaviors on sibling subtrees of a single root:
/// - a PRODUCT child `sub part : Part at +30 mm X` — its composed world
///   placement is applied to REAL geometry and it surfaces visible; and
/// - an AUX child `aux sub jig : Jig at +50 mm Y` — still realized, transformed,
///   tessellated and shipped (mesh payload present) but `default_visible ==
///   false` (hidden, not skipped).
///
/// `Part` is `box(20mm)` (centered AABB `[-0.01, 0.01]³` m) and `Jig` is
/// `box(10mm)` (centered AABB `[-0.005, 0.005]³` m), so the two surfaces are
/// distinguishable by extent as well as by placement axis. Both children
/// reference the assembly root only as subs, so each surfaces exactly once under
/// its composed `Assembly.<sub>#realization[0]` path.
///
/// Asserts the full signal: (a) one mesh per surfaced descendant, each at its
/// composed world AABB (golden per child); (b) the aux body's surface is PRESENT
/// in meshes but `default_visible == false`; (c) the product child surface is
/// `default_visible == true`; (d) no standalone duplicate of either contained
/// child (exactly two surfaces total). Reconciles aux-with-placement on the same
/// walk: the aux subtree is transformed AND hidden, while the product subtree is
/// transformed AND visible.
#[test]
fn placed_product_and_aux_children_surface_at_world_aabb_with_visibility() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure Part {
    let body = box(20mm, 20mm, 20mm)
}
structure Jig {
    let body = box(10mm, 10mm, 10mm)
}
structure Assembly {
    sub part : Part at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
    aux sub jig : Jig at transform3(orient_identity(), vec3(0mm, 50mm, 0mm))
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

    let part = compiled
        .templates
        .iter()
        .find(|t| t.name == "Part")
        .expect("Part template not found");
    let jig = compiled
        .templates
        .iter()
        .find(|t| t.name == "Jig")
        .expect("Jig template not found");
    let part_path = composed_path(part, "Assembly.part", "body");
    let jig_path = composed_path(jig, "Assembly.jig", "body");

    // Build engine with the real OCCT kernel (Mock cannot validate placement).
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    let result = engine.tessellate_realizations(&compiled);
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(geom_errors.is_empty(), "geometry errors: {:?}", geom_errors);

    let paths: Vec<&str> = result
        .meshes
        .iter()
        .map(|s| s.entity_path.as_str())
        .collect();

    // (d) no standalone duplicate of either contained child.
    assert!(
        !paths.contains(&"Part#realization[0]"),
        "standalone `Part#realization[0]` must be suppressed (no double-surfacing); got {:?}",
        paths
    );
    assert!(
        !paths.contains(&"Jig#realization[0]"),
        "standalone `Jig#realization[0]` must be suppressed (no double-surfacing); got {:?}",
        paths
    );
    // (a) one mesh per surfaced descendant — exactly the product + the aux child.
    assert_eq!(
        result.meshes.len(),
        2,
        "exactly two surfaced descendants expected (product + aux); got {:?}",
        paths
    );

    let part_surface = result
        .meshes
        .iter()
        .find(|s| s.entity_path == part_path)
        .unwrap_or_else(|| panic!("product surface `{part_path}` must be present"));
    let jig_surface = result
        .meshes
        .iter()
        .find(|s| s.entity_path == jig_path)
        .unwrap_or_else(|| {
            panic!("aux surface `{jig_path}` must still be shipped (hidden, not skipped)")
        });

    // (c) product child surfaces visible; (b) aux child surfaces hidden but
    // present with a real mesh payload.
    assert!(
        part_surface.default_visible,
        "product child `part` must surface with default_visible == true"
    );
    assert!(
        !jig_surface.default_visible,
        "aux child `jig` must surface with default_visible == false (hidden)"
    );
    assert!(
        !jig_surface.mesh.vertices.is_empty(),
        "aux `jig` mesh payload must still be shipped (realized + transformed + tessellated)"
    );

    // Box tessellation places vertices exactly at the 8 corners (no curvature
    // deflection), so a tight tolerance is reliable (matches step-9).
    let tol = 1e-5_f32;

    // (a) product child: box(20mm) centered, composed-translated +30 mm on X.
    let (part_min, part_max) = mesh_aabb(&part_surface.mesh);
    let expect_part_min = [0.02_f32, -0.01, -0.01];
    let expect_part_max = [0.04_f32, 0.01, 0.01];
    for axis in 0..3 {
        assert!(
            (part_min[axis] - expect_part_min[axis]).abs() < tol,
            "product min[{axis}] expected {}, got {} (composed placement not applied?)",
            expect_part_min[axis],
            part_min[axis]
        );
        assert!(
            (part_max[axis] - expect_part_max[axis]).abs() < tol,
            "product max[{axis}] expected {}, got {}",
            expect_part_max[axis],
            part_max[axis]
        );
    }

    // (a) aux child: box(10mm) centered, composed-translated +50 mm on Y. The
    // placement is applied even though the body is hidden — aux is realized +
    // transformed + tessellated, not skipped.
    let (jig_min, jig_max) = mesh_aabb(&jig_surface.mesh);
    let expect_jig_min = [-0.005_f32, 0.045, -0.005];
    let expect_jig_max = [0.005_f32, 0.055, 0.005];
    for axis in 0..3 {
        assert!(
            (jig_min[axis] - expect_jig_min[axis]).abs() < tol,
            "aux min[{axis}] expected {}, got {} (aux placement not applied?)",
            expect_jig_min[axis],
            jig_min[axis]
        );
        assert!(
            (jig_max[axis] - expect_jig_max[axis]).abs() < tol,
            "aux max[{axis}] expected {}, got {}",
            expect_jig_max[axis],
            jig_max[axis]
        );
    }
}

/// Amendment (reviewer test_coverage, suggestion 3 — Mock): a single `Child`
/// contained by TWO distinct non-collection parents (`AsmA`, `AsmB`, with
/// DIFFERENT `at` poses) surfaces TWICE — once under each composed path — from
/// the SAME recorded terminal handle. This locks the non-destructive
/// `ApplyTransform` reuse: the second parent's walk re-applies a transform to
/// the same Phase-A child handle, so the source handle must survive the first
/// parent's transform (T3 `ApplyTransform` preserves its source).
///
/// Mock cannot validate coordinates (its `tessellate` returns a fixed triangle),
/// so this pins surface SHAPE only: both composed paths present and distinct, the
/// standalone `Child#realization[0]` suppressed, and exactly two surfaces total.
#[test]
fn shared_child_surfaces_under_each_parent_from_one_handle() {
    let source = r#"structure Child {
    let body = box(20mm, 20mm, 20mm)
}
structure AsmA {
    sub c : Child at transform3(orient_identity(), vec3(10mm, 0mm, 0mm))
}
structure AsmB {
    sub c : Child at transform3(orient_identity(), vec3(0mm, 20mm, 0mm))
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

    // Both parents surface the shared child under their own composed path.
    assert!(
        paths.contains(&"AsmA.c#realization[0]"),
        "shared child must surface under `AsmA.c#realization[0]`; got {:?}",
        paths
    );
    assert!(
        paths.contains(&"AsmB.c#realization[0]"),
        "shared child must surface under `AsmB.c#realization[0]`; got {:?}",
        paths
    );
    // The two composed paths are distinct instances of the same child template.
    assert_ne!(
        "AsmA.c#realization[0]", "AsmB.c#realization[0]",
        "the two parents must yield distinct composed paths"
    );
    // No standalone duplicate of the shared child.
    assert!(
        !paths.contains(&"Child#realization[0]"),
        "standalone `Child#realization[0]` must be suppressed; got {:?}",
        paths
    );
    // Exactly two surfaces: the child under each of the two parents.
    assert_eq!(
        result.meshes.len(),
        2,
        "exactly two surfaces expected (shared child under each parent); got {:?}",
        paths
    );
}

/// Amendment (reviewer robustness_edge_case, suggestion 2 — Mock): a
/// self-recursive structure (`sub child : Self`) is excluded from the root set
/// (it is its own sub's `structure_name`) AND no root reaches it, so without a
/// fallback it would be SILENTLY DROPPED — a geometry-loss regression vs. pre-T5,
/// where every template surfaced standalone.
///
/// Locks the contract: such a template is surfaced via the Phase-B fallback at
/// its standalone path (`Node#realization[0]`), and the walk TERMINATES — the
/// `depth > templates.len()` cycle guard bounds the recursion (suggestion 3's
/// cycle-guard branch), so the surface count stays finite (`<= templates.len() +
/// 1`) rather than recursing forever.
///
/// A self-recursive non-collection sub additionally emits a compile-time
/// termination error ("recursive sub has no termination condition"); that
/// diagnostic — not an eval-time one — is what makes the cycle observable, so the
/// fallback surfaces the geometry silently (no redundant eval warning, and no
/// false warning for a *terminating* recursive structure used standalone).
#[test]
fn self_recursive_structure_surfaces_via_fallback_and_terminates() {
    let source = r#"structure Node {
    let body = box(20mm, 20mm, 20mm)
    sub child : Node
}"#;
    let compiled = compile_source_with_stdlib(source);

    let mut engine = mock_engine();
    let result = engine.tessellate_realizations(&compiled);
    let tess_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        tess_errors.is_empty(),
        "fallback surfacing must not introduce tessellation errors: {:?}",
        tess_errors
    );

    let paths: Vec<&str> = result
        .meshes
        .iter()
        .map(|s| s.entity_path.as_str())
        .collect();

    // PRIMARY contract: the self-recursive template's geometry is NOT silently
    // dropped — it surfaces standalone via the fallback.
    assert!(
        paths.contains(&"Node#realization[0]"),
        "self-recursive `Node` must surface standalone via the fallback (geometry not lost); got {:?}",
        paths
    );

    // Cycle-guard branch: the walk terminates with a bounded surface count
    // instead of recursing forever (a simple path visits at most
    // `templates.len()` distinct templates; the guard allows one extra level).
    assert!(
        !result.meshes.is_empty(),
        "fallback must surface at least the self-recursive template"
    );
    assert!(
        result.meshes.len() <= compiled.templates.len() + 1,
        "cycle guard must bound the recursive walk to a finite surface count; got {} surfaces: {:?}",
        result.meshes.len(),
        paths
    );
}

/// Amendment (reviewer robustness_edge_case, suggestion 2 — Mock): a MUTUAL
/// containment cycle (`A` subs `B`, `B` subs `A`) with no acyclic entry point.
/// Neither template is a root, and neither is reachable from a root, so both
/// would be silently dropped without the fallback.
///
/// Locks the fallback DEDUP contract: the driver seeds the first uncovered
/// template (`A`, declaration order) as a fallback root, whose walk also covers
/// its cycle peer `B`, so `B` is NOT seeded a second time as its own standalone
/// root. Thus `A` surfaces standalone (`A#realization[0]`), `B`'s geometry
/// surfaces as `A`'s descendant (`A.b#realization[0]` — not lost), and there is
/// NO standalone `B#realization[0]`. The `depth > templates.len()` cycle guard
/// keeps the surface count finite.
#[test]
fn mutual_recursion_cycle_surfaces_via_single_fallback_seed() {
    let source = r#"structure A {
    let abody = box(20mm, 20mm, 20mm)
    sub b : B
}
structure B {
    let bbody = box(10mm, 10mm, 10mm)
    sub a : A
}"#;
    let compiled = compile_source_with_stdlib(source);

    let mut engine = mock_engine();
    let result = engine.tessellate_realizations(&compiled);
    let tess_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        tess_errors.is_empty(),
        "fallback surfacing must not introduce tessellation errors: {:?}",
        tess_errors
    );

    let paths: Vec<&str> = result
        .meshes
        .iter()
        .map(|s| s.entity_path.as_str())
        .collect();

    // `A` (first declared, hence first uncovered) is seeded as the fallback root.
    assert!(
        paths.contains(&"A#realization[0]"),
        "`A` must surface standalone via the fallback; got {:?}",
        paths
    );
    // `B`'s geometry is preserved — it surfaces as `A`'s descendant.
    assert!(
        paths.contains(&"A.b#realization[0]"),
        "`B` geometry must surface under `A`'s walk (`A.b#realization[0]`); got {:?}",
        paths
    );
    // DEDUP: `B` is covered by `A`'s walk, so it is NOT re-seeded standalone.
    assert!(
        !paths.contains(&"B#realization[0]"),
        "`B` must NOT be seeded as its own fallback root (covered by `A`); got {:?}",
        paths
    );
    // The cycle guard bounds the walk to a finite surface count.
    assert!(
        result.meshes.len() <= compiled.templates.len() + 1,
        "cycle guard must bound the mutual-recursion walk; got {} surfaces: {:?}",
        result.meshes.len(),
        paths
    );
}

/// step-3 (T7, real OCCT, `OCCT_AVAILABLE`-gated) — RED: `engine.build()` on
/// a 2-product + 1-aux assembly must export exactly **2** solids in the STEP
/// output (one per product sub, with composed world transforms baked in); the
/// `aux` body must be ABSENT (excluded → not 3), and the un-wired single-last-
/// handle export must NOT produce only 1 un-placed solid.
///
/// Golden geometry (box(20mm) centered at origin, half-extent 0.01 m):
///   Assembly.a  placed at +10 mm X  → world centre (0.01, 0, 0)
///   Assembly.b  placed at +50 mm X  → world centre (0.05, 0, 0)
///   Assembly.marker (aux)  placed at +100 mm Y  — excluded from export
///
/// Fails on base because `Engine::build` exports only `*step_handles.last()`
/// — a single un-placed solid — not the two placed product bodies.
#[test]
fn multi_body_export_has_two_product_solids_not_three_not_one() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure Child {
    let body = box(20mm, 20mm, 20mm)
}
structure AuxPart {
    let body = box(5mm, 5mm, 5mm)
}
structure Assembly {
    sub a : Child at transform3(orient_identity(), vec3(10mm, 0mm, 0mm))
    sub b : Child at transform3(orient_identity(), vec3(50mm, 0mm, 0mm))
    aux sub marker : AuxPart at transform3(orient_identity(), vec3(0mm, 100mm, 0mm))
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

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    let result = engine.build(&compiled, ExportFormat::Step);
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors.is_empty(),
        "unexpected geometry errors: {:?}",
        geom_errors
    );

    let step_bytes = result
        .geometry_output
        .expect("build must produce geometry output for an assembly with product subs");
    let step_str = String::from_utf8(step_bytes).expect("STEP output must be valid UTF-8");

    // Count manifold solid B-Reps: each solid body appears as one
    // MANIFOLD_SOLID_BREP entity in the STEP data section.
    let solid_count = step_str.matches("MANIFOLD_SOLID_BREP(").count();
    assert_eq!(
        solid_count, 2,
        "exported STEP must contain exactly 2 product solids (aux excluded); \
         got {solid_count} MANIFOLD_SOLID_BREP entities.\n\
         (1 → old last-handle bug; 3 → aux not excluded)"
    );
}

/// Regression lock (T7): a single-body structure still exports exactly 1
/// solid after the placed-product export path is wired in — the single-body
/// path (0 sub children) must not be wrapped in a compound or otherwise
/// regressed.
///
/// This test is GREEN on base (the old `*step_handles.last()` export produces
/// 1 solid for single-body structures) and must remain GREEN after step-4.
#[test]
fn single_body_export_regression_one_solid() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure Bracket {
    let body = box(30mm, 20mm, 10mm)
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

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    let result = engine.build(&compiled, ExportFormat::Step);
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors.is_empty(),
        "unexpected geometry errors: {:?}",
        geom_errors
    );

    let step_bytes = result
        .geometry_output
        .expect("single-body build must produce geometry output");
    let step_str = String::from_utf8(step_bytes).expect("STEP output must be valid UTF-8");

    let solid_count = step_str.matches("MANIFOLD_SOLID_BREP(").count();
    assert_eq!(
        solid_count, 1,
        "single-body structure must export exactly 1 solid; got {solid_count}"
    );
}
