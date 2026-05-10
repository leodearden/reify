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

use std::collections::BTreeSet;

use rustc_hash::FxHashMap;

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
/// One [`TopologyAttribute`] is emitted per unique inter-region adjacency
/// edge, sorted ascending by canonical `(min, max)` region-pair tuple.
/// `role = Role::MidSurfaceEdge`, `local_index` = sorted position. The
/// derivation algorithm builds a mesh-edge → triangle-list adjacency map
/// (using sorted vertex pairs as keys) and, for every mesh edge shared
/// by ≥2 triangles in distinct regions, records the canonical
/// `(min(region_a, region_b), max(region_a, region_b))` pair into a
/// [`BTreeSet`] (auto-sorts). The set is then drained into both
/// `edge_records` and the parallel `edge_region_pairs` sidecar.
///
/// Sentinel triangles (`segmentation.triangle_labels[t] == u32::MAX`)
/// are excluded from edge derivation — see
/// `reify_shell_extract::segmentation` lines 188-191 for the sentinel
/// definition. A pair `(u32::MAX, region_x)` would be a spurious
/// "boundary" between an unknown-region and a known-region; skipping
/// sentinel triangles means well-formed inputs produce zero spurious
/// edges.
pub fn populate_mid_surface_attributes(
    parent: &FeatureId,
    mesh: &MidSurfaceMesh,
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

    // Mesh-edge → list of incident triangle indices. Key is the sorted
    // vertex pair (min, max) so each undirected edge is canonical.
    let mut edge_to_triangles: FxHashMap<(u32, u32), Vec<usize>> = FxHashMap::default();
    for (t_idx, tri) in mesh.triangles.iter().enumerate() {
        // Sentinel-triangle filter: skip triangles with no region
        // identity. (segmentation.rs:188-191 defines u32::MAX as the
        // sentinel for triangles whose three vertices are all unlabeled.)
        if t_idx >= segmentation.triangle_labels.len() {
            continue;
        }
        if segmentation.triangle_labels[t_idx] == u32::MAX {
            continue;
        }
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let key = if a < b { (a, b) } else { (b, a) };
            edge_to_triangles.entry(key).or_default().push(t_idx);
        }
    }

    // Pairwise distinct-region scan over each shared mesh-edge. Use a
    // BTreeSet to auto-sort the canonical (min, max) pairs.
    let mut region_pairs: BTreeSet<(u32, u32)> = BTreeSet::new();
    for triangles in edge_to_triangles.values() {
        if triangles.len() < 2 {
            continue;
        }
        for i in 0..triangles.len() {
            let label_a = segmentation.triangle_labels[triangles[i]];
            if label_a == u32::MAX {
                continue;
            }
            for j in (i + 1)..triangles.len() {
                let label_b = segmentation.triangle_labels[triangles[j]];
                if label_b == u32::MAX || label_a == label_b {
                    continue;
                }
                let pair = if label_a < label_b {
                    (label_a, label_b)
                } else {
                    (label_b, label_a)
                };
                region_pairs.insert(pair);
            }
        }
    }

    let edge_region_pairs: Vec<(u32, u32)> = region_pairs.into_iter().collect();
    let edge_records: Vec<TopologyAttribute> = edge_region_pairs
        .iter()
        .enumerate()
        .map(|(i, _pair)| TopologyAttribute {
            feature_id: derived_feature_id.clone(),
            role: Role::MidSurfaceEdge,
            local_index: i as u32,
            user_label: None,
            mod_history: Vec::new(),
        })
        .collect();

    MidSurfaceAttributes {
        face_records,
        edge_records,
        edge_region_pairs,
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

    /// Test helper: build a `RegionInfo` with the given label and dummy
    /// metrics. Tests in this module only care about `label`; metrics are
    /// realistic enough to keep `extent > 0` so `thickness_extent_ratio`
    /// is finite.
    fn region(label: u32) -> RegionInfo {
        RegionInfo {
            label,
            voxels: vec![],
            mean_thickness: 1.0,
            extent: 10.0,
            thickness_extent_ratio: 0.1,
            classification: RegionClassification::ShellEligible,
        }
    }

    #[test]
    fn populate_emits_one_edge_record_per_unique_inter_region_adjacency_with_canonical_min_max_pair_ordering(
    ) {
        // Two triangles sharing edge (1,2): triangle 0 = (0,1,2),
        // triangle 1 = (0,2,3). Triangle labels live in two different
        // regions; the shared mesh-edge is therefore an inter-region
        // adjacency edge and must be emitted exactly once with canonical
        // (min, max) ordering — i.e. (5, 7) regardless of which triangle
        // carries label 5 vs 7.
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            thickness: vec![1.0, 1.0, 1.0, 1.0],
        };
        // Region 11 is intentionally unused — exercises that the face
        // count tracks regions, not edges.
        let parent = FeatureId::new("Body#realization[0]");
        let derived = FeatureId::new("Body#realization[0]/mid_surface");

        // Sub-test A: triangle_labels = [5, 7] — canonical pair (5, 7).
        let segmentation_a = SegmentationResult {
            regions: vec![region(5), region(7), region(11)],
            vertex_labels: vec![5, 5, 7, 7],
            triangle_labels: vec![5, 7],
        };
        let attrs_a = populate_mid_surface_attributes(&parent, &mesh, &segmentation_a);
        assert_eq!(attrs_a.edge_records.len(), 1, "sub-test A: one edge");
        assert_eq!(attrs_a.edge_region_pairs, vec![(5, 7)]);
        assert_eq!(attrs_a.edge_records[0].feature_id, derived);
        assert_eq!(attrs_a.edge_records[0].role, Role::MidSurfaceEdge);
        assert_eq!(attrs_a.edge_records[0].local_index, 0);
        assert!(attrs_a.edge_records[0].user_label.is_none());
        assert!(attrs_a.edge_records[0].mod_history.is_empty());

        // Sub-test B: triangle_labels = [7, 5] — same canonical pair
        // (5, 7) regardless of which triangle's label is bigger.
        let segmentation_b = SegmentationResult {
            regions: vec![region(5), region(7), region(11)],
            vertex_labels: vec![7, 7, 5, 5],
            triangle_labels: vec![7, 5],
        };
        let attrs_b = populate_mid_surface_attributes(&parent, &mesh, &segmentation_b);
        assert_eq!(
            attrs_b.edge_region_pairs,
            vec![(5, 7)],
            "canonical (min, max) ordering must be insensitive to triangle label order"
        );

        // Sub-test C: three triangles forming three pairwise-adjacent
        // regions. Mesh: 5 vertices forming a fan; triangles share
        // pairwise edges (0,1), (1,2), and (0,2) ... — easier to set
        // up with three non-overlapping triangles that share three
        // distinct mesh-edges across three labels.
        //
        // We use the configuration:
        //   triangle 0 = (0,1,2)  label=0
        //   triangle 1 = (0,2,3)  label=1   shares edge (0,2) with t0
        //   triangle 2 = (0,3,1)  label=2   shares edge (0,3) with t1
        //                                   shares edge (0,1) with t0
        //
        // → three inter-region pairs: (0,1) from edge (0,2),
        //   (0,2) from edge (0,1), (1,2) from edge (0,3). After
        //   canonical sort: [(0,1), (0,2), (1,2)].
        let mesh3 = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3], [0, 3, 1]],
            thickness: vec![1.0, 1.0, 1.0, 1.0],
        };
        let segmentation_c = SegmentationResult {
            regions: vec![region(0), region(1), region(2)],
            vertex_labels: vec![0, 0, 1, 2],
            triangle_labels: vec![0, 1, 2],
        };
        let attrs_c = populate_mid_surface_attributes(&parent, &mesh3, &segmentation_c);
        assert_eq!(
            attrs_c.edge_region_pairs,
            vec![(0, 1), (0, 2), (1, 2)],
            "three-region case: canonical ascending pair sort"
        );
        assert_eq!(attrs_c.edge_records.len(), 3);
        for (i, rec) in attrs_c.edge_records.iter().enumerate() {
            assert_eq!(rec.local_index, i as u32, "edge_records[{i}].local_index");
            assert_eq!(rec.role, Role::MidSurfaceEdge);
            assert_eq!(rec.feature_id, derived);
        }
    }

    #[test]
    fn populate_skips_inter_region_edges_when_triangle_label_is_u32_max_sentinel() {
        // Pins the segmentation.rs:188-191 sentinel contract:
        // triangle_labels[t] == u32::MAX means "all three vertices are
        // unlabeled / no associated mask voxel". Such triangles must
        // NOT contribute to the inter-region edge set (otherwise we'd
        // emit spurious (3, u32::MAX) edges, violating the canonical
        // ordering invariant since u32::MAX always sorts to the top).
        //
        // This test passes today because step-10's impl includes the
        // filter; it will fail (RED) only if a future refactor strips
        // it, behaviorally locking in the contract.
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            thickness: vec![1.0, 1.0, 1.0, 1.0],
        };
        let parent = FeatureId::new("Body#realization[0]");

        // Sub-test A: one labeled triangle (label=3), one sentinel.
        // Three regions [3, 8, 11] must all yield face_records, but
        // edge_records must be empty — the (3, u32::MAX) "boundary"
        // is suppressed.
        let segmentation_a = SegmentationResult {
            regions: vec![region(3), region(8), region(11)],
            vertex_labels: vec![3, 3, u32::MAX, u32::MAX],
            triangle_labels: vec![3, u32::MAX],
        };
        let attrs_a = populate_mid_surface_attributes(&parent, &mesh, &segmentation_a);
        assert_eq!(
            attrs_a.face_records.len(),
            3,
            "faces follow regions, not triangles"
        );
        assert!(
            attrs_a.edge_records.is_empty(),
            "(3, u32::MAX) sentinel boundary must NOT be emitted"
        );
        assert!(attrs_a.edge_region_pairs.is_empty());

        // Sub-test B: BOTH triangles share the sentinel. No edges of
        // any kind — no spurious (MAX, MAX) record.
        let segmentation_b = SegmentationResult {
            regions: vec![region(0)],
            vertex_labels: vec![u32::MAX, u32::MAX, u32::MAX, u32::MAX],
            triangle_labels: vec![u32::MAX, u32::MAX],
        };
        let attrs_b = populate_mid_surface_attributes(&parent, &mesh, &segmentation_b);
        assert!(
            attrs_b.edge_records.is_empty(),
            "two sentinel triangles must yield no spurious (MAX, MAX) edge"
        );
        assert!(attrs_b.edge_region_pairs.is_empty());
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
