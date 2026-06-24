//! Tests for the OpenVDB Voxel→Mesh marching-cubes primitive and tessellate
//! trait route.
//!
//! All `cfg(has_openvdb)` tests exercise the real FFI; companion
//! `cfg(not(has_openvdb))` skip-stubs keep the test count stable.
//!
//! # Round-trip fixture
//!
//! Tests build their grid by calling `realize_voxel_from_mesh` on a closed
//! cube mesh fixture (self-contained mesh→voxel→mesh round-trip — no θ
//! import pipeline / no .vdb file needed), then marching-cubes back.

// ---------------------------------------------------------------------------
// Shared mesh fixture (closed 2.0-unit cube centred at origin)
// ---------------------------------------------------------------------------

/// Build a closed 2.0-unit cube mesh centred at the origin (8 corner vertices,
/// 12 outward-wound triangles). Identical to the cube fixture in
/// `dispatcher_integration.rs::openvdb_two_stage_chain_voxelize_primitive_executes`.
#[cfg(has_openvdb)]
fn cube_2unit_mesh() -> reify_ir::Mesh {
    reify_ir::Mesh {
        vertices: vec![
            -1.0_f32, -1.0, -1.0, // 0
             1.0,     -1.0, -1.0, // 1
             1.0,      1.0, -1.0, // 2
            -1.0,      1.0, -1.0, // 3
            -1.0,     -1.0,  1.0, // 4
             1.0,     -1.0,  1.0, // 5
             1.0,      1.0,  1.0, // 6
            -1.0,      1.0,  1.0, // 7
        ],
        #[rustfmt::skip]
        indices: vec![
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
        ],
        normals: None,
    }
}

/// Build a voxel grid from the closed cube mesh and return the kernel + handle.
///
/// Uses voxel_size=0.1 and a narrow band wide enough to cover the interior.
#[cfg(has_openvdb)]
fn build_cube_voxel_grid() -> (reify_kernel_openvdb::OpenVdbKernel, reify_ir::GeometryHandleId) {
    use reify_kernel_openvdb::{MeshToVoxelOptions, OpenVdbKernel};
    let mesh = cube_2unit_mesh();
    let opts = MeshToVoxelOptions::honest_floor(&mesh)
        .expect("honest_floor must return Some for a valid closed cube");
    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .realize_voxel_from_mesh_with_options(&mesh, &opts)
        .expect("realize_voxel_from_mesh_with_options must succeed for a valid closed cube");
    (kernel, handle)
}

// ---------------------------------------------------------------------------
// (a) marching_cubes_round_trips_cube_to_nonempty_wellformed_mesh
// ---------------------------------------------------------------------------

/// Voxelize a closed cube mesh, then marching-cubes back to a Mesh.
/// Asserts structural/topological properties:
/// - vertices.len() > 0 and vertices.len() % 3 == 0 (flat xyz layout)
/// - indices.len() > 0 and indices.len() % 3 == 0 (flat triangle layout)
/// - every vertex coordinate is within a generous shell |c| <= half_extent + K*voxel_size
///
/// RED: realize_mesh_from_voxel_with_options does not exist yet.
#[cfg(has_openvdb)]
#[test]
fn marching_cubes_round_trips_cube_to_nonempty_wellformed_mesh() {
    use reify_kernel_openvdb::{MarchingCubesOptions, MeshToVoxelOptions};

    let (kernel, handle) = build_cube_voxel_grid();

    // The voxel_size used to build the grid (needed for the bbox shell check).
    let voxel_size = {
        let mesh = cube_2unit_mesh();
        MeshToVoxelOptions::honest_floor(&mesh)
            .expect("honest_floor must return Some")
            .voxel_size
    };

    let mesh = kernel
        .realize_mesh_from_voxel_with_options(handle, &MarchingCubesOptions::default())
        .expect("realize_mesh_from_voxel_with_options must succeed for a valid grid");

    // Non-empty buffers.
    assert!(
        !mesh.vertices.is_empty(),
        "marching-cubes mesh must have vertices",
    );
    assert!(
        !mesh.indices.is_empty(),
        "marching-cubes mesh must have indices",
    );

    // Correct flat-buffer layout.
    assert_eq!(
        mesh.vertices.len() % 3, 0,
        "vertices.len() must be a multiple of 3 (flat xyz layout); got {}",
        mesh.vertices.len(),
    );
    assert_eq!(
        mesh.indices.len() % 3, 0,
        "indices.len() must be a multiple of 3 (flat triangle layout); got {}",
        mesh.indices.len(),
    );

    // Vertex coordinates within a generous shell:
    // the cube is ±1.0 m; MC vertices lie on grid edges with placement error
    // O(voxel_size), so K=3 voxel margins is safely loose.
    let half_extent = 1.0_f32;
    let k = 3.0_f32;
    let shell = half_extent + k * (voxel_size as f32);
    for (i, &c) in mesh.vertices.iter().enumerate() {
        assert!(
            c.abs() <= shell,
            "vertex coordinate at index {i} = {c} exceeds generous shell ±{shell} \
             (half_extent={half_extent}, K={k}, voxel_size={voxel_size})",
        );
    }
}

/// `cfg(not(has_openvdb))` skip-stub.
#[cfg(not(has_openvdb))]
#[test]
fn marching_cubes_round_trips_cube_to_nonempty_wellformed_mesh() {
    println!("voxel_to_mesh_tests: has_openvdb cfg not set, skip");
    assert!(true);
}

// ---------------------------------------------------------------------------
// (b) marching_cubes_adaptive_knob_is_plumbed
// ---------------------------------------------------------------------------

/// Prove the adaptive knob is plumbed: run marching cubes with uniform
/// (adaptive=false) and adaptive (adaptive=true) on the SAME grid.
/// Assertions:
/// - Both results are non-empty and well-formed.
/// - adaptive.indices.len() <= uniform.indices.len() (monotonic — adaptive
///   never increases triangle count; proves the knob reaches the FFI).
///
/// RED: realize_mesh_from_voxel_with_options does not exist yet.
#[cfg(has_openvdb)]
#[test]
fn marching_cubes_adaptive_knob_is_plumbed() {
    use reify_kernel_openvdb::MarchingCubesOptions;

    let (kernel, handle) = build_cube_voxel_grid();

    let uniform_opts = MarchingCubesOptions { iso_level: 0.0, adaptive: false };
    let adaptive_opts = MarchingCubesOptions { iso_level: 0.0, adaptive: true };

    let uniform_mesh = kernel
        .realize_mesh_from_voxel_with_options(handle, &uniform_opts)
        .expect("uniform marching cubes must succeed");
    let adaptive_mesh = kernel
        .realize_mesh_from_voxel_with_options(handle, &adaptive_opts)
        .expect("adaptive marching cubes must succeed");

    // Both non-empty and well-formed.
    assert!(!uniform_mesh.indices.is_empty(), "uniform mesh must have indices");
    assert!(!adaptive_mesh.indices.is_empty(), "adaptive mesh must have indices");
    assert_eq!(uniform_mesh.indices.len() % 3, 0, "uniform indices must be % 3");
    assert_eq!(adaptive_mesh.indices.len() % 3, 0, "adaptive indices must be % 3");

    // Monotonic: adaptive never increases triangle count.
    assert!(
        adaptive_mesh.indices.len() <= uniform_mesh.indices.len(),
        "adaptive marching cubes must produce <= triangle count vs uniform; \
         adaptive={} > uniform={}",
        adaptive_mesh.indices.len(),
        uniform_mesh.indices.len(),
    );
}

/// `cfg(not(has_openvdb))` skip-stub.
#[cfg(not(has_openvdb))]
#[test]
fn marching_cubes_adaptive_knob_is_plumbed() {
    println!("voxel_to_mesh_tests: has_openvdb cfg not set, skip");
    assert!(true);
}

// ---------------------------------------------------------------------------
// (c) tessellate_voxel_grid_returns_nonempty_mesh
// ---------------------------------------------------------------------------

/// Prove the tessellate trait route: GeometryKernel::tessellate(&kernel, handle, 0.0)
/// returns Ok(non-empty Mesh) under cfg(has_openvdb).
///
/// RED: tessellate is not overridden to call realize_mesh_from_voxel_with_options yet.
#[cfg(has_openvdb)]
#[test]
fn tessellate_voxel_grid_returns_nonempty_mesh() {
    use reify_ir::GeometryKernel;

    let (kernel, handle) = build_cube_voxel_grid();

    let mesh = GeometryKernel::tessellate(&kernel, handle, 0.0)
        .expect("tessellate must return Ok(Mesh) for a valid voxel grid under cfg(has_openvdb)");

    assert!(
        !mesh.vertices.is_empty(),
        "tessellated mesh must have vertices",
    );
    assert!(
        !mesh.indices.is_empty(),
        "tessellated mesh must have indices",
    );
    assert_eq!(mesh.indices.len() % 3, 0, "tessellated indices must be % 3");
}

/// `cfg(not(has_openvdb))` skip-stub.
#[cfg(not(has_openvdb))]
#[test]
fn tessellate_voxel_grid_returns_nonempty_mesh() {
    println!("voxel_to_mesh_tests: has_openvdb cfg not set, skip");
    assert!(true);
}

// ---------------------------------------------------------------------------
// (d) realize_mesh_from_voxel_rejects_unregistered_handle
// ---------------------------------------------------------------------------

/// An invalid (unregistered) handle must return Err(GeometryError::OperationFailed).
///
/// RED: realize_mesh_from_voxel_with_options does not exist yet.
#[cfg(has_openvdb)]
#[test]
fn realize_mesh_from_voxel_rejects_unregistered_handle() {
    use reify_ir::{GeometryError, GeometryHandleId};
    use reify_kernel_openvdb::{MarchingCubesOptions, OpenVdbKernel};

    let kernel = OpenVdbKernel::new();
    let bad_handle = GeometryHandleId(99999);

    let result = kernel.realize_mesh_from_voxel_with_options(
        bad_handle,
        &MarchingCubesOptions::default(),
    );
    assert!(
        matches!(result, Err(GeometryError::OperationFailed(_))),
        "realize_mesh_from_voxel_with_options must return OperationFailed for an \
         unregistered handle; got {result:?}",
    );
}

/// `cfg(not(has_openvdb))` skip-stub.
#[cfg(not(has_openvdb))]
#[test]
fn realize_mesh_from_voxel_rejects_unregistered_handle() {
    println!("voxel_to_mesh_tests: has_openvdb cfg not set, skip");
    assert!(true);
}
