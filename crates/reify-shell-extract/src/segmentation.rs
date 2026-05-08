//! Per-region segmentation classifier for thin-solid medial masks (PRD task T4).
//!
//! Implements connected-component analysis on the medial mask and per-region
//! thickness/extent classification (`shell-eligible` / `tet-eligible` /
//! `mixed-component-of-body`) as specified in
//! `docs/prds/v0_4/structural-analysis-shells.md` §60–65.

#[cfg(test)]
mod tests {
    use crate::{
        segment_regions, MedialMask, MidSurfaceMesh, RegionClassification, RegionInfo,
        SegmentationError, SegmentationOptions, SegmentationResult,
    };

    // ── Step 1: public-surface smoke test ────────────────────────────────────

    /// Smoke test: all public types are reachable from the crate root and
    /// `segment_regions` is callable.  Empty mask + empty mesh → `Ok` with
    /// all output vecs empty.
    #[test]
    fn segment_regions_public_surface_is_callable_on_empty_mask() {
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        };
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let result: SegmentationResult =
            segment_regions(&mask, &mesh, &SegmentationOptions::default())
                .expect("empty mask + empty mesh should return Ok");
        assert!(result.regions.is_empty(), "empty mask → no regions");
        assert!(result.vertex_labels.is_empty(), "empty mesh → no vertex labels");
        assert!(result.triangle_labels.is_empty(), "empty mesh → no triangle labels");
        // Compile-probes: error type and subtypes are reachable.
        let _: SegmentationError = SegmentationError::InvalidThreshold { value: 0.0 };
        let _: Option<RegionInfo> = None;
        let _: Option<RegionClassification> = None;
    }
}
