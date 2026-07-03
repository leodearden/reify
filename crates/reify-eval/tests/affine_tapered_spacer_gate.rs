//! AffineMap stdlib integration gate (task 3965, PRD
//! `docs/prds/v0_6/affine-map-type.md` task η, "C-as-integration-gate").
//!
//! Reads the committed example `examples/affine_tapered_spacer.ri` and
//! asserts the end-to-end signal tying tasks α–ζ together:
//!
//! - **Compile-cleanliness**: zero Error-severity diagnostics.
//! - **Mock-kernel surfacing** (ACTIVE, no OCCT required): the deformed
//!   `body` realization tessellates to a non-empty mesh at its composed
//!   entity path.
//! - **OCCT golden AABB** (`#[ignore = "requires OCCT"]`): the Z-stretched
//!   `body` solid's tessellated AABB extents match the analytic
//!   `box(20,20,10mm) · diag(1,1,1.5)` golden of `[0.02, 0.02, 0.015]` m.
//!
//! Modeled on `sub_placement_assembly_gate.rs` (task 3908) and
//! `affine_apply_e2e.rs` (task 3963).

use reify_core::Severity;
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib};

/// Path to the committed example, relative to this crate's manifest.
///
/// The crate lives at `<root>/crates/reify-eval`; the examples directory is
/// two levels up. The path is evaluated at compile time via `env!`.
const EXAMPLE_SRC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/affine_tapered_spacer.ri"
);

/// Read and compile the committed example, panicking with a helpful message
/// if the file is missing. Shared by all three gate tests below to avoid
/// repeating the read+compile boilerplate.
fn compiled_example() -> reify_compiler::CompiledModule {
    let source = std::fs::read_to_string(EXAMPLE_SRC)
        .expect("examples/affine_tapered_spacer.ri must exist (created in step-2)");
    compile_source_with_stdlib(&source)
}

// ── Engine builder helpers (copied from sub_placement_assembly_gate.rs) ───────

/// Build a Mock-kernel engine for structural surfacing assertions.
fn mock_engine() -> reify_eval::Engine {
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Build a real-OCCT engine via the production `SingleKernelHolder` planner.
fn occt_engine_via_holder() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)))
}

// ── Path-resolution helper (copied from sub_placement_assembly_gate.rs) ──────

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

// ── Mesh geometry helper (copied from sub_placement_assembly_gate.rs) ────────

/// Compute the axis-aligned bounding-box of a mesh as `(min, max)` over the flat
/// vertex buffer. Panics if the mesh has no vertices.
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
    let compiled = compiled_example();
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected compile errors in examples/affine_tapered_spacer.ri: {:?}",
        errors
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// step-1: Mock-kernel structural surfacing — deformed body
// ═══════════════════════════════════════════════════════════════════════════════

/// Mock-kernel gate: the `body` realization (Z-stretched via `affine_apply`)
/// tessellates to a non-empty mesh at its exact entity path.
///
/// This is a *structural* signal only: `MockGeometryKernel` returns a fixed
/// placeholder mesh regardless of input geometry, so a non-empty payload here
/// does not verify that the Z-stretch was numerically applied — only that the
/// `body` realization surfaces at all under tessellation. The numeric
/// deformation itself is pinned by the OCCT-gated `body_z_stretch_aabb_occt`
/// below.
///
/// ACTIVE (no `#[ignore]`): Mock-kernel runs without OCCT.
#[test]
fn deformed_body_surfaces_under_mock() {
    let compiled = compiled_example();

    let spacer = compiled
        .templates
        .iter()
        .find(|t| t.name == "TaperedSpacer")
        .expect("TaperedSpacer template not found in compiled module");
    let body_path = root_realization_path(spacer, "body");

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
        .find(|s| s.entity_path == body_path)
        .unwrap_or_else(|| {
            panic!(
                "expected a mesh with entity_path `{body_path}`; got: {:?}",
                result
                    .meshes
                    .iter()
                    .map(|s| &s.entity_path)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        !surface.mesh.vertices.is_empty(),
        "body mesh payload must be non-empty"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// step-1: OCCT-gated golden AABB for the Z-stretched body
// ═══════════════════════════════════════════════════════════════════════════════

/// OCCT gate: `body = affine_apply(box(20mm,20mm,10mm), affine_scale(1,1,1.5))`
/// tessellates to a solid whose AABB EXTENTS (max−min, not absolute corners —
/// robust to OCCT box centering, matching `affine_apply_e2e.rs`) equal the
/// analytic golden `[0.02, 0.02, 0.015]` m within 1e-4 (the same tolerance
/// `affine_apply_non_uniform_scale_occt_acceptance` already proves achievable
/// for this exact box+scale mechanism).
#[test]
#[ignore = "requires OCCT"]
fn body_z_stretch_aabb_occt() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping body_z_stretch_aabb_occt: OCCT not available");
        return;
    }

    let compiled = compiled_example();

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let spacer = compiled
        .templates
        .iter()
        .find(|t| t.name == "TaperedSpacer")
        .expect("TaperedSpacer template not found in compiled module");
    let body_path = root_realization_path(spacer, "body");

    let mut engine = occt_engine_via_holder();
    let result = engine.tessellate_realizations(&compiled);
    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(geom_errors.is_empty(), "geometry errors: {:?}", geom_errors);

    let surface = result
        .meshes
        .iter()
        .find(|s| s.entity_path == body_path)
        .unwrap_or_else(|| {
            let all: Vec<&str> = result.meshes.iter().map(|s| s.entity_path.as_str()).collect();
            panic!("body surface `{body_path}` not found; surfaces: {:?}", all)
        });

    let (min, max) = mesh_aabb(&surface.mesh);
    let extents = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let expected = [0.02_f32, 0.02, 0.015];
    let tol = 1e-4_f32;
    for axis in 0..3 {
        assert!(
            (extents[axis] - expected[axis]).abs() < tol,
            "body AABB extent[{axis}] expected {}, got {} (Z-stretch not applied?)",
            expected[axis],
            extents[axis]
        );
    }
}
