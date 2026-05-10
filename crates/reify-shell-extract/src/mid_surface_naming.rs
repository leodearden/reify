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

use reify_types::geometry::{FeatureId, Role, TopologyAttribute};

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
/// `<parent>/mid_surface` (see [`FeatureId::derived_mid_surface`]).
///
/// # Face records
///
/// One [`TopologyAttribute`] is emitted per
/// [`crate::segmentation::RegionInfo`], in segmentation-supplied order:
/// `role = Role::MidSurfaceFace`, `local_index = region.label`,
/// `user_label = None`, `mod_history = vec![]`. Region label is the
/// BFS-discovery order assigned by `reify_shell_extract::segmentation`
/// (deterministic for a given mask).
///
/// # Edge records
///
/// Inter-region adjacency edge derivation is added by later plan steps.
pub fn populate_mid_surface_attributes(
    parent: &FeatureId,
    _mesh: &MidSurfaceMesh,
    segmentation: &SegmentationResult,
) -> MidSurfaceAttributes {
    let derived_feature_id = FeatureId::derived_mid_surface(parent);

    let face_records: Vec<TopologyAttribute> = segmentation
        .regions
        .iter()
        .map(|region| TopologyAttribute {
            feature_id: derived_feature_id.clone(),
            role: Role::MidSurfaceFace,
            local_index: region.label,
            user_label: None,
            mod_history: Vec::new(),
        })
        .collect();

    MidSurfaceAttributes {
        face_records,
        edge_records: Vec::new(),
        edge_region_pairs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::geometry::{FeatureId, Role};
    use crate::mid_surface::MidSurfaceMesh;
    use crate::segmentation::{RegionClassification, RegionInfo, SegmentationResult};

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

    #[test]
    fn populate_emits_one_face_record_per_region_with_derived_feature_id_role_and_local_index_eq_region_label(
    ) {
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let segmentation = SegmentationResult {
            regions: vec![
                RegionInfo {
                    label: 0,
                    voxels: vec![],
                    mean_thickness: 1.0,
                    extent: 10.0,
                    thickness_extent_ratio: 0.1,
                    classification: RegionClassification::ShellEligible,
                },
                RegionInfo {
                    label: 1,
                    voxels: vec![],
                    mean_thickness: 0.5,
                    extent: 4.0,
                    thickness_extent_ratio: 0.125,
                    classification: RegionClassification::ShellEligible,
                },
            ],
            vertex_labels: vec![],
            triangle_labels: vec![],
        };
        let parent = FeatureId::new("Bracket#realization[0]");
        let attrs = populate_mid_surface_attributes(&parent, &mesh, &segmentation);

        let derived = FeatureId::new("Bracket#realization[0]/mid_surface");
        assert_eq!(attrs.face_records.len(), 2);
        for i in 0..2 {
            let rec = &attrs.face_records[i];
            assert_eq!(rec.feature_id, derived, "face_records[{i}].feature_id");
            assert_eq!(rec.role, Role::MidSurfaceFace, "face_records[{i}].role");
            assert_eq!(
                rec.local_index, segmentation.regions[i].label,
                "face_records[{i}].local_index must equal region.label"
            );
            assert!(
                rec.user_label.is_none(),
                "face_records[{i}].user_label must be None"
            );
            assert!(
                rec.mod_history.is_empty(),
                "face_records[{i}].mod_history must be empty"
            );
        }
    }
}
