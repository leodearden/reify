//! Combined mesh-morphing eligibility predicate.
//!
//! Implements task #3 of the mesh-morphing PRD
//! (`docs/prds/v0_3/mesh-morphing.md`): wraps Stage A (pre-realization
//! design-tree classifier) and Stage B (post-realization persistent-naming
//! bijection check) per the documented Stage A → realize → Stage B
//! invocation order.

use reify_eval::graph::EvaluationGraph;
use reify_eval::{
    BijectionFailure, CorrespondenceMap, NamingLayerErrorReason, SubShapeKind, stage_a_eligible,
    stage_b_eligible,
};
use reify_ir::{GeometryHandleId, TopologyAttributeTable, ValueMap};

// ── Public types ──────────────────────────────────────────────────────────────

/// A snapshot of one side (old or new) of a mesh-morph eligibility check.
///
/// All fields are borrows — `MorphSnapshot` derives `Copy` for ergonomic
/// pass-by-value at call sites.
///
/// Field roles:
/// - `graph`: the evaluation graph (design tree) for this side.
/// - `values`: the runtime value map for this side; lives outside the graph.
/// - `topology_attributes`: snapshot of the persistent-naming attribute table
///   for this side's B-rep. **MUST be snapshotted BEFORE the other side's
///   realization** — the engine wipes its table on every rebuild, so the
///   caller must preserve the old table before triggering the new-side
///   realization (same caveat as Stage B).
/// - `faces`, `edges`, `vertices`: handle slices extracted from this side's
///   B-rep via `kernel.extract_faces(...)` / `kernel.extract_edges(...)`.
///   `vertices` is accepted for API forward-compatibility; not processed in
///   v0.2.
#[derive(Debug, Clone, Copy)]
pub struct MorphSnapshot<'a> {
    pub graph: &'a EvaluationGraph,
    pub values: &'a ValueMap,
    pub topology_attributes: &'a TopologyAttributeTable,
    pub faces: &'a [GeometryHandleId],
    pub edges: &'a [GeometryHandleId],
    pub vertices: &'a [GeometryHandleId],
}

/// Combined result of [`morph_eligible`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eligibility {
    /// Both Stage A and Stage B passed; the bijection map is ready for the
    /// surface-node projection step.
    Eligible(CorrespondenceMap),
    /// One of the stages rejected the edit; the structured diagnostic is
    /// available for failure-mode visibility counters (PRD task #11).
    Ineligible(Reason),
}

/// Structured rejection reason from [`morph_eligible`].
///
/// Has three top-level variants matching the three failure categories in
/// PRD `docs/prds/v0_3/mesh-morphing.md` (structural-change, bijection-
/// failure, naming-layer-error). The three-way split lets the failure-mode
/// visibility scheme (PRD task #11, task #2948) maintain separate counters
/// per reject category without nested pattern matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Stage A rejected: design-tree shape diverged or a structure-controlling
    /// parameter changed.
    StructuralChange,
    /// Stage B rejected with a count mismatch or an unmapped element.
    ///
    /// **Invariant** (held by `morph_eligible`'s match arm ordering): this
    /// variant never carries `BijectionFailure::NamingLayerError` — that case
    /// is projected to [`Reason::NamingLayerError`] instead. The specific
    /// `NamingLayerError` arm in `morph_eligible` precedes this catch-all,
    /// so `BijectionFailure::NamingLayerError` never reaches here.
    BijectionFailure(BijectionFailure),
    /// Stage B aborted because the persistent-naming layer surfaced an
    /// imported-geometry or partial-attribution diagnostic.
    ///
    /// Projected out of `BijectionFailure::NamingLayerError` so downstream
    /// consumers can match it without nested pattern matching.
    NamingLayerError {
        kind: SubShapeKind,
        reason: NamingLayerErrorReason,
    },
}

// ── Core API ──────────────────────────────────────────────────────────────────

/// Combined mesh-morphing eligibility predicate.
///
/// Combines Stage A (pre-realization classifier) and Stage B (post-realization
/// bijection check) per the documented invocation order:
///
/// ## Order of checks
///
/// 1. **Stage A** — inspects `old.graph` / `new.graph` and `old.values` /
///    `new.values` to decide whether the design-tree shape is unchanged and
///    all differing parameters are dimensional. Cheap; no kernel calls.
///
/// 2. **Realization gate** — if Stage A passes, this function ASSUMES the
///    caller has already realized the new B-rep and populated
///    `new.topology_attributes`, `new.faces`, `new.edges`, and `new.vertices`
///    accordingly. The function does not trigger realization itself.
///
/// 3. **Stage B** — attempts to construct a 1-to-1 correspondence map
///    between old and new B-rep sub-shapes using the persistent-naming
///    attribute tables and handle slices.
///
/// ## Realization deferral
///
/// Callers that want to skip new-side realization on Stage A reject can call
/// [`reify_eval::stage_a_eligible`] directly first. This wrapper is the
/// unified entry point for the failure-mode visibility scheme (PRD task #11)
/// and provides a single call site for combined diagnostics.
///
/// ## Returns
///
/// - [`Eligibility::Eligible(map)`][Eligibility::Eligible] — both stages
///   passed; `map` carries the bijection downstream.
/// - [`Eligibility::Ineligible(reason)`][Eligibility::Ineligible] — a stage
///   rejected the edit; `reason` carries the structured diagnostic.
pub fn morph_eligible(old: MorphSnapshot, new: MorphSnapshot) -> Eligibility {
    // Step 1: Stage A (cheap pre-flight; short-circuits before touching the
    // new-side handles/table).
    if !stage_a_eligible(old.graph, new.graph, old.values, new.values) {
        return Eligibility::Ineligible(Reason::StructuralChange);
    }

    // Step 2: Realization is the caller's responsibility — see doc-comment.
    // Step 3: Stage B (caller is expected to have realized the new B-rep —
    // see doc-comment above).
    match stage_b_eligible(
        old.topology_attributes,
        new.topology_attributes,
        old.faces,
        new.faces,
        old.edges,
        new.edges,
        old.vertices,
        new.vertices,
    ) {
        Ok(map) => Eligibility::Eligible(map),
        // The NamingLayerError variant of BijectionFailure is intentionally
        // projected to its own top-level Reason variant — see
        // Reason::BijectionFailure doc-comment and the failure-mode visibility
        // scheme in PRD docs/prds/v0_3/mesh-morphing.md.
        Err(BijectionFailure::NamingLayerError { kind, reason }) => {
            Eligibility::Ineligible(Reason::NamingLayerError { kind, reason })
        }
        Err(other) => Eligibility::Ineligible(Reason::BijectionFailure(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::ValueCellKind;
    use reify_eval::graph::{EvaluationGraph, RealizationNodeData, ValueCellNode};
    use reify_core::{ContentHash, RealizationNodeId, Type, ValueCellId};
    use reify_ir::{CapKind, FeatureId, ReprKind, Role, TopologyAttribute, TopologyAttributeTable, Value, ValueMap};

    // ── Test fixture helpers ──────────────────────────────────────────────

    /// Build an `EvaluationGraph` with a single value cell of the given type.
    fn graph_with_cell(id: &ValueCellId, cell_type: Type) -> EvaluationGraph {
        let mut g = EvaluationGraph::default();
        g.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Param,
                cell_type,
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{id}")),
            },
        );
        g
    }

    fn h(n: u64) -> GeometryHandleId {
        GeometryHandleId(n)
    }

    fn feat() -> FeatureId {
        FeatureId::new("Feature#realization[0]")
    }

    fn feat2() -> FeatureId {
        FeatureId::new("Feature#realization[1]")
    }

    fn attr(role: Role, local_index: u32) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: feat(),
            role,
            local_index,
            user_label: None,
            mod_history: Vec::new(),
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
    // Note: TopologyAttributeTable uses .record(handle, attr) not .insert()

    // ── Step-3: happy path ────────────────────────────────────────────────

    #[test]
    fn morph_eligible_happy_path_identical_inputs_returns_eligible_with_empty_correspondence_map() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();

        let mut old_values = ValueMap::new();
        old_values.insert(id.clone(), Value::length(0.05));
        let new_values = old_values.clone();

        let old_table = TopologyAttributeTable::default();
        let new_table = TopologyAttributeTable::default();

        let old_snap = MorphSnapshot {
            graph: &old_graph,
            values: &old_values,
            topology_attributes: &old_table,
            faces: &[],
            edges: &[],
            vertices: &[],
        };
        let new_snap = MorphSnapshot {
            graph: &new_graph,
            values: &new_values,
            topology_attributes: &new_table,
            faces: &[],
            edges: &[],
            vertices: &[],
        };

        assert_eq!(
            morph_eligible(old_snap, new_snap),
            Eligibility::Eligible(CorrespondenceMap::default())
        );
    }

    // ── Step-5: Stage A reject ────────────────────────────────────────────

    #[test]
    fn morph_eligible_stage_a_reject_shape_divergence_returns_structural_change() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let mut new_graph = old_graph.clone();

        // Insert a realization node to diverge the topology fingerprint.
        let rnid = RealizationNodeId::new("Extra", 0);
        new_graph.realizations.insert(
            rnid.clone(),
            RealizationNodeData { geometry_cell: None,
                id: rnid,
                operations: Vec::new(),
                content_hash: ContentHash::of_str("diverge"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
            },
        );

        let mut old_values = ValueMap::new();
        old_values.insert(id.clone(), Value::length(0.05));
        let new_values = old_values.clone();

        let old_table = TopologyAttributeTable::default();
        let new_table = TopologyAttributeTable::default();

        let old_snap = MorphSnapshot {
            graph: &old_graph,
            values: &old_values,
            topology_attributes: &old_table,
            faces: &[],
            edges: &[],
            vertices: &[],
        };
        let new_snap = MorphSnapshot {
            graph: &new_graph,
            values: &new_values,
            topology_attributes: &new_table,
            faces: &[],
            edges: &[],
            vertices: &[],
        };

        assert_eq!(
            morph_eligible(old_snap, new_snap),
            Eligibility::Ineligible(Reason::StructuralChange)
        );
    }

    #[test]
    fn morph_eligible_stage_a_reject_dominates_over_stage_b_failure_inputs() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let mut new_graph = old_graph.clone();

        // Diverge topology fingerprint.
        let rnid = RealizationNodeId::new("Extra", 0);
        new_graph.realizations.insert(
            rnid.clone(),
            RealizationNodeData { geometry_cell: None,
                id: rnid,
                operations: Vec::new(),
                content_hash: ContentHash::of_str("diverge2"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
            },
        );

        let mut old_values = ValueMap::new();
        old_values.insert(id.clone(), Value::length(0.05));
        let new_values = old_values.clone();

        // Stage B would also fail: non-empty handle slices, empty tables
        // → NamingLayerError::Imported.
        let old_table = TopologyAttributeTable::default();
        let new_table = TopologyAttributeTable::default();

        let old_snap = MorphSnapshot {
            graph: &old_graph,
            values: &old_values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_snap = MorphSnapshot {
            graph: &new_graph,
            values: &new_values,
            topology_attributes: &new_table,
            faces: &[h(20)],
            edges: &[],
            vertices: &[],
        };

        // Stage A dominates: result is StructuralChange, not NamingLayerError,
        // and not a panic from the Stage B path.
        assert_eq!(
            morph_eligible(old_snap, new_snap),
            Eligibility::Ineligible(Reason::StructuralChange)
        );
    }

    // ── Step-7: Stage B BijectionFailure propagation ──────────────────────

    #[test]
    fn morph_eligible_stage_b_count_mismatch_returns_bijection_failure_reason() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();

        let mut old_values = ValueMap::new();
        old_values.insert(id.clone(), Value::length(0.05));
        let new_values = old_values.clone();

        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0));

        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0));
        new_table.record(h(21), attr(Role::Cap(CapKind::Bottom), 1));

        let old_snap = MorphSnapshot {
            graph: &old_graph,
            values: &old_values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_snap = MorphSnapshot {
            graph: &new_graph,
            values: &new_values,
            topology_attributes: &new_table,
            faces: &[h(20), h(21)],
            edges: &[],
            vertices: &[],
        };

        assert_eq!(
            morph_eligible(old_snap, new_snap),
            Eligibility::Ineligible(Reason::BijectionFailure(BijectionFailure::CountMismatch {
                kind: SubShapeKind::Face,
                old_count: 1,
                new_count: 2,
            }))
        );
    }

    #[test]
    fn morph_eligible_stage_b_unmapped_element_returns_bijection_failure_reason() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();

        let mut old_values = ValueMap::new();
        old_values.insert(id.clone(), Value::length(0.05));
        let new_values = old_values.clone();

        // old and new use different FeatureIds → attributes don't match → UnmappedElement.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr_for_feat(feat(), Role::Cap(CapKind::Top), 0));

        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr_for_feat(feat2(), Role::Cap(CapKind::Top), 0));

        let old_snap = MorphSnapshot {
            graph: &old_graph,
            values: &old_values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_snap = MorphSnapshot {
            graph: &new_graph,
            values: &new_values,
            topology_attributes: &new_table,
            faces: &[h(20)],
            edges: &[],
            vertices: &[],
        };

        let result = morph_eligible(old_snap, new_snap);
        match result {
            Eligibility::Ineligible(Reason::BijectionFailure(
                BijectionFailure::UnmappedElement {
                    kind: SubShapeKind::Face,
                    ..
                },
            )) => {}
            other => panic!("expected UnmappedElement, got {other:?}"),
        }
    }

    // ── Step-9: NamingLayerError projection ───────────────────────────────

    #[test]
    fn morph_eligible_stage_b_naming_layer_imported_returns_top_level_naming_layer_error_reason() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();

        let mut old_values = ValueMap::new();
        old_values.insert(id.clone(), Value::length(0.05));
        let new_values = old_values.clone();

        // Empty tables + non-empty face slices → Imported.
        let old_table = TopologyAttributeTable::default();
        let new_table = TopologyAttributeTable::default();

        let old_snap = MorphSnapshot {
            graph: &old_graph,
            values: &old_values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_snap = MorphSnapshot {
            graph: &new_graph,
            values: &new_values,
            topology_attributes: &new_table,
            faces: &[h(20)],
            edges: &[],
            vertices: &[],
        };

        let result = morph_eligible(old_snap, new_snap);
        match &result {
            Eligibility::Ineligible(Reason::NamingLayerError {
                kind: SubShapeKind::Face,
                reason: NamingLayerErrorReason::Imported,
            }) => {}
            other => panic!(
                "expected top-level NamingLayerError(Imported), not Reason::BijectionFailure \
                 wrapping it; got {other:?}"
            ),
        }
    }

    #[test]
    fn morph_eligible_stage_b_naming_layer_partial_returns_top_level_naming_layer_error_reason() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();

        let mut old_values = ValueMap::new();
        old_values.insert(id.clone(), Value::length(0.05));
        let new_values = old_values.clone();

        // old side: only h(10) has an attribute; h(11) is unattributed →
        // partial attribution on old side → NamingLayerError::Partial.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0));
        // h(11) intentionally absent

        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0));
        new_table.record(h(21), attr(Role::Cap(CapKind::Bottom), 1));

        let old_snap = MorphSnapshot {
            graph: &old_graph,
            values: &old_values,
            topology_attributes: &old_table,
            faces: &[h(10), h(11)],
            edges: &[],
            vertices: &[],
        };
        let new_snap = MorphSnapshot {
            graph: &new_graph,
            values: &new_values,
            topology_attributes: &new_table,
            faces: &[h(20), h(21)],
            edges: &[],
            vertices: &[],
        };

        let result = morph_eligible(old_snap, new_snap);
        match &result {
            Eligibility::Ineligible(Reason::NamingLayerError {
                kind: SubShapeKind::Face,
                reason: NamingLayerErrorReason::Partial,
            }) => {}
            other => panic!("expected top-level NamingLayerError(Partial), got {other:?}"),
        }
    }

    // ── Compile-time Copy contract ────────────────────────────────────────
    // `MorphSnapshot` derives `Copy` to allow ergonomic pass-by-value at call
    // sites (see struct doc-comment). Assert the bound here so a future refactor
    // that drops `Copy` or adds a non-Copy field fails immediately at compile
    // time — no fixture setup needed because the check is purely type-level.
    const _: fn() = || {
        fn assert_copy<T: Copy>() {}
        assert_copy::<MorphSnapshot<'static>>();
    };
}
