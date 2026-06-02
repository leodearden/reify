//! End-to-end acceptance test for geometry-valued if-then-else hoisting.
//!
//! Tests the full pipeline for scalar-arg hoisting of a geometry-typed
//! if-then-else expression:
//!   parse → compile → Engine (with OcctKernelHandle) → tessellate_realizations
//!
//! The test exercises the `Note3IfSolid` repro: an enum-driven conditional
//! selects between two differently-sized boxes. After hoisting, both box
//! variants fold into a single `Primitive{Box}` op with `Conditional` scalar
//! args, and the OCCT tessellator must produce correctly-sized meshes for
//! each enum choice.
//!
//! All tests are guarded by `reify_kernel_occt::OCCT_AVAILABLE` and are
//! skipped if the OCCT library is not present.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Value;

/// Compute the axis-aligned bounding-box of a mesh.
///
/// Returns `(min: [f32; 3], max: [f32; 3])` over the flat vertex buffer.
/// Panics if the mesh has no vertices.
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

/// step-5 e2e: enum-driven geometry conditional produces correctly-sized meshes.
///
/// Source:
///   `enum Pick { A, B }`
///   `structure Note3IfSolid { param pick: Pick = Pick.A … }`
///
/// - Pick.A → box(40mm, 40mm, 40mm) → bbox extents ≈ [0.04, 0.04, 0.04] m
/// - Pick.B → box(80mm, 20mm, 20mm) → sorted extents ≈ [0.02, 0.02, 0.08] m
#[test]
fn geometry_conditional_enum_pick_tessellates_correctly() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"enum Pick { A, B }
structure Note3IfSolid {
    param pick : Pick = Pick.A
    let body = if pick == Pick.A then box(40mm, 40mm, 40mm) else box(80mm, 20mm, 20mm)
}"#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test_geom_conditional"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {:?}",
        compile_errors
    );
    assert_eq!(compiled.templates.len(), 1, "expected 1 template");
    assert!(
        !compiled.templates[0].realizations.is_empty(),
        "expected at least 1 realization"
    );

    // Build engine with real OCCT kernel
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    // ── Pick.A (default) — box(40mm, 40mm, 40mm) ──────────────────────────────

    let result_a = engine.tessellate_realizations(&compiled);

    let geom_errors_a: Vec<_> = result_a
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors_a.is_empty(),
        "Pick.A geometry errors: {:?}",
        geom_errors_a
    );
    assert_eq!(
        result_a.meshes.len(),
        1,
        "Pick.A: expected 1 mesh, got {}",
        result_a.meshes.len()
    );
    let mesh_a = &result_a.meshes[0].mesh;
    assert!(
        !mesh_a.vertices.is_empty(),
        "Pick.A: mesh must have vertices"
    );

    let (min_a, max_a) = mesh_aabb(mesh_a);
    let extents_a = [
        max_a[0] - min_a[0],
        max_a[1] - min_a[1],
        max_a[2] - min_a[2],
    ];
    for (axis, &ext) in extents_a.iter().enumerate() {
        assert!(
            (ext - 0.04_f32).abs() < 1e-4_f32,
            "Pick.A: axis {} extent should be ≈0.04 m, got {:.6}",
            axis,
            ext
        );
    }

    // ── Pick.B — box(80mm, 20mm, 20mm) ────────────────────────────────────────

    let pick_id = ValueCellId::new("Note3IfSolid", "pick");
    engine.set_param_and_invalidate(
        &pick_id,
        Value::Enum {
            type_name: "Pick".to_string(),
            variant: "B".to_string(),
        },
    );

    let result_b = engine.tessellate_realizations(&compiled);

    let geom_errors_b: Vec<_> = result_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors_b.is_empty(),
        "Pick.B geometry errors: {:?}",
        geom_errors_b
    );
    assert_eq!(
        result_b.meshes.len(),
        1,
        "Pick.B: expected 1 mesh, got {}",
        result_b.meshes.len()
    );
    let mesh_b = &result_b.meshes[0].mesh;
    assert!(
        !mesh_b.vertices.is_empty(),
        "Pick.B: mesh must have vertices"
    );

    let (min_b, max_b) = mesh_aabb(mesh_b);
    let mut extents_b = [
        max_b[0] - min_b[0],
        max_b[1] - min_b[1],
        max_b[2] - min_b[2],
    ];
    // Sort so we compare against [0.02, 0.02, 0.08] regardless of axis mapping.
    extents_b.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let expected_b = [0.02_f32, 0.02_f32, 0.08_f32];
    for (axis, (&ext, &exp)) in extents_b.iter().zip(expected_b.iter()).enumerate() {
        assert!(
            (ext - exp).abs() < 1e-4_f32,
            "Pick.B: sorted extent[{}] should be ≈{:.3} m, got {:.6}",
            axis,
            exp,
            ext
        );
    }
}
