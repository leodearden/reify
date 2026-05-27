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
//!
//! # Why a struct return rather than `TopologyAttributeTable` mutation
//!
//! The existing per-op populators (e.g. `populate_loft_attributes`,
//! `populate_revolve_attributes` in
//! `reify-eval/src/topology_attribute_propagation.rs`) mutate a
//! `TopologyAttributeTable` keyed by `GeometryHandleId` (the OCCT
//! topology handle). Mid-surface geometry is **derived** (voxel-side)
//! and pre-dates OCCT-handle assignment — there is no handle to key
//! against at the point of population. The downstream engine integration
//! (deferred to T18) takes this struct, assigns handles to the regions
//! and edges, and then folds the records into the table. Co-locating
//! the populator with its inputs (`MidSurfaceMesh`, `SegmentationResult`,
//! `RegionInfo`) in `reify-shell-extract` mirrors the existing pattern
//! where each crate owns its own value-producers; the asymmetry from
//! the mutate-table populators is intentional and contained.

use std::collections::BTreeSet;

use rustc_hash::FxHashMap;

use reify_ir::geometry::{FeatureId, Role, TopologyAttribute};

use crate::mid_surface::MidSurfaceMesh;
use crate::segmentation::SegmentationResult;

/// One inter-region adjacency edge of a derived mid-surface, paired with
/// the canonical `(min, max)` segmentation-region pair that produced it.
///
/// Bundling the [`TopologyAttribute`] and its companion region pair into
/// a single record keeps the mapping structurally enforced — callers
/// cannot filter or sort one without the other and risk desync. The
/// `region_pair` is purely diagnostic; the persistent-naming-relevant
/// identity is encoded in `attribute.local_index` (the 0-based sorted
/// position among all inter-region pairs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidSurfaceEdgeRecord {
    /// The persistent-naming record. `role = Role::MidSurfaceEdge`,
    /// `local_index` = 0-based sorted position among all inter-region
    /// pairs (canonical ascending order on `(min, max)` tuples).
    pub attribute: TopologyAttribute,
    /// The canonical `(min(region_a, region_b), max(region_a, region_b))`
    /// segmentation-region pair this edge separates.
    pub region_pair: (u32, u32),
}

/// Per-region face records and per-inter-region-pair edge records for a
/// derived mid-surface, populated by [`populate_mid_surface_attributes`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MidSurfaceAttributes {
    /// One [`TopologyAttribute`] per [`crate::segmentation::RegionInfo`],
    /// in segmentation-supplied order. `local_index` carries the region
    /// label.
    pub face_records: Vec<TopologyAttribute>,
    /// One [`MidSurfaceEdgeRecord`] per inter-region adjacency edge,
    /// sorted by canonical `(min, max)` region-pair tuple. The bundled
    /// shape (vs. parallel `Vec`s) prevents callers from desyncing the
    /// record/pair mapping.
    pub edges: Vec<MidSurfaceEdgeRecord>,
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
/// One [`MidSurfaceEdgeRecord`] is emitted per unique inter-region
/// adjacency edge, sorted ascending by canonical `(min, max)`
/// region-pair tuple. `role = Role::MidSurfaceEdge`, `local_index` =
/// sorted position. The derivation algorithm builds a mesh-edge →
/// triangle-list adjacency map (using sorted vertex pairs as keys) and,
/// for every mesh edge shared by ≥2 triangles in distinct regions,
/// records the canonical `(min(region_a, region_b), max(region_a,
/// region_b))` pair into a [`BTreeSet`] (auto-sorts). The set is then
/// drained into the bundled `edges: Vec<MidSurfaceEdgeRecord>` so the
/// attribute and its companion region pair stay structurally linked.
///
/// # Input-parallelism contract
///
/// Per [`crate::segmentation::SegmentationResult::triangle_labels`]
/// (segmentation.rs:186), `triangle_labels` must be parallel to
/// `mesh.triangles` (same length, same indexing). This invariant is
/// enforced in all builds via `assert_eq!` at function entry, so both
/// directions of the length mismatch panic with the contract message
/// rather than silently truncating (the release-build failure mode that
/// previously occurred when `triangle_labels.len() > mesh.triangles.len()`).
/// The cost is a single `usize` comparison per call at a derived-geometry
/// boundary — negligible.
///
/// # Sentinel-triangle exclusion contract
///
/// Triangles with `segmentation.triangle_labels[t] == u32::MAX` are
/// excluded from edge derivation. Per the
/// [`crate::segmentation::SegmentationResult::triangle_labels`] doc
/// (segmentation.rs lines 188-191), `u32::MAX` is the sentinel for a
/// triangle whose three vertices have no associated mask voxel — such a
/// triangle has no well-defined region identity. The exclusion is
/// applied in two places:
///
/// 1. The mesh-edge adjacency map skips sentinel triangles entirely,
///    so they do not even appear in the per-edge incident-triangle list.
/// 2. The pairwise label scan rejects any pair touching `u32::MAX`,
///    defending against the case where an upstream change feeds a
///    sentinel through the adjacency map.
///
/// The double-defense matters because a `(u32::MAX, region_x)` pair
/// would always sort to the top of the canonical-pair set and pollute
/// every other edge's `local_index`. The
/// `populate_skips_inter_region_edges_when_triangle_label_is_u32_max_sentinel`
/// test pins this contract behaviorally.
pub fn populate_mid_surface_attributes(
    parent: &FeatureId,
    mesh: &MidSurfaceMesh,
    segmentation: &SegmentationResult,
) -> MidSurfaceAttributes {
    assert_eq!(
        mesh.triangles.len(),
        segmentation.triangle_labels.len(),
        "triangle_labels must be parallel to mesh.triangles \
         (segmentation.rs:186 contract)"
    );

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
        // The triangle_labels-vs-mesh.triangles parallelism is asserted
        // above; an out-of-range index here is a contract violation
        // upstream and will panic in release.
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
        for (i, &t_a) in triangles.iter().enumerate() {
            let label_a = segmentation.triangle_labels[t_a];
            if label_a == u32::MAX {
                continue;
            }
            for &t_b in triangles.iter().skip(i + 1) {
                let label_b = segmentation.triangle_labels[t_b];
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

    let edges: Vec<MidSurfaceEdgeRecord> = region_pairs
        .into_iter()
        .enumerate()
        .map(|(i, region_pair)| MidSurfaceEdgeRecord {
            attribute: TopologyAttribute {
                feature_id: derived_feature_id.clone(),
                role: Role::MidSurfaceEdge,
                local_index: i as u32,
                user_label: None,
                mod_history: Vec::new(),
            },
            region_pair,
        })
        .collect();

    MidSurfaceAttributes {
        face_records,
        edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mid_surface::MidSurfaceMesh;
    use crate::segmentation::{RegionClassification, RegionInfo, SegmentationResult};
    use reify_ir::geometry::{FeatureId, Role};

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
            attrs.face_records.is_empty() && attrs.edges.is_empty(),
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
    fn populate_emits_one_edge_record_per_unique_inter_region_adjacency_with_canonical_min_max_pair_ordering()
     {
        // Two triangles sharing edge (1,2): triangle 0 = (0,1,2),
        // triangle 1 = (0,2,3). Triangle labels live in two different
        // regions; the shared mesh-edge is therefore an inter-region
        // adjacency edge and must be emitted exactly once with canonical
        // (min, max) ordering — i.e. (5, 7) regardless of which triangle
        // carries label 5 vs 7.
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            thickness: vec![1.0, 1.0, 1.0, 1.0],
        };
        // Region 11 is intentionally unused — exercises that the face
        // count tracks regions, not edges.
        let parent = FeatureId::new("Body#realization[0]");
        let derived = FeatureId::new("Body#realization[0]/mid_surface");

        // Sub-test A: triangle_labels = [5, 7] — canonical pair (5, 7).
        // (`vertex_labels` is unread by `populate_mid_surface_attributes`;
        // the populator's only segmentation input is `triangle_labels`.
        // We pass empty vertex_labels here to avoid suggesting otherwise.)
        let segmentation_a = SegmentationResult {
            regions: vec![region(5), region(7), region(11)],
            vertex_labels: vec![],
            triangle_labels: vec![5, 7],
        };
        let attrs_a = populate_mid_surface_attributes(&parent, &mesh, &segmentation_a);
        assert_eq!(attrs_a.edges.len(), 1, "sub-test A: one edge");
        assert_eq!(attrs_a.edges[0].region_pair, (5, 7));
        assert_eq!(attrs_a.edges[0].attribute.feature_id, derived);
        assert_eq!(attrs_a.edges[0].attribute.role, Role::MidSurfaceEdge);
        assert_eq!(attrs_a.edges[0].attribute.local_index, 0);
        assert!(attrs_a.edges[0].attribute.user_label.is_none());
        assert!(attrs_a.edges[0].attribute.mod_history.is_empty());

        // Sub-test B: triangle_labels = [7, 5] — same canonical pair
        // (5, 7) regardless of which triangle's label is bigger.
        let segmentation_b = SegmentationResult {
            regions: vec![region(5), region(7), region(11)],
            vertex_labels: vec![],
            triangle_labels: vec![7, 5],
        };
        let attrs_b = populate_mid_surface_attributes(&parent, &mesh, &segmentation_b);
        assert_eq!(attrs_b.edges.len(), 1);
        assert_eq!(
            attrs_b.edges[0].region_pair,
            (5, 7),
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
            vertex_labels: vec![], // unread by populator (see sub-test A comment)
            triangle_labels: vec![0, 1, 2],
        };
        let attrs_c = populate_mid_surface_attributes(&parent, &mesh3, &segmentation_c);
        let pairs_c: Vec<(u32, u32)> = attrs_c.edges.iter().map(|e| e.region_pair).collect();
        assert_eq!(
            pairs_c,
            vec![(0, 1), (0, 2), (1, 2)],
            "three-region case: canonical ascending pair sort"
        );
        assert_eq!(attrs_c.edges.len(), 3);
        for (i, edge) in attrs_c.edges.iter().enumerate() {
            assert_eq!(
                edge.attribute.local_index, i as u32,
                "edges[{i}].attribute.local_index"
            );
            assert_eq!(edge.attribute.role, Role::MidSurfaceEdge);
            assert_eq!(edge.attribute.feature_id, derived);
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
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            thickness: vec![1.0, 1.0, 1.0, 1.0],
        };
        let parent = FeatureId::new("Body#realization[0]");

        // Sub-test A: one labeled triangle (label=3), one sentinel.
        // Three regions [3, 8, 11] must all yield face_records, but
        // edges must be empty — the (3, u32::MAX) "boundary" is
        // suppressed.
        // (`vertex_labels` is unread by `populate_mid_surface_attributes`;
        // empty here for clarity.)
        let segmentation_a = SegmentationResult {
            regions: vec![region(3), region(8), region(11)],
            vertex_labels: vec![],
            triangle_labels: vec![3, u32::MAX],
        };
        let attrs_a = populate_mid_surface_attributes(&parent, &mesh, &segmentation_a);
        assert_eq!(
            attrs_a.face_records.len(),
            3,
            "faces follow regions, not triangles"
        );
        assert!(
            attrs_a.edges.is_empty(),
            "(3, u32::MAX) sentinel boundary must NOT be emitted"
        );

        // Sub-test B: BOTH triangles share the sentinel. No edges of
        // any kind — no spurious (MAX, MAX) record.
        let segmentation_b = SegmentationResult {
            regions: vec![region(0)],
            vertex_labels: vec![],
            triangle_labels: vec![u32::MAX, u32::MAX],
        };
        let attrs_b = populate_mid_surface_attributes(&parent, &mesh, &segmentation_b);
        assert!(
            attrs_b.edges.is_empty(),
            "two sentinel triangles must yield no spurious (MAX, MAX) edge"
        );
    }

    #[test]
    fn populate_dedups_to_one_edge_when_two_distinct_mesh_edges_separate_same_region_pair() {
        // Pins the BTreeSet dedup contract: one MidSurfaceEdgeRecord per
        // unique inter-region pair, even when multiple distinct mesh-edges
        // straddle the same canonical (min, max) region pair.
        //
        // Setup: 8 vertices forming two independent unit-square pairs
        // (no shared vertices between pair A [0..3] and pair B [4..7]).
        // Each pair contributes exactly one shared mesh-edge; both shared
        // edges straddle region pair (5, 7):
        //
        //   triangle 0 = [0,1,2] (label 5) \
        //   triangle 1 = [0,1,3] (label 7) /  → shared edge (0,1) → (5,7)
        //
        //   triangle 2 = [4,5,6] (label 5) \
        //   triangle 3 = [4,5,7] (label 7) /  → shared edge (4,5) → (5,7)
        //
        // The BTreeSet receives (5,7) from edge (0,1) and (5,7) again
        // from edge (4,5); auto-dedup collapses both insertions to one
        // entry → attrs.edges.len() == 1.
        //
        // Independent pairs mean every other mesh-edge has exactly one
        // incident triangle and is skipped by the `triangles.len() < 2`
        // guard — no confounding edges to investigate on failure.
        //
        // Passes today because the impl uses `BTreeSet<(u32, u32)>` to
        // collect region pairs; will fail RED only if a future refactor
        // swaps it for a `Vec` without manual dedup, behaviorally locking
        // in the BTreeSet dedup contract.
        let mesh = MidSurfaceMesh {
            vertices: vec![
                // pair A — indices 0..3
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                // pair B — indices 4..7, no vertex overlap with pair A
                [2.0, 0.0, 0.0],
                [3.0, 0.0, 0.0],
                [3.0, 1.0, 0.0],
                [2.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 1, 3], [4, 5, 6], [4, 5, 7]],
            thickness: vec![1.0; 8],
        };
        let parent = FeatureId::new("Body#realization[0]");
        let derived = FeatureId::derived_mid_surface(&parent);
        let segmentation = SegmentationResult {
            regions: vec![region(5), region(7)],
            vertex_labels: vec![], // unread by populate_mid_surface_attributes
            triangle_labels: vec![5, 7, 5, 7],
        };
        let attrs = populate_mid_surface_attributes(&parent, &mesh, &segmentation);

        assert_eq!(
            attrs.edges.len(),
            1,
            "BTreeSet must collapse two (5,7) insertions into one edge record"
        );
        assert_eq!(attrs.edges[0].region_pair, (5, 7));
        assert_eq!(attrs.edges[0].attribute.local_index, 0);
        assert_eq!(attrs.edges[0].attribute.role, Role::MidSurfaceEdge);
        assert_eq!(attrs.edges[0].attribute.feature_id, derived);
        assert_eq!(
            attrs.face_records.len(),
            2,
            "face records follow regions, not mesh-edges"
        );
    }

    #[test]
    fn populate_emits_no_edge_when_two_triangles_share_edge_with_same_triangle_label() {
        // Pins the intra-region exclusion contract: a mesh-edge shared by
        // two triangles that both belong to the SAME region must not
        // produce an edge record.
        //
        // Setup: the standard 4-vertex 2-triangle motif (triangles
        // [0,1,2] and [0,2,3] sharing edge (0,2)), but with both
        // triangles carrying the same label (3).  The pairwise label scan
        // hits the `label_a == label_b` guard and short-circuits before
        // any insert → no region pairs → attrs.edges is empty.
        //
        // The face_records assertion confirms the populator fully processed
        // the input; an entirely no-op populator would also pass
        // edges.is_empty() but would fail face_records.len() == 1.  This
        // pins that the empty-edges outcome is specifically due to the
        // intra-region filter, not a no-op early exit elsewhere.
        //
        // Passes today because the pairwise scan filters on `label_a ==
        // label_b`; will fail RED only if a future refactor strips that
        // guard, behaviorally locking the intra-region exclusion contract.
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            thickness: vec![1.0, 1.0, 1.0, 1.0],
        };
        let parent = FeatureId::new("Body#realization[0]");
        let segmentation = SegmentationResult {
            regions: vec![region(3)],
            vertex_labels: vec![], // unread by populate_mid_surface_attributes
            triangle_labels: vec![3, 3],
        };
        let attrs = populate_mid_surface_attributes(&parent, &mesh, &segmentation);

        assert!(
            attrs.edges.is_empty(),
            "intra-region shared edge must produce no edge record"
        );
        assert_eq!(
            attrs.face_records.len(),
            1,
            "face emission is independent of edge filtering — one face per region"
        );
        assert_eq!(
            attrs.face_records[0].local_index, 3,
            "face_records[0].local_index must equal the region label"
        );
    }

    #[test]
    fn populate_emits_one_face_record_per_region_with_derived_feature_id_role_and_local_index_eq_region_label()
     {
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

    // Pins the silent-truncation failure mode: when
    // `triangle_labels.len() > mesh.triangles.len()`, a `debug_assert_eq!`
    // is a no-op in release builds, so the populator silently iterates
    // `mesh.triangles.len()` times without panicking, discarding the
    // surplus labels. The impl change replaces `debug_assert_eq!` with
    // `assert_eq!` so both directions of the length mismatch panic with
    // the contract message in all builds.
    //
    // Note: the OTHER direction (`triangle_labels.len() < mesh.triangles.len()`)
    // already panicked pre-fix via index-OOB in the loop body, so it is not
    // separately tested here.
    #[test]
    #[should_panic(expected = "triangle_labels must be parallel")]
    fn populate_panics_when_triangle_labels_longer_than_mesh_triangles_silent_truncation_case() {
        // Standard 4-vertex / 2-triangle mesh (triangles.len() == 2).
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            thickness: vec![1.0, 1.0, 1.0, 1.0],
        };
        // triangle_labels has length 3 — ONE more than mesh.triangles.len() (2).
        // This is the silent-truncation case that the assert_eq! must catch.
        let segmentation = SegmentationResult {
            regions: vec![region(0), region(1), region(2)],
            vertex_labels: vec![],
            triangle_labels: vec![0, 1, 2],
        };
        populate_mid_surface_attributes(
            &FeatureId::new("Body#realization[0]"),
            &mesh,
            &segmentation,
        );
    }
}
