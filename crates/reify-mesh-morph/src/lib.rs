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
//!
//! ## PRD task #9 — quality check — quality module
//!
//! The [`quality`] module implements the two-tier quality-check pass that
//! runs after the morph engine produces a deformed mesh. Returns
//! [`QualityVerdict::Pass`], [`QualityVerdict::HardFail`] (element
//! inversion), or [`QualityVerdict::SoftFail`] (metric threshold breach).
//! Engine wiring (PRD task #10) maps hard/soft fail to remesh fallback.

pub mod boundary;
pub mod eligibility;
pub mod laplacian;
pub mod options;
pub mod quality;
pub mod types;

pub use boundary::{
    BoundaryAssociation, NodeAttachment, ProjectionFailure, ProjectorPayload, Projector,
    compute_dirichlet_bcs,
};
pub use eligibility::{Eligibility, MorphSnapshot, Reason, morph_eligible};
pub use laplacian::{LaplacianFailure, laplacian_smooth};
pub use options::{MorphFailure, MorphOptions};
pub use quality::{QualityVerdict, quality_check};
pub use types::{BRep, InversionDetails, SoftFailDetails, SolverErrorPayload};

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
pub fn eligible(old_brep: BRep, new_brep: BRep) -> bool {
    // `BRep` is `Copy` (alias for `MorphSnapshot<'a>`); pass by value matches
    // `morph_eligible`'s signature directly.
    matches!(
        eligibility::morph_eligible(old_brep, new_brep),
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
    old_brep: BRep,
    new_brep: BRep,
    options: &MorphOptions,
) -> Result<reify_types::VolumeMesh, MorphFailure> {
    let _ = old_mesh;
    let _ = options;
    match eligibility::morph_eligible(old_brep, new_brep) {
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
        CapKind, ContentHash, FeatureId, GeometryHandleId, RealizationNodeId, Role,
        TopologyAttribute, TopologyAttributeTable, Type, Value, ValueCellId, ValueMap,
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

    // ── Stage-B fixture helpers (mirrored from eligibility::tests) ────────────

    fn h(n: u64) -> GeometryHandleId {
        GeometryHandleId(n)
    }

    fn feat() -> FeatureId {
        FeatureId::new("Feature#realization[0]")
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
        assert!(eligible(old_brep, new_brep));
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
        assert!(!eligible(old_brep, new_brep));
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
        let result = morph(&mesh, old_brep, new_brep, &options);
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
        let result = morph(&mesh, old_brep, new_brep, &options);
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
        #[allow(clippy::type_complexity)] // pinning the full public signature is the point of the fence
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
        #[allow(clippy::type_complexity)] // pinning the full public signature is the point of the fence
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

    // ── Step-12: lib re-exports make quality module public surface accessible ──

    // Compile fence: verifies quality_check and QualityVerdict are accessible
    // from the crate root, pins the quality_check signature, and exhaustively
    // mentions all three QualityVerdict variant constructors.
    // Same discipline as the boundary and laplacian fences above.
    const _: fn() = || {
        use crate::{QualityVerdict, quality_check};
        let _fn_ref: fn(
            &reify_types::VolumeMesh,
            &reify_types::VolumeMesh,
            &MorphOptions,
        ) -> QualityVerdict = quality_check;
        // Variant mentions — exhaustive constructor coverage:
        let _: QualityVerdict = QualityVerdict::Pass;
        let _: QualityVerdict =
            QualityVerdict::HardFail(crate::types::InversionDetails {
                element_index: 0,
                jacobian: -0.5,
            });
        let _: QualityVerdict =
            QualityVerdict::SoftFail(crate::types::SoftFailDetails {
                min_scaled_jacobian: None,
                pct_below_025: None,
                max_aspect_ratio_increase: None,
                degenerate_morphed_element: None,
            });
    };

    // ── Step 1 (task 3153): pin the by-value `eligible` signature ────────────

    // Compile fence: fails to compile until `eligible` takes `BRep` by value.
    // Mirrors the boundary/laplacian/quality fence discipline above.
    #[allow(unused)]
    const _: fn() = || {
        let _fn_ref: fn(BRep, BRep) -> bool = eligible;
        let _ = _fn_ref;
    };

    // ── Step 3 (task 3153): pin the by-value `morph` signature ───────────────

    // Compile fence: fails to compile until `morph` takes `BRep` by value.
    // `old_mesh` and `options` remain `&`-bound (not `Copy`).
    #[allow(unused)]
    const _: fn() = || {
        let _fn_ref: fn(
            &reify_types::VolumeMesh,
            BRep,
            BRep,
            &MorphOptions,
        ) -> Result<reify_types::VolumeMesh, MorphFailure> = morph;
        let _ = _fn_ref;
    };

    // ── Steps 1-2 (task 3142): Stage-B regression guards ─────────────────────

    #[test]
    fn morph_returns_ineligible_bijection_failure_on_stage_b_count_mismatch() {
        // Regression guard: morph() must project Stage-B CountMismatch into
        // MorphFailure::Ineligible(Reason::BijectionFailure(_)), not SolverError
        // or panic.
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));

        // old: 1 face with Cap(Top); new: 2 faces Cap(Top)+Cap(Bottom).
        // Stage A passes (identical graphs); Stage B rejects on CountMismatch.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0));

        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0));
        new_table.record(h(21), attr(Role::Cap(CapKind::Bottom), 1));

        let old_brep = BRep {
            graph: &old_graph,
            values: &values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_brep = BRep {
            graph: &new_graph,
            values: &values,
            topology_attributes: &new_table,
            faces: &[h(20), h(21)],
            edges: &[],
            vertices: &[],
        };

        let mesh = empty_mesh();
        let options = MorphOptions::default();
        let result = morph(&mesh, old_brep, new_brep, &options);
        assert!(
            matches!(result, Err(MorphFailure::Ineligible(Reason::BijectionFailure(_)))),
            "Stage-B CountMismatch should project to MorphFailure::Ineligible(BijectionFailure), got: {result:?}"
        );
    }

    #[test]
    fn morph_returns_ineligible_naming_layer_error_on_stage_b_imported_geometry() {
        // Regression guard: morph() must project Stage-B NamingLayerError::Imported
        // into MorphFailure::Ineligible(Reason::NamingLayerError {..}), not into
        // Reason::BijectionFailure or SolverError.
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));

        // Empty tables + non-empty face slices → Stage B surfaces
        // BijectionFailure::NamingLayerError { kind: Face, reason: Imported },
        // which morph_eligible projects to top-level Reason::NamingLayerError.
        let old_table = TopologyAttributeTable::default();
        let new_table = TopologyAttributeTable::default();

        let old_brep = BRep {
            graph: &old_graph,
            values: &values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_brep = BRep {
            graph: &new_graph,
            values: &values,
            topology_attributes: &new_table,
            faces: &[h(20)],
            edges: &[],
            vertices: &[],
        };

        let mesh = empty_mesh();
        let options = MorphOptions::default();
        let result = morph(&mesh, old_brep, new_brep, &options);
        assert!(
            matches!(
                result,
                Err(MorphFailure::Ineligible(Reason::NamingLayerError {
                    kind: SubShapeKind::Face,
                    reason: NamingLayerErrorReason::Imported,
                }))
            ),
            "Stage-B NamingLayerError::Imported should project to \
             MorphFailure::Ineligible(Reason::NamingLayerError), got: {result:?}"
        );
    }
}
