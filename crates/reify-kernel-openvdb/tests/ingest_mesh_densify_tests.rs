//! Tests for `GeometryKernel::ingest_mesh` override and
//! `OpenVdbKernel::densify_grid_to_sampled` on the real OpenVDB kernel.
//!
//! Structure follows the `cfg(has_openvdb)` real-test / `cfg(not(has_openvdb))`
//! skip-stub pattern from `tests/realize_voxel_tests.rs` so the test count is
//! stable across stub builds.

// ---------------------------------------------------------------------------
// Shared fixture
// ---------------------------------------------------------------------------

/// Build a closed 2.0 mm box mesh centred at the origin (vertices at ±1.0).
/// 8 corners, 12 outward-wound triangles.
///
/// This is the canonical α test fixture (PRD §α "2.0 mm box", φ(centre)
/// ≈ −1.0 mm).
fn box_2mm() -> reify_ir::Mesh {
    let v: Vec<f32> = vec![
        -1.0, -1.0, -1.0, // 0
         1.0, -1.0, -1.0, // 1
         1.0,  1.0, -1.0, // 2
        -1.0,  1.0, -1.0, // 3
        -1.0, -1.0,  1.0, // 4
         1.0, -1.0,  1.0, // 5
         1.0,  1.0,  1.0, // 6
        -1.0,  1.0,  1.0, // 7
    ];
    #[rustfmt::skip]
    let i: Vec<u32> = vec![
        // Bottom (-Z)
        0, 2, 1,  0, 3, 2,
        // Top (+Z)
        4, 5, 6,  4, 6, 7,
        // Front (-Y)
        0, 1, 5,  0, 5, 4,
        // Back (+Y)
        2, 3, 7,  2, 7, 6,
        // Left (-X)
        0, 4, 7,  0, 7, 3,
        // Right (+X)
        1, 2, 6,  1, 6, 5,
    ];
    reify_ir::Mesh { vertices: v, indices: i, normals: None }
}

// ---------------------------------------------------------------------------
// ingest_mesh tests
// ---------------------------------------------------------------------------

/// `GeometryKernel::ingest_mesh` with a valid closed box mesh must:
/// - return `Ok(handle)` with a non-INVALID id,
/// - set `handle.repr = None` (Voxel kernel — no BRep sub-shape),
/// - register active voxels (`active_voxel_count(handle.id) > 0`).
///
/// RED: the trait default returns `Err(OperationFailed("… does not accept
/// Mesh inputs"))`, so the `Ok(handle)` assertion fails.
#[cfg(has_openvdb)]
#[test]
fn ingest_mesh_valid_box_returns_handle_with_no_repr_and_active_voxels() {
    use reify_ir::{GeometryHandleId, GeometryKernel};
    use reify_kernel_openvdb::OpenVdbKernel;

    let mesh = box_2mm();
    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .ingest_mesh(&mesh)
        .expect("ingest_mesh must succeed for a valid closed box");

    assert!(
        handle.id != GeometryHandleId::INVALID,
        "ingest_mesh must return a valid (non-INVALID) GeometryHandleId"
    );
    assert!(
        handle.repr.is_none(),
        "ingest_mesh on OpenVdbKernel must set repr=None (Voxel kernel has no BRep sub-shape)"
    );

    let count = kernel
        .active_voxel_count(handle.id)
        .expect("active_voxel_count must succeed for a freshly-ingested handle");
    assert!(
        count > 0,
        "active_voxel_count must be > 0 after ingesting a valid closed mesh"
    );
}

/// `ingest_mesh` with an empty mesh (no vertices) must return
/// `Err(GeometryError::OperationFailed(_))` because `honest_floor` returns
/// `None` for an empty mesh.
#[cfg(has_openvdb)]
#[test]
fn ingest_mesh_empty_mesh_returns_operation_failed() {
    use reify_ir::{GeometryError, GeometryKernel, Mesh};
    use reify_kernel_openvdb::OpenVdbKernel;

    let mesh = Mesh { vertices: vec![], indices: vec![], normals: None };
    let mut kernel = OpenVdbKernel::new();
    let result = kernel.ingest_mesh(&mesh);
    assert!(
        matches!(result, Err(GeometryError::OperationFailed(_))),
        "ingest_mesh must return Err(OperationFailed) for an empty mesh; got {result:?}"
    );
}

/// `cfg(not(has_openvdb))` skip-stub for the ingest_mesh tests.
#[cfg(not(has_openvdb))]
#[test]
fn ingest_mesh_skipped_without_cfg() {
    println!("ingest_mesh_densify_tests: has_openvdb cfg not set, skipping ingest_mesh tests");
    assert!(true);
}

// ---------------------------------------------------------------------------
// densify_grid_to_sampled tests  (step-5 RED — added later)
// ---------------------------------------------------------------------------
// (populated in step-5)
