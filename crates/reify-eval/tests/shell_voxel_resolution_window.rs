//! The shell-voxel resolution window must not close (task #6566).
//!
//! # The invariant neither crate can state alone
//!
//! A shell measurement passes through two independent gates that bound the
//! grid from opposite sides, and each crate knows only its own:
//!
//! - **Below**, `reify-kernel-openvdb` refuses a request coarser than
//!   `MIN_FEATURE_VOXELS_ACROSS` voxels across the thinnest extent — below
//!   that the grid is non-null but its interior never signs negative.
//! - **Above**, `reify-shell-extract` discards any voxel with `|φ| >
//!   narrow_band_half_width_voxels × spacing`. A wall's medial plane sits at
//!   `|φ| = half-thickness`, so it survives only while
//!   `voxels-per-thickness ≤ 2 × narrow_band_half_width_voxels`.
//!
//! Nothing connects the two. They live in crates that do not depend on each
//! other — deliberately: `reify-kernel-openvdb` has no dependency on
//! `reify-shell-extract`, and giving it one would invert the layering.
//! `reify-eval` is the only crate that depends on BOTH, which is why this
//! assertion can exist here and nowhere else.
//!
//! If the lower bound ever rises past the upper one, there is no grid the
//! producer will build that the extractor can measure — and the failure is
//! SILENT. Every measurement degrades to `NoMeasurement`; no error path names
//! over-refinement, and no test in either crate alone would go red.
//!
//! # What this does NOT cover
//!
//! This is arithmetic over two published constants, not an end-to-end proof.
//! It says the window is non-empty; it does not re-demonstrate that a real
//! voxelized plate measures correctly inside it. That evidence already exists
//! and is CITED rather than duplicated:
//! `crates/reify-eval/tests/harness_kernel_realization/medial_alignment_invariance_e2e.rs`
//! drives a real plate through `MinFeature(t)` → `densify_grid_to_sampled` →
//! `min_wall_thickness` and asserts `|v − t| ≤ 1e-6` at five sub-voxel plate
//! offsets, with `h = 0.25` on a `t = 1.0` plate — exactly the lower edge,
//! `voxels-per-thickness = 4`. The extractor's upper edge is owned by
//! `crates/reify-shell-extract/tests/medial_resolution_window.rs`, and the
//! producer's refusal of the retired `t/3` figure by
//! `crates/reify-kernel-openvdb/tests/mesh_to_voxel_resolution_tests.rs`.
//!
//! # Why no `cfg(has_openvdb)` gate
//!
//! Both constants are plain `f64`s; reading them allocates no grid and touches
//! no FFI. The cited e2e evidence above IS gated, which is precisely why this
//! guard must not be: a stub build is exactly where a silent window closure
//! would otherwise go unnoticed until it reached a machine with OpenVDB.

use reify_kernel_openvdb::MIN_FEATURE_VOXELS_ACROSS;
use reify_shell_extract::MedialOptions;

/// The producer's COARSEST admissible grid must still fall inside the
/// extractor's narrow band.
///
/// Follows the shape of `reify-shell-extract`'s own
/// `medial_options_defaults_pin_empirical_constants`, one layer up: rather
/// than pinning each default in isolation, it pins the RELATION between two
/// constants that live in different crates, so a change to either has to come
/// here and re-justify the window rather than silently narrowing it.
#[test]
fn the_producers_coarsest_grid_is_inside_the_extractors_narrow_band() {
    let lower = MIN_FEATURE_VOXELS_ACROSS;
    let upper = 2.0 * MedialOptions::default().narrow_band_half_width_voxels;

    assert!(
        lower <= upper,
        "the shell-voxel resolution window has CLOSED: the producer's coarsest \
         admissible grid is {lower} voxels across the thinnest feature, but the \
         medial extractor can only measure up to {upper} \
         (= 2 × narrow_band_half_width_voxels). With lower > upper there is no \
         grid reify-kernel-openvdb will build that reify-shell-extract can \
         measure, and the failure is SILENT — every shell measurement degrades \
         to NoMeasurement with no error path naming the cause. Whichever \
         constant moved must move back, or the extractor's band filter must be \
         widened to match."
    );

    assert_eq!(
        lower, 4.0,
        "MIN_FEATURE_VOXELS_ACROSS sets the window's LOWER edge (the OpenVDB \
         interior-signing floor). Changing it re-opens the question this test \
         answers — update the bound here and re-justify the window in the \
         constant's own doc."
    );
    assert_eq!(
        upper, 6.0,
        "2 × MedialOptions::default().narrow_band_half_width_voxels sets the \
         window's UPPER edge (the medial plane at |φ| = half-thickness must stay \
         in band). Changing it re-opens the question this test answers — update \
         the bound here and re-justify the window in the field's own doc."
    );
}
