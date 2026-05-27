//! Tests for the OpenVDB mesh-to-volume realization and SDF sampling.
//!
//! All `cfg(has_openvdb)` tests exercise the real FFI; companion
//! `cfg(not(has_openvdb))` skip-stubs keep the test count stable.

// ---------------------------------------------------------------------------
// Shared mesh fixtures
// ---------------------------------------------------------------------------

/// Build a 10mm × 10mm × 1mm thin-slab mesh: 8 corner vertices, 12 triangles
/// closing the box. Faces are wound consistently (outward normals).
///
/// Used by `realize_voxel_from_thin_slab_mesh_returns_handle_with_expected_active_count`.
#[cfg(has_openvdb)]
fn thin_slab_mesh() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    // 8 corners of a 10×10×1 box (mm units).
    let verts: Vec<[f32; 3]> = vec![
        [0.0, 0.0, 0.0],   // 0 bottom face
        [10.0, 0.0, 0.0],  // 1
        [10.0, 10.0, 0.0], // 2
        [0.0, 10.0, 0.0],  // 3
        [0.0, 0.0, 1.0],   // 4 top face
        [10.0, 0.0, 1.0],  // 5
        [10.0, 10.0, 1.0], // 6
        [0.0, 10.0, 1.0],  // 7
    ];
    // 12 triangles (2 per face × 6 faces). Outward-normal convention.
    let tris: Vec<[u32; 3]> = vec![
        // Bottom face (z=0, normal -Z)
        [0, 2, 1],
        [0, 3, 2],
        // Top face (z=1, normal +Z)
        [4, 5, 6],
        [4, 6, 7],
        // Front face (y=0, normal -Y)
        [0, 1, 5],
        [0, 5, 4],
        // Back face (y=10, normal +Y)
        [2, 3, 7],
        [2, 7, 6],
        // Left face (x=0, normal -X)
        [0, 4, 7],
        [0, 7, 3],
        // Right face (x=10, normal +X)
        [1, 2, 6],
        [1, 6, 5],
    ];
    (verts, tris)
}

/// Build a unit-sphere mesh as a regular octahedron (8 triangles, 6 vertices).
///
/// The octahedron is a crude approximation but sufficient for testing that
/// the interior is negative and the exterior is positive after SDF realization.
/// Vertices are at (±1, 0, 0), (0, ±1, 0), (0, 0, ±1).
#[cfg(has_openvdb)]
fn unit_sphere_octahedron() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let verts: Vec<[f32; 3]> = vec![
        [1.0, 0.0, 0.0],  // 0 +X
        [-1.0, 0.0, 0.0], // 1 -X
        [0.0, 1.0, 0.0],  // 2 +Y
        [0.0, -1.0, 0.0], // 3 -Y
        [0.0, 0.0, 1.0],  // 4 +Z
        [0.0, 0.0, -1.0], // 5 -Z
    ];
    let tris: Vec<[u32; 3]> = vec![
        // Top hemisphere (+Z cap)
        [0, 2, 4],
        [2, 1, 4],
        [1, 3, 4],
        [3, 0, 4],
        // Bottom hemisphere (-Z cap)
        [2, 0, 5],
        [1, 2, 5],
        [3, 1, 5],
        [0, 3, 5],
    ];
    (verts, tris)
}

// ---------------------------------------------------------------------------
// realize_voxel_from_mesh tests
// ---------------------------------------------------------------------------

/// Realize a thin slab mesh → FloatGrid SDF and assert:
/// 1. The returned handle is valid (not INVALID).
/// 2. The active voxel count is non-zero.
/// 3. The count is within a plausible narrow-band range:
///    (surface_area_voxels × 2 × half_width × 0.5) to
///    (surface_area_voxels × 2 × half_width × 1.5).
///
/// RED: fails to compile because `realize_voxel_from_mesh` and
/// `active_voxel_count` don't exist on `OpenVdbKernel` yet.
#[cfg(has_openvdb)]
#[test]
fn realize_voxel_from_thin_slab_mesh_returns_handle_with_expected_active_count() {
    use reify_kernel_openvdb::OpenVdbKernel;

    let (verts, tris) = thin_slab_mesh();
    let voxel_size = 0.1_f64; // 0.1 mm
    let half_width = 3.0_f64; // 3 voxels narrow band

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .realize_voxel_from_mesh(&verts, &tris, voxel_size, half_width)
        .expect("realize_voxel_from_mesh should succeed for a valid slab mesh");

    // Handle must be valid.
    use reify_ir::GeometryHandleId;
    assert!(
        handle != GeometryHandleId::INVALID,
        "expected a valid GeometryHandleId, got INVALID"
    );

    // Active voxel count must be non-zero.
    let count = kernel
        .active_voxel_count(handle)
        .expect("active_voxel_count should succeed for a registered handle");

    assert!(count > 0, "active voxel count must be non-zero");

    // Empirical reference (captured 2026-05-07 with libopenvdb 13.0.0, voxel_size=0.1mm,
    // half_width=3.0): 119_366 active voxels.
    //
    // Band widened to ±20% so the test tolerates `meshToLevelSet` algorithm
    // tweaks across libopenvdb minor/patch releases (the build.rs probe
    // accepts any libopenvdb.so under /opt/reify-deps or /usr/lib, so a
    // future upgrade is in-scope). ±20% still rules out catastrophic
    // regressions (empty grid, off-by-axis bbox, mis-scaled transform) which
    // shift the count by orders of magnitude — the failure modes this test
    // is meant to catch.
    const EMPIRICAL_COUNT: usize = 119_366;
    let lower = (EMPIRICAL_COUNT as f64 * 0.80) as usize;
    let upper = (EMPIRICAL_COUNT as f64 * 1.20) as usize;

    assert!(
        count >= lower && count <= upper,
        "active voxel count {count} outside ±20% of empirical reference \
         [{lower}, {upper}] (empirical={EMPIRICAL_COUNT})"
    );
}

/// `cfg(not(has_openvdb))` skip-stub.
#[cfg(not(has_openvdb))]
#[test]
fn realize_voxel_skipped_without_cfg() {
    println!("realize_voxel_tests: has_openvdb cfg not set, skip");
    assert!(true);
}

// ---------------------------------------------------------------------------
// sample_sdf_at tests (step-5 RED arm)
// ---------------------------------------------------------------------------

/// Realize an octahedron-approximated unit sphere, sample the SDF at three
/// characteristic points and assert sign conventions:
/// - Interior (0,0,0): negative (inside the sphere).
/// - Far exterior (2,0,0): positive (outside).
/// - On-surface (1,0,0): near zero (within ±voxel_size).
///
/// RED: fails to compile because `sample_sdf_at` doesn't exist yet.
#[cfg(has_openvdb)]
#[test]
fn sample_sdf_at_returns_signed_distance_for_realized_grid() {
    use reify_kernel_openvdb::OpenVdbKernel;

    let (verts, tris) = unit_sphere_octahedron();
    let voxel_size = 0.05_f64;
    let half_width = 4.0_f64;

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .realize_voxel_from_mesh(&verts, &tris, voxel_size, half_width)
        .expect("realize_voxel_from_mesh should succeed for octahedron");

    // (a) Interior: center should be negative (inside the volume).
    let center_sdf = kernel
        .sample_sdf_at(handle, 0.0, 0.0, 0.0)
        .expect("sample_sdf_at should succeed for a registered handle");
    assert!(
        center_sdf < 0.0,
        "SDF at center (0,0,0) should be negative (interior); got {center_sdf}"
    );
    // Loose bounds: saturated interior is at most -half_width × voxel_size.
    let band_limit = half_width * voxel_size;
    assert!(
        center_sdf >= -band_limit - voxel_size,
        "SDF at center {center_sdf} is more negative than band limit {band_limit}"
    );

    // (b) Far exterior: (2,0,0) is outside the unit sphere, should be positive.
    let far_sdf = kernel
        .sample_sdf_at(handle, 2.0, 0.0, 0.0)
        .expect("sample_sdf_at should succeed for exterior point");
    assert!(
        far_sdf > 0.0,
        "SDF at far exterior (2,0,0) should be positive; got {far_sdf}"
    );

    // (c) On-surface: (1,0,0) is a vertex of the octahedron, near the surface.
    let surface_sdf = kernel
        .sample_sdf_at(handle, 1.0, 0.0, 0.0)
        .expect("sample_sdf_at should succeed for surface point");
    assert!(
        surface_sdf.abs() <= voxel_size * 2.0,
        "SDF at on-surface (1,0,0) should be near zero (within 2×voxel_size); \
         got {surface_sdf}, voxel_size={voxel_size}"
    );
}

/// `cfg(not(has_openvdb))` skip-stub for the SDF test.
#[cfg(not(has_openvdb))]
#[test]
fn sample_sdf_at_skipped_without_cfg() {
    println!("sample_sdf_at_returns_signed_distance: has_openvdb cfg not set, skip");
    assert!(true);
}

// ---------------------------------------------------------------------------
// realize_voxel_from_mesh_with_options tests (task η wrapper)
// ---------------------------------------------------------------------------

/// Build a flat Mesh from the thin-slab triplets for use in the options-wrapper test.
#[cfg(has_openvdb)]
fn thin_slab_flat_mesh() -> reify_ir::Mesh {
    let (verts, tris) = thin_slab_mesh();
    reify_ir::Mesh {
        vertices: verts.iter().flat_map(|v| v.iter().copied()).collect(),
        indices: tris.iter().flat_map(|t| t.iter().copied()).collect(),
        normals: None,
    }
}

/// Verify that `realize_voxel_from_mesh_with_options` produces active voxels
/// and returns a count consistent with a direct `realize_voxel_from_mesh` call
/// on the same geometry (same FFI path, deterministic).
#[cfg(has_openvdb)]
#[test]
fn realize_voxel_from_mesh_with_options_produces_active_voxels() {
    use reify_kernel_openvdb::{MeshToVoxelOptions, OpenVdbKernel};

    let mesh = thin_slab_flat_mesh();
    let opts = MeshToVoxelOptions {
        voxel_size: 0.1,
        narrow_band: 3.0,
    };

    let mut kernel = OpenVdbKernel::new();

    // Call the wrapper.
    let handle = kernel
        .realize_voxel_from_mesh_with_options(&mesh, &opts)
        .expect("realize_voxel_from_mesh_with_options should succeed for a valid slab mesh");

    use reify_ir::GeometryHandleId;
    assert!(
        handle != GeometryHandleId::INVALID,
        "expected a valid GeometryHandleId, got INVALID"
    );

    let wrapper_count = kernel
        .active_voxel_count(handle)
        .expect("active_voxel_count should succeed for a registered handle");
    assert!(wrapper_count > 0, "active voxel count must be non-zero");

    // Consistency: direct call must produce the same count (same FFI, same params).
    let (verts, tris) = thin_slab_mesh();
    let direct_handle = kernel
        .realize_voxel_from_mesh(&verts, &tris, opts.voxel_size, opts.narrow_band)
        .expect("direct realize_voxel_from_mesh should succeed");
    let direct_count = kernel
        .active_voxel_count(direct_handle)
        .expect("active_voxel_count should succeed for direct handle");

    assert_eq!(
        wrapper_count, direct_count,
        "realize_voxel_from_mesh_with_options must produce the same active voxel count \
         as a direct realize_voxel_from_mesh call with the same parameters; \
         wrapper={wrapper_count}, direct={direct_count}",
    );
}

/// `cfg(not(has_openvdb))` skip-stub for the options-wrapper test.
#[cfg(not(has_openvdb))]
#[test]
fn realize_voxel_from_mesh_with_options_skipped_without_cfg() {
    println!("realize_voxel_from_mesh_with_options: has_openvdb cfg not set, skip");
    assert!(true);
}
