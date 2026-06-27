//! The mesh-morph producer hook seam (task 4744 β / PRD
//! `docs/prds/v0_6/volume-mesh-realization-and-morph-wiring.md` §4.2, D3).
//!
//! `reify-eval` **owns** this seam — the trait + the borrowing request/result
//! types live here, NOT in `reify-mesh-morph`. That direction is forced by the
//! crate graph: `reify-mesh-morph` normal-deps `reify-eval`, so `reify-eval`
//! cannot normal-dep `reify-mesh-morph` back (a cycle). The morph
//! implementation + `register_morph_producer` live in `reify-mesh-morph`
//! (mirroring how `compute_targets::register_compute_fns` is called at Engine
//! construction); the engine reaches the impl only through the
//! [`MorphProducer`] trait object installed via
//! [`Engine::register_morph_producer`][crate::Engine::register_morph_producer].
//!
//! ## Cycle-free request shape
//!
//! [`MorphRequest`] carries the *constituents* of the old/new BReps as
//! [`BRepSnapshot`]s (graph + values + topology-attribute table + handle
//! slices) rather than `reify-mesh-morph`'s own `BRep`/`MorphSnapshot` alias —
//! those constituents are all `reify-eval`/`reify-ir`/`reify-core` types the
//! engine can name. The `reify-mesh-morph` producer impl re-assembles them into
//! its `MorphSnapshot` before calling `morph_eligible` / `compose_morph`. The
//! new-BRep projection kernel is handed across as a `&dyn
//! reify_ir::GeometryKernel` (the impl wraps it in `KernelProjector`).

use crate::graph::EvaluationGraph;
use reify_ir::{
    BoundaryAssociation, GeometryHandleId, GeometryKernel, TopologyAttributeTable, ValueMap,
    VolumeMesh,
};

/// One side (old or new) of the BRep snapshot the morph pipeline needs.
///
/// Mirrors `reify_mesh_morph::eligibility::MorphSnapshot` field-for-field, but
/// is defined here so the engine can construct it without naming
/// `reify-mesh-morph`. The `reify-mesh-morph` producer impl copies these refs
/// straight into its own `MorphSnapshot` (both are `Copy` snapshots of borrowed
/// engine state).
#[derive(Debug, Clone, Copy)]
pub struct BRepSnapshot<'a> {
    /// The evaluation graph for this BRep side (Stage-A shape/parameter check).
    pub graph: &'a EvaluationGraph,
    /// The value bindings for this BRep side (Stage-A dimensional check).
    pub values: &'a ValueMap,
    /// The persistent-naming attribute table for this BRep side (Stage-B
    /// bijection construction).
    pub topology_attributes: &'a TopologyAttributeTable,
    /// Face handle slice extracted from this BRep side.
    pub faces: &'a [GeometryHandleId],
    /// Edge handle slice extracted from this BRep side.
    pub edges: &'a [GeometryHandleId],
    /// Vertex handle slice extracted from this BRep side.
    pub vertices: &'a [GeometryHandleId],
}

/// Borrowing request handed to [`MorphProducer::try_morph`].
///
/// Borrows (never clones) the source mesh, its boundary association, the
/// old/new BRep snapshots, and the new-BRep projection kernel. Per PRD OQ-2 the
/// engine owns this state for the realization's lifetime, which strictly
/// exceeds the single `try_morph` call, so a borrowing request avoids cloning
/// large meshes/kernels per parameter tick.
pub struct MorphRequest<'a> {
    /// The current (pre-morph) tetrahedral mesh to deform. Carries its
    /// task-4092 `boundary` association when produced via the attributed path.
    pub source: &'a VolumeMesh,
    /// Per-node attachment of the source mesh's surface nodes to old-BRep
    /// entities (from the task-4092 attributed VolumeMesh producer).
    pub boundary: &'a BoundaryAssociation,
    /// Snapshot of the old BRep (the shape the source mesh was meshed from).
    pub old_brep: BRepSnapshot<'a>,
    /// Snapshot of the new BRep (the post-edit shape to morph onto).
    pub new_brep: BRepSnapshot<'a>,
    /// Geometry kernel holding the **new** BRep, used to project boundary nodes
    /// onto the morphed shape (the impl wraps it in `KernelProjector`).
    pub kernel: &'a dyn GeometryKernel,
}

/// Outcome of a [`MorphProducer::try_morph`] attempt.
///
/// One variant per failure class so the dispatch decision helper (task 4744
/// step-15/16) can route the matching diagnostic counter and log behaviour
/// (info-log on quality reject) without re-deriving the failure category. The
/// non-`Ok` reason payloads are human-readable strings — the structured
/// `reify-mesh-morph` reason/verdict types cannot be named here (they live
/// across the cycle boundary), so the impl renders them to text.
#[derive(Debug)]
pub enum MorphResult {
    /// The morph succeeded; the connectivity-preserving deformed mesh.
    Ok(VolumeMesh),
    /// The edit was ineligible for morphing (structural change / bijection
    /// failure / naming-layer error). The caller remeshes.
    Ineligible(String),
    /// The mesh was morphed but rejected by the quality gate (hard/soft fail).
    /// The caller info-logs and remeshes.
    QualityReject(String),
    /// The boundary projection or elastic/Laplacian solve failed. The caller
    /// remeshes.
    SolverError(String),
}

/// The morph-producer hook installed on the [`Engine`][crate::Engine].
///
/// A single producer is installed at Engine construction by
/// `reify_mesh_morph::register_morph_producer` (mirroring
/// `compute_targets::register_compute_fns`). At the VolumeMesh realization
/// dispatch point the engine probes [`Engine::morph_producer`][crate::Engine::morph_producer];
/// if `Some` and a prior morph source exists, it builds a [`MorphRequest`] and
/// calls [`try_morph`][MorphProducer::try_morph], remeshing on any non-`Ok`
/// outcome (honest fallback).
///
/// `Send + Sync` so the boxed producer can live on the (potentially
/// shared) Engine without constraining the engine's own auto-traits beyond its
/// existing `dyn` fields.
pub trait MorphProducer: Send + Sync {
    /// Attempt to morph the source mesh in `ctx` onto the new BRep.
    ///
    /// Returns [`MorphResult::Ok`] with the deformed mesh on success, or one of
    /// the structured failure variants — every non-`Ok` outcome causes the
    /// engine to fall back to a real Gmsh remesh.
    fn try_morph(&self, ctx: MorphRequest<'_>) -> MorphResult;
}

// ── Morph-source side-table types (task 4744 β / PRD OQ-3, D6) ───────────────

/// Owned snapshot of one BRep side, captured *before* a rebuild wipes the
/// engine's live topology-attribute table.
///
/// The borrowing [`BRepSnapshot`] the morph pipeline consumes cannot outlive
/// the rebuild — it would borrow the live engine tables, which the next build
/// overwrites — so the side-table stores OWNED copies and reconstructs a
/// borrowing snapshot on demand via [`as_snapshot`][Self::as_snapshot]. Every
/// field is a `Clone` owned type; `EvaluationGraph` clones in O(1) via its
/// persistent maps, so snapshotting the old graph each tick is cheap.
///
/// Not `Clone`: `TopologyAttributeTable` is not `Clone`, and the side-table
/// never needs to duplicate a snapshot — it stores by move and reads by
/// reference.
#[derive(Debug)]
pub struct OwnedBRepSnapshot {
    /// Owned evaluation graph (Stage-A shape/parameter check).
    pub graph: EvaluationGraph,
    /// Owned value bindings (Stage-A dimensional check).
    pub values: ValueMap,
    /// Owned persistent-naming attribute table (Stage-B bijection).
    pub topology_attributes: TopologyAttributeTable,
    /// Owned face handle slice.
    pub faces: Vec<GeometryHandleId>,
    /// Owned edge handle slice.
    pub edges: Vec<GeometryHandleId>,
    /// Owned vertex handle slice.
    pub vertices: Vec<GeometryHandleId>,
}

impl OwnedBRepSnapshot {
    /// Reconstruct a borrowing [`BRepSnapshot`] over this owned snapshot — the
    /// `old_brep` side of a [`MorphRequest`] at morph time.
    pub fn as_snapshot(&self) -> BRepSnapshot<'_> {
        BRepSnapshot {
            graph: &self.graph,
            values: &self.values,
            topology_attributes: &self.topology_attributes,
            faces: &self.faces,
            edges: &self.edges,
            vertices: &self.vertices,
        }
    }
}

/// The most-recent in-memory morph source for a realization node.
///
/// Holds the source tetrahedral mesh (carrying its task-4092
/// [`reify_ir::BoundaryAssociation`] when produced via the attributed path) and
/// the [`OwnedBRepSnapshot`] of the OLD BRep the mesh was built from. Populated
/// on every VolumeMesh production and probed at the next dispatch (PRD OQ-3).
///
/// **In-memory realization cache only — never persistent** (PRD D6: the
/// persistent cache key is path-independent, but morph is path-dependent, so a
/// morphed result must not leak across the persistent boundary).
///
/// Not `Clone` (its [`OwnedBRepSnapshot`] is not `Clone`); stored by move.
#[derive(Debug)]
pub struct MorphSource {
    /// The source mesh to deform (with its task-4092 boundary association).
    pub source_mesh: VolumeMesh,
    /// Snapshot of the old BRep the source mesh was meshed from.
    pub old_brep: OwnedBRepSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use reify_core::{Diagnostic, RealizationNodeId, Severity};
    use reify_test_support::mocks::{FailingMockGeometryKernel, MockConstraintChecker};

    fn mesh_with_tets(tets: Vec<u32>) -> VolumeMesh {
        VolumeMesh {
            vertices: vec![
                0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
            ],
            tet_indices: tets,
            element_order: reify_ir::ElementOrderTag::P1,
            normals: None,
            boundary: None,
        }
    }

    fn owned_brep() -> OwnedBRepSnapshot {
        OwnedBRepSnapshot {
            graph: EvaluationGraph::default(),
            values: ValueMap::new(),
            topology_attributes: TopologyAttributeTable::default(),
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
        }
    }

    #[test]
    fn morph_source_absent_key_returns_none() {
        let engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        let rnid = RealizationNodeId::new("Part", 0);
        assert!(
            engine.morph_source(&rnid).is_none(),
            "an unstored realization key must read back None"
        );
    }

    #[test]
    fn morph_source_stores_and_reads_back_most_recent() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        let rnid = RealizationNodeId::new("Part", 0);

        // Store v1, read it back.
        engine.store_morph_source(
            rnid.clone(),
            MorphSource {
                source_mesh: mesh_with_tets(vec![0, 1, 2, 3]),
                old_brep: owned_brep(),
            },
        );
        assert_eq!(
            engine
                .morph_source(&rnid)
                .expect("v1 stored")
                .source_mesh
                .tet_indices,
            vec![0, 1, 2, 3]
        );

        // Store v2 for the SAME key — most-recent wins (overwrite).
        engine.store_morph_source(
            rnid.clone(),
            MorphSource {
                source_mesh: mesh_with_tets(vec![4, 5, 6, 7]),
                old_brep: owned_brep(),
            },
        );
        assert_eq!(
            engine
                .morph_source(&rnid)
                .expect("v2 stored")
                .source_mesh
                .tet_indices,
            vec![4, 5, 6, 7],
            "store for an existing realization key must overwrite (most-recent wins)"
        );

        // A different realization key is still absent.
        let other = RealizationNodeId::new("Part", 1);
        assert!(engine.morph_source(&other).is_none());
    }

    #[test]
    fn owned_brep_snapshot_borrows_as_brep_snapshot() {
        // The owned snapshot reconstructs a borrowing BRepSnapshot for the
        // morph pipeline (so morph_eligible can run after the live topology
        // table has been wiped by the rebuild).
        let owned = owned_brep();
        let snap: BRepSnapshot<'_> = owned.as_snapshot();
        assert!(snap.faces.is_empty());
        assert!(snap.edges.is_empty());
        assert!(snap.vertices.is_empty());
    }

    // ── step-15: morph-or-remesh decision helper (PRD §4.3 decision tree) ────
    //
    // The helper `decide_morph_or_remesh` + the `MorphDecision` enum land in
    // step-16 (GREEN); these tests are RED until then. They pin the decision
    // ROUTING in isolation from the engine_build dispatch wiring: a mock
    // producer feeds each `MorphResult` variant through the helper and the test
    // asserts the resulting `MorphDecision` + the engine-level diagnostic
    // surfaced (info-log on quality-reject, warning on solver-error, silent on
    // ineligible). The process-global morph counters are recorded INSIDE the
    // producer (steps 8/10), not the helper, so the helper's only side effect
    // is the user-facing build diagnostic.

    /// Which `MorphResult` variant the mock producer returns.
    enum MockOutcome {
        Ok,
        Ineligible,
        QualityReject,
        SolverError,
    }

    /// A configurable mock [`MorphProducer`]. On the `Ok` arm it echoes the
    /// source connectivity (same `tet_indices`) with one perturbed vertex, so
    /// the decision test can assert the morphed mesh flowed back with its
    /// topology preserved.
    struct DecisionMockProducer {
        outcome: MockOutcome,
    }

    impl MorphProducer for DecisionMockProducer {
        fn try_morph(&self, ctx: MorphRequest<'_>) -> MorphResult {
            match self.outcome {
                MockOutcome::Ok => {
                    let mut morphed = mesh_with_tets(ctx.source.tet_indices.clone());
                    morphed.vertices[0] += 1.0; // mark as deformed
                    MorphResult::Ok(morphed)
                }
                MockOutcome::Ineligible => MorphResult::Ineligible("count-mismatch".to_string()),
                MockOutcome::QualityReject => {
                    MorphResult::QualityReject("min-scaled-jacobian".to_string())
                }
                MockOutcome::SolverError => {
                    MorphResult::SolverError("singular-system".to_string())
                }
            }
        }
    }

    /// Build a [`MorphSource`] whose source mesh carries a (non-`None`)
    /// boundary association — the precondition the helper checks before it can
    /// build a [`MorphRequest`] (only the task-4092 attributed path threads a
    /// boundary; the plain path leaves it `None`).
    fn source_with_boundary(tets: Vec<u32>) -> MorphSource {
        let mut mesh = mesh_with_tets(tets);
        mesh.boundary = Some(BoundaryAssociation::default());
        MorphSource {
            source_mesh: mesh,
            old_brep: owned_brep(),
        }
    }

    /// Run `decide_morph_or_remesh` with a fresh new-BRep snapshot + stub
    /// kernel, returning the decision and any engine diagnostics emitted. The
    /// stub kernel is never actually projected through (the mock producer does
    /// not touch it), so a `FailingMockGeometryKernel` suffices.
    fn run_decision(
        producer: Option<&dyn MorphProducer>,
        source: Option<&MorphSource>,
    ) -> (MorphDecision, Vec<Diagnostic>) {
        let kernel = FailingMockGeometryKernel;
        let graph = EvaluationGraph::default();
        let values = ValueMap::new();
        let table = TopologyAttributeTable::default();
        let new_brep = BRepSnapshot {
            graph: &graph,
            values: &values,
            topology_attributes: &table,
            faces: &[],
            edges: &[],
            vertices: &[],
        };
        let rnid = RealizationNodeId::new("Part", 0);
        let mut diagnostics = Vec::new();
        let decision =
            decide_morph_or_remesh(producer, source, new_brep, &kernel, &rnid, &mut diagnostics);
        (decision, diagnostics)
    }

    #[test]
    fn decide_no_producer_registered_remeshes() {
        let source = source_with_boundary(vec![0, 1, 2, 3]);
        let (decision, diags) = run_decision(None, Some(&source));
        assert!(
            matches!(decision, MorphDecision::Remesh),
            "no producer registered → Remesh"
        );
        assert!(
            diags.is_empty(),
            "no diagnostic when there is nothing to morph"
        );
    }

    #[test]
    fn decide_producer_but_no_source_remeshes() {
        let producer = DecisionMockProducer {
            outcome: MockOutcome::Ok,
        };
        let (decision, diags) = run_decision(Some(&producer), None);
        assert!(
            matches!(decision, MorphDecision::Remesh),
            "producer present but no MorphSource → Remesh"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn decide_source_without_boundary_remeshes() {
        // A source mesh produced by the PLAIN (non-attributed) path carries
        // boundary: None — it cannot be projected onto the new BRep, so the
        // decision must remesh even with a producer + source present.
        let producer = DecisionMockProducer {
            outcome: MockOutcome::Ok,
        };
        let source = MorphSource {
            source_mesh: mesh_with_tets(vec![0, 1, 2, 3]), // boundary: None
            old_brep: owned_brep(),
        };
        let (decision, diags) = run_decision(Some(&producer), Some(&source));
        assert!(
            matches!(decision, MorphDecision::Remesh),
            "a source mesh with no boundary attribution cannot be morphed"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn decide_ok_morph_returns_morphed_connectivity_preserved() {
        let producer = DecisionMockProducer {
            outcome: MockOutcome::Ok,
        };
        let source = source_with_boundary(vec![0, 1, 2, 3]);
        let (decision, diags) = run_decision(Some(&producer), Some(&source));
        match decision {
            MorphDecision::Morphed(mesh) => assert_eq!(
                mesh.tet_indices,
                vec![0, 1, 2, 3],
                "morph preserves the source connectivity (same tet_indices)"
            ),
            MorphDecision::Remesh => {
                panic!("producer + source + try_morph Ok → Morphed, got Remesh")
            }
        }
        assert!(
            diags.is_empty(),
            "a successful morph emits no engine-level diagnostic"
        );
    }

    #[test]
    fn decide_ineligible_remeshes_silently() {
        let producer = DecisionMockProducer {
            outcome: MockOutcome::Ineligible,
        };
        let source = source_with_boundary(vec![0, 1, 2, 3]);
        let (decision, diags) = run_decision(Some(&producer), Some(&source));
        assert!(matches!(decision, MorphDecision::Remesh));
        // Ineligible is the common, expected edit class (a structural change);
        // the producer already recorded the process-global counter, so the
        // engine layer stays silent (no per-tick user-facing diagnostic spam).
        assert!(
            diags.is_empty(),
            "ineligible must remesh without an engine diagnostic"
        );
    }

    #[test]
    fn decide_quality_reject_remeshes_with_info_log() {
        let producer = DecisionMockProducer {
            outcome: MockOutcome::QualityReject,
        };
        let source = source_with_boundary(vec![0, 1, 2, 3]);
        let (decision, diags) = run_decision(Some(&producer), Some(&source));
        assert!(matches!(decision, MorphDecision::Remesh));
        assert_eq!(
            diags.len(),
            1,
            "quality reject surfaces exactly one engine diagnostic"
        );
        assert_eq!(
            diags[0].severity,
            Severity::Info,
            "a quality reject is an INFO log (the morph ran but the gate rejected it)"
        );
    }

    #[test]
    fn decide_solver_error_remeshes_with_warning() {
        let producer = DecisionMockProducer {
            outcome: MockOutcome::SolverError,
        };
        let source = source_with_boundary(vec![0, 1, 2, 3]);
        let (decision, diags) = run_decision(Some(&producer), Some(&source));
        assert!(matches!(decision, MorphDecision::Remesh));
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            Severity::Warning,
            "a solver error WARNs (an unexpected projection/solve failure)"
        );
    }
}
