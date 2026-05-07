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
