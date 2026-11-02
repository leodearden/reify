//! The medial extractor must not depend on how close a surface sits to the
//! grid's bounds (task #7631).
//!
//! Each candidate voxel's two surface-hit normals are read from a ±½-spacing
//! finite-difference stencil around the hit point. When a surface lies within
//! half a voxel of a grid face, that stencil straddles the face: one probe
//! lands on the grid and the other does not. The normal must then come from
//! the samples that do exist, not from a component fabricated as zero.

mod common;

use common::{regular3d_field, regular3d_field_with_counts};
use reify_ir::value::SampledField;
use reify_shell_extract::{
    MedialOptions, MinWallThickness, compute_medial_mask, min_wall_thickness,
};

/// Assert the medial mask of `sdf` is non-empty and lies wholly on the
/// x-index plane `mid_plane_i`.
fn assert_mask_is_the_mid_plane(sdf: &SampledField, mid_plane_i: i32) {
    let mask = compute_medial_mask(sdf, &MedialOptions::default()).expect("Ok mask");
    assert!(
        !mask.voxels.is_empty(),
        "{}: medial mask is empty",
        sdf.name
    );
    let off_plane: Vec<[i32; 3]> = mask
        .voxels
        .iter()
        .copied()
        .filter(|v| v[0] != mid_plane_i)
        .collect();
    assert!(
        off_plane.is_empty(),
        "{}: medial voxels off the i = {mid_plane_i} mid-plane: {off_plane:?}",
        sdf.name
    );
}

/// Assert `min_wall_thickness` measures `expected` to within 1e-9.
fn assert_wall_measured(sdf: &SampledField, expected: f64) {
    match min_wall_thickness(sdf, 1.0).expect("Ok min-wall") {
        MinWallThickness::Measured(v) => assert!(
            (v - expected).abs() <= 1e-9,
            "{}: measured {v}, expected {expected}",
            sdf.name
        ),
        other => panic!("{}: expected Measured({expected}), got {other:?}", sdf.name),
    }
}

/// An x-wall `φ = |x| − 1.8` (true thickness 3.6) on a 5-voxel grid spanning
/// x ∈ [−2, 2]. Its faces at ±1.8 sit 0.2 inside the grid's x-faces, so each
/// hit point's outer ½-spacing probe lands at ±2.3, off the grid.
///
/// Before #7631 that x component was silently zeroed: the hit normal
/// degenerated to (0, ~0, ~0), every candidate was rejected, and the mask
/// came back EMPTY (measured: 0 voxels, `NoMeasurement`). The field fully
/// determines the normal there; a one-sided difference against the hit
/// sample recovers it exactly.
#[test]
fn a_wall_whose_faces_lie_within_half_a_voxel_of_the_grid_bounds_is_still_measured() {
    let sdf = regular3d_field("near-bounds-wall", 1.0, 5, -2.0, |x, _y, _z| x.abs() - 1.8);
    assert_mask_is_the_mid_plane(&sdf, 2);
    assert_wall_measured(&sdf, 3.6);
}

/// Guard: a single-sample axis is flat, not unknown.
///
/// A 1-voxel axis is legal input (`compute_medial_mask`'s geometry check
/// accepts a zero-extent axis). Both of its ±½-spacing probes are off-grid at
/// EVERY point, so a gradient that went "unknown" whenever any probe left the
/// grid would empty this mask (measured: 0 voxels, `NoMeasurement`). This
/// passes before and after #7631 and pins the single-sample convention against
/// that regression.
#[test]
fn a_single_sample_axis_is_flat_rather_than_unknown() {
    let sdf = regular3d_field_with_counts(
        "single-sample-y-slice",
        1.0,
        [9, 1, 9],
        -4.0,
        |x, _y, _z| x.abs() - 1.8,
    );
    assert_mask_is_the_mid_plane(&sdf, 4);
    assert_wall_measured(&sdf, 3.6);
}
