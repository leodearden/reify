//! The medial extractor has an UPPER resolution edge, and refining past it
//! destroys the measurement rather than improving it (task #6566).
//!
//! # What this pins
//!
//! `compute_medial_mask` admits a voxel only while `|φ(v)| ≤
//! narrow_band_half_width_voxels × min_spacing` (`medial.rs`, `band_width`
//! derived at ~:617 and applied at ~:675). A slab's medial plane sits at
//! exactly `|φ| = half_thickness`, so the mid-plane voxel survives the filter
//! only while `half_thickness ≤ nb·h` — that is, while
//!
//! ```text
//! voxels_per_thickness ≤ 2 × narrow_band_half_width_voxels
//! ```
//!
//! Crucially that filter runs BEFORE `medial_walk_direction`, so an
//! out-of-band mid-plane voxel is discarded before #7527's ridge-axis
//! fallback ever sees it. The fallback cannot rescue what was never
//! enumerated, which is why the failure past the edge is total (an EMPTY mask
//! at every alignment) rather than degraded.
//!
//! This is the counter-intuitive half of the shell-voxel working window:
//! a FINER grid is not a safer one. Past the edge the extractor returns
//! `NoMeasurement` with no diagnostic naming over-refinement as the cause —
//! the producer (`reify-kernel-openvdb`'s `MeshToVoxelOptions::for_resolution`)
//! happily serves such a grid, since it enforces only the opposite, COARSE
//! bound. `crates/reify-eval/tests/shell_voxel_resolution_window.rs` brackets
//! the two constants against each other; this file owns the upper edge itself.
//!
//! # Why the edges, and only the edges
//!
//! `voxels_per_thickness` of 3, 4 and 5 are deliberately NOT re-asserted here:
//! #7527's `min_wall_thickness_is_alignment_invariant` in
//! `medial_alignment_invariance.rs` already owns that interior
//! (`SWEEP_THICKNESS_VOXELS = [3.0, 4.0, 5.0]`), and duplicating it would put
//! the same contract in two files. What is asserted nowhere else — and what
//! this file exists for — is where the interior STOPS.
//!
//! # Why the `+2` margin, and why nothing is asserted below it
//!
//! Both swept values are derived from
//! `MedialOptions::default().narrow_band_half_width_voxels`, never hard-coded:
//! the upper edge is not an independent empirical fact, it IS that constant
//! doubled. Hard-coding `6.0` would duplicate the constant and let a future
//! change to `nb` invalidate this test while it kept passing against a stale
//! number.
//!
//! The `+2` is likewise derived rather than tuned. The medial plane lies at
//! most `h/2` from the nearest sample plane, so the nearest-to-mid voxel has
//! `|φ| = half_thickness − d` for some `d ≤ h/2`, and it stays in band iff
//! `half_thickness − d ≤ nb·h`. At `vpt = 2·nb + 2` that demands `d ≥ h`,
//! which is impossible for ANY `nb`. So the decline at `2·nb + 2` is robust at
//! every alignment by construction, and stays correct if `nb` ever moves.
//!
//! Between the two — the open interval `2·nb < vpt < 2·nb + 2` — behaviour is
//! genuinely alignment-DEPENDENT, and this file asserts nothing there BY
//! DESIGN. Measured at the default `nb = 3.0` (h = 1, all eight offsets):
//! `vpt = 6.5` measures at 5 of 8 alignments and declines at 3; `vpt = 7.0`
//! measures at exactly 1 of 8. An assertion of either polarity in that band
//! would pass or fail on where the fixture's mid-plane happens to fall
//! relative to the grid — precisely the fixture-alignment accident #7527 was
//! filed to eliminate. The window's edge is a cliff with a rough lip, and
//! recording that as a deliberate non-assertion is more durable than pinning
//! one arbitrary alignment.

mod common;

use common::{SUB_VOXEL_OFFSETS, slab_field};
use reify_shell_extract::{
    MedialOptions, MinWallThickness, compute_medial_mask, min_wall_thickness,
};

/// This file's own grid: `h = 1`, a 40³ box spanning `[−20, 20]`. Wider than
/// the alignment sweep's 20³ because the walls here run up to `2·nb + 2 = 8`
/// voxels thick and the bidirectional walk must stay on-grid well past the
/// wall's own faces.
const WINDOW_H: f64 = 1.0;
const WINDOW_N: usize = 40;
const WINDOW_BOUNDS_MIN: f64 = -20.0;

/// At exactly the band edge — `vpt = 2·nb`, where the medial plane sits at
/// `|φ| = nb·h`, the last value the filter admits — the wall must still
/// measure, at every alignment.
///
/// The walk measures the wall EXACTLY rather than approximately: `φ` is
/// piecewise-linear along the walk axis and the trilinear interpolant
/// reproduces it exactly outside the one cell holding the kink, which holds no
/// zero crossing at these thicknesses. So the expected value is the true
/// thickness itself, with no one-voxel allowance. The `1e-9` is headroom over
/// a measured bit-exact result (error exactly `0.0` at all eight alignments),
/// matching the identity and the tolerance already documented for
/// `min_wall_thickness_is_alignment_invariant`.
///
/// The mask is asserted non-empty alongside the measurement so a future
/// regression says WHICH stage failed: an empty mask means the band filter
/// dropped the mid-plane voxel, whereas a non-empty mask with `NoMeasurement`
/// means the re-walk rejected it.
#[test]
fn the_band_edge_voxels_per_thickness_is_still_measured_at_every_alignment() {
    let options = MedialOptions::default();
    let voxels_per_thickness = 2.0 * options.narrow_band_half_width_voxels;
    let thickness = voxels_per_thickness * WINDOW_H;

    for offset in SUB_VOXEL_OFFSETS {
        let mid = offset * WINDOW_H;
        let sdf = slab_field(mid, thickness, WINDOW_H, WINDOW_N, WINDOW_BOUNDS_MIN);

        let mask = compute_medial_mask(&sdf, &options)
            .expect("the analytic slab is a structurally valid Regular3D field");
        assert!(
            !mask.voxels.is_empty(),
            "empty medial mask at the band edge (voxels-per-thickness \
             {voxels_per_thickness} = 2 × narrow_band_half_width_voxels {}, \
             offset {offset}); the medial plane sits at |φ| = {} = nb·h, which \
             the band filter must still admit",
            options.narrow_band_half_width_voxels,
            0.5 * thickness
        );

        let measured = min_wall_thickness(&sdf, WINDOW_H)
            .expect("the analytic slab is a structurally valid Regular3D field");
        let MinWallThickness::Measured(v) = measured else {
            panic!(
                "expected Measured at the band edge (voxels-per-thickness \
                 {voxels_per_thickness}, offset {offset}); got {measured:?}"
            );
        };
        assert!(
            (v - thickness).abs() <= 1e-9,
            "min-wall {v} differs from the true thickness {thickness} at the \
             band edge (voxels-per-thickness {voxels_per_thickness}, offset \
             {offset})"
        );
    }
}

/// Two voxels past the band edge the extractor declines to measure at EVERY
/// alignment — and declines by returning an empty mask, which is what makes
/// the failure total rather than partial.
///
/// This is the assertion that makes the upper edge a contract: without it,
/// "finer is better" remains a plausible reading of the resolution knob, and a
/// caller who over-refines gets silence instead of a diagnostic. Pinning the
/// silence is the most this layer can do; naming it as over-refinement needs a
/// producer-side guard, which is filed as follow-up work.
///
/// See the module doc for why `+2` specifically, and why the interval between
/// the two swept values is left unasserted.
#[test]
fn two_voxels_past_the_band_edge_declines_to_measure_at_every_alignment() {
    let options = MedialOptions::default();
    let voxels_per_thickness = 2.0 * options.narrow_band_half_width_voxels + 2.0;
    let thickness = voxels_per_thickness * WINDOW_H;

    for offset in SUB_VOXEL_OFFSETS {
        let mid = offset * WINDOW_H;
        let sdf = slab_field(mid, thickness, WINDOW_H, WINDOW_N, WINDOW_BOUNDS_MIN);

        let mask = compute_medial_mask(&sdf, &options)
            .expect("the analytic slab is a structurally valid Regular3D field");
        assert!(
            mask.voxels.is_empty(),
            "medial mask is non-empty {} voxels past the band edge \
             (voxels-per-thickness {voxels_per_thickness}, offset {offset}); \
             the nearest-to-mid voxel sits at |φ| ≥ {} > nb·h = {}, so the band \
             filter must have dropped every candidate — got {} voxels",
            2,
            0.5 * thickness - 0.5 * WINDOW_H,
            options.narrow_band_half_width_voxels * WINDOW_H,
            mask.voxels.len()
        );

        let measured = min_wall_thickness(&sdf, WINDOW_H)
            .expect("the analytic slab is a structurally valid Regular3D field");
        assert_eq!(
            measured,
            MinWallThickness::NoMeasurement,
            "expected NoMeasurement two voxels past the band edge \
             (voxels-per-thickness {voxels_per_thickness}, offset {offset}); \
             got {measured:?}"
        );
    }
}
