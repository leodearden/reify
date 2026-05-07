//! Tests for OpenVDB grid file I/O round-trip.
//!
//! All `cfg(has_openvdb)` tests exercise the real FFI; companion
//! `cfg(not(has_openvdb))` skip-stubs keep the test count stable.

/// Write an octahedron-approximated unit sphere as a VDB level set, read it
/// back via `read_vdb_file`, and assert:
///
/// 1. The returned field `kind` is `SampledGridKind::Regular3D`.
/// 2. The field `data` is non-empty (densified bbox slice).
/// 3. The 3D `bounds_min` / `bounds_max` are non-degenerate.
/// 4. Re-opening the same file via `open_vdb_grid_for_test` gives an
///    active-voxel-count that matches the original exactly.
///
/// **RED**: fails at step 5 because `read_vdb_file` currently returns
/// `IngestError::FfiNotImplemented` instead of `Ok(IngestOutcome)`.
/// Step-8 GREEN replaces the stub body with the real FFI read path.
#[cfg(has_openvdb)]
#[test]
fn vdb_grid_round_trip_preserves_metadata_and_active_count() {
    use reify_kernel_openvdb::{
        OpenVdbKernel,
        ingest::read_vdb_file,
    };
    use reify_types::{SampledGridKind, Type};

    // ---------------------------------------------------------------------------
    // Octahedron unit-sphere mesh fixture (6 verts, 8 tris)
    // ---------------------------------------------------------------------------
    let verts: Vec<[f32; 3]> = vec![
        [1.0, 0.0, 0.0],   // 0 +X
        [-1.0, 0.0, 0.0],  // 1 -X
        [0.0, 1.0, 0.0],   // 2 +Y
        [0.0, -1.0, 0.0],  // 3 -Y
        [0.0, 0.0, 1.0],   // 4 +Z
        [0.0, 0.0, -1.0],  // 5 -Z
    ];
    let tris: Vec<[u32; 3]> = vec![
        // Top hemisphere
        [0, 2, 4], [2, 1, 4], [1, 3, 4], [3, 0, 4],
        // Bottom hemisphere
        [2, 0, 5], [1, 2, 5], [3, 1, 5], [0, 3, 5],
    ];

    let voxel_size = 0.05_f64;
    let half_width = 4.0_f64;

    let mut kernel = OpenVdbKernel::new();

    // ---------------------------------------------------------------------------
    // Step 1: Realize the sphere as a narrow-band SDF level set.
    // ---------------------------------------------------------------------------
    let original_handle = kernel
        .realize_voxel_from_mesh(&verts, &tris, voxel_size, half_width)
        .expect("realize_voxel_from_mesh should succeed for octahedron");

    // ---------------------------------------------------------------------------
    // Step 2: Capture original active voxel count.
    // ---------------------------------------------------------------------------
    let original_count = kernel
        .active_voxel_count(original_handle)
        .expect("active_voxel_count should succeed for the realized grid");
    assert!(original_count > 0, "expected non-zero active voxels after realization");

    // ---------------------------------------------------------------------------
    // Step 3: Write to a temporary file.
    // ---------------------------------------------------------------------------
    let tmp = tempfile::NamedTempFile::new()
        .expect("tempfile creation should succeed");
    let vdb_path = tmp.path();

    // ---------------------------------------------------------------------------
    // Step 4: Write the grid via write_vdb_grid.
    // ---------------------------------------------------------------------------
    kernel
        .write_vdb_grid(original_handle, vdb_path, "level_set")
        .expect("write_vdb_grid should succeed for a realized grid");

    // ---------------------------------------------------------------------------
    // Step 5: Read back via read_vdb_file.
    //
    // RED: this call currently returns Err(IngestError::FfiNotImplemented)
    // and the test panics here. Step-8 GREEN replaces the stub body.
    // ---------------------------------------------------------------------------
    let path_str = vdb_path.to_str().expect("temp path must be valid UTF-8");
    let outcome = read_vdb_file(path_str, "level_set", &Type::Real)
        .expect("read_vdb_file should succeed for a file written by write_vdb_grid");

    // ---------------------------------------------------------------------------
    // Step 6: Assert field kind is Regular3D.
    // ---------------------------------------------------------------------------
    assert_eq!(
        outcome.field.kind,
        SampledGridKind::Regular3D,
        "VDB level set must be read back as Regular3D"
    );

    // ---------------------------------------------------------------------------
    // Step 7: Assert densified data is non-empty.
    // ---------------------------------------------------------------------------
    assert!(
        !outcome.field.data.is_empty(),
        "densified bbox buffer must be non-empty"
    );

    // ---------------------------------------------------------------------------
    // Step 8: Assert bounds are 3D and non-degenerate.
    // ---------------------------------------------------------------------------
    assert_eq!(outcome.field.bounds_min.len(), 3, "3D grid must have 3 bounds_min elements");
    assert_eq!(outcome.field.bounds_max.len(), 3, "3D grid must have 3 bounds_max elements");
    for i in 0..3 {
        assert!(
            outcome.field.bounds_max[i] > outcome.field.bounds_min[i],
            "bounds_max[{i}]={} must exceed bounds_min[{i}]={}",
            outcome.field.bounds_max[i], outcome.field.bounds_min[i]
        );
    }

    // ---------------------------------------------------------------------------
    // Step 9: Re-open the file via open_vdb_grid_for_test; assert active count
    //         round-trips exactly.
    // ---------------------------------------------------------------------------
    let reloaded_handle = kernel
        .open_vdb_grid_for_test(vdb_path, "level_set")
        .expect("open_vdb_grid_for_test should succeed for the written file");
    let reloaded_count = kernel
        .active_voxel_count(reloaded_handle)
        .expect("active_voxel_count should succeed for the reloaded grid");
    assert_eq!(
        reloaded_count, original_count,
        "active voxel count must round-trip exactly: \
         original={original_count}, reloaded={reloaded_count}"
    );
}

/// `cfg(not(has_openvdb))` skip-stub to keep test-count parity across build modes.
#[cfg(not(has_openvdb))]
#[test]
fn vdb_grid_round_trip_skipped_without_cfg() {
    println!("grid_io_tests: has_openvdb cfg not set — skipping round-trip test");
    assert!(true);
}

// ---------------------------------------------------------------------------
// Layout-detection test: ensures the densified buffer is X-outermost.
//
// The existing `vdb_grid_round_trip_preserves_metadata_and_active_count` test
// uses an octahedron-symmetric sphere where nx ≈ ny ≈ nz, so any X↔Z
// transposition in `grid_densify_to_buffer` yields the same data back through
// the symmetric axes — the bug would be invisible.
//
// This test uses an asymmetric slab (X=10mm, Y=10mm, Z=1mm at voxel_size=0.1
// with half_width=10 voxels = 1.0mm), making nx ≈ ny ≈ 120 but nz ≈ 30 so the
// transposition produces measurably different sampled values. Specifically:
//
//   - Sampling at slab-interior (5, 5, 0.5) with the correct X-outermost
//     layout returns the negative interior SDF (inside the slab).
//   - With a Z-outermost buffer interpreted as X-outermost (the bug),
//     `interpolate_3d` reads at the buffer position implied by
//     (X=0.5, Y=5, Z=5), which is outside the slab's actual domain in Z and
//     reads a saturated POSITIVE value.
// ---------------------------------------------------------------------------

/// 10mm × 10mm × 1mm thin slab (8 verts, 12 tris). Outward normals.
///
/// Identical fixture to `realize_voxel_tests::thin_slab_mesh` — duplicated
/// here to keep test files self-contained without cross-test imports.
#[cfg(has_openvdb)]
fn slab_mesh() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let verts: Vec<[f32; 3]> = vec![
        [0.0, 0.0, 0.0],   // 0
        [10.0, 0.0, 0.0],  // 1
        [10.0, 10.0, 0.0], // 2
        [0.0, 10.0, 0.0],  // 3
        [0.0, 0.0, 1.0],   // 4
        [10.0, 0.0, 1.0],  // 5
        [10.0, 10.0, 1.0], // 6
        [0.0, 10.0, 1.0],  // 7
    ];
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

/// Round-trip a 10×10×1mm slab through write_vdb_grid → read_vdb_file and
/// assert the densified buffer is laid out X-outermost (axis-0 = X), matching
/// the workspace-wide row-major-axis-0-outermost convention used by
/// `interp::interpolate_3d` and `engine_eval::build_sampled_field`.
///
/// **RED**: with the current `grid_densify_to_buffer` Z-outermost loop, the
/// interior probe at (5, 5, 0.5) returns a saturated POSITIVE value because
/// the buffer is read as X-outermost while it was filled Z-outermost — i.e.
/// the X and Z indices are transposed. After step-12 swaps the C++ loop
/// order, this test transitions GREEN.
#[cfg(has_openvdb)]
#[test]
fn vdb_round_trip_data_layout_is_axis0_x_outermost() {
    use reify_expr::interp::{InterpolationMethod, interpolate_3d};
    use reify_kernel_openvdb::{OpenVdbKernel, ingest::read_vdb_file};
    use reify_types::Type;

    let (verts, tris) = slab_mesh();

    // voxel_size = 0.1mm, half_width = 10 voxels = 1.0mm so the narrow band
    // fully covers the 1mm-thick slab interior in Z (band = ±1mm around the
    // surface, total Z extent ≈ −1 .. 2mm).
    let voxel_size = 0.1_f64;
    let half_width = 10.0_f64;

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .realize_voxel_from_mesh(&verts, &tris, voxel_size, half_width)
        .expect("realize_voxel_from_mesh should succeed for the slab");

    // Round-trip through a tempfile.
    let tmp = tempfile::NamedTempFile::new().expect("tempfile creation should succeed");
    let vdb_path = tmp.path();
    kernel
        .write_vdb_grid(handle, vdb_path, "slab")
        .expect("write_vdb_grid should succeed for the realized slab");

    let path_str = vdb_path.to_str().expect("temp path must be valid UTF-8");
    let outcome = read_vdb_file(path_str, "slab", &Type::Real)
        .expect("read_vdb_file should succeed for the freshly-written slab");

    let field = &outcome.field;

    // Sanity: nx (axis-0) > nz (axis-2) for this asymmetric slab geometry.
    // If layout were Z-outermost while the SampledField is interpreted
    // X-outermost, the assertion would still hold (axis labels just shift),
    // so this is a precondition rather than the actual layout test.
    assert!(
        field.axis_grids[0].len() > field.axis_grids[2].len(),
        "asymmetric slab fixture must have axis_grids[0]={} > axis_grids[2]={}",
        field.axis_grids[0].len(),
        field.axis_grids[2].len()
    );

    // ---------------------------------------------------------------------
    // Interior probe: (5, 5, 0.5) — center of the slab, well inside in X/Y/Z.
    //
    // Correct X-outermost layout: the buffer is interpreted with axis-0 = X
    // and the SDF at this point is negative (interior).
    //
    // Bug layout (Z-outermost buffer read X-outermost): `interpolate_3d`
    // looks up `data[ix * ny * nz + iy * nz + iz]`, but the buffer was
    // populated with `data[iz * ny * nx + iy * nx + ix]`. So a query at
    // physical (5, 5, 0.5) reads the value that the C++ side stored at
    // index (X=0.5 voxel, Y=5 voxel, Z=5 voxel) — which is OUTSIDE the slab
    // in Z (slab Z extent is ~[−1, 2] but Z=5 is far past +Z) → saturated
    // POSITIVE band-limit value.
    // ---------------------------------------------------------------------
    let interior = interpolate_3d(
        InterpolationMethod::Linear,
        &field.axis_grids[0],
        &field.axis_grids[1],
        &field.axis_grids[2],
        &field.data,
        (5.0, 5.0, 0.5),
    );
    assert!(
        interior.value < 0.0,
        "interior SDF at (5, 5, 0.5) must be NEGATIVE (inside the slab); \
         got {value} — buffer layout is likely Z-outermost (bug) instead of \
         X-outermost (axis-0) row-major",
        value = interior.value
    );

    // ---------------------------------------------------------------------
    // Exterior probe: (5, 5, 1.5) — just past the +Z face (Z=1mm) but still
    // inside the narrow band (band = ±1mm around the surface, so Z ≤ 2mm
    // remains banded). Must be POSITIVE (outside the slab).
    // ---------------------------------------------------------------------
    let exterior = interpolate_3d(
        InterpolationMethod::Linear,
        &field.axis_grids[0],
        &field.axis_grids[1],
        &field.axis_grids[2],
        &field.data,
        (5.0, 5.0, 1.5),
    );
    assert!(
        exterior.value > 0.0,
        "exterior SDF at (5, 5, 1.5) must be POSITIVE (outside the slab); \
         got {value}",
        value = exterior.value
    );
}

/// `cfg(not(has_openvdb))` skip-stub for the layout test.
#[cfg(not(has_openvdb))]
#[test]
fn vdb_round_trip_data_layout_skipped_without_cfg() {
    println!("grid_io_tests: has_openvdb cfg not set — skipping data-layout test");
    assert!(true);
}
