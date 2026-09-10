//! Kernel-primitive contract for the `VoxelResolution` request seam (task 6560).
//!
//! # What this covers
//!
//! `GeometryKernel::ingest_mesh_at_resolution` is the seam through which a
//! caller that knows its thinnest feature can ask the OpenVDB kernel for a grid
//! that actually resolves it. These tests pin the four things that make the
//! seam trustworthy end-to-end:
//!
//! 1. `HonestFloor` is a pass-through — every pre-6560 caller is unaffected.
//! 2. A `MinFeature` request reaches the FFI and the resulting grid signs the
//!    feature's interior correctly (real-FFI SDF probe).
//! 3. The request genuinely changed the grid (active-voxel count).
//! 4. A malformed, too-coarse or over-budget request is rejected as a kernel
//!    error with a diagnostic that names the offending value — and, because
//!    every one of those guards is pure Rust, without any FFI allocation.
//!
//! # Dual-cfg shape
//!
//! Follows `tests/dispatcher_integration.rs::openvdb_two_stage_chain_voxelize_primitive_executes`
//! (dispatcher_integration.rs:224): the real-FFI assertions are `cfg(has_openvdb)`,
//! and a stub build asserts honest degradation instead. Assertions that are pure
//! Rust arithmetic are asserted unconditionally.

use reify_ir::{GeometryError, GeometryKernel, Mesh, VoxelResolution};
use reify_kernel_openvdb::{
    DENSIFY_BUDGET_VOXELS, MeshToVoxelOptions, OpenVdbKernel, VoxelResolutionError,
};

/// The shells PRD's motivating PROPORTIONS (`structural-analysis-shells.md`,
/// "Background"): a thin feature 1/100 of the part across, which the PRD states
/// as a 1 mm feature in a 100 mm part. Axis-aligned closed box,
/// x,y ∈ [0,100], z ∈ [0,1] — 8 corners, 12 outward-wound triangles, the same
/// shape as the plate fixture in `dispatcher_integration.rs`.
///
/// # Units
///
/// The coordinates are model-space lengths (SI metres, per `Mesh::vertices`),
/// so this body is literally 100 × 100 × 1 METRES — the PRD's millimetre figures
/// written at ×1000 so the probe arithmetic below stays in round numbers. That
/// is sound because the resolution policy is scale-invariant (`h = t/4` and the
/// band identity are ratios), so the results hold verbatim for the real
/// 0.1 × 0.1 × 0.001 m part with `MinFeature(0.001)`. It is spelled out because
/// a caller must NOT copy `MinFeature(1.0)` for a 1 mm feature — see
/// `VoxelResolution`'s "Units" section, and
/// `for_resolution_rejects_a_request_coarser_than_the_thinnest_extent` for what
/// that mistake now costs.
///
/// Deliberately NOT origin-centred: OpenVDB grids index from the world origin,
/// so with h = 0.25 the voxel planes fall at z = 0, 0.25, 0.5, 0.75, 1.0 and the
/// mid-plane probe below lands exactly on a sample point.
#[rustfmt::skip]
fn plate_100x100x1() -> Mesh {
    Mesh {
        vertices: vec![
              0.0_f32,   0.0, 0.0, // 0
            100.0,       0.0, 0.0, // 1
            100.0,     100.0, 0.0, // 2
              0.0,     100.0, 0.0, // 3
              0.0,       0.0, 1.0, // 4
            100.0,       0.0, 1.0, // 5
            100.0,     100.0, 1.0, // 6
              0.0,     100.0, 1.0, // 7
        ],
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

/// `HonestFloor` must behave exactly like plain `ingest_mesh` — same outcome,
/// same build-mode degradation. This is the pin that keeps every pre-6560
/// caller (`measure_thickness_pair`, `measure_min_feature`, the conversion
/// executor's `Voxelize` stage) on the code path it has always taken.
#[test]
fn ingest_mesh_at_resolution_honest_floor_matches_ingest_mesh() {
    let plate = plate_100x100x1();

    let mut k_plain = OpenVdbKernel::new();
    let plain = GeometryKernel::ingest_mesh(&mut k_plain, &plate);

    let mut k_req = OpenVdbKernel::new();
    let requested =
        GeometryKernel::ingest_mesh_at_resolution(&mut k_req, &plate, VoxelResolution::HonestFloor);

    assert_eq!(
        plain.is_ok(),
        requested.is_ok(),
        "HonestFloor must succeed or fail exactly as ingest_mesh does; \
         ingest_mesh={plain:?}, ingest_mesh_at_resolution={requested:?}"
    );

    #[cfg(has_openvdb)]
    assert!(
        requested.is_ok(),
        "HonestFloor on a valid closed plate must succeed under cfg(has_openvdb); got {requested:?}"
    );

    #[cfg(not(has_openvdb))]
    assert!(
        matches!(requested, Err(GeometryError::OperationFailed(_))),
        "HonestFloor must degrade to OperationFailed on the stub kernel; got {requested:?}"
    );
}

/// **The gate assertion.** A `MinFeature(1.0)` request must reach the FFI and
/// produce a grid whose interior at the plate's mid-plane signs NEGATIVE with
/// approximately the true distance.
///
/// # Derivation (no tuned constants)
///
/// `h = 1.0 / MIN_FEATURE_VOXELS_ACROSS = 0.25`. OpenVDB grids index from the
/// world origin, so voxel planes fall at z = 0, 0.25, 0.5, 0.75, 1.0. The exact
/// SDF at the mid-plane (50, 50, 0.5) is −0.5: the nearest boundary is either
/// cap face, half the plate's thickness away. Half-thickness = 0.5 = 2h, clearing the
/// documented interior-signing floor ("half-thickness ≥ 2 × voxel_size",
/// `reify-eval/tests/harness_kernel_realization/realization_read_api.rs:505-510`).
///
/// The tolerance is method error, not a fudge: `sample_sdf_at` trilinearly
/// interpolates a distance function with a kink at the medial surface, and that
/// interpolant errs toward zero by at most one voxel — hence `|v − (−0.5)| ≤ h`.
#[cfg(has_openvdb)]
#[test]
fn min_feature_request_resolves_the_thin_plate_interior() {
    let plate = plate_100x100x1();
    let mut k = OpenVdbKernel::new();

    let handle =
        GeometryKernel::ingest_mesh_at_resolution(&mut k, &plate, VoxelResolution::MinFeature(1.0))
            .expect("MinFeature(1.0) on a valid closed plate must succeed");

    let v = k
        .sample_sdf_at(handle.id, 50.0, 50.0, 0.5)
        .expect("the freshly-ingested handle must be samplable");

    assert!(
        v < 0.0,
        "the plate's mid-plane must sign as INTERIOR; got {v} — a non-negative value \
         means the thin feature was not resolved at all"
    );
    assert!(
        (v + 0.5).abs() <= 0.25,
        "the sampled SDF {v} must be within one voxel (0.25) of the exact value -0.5"
    );
}

/// The resolution request must actually change the grid, not merely be accepted.
///
/// `MinFeature(1.0)` gives h = 0.25 against `HonestFloor`'s 1.5625 — a strictly
/// finer discretisation of the same body, so strictly more active voxels.
/// Strict inequality only; no tuned ratio.
#[cfg(has_openvdb)]
#[test]
fn min_feature_request_produces_a_strictly_finer_grid_than_honest_floor() {
    let plate = plate_100x100x1();

    let mut k = OpenVdbKernel::new();
    let coarse =
        GeometryKernel::ingest_mesh_at_resolution(&mut k, &plate, VoxelResolution::HonestFloor)
            .expect("HonestFloor must succeed");
    let fine =
        GeometryKernel::ingest_mesh_at_resolution(&mut k, &plate, VoxelResolution::MinFeature(1.0))
            .expect("MinFeature(1.0) must succeed");

    let coarse_count = k
        .active_voxel_count(coarse.id)
        .expect("coarse handle must be registered");
    let fine_count = k
        .active_voxel_count(fine.id)
        .expect("fine handle must be registered");

    assert!(
        fine_count > coarse_count,
        "the MinFeature grid must be strictly finer than the HonestFloor grid; \
         fine={fine_count}, coarse={coarse_count}"
    );
}

/// An over-budget request is rejected as a kernel error whose message names the
/// requested voxel size and the budget — and no FFI allocation is paid for it.
///
/// The guard itself is pure Rust (`MeshToVoxelOptions::for_resolution`), so the
/// second half of this test is asserted unconditionally: the rejection is
/// cfg-independent by construction, which is exactly what "before the FFI is
/// consulted" means.
#[test]
fn over_budget_request_is_rejected_with_a_named_diagnostic() {
    let plate = plate_100x100x1();

    // (a) The guard is pure Rust — it fires identically in stub and real builds.
    match MeshToVoxelOptions::for_resolution(&plate, VoxelResolution::TargetVoxelSize(0.001)) {
        Err(VoxelResolutionError::DensifyBudgetExceeded {
            requested_voxel_size,
            implied_voxels,
            budget,
        }) => {
            assert_eq!(requested_voxel_size, 0.001);
            assert_eq!(budget, DENSIFY_BUDGET_VOXELS);
            assert!(implied_voxels > budget);
        }
        other => panic!("expected Err(DensifyBudgetExceeded); got {other:?}"),
    }

    // (b) The kernel surfaces it as OperationFailed under both cfgs.
    let mut k = OpenVdbKernel::new();
    let got = GeometryKernel::ingest_mesh_at_resolution(
        &mut k,
        &plate,
        VoxelResolution::TargetVoxelSize(0.001),
    );
    let Err(GeometryError::OperationFailed(msg)) = got else {
        panic!("expected Err(OperationFailed) for an over-budget request; got {got:?}");
    };

    // On a real build the kernel's message must carry the guard's diagnostic
    // through verbatim. On a stub build there is no resolution-honouring kernel
    // at all, so the default `ingest_mesh` degradation is the honest outcome and
    // (a) above is what pins the guard.
    #[cfg(has_openvdb)]
    {
        assert!(
            msg.contains("0.001"),
            "the diagnostic must name the requested voxel size; got {msg:?}"
        );
        assert!(
            msg.contains(&DENSIFY_BUDGET_VOXELS.to_string()),
            "the diagnostic must name the budget; got {msg:?}"
        );
    }
    #[cfg(not(has_openvdb))]
    let _ = msg;
}

/// Malformed resolution requests are rejected, never silently clamped.
///
/// Part (a) is the load-bearing half and is deliberately pure Rust, mirroring
/// part (a) of `over_budget_request_is_rejected_with_a_named_diagnostic`: on a
/// stub build the kernel overrides neither ingest entry point, so EVERY input
/// already yields `Err(OperationFailed)` and part (b) alone would pass whether
/// `validate_requested_length` exists, is deleted, or silently clamps.
#[test]
fn invalid_resolution_requests_are_rejected() {
    let plate = plate_100x100x1();
    for (resolution, asked, variant_name) in [
        (VoxelResolution::MinFeature(0.0), 0.0_f64, "MinFeature"),
        (
            VoxelResolution::MinFeature(f64::NAN),
            f64::NAN,
            "MinFeature",
        ),
        (
            VoxelResolution::TargetVoxelSize(-1.0),
            -1.0,
            "TargetVoxelSize",
        ),
    ] {
        // (a) The guard is pure Rust — it fires identically in stub and real
        // builds, and names both the offending value and the request site.
        match MeshToVoxelOptions::for_resolution(&plate, resolution) {
            Err(VoxelResolutionError::InvalidRequest { requested, variant }) => {
                assert_eq!(
                    requested.to_bits(),
                    asked.to_bits(),
                    "the error must carry the offending value verbatim, NaN payload included"
                );
                assert_eq!(
                    variant, variant_name,
                    "the error must name the request site, not the arithmetic"
                );
            }
            other => panic!("expected Err(InvalidRequest) for {resolution:?}; got {other:?}"),
        }

        // (b) The kernel surfaces it as OperationFailed under both cfgs.
        let mut k = OpenVdbKernel::new();
        let got = GeometryKernel::ingest_mesh_at_resolution(&mut k, &plate, resolution);
        let Err(GeometryError::OperationFailed(msg)) = got else {
            panic!("expected Err(OperationFailed) for {resolution:?}; got {got:?}");
        };

        // On a real build the kernel carries the guard's diagnostic through
        // verbatim. On a stub build there is no resolution-honouring kernel at
        // all, so the default `ingest_mesh` degradation is the honest outcome
        // and (a) above is what pins the guard.
        #[cfg(has_openvdb)]
        assert!(
            msg.contains(variant_name),
            "the diagnostic must name the offending request variant; got {msg:?}"
        );
        #[cfg(not(has_openvdb))]
        let _ = msg;
    }
}

/// A request coarser than the body's thinnest extent is rejected at the kernel
/// boundary too, with the offending value named.
///
/// The symmetric counterpart of `over_budget_request_is_rejected_with_a_named_diagnostic`:
/// that one pins "too fine to afford", this one "too coarse to mean anything".
/// Same dual-cfg shape — part (a) is the cfg-independent pin.
#[test]
fn too_coarse_request_is_rejected_with_a_named_diagnostic() {
    let plate = plate_100x100x1();

    // (a) Pure Rust: the plate is 1 unit thick, so a 4-unit voxel resolves
    // nothing along z.
    match MeshToVoxelOptions::for_resolution(&plate, VoxelResolution::TargetVoxelSize(4.0)) {
        Err(VoxelResolutionError::RequestTooCoarse {
            requested_voxel_size,
            min_extent,
        }) => {
            assert_eq!(requested_voxel_size, 4.0);
            assert_eq!(min_extent, 1.0);
        }
        other => panic!("expected Err(RequestTooCoarse); got {other:?}"),
    }

    // (b) The kernel surfaces it as OperationFailed under both cfgs.
    let mut k = OpenVdbKernel::new();
    let got = GeometryKernel::ingest_mesh_at_resolution(
        &mut k,
        &plate,
        VoxelResolution::TargetVoxelSize(4.0),
    );
    let Err(GeometryError::OperationFailed(msg)) = got else {
        panic!("expected Err(OperationFailed) for a too-coarse request; got {got:?}");
    };

    #[cfg(has_openvdb)]
    assert!(
        msg.contains('4') && msg.contains("extent"),
        "the diagnostic must name the requested size and the extent it overshot; got {msg:?}"
    );
    #[cfg(not(has_openvdb))]
    let _ = msg;
}
