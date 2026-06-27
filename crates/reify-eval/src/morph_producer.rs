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
