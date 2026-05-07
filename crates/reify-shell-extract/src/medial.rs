//! Per-voxel medial-mask algorithm for thin-solid mid-surface extraction.
//!
//! Implements PRD task T1
//! (`docs/prds/v0_4/structural-analysis-shells.md`): for each active voxel
//! in a 3D narrow-band SDF, walk the SDF gradient in two opposing
//! directions to find the nearest surface points; tag the voxel as medial
//! iff the two distances are within `distance_tolerance` AND the two
//! surface-hit gradients are roughly antiparallel (encoding the gradient
//! discontinuity at the medial axis).

use reify_types::value::SampledField;

// Step-2 will introduce the public types (`MedialMask`, `MedialOptions`,
// `MedialError`) and the stub `compute_medial_mask` here.
//
// Without a placeholder use, the import would be flagged unused.
#[allow(dead_code)]
fn _force_use(_sdf: &SampledField) {}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::value::{InterpolationKind, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    /// Build a trivial 1×1×1 Regular3D `SampledField` with the given
    /// scalar SDF value at the single voxel. Used as a public-surface
    /// smoke test that exercises every type the crate re-exports
    /// without invoking the algorithm body.
    fn one_voxel_field(phi: f64) -> SampledField {
        SampledField {
            name: "test-1x1x1".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![0.0, 0.0, 0.0],
            spacing: vec![1.0, 1.0, 1.0],
            axis_grids: vec![vec![0.0], vec![0.0], vec![0.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![phi],
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Public-surface compile-test: `MedialMask`, `MedialOptions`,
    /// `MedialError`, and `compute_medial_mask` are reachable from
    /// the crate root, and the function is callable.
    ///
    /// The single voxel sits at `phi = +1.0`, well outside any
    /// reasonable narrow band — the mask must be empty regardless of
    /// downstream-algorithm behaviour.
    #[test]
    fn public_surface_is_callable_on_empty_field() {
        let sdf = one_voxel_field(1.0);
        let opts = MedialOptions::default();
        let mask: MedialMask = compute_medial_mask(&sdf, &opts).expect("Ok mask");
        assert!(
            mask.voxels.is_empty(),
            "single-voxel grid with phi=+1.0 (entirely outside any narrow band) \
             must yield an empty medial mask"
        );

        // Reach the error type from the crate root too — sanity-checks
        // that `MedialError` is publicly named.
        let _: MedialError = MedialError::EmptyAxisGrid { axis: 0 };
    }
}
