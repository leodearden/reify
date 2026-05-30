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
//! ## PRD task #7 — linear-elasticity morph — elasticity module
//!
//! The [`elasticity`] module implements the primary morph algorithm: treat
//! the source mesh as a fictitious-elastic continuum, prescribe surface-
//! node displacements as Dirichlet BCs, and solve the linear-elastostatic
//! BVP `K · u = 0` for interior-node displacements. Composes four
//! `reify-solver-elastic` primitives (`element_stiffness`,
//! `assemble_global_stiffness`, `apply_dirichlet_row_elimination`,
//! `solve_cg`); the output mesh is `vertices_old + u`. Engine wiring (PRD
//! task #10) selects between this morph and the Laplacian quick-pass.
//!
//! ## PRD task #9 — quality check — quality module
//!
//! The [`quality`] module implements the two-tier quality-check pass that
//! runs after the morph engine produces a deformed mesh. Returns
//! [`QualityVerdict::Pass`], [`QualityVerdict::HardFail`] (element
//! inversion), or [`QualityVerdict::SoftFail`] (metric threshold breach).
//! Engine wiring (PRD task #10) maps hard/soft fail to remesh fallback.
//!
//! ## PRD task #13 — quality-threshold calibration — tests/calibration.rs
//!
//! The three quality-floor knobs on [`MorphOptions`]
//! (`quality_floor_min_scaled_jacobian`, `quality_floor_pct_below_025`,
//! `quality_aspect_ratio_factor_max`) are calibrated against two
//! procedural parametric fixtures shipped as a regression-guard suite in
//! `tests/calibration.rs`:
//!
//! - **plate hole-diameter sweep** — polar-radial grid with hex-to-6-tet
//!   decomposition; intrinsic min-scaled-J ≈ 0.022 at small steps.
//! - **bracket fillet-radius sweep** — L-bracket with parametric inner
//!   fillet; the discriminating fixture in calibration coverage — exercises
//!   both well-conditioned and near-degenerate fillet radii so the
//!   calibrated thresholds are stressed across the parameter range.
//!   Pinned by an explicit Pass/Reject verdict-mix assertion at the end of
//!   the test.
//!
//! The calibration rule: morph is rejected only when a from-scratch remesh
//! is *materially better* (> 20 % improvement on the relevant metric). This
//! is encoded as `from_scratch > MATERIALITY_FACTOR * morph` for
//! higher-is-better metrics (min scaled J) and
//! `from_scratch_max_ar_factor > MATERIALITY_FACTOR` for lower-is-better
//! metrics, where `from_scratch_max_ar_factor` is the true
//! `max(morphed_AR / from_scratch_AR)` ratio computed in
//! `tests/calibration/sweep.rs::extract_metrics` and exposed via the
//! `SweepReport` field of the same name. The live predicates live in
//! `tests/calibration/sweep.rs::sj_materially_better` and
//! `tests/calibration/sweep.rs::ar_materially_better`; the canonical
//! materiality constant lives in
//! `tests/calibration/sweep.rs::MATERIALITY_FACTOR`.
//!
//! Calibration was performed against the [`StiffnessRule::InverseVolume`]
//! production default (PRD task #8 / task 2945, shipped on main).
pub mod boundary;
pub mod diagnostics;
pub mod elasticity;
pub mod eligibility;
pub mod laplacian;
pub mod options;
pub mod quality;
pub mod stats;
pub mod types;

pub use stats::{MorphStats, record_morph_attempt, record_rejection, record_remesh, snapshot};
pub use boundary::{
    BoundaryAssociation, NodeAttachment, ProjectionFailure, Projector, ProjectorPayload,
    compute_dirichlet_bcs,
};
pub use elasticity::{ElasticityFailure, elasticity_morph, elasticity_morph_with_cg_opts};
pub use eligibility::{Eligibility, MorphSnapshot, Reason, morph_eligible};
pub use laplacian::{LaplacianFailure, laplacian_smooth};
pub use options::{MorphFailure, MorphOptions, StiffnessRule};
pub use quality::{QualityVerdict, quality_check};
pub use types::{BRep, InversionDetails, SoftFailDetails, SolverErrorPayload};

/// Re-exported so consumers can pattern-match `Reason::BijectionFailure(_)`
/// without depending on `reify-eval` directly.
pub use reify_eval::{
    BijectionFailure, CorrespondenceMap, NamingLayerErrorReason, SubShapeKind, SubShapeSide,
};
/// Re-exported so consumers of [`elasticity_morph_with_cg_opts`] can construct
/// `CgSolverOptions` without depending on `reify-solver-elastic` directly.
pub use reify_solver_elastic::CgSolverOptions;

// ── Public API ────────────────────────────────────────────────────────────────

/// Bool-only wrapper around [`morph_eligible`] per PRD task #4.
///
/// Returns `true` if both Stage A and Stage B pass (the edit is morphable),
/// `false` otherwise. The structured rejection [`Reason`] is discarded;
/// callers that need it for failure-mode visibility counters (PRD task #11)
/// should call [`morph_eligible`] directly.
// G-allow: mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #2947 (mesh-morph VolumeMesh realization wiring) / CN-contract §8 task κ #3429
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
    old_mesh: &reify_ir::VolumeMesh,
    old_brep: BRep,
    new_brep: BRep,
    options: &MorphOptions,
) -> Result<reify_ir::VolumeMesh, MorphFailure> {
    let _ = old_mesh;
    let _ = options;
    match eligibility::morph_eligible(old_brep, new_brep) {
        Eligibility::Ineligible(reason) => Err(MorphFailure::Ineligible(reason)),
        Eligibility::Eligible(_correspondence_map) => {
            Err(MorphFailure::SolverError(SolverErrorPayload::new(
                "engine not yet implemented (PRD docs/prds/v0_3/mesh-morphing.md tasks #5-#9)",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::ValueCellKind;
    use reify_eval::graph::{EvaluationGraph, RealizationNodeData, ValueCellNode};
    use reify_core::{ContentHash, RealizationNodeId, Type, ValueCellId};
    use reify_ir::{CapKind, FeatureId, GeometryHandleId, ReprKind, Role, TopologyAttribute, TopologyAttributeTable, Value, ValueMap};

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
            RealizationNodeData { geometry_cell: None,
                id: rnid,
                operations: Vec::new(),
                content_hash: ContentHash::of_str("diverge"),
                produced_repr: ReprKind::BRep,
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

    fn empty_mesh() -> reify_ir::VolumeMesh {
        reify_ir::VolumeMesh {
            vertices: Vec::new(),
            tet_indices: Vec::new(),
            element_order: reify_ir::ElementOrderTag::P1,
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
            BoundaryAssociation, NodeAttachment, ProjectionFailure, Projector, ProjectorPayload,
            compute_dirichlet_bcs,
        };
        #[allow(clippy::type_complexity)]
        // pinning the full public signature is the point of the fence
        let _fn_ref: fn(
            &reify_ir::VolumeMesh,
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
        #[allow(clippy::type_complexity)]
        // pinning the full public signature is the point of the fence
        let _fn_ref: fn(
            &reify_ir::VolumeMesh,
            &[(u32, [f64; 3])],
            u32,
        ) -> Result<reify_ir::VolumeMesh, LaplacianFailure> = laplacian_smooth;
        // Variant mentions force the enum's variant set into the fence — adding
        // or removing a variant under the same names elsewhere would still
        // require these constructors to compile.
        let _: LaplacianFailure = LaplacianFailure::InvalidNodeIndex(0u32);
        let _: LaplacianFailure =
            LaplacianFailure::UnsupportedElementOrder(reify_ir::ElementOrderTag::P2);
    };

    // ── Step-17: lib re-exports make elasticity module public surface accessible ─

    // Compile fence: verifies ElasticityFailure variants and elasticity_morph
    // are accessible from the crate root, and pins the elasticity_morph
    // signature. Same discipline as the boundary, laplacian, and quality
    // fences above — fails to compile if a re-export drops, the public
    // signature drifts, or a variant is renamed.
    const _: fn() = || {
        use crate::{
            CgSolverOptions, ElasticityFailure, elasticity_morph, elasticity_morph_with_cg_opts,
        };
        #[allow(clippy::type_complexity)]
        // pinning the full public signature is the point of the fence
        let _fn_ref: fn(
            &reify_ir::VolumeMesh,
            &[(u32, [f64; 3])],
            &MorphOptions,
        ) -> Result<reify_ir::VolumeMesh, ElasticityFailure> = elasticity_morph;
        #[allow(clippy::type_complexity)]
        // pinning the full public signature is the point of the fence
        let _fn_with_opts: fn(
            &reify_ir::VolumeMesh,
            &[(u32, [f64; 3])],
            &MorphOptions,
            CgSolverOptions,
        )
            -> Result<reify_ir::VolumeMesh, ElasticityFailure> = elasticity_morph_with_cg_opts;
        let _ = _fn_with_opts;
        // Variant mentions force the enum's variant set into the fence —
        // adding or removing a variant under the same names elsewhere would
        // still require these constructors to compile.
        let _: ElasticityFailure = ElasticityFailure::InvalidNodeIndex(0u32);
        let _: ElasticityFailure =
            ElasticityFailure::UnsupportedElementOrder(reify_ir::ElementOrderTag::P2);
        let _: ElasticityFailure = ElasticityFailure::SolverNotConverged { iterations: 0 };
        let _: ElasticityFailure = ElasticityFailure::InvalidTetIndex(0u32);
        let _: ElasticityFailure = ElasticityFailure::NoElementsForPrescribedDisplacements;
        let _: ElasticityFailure = ElasticityFailure::MalformedTetIndices { len: 0 };
    };

    // ── Step-12: lib re-exports make quality module public surface accessible ──

    // Compile fence: verifies quality_check and QualityVerdict are accessible
    // from the crate root, pins the quality_check signature, and exhaustively
    // mentions all three QualityVerdict variant constructors.
    // Same discipline as the boundary and laplacian fences above.
    const _: fn() = || {
        use crate::{QualityVerdict, quality_check};
        let _fn_ref: fn(
            &reify_ir::VolumeMesh,
            &reify_ir::VolumeMesh,
            &MorphOptions,
        ) -> QualityVerdict = quality_check;
        // Variant mentions — exhaustive constructor coverage:
        let _: QualityVerdict = QualityVerdict::Pass;
        let _: QualityVerdict = QualityVerdict::HardFail(crate::types::InversionDetails {
            element_index: 0,
            jacobian: -0.5,
        });
        let _: QualityVerdict = QualityVerdict::SoftFail(crate::types::SoftFailDetails {
            min_scaled_jacobian: None,
            pct_below_025: None,
            max_aspect_ratio_factor: None,
            degenerate_morphed_element: None,
        });
    };

    // ── task 2945: lib re-export + variant fence for StiffnessRule ───────────

    // Compile fence: verifies StiffnessRule and all three variants are accessible
    // from the crate root. Adding, removing, or renaming a variant or dropping the
    // re-export breaks compilation immediately.
    const _: fn() = || {
        use crate::StiffnessRule;
        let _: StiffnessRule = StiffnessRule::Uniform;
        let _: StiffnessRule = StiffnessRule::InverseVolume;
        let _: StiffnessRule = StiffnessRule::InverseEdgeLengthSquared;
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
            &reify_ir::VolumeMesh,
            BRep,
            BRep,
            &MorphOptions,
        ) -> Result<reify_ir::VolumeMesh, MorphFailure> = morph;
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
            matches!(
                result,
                Err(MorphFailure::Ineligible(Reason::BijectionFailure(_)))
            ),
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
