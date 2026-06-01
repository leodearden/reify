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
