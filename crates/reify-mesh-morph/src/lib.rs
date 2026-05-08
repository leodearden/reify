//! Mesh morphing classifier and engine for Reify.
//!
//! This crate provides the combined eligibility predicate for mesh morphing
//! (PRD `docs/prds/v0_3/mesh-morphing.md`, tasks #3 and #10).
//!
//! ## PRD task #5 — boundary-node correspondence + closest-point projection — boundary module
//!
//! The [`boundary`] module implements the surface-node → Dirichlet-BC
//! translation step that gates the elasticity morph (PRD task #7).
//!
//! ## PRD task #6 — Laplacian quick-pass — laplacian module
//!
//! The [`laplacian`] module implements the constrained Laplacian smoother
//! used as the cheap fast path for trivially small parameter changes —
//! surface nodes pinned to `prescribed_positions` (produced by
//! [`compute_dirichlet_bcs`]), interior nodes iteratively averaged with
//! their topological neighbours via Jacobi iteration. Engine wiring (PRD
//! task #10) selects between this smoother and the elasticity morph.

pub mod boundary;
pub mod eligibility;
pub mod laplacian;
pub mod options;
pub mod types;

pub use boundary::{
    BoundaryAssociation, NodeAttachment, ProjectionFailure, ProjectorPayload, Projector,
    compute_dirichlet_bcs,
};
pub use eligibility::{Eligibility, MorphSnapshot, Reason, morph_eligible};
pub use laplacian::{LaplacianFailure, laplacian_smooth};
pub use options::{MorphFailure, MorphOptions};
pub use types::{BRep, InversionDetails, MetricsBreached, SolverErrorPayload};

/// Re-exported so consumers can pattern-match `Reason::BijectionFailure(_)`
/// without depending on `reify-eval` directly.
pub use reify_eval::{
    BijectionFailure, CorrespondenceMap, NamingLayerErrorReason, SubShapeKind, SubShapeSide,
};

// ── Public API ────────────────────────────────────────────────────────────────

/// Bool-only wrapper around [`morph_eligible`] per PRD task #4.
///
/// Returns `true` if both Stage A and Stage B pass (the edit is morphable),
/// `false` otherwise. The structured rejection [`Reason`] is discarded;
/// callers that need it for failure-mode visibility counters (PRD task #11)
/// should call [`morph_eligible`] directly.
pub fn eligible(old_brep: &BRep, new_brep: &BRep) -> bool {
    // Deref the &BRep — `BRep<'a>` is a `Copy` type alias for
    // `MorphSnapshot<'a>` and `morph_eligible` takes the snapshot by value.
    matches!(
        eligibility::morph_eligible(*old_brep, *new_brep),
        Eligibility::Eligible(_)
    )
}

/// Morph `old_mesh` to the shape described by `new_brep`, returning the
/// deformed [`reify_types::VolumeMesh`] on success.
///
/// ## API contract (task #4)
///
/// The function commits the full public signature. The engine logic is deferred:
///
/// | Path | Behaviour |
/// |------|-----------|
/// | Ineligible edit | Returns `Err(MorphFailure::Ineligible(reason))` immediately |
/// | Eligible edit | Returns `Err(MorphFailure::SolverError(...))` until PRD tasks #5–#9 land the engine |
///
/// ## Parameters
///
/// - `old_mesh` — the current tetrahedral mesh to deform.
/// - `old_brep` / `new_brep` — boundary-rep snapshots for eligibility and
///   boundary-node projection (PRD task #5).
/// - `options` — quality thresholds and fictitious-stiffness parameters;
///   see [`MorphOptions`].
///
/// ## Failure modes
///
/// See [`MorphFailure`] for the four-variant taxonomy. Only `Ineligible` is
/// produced by this skeleton; the remaining three variants are wired in PRD
/// tasks #7 and #9.
pub fn morph(
    old_mesh: &reify_types::VolumeMesh,
    old_brep: &BRep,
    new_brep: &BRep,
    options: &MorphOptions,
) -> Result<reify_types::VolumeMesh, MorphFailure> {
    let _ = old_mesh;
    let _ = options;
    // Deref — see note in `eligible()` above.
    match eligibility::morph_eligible(*old_brep, *new_brep) {
        Eligibility::Ineligible(reason) => Err(MorphFailure::Ineligible(reason)),
        Eligibility::Eligible(_correspondence_map) => Err(MorphFailure::SolverError(
            SolverErrorPayload::new(
                "engine not yet implemented (PRD docs/prds/v0_3/mesh-morphing.md tasks #5-#9)",
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::ValueCellKind;
    use reify_eval::graph::{EvaluationGraph, RealizationNodeData, ValueCellNode};
    use reify_types::{
        ContentHash, RealizationNodeId, TopologyAttributeTable, Type, Value, ValueCellId, ValueMap,
    };

    // ── Test fixture helpers (mirrored from eligibility::tests) ───────────────

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

    fn diverged_graph(base: &EvaluationGraph) -> EvaluationGraph {
        let mut g = base.clone();
        let rnid = RealizationNodeId::new("Extra", 0);
        g.realizations.insert(
            rnid.clone(),
            RealizationNodeData {
                id: rnid,
                operations: Vec::new(),
                content_hash: ContentHash::of_str("diverge"),
            },
        );
        g
    }

    fn make_brep<'a>(
        graph: &'a EvaluationGraph,
        values: &'a ValueMap,
        table: &'a TopologyAttributeTable,
    ) -> BRep<'a> {
        BRep {
            graph,
            values,
            topology_attributes: table,
            faces: &[],
            edges: &[],
            vertices: &[],
        }
    }

    fn empty_mesh() -> reify_types::VolumeMesh {
        reify_types::VolumeMesh {
            vertices: Vec::new(),
            tet_indices: Vec::new(),
            element_order: reify_types::ElementOrderTag::P1,
            normals: None,
        }
    }

    // ── Step-5: eligible() contract ───────────────────────────────────────────

    #[test]
    fn eligible_returns_true_when_morph_eligible_yields_eligible() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));
        let table = TopologyAttributeTable::default();
        let old_brep = make_brep(&old_graph, &values, &table);
        let new_brep = make_brep(&new_graph, &values, &table);
        assert!(eligible(&old_brep, &new_brep));
    }

    #[test]
    fn eligible_returns_false_on_stage_a_structural_change() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = diverged_graph(&old_graph);
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));
        let table = TopologyAttributeTable::default();
        let old_brep = make_brep(&old_graph, &values, &table);
        let new_brep = make_brep(&new_graph, &values, &table);
        assert!(!eligible(&old_brep, &new_brep));
    }

    // ── Step-7/amendment: morph() Ineligible and Eligible paths ─────────────

    #[test]
    fn morph_returns_solver_error_on_eligible_path() {
        // Verifies the Eligible arm of morph() returns SolverError (not panics)
        // until the engine lands in PRD tasks #5–#9.
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));
        let table = TopologyAttributeTable::default();
        let old_brep = make_brep(&old_graph, &values, &table);
        let new_brep = make_brep(&new_graph, &values, &table);
        let mesh = empty_mesh();
        let options = MorphOptions::default();
        let result = morph(&mesh, &old_brep, &new_brep, &options);
        assert!(
            matches!(result, Err(MorphFailure::SolverError(_))),
            "eligible path should return SolverError (unimplemented), got: {result:?}"
        );
    }

    #[test]
    fn morph_returns_ineligible_failure_on_stage_a_structural_change() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = diverged_graph(&old_graph);
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));
        let table = TopologyAttributeTable::default();
        let old_brep = make_brep(&old_graph, &values, &table);
        let new_brep = make_brep(&new_graph, &values, &table);
        let mesh = empty_mesh();
        let options = MorphOptions::default();
        let result = morph(&mesh, &old_brep, &new_brep, &options);
        assert!(matches!(
            result,
            Err(MorphFailure::Ineligible(Reason::StructuralChange))
        ));
    }

    // ── Step-31: lib re-exports make boundary module public surface accessible ─

    // Compile fence: verifies each name from the boundary module is accessible
    // from the crate root, and pins the compute_dirichlet_bcs signature.
    // Follows the `const _: fn() = || { ... }` discipline in eligibility.rs —
    // no runtime assertions, just type-check guarantees.
    const _: fn() = || {
        use crate::{
            BoundaryAssociation, NodeAttachment, ProjectionFailure, ProjectorPayload, Projector,
            compute_dirichlet_bcs,
        };
        let _fn_ref: fn(
            &reify_types::VolumeMesh,
            &BoundaryAssociation,
            &reify_eval::CorrespondenceMap,
            &dyn Projector,
        ) -> Result<Vec<(u32, [f64; 3])>, ProjectionFailure> = compute_dirichlet_bcs;
        // Type mentions for names not in _fn_ref; avoids unused-import warnings.
        let _: Option<NodeAttachment> = None;
        let _: Option<ProjectorPayload> = None;
    };

    // ── Step-18: lib re-exports make laplacian module public surface accessible ─

    // Compile fence: verifies LaplacianFailure variants and laplacian_smooth
    // are accessible from the crate root, and pins the laplacian_smooth
    // signature. Same discipline as the boundary fence above — fails to
    // compile if a re-export drops or the public signature drifts.
    const _: fn() = || {
        use crate::{LaplacianFailure, laplacian_smooth};
        let _fn_ref: fn(
            &reify_types::VolumeMesh,
            &[(u32, [f64; 3])],
            u32,
        ) -> Result<reify_types::VolumeMesh, LaplacianFailure> = laplacian_smooth;
        // Variant mentions force the enum's variant set into the fence — adding
        // or removing a variant under the same names elsewhere would still
        // require these constructors to compile.
        let _: LaplacianFailure = LaplacianFailure::InvalidNodeIndex(0u32);
        let _: LaplacianFailure =
            LaplacianFailure::UnsupportedElementOrder(reify_types::ElementOrderTag::P2);
    };
}
