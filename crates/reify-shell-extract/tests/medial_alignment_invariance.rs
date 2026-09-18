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
//! The slab builders and the alignment sweep live in `tests/common/mod.rs`,
//! which carries the row-major/axis-0-outermost layout warning they depend on.
//! Read it before adding a fixture here.

mod common;

use common::{SUB_VOXEL_OFFSETS, nearest_index, regular3d_field, slab_field};
use reify_ir::value::SampledField;
use reify_shell_extract::{
    MedialOptions, MinFeatureSize, MinWallThickness, compute_medial_mask, min_feature_size_measure,
    min_wall_thickness,
};

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

/// `min_wall_thickness` re-walks the mask voxels rather than reusing the
/// distances `compute_medial_mask` already computed, so it decides the walk
/// direction a SECOND time — and a non-empty mask is worth nothing if that
/// second decision still skips every voxel in it.
///
/// Unlike the `2|φ|` reduction, the walk measures the wall exactly: `φ` is
/// piecewise-linear along the walk axis and the trilinear interpolant
/// reproduces it exactly outside the one cell holding the kink, which never
/// contains a zero crossing for these thicknesses. So the expected value is `t`
/// itself at every alignment, with no one-voxel allowance. The `1e-9` tolerance
/// is headroom over a measured bit-exact result.
///
/// `BelowResolution` would itself be a regression here: the thinnest wall swept
/// is `3h`, comfortably above the `2h` resolution floor.
#[test]
fn min_wall_thickness_is_alignment_invariant() {
    for thickness_voxels in SWEEP_THICKNESS_VOXELS {
        let thickness = thickness_voxels * SWEEP_H;
        for offset in SUB_VOXEL_OFFSETS {
            let mid = offset * SWEEP_H;
            let sdf = slab_field(mid, thickness, SWEEP_H, SWEEP_N, SWEEP_BOUNDS_MIN);
            let measured = min_wall_thickness(&sdf, SWEEP_H)
                .expect("the analytic slab is a structurally valid Regular3D field");

            let MinWallThickness::Measured(v) = measured else {
                panic!(
                    "expected Measured for a {thickness_voxels}-voxel wall at \
                     offset {offset}; got {measured:?}"
                );
            };
            assert!(
                (v - thickness).abs() <= 1e-9,
                "min-wall {v} differs from the true thickness {thickness} for a \
                 {thickness_voxels}-voxel wall at offset {offset}"
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
/// that centre line, and the min-wall walk must return the bar's full `4h`
/// cross-section — whichever of the two axes it picked has to be an axis that
/// actually measures the bar, not merely one that got the voxel tagged.
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

    let expected = 2.0 * half_width;
    let measured = min_wall_thickness(&sdf, h)
        .expect("the square-bar field is a structurally valid Regular3D field");
    let MinWallThickness::Measured(v) = measured else {
        panic!("expected Measured min-wall for the square bar; got {measured:?}");
    };
    assert!(
        (v - expected).abs() <= 1e-9,
        "min-wall {v} differs from the bar's {expected} cross-section"
    );
}

/// Analytic OBLIQUE slab `φ = |n̂ · p| − thickness/2`: a wall of the given
/// thickness whose medial surface is the plane through the origin with unit
/// normal `n̂`. `normal` is normalised here so callers pass readable integer
/// triples.
fn oblique_slab_field(
    normal: [f64; 3],
    thickness: f64,
    h: f64,
    voxel_count: usize,
    bounds_min: f64,
) -> SampledField {
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    let unit = [normal[0] / length, normal[1] / length, normal[2] / length];
    regular3d_field(
        &format!(
            "oblique-slab-n{}_{}_{}-t{thickness}",
            normal[0], normal[1], normal[2]
        ),
        h,
        voxel_count,
        bounds_min,
        move |x, y, z| (unit[0] * x + unit[1] * y + unit[2] * z).abs() - 0.5 * thickness,
    )
}

/// The oblique sweep's own grid: a 31³ box spanning `[−15, 15]`, wider than the
/// axis-aligned sweep's because an oblique walk is stretched by `1/|n̂ · axis|`
/// and must stay on-grid (the 45° walk covers `2√2 ≈ 2.83` per side).
const OBLIQUE_N: usize = 31;
const OBLIQUE_BOUNDS_MIN: f64 = -15.0;
const OBLIQUE_THICKNESS: f64 = 4.0;

/// Three medial-plane orientations, from shallow to the worst case. All three
/// degenerate the central difference exactly as the axis-aligned sweep does —
/// for `φ = |n̂ · p| − t/2` the samples at `±h` on EVERY axis are both
/// `|n_a·h| − t/2` — so all three take the ridge-axis fallback.
const OBLIQUE_NORMALS: [[f64; 3]; 3] = [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [3.0, 0.0, 1.0]];

/// The walk direction the fallback returns is a grid AXIS, not the medial
/// plane's normal, so on an oblique plane it crosses the wall diagonally and
/// `d⁺ + d⁻` reads `t / |n̂ · axis|` — an OVER-read of up to √3×.
///
/// That breaks `min_wall_thickness`'s conservative-lower-bound contract, and an
/// over-read reaches production as a too-thin wall passing a DFM constraint
/// (`reify-eval`'s `engine_constraints.rs`). The sweep above cannot see it: it
/// varies the medial plane's OFFSET but never its ORIENTATION.
///
/// The bound is deliberately TWO-SIDED rather than `v ≤ t`: an over-CORRECTION
/// is equally wrong, and a one-sided bound would pass one (the hit-point
/// gradient variant rejected in #7527 read `2.0` here, a 2× under-read).
///
/// Every contributing voxel is on the fallback path: a gradient-path voxel would
/// contribute exactly `t`, so any pre-fix reading above `t` proves the minimum
/// comes from the fallback and that this test pins the correction rather than
/// passing incidentally.
///
/// `min_feature_size_measure` needs no obliquity correction — its `2|φ|`
/// reduction is already perpendicular — so it is held to the same `[t − h, t]`
/// band as the axis-aligned sweep.
#[test]
fn min_wall_thickness_is_not_inflated_by_an_oblique_medial_plane() {
    for normal in OBLIQUE_NORMALS {
        let sdf = oblique_slab_field(
            normal,
            OBLIQUE_THICKNESS,
            SWEEP_H,
            OBLIQUE_N,
            OBLIQUE_BOUNDS_MIN,
        );

        let mask = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect("the oblique slab is a structurally valid Regular3D field");
        assert!(
            !mask.voxels.is_empty(),
            "empty medial mask for a wall whose medial plane has normal {normal:?}"
        );

        let measured = min_wall_thickness(&sdf, SWEEP_H)
            .expect("the oblique slab is a structurally valid Regular3D field");
        let MinWallThickness::Measured(v) = measured else {
            panic!("expected Measured min-wall for normal {normal:?}; got {measured:?}");
        };
        assert!(
            (v - OBLIQUE_THICKNESS).abs() <= 1e-9,
            "min-wall {v} differs from the true thickness {OBLIQUE_THICKNESS} for a \
             medial plane with normal {normal:?}; the walk must measure the wall \
             PERPENDICULARLY, not along whichever grid axis it stepped down"
        );

        let feature = min_feature_size_measure(&sdf, SWEEP_H)
            .expect("the oblique slab is a structurally valid Regular3D field");
        let MinFeatureSize::Measured(f) = feature else {
            panic!("expected Measured min-feature for normal {normal:?}; got {feature:?}");
        };
        assert!(
            (OBLIQUE_THICKNESS - SWEEP_H - 1e-9..=OBLIQUE_THICKNESS + 1e-9).contains(&f),
            "min-feature {f} outside [t − h, t] = [{}, {OBLIQUE_THICKNESS}] for a \
             medial plane with normal {normal:?}",
            OBLIQUE_THICKNESS - SWEEP_H
        );
    }
}

/// The obliquity factor must not be read off a valley that no obliquity could
/// have produced.
///
/// A narrow-band SDF saturates `φ` beyond its band (OpenVDB's `meshToLevelSet`
/// does), and where the interior clamp plateau is exactly one voxel thick the
/// mid-plane voxel still presents the ridge fallback a strict valley — but one
/// whose one-sided slopes are a FRACTION of a unit rather than a direction
/// cosine. Read as a conversion factor it scales the wall down by that
/// fraction: `0.5` here, turning a `5h` wall into a confident `Measured(2.5)`
/// where the honest answer is `NoMeasurement`. An under-read reaches
/// `reify-eval`'s min-wall DFM verdict as a spurious violation exactly as an
/// over-read reaches it as a missed one.
///
/// `1/√3` is the smallest cosine any unit normal can present to its own
/// sharpest axis, so a slope of `0.5` is PROOF this is not a medial kink, and
/// the fallback declines rather than measuring. The unsaturated control pins
/// that the geometry and the narrow band are otherwise fine: it is the plateau
/// that is being declined, not the fixture.
///
/// No in-tree producer reaches this state — `MeshToVoxelOptions::honest_floor`
/// and `::for_resolution` both size the band to cover the whole interior — so
/// this pins an invariant the extractor now enforces for itself instead of
/// inheriting it from a producer it cannot see.
#[test]
fn a_saturated_narrow_band_declines_to_measure_rather_than_under_reading() {
    let h = 1.0;
    let n = 25;
    let bounds_min = -12.0;
    let thickness = 5.0 * h;
    let saturation = 2.0 * h;

    let control = slab_field(0.0, thickness, h, n, bounds_min);
    let control_measured = min_wall_thickness(&control, h)
        .expect("the analytic slab is a structurally valid Regular3D field");
    let MinWallThickness::Measured(v) = control_measured else {
        panic!(
            "control: expected Measured for the same wall without saturation; \
             got {control_measured:?}"
        );
    };
    assert!(
        (v - thickness).abs() <= 1e-9,
        "control min-wall {v} differs from the true thickness {thickness}"
    );

    let saturated = regular3d_field("saturated-band-slab", h, n, bounds_min, |_x, _y, z| {
        (z.abs() - 0.5 * thickness).clamp(-saturation, saturation)
    });

    let mask = compute_medial_mask(&saturated, &MedialOptions::default())
        .expect("the saturated slab is a structurally valid Regular3D field");
    assert!(
        mask.voxels.is_empty(),
        "the plateau's one-sided slopes are ±0.5, below the 1/√3 floor any real \
         obliquity clears, so no voxel may be tagged medial; got {} voxels",
        mask.voxels.len()
    );

    assert_eq!(
        min_wall_thickness(&saturated, h)
            .expect("the saturated slab is a structurally valid Regular3D field"),
        MinWallThickness::NoMeasurement,
        "a {thickness}-thick wall whose band saturates one voxel short of its \
         own mid-plane must not be measured at all"
    );
    assert_eq!(
        min_feature_size_measure(&saturated, h)
            .expect("the saturated slab is a structurally valid Regular3D field"),
        MinFeatureSize::NoMeasurement,
        "the same plateau caps 2·min|φ| at {}, a full voxel below the true \
         {thickness}, and must not be reported either",
        2.0 * saturation
    );
}
