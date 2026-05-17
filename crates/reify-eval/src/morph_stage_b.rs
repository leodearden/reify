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
    old_vertices: &[GeometryHandleId],
    new_vertices: &[GeometryHandleId],
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

    match_one_kind(
        SubShapeKind::Vertex,
        old_table,
        new_table,
        old_vertices,
        new_vertices,
        &mut map.vertex_to_vertex,
    )?;

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
/// ## Preconditions
///
/// Both `old` and `new` MUST contain distinct [`GeometryHandleId`] values
/// (no duplicates within either slice). This mirrors the upstream kernel's
/// `extract_faces` / `extract_edges` per-handle-once guarantee.
///
/// **Failure mode:** duplicates in `old` would silently overwrite earlier
/// entries in `out` via [`HashMap::insert`] while the step-7
/// `consumed.len() == new.len()` check still passes — the count-equality
/// guard cannot detect the collision because both `old.len()` and
/// `consumed.len()` inflate identically. The violation is caught in debug
/// builds by the step-2 `debug_assert!` checks.
///
/// ## Algorithm
///
/// 1. **Empty-input early-exit** — both slices empty → `Ok(())`; exactly one
///    side empty → `CountMismatch` (asymmetry rules out a bijection regardless
///    of attribution; bypasses the imported pre-pass to avoid mis-routing the
///    asymmetric case).
/// 2. **Distinct-handle precondition** (debug builds only) — assert `old` and
///    `new` each contain no duplicate `GeometryHandleId` values.
/// 3. **Imported-geometry pre-pass** (mirrors `topology_attribute_resolver.rs:172`)
///    — reached only when BOTH sides are non-empty. If no handle on either side
///    carries an attribute, signal imported geometry as `NamingLayerError::Imported`.
/// 4. **Partial-attribution guard** — if some handles are attributed and
///    others are not, signal malformed state as `NamingLayerError::Partial`.
/// 5. **Count guard** — if `old.len() != new.len()`, return `CountMismatch`.
/// 6. **Matching loop** — walk old handles in slice order. For each old
///    handle look up its attribute and find the first unconsumed new handle
///    whose attribute is equal (linear O(n²) scan; see design decision on
///    !Hash). If no match → `UnmappedElement { Old, handle }`.
/// 7. **Unconsumed-new scan** — any new handle not claimed by an old handle →
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

    // 2. Distinct-handle precondition (debug builds only).
    // Duplicate handles in `old` would silently overwrite earlier entries in
    // `out` (HashMap::insert) while the `consumed.len() == new.len()` count
    // check in step 6 still passes — the count cannot detect the collision.
    // The upstream kernel (`extract_faces` / `extract_edges`) guarantees
    // uniqueness by construction; we assert it here to catch any future caller
    // that violates the contract.
    // Gated behind `#[cfg(debug_assertions)]` so the HashSet allocation and
    // per-iteration insert are structurally absent (not merely DCE-eligible)
    // in release builds. See task 3102 / Task 2727 (`tolerance_scope.rs`) for
    // precedent.
    #[cfg(debug_assertions)]
    {
        let mut seen = HashSet::with_capacity(old.len());
        for &h in old {
            assert!(
                seen.insert(h),
                "old slice passed to match_one_kind contains duplicate GeometryHandleId: {:?}",
                h
            );
        }
    }
    #[cfg(debug_assertions)]
    {
        let mut seen = HashSet::with_capacity(new.len());
        for &h in new {
            assert!(
                seen.insert(h),
                "new slice passed to match_one_kind contains duplicate GeometryHandleId: {:?}",
                h
            );
        }
    }

    // 3. Imported-geometry pre-pass.
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

    // 4. Partial-attribution guard.
    if old_attributed != old.len() || new_attributed != new.len() {
        return Err(BijectionFailure::NamingLayerError {
            kind,
            reason: NamingLayerErrorReason::Partial,
        });
    }

    // 5. Count guard.
    if old.len() != new.len() {
        return Err(BijectionFailure::CountMismatch {
            kind,
            old_count: old.len(),
            new_count: new.len(),
        });
    }

    // 6. Matching loop.
    // Track which new handles have already been claimed so 1-to-1 is enforced.
    let mut consumed: HashSet<GeometryHandleId> = HashSet::new();

    for &old_handle in old {
        // The step-3 pre-pass guarantees all old handles are attributed; the
        // step-2 precondition check guarantees no duplicates in `old`.
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

    // 7. Unconsumed-new invariant: given old.len() == new.len() (step 5 guard)
    // and every old handle consuming exactly one distinct new handle (step 6
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
    use reify_types::{
        AxisSign, CapKind, FeatureId, ModEntry, Role, TopologyAttribute, TopologyAttributeTable,
    };

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
        let result = stage_b_eligible(&old_table, &new_table, &[], &[], &[], &[], &[], &[]);
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

    // step-1 (task 3590): vertex bijection fill — positive case
    #[test]
    fn stage_b_eligible_populates_vertex_to_vertex_when_vertex_attrs_present() {
        let corner = Role::CornerVertex {
            x: AxisSign::Pos,
            y: AxisSign::Pos,
            z: AxisSign::Pos,
        };
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(100), attr(corner.clone(), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(200), attr(corner, 0, None));

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
        let map = result.expect("single matching vertex must succeed");
        assert!(map.face_to_face.is_empty(), "face_to_face must be empty");
        assert!(map.edge_to_edge.is_empty(), "edge_to_edge must be empty");
        assert_eq!(
            map.vertex_to_vertex.len(),
            1,
            "vertex_to_vertex must have exactly one entry"
        );
        assert_eq!(
            map.vertex_to_vertex.get(&h(100)),
            Some(&h(200)),
            "h(100) must map to h(200) in vertex_to_vertex"
        );
    }

    // step-3 (task 3590): vertex count mismatch
    #[test]
    fn stage_b_eligible_handles_count_mismatch_for_vertices() {
        let corner = Role::CornerVertex {
            x: AxisSign::Pos,
            y: AxisSign::Pos,
            z: AxisSign::Pos,
        };
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(100), attr(corner.clone(), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(200), attr(corner.clone(), 0, None));
        new_table.record(
            h(201),
            attr(
                Role::CornerVertex {
                    x: AxisSign::Neg,
                    y: AxisSign::Pos,
                    z: AxisSign::Pos,
                },
                0,
                None,
            ),
        );

        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[],
            &[],
            &[],
            &[],
            &[h(100)],
            &[h(200), h(201)],
        );
        assert_eq!(
            result,
            Err(BijectionFailure::CountMismatch {
                kind: SubShapeKind::Vertex,
                old_count: 1,
                new_count: 2,
            }),
            "1 old vertex vs 2 new vertices must be CountMismatch with kind: Vertex"
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
        assert_eq!(
            map.face_to_face.len(),
            2,
            "both pairs must be in face_to_face"
        );
        assert_eq!(map.face_to_face.get(&h(10)), Some(&h(20)), "h(10) → h(20)");
        assert_eq!(map.face_to_face.get(&h(11)), Some(&h(21)), "h(11) → h(21)");
        assert!(map.edge_to_edge.is_empty(), "edge_to_edge must be empty");
        assert!(
            map.vertex_to_vertex.is_empty(),
            "vertex_to_vertex must be empty"
        );
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
        assert_eq!(
            map.face_to_face.len(),
            2,
            "both duplicate-attr pairs must be matched"
        );
        // Greedy first-fit assigns h(10)→h(20) and h(11)→h(21) (slice order).
        assert_eq!(
            map.face_to_face.get(&h(10)),
            Some(&h(20)),
            "h(10) → h(20) (first-fit)"
        );
        assert_eq!(
            map.face_to_face.get(&h(11)),
            Some(&h(21)),
            "h(11) → h(21) (first-fit)"
        );
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
        assert_eq!(
            map.face_to_face.get(&h(10)),
            Some(&h(20)),
            "face: h(10) → h(20)"
        );
        assert_eq!(map.edge_to_edge.len(), 1, "one edge pair");
        assert_eq!(
            map.edge_to_edge.get(&h(30)),
            Some(&h(40)),
            "edge: h(30) → h(40)"
        );
        assert!(
            map.vertex_to_vertex.is_empty(),
            "vertex_to_vertex must be empty"
        );
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

    /// Symmetric counterpart of `stage_b_eligible_empty_old_with_unattributed_new_returns_count_mismatch`.
    /// Guards against a future "fix" that special-cases only `old.is_empty()` and
    /// re-introduces the bug for the inverse direction (`old` non-empty, `new` empty).
    ///
    /// `old_faces=[h(10)]` with h(10) deliberately absent from `old_table` (unattributed),
    /// `new_faces=[]`. Asymmetric counts make bijection impossible regardless of
    /// attribution status → must return `CountMismatch { old_count: 1, new_count: 0 }`.
    ///
    /// See task 3057: symmetric regression guard for the asymmetric-empty fix.
    #[test]
    fn stage_b_eligible_unattributed_old_with_empty_new_returns_count_mismatch() {
        let old_table = TopologyAttributeTable::default(); // empty — h(10) deliberately absent
        let new_table = TopologyAttributeTable::default(); // empty — no attributes
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10)], // old_faces: one handle, unattributed
            &[],      // new_faces: empty
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
                new_count: 0,
            }),
            "one unattributed old face vs empty new_faces must be CountMismatch, \
             not NamingLayerError::Imported"
        );
    }

    // step-1 (task 3055): duplicate old handles → should panic in debug builds
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(
        expected = "old slice passed to match_one_kind contains duplicate GeometryHandleId"
    )]
    fn match_one_kind_panics_in_debug_on_duplicate_old_handles() {
        // old_table: one attribute under h(10); new_table: two attributes under
        // h(20)/h(30). Both sides are attributed so the imported pre-pass is
        // bypassed and execution reaches the precondition site.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        new_table.record(h(30), attr(Role::Cap(CapKind::Bottom), 0, None));
        // h(10) appears twice in old_faces — contract violation
        let _ = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10), h(10)],
            &[h(20), h(30)],
            &[],
            &[],
            &[],
            &[],
        );
    }

    // step-3 (task 3055): duplicate new handles → should panic in debug builds
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(
        expected = "new slice passed to match_one_kind contains duplicate GeometryHandleId"
    )]
    fn match_one_kind_panics_in_debug_on_duplicate_new_handles() {
        // old_table: two attributes under h(10)/h(11); new_table: one attribute
        // under h(20). Both sides are attributed so the imported pre-pass is
        // bypassed and execution reaches the precondition site.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        old_table.record(h(11), attr(Role::Cap(CapKind::Bottom), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        // h(20) appears twice in new_faces — contract violation
        let _ = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10), h(11)],
            &[h(20), h(20)],
            &[],
            &[],
            &[],
            &[],
        );
    }

    // task 3102: release-mode contract — duplicate handles do NOT panic in release builds
    /// In release builds, the distinct-handle precondition is structurally absent;
    /// duplicate handles flow through to the matching loop where they may produce silent
    /// overwrite or `UnmappedElement`. Callers must not rely on debug-mode panics for
    /// input validation.
    //
    // NOTE: This test is gated `#[cfg(not(debug_assertions))]` and therefore only
    // compiles when the test binary is built without debug_assertions — i.e. via
    // `cargo test --release`. It is silently skipped by a plain `cargo test` (dev
    // profile, debug_assertions = true). Verify it runs in CI by confirming a
    // dedicated `cargo test --release -p reify-eval` step exists in the pipeline.
    #[cfg(not(debug_assertions))]
    #[test]
    fn match_one_kind_does_not_panic_on_duplicate_handles_in_release() {
        // Mirror the fixture from match_one_kind_panics_in_debug_on_duplicate_old_handles:
        // old_table has one attribute under h(10); new_table has two under h(20)/h(30).
        // Both sides are attributed so the imported pre-pass is bypassed and execution
        // reaches the precondition site — which must be structurally absent in release.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0, None));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0, None));
        new_table.record(h(30), attr(Role::Cap(CapKind::Bottom), 0, None));
        // h(10) appears twice — would panic in debug; must not panic in release.
        // In the matching loop the second h(10) finds h(20) already consumed and
        // h(30) with a non-matching attribute, so it returns UnmappedElement.
        let result = stage_b_eligible(
            &old_table,
            &new_table,
            &[h(10), h(10)],
            &[h(20), h(30)],
            &[],
            &[],
            &[],
            &[],
        );
        // Pin the observed release-mode outcome: the second duplicate handle is
        // unmapped (the first consumed h(20), leaving no Cap(Top) match for the
        // second). This is tighter than a no-panic check and catches a regression
        // where a plain `assert!` is reintroduced (which would panic before reaching
        // the matching loop, changing the failure mode).
        assert_eq!(
            result,
            Err(BijectionFailure::UnmappedElement {
                kind: SubShapeKind::Face,
                side: SubShapeSide::Old,
                handle: h(10),
            }),
            "expected UnmappedElement for the second duplicate handle in release mode"
        );
    }

}
