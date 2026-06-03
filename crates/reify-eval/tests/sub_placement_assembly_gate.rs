//! Sub-placement T10 integration gate (task 3908).
//!
//! Reads the committed example `examples/sub_placement_assembly.ri` and asserts
//! the §8 boundary conditions end-to-end:
//!
//! - **Compile-cleanliness**: zero Error-severity diagnostics.
//! - **Structural surfacing** (Mock-kernel, ACTIVE in CI): composed entity paths,
//!   `default_visible` flags, no double-surfacing for contained children.
//! - **World-coord AABBs** (OCCT-gated, `#[ignore]`): depth-0/1/2 composed
//!   transforms match exact box extents at their world poses.
//! - **Placed-distance queries** (OCCT-gated, `#[ignore]`): §8.4 placed-world
//!   face gaps between the three product solids.
//!
//! Steps 1-2 cover the visible arm→motor→shaft chain.
//! Steps 3-4 extend coverage to the aux fixture.
//! Step 5 locks the distance-query seam (§8.4).
//!
//! All OCCT-dependent tests follow the `#[ignore = "requires OCCT"]` convention
//! established by `sub_placement_surfacing.rs` (T5/T7).

use reify_core::Severity;
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib};

/// Path to the committed example, relative to this crate's manifest.
///
/// The crate lives at `<root>/crates/reify-eval`; the examples directory is
/// two levels up.  The path is evaluated at compile time via `env!`.
const EXAMPLE_SRC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/sub_placement_assembly.ri"
);

// ── Engine builder helpers (copied from sub_placement_surfacing.rs) ───────────

/// Build a Mock-kernel engine for structural surfacing assertions.
fn mock_engine() -> reify_eval::Engine {
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Build a real-OCCT engine via the production `SingleKernelHolder` planner.
///
/// Use for tessellation tests where a single-kernel planner suffices.
fn occt_engine_via_holder() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)))
}

/// Build a real-OCCT engine using `OcctKernelHandle` directly (no holder).
///
/// Required for `build()` and `distance_between_placed()` tests where
/// `make_compound` must be reachable on the kernel itself (step-5).
fn occt_engine_direct() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    reify_eval::Engine::new(
        Box::new(checker),
        Some(Box::new(reify_kernel_occt::OcctKernelHandle::spawn())),
    )
}

// ── Path-resolution helpers (copied from sub_placement_surfacing.rs) ──────────

/// Resolve the `entity_path` a realization named `name` surfaces under on the
/// flat (root) path: `<Entity>#realization[<index>]`.
fn root_realization_path(template: &reify_compiler::TopologyTemplate, name: &str) -> String {
    template
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("realization for `{name}` not found in template"))
        .id
        .to_string()
}

/// Compose the descendant `entity_path` `<prefix>#realization[<index>]` for the
/// realization named `name` in `template`, using the realization's own index.
fn composed_path(template: &reify_compiler::TopologyTemplate, prefix: &str, name: &str) -> String {
    let index = template
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("realization for `{name}` not found in template"))
        .id
        .index;
    format!("{prefix}#realization[{index}]")
}

// ── Mesh geometry helper ──────────────────────────────────────────────────────

/// Compute the axis-aligned bounding-box of a mesh as `(min, max)` over the flat
/// vertex buffer.  Panics if the mesh has no vertices.
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

// ═══════════════════════════════════════════════════════════════════════════════
// step-1: Compile-cleanliness
// ═══════════════════════════════════════════════════════════════════════════════

/// Compile the committed example and assert zero Error-severity diagnostics.
#[test]
fn example_compiles_clean() {
    let source = std::fs::read_to_string(EXAMPLE_SRC)
        .expect("examples/sub_placement_assembly.ri must exist (create it in step-2)");
    let compiled = compile_source_with_stdlib(&source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected compile errors in examples/sub_placement_assembly.ri: {:?}",
        errors
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// step-1: Mock-kernel structural surfacing — visible product chain
// ═══════════════════════════════════════════════════════════════════════════════

/// Mock-kernel gate: the 3-level visible assembly (Arm → Motor → Shaft) surfaces
/// exactly **3** meshes — one per product body — each at its composed entity path
/// with `default_visible == true`.
///
/// Asserts:
/// - `Arm#realization[0]` present, visible (depth 0 — Arm's own body).
/// - `Arm.motor#realization[0]` present, visible (Motor's body at depth 1).
/// - `Arm.motor.shaft#realization[0]` present, visible (Shaft's body at depth 2).
/// - No standalone `Motor#realization[0]` or `Shaft#realization[0]` (contained
///   children are suppressed from the root set).
/// - Total surface count >= 3 (exact count == 4 is locked by
///   `assembly_has_four_surfaces_with_aux_hidden`).
///
/// ACTIVE (no `#[ignore]`): Mock-kernel runs without OCCT.
#[test]
fn visible_chain_surfaces_at_composed_paths() {
    let source = std::fs::read_to_string(EXAMPLE_SRC)
        .expect("examples/sub_placement_assembly.ri must exist (create it in step-2)");
    let compiled = compile_source_with_stdlib(&source);

    let arm = compiled
        .templates
        .iter()
        .find(|t| t.name == "Arm")
        .expect("Arm template not found in compiled module");
    let motor = compiled
        .templates
        .iter()
        .find(|t| t.name == "Motor")
        .expect("Motor template not found in compiled module");
    let shaft = compiled
        .templates
        .iter()
        .find(|t| t.name == "Shaft")
        .expect("Shaft template not found in compiled module");

    let arm_path = root_realization_path(arm, "body");
    let motor_path = composed_path(motor, "Arm.motor", "body");
    let shaft_path = composed_path(shaft, "Arm.motor.shaft", "body");

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

    // No standalone duplicates of contained children (suppression check).
    assert!(
        !paths.contains(&"Motor#realization[0]"),
        "standalone `Motor#realization[0]` must be suppressed (no double-surfacing); got {:?}",
        paths
    );
    assert!(
        !paths.contains(&"Shaft#realization[0]"),
        "standalone `Shaft#realization[0]` must be suppressed (no double-surfacing); got {:?}",
        paths
    );

    // Each product body surfaces at its composed path with default_visible == true.
    for (label, expected_path) in [
        ("arm (depth 0)", &arm_path),
        ("motor (depth 1)", &motor_path),
        ("shaft (depth 2)", &shaft_path),
    ] {
        let surface = result
            .meshes
            .iter()
            .find(|s| &s.entity_path == expected_path)
            .unwrap_or_else(|| {
                panic!(
                    "{label} surface `{expected_path}` must be present; got {:?}",
                    paths
                )
            });
        assert!(
            surface.default_visible,
            "{label} surface `{expected_path}` must have default_visible == true"
        );
    }

    // Exact surface-count ownership is in `assembly_has_four_surfaces_with_aux_hidden`
    // (step-3), which asserts 4 once the aux fixture is added.  Here we only pin
    // that AT LEAST the 3 product surfaces are present.
    assert!(
        result.meshes.len() >= 3,
        "at least 3 product surfaces expected; got {:?}",
        paths
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// step-1: OCCT-gated golden-AABB for the visible product chain
// ═══════════════════════════════════════════════════════════════════════════════

/// OCCT gate: each product body surfaces at its composed world AABB.
///
/// All boxes are OCCT-centered (placed at origin, half-extent each side):
///
/// | Body  | Dimensions     | World center  | AABB min            | AABB max           |
/// |-------|----------------|---------------|---------------------|--------------------|
/// | Arm   | 40×20×20 mm    | (0, 0, 0)     | (−0.02,−0.01,−0.01) | (0.02,0.01,0.01)   |
/// | Motor | 40×30×30 mm    | (0.1, 0, 0)   | (0.08,−0.015,−0.015)| (0.12,0.015,0.015) |
/// | Shaft | 40×10×10 mm    | (0.2, 0, 0)   | (0.18,−0.005,−0.005)| (0.22,0.005,0.005) |
///
/// Shaft at depth 2: pose chain = Arm(identity) ∘ Motor(+100 mm X) ∘ Shaft(+100 mm X)
/// → composed offset +200 mm X.  Proves PRD §4.2 arbitrary-depth composition.
///
/// Tolerance 1e-5 m (f32): box vertices land exactly at corners (no curvature
/// deflection), matching the achievability basis used by T5/T7 tests.
#[test]
#[ignore = "requires OCCT"]
fn product_bodies_surface_at_composed_world_aabb() {
    let source = std::fs::read_to_string(EXAMPLE_SRC)
        .expect("examples/sub_placement_assembly.ri must exist");
    let compiled = compile_source_with_stdlib(&source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let arm = compiled.templates.iter().find(|t| t.name == "Arm").expect("Arm");
    let motor = compiled.templates.iter().find(|t| t.name == "Motor").expect("Motor");
    let shaft = compiled.templates.iter().find(|t| t.name == "Shaft").expect("Shaft");

    let arm_path = root_realization_path(arm, "body");
    let motor_path = composed_path(motor, "Arm.motor", "body");
    let shaft_path = composed_path(shaft, "Arm.motor.shaft", "body");

    let mut engine = occt_engine_via_holder();
    let result = engine.tessellate_realizations(&compiled);
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(geom_errors.is_empty(), "geometry errors: {:?}", geom_errors);

    let tol = 1e-5_f32;

    // (label, entity_path, expected_min, expected_max)
    let checks: &[(&str, &str, [f32; 3], [f32; 3])] = &[
        (
            "arm (depth 0)",
            &arm_path,
            [-0.02, -0.01, -0.01],
            [0.02, 0.01, 0.01],
        ),
        (
            "motor (depth 1, +100 mm X)",
            &motor_path,
            [0.08, -0.015, -0.015],
            [0.12, 0.015, 0.015],
        ),
        (
            "shaft (depth 2, +200 mm X composed)",
            &shaft_path,
            [0.18, -0.005, -0.005],
            [0.22, 0.005, 0.005],
        ),
    ];

    for (label, path, exp_min, exp_max) in checks {
        let surface = result
            .meshes
            .iter()
            .find(|s| s.entity_path == *path)
            .unwrap_or_else(|| {
                let all: Vec<&str> = result.meshes.iter().map(|s| s.entity_path.as_str()).collect();
                panic!("{label} surface `{path}` not found; surfaces: {:?}", all)
            });
        let (got_min, got_max) = mesh_aabb(&surface.mesh);
        for axis in 0..3 {
            assert!(
                (got_min[axis] - exp_min[axis]).abs() < tol,
                "{label} min[{axis}] expected {}, got {} (composed transform not applied?)",
                exp_min[axis],
                got_min[axis]
            );
            assert!(
                (got_max[axis] - exp_max[axis]).abs() < tol,
                "{label} max[{axis}] expected {}, got {}",
                exp_max[axis],
                got_max[axis]
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// step-3: Mock-kernel structural surfacing — full 4-surface set with aux fixture
// ═══════════════════════════════════════════════════════════════════════════════

/// Mock-kernel gate: the complete assembly (arm + motor + shaft + aux fixture)
/// surfaces exactly **4** meshes.
///
/// Asserts:
/// - Total surface count == 4.
/// - `Arm.fixture#realization[0]` is present with `default_visible == false` and
///   a NON-EMPTY mesh payload (realized + tessellated + shipped, not skipped).
/// - The three product surfaces remain `default_visible == true`.
/// - No standalone `Fixture#realization[0]` (contained child suppressed).
///
/// ACTIVE (no `#[ignore]`): Mock-kernel runs without OCCT.
#[test]
fn assembly_has_four_surfaces_with_aux_hidden() {
    let source = std::fs::read_to_string(EXAMPLE_SRC)
        .expect("examples/sub_placement_assembly.ri must exist");
    let compiled = compile_source_with_stdlib(&source);

    let arm = compiled
        .templates
        .iter()
        .find(|t| t.name == "Arm")
        .expect("Arm template not found");
    let motor = compiled
        .templates
        .iter()
        .find(|t| t.name == "Motor")
        .expect("Motor template not found");
    let shaft = compiled
        .templates
        .iter()
        .find(|t| t.name == "Shaft")
        .expect("Shaft template not found");
    let fixture = compiled
        .templates
        .iter()
        .find(|t| t.name == "Fixture")
        .expect("Fixture template not found in compiled module");

    let arm_path = root_realization_path(arm, "body");
    let motor_path = composed_path(motor, "Arm.motor", "body");
    let shaft_path = composed_path(shaft, "Arm.motor.shaft", "body");
    let fixture_path = composed_path(fixture, "Arm.fixture", "body");

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

    // Exactly 4 surfaces: 3 product + 1 aux fixture.
    assert_eq!(
        result.meshes.len(),
        4,
        "exactly 4 surfaces expected (3 product + 1 aux fixture); got {:?}",
        paths
    );

    // No standalone duplicate of the contained fixture child.
    assert!(
        !paths.contains(&"Fixture#realization[0]"),
        "standalone `Fixture#realization[0]` must be suppressed; got {:?}",
        paths
    );

    // The aux fixture surfaces under the composed path with default_visible == false
    // and a NON-EMPTY mesh payload (it is realized + tessellated, just hidden).
    let fixture_surface = result
        .meshes
        .iter()
        .find(|s| s.entity_path == fixture_path)
        .unwrap_or_else(|| {
            panic!(
                "aux fixture surface `{fixture_path}` must be present; got {:?}",
                paths
            )
        });
    assert!(
        !fixture_surface.default_visible,
        "aux fixture must surface with default_visible == false (hidden, not skipped)"
    );
    assert!(
        !fixture_surface.mesh.vertices.is_empty(),
        "aux fixture mesh payload must still be shipped (realized + tessellated)"
    );

    // The three product surfaces remain default_visible == true.
    for (label, expected_path) in [
        ("arm (depth 0)", &arm_path),
        ("motor (depth 1)", &motor_path),
        ("shaft (depth 2)", &shaft_path),
    ] {
        let surface = result
            .meshes
            .iter()
            .find(|s| &s.entity_path == expected_path)
            .unwrap_or_else(|| {
                panic!("{label} surface `{expected_path}` must be present; got {:?}", paths)
            });
        assert!(
            surface.default_visible,
            "{label} surface must remain default_visible == true"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// step-3: OCCT-gated golden-AABB for the aux fixture
// ═══════════════════════════════════════════════════════════════════════════════

/// OCCT gate: the aux fixture body surfaces at its composed world AABB with
/// `default_visible == false`.
///
/// Fixture: `box(8mm,8mm,8mm)` centered at origin, placed `+100 mm Y` from Arm.
/// World center: (0, 0.1, 0).
/// AABB: X[−0.004, 0.004]  Y[0.096, 0.104]  Z[−0.004, 0.004].
///
/// Proves that aux subs are REALIZED + TRANSFORMED + TESSELLATED (not skipped)
/// even though they are hidden from the default view.
#[test]
#[ignore = "requires OCCT"]
fn aux_fixture_surfaces_at_composed_world_aabb() {
    let source = std::fs::read_to_string(EXAMPLE_SRC)
        .expect("examples/sub_placement_assembly.ri must exist");
    let compiled = compile_source_with_stdlib(&source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let fixture = compiled
        .templates
        .iter()
        .find(|t| t.name == "Fixture")
        .expect("Fixture template not found");

    let fixture_path = composed_path(fixture, "Arm.fixture", "body");

    let mut engine = occt_engine_via_holder();
    let result = engine.tessellate_realizations(&compiled);
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(geom_errors.is_empty(), "geometry errors: {:?}", geom_errors);

    let fixture_surface = result
        .meshes
        .iter()
        .find(|s| s.entity_path == fixture_path)
        .unwrap_or_else(|| {
            let all: Vec<&str> = result.meshes.iter().map(|s| s.entity_path.as_str()).collect();
            panic!(
                "aux fixture surface `{fixture_path}` must be present; surfaces: {:?}",
                all
            )
        });

    // The fixture is aux → hidden but payload present.
    assert!(
        !fixture_surface.default_visible,
        "aux fixture must surface with default_visible == false"
    );
    assert!(
        !fixture_surface.mesh.vertices.is_empty(),
        "aux fixture mesh payload must be shipped (placement still applied)"
    );

    // Golden AABB: box(8mm) centered at origin translated +100 mm Y.
    // Half-extent = 4 mm = 0.004 m; center Y = 0.1 m.
    let (got_min, got_max) = mesh_aabb(&fixture_surface.mesh);
    let exp_min = [-0.004_f32, 0.096, -0.004];
    let exp_max = [0.004_f32, 0.104, 0.004];
    let tol = 1e-5_f32;

    for axis in 0..3 {
        assert!(
            (got_min[axis] - exp_min[axis]).abs() < tol,
            "fixture min[{axis}] expected {}, got {} (aux placement not applied?)",
            exp_min[axis],
            got_min[axis]
        );
        assert!(
            (got_max[axis] - exp_max[axis]).abs() < tol,
            "fixture max[{axis}] expected {}, got {}",
            exp_max[axis],
            got_max[axis]
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// step-5: §8.4 placed-distance seam gate (OCCT-gated)
// ═══════════════════════════════════════════════════════════════════════════════

/// OCCT seam-lock gate: `distance_between_placed` returns the PLACED world face
/// gaps between each pair of product solids in the committed example.
///
/// All three solids are collinear on the X axis with full Y/Z overlap, so
/// BRepExtrema computes exact planar-face distances:
///
/// | Pair                          | World faces         | Gap    |
/// |-------------------------------|---------------------|--------|
/// | Arm ↔ Arm.motor               | 0.02 m ↔ 0.08 m    | 0.06 m |
/// | Arm.motor ↔ Arm.motor.shaft   | 0.12 m ↔ 0.18 m    | 0.06 m |
/// | Arm ↔ Arm.motor.shaft         | 0.02 m ↔ 0.18 m    | 0.16 m |
///
/// Tolerance 1e-6 m (f64): planar-face BRepExtrema is exact for aligned boxes.
///
/// This test is a seam-lock (capability delivered by T7/3905); no production
/// code change is required.
#[test]
#[ignore = "requires OCCT"]
fn placed_distances_match_composed_world_gaps() {
    let source = std::fs::read_to_string(EXAMPLE_SRC)
        .expect("examples/sub_placement_assembly.ri must exist");
    let compiled = compile_source_with_stdlib(&source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let arm = compiled.templates.iter().find(|t| t.name == "Arm").expect("Arm");
    let motor = compiled.templates.iter().find(|t| t.name == "Motor").expect("Motor");
    let shaft = compiled.templates.iter().find(|t| t.name == "Shaft").expect("Shaft");

    let arm_path = root_realization_path(arm, "body");
    let motor_path = composed_path(motor, "Arm.motor", "body");
    let shaft_path = composed_path(shaft, "Arm.motor.shaft", "body");

    let mut engine = occt_engine_direct();

    let tol = 1e-6_f64;

    // Arm ↔ Arm.motor: face gap = 0.08 − 0.02 = 0.06 m.
    let d_arm_motor = engine
        .distance_between_placed(&compiled, &arm_path, &motor_path)
        .expect("distance_between_placed(arm, motor) must return Some");
    assert!(
        (d_arm_motor - 0.06).abs() < tol,
        "arm↔motor distance expected 0.06 m, got {d_arm_motor} m \
         (composed placement not applied to distance query?)"
    );

    // Arm.motor ↔ Arm.motor.shaft: face gap = 0.18 − 0.12 = 0.06 m.
    let d_motor_shaft = engine
        .distance_between_placed(&compiled, &motor_path, &shaft_path)
        .expect("distance_between_placed(motor, shaft) must return Some");
    assert!(
        (d_motor_shaft - 0.06).abs() < tol,
        "motor↔shaft distance expected 0.06 m, got {d_motor_shaft} m"
    );

    // Arm ↔ Arm.motor.shaft: face gap = 0.18 − 0.02 = 0.16 m.
    let d_arm_shaft = engine
        .distance_between_placed(&compiled, &arm_path, &shaft_path)
        .expect("distance_between_placed(arm, shaft) must return Some");
    assert!(
        (d_arm_shaft - 0.16).abs() < tol,
        "arm↔shaft distance expected 0.16 m, got {d_arm_shaft} m \
         (3-level compose_pose_chain not reflected in distance?)"
    );
}
