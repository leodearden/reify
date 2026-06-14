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

/// `ingest_mesh` with a vertices buffer whose length is not a multiple of 3
/// must return `Err(GeometryError::OperationFailed(_))` with a diagnostic that
/// names the buffer-layout cause — NOT the misleading "bbox extent" message
/// that `honest_floor` would emit if called directly on the malformed mesh.
#[cfg(has_openvdb)]
#[test]
fn ingest_mesh_non_multiple_of_3_vertices_returns_layout_error() {
    use reify_ir::{GeometryError, GeometryKernel, Mesh};
    use reify_kernel_openvdb::OpenVdbKernel;

    // 10 floats — not divisible by 3 (malformed flat xyz buffer).
    let mesh = Mesh { vertices: vec![0.0_f32; 10], indices: vec![], normals: None };
    let mut kernel = OpenVdbKernel::new();
    let result = kernel.ingest_mesh(&mesh);

    let msg = match result {
        Err(GeometryError::OperationFailed(m)) => m,
        other => panic!(
            "ingest_mesh must return Err(OperationFailed) for non-multiple-of-3 \
             vertices; got {other:?}"
        ),
    };
    assert!(
        msg.contains("not a multiple of 3"),
        "error message must mention the malformed layout (\"not a multiple of 3\"); \
         got: {msg}"
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
// densify_grid_to_sampled tests  (step-5 RED)
// ---------------------------------------------------------------------------

/// Ingest a 2mm box → densify → check the resulting SampledField.
///
/// Asserts:
/// - `field.kind == SampledGridKind::Regular3D`
/// - `field.spacing.len() == 3` and all spacings are positive
/// - `!field.data.is_empty()`
/// - Headline α signal: sample φ at the box centre (0,0,0) via trilinear
///   interpolation is negative (interior) and within `h` of the true distance
///   `-1.0` (half-extent of the 2mm box).
/// - An invalid handle returns `Err(QueryError::InvalidHandle(_))`.
///
/// RED: `densify_grid_to_sampled` does not exist yet (compile error).
#[cfg(has_openvdb)]
#[test]
fn densify_grid_to_sampled_from_ingested_box_mesh() {
    use reify_expr::interp::{InterpolationMethod, interpolate_3d};
    use reify_ir::{GeometryHandleId, GeometryKernel, QueryError, SampledGridKind};
    use reify_kernel_openvdb::OpenVdbKernel;

    let mesh = box_2mm();
    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .ingest_mesh(&mesh)
        .expect("ingest_mesh must succeed for the 2mm box");

    let field = kernel
        .densify_grid_to_sampled(handle.id)
        .expect("densify_grid_to_sampled must succeed for a freshly-ingested handle");

    // Shape checks.
    assert_eq!(
        field.kind,
        SampledGridKind::Regular3D,
        "densified field must be Regular3D"
    );
    assert_eq!(
        field.spacing.len(),
        3,
        "spacing must have 3 entries for a Regular3D field"
    );
    for (i, &s) in field.spacing.iter().enumerate() {
        assert!(
            s > 0.0 && s.is_finite(),
            "spacing[{i}] = {s} must be positive and finite"
        );
    }
    assert!(!field.data.is_empty(), "densified field data must not be empty");

    // Headline α signal: interior SDF at (0, 0, 0) ≈ -1.0 (half-extent).
    // The honest-floor band covers the interior so this is the TRUE signed
    // distance, not the saturated background sentinel.
    let h = field.spacing[0];
    let phi = interpolate_3d(
        InterpolationMethod::Linear,
        &field.axis_grids[0], // X axis (axis-0, outermost)
        &field.axis_grids[1], // Y axis
        &field.axis_grids[2], // Z axis
        &field.data,
        (0.0, 0.0, 0.0),
    )
    .value;

    assert!(
        phi < 0.0,
        "SDF at centre (0,0,0) must be negative (interior); got phi={phi}"
    );
    // Accept ±h tolerance around -1.0 (the true half-extent = 1.0 mm for 2mm box).
    // A tighter bound (±h/2) risks flipping on voxel-centre rounding; ±h is
    // the documented spec (PRD §α "within h").
    assert!(
        (phi - (-1.0_f64)).abs() <= h,
        "SDF at centre must be within h={h} of -1.0 (true half-extent); \
         got phi={phi}, |phi - (-1.0)| = {}",
        (phi - (-1.0_f64)).abs()
    );

    // Invalid handle → QueryError::InvalidHandle.
    let bad_result = kernel.densify_grid_to_sampled(GeometryHandleId(999_999));
    assert!(
        matches!(bad_result, Err(QueryError::InvalidHandle(_))),
        "densify_grid_to_sampled with an unknown handle must return \
         Err(InvalidHandle); got {bad_result:?}"
    );
}

/// `cfg(not(has_openvdb))` skip-stub for the densify test.
#[cfg(not(has_openvdb))]
#[test]
fn densify_grid_to_sampled_skipped_without_cfg() {
    println!(
        "ingest_mesh_densify_tests: has_openvdb cfg not set, skipping densify test"
    );
    assert!(true);
}

// ---------------------------------------------------------------------------
// densify_grid_to_sampled via trait object  (step-3 RED → step-4 GREEN)
// ---------------------------------------------------------------------------

/// Calls `densify_grid_to_sampled` through a `&mut dyn GeometryKernel` trait
/// object rather than a concrete `OpenVdbKernel` receiver.
///
/// This pins the trait-object dispatch path that δ
/// (`project_realization_read_handle`) relies on: the Engine holds its kernels
/// as `Box<dyn GeometryKernel>`, so the call site in `realization_content.rs`
/// never has a concrete receiver.
///
/// Asserts:
/// - `Ok` result when called on a real ingested handle.
/// - `field.kind == SampledGridKind::Regular3D`
/// - `field.spacing.len() == 3`
/// - `Err(QueryError::InvalidHandle(_))` for an unknown handle via the same
///   trait object — ensures the override, not the default, is dispatched.
#[cfg(has_openvdb)]
#[test]
fn densify_grid_to_sampled_via_trait_object() {
    use reify_ir::{GeometryHandleId, GeometryKernel, QueryError, SampledGridKind};
    use reify_kernel_openvdb::OpenVdbKernel;

    let mesh = box_2mm();
    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .ingest_mesh(&mesh)
        .expect("ingest_mesh must succeed for the 2mm box");

    // Call through a trait object — this is the path δ uses.
    let k: &mut dyn GeometryKernel = &mut kernel;
    let field = k
        .densify_grid_to_sampled(handle.id)
        .expect("densify_grid_to_sampled must succeed through a trait object");

    assert_eq!(
        field.kind,
        SampledGridKind::Regular3D,
        "densified field must be Regular3D when dispatched via trait object"
    );
    assert_eq!(
        field.spacing.len(),
        3,
        "spacing must have 3 entries for a Regular3D field via trait object"
    );

    // Invalid handle through the same trait object path.
    let bad = k.densify_grid_to_sampled(GeometryHandleId(999_999));
    assert!(
        matches!(bad, Err(QueryError::InvalidHandle(_))),
        "densify_grid_to_sampled with unknown handle via trait object must return \
         Err(InvalidHandle); got {bad:?}"
    );
}

/// `cfg(not(has_openvdb))` skip-stub for the trait-object dispatch test.
#[cfg(not(has_openvdb))]
#[test]
fn densify_grid_to_sampled_via_trait_object_skipped_without_cfg() {
    println!(
        "ingest_mesh_densify_tests: has_openvdb cfg not set, skipping trait-object dispatch test"
    );
    assert!(true);
}
