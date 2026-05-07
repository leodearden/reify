//! Stage B of the mesh-morphing pipeline — persistent-naming bijection check.
//!
//! Implements the post-realization classifier described in
//! `docs/prds/v0_3/mesh-morphing.md`. This is "Stage B" of the morphing
//! pipeline: after the new B-rep has been realized it asks the persistent-naming
//! v2 layer for a face/edge/vertex bijection between the old and new B-reps.
//!
//! On success it returns a [`CorrespondenceMap`] (consumed by the
//! surface-node projection task #5); on rejection it returns a structured
//! [`BijectionFailure`].
//!
//! ## Purity
//!
//! This module is pure Rust: it does NOT take a `&mut dyn GeometryKernel`.
//! Callers pre-extract handle slices via `kernel.extract_faces(...)` /
//! `kernel.extract_edges(...)` and pass them in alongside two
//! [`reify_types::TopologyAttributeTable`] snapshots — one for the old B-rep,
//! one for the new. This mirrors the discipline of
//! `topology_attribute_resolver.rs` and keeps Stage B testable without an
//! OCCT build.

use std::collections::{HashMap, HashSet};

use reify_types::{GeometryHandleId, TopologyAttributeTable};

// ── Public types ──────────────────────────────────────────────────────────────

/// Successful result of [`stage_b_eligible`]: a 1-to-1 bijection between
/// the old and new B-rep sub-shapes, keyed by the old handle and valued by
/// the corresponding new handle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorrespondenceMap {
    /// Face correspondence: old handle → new handle.
    pub face_to_face: HashMap<GeometryHandleId, GeometryHandleId>,
    /// Edge correspondence: old handle → new handle.
    pub edge_to_edge: HashMap<GeometryHandleId, GeometryHandleId>,
    /// Vertex correspondence: old handle → new handle.
    ///
    /// **Always empty in v0.2** — persistent-naming v2 does not propagate
    /// vertex attributes (`GeometryKernel` exposes only `extract_faces` and
    /// `extract_edges`; there is no `extract_vertices`, and `BRepKind` has no
    /// `Vertex` variant). Reserved for the v0.3+ implementation; downstream
    /// consumers can derive vertex correspondence from edge endpoints in the
    /// meantime. See `docs/prds/v0_2/persistent-naming-v2.md` and the
    /// deferred-bookmarks list.
    pub vertex_to_vertex: HashMap<GeometryHandleId, GeometryHandleId>,
}

/// Which kind of B-rep sub-shape was involved in a [`BijectionFailure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubShapeKind {
    Face,
    Edge,
    Vertex,
}

/// Which side of the bijection (old vs new B-rep) a [`BijectionFailure`]
/// references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubShapeSide {
    Old,
    New,
}

/// Reason sub-variant for [`BijectionFailure::NamingLayerError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingLayerErrorReason {
    /// No attributes exist for any handle on either side — signals imported
    /// geometry that bypassed the per-op populator path (STEP/STL/...).
    /// Mirrors the imported-geometry pre-pass in
    /// `topology_attribute_resolver.rs:172`.
    Imported,
    /// Some handles are attributed, others are not — indicates malformed engine
    /// state (e.g. a partial re-build that populated some ops but not others).
    Partial,
}

/// Failure returned by [`stage_b_eligible`] when a bijection cannot be
/// established.
///
/// Per the task spec's three variant families: count mismatch, unmapped
/// element, and naming-layer error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BijectionFailure {
    /// The total count of old and new sub-shapes of the given kind differs;
    /// a 1-to-1 bijection is impossible.
    CountMismatch {
        kind: SubShapeKind,
        old_count: usize,
        new_count: usize,
    },
    /// A handle on one side has no counterpart on the other side (either the
    /// attribute key is absent, or a 1→2 split means the parent has no single
    /// match).
    ///
    /// See design decision in plan.json: a 1→2 split is reported as
    /// `UnmappedElement` rather than a separate `SplitDetected` variant —
    /// splits violate the bijection and Stage B is strictly 1-to-1.
    UnmappedElement {
        kind: SubShapeKind,
        side: SubShapeSide,
        handle: GeometryHandleId,
    },
    /// The attribute table contains no or partial attribution for the supplied
    /// handles, signalling imported geometry or a malformed engine state.
    ///
    /// Note: `kind` records *which match call surfaced the diagnostic* (the
    /// first kind checked in [`stage_b_eligible`] — faces, then edges). It is
    /// NOT a claim that attribution is missing only for that kind; imported
    /// B-reps typically lack attributes for all kinds simultaneously.
    NamingLayerError {
        kind: SubShapeKind,
        reason: NamingLayerErrorReason,
    },
}

// ── Core API ──────────────────────────────────────────────────────────────────

/// Stage B bijection classifier.
///
/// Attempts to construct a 1-to-1 correspondence map between the faces,
/// edges, and vertices of two B-reps by matching their
/// [`reify_types::TopologyAttribute`] records.
///
/// # Parameters
///
/// * `old_table` / `new_table` — attribute tables snapshotted before / after
///   the new B-rep was realized (the engine wipes its table on every rebuild,
///   so the caller must snapshot the old table before triggering realization).
/// * `old_faces` / `new_faces` — face handle slices extracted by
///   `kernel.extract_faces(...)` on the respective B-reps.
/// * `old_edges` / `new_edges` — edge handle slices extracted by
///   `kernel.extract_edges(...)` on the respective B-reps.
/// * `old_vertices` / `new_vertices` — vertex handle slices (accepted for
///   API forward-compatibility; not processed in v0.2).
///
/// # Returns
///
/// * `Ok(CorrespondenceMap)` — a complete bijection for faces and edges;
///   `vertex_to_vertex` is always empty in v0.2.
/// * `Err(BijectionFailure)` — the first failure encountered while checking
///   faces, then edges (vertices are not checked in v0.2).
#[allow(clippy::too_many_arguments)]
pub fn stage_b_eligible(
    old_table: &TopologyAttributeTable,
    new_table: &TopologyAttributeTable,
    old_faces: &[GeometryHandleId],
    new_faces: &[GeometryHandleId],
    old_edges: &[GeometryHandleId],
    new_edges: &[GeometryHandleId],
    // Accepted for forward-compatibility; not processed in v0.2.
    _old_vertices: &[GeometryHandleId],
    _new_vertices: &[GeometryHandleId],
) -> Result<CorrespondenceMap, BijectionFailure> {
    let mut map = CorrespondenceMap::default();

    match_one_kind(
        SubShapeKind::Face,
        old_table,
        new_table,
        old_faces,
        new_faces,
        &mut map.face_to_face,
    )?;

    match_one_kind(
        SubShapeKind::Edge,
        old_table,
        new_table,
        old_edges,
        new_edges,
        &mut map.edge_to_edge,
    )?;

    // Vertices are intentionally not processed in v0.2 —
    // see CorrespondenceMap::vertex_to_vertex doc-comment.
    // map.vertex_to_vertex stays empty (HashMap::new()).

    Ok(map)
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Attempt to build a 1-to-1 correspondence for one sub-shape kind
/// (faces or edges).
///
/// The caller supplies pre-extracted handle slices and the two attribute
/// tables. On success the matched pairs are inserted into `out`. On
/// failure the appropriate [`BijectionFailure`] variant is returned.
///
/// ## Algorithm
///
/// 1. **Empty-input early-exit** — both slices empty → `Ok(())`; exactly one
///    side empty → `CountMismatch` (asymmetry rules out a bijection regardless
///    of attribution; bypasses the imported pre-pass to avoid mis-routing the
///    asymmetric case).
/// 2. **Imported-geometry pre-pass** (mirrors `topology_attribute_resolver.rs:172`)
///    — reached only when BOTH sides are non-empty. If no handle on either side
///    carries an attribute, signal imported geometry as `NamingLayerError::Imported`.
/// 3. **Partial-attribution guard** — if some handles are attributed and
///    others are not, signal malformed state as `NamingLayerError::Partial`.
/// 4. **Count guard** — if `old.len() != new.len()`, return `CountMismatch`.
/// 5. **Matching loop** — walk old handles in slice order. For each old
///    handle look up its attribute and find the first unconsumed new handle
///    whose attribute is equal (linear O(n²) scan; see design decision on
///    !Hash). If no match → `UnmappedElement { Old, handle }`.
/// 6. **Unconsumed-new scan** — any new handle not claimed by an old handle →
///    `UnmappedElement { New, handle }` (defensive; should not fire when
///    counts match and all old handles paired).
fn match_one_kind(
    kind: SubShapeKind,
    old_table: &TopologyAttributeTable,
    new_table: &TopologyAttributeTable,
    old: &[GeometryHandleId],
    new: &[GeometryHandleId],
    out: &mut HashMap<GeometryHandleId, GeometryHandleId>,
) -> Result<(), BijectionFailure> {
    // 1. Empty-input early-exit — no checks needed for both-empty;
    //    asymmetric-empty is CountMismatch (bypasses imported pre-pass).
    if old.is_empty() && new.is_empty() {
        return Ok(());
    }
    if old.is_empty() || new.is_empty() {
        return Err(BijectionFailure::CountMismatch {
            kind,
            old_count: old.len(),
            new_count: new.len(),
        });
    }

    // 2. Imported-geometry pre-pass.
    let old_attributed = old
        .iter()
        .filter(|&&h| old_table.lookup(h).is_some())
        .count();
    let new_attributed = new
        .iter()
        .filter(|&&h| new_table.lookup(h).is_some())
        .count();

    if old_attributed == 0 && new_attributed == 0 {
        return Err(BijectionFailure::NamingLayerError {
            kind,
            reason: NamingLayerErrorReason::Imported,
        });
    }

    // 3. Partial-attribution guard.
    if old_attributed != old.len() || new_attributed != new.len() {
        return Err(BijectionFailure::NamingLayerError {
            kind,
            reason: NamingLayerErrorReason::Partial,
        });
    }

    // 4. Count guard.
    if old.len() != new.len() {
        return Err(BijectionFailure::CountMismatch {
            kind,
            old_count: old.len(),
            new_count: new.len(),
        });
    }

    // 5. Matching loop.
    // Track which new handles have already been claimed so 1-to-1 is enforced.
    let mut consumed: HashSet<GeometryHandleId> = HashSet::new();

    for &old_handle in old {
        // The pre-pass guarantees all old handles are attributed. If the slice
        // contains duplicate handles, lookup is idempotent — both the attributed
        // count and old.len() inflate identically, so the invariant survives
        // duplicates and .expect cannot fire.
        let old_attr = old_table
            .lookup(old_handle)
            .expect("all old handles attributed — guaranteed by pre-pass above");

        // Linear scan over new handles — O(n) per old handle, O(n²) overall.
        // See design decision: TopologyAttribute is intentionally !Hash, so we
        // cannot use a hash map keyed by attribute; microseconds for n≤200.
        //
        // Match equality is exact (PartialEq on TopologyAttribute, which compares
        // all fields including mod_history), so greedy first-fit is sound: any
        // 1-to-1 pairing of identical attributes is interchangeable.
        let matched_new = new
            .iter()
            .copied()
            .find(|&nh| !consumed.contains(&nh) && new_table.lookup(nh) == Some(old_attr));

        match matched_new {
            Some(new_handle) => {
                consumed.insert(new_handle);
                out.insert(old_handle, new_handle);
            }
            None => {
                return Err(BijectionFailure::UnmappedElement {
                    kind,
                    side: SubShapeSide::Old,
                    handle: old_handle,
                });
            }
        }
    }

    // 6. Unconsumed-new invariant: given old.len() == new.len() (step 4 guard)
    // and every old handle consuming exactly one distinct new handle (step 5
    // loop), the consumed set must cover all of new. No Err can surface here.
    debug_assert_eq!(
        consumed.len(),
        new.len(),
        "consumed set must equal new slice after successful matching loop"
    );

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{CapKind, FeatureId, ModEntry, Role, TopologyAttribute, TopologyAttributeTable};

    fn feat() -> FeatureId {
        FeatureId::new("Feature#realization[0]")
    }

    fn feat2() -> FeatureId {
        FeatureId::new("Feature#realization[1]")
    }

    fn attr(role: Role, local_index: u32, user_label: Option<&str>) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: feat(),
            role,
            local_index,
            user_label: user_label.map(|s| s.to_string()),
            mod_history: Vec::new(),
        }
    }

    fn attr_with_mod(
        role: Role,
        local_index: u32,
        mod_history: Vec<ModEntry>,
    ) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: feat(),
            role,
            local_index,
            user_label: None,
            mod_history,
        }
    }

    fn attr_for_feat(feat_id: FeatureId, role: Role, local_index: u32) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: feat_id,
            role,
            local_index,
            user_label: None,
            mod_history: Vec::new(),
        }
    }

    fn h(n: u64) -> GeometryHandleId {
        GeometryHandleId(n)
    }

    // step-1: empty inputs → empty CorrespondenceMap
    #[test]
    fn stage_b_eligible_empty_inputs_returns_empty_correspondence_map() {
        let old_table = TopologyAttributeTable::default();
        let new_table = TopologyAttributeTable::default();
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let map = result.expect("empty inputs must succeed");
        assert!(
            map.face_to_face.is_empty(),
            "face_to_face must be empty for empty input"
        );
        assert!(
            map.edge_to_edge.is_empty(),
            "edge_to_edge must be empty for empty input"
        );
        assert!(
            map.vertex_to_vertex.is_empty(),
            "vertex_to_vertex must be empty for empty input"
        );
    }

    // step-3: single face with matching attribute pairs handles
    #[test]
    fn stage_b_eligible_single_face_with_matching_attribute_pairs_handles() {
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10)],
            &[h(20)],
            &[],
            &[],
            &[],
            &[],
        );
        let map = result.expect("single matching face must succeed");
        assert_eq!(
            map.face_to_face.len(),
            1,
            "face_to_face must have exactly one entry"
        );
        assert_eq!(
            map.face_to_face.get(&h(10)),
            Some(&h(20)),
            "h(10) must map to h(20)"
        );
        assert!(map.edge_to_edge.is_empty(), "edge_to_edge must be empty");
        assert!(
            map.vertex_to_vertex.is_empty(),
            "vertex_to_vertex must be empty"
        );
    }

    // step-5: single edge with matching attribute pairs handles
    #[test]
    fn stage_b_eligible_single_edge_with_matching_attribute_pairs_handles() {
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::NewEdge, 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::NewEdge, 0, None));
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[],
            &[],
            &[h(10)],
            &[h(20)],
            &[],
            &[],
        );
        let map = result.expect("single matching edge must succeed");
        assert!(map.face_to_face.is_empty(), "face_to_face must be empty");
        assert_eq!(
            map.edge_to_edge.len(),
            1,
            "edge_to_edge must have exactly one entry"
        );
        assert_eq!(
            map.edge_to_edge.get(&h(10)),
            Some(&h(20)),
            "h(10) must map to h(20) in edge_to_edge"
        );
        assert!(
            map.vertex_to_vertex.is_empty(),
            "vertex_to_vertex must be empty"
        );
    }

    // step-7: face count mismatch returns CountMismatch failure
    #[test]
    fn stage_b_eligible_face_total_count_mismatch_returns_count_mismatch_failure() {
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        new_table.record(h(21), attr(Role::Cap(CapKind::Bottom), 0, None));
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10)],
            &[h(20), h(21)],
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(BijectionFailure::CountMismatch {
                kind: SubShapeKind::Face,
                old_count: 1,
                new_count: 2,
            }),
            "1 old face vs 2 new faces must be CountMismatch"
        );
    }

    #[test]
    fn stage_b_eligible_edge_total_count_mismatch_returns_count_mismatch_failure() {
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::NewEdge, 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::NewEdge, 0, None));
        new_table.record(h(21), attr(Role::NewEdge, 1, None));
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[],
            &[],
            &[h(10)],
            &[h(20), h(21)],
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(BijectionFailure::CountMismatch {
                kind: SubShapeKind::Edge,
                old_count: 1,
                new_count: 2,
            }),
            "1 old edge vs 2 new edges must be CountMismatch"
        );
    }

    // step-9: disjoint keys → UnmappedElement
    #[test]
    fn stage_b_eligible_face_with_disjoint_keys_returns_unmapped_element() {
        // K1: (feat(), Cap(Top), 0)
        // K2: (feat2(), Cap(Top), 0) — different feature_id → disjoint
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr_for_feat(feat2(), Role::Cap(CapKind::Top), 0));
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10)],
            &[h(20)],
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(BijectionFailure::UnmappedElement {
                kind: SubShapeKind::Face,
                side: SubShapeSide::Old,
                handle: h(10),
            }),
            "disjoint keys: h(10) has no new counterpart → UnmappedElement Old h(10)"
        );
    }

    // step-11: 1→2 feature split → UnmappedElement
    /// A 1→2 feature split violates the bijection because the parent has no single
    /// counterpart. Stage B is stricter than the resolver (which tolerates splits
    /// via AmbiguousAfterSplit) because morphing requires a literal 1-to-1 bijection.
    ///
    /// Per design decision in plan.json and PRD docs/prds/v0_3/mesh-morphing.md,
    /// splits are subsumed by UnmappedElement — the parent (h(10)) has no exact
    /// attribute match in new (both h(20) and h(21) have non-empty mod_history).
    /// PRD line 64 (mod_history clustering).
    #[test]
    fn stage_b_eligible_face_split_into_two_children_returns_unmapped_element() {
        // old: h(10) → key A (parent, empty mod_history)
        //      h(11) → key B (unrelated parent, empty mod_history)
        // new: h(20) → key A + mod_history[{split_feat, 0}]  (split child 0 of A)
        //      h(21) → key A + mod_history[{split_feat, 1}]  (split child 1 of A)
        // Total counts: old=2, new=2 → bypasses CountMismatch.
        // h(10) cannot match h(20) or h(21) by full-attribute equality (mod_history differs).
        // → UnmappedElement Old h(10).
        let split_feat = FeatureId::new("SplitFeature#realization[0]");
        let child_0 = attr_with_mod(
            Role::Cap(CapKind::Top),
            0,
            vec![ModEntry {
                splitting_feature_id: split_feat.clone(),
                split_index: 0,
            }],
        );
        let child_1 = attr_with_mod(
            Role::Cap(CapKind::Top),
            0,
            vec![ModEntry {
                splitting_feature_id: split_feat.clone(),
                split_index: 1,
            }],
        );

        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None)); // key A, no mod_history
        old_table.record(h(11), attr(Role::Cap(CapKind::Bottom), 0, None)); // key B

        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), child_0); // key A + split mod_history[0]
        new_table.record(h(21), child_1); // key A + split mod_history[1]

        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10), h(11)],
            &[h(20), h(21)],
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(BijectionFailure::UnmappedElement {
                kind: SubShapeKind::Face,
                side: SubShapeSide::Old,
                handle: h(10),
            }),
            "split 1→2: parent h(10) has no exact match in new → UnmappedElement Old h(10)"
        );
    }

    // step-13: imported geometry (empty tables, non-empty slices) → NamingLayerError::Imported
    #[test]
    fn stage_b_eligible_imported_geometry_returns_naming_layer_error() {
        let old_table = TopologyAttributeTable::default(); // empty — no attributes
        let new_table = TopologyAttributeTable::default(); // empty — no attributes
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10)],
            &[h(20)],
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(BijectionFailure::NamingLayerError {
                kind: SubShapeKind::Face,
                reason: NamingLayerErrorReason::Imported,
            }),
            "empty tables with non-empty slices → NamingLayerError::Imported"
        );
    }

    // step-13 companion: partial attribution → NamingLayerError::Partial
    #[test]
    fn stage_b_eligible_partial_attribution_returns_naming_layer_error_partial() {
        // old has h(10) attributed but h(11) NOT in table — partial old attribution.
        // new is fully attributed.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        // h(11) is deliberately absent from old_table
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        new_table.record(h(21), attr(Role::Cap(CapKind::Bottom), 0, None));
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10), h(11)],
            &[h(20), h(21)],
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(BijectionFailure::NamingLayerError {
                kind: SubShapeKind::Face,
                reason: NamingLayerErrorReason::Partial,
            }),
            "partial attribution (h(11) missing) → NamingLayerError::Partial"
        );
    }

    // amend-1: multiple faces with distinct matching attributes — exercises the
    // consumed-set bookkeeping across more than one pair.
    #[test]
    fn stage_b_eligible_multiple_faces_with_distinct_attributes_pairs_all_handles() {
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        old_table.record(h(11), attr(Role::Cap(CapKind::Bottom), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        new_table.record(h(21), attr(Role::Cap(CapKind::Bottom), 0, None));

        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10), h(11)],
            &[h(20), h(21)],
            &[],
            &[],
            &[],
            &[],
        );
        let map = result.expect("two distinct matching faces must succeed");
        assert_eq!(map.face_to_face.len(), 2, "both pairs must be in face_to_face");
        assert_eq!(map.face_to_face.get(&h(10)), Some(&h(20)), "h(10) → h(20)");
        assert_eq!(map.face_to_face.get(&h(11)), Some(&h(21)), "h(11) → h(21)");
        assert!(map.edge_to_edge.is_empty(), "edge_to_edge must be empty");
        assert!(map.vertex_to_vertex.is_empty(), "vertex_to_vertex must be empty");
    }

    // amend-1b: duplicate-attribute pairs (two old + two new sharing identical
    // TopologyAttribute) — greedy first-fit must still produce a complete map
    // without double-claiming.
    #[test]
    fn stage_b_eligible_duplicate_attributes_both_pairs_land_in_map() {
        // Both old faces share the same attribute; both new faces share the same
        // attribute. The match is interchangeable — any 1-to-1 assignment is valid.
        let dup_attr = attr(Role::Cap(CapKind::Top), 0, None);

        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), dup_attr.clone());
        old_table.record(h(11), dup_attr.clone());
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), dup_attr.clone());
        new_table.record(h(21), dup_attr.clone());

        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10), h(11)],
            &[h(20), h(21)],
            &[],
            &[],
            &[],
            &[],
        );
        let map = result.expect("duplicate attributes: greedy first-fit must succeed");
        assert_eq!(map.face_to_face.len(), 2, "both duplicate-attr pairs must be matched");
        // Greedy first-fit assigns h(10)→h(20) and h(11)→h(21) (slice order).
        assert_eq!(map.face_to_face.get(&h(10)), Some(&h(20)), "h(10) → h(20) (first-fit)");
        assert_eq!(map.face_to_face.get(&h(11)), Some(&h(21)), "h(11) → h(21) (first-fit)");
    }

    // amend-2: faces AND edges populated together in one call — guards against
    // swapped slice arguments or accidental kind-only wiring.
    #[test]
    fn stage_b_eligible_faces_and_edges_both_populated_in_single_call() {
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        old_table.record(h(30), attr(Role::NewEdge, 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        new_table.record(h(40), attr(Role::NewEdge, 0, None));

        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10)],
            &[h(20)],
            &[h(30)],
            &[h(40)],
            &[],
            &[],
        );
        let map = result.expect("matching face + edge in same call must succeed");
        assert_eq!(map.face_to_face.len(), 1, "one face pair");
        assert_eq!(map.face_to_face.get(&h(10)), Some(&h(20)), "face: h(10) → h(20)");
        assert_eq!(map.edge_to_edge.len(), 1, "one edge pair");
        assert_eq!(map.edge_to_edge.get(&h(30)), Some(&h(40)), "edge: h(30) → h(40)");
        assert!(map.vertex_to_vertex.is_empty(), "vertex_to_vertex must be empty");
    }

    // amend-3: partial attribution on the NEW side — symmetric coverage for the
    // new_attributed != new.len() branch of the Partial guard.
    #[test]
    fn stage_b_eligible_new_side_partial_attribution_returns_naming_layer_error_partial() {
        // old is fully attributed (two handles).
        // new has h(20) attributed but h(21) is absent from new_table — partial new attribution.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        old_table.record(h(11), attr(Role::Cap(CapKind::Bottom), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        // h(21) is deliberately absent from new_table

        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10), h(11)],
            &[h(20), h(21)],
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(BijectionFailure::NamingLayerError {
                kind: SubShapeKind::Face,
                reason: NamingLayerErrorReason::Partial,
            }),
            "new-side partial attribution (h(21) missing from new_table) → NamingLayerError::Partial"
        );
    }

    /// Bug: when `old_faces` is empty and `new_faces` contains a single unattributed
    /// handle, the current code enters the imported-geometry pre-pass which computes
    /// `old_attributed == 0` (vacuously — empty slice) and `new_attributed == 0`
    /// (h(20) absent from new_table), triggering `NamingLayerError::Imported`.
    ///
    /// The correct result is `CountMismatch { old_count: 0, new_count: 1 }` because
    /// asymmetric counts make a bijection impossible regardless of attribution status.
    ///
    /// See task 3057: asymmetric-empty + unattributed-handle scenario.
    #[test]
    fn stage_b_eligible_empty_old_with_unattributed_new_returns_count_mismatch() {
        let old_table = TopologyAttributeTable::default(); // empty — no attributes
        let new_table = TopologyAttributeTable::default(); // empty — h(20) deliberately absent
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[],      // old_faces: empty
            &[h(20)], // new_faces: one handle, unattributed
            &[],
            &[],
            &[],
            &[],
        );
        assert_eq!(
            result,
            Err(BijectionFailure::CountMismatch {
                kind: SubShapeKind::Face,
                old_count: 0,
                new_count: 1,
            }),
            "empty old_faces vs one unattributed new face must be CountMismatch, \
             not NamingLayerError::Imported"
        );
    }

    // step-15: vertex_to_vertex is always empty in v0.2
    /// Behaviour guard: even when old_vertices and new_vertices are non-empty and
    /// both carry attributes in their respective tables, `vertex_to_vertex` must
    /// remain empty. Documents the v0.2 limitation — persistent-naming v2 does not
    /// propagate vertex attributes (see docs/prds/v0_2/persistent-naming-v2.md).
    /// A future contributor enabling vertex_to_vertex must update this test AND the
    /// doc-comment on `CorrespondenceMap::vertex_to_vertex`.
    #[test]
    fn stage_b_eligible_vertex_to_vertex_is_always_empty_in_v0_2() {
        // Give both old and new vertex handles attributes — the function still must
        // not populate vertex_to_vertex in v0.2.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(100), attr(Role::NewEdge, 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(200), attr(Role::NewEdge, 0, None));

        // Faces and edges are empty so the only non-trivial input is the vertex slices.
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[],
            &[],
            &[],
            &[],
            &[h(100)],
            &[h(200)],
        );
        let map = result.expect(
            "non-empty vertex slices with matching attributes must not cause failure \
             (vertex matching is not performed in v0.2 — \
             see docs/prds/v0_2/persistent-naming-v2.md)",
        );
        assert!(
            map.vertex_to_vertex.is_empty(),
            "vertex_to_vertex must always be empty in v0.2 — \
             persistent-naming v2 does not propagate vertex attributes \
             (see docs/prds/v0_2/persistent-naming-v2.md)"
        );
    }
}
