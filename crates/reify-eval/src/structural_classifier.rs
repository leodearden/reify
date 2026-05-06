//! Stage A of the mesh-morphing pipeline — design-tree structural classifier.
//!
//! Implements the pre-flight classifier described in
//! `docs/prds/v0_3/mesh-morphing.md` (Stage A, lines 30–37). This is
//! "Stage A" of the morphing pipeline: before any geometry-kernel work it
//! inspects the design-tree (evaluation graph + runtime value map) and
//! decides whether the parameter edit is *eligible* for mesh morphing.
//!
//! Three public entry points:
//!
//! * [`realization_graph_shape_hash`] — hashes the feature DAG ignoring
//!   runtime leaf parameter values.
//! * [`classify_cell`] — classifies one value-cell as
//!   [`ParameterClass::Dimensional`] or [`ParameterClass::Structural`].
//! * [`stage_a_eligible`] — the top-level predicate: `true` iff (a) the
//!   graph shape is unchanged, (b) every differing leaf is dimensional,
//!   and (c) no feature was added, removed, or reordered.
//!
//! ## Purity
//!
//! This module is pure Rust and does **not** call any geometry kernel.
//! It operates solely on [`EvaluationGraph`] and [`reify_types::ValueMap`].

use std::collections::HashSet;

use reify_types::{ContentHash, Type, ValueCellId, ValueMap};

use crate::graph::EvaluationGraph;

// ── Public types ──────────────────────────────────────────────────────────────

/// Classification of a design-tree value cell for Stage A mesh-morphing
/// eligibility.
///
/// The conservative default is `Structural` — anything that is not clearly a
/// dimensioned scalar, real, or integer is treated as structural. This biases
/// Stage A toward false-rejection (one extra remesh) rather than
/// false-eligibility (a topology-changing edit slipping through to Stage B).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterClass {
    /// The cell holds a dimensioned or numeric quantity whose change cannot
    /// affect feature topology. Includes `Type::Scalar { .. }`, `Type::Real`,
    /// and `Type::Int` (subject to the `structure_controlling` and
    /// `collection_subs` overrides in [`classify_cell`]).
    Dimensional,
    /// The cell controls topology — feature suppression toggles, pattern
    /// counts, enum-typed mode selectors, or any type not whitelisted as
    /// Dimensional. A differing Structural cell makes the edit Stage-A
    /// ineligible.
    Structural,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Compute the shape hash of a realization graph.
///
/// This is Stage A's *shape primitive* (PRD line 33). It produces a
/// deterministic hash of the feature DAG structure — value cells, constraints,
/// realizations, resolutions, guarded groups, connections, and auto-type
/// substitution — **excluding** the runtime [`reify_types::ValueMap`].
///
/// The runtime value map is precisely what the PRD means by "leaf parameter
/// values": it lives in the engine's snapshot, not in the graph. Two graphs
/// with identical shape hashes have identical structural topology, so a
/// differing value map reflects only parameter-value changes (which Stage A
/// then audits cell-by-cell via [`classify_cell`]).
///
/// # Implementation
///
/// Delegates to [`EvaluationGraph::topology_fingerprint`]
/// (`crates/reify-eval/src/graph.rs:507–693`). Using the existing fingerprint
/// ensures Stage A and the realization cache key over the same hash, so a
/// future fingerprint bucket addition automatically applies to Stage A too.
pub fn realization_graph_shape_hash(graph: &EvaluationGraph) -> ContentHash {
    graph.topology_fingerprint()
}


/// Classify a single value cell as [`ParameterClass::Dimensional`] or
/// [`ParameterClass::Structural`].
///
/// Resolution order (first matching rule wins):
///
/// 1. **Cell absent** — cell not in `graph.value_cells` → `Structural`
///    (conservative: unknown cells are assumed topology-controlling).
/// 2. **`structure_controlling`** — cell is in `graph.structure_controlling`
///    → `Structural`. This catches feature-suppression toggles (guard cells
///    added by `EvaluationGraph::from_templates` at graph.rs:442).
/// 3. **Collection count** — cell appears as `count_cell` of any entry in
///    `graph.collection_subs` → `Structural`. Pattern/array counts have
///    `Type::Int` but drive topology via collection elaboration
///    (graph.rs:300–320).
/// 4. **Type dispatch** — `Type::Scalar { .. } | Type::Real | Type::Int`
///    → `Dimensional`; everything else → `Structural`.
pub fn classify_cell(graph: &EvaluationGraph, cell_id: &ValueCellId) -> ParameterClass {
    // Rule 1: missing cell → Structural.
    let Some(node) = graph.value_cells.get(cell_id) else {
        return ParameterClass::Structural;
    };

    // Rule 2: structure-controlling override (feature-suppression toggles,
    // guard cells). Checked before type dispatch so a Bool guard that also
    // happens to be Scalar-shaped is still classified Structural.
    if graph.structure_controlling.contains(cell_id) {
        return ParameterClass::Structural;
    }

    // Rule 3: collection-count override. Pattern/array counts have Type::Int
    // (which would otherwise be Dimensional) but structurally drive collection
    // elaboration (graph.rs:300–320).
    if graph
        .collection_subs
        .iter()
        .any(|sub| &sub.count_cell == cell_id)
    {
        return ParameterClass::Structural;
    }

    // Rule 4: type-based dispatch.
    match &node.cell_type {
        Type::Scalar { .. } | Type::Real | Type::Int => ParameterClass::Dimensional,
        _ => ParameterClass::Structural,
    }
}

/// Stage A top-level eligibility predicate.
///
/// Returns `true` iff the parameter edit from `(old_graph, old_values)` to
/// `(new_graph, new_values)` is eligible for mesh morphing:
///
/// 1. **Shape gate** — `realization_graph_shape_hash(old_graph) ==
///    realization_graph_shape_hash(new_graph)`. This covers PRD criteria (a)
///    and (c): graph structure unchanged, no features added/removed/reordered.
///    Short-circuits before any per-cell work if shapes differ.
///
/// 2. **Value diff** — walk the union of cell IDs in `old_values` and
///    `new_values`. For each cell where the old and new values differ (or the
///    cell is present on only one side), classify it via [`classify_cell`]
///    using `new_graph` (which equals `old_graph` structurally after the shape
///    gate passes). A [`ParameterClass::Dimensional`] diff is allowed; any
///    [`ParameterClass::Structural`] diff makes the edit ineligible → `false`.
///
/// # Why four arguments?
///
/// The PRD (line 33) writes `stage_a_eligible(old_graph, new_graph)` as
/// shorthand, but runtime values live in [`ValueMap`] (maintained by
/// `Engine::edit_param`), not in the graph. Without both ValueMaps there is no
/// way to detect which cells changed. See design decision in plan.json.
pub fn stage_a_eligible(
    old_graph: &EvaluationGraph,
    new_graph: &EvaluationGraph,
    old_values: &ValueMap,
    new_values: &ValueMap,
) -> bool {
    // 1. Shape gate (PRD criterion a + c). Cheap; short-circuits feature
    //    add/remove/reorder before any per-cell classification work.
    if realization_graph_shape_hash(old_graph) != realization_graph_shape_hash(new_graph) {
        return false;
    }

    // 2. Value diff over the union of cell IDs from both maps.
    //
    // Collect the union without allocation duplication: drain old IDs first,
    // then add new IDs that didn't appear in old.
    let mut union_ids: HashSet<&ValueCellId> = HashSet::new();
    union_ids.extend(old_values.iter().map(|(id, _)| id));
    union_ids.extend(new_values.iter().map(|(id, _)| id));

    for id in union_ids {
        if old_values.get(id) == new_values.get(id) {
            continue;
        }
        // Values differ (or the cell is present on only one side: Some vs None).
        // Use new_graph for classification — by the shape gate it is
        // structurally identical to old_graph.
        match classify_cell(new_graph, id) {
            ParameterClass::Dimensional => continue,
            ParameterClass::Structural => return false,
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::ValueCellKind;
    use reify_types::{ContentHash, Type, ValueCellId};

    use crate::graph::{CollectionSubInfo, EvaluationGraph, ValueCellNode};

    // ── Minimal-graph builder helpers ─────────────────────────────────────

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
                content_hash: ContentHash::of_str(&format!("{}", id)),
            },
        );
        g
    }

    // ── Step-1: classify_cell baseline behavior ────────────────────────────

    #[test]
    fn classify_cell_scalar_length_returns_dimensional() {
        let id = ValueCellId::new("Part", "width");
        let g = graph_with_cell(&id, Type::length());
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_scalar_angle_returns_dimensional() {
        let id = ValueCellId::new("Part", "twist");
        let g = graph_with_cell(&id, Type::angle());
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_scalar_dimensionless_returns_dimensional() {
        let id = ValueCellId::new("Part", "ratio");
        let g = graph_with_cell(&id, Type::dimensionless_scalar());
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_real_returns_dimensional() {
        let id = ValueCellId::new("Part", "scale");
        let g = graph_with_cell(&id, Type::Real);
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_int_returns_dimensional() {
        let id = ValueCellId::new("Part", "sides");
        let g = graph_with_cell(&id, Type::Int);
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_enum_returns_structural() {
        let id = ValueCellId::new("Part", "mode");
        let g = graph_with_cell(&id, Type::Enum("Mode".to_string()));
        assert_eq!(classify_cell(&g, &id), ParameterClass::Structural);
    }

    #[test]
    fn classify_cell_bool_returns_structural() {
        // Bool NOT in structure_controlling — still Structural via conservative default.
        let id = ValueCellId::new("Part", "mirrored");
        let g = graph_with_cell(&id, Type::Bool);
        assert_eq!(classify_cell(&g, &id), ParameterClass::Structural);
    }

    #[test]
    fn classify_cell_string_returns_structural() {
        let id = ValueCellId::new("Part", "label");
        let g = graph_with_cell(&id, Type::String);
        assert_eq!(classify_cell(&g, &id), ParameterClass::Structural);
    }

    #[test]
    fn classify_cell_missing_cell_returns_structural() {
        // Cell not present in graph.value_cells → Structural (conservative).
        let g = EvaluationGraph::default();
        let unknown_id = ValueCellId::new("Part", "does_not_exist");
        assert_eq!(classify_cell(&g, &unknown_id), ParameterClass::Structural);
    }

    // ── Step-9: stage_a_eligible – identical graph and values ─────────────

    #[test]
    fn stage_a_eligible_identical_graph_and_values_returns_true() {
        use reify_types::ValueMap;

        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let g2 = g1.clone(); // O(1) structural-sharing clone
        let mut v1 = ValueMap::new();
        v1.insert(id.clone(), reify_types::Value::length(0.08));
        let v2 = v1.clone();
        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "identical graph and values must be stage-A eligible"
        );
    }

    // ── Step-7: realization_graph_shape_hash ──────────────────────────────

    #[test]
    fn realization_graph_shape_hash_two_identical_graphs_produce_equal_hashes() {
        // Two graphs built with the same content must hash identically.
        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let g2 = graph_with_cell(&id, Type::length());
        assert_eq!(
            realization_graph_shape_hash(&g1),
            realization_graph_shape_hash(&g2),
            "identical graphs must produce equal hashes"
        );
    }

    #[test]
    fn realization_graph_shape_hash_added_realization_diverges() {
        use reify_types::RealizationNodeId;
        use crate::graph::RealizationNodeData;

        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let mut g2 = g1.clone();
        // Insert an extra realization node into g2.
        let rid = RealizationNodeId::new("Part", 99);
        g2.realizations.insert(
            rid.clone(),
            RealizationNodeData {
                id: rid,
                operations: vec![],
                content_hash: reify_types::ContentHash::of_str("extra-realization"),
            },
        );
        assert_ne!(
            realization_graph_shape_hash(&g1),
            realization_graph_shape_hash(&g2),
            "adding a realization must change the shape hash"
        );
    }

    #[test]
    fn realization_graph_shape_hash_added_value_cell_diverges() {
        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let mut g2 = g1.clone();
        // Insert an additional value cell into g2.
        let extra_id = ValueCellId::new("Part", "height");
        g2.value_cells.insert(
            extra_id.clone(),
            ValueCellNode {
                id: extra_id.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{}", extra_id)),
            },
        );
        assert_ne!(
            realization_graph_shape_hash(&g1),
            realization_graph_shape_hash(&g2),
            "adding a value cell must change the shape hash"
        );
    }

    #[test]
    fn realization_graph_shape_hash_matches_topology_fingerprint() {
        // Locks the delegation contract: realization_graph_shape_hash must
        // exactly equal graph.topology_fingerprint() — no forked implementation.
        let id = ValueCellId::new("Part", "width");
        let g = graph_with_cell(&id, Type::length());
        assert_eq!(
            realization_graph_shape_hash(&g),
            g.topology_fingerprint(),
            "realization_graph_shape_hash must delegate to topology_fingerprint"
        );
    }

    // ── Step-5: collection_subs count_cell overrides Int → Dimensional ────

    #[test]
    fn classify_cell_collection_count_returns_structural() {
        // An Int-typed cell registered as a collection count (count_cell) must
        // return Structural, overriding the default Int → Dimensional path.
        let id = ValueCellId::new("Part", "__count_bolts");
        let mut g = graph_with_cell(&id, Type::Int);
        // Register this cell as the count_cell of a CollectionSubInfo entry.
        g.collection_subs.push(CollectionSubInfo {
            parent_entity: "Part".to_string(),
            sub_name: "bolts".to_string(),
            structure_name: "Bolt".to_string(),
            count_cell: id.clone(),
            child_value_cells: vec![],
        });
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Structural,
            "collection count_cell must be Structural even though its Type is Int"
        );
    }

    #[test]
    fn classify_cell_int_not_in_collection_subs_remains_dimensional() {
        // Regression guard: an Int cell that is NOT registered as any
        // collection's count_cell must still return Dimensional.  This proves
        // the collection-count check is targeted (count_cell match), not
        // over-broad (all Int cells → Structural).
        let id = ValueCellId::new("Part", "sides");
        let other_id = ValueCellId::new("Part", "__count_bolts");
        let mut g = graph_with_cell(&id, Type::Int);
        // Add a collection_subs entry whose count_cell is a DIFFERENT cell.
        g.collection_subs.push(CollectionSubInfo {
            parent_entity: "Part".to_string(),
            sub_name: "bolts".to_string(),
            structure_name: "Bolt".to_string(),
            count_cell: other_id,
            child_value_cells: vec![],
        });
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Dimensional,
            "Int cell NOT in collection_subs.count_cell must remain Dimensional"
        );
    }

    // ── Step-3: structure_controlling overrides dimensional type ───────────

    #[test]
    fn classify_cell_structure_controlling_overrides_dimensional_type() {
        // A cell whose Type is Scalar { LENGTH } would normally classify as
        // Dimensional — but if it's in graph.structure_controlling it must
        // return Structural. This covers feature-suppression guard cells whose
        // concrete type might be Bool or even a dimensioned scalar in unusual
        // designs.
        let id = ValueCellId::new("Part", "guard");
        let mut g = graph_with_cell(&id, Type::length());
        // Insert the cell's id into structure_controlling.
        g.structure_controlling.insert(id.clone());
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Structural,
            "structure_controlling must override the Dimensional type dispatch"
        );
    }
}
