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
/// Regression guard: ensures the round-trip pipeline (write_vdb_grid →
/// read_vdb_file → lower_to_sampled) preserves grid kind, densified-buffer
/// presence, non-degenerate bounds, per-axis spacing, structural span/spacing
/// alignment, and active-voxel count under the canonical isotropic FloatGrid
/// contract.
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
    // Step 8b: Assert per-axis spacing round-trips to the input voxel_size.
    //
    // The original grid was built isotropically via meshToLevelSet at
    // voxel_size=0.05, so all three components of `spacing` must equal 0.05
    // within FP tolerance. A bug that scaled the transform (e.g. by 10×) or
    // dropped a per-axis component would silently pass the kind/active-count
    // checks but fail this assertion. Pinning spacing here is what makes the
    // test name's "preserves metadata" claim accurate.
    // ---------------------------------------------------------------------------
    assert_eq!(outcome.field.spacing.len(), 3, "3D grid must have 3 spacing elements");
    for i in 0..3 {
        let delta = (outcome.field.spacing[i] - voxel_size).abs();
        assert!(
            delta < 1e-9,
            "spacing[{i}]={} must round-trip to voxel_size={voxel_size} within 1e-9 (Δ={delta})",
            outcome.field.spacing[i]
        );
    }

    // ---------------------------------------------------------------------------
    // Step 8c: Assert bounds span matches (n−1)·spacing within voxel_size
    // tolerance — a structural sanity check that the bbox endpoints align
    // with voxel-center spacing rather than e.g. half-shifted or scaled by
    // some accidental factor. Mirrors the linspace_inclusive contract used
    // by lower_to_sampled.
    // ---------------------------------------------------------------------------
    for i in 0..3 {
        let span = outcome.field.bounds_max[i] - outcome.field.bounds_min[i];
        let n = outcome.field.axis_grids[i].len();
        assert!(n >= 2, "axis_grids[{i}] must have ≥ 2 nodes after lowering, got {n}");
        let expected_span = (n - 1) as f64 * outcome.field.spacing[i];
        let delta = (span - expected_span).abs();
        assert!(
            delta < voxel_size,
            "axis {i} span={span} must equal (n-1)·spacing={expected_span} within voxel_size={voxel_size} (Δ={delta})"
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
/// Regression guard: if `grid_densify_to_buffer` ever reverts to Z-outermost
/// (or any non-X-outermost ordering), the interior probe at (5, 5, 0.5) is
/// read at the buffer position implied by (X=0.5, Y=5, Z=5), which is well
/// outside the slab's actual Z extent (~[−1, 2]) and produces a saturated
/// POSITIVE band-limit value — flipping the sign of `interior.value` and
/// failing the negative-SDF assertion below.
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

// ---------------------------------------------------------------------------
// `&self` no-mutation invariant for `write_vdb_grid`.
//
// `OpenVdbKernel` declares `unsafe impl Sync` (see kernel_real.rs:220-260).
// Sync soundness requires that every `&self` method is genuinely read-only
// against the shared FloatGrid tree — otherwise concurrent threads holding
// `&OpenVdbKernel` could observe a mid-mutation grid.
//
// `write_vdb_grid`'s signature is `&self`, so it MUST NOT mutate the
// registered handle's grid. Earlier revisions called
// `h.grid->setName(grid_name)` directly on the registered FloatGrid, which
// silently flipped the in-memory grid name on every export — a behavioral
// surprise even for sequential callers, and a Sync soundness violation
// the moment any reader started observing names.
//
// This test pins the contract: writing through `write_vdb_grid` must NOT
// change the registered handle's grid name. The fix (step-14) is to
// deep-copy the FloatGrid on the C++ side before mutating its metadata,
// so the on-disk file gets the requested name while the in-memory grid
// keeps whatever name it had at registration.
// ---------------------------------------------------------------------------

/// Pin the `&self` no-mutation invariant for `write_vdb_grid`.
///
/// Realize a small sphere via `realize_voxel_from_mesh` (which produces an
/// unnamed FloatGrid → default name `""`), capture the grid name, write the
/// grid to a tempfile under a DIFFERENT name (`"renamed_for_export"`),
/// then re-read the registered handle's grid name and assert it is
/// unchanged.
///
/// Regression guard: any future revision that re-introduces an in-place
/// `setName` (or any other mutation) on the registered grid inside
/// `write_vdb_grid_ffi` will flip the assertion.
#[cfg(has_openvdb)]
#[test]
fn write_vdb_grid_does_not_mutate_registered_handle_grid_name() {
    use reify_kernel_openvdb::OpenVdbKernel;

    // Reuse the octahedron unit-sphere mesh fixture from
    // vdb_grid_round_trip_preserves_metadata_and_active_count (6 verts,
    // 8 tris). Kept inline so this test is self-contained and the fixture
    // does not get coupled across test functions.
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

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .realize_voxel_from_mesh(&verts, &tris, 0.05, 4.0)
        .expect("realize_voxel_from_mesh should succeed for the octahedron");

    // meshToVolume / meshToLevelSet produces an unnamed FloatGrid →
    // default name is "" (per the OpenVDB Grid::getName() contract for a
    // grid that has not had setName called).
    let original_name = kernel
        .grid_name_for_test(handle)
        .expect("grid_name_for_test should succeed for the registered handle");

    let tmp = tempfile::NamedTempFile::new()
        .expect("tempfile creation should succeed");
    kernel
        .write_vdb_grid(handle, tmp.path(), "renamed_for_export")
        .expect("write_vdb_grid should succeed for the realized grid");

    let post_write_name = kernel
        .grid_name_for_test(handle)
        .expect("grid_name_for_test should succeed after write");

    assert_eq!(
        post_write_name, original_name,
        "write_vdb_grid must not mutate the registered handle's grid name \
         (Sync soundness contract — see kernel_real.rs:220-260); \
         original={original_name:?}, post_write={post_write_name:?}"
    );
}

/// `cfg(not(has_openvdb))` skip-stub for the no-mutation test.
#[cfg(not(has_openvdb))]
#[test]
fn write_vdb_grid_does_not_mutate_skipped_without_cfg() {
    println!("grid_io_tests: has_openvdb cfg not set — skipping no-mutation test");
    assert!(true);
}
