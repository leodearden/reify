//! Per-region/per-edge attribute population for the derived mid-surface
//! of a body — the data layer for v0.4 derived-geometry persistent naming.
//!
//! # PRD references
//!
//! - `docs/prds/v0_4/structural-analysis-shells.md` line 81 (T20):
//!   derived-geometry naming sub-vocabulary
//!   (`body.mid_surface().face("region_0")`,
//!   `body.mid_surface().edge("flex_root")`).
//! - `docs/prds/v0_2/persistent-naming-v2.md` lines 52-66
//!   (TopologyAttribute shape / per-op populator pattern).

use reify_types::geometry::{FeatureId, TopologyAttribute};

use crate::mid_surface::MidSurfaceMesh;
use crate::segmentation::SegmentationResult;

/// Per-region face records and per-inter-region-pair edge records for a
/// derived mid-surface, populated by [`populate_mid_surface_attributes`].
///
/// `edge_region_pairs[i]` is the canonical `(min, max)` segmentation-region
/// pair for `edge_records[i]` — a parallel sidecar so callers can introspect
/// the pair without re-scanning the records.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MidSurfaceAttributes {
    /// One [`TopologyAttribute`] per [`crate::segmentation::RegionInfo`],
    /// in segmentation-supplied order. `local_index` carries the region
    /// label.
    pub face_records: Vec<TopologyAttribute>,
    /// One [`TopologyAttribute`] per inter-region adjacency edge, sorted
    /// by canonical `(min, max)` region-pair tuple. `local_index` is the
    /// 0-based sorted position.
    pub edge_records: Vec<TopologyAttribute>,
    /// Parallel-array sidecar: `edge_region_pairs[i]` is the canonical
    /// `(min(region_a, region_b), max(region_a, region_b))` for
    /// `edge_records[i]`.
    pub edge_region_pairs: Vec<(u32, u32)>,
}

/// Populate per-region face attributes and inter-region edge attributes
/// for the derived mid-surface of `parent`.
///
/// All emitted records carry the derived `FeatureId`
/// `<parent>/mid_surface` (see [`FeatureId::derived_mid_surface`]). Step
/// 6 implements only the empty-input contract; later steps in plan 3033
/// add face and edge population.
pub fn populate_mid_surface_attributes(
    parent: &FeatureId,
    _mesh: &MidSurfaceMesh,
    _segmentation: &SegmentationResult,
) -> MidSurfaceAttributes {
    // Reserved for use by later steps; cite usage so the unused-binding
    // lint stays quiet without `_` prefixing the parameter — it IS used,
    // just not in this placeholder body.
    let _ = FeatureId::derived_mid_surface(parent);
    MidSurfaceAttributes::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::geometry::FeatureId;
    use crate::mid_surface::MidSurfaceMesh;
    use crate::segmentation::SegmentationResult;

    #[test]
    fn populate_mid_surface_attributes_returns_empty_records_when_segmentation_has_no_regions() {
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let segmentation = SegmentationResult {
            regions: vec![],
            vertex_labels: vec![],
            triangle_labels: vec![],
        };
        let attrs = populate_mid_surface_attributes(
            &FeatureId::new("Body#realization[0]"),
            &mesh,
            &segmentation,
        );
        assert!(
            attrs.face_records.is_empty()
                && attrs.edge_records.is_empty()
                && attrs.edge_region_pairs.is_empty(),
            "empty segmentation must yield empty face/edge records"
        );
    }
}
