//! The alignment-invariance contract of `reify-shell-extract`'s medial
//! measurements, asserted end-to-end over the REAL OpenVDB voxelizer (#7527).
//!
//! # Why this exists next to the in-crate sweep
//!
//! `crates/reify-shell-extract/tests/medial_alignment_invariance.rs` pins the
//! same contract on an analytic slab, where φ is exact. This file pins it on
//! the producer that actually feeds it: `ingest_mesh_at_resolution` →
//! `densify_grid_to_sampled`, whose grid is float32 and whose voxel planes are
//! anchored at the WORLD origin — so where a plate's mid-plane lands relative
//! to the sample planes is decided by the plate's own Z position, not by
//! anything the caller passes to the medial code.
//!
//! Sweeping `z0` over a voxel is therefore the only way to see the defect from
//! the outside: at `z0 ∈ {0, 0.25, 0.5}` the 1-unit-thick plate's mid-plane
//! falls exactly on a sample plane (h = 0.25) and the mask came back EMPTY
//! before #7527, while `z0 ∈ {0.0625, 0.125}` measured correctly all along.
//!
//! # Dual-cfg shape
//!
//! Follows the shape documented in
//! `crates/reify-kernel-openvdb/tests/mesh_to_voxel_resolution_tests.rs`
//! ("Dual-cfg shape"): the real-FFI assertions are `cfg(has_openvdb)`, and a
//! stub build asserts honest degradation instead of silently passing.
//!
//! # Footprint
//!
//! A 10 × 10 × 1 plate, not the 100 × 100 × 1 PRD fixture: the two carry the
//! same alignment information (the measured min-wall error is identical on
//! both, so it is a property of the voxelizer, not of the footprint) and the
//! small one runs the whole sweep in ~4 s against ~102 s.

#[cfg(has_openvdb)]
use reify_ir::{GeometryKernel, VoxelResolution};
#[cfg(has_openvdb)]
use reify_kernel_openvdb::{OpenVdbKernel, test_fixtures::axis_aligned_box};
#[cfg(has_openvdb)]
use reify_shell_extract::{
    MedialOptions, MinFeatureSize, MinWallThickness, compute_medial_mask, min_feature_size_measure,
    min_wall_thickness,
};

/// Z offsets of the plate's bottom face, in units of the h = 0.25 voxel:
/// 0 (mid-plane exactly on a sample plane), ¼, ½, 1 and 2 voxels.
#[cfg(has_openvdb)]
const PLATE_Z_OFFSETS: [f32; 5] = [0.0, 0.0625, 0.125, 0.25, 0.5];

/// The plate's through-thickness dimension, and the `MinFeature` request that
/// must resolve it: `MinFeature(t)` asks for `h = t / 4`.
#[cfg(has_openvdb)]
const PLATE_THICKNESS: f64 = 1.0;
#[cfg(has_openvdb)]
const EXPECTED_H: f64 = 0.25;

/// Voxelize the 10 × 10 × 1 plate with its bottom face at `z0` and return the
/// densified narrow-band SDF.
#[cfg(has_openvdb)]
fn plate_sdf(z0: f32) -> reify_ir::value::SampledField {
    let plate = axis_aligned_box([0.0, 0.0, z0], [10.0, 10.0, z0 + PLATE_THICKNESS as f32]);
    let mut kernel = OpenVdbKernel::new();
    let handle = GeometryKernel::ingest_mesh_at_resolution(
        &mut kernel,
        &plate,
        VoxelResolution::MinFeature(PLATE_THICKNESS),
    )
    .expect("MinFeature(1.0) on a valid closed plate must succeed");
    kernel
        .densify_grid_to_sampled(handle.id)
        .expect("densify_grid_to_sampled must succeed for an ingested plate")
}

/// The mask must be non-empty, and both measurements must land on the plate's
/// true thickness, at EVERY sub-voxel placement of the plate.
///
/// Non-emptiness is asserted structurally — the voxel COUNT is a property of
/// the voxelizer version (measured 1225 at four of the five offsets and 2730 at
/// `z0 = 0.125`), not of this contract.
///
/// Tolerance: the OpenVDB grid stores float32 (relative eps 1.19e-7), so `1e-6`
/// is roughly 8 f32 ulp at this magnitude — headroom over the measured worst
/// case rather than a fixture-fitted threshold. Anything near or below 1e-7
/// would be asserting inside float32 noise. `min_feature`'s lower bound is
/// additionally `t − h`, the documented ≤ 1-voxel bias-low of the `2|φ|`
/// reduction; the min-wall walk has no such bias and is held to `t` directly.
#[cfg(has_openvdb)]
#[test]
fn medial_measurements_are_alignment_invariant_on_the_real_voxelizer() {
    for z0 in PLATE_Z_OFFSETS {
        let sdf = plate_sdf(z0);

        let h = sdf.spacing[0].min(sdf.spacing[1]).min(sdf.spacing[2]);
        assert!(
            (h - EXPECTED_H).abs() <= 1e-9,
            "MinFeature({PLATE_THICKNESS}) must give h = {EXPECTED_H}; got {h} at z0 = {z0}"
        );

        let mask = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect("a densified openvdb grid is a structurally valid Regular3D field");
        assert!(
            !mask.voxels.is_empty(),
            "empty medial mask for the plate at z0 = {z0}; the mask must not depend on \
             where the plate's mid-plane falls relative to the voxel sample planes"
        );

        let measured = min_feature_size_measure(&sdf, h)
            .expect("a densified openvdb grid is a structurally valid Regular3D field");
        let MinFeatureSize::Measured(v) = measured else {
            panic!("expected Measured min-feature at z0 = {z0}; got {measured:?}");
        };
        assert!(
            v >= PLATE_THICKNESS - h - 1e-6 && v <= PLATE_THICKNESS + 1e-6,
            "min-feature {v} outside [t − h, t] = [{}, {PLATE_THICKNESS}] at z0 = {z0}",
            PLATE_THICKNESS - h
        );

        let measured = min_wall_thickness(&sdf, h)
            .expect("a densified openvdb grid is a structurally valid Regular3D field");
        let MinWallThickness::Measured(v) = measured else {
            panic!("expected Measured min-wall at z0 = {z0}; got {measured:?}");
        };
        assert!(
            (v - PLATE_THICKNESS).abs() <= 1e-6,
            "min-wall {v} differs from the plate's thickness {PLATE_THICKNESS} at z0 = {z0}"
        );
    }
}

/// Honest degradation on a stub build: without the OpenVDB FFI there is no grid
/// to measure, and the kernel must say so rather than hand back an empty one.
#[cfg(not(has_openvdb))]
#[test]
fn min_feature_ingest_degrades_honestly_without_openvdb() {
    use reify_ir::{GeometryError, GeometryKernel, VoxelResolution};
    use reify_kernel_openvdb::{OpenVdbKernel, test_fixtures::axis_aligned_box};

    let plate = axis_aligned_box([0.0, 0.0, 0.0], [10.0, 10.0, 1.0]);
    let mut kernel = OpenVdbKernel::new();
    let requested = GeometryKernel::ingest_mesh_at_resolution(
        &mut kernel,
        &plate,
        VoxelResolution::MinFeature(1.0),
    );

    assert!(
        matches!(requested, Err(GeometryError::OperationFailed(_))),
        "MinFeature must degrade to OperationFailed on the stub kernel; got {requested:?}"
    );
}
