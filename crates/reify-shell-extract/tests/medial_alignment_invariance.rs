//! The medial mask must not depend on where the medial surface happens to fall
//! relative to the voxel sample planes (task #7527).
//!
//! # What this pins
//!
//! A slab SDF is symmetric about its own medial plane, so when that plane
//! coincides with a sample plane the central-difference gradient cancels
//! identically. Before #7527 both `compute_medial_mask` and `min_wall_thickness`
//! skipped such a voxel as "degenerate gradient", and the off-plane neighbours
//! were then rejected by the bidirectional-distance equality test (at ±h from
//! the mid-plane `|d⁺ − d⁻| = 2h`, while the equality threshold is
//! `0.05·dmax + h < 2h` for any thin wall) — so the whole mask came back EMPTY
//! at exactly one of the eight sub-voxel alignments, and every measurement
//! built on it degraded to `NoMeasurement`.
//!
//! These are alignment SWEEPS rather than single fixtures: the defect is
//! invisible at seven of the eight offsets, so a fixture that happens to sit
//! off-plane (as every pre-existing slab fixture in this crate does) cannot
//! see it.
//!
//! # Why integration tests rather than in-crate unit tests
//!
//! The whole contract is observable through the crate's public surface —
//! `compute_medial_mask` / `min_feature_size_measure` / `min_wall_thickness`.
//! Nothing here needs to reach a private item.
//!
//! # Grid-layout discipline
//!
//! `SampledField::data` is row-major with axis 0 OUTERMOST
//! (`data[i*ny*nz + j*nz + k]`), which is what `sample_at_index` reads. The
//! builder below follows `medial.rs`'s own `slab_sdf_3d` fixture. Note the
//! reify-eval-side slab fixtures (`shell_solve.rs::build_slab_sdf`,
//! `realization_read_api.rs::slab_field`) flatten z-outermost and are
//! transposed relative to this convention — do not copy those.

use reify_ir::value::{InterpolationKind, SampledField, SampledGridKind};
use reify_shell_extract::{
    MedialOptions, MinFeatureSize, compute_medial_mask, min_feature_size_measure,
};
use std::sync::atomic::AtomicBool;

/// The eight sub-voxel alignments swept by every alignment test here: the
/// medial surface placed at `offset × h` past a sample plane. `0.0` is the
/// coincident alignment that #7527 fixes; the other seven already worked and
/// are kept so a regression in either direction is visible.
const SUB_VOXEL_OFFSETS: [f64; 8] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];

/// Build an isotropic `voxel_count³` Regular3D [`SampledField`] whose value at
/// each grid point is `phi(x, y, z)`, with grid point `(i, j, k)` at world
/// `(bounds_min + i·h, bounds_min + j·h, bounds_min + k·h)`.
fn regular3d_field(
    name: &str,
    h: f64,
    voxel_count: usize,
    bounds_min: f64,
    phi: impl Fn(f64, f64, f64) -> f64,
) -> SampledField {
    let n = voxel_count;
    let axis_grid: Vec<f64> = (0..n).map(|i| bounds_min + (i as f64) * h).collect();
    let bounds_max = axis_grid[n - 1];

    // Row-major flat layout: data[i*n*n + j*n + k] at index (i,j,k).
    let mut data = Vec::with_capacity(n * n * n);
    for &x in &axis_grid {
        for &y in &axis_grid {
            for &z in &axis_grid {
                data.push(phi(x, y, z));
            }
        }
    }

    SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![bounds_min, bounds_min, bounds_min],
        bounds_max: vec![bounds_max, bounds_max, bounds_max],
        spacing: vec![h, h, h],
        axis_grids: vec![axis_grid.clone(), axis_grid.clone(), axis_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    }
}

/// Analytic Z-slab `φ = |z − mid_plane_world| − thickness/2`: a wall of the
/// given thickness whose medial surface is exactly the plane
/// `z = mid_plane_world`, placeable anywhere relative to the sample planes.
fn slab_field(
    mid_plane_world: f64,
    thickness: f64,
    h: f64,
    voxel_count: usize,
    bounds_min: f64,
) -> SampledField {
    regular3d_field(
        &format!("slab-mid{mid_plane_world}-t{thickness}"),
        h,
        voxel_count,
        bounds_min,
        |_x, _y, z| (z - mid_plane_world).abs() - 0.5 * thickness,
    )
}

/// Index of the sample plane nearest `world` on an axis starting at
/// `bounds_min` with spacing `h`.
fn nearest_index(world: f64, bounds_min: f64, h: f64) -> i32 {
    ((world - bounds_min) / h).round() as i32
}

/// The sweep shared by the alignment tests: `h = 1`, a 20³ grid centred so that
/// `z = 0` is a sample plane (index 10), and walls of 3, 4 and 5 voxels — all
/// inside the default 3-voxel narrow-band half-width, so the mid-plane voxel is
/// never excluded by the band filter itself.
const SWEEP_H: f64 = 1.0;
const SWEEP_N: usize = 20;
const SWEEP_BOUNDS_MIN: f64 = -10.0;
const SWEEP_THICKNESS_VOXELS: [f64; 3] = [3.0, 4.0, 5.0];

/// The mask must be non-empty at EVERY sub-voxel alignment, and must stay
/// confined to the mid-plane.
///
/// Before #7527 the `offset = 0` rows returned an empty mask; the other seven
/// offsets returned ≥ 400 voxels (a full 20×20 plane). Non-emptiness is
/// asserted structurally rather than by a pinned count: the count depends on
/// how many planes the equality threshold admits, which is not what this test
/// owns.
#[test]
fn medial_mask_is_non_empty_at_every_sub_voxel_alignment() {
    for thickness_voxels in SWEEP_THICKNESS_VOXELS {
        let thickness = thickness_voxels * SWEEP_H;
        for offset in SUB_VOXEL_OFFSETS {
            let mid = offset * SWEEP_H;
            let sdf = slab_field(mid, thickness, SWEEP_H, SWEEP_N, SWEEP_BOUNDS_MIN);
            let mask = compute_medial_mask(&sdf, &MedialOptions::default())
                .expect("the analytic slab is a structurally valid Regular3D field");

            assert!(
                !mask.voxels.is_empty(),
                "empty medial mask for a {thickness_voxels}-voxel wall whose mid-plane \
                 sits {offset} voxel past a sample plane; the mask must not depend on \
                 where the medial surface falls relative to the grid"
            );

            let mid_k = nearest_index(mid, SWEEP_BOUNDS_MIN, SWEEP_H);
            for &[i, j, k] in &mask.voxels {
                assert!(
                    (k - mid_k).abs() <= 1,
                    "medial voxel [{i}, {j}, {k}] is more than one voxel off the \
                     mid-plane index {mid_k} (wall {thickness_voxels} voxels, \
                     offset {offset})"
                );
            }
        }
    }
}

/// `min_feature_size_measure` reads `2·min|φ|` over the mask voxels, so it
/// recovers as soon as the mask is non-empty — but it recovers to a value that
/// must be within one voxel of the true thickness at every alignment.
///
/// Bounds: `t` is attained exactly when the mid-plane IS a sample plane
/// (`|φ| = t/2` there); `t − h` is attained exactly at the half-voxel offset,
/// where the two nearest voxels each sit `h/2` off the mid-plane. That one-voxel
/// bias-low is the documented property of the `2|φ|` reduction, not slop.
///
/// The `1e-9` tolerance is headroom, not a fitted threshold: the sampled field
/// is exactly piecewise-linear and every quantity here is exactly representable,
/// so the measured values are bit-exact.
#[test]
fn min_feature_size_measure_is_alignment_invariant() {
    for thickness_voxels in SWEEP_THICKNESS_VOXELS {
        let thickness = thickness_voxels * SWEEP_H;
        for offset in SUB_VOXEL_OFFSETS {
            let mid = offset * SWEEP_H;
            let sdf = slab_field(mid, thickness, SWEEP_H, SWEEP_N, SWEEP_BOUNDS_MIN);
            let measured = min_feature_size_measure(&sdf, SWEEP_H)
                .expect("the analytic slab is a structurally valid Regular3D field");

            let MinFeatureSize::Measured(v) = measured else {
                panic!(
                    "expected Measured for a {thickness_voxels}-voxel wall at \
                     offset {offset}; got {measured:?}"
                );
            };
            assert!(
                v >= thickness - SWEEP_H - 1e-9 && v <= thickness + 1e-9,
                "min-feature {v} outside [t − h, t] = [{}, {thickness}] for a \
                 {thickness_voxels}-voxel wall at offset {offset}",
                thickness - SWEEP_H
            );
        }
    }
}

/// The ridge fallback must tag the medial surface of a SOLID, and must NOT tag
/// the medial surface of the GAP between two solids.
///
/// Two 2-voxel walls at `z = ±3` with a 4-voxel gap between them: all three
/// symmetry planes (the two walls' own mid-planes at `k = 4`/`k = 10`, and the
/// gap's mid-plane at `k = 7`) sit exactly on sample planes, so all three have
/// an identically-cancelling central difference. Only the two interior ones are
/// medial axes of the material; the gap plane is a medial axis of the
/// COMPLEMENT, and tagging it would feed a spuriously small `2|φ|` into
/// `min_feature_size_measure`.
#[test]
fn medial_mask_finds_interior_ridges_but_not_the_exterior_ridge_between_two_slabs() {
    let h = 1.0;
    let n = 15;
    let bounds_min = -7.0;
    let sdf = regular3d_field("two-slabs-with-a-gap", h, n, bounds_min, |_x, _y, z| {
        ((z - 3.0 * h).abs() - h).min((z + 3.0 * h).abs() - h)
    });

    let mask = compute_medial_mask(&sdf, &MedialOptions::default())
        .expect("the two-slab field is a structurally valid Regular3D field");

    assert!(
        !mask.voxels.is_empty(),
        "empty medial mask for two grid-aligned walls"
    );

    let k_planes: Vec<i32> = {
        let mut ks: Vec<i32> = mask.voxels.iter().map(|&[_, _, k]| k).collect();
        ks.sort_unstable();
        ks.dedup();
        ks
    };
    assert!(
        k_planes.contains(&4) && k_planes.contains(&10),
        "the two walls' own mid-planes (k = 4 and k = 10) must both be medial; \
         got k-planes {k_planes:?}"
    );
    assert!(
        !k_planes.contains(&7),
        "k = 7 is the mid-plane of the GAP — a medial axis of the complement, not \
         of the material — and must not be tagged; got k-planes {k_planes:?}"
    );
}

/// A medial LINE whose two kink axes BOTH lie on sample planes.
///
/// A square bar `φ = max(|x| − 2h, |z| − 2h)` degenerates the central difference
/// on the x AND z axes simultaneously along its centre line, so the fallback has
/// to choose between two equally sharp kinks. The mask must come back as exactly
/// that centre line.
#[test]
fn medial_mask_finds_a_medial_line_whose_two_kink_axes_both_lie_on_sample_planes() {
    let h = 1.0;
    let n = 15;
    let bounds_min = -7.0;
    let half_width = 2.0 * h;
    let sdf = regular3d_field("square-bar", h, n, bounds_min, |x, _y, z| {
        (x.abs() - half_width).max(z.abs() - half_width)
    });

    let mask = compute_medial_mask(&sdf, &MedialOptions::default())
        .expect("the square-bar field is a structurally valid Regular3D field");

    assert!(
        !mask.voxels.is_empty(),
        "empty medial mask for a grid-aligned square bar"
    );

    let centre = nearest_index(0.0, bounds_min, h);
    for &[i, j, k] in &mask.voxels {
        assert_eq!(
            [i, k],
            [centre, centre],
            "medial voxel [{i}, {j}, {k}] is off the bar's centre line \
             (i = k = {centre})"
        );
    }
}
