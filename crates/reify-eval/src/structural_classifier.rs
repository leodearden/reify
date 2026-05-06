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

use reify_types::{Type, ValueCellId};

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
}
