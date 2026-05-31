//! Boundary-node correspondence and closest-point projection.
//!
//! Implements task #5 of the mesh-morphing PRD
//! (`docs/prds/v0_3/mesh-morphing.md`): the surface-node → Dirichlet-BC
//! translation step that gates the elasticity morph (PRD task #7).
//!
//! ## Critical correctness anchor (PRD)
//!
//! > "corner / edge nodes must project onto the *mapped counterpart* face,
//! > not the closest face globally. Without per-face correspondence, nodes near
//! > corners can 'jump' across to the wrong face when the geometry deforms —
//! > producing an invalid morph that no quality check would catch cleanly."
//!
//! The design enforces this structurally: the projection target is *always*
//! derived from the per-node B-rep attachment via [`CorrespondenceMap`] lookup.
//! There is no closest-face fallback.

use reify_eval::{CorrespondenceMap, SubShapeKind};
use reify_ir::{GeometryHandleId, VolumeMesh};

// ── NodeAttachment / BoundaryAssociation ──────────────────────────────────────
//
// These two types live in `reify-types` (see `reify_types::boundary_attachment`)
// so adapter crates (e.g. `reify-kernel-gmsh`) can emit a `BoundaryAssociation`
// without taking a transitive dep on `reify-mesh-morph` → `reify-eval`, which
// would form a Cargo cycle through `reify-eval` → `reify-solver-elastic` →
// `reify-kernel-gmsh`. Re-exported here so existing consumers that import via
// `reify_mesh_morph::boundary::{NodeAttachment, BoundaryAssociation}` keep
// working unchanged.
pub use reify_ir::{BoundaryAssociation, NodeAttachment};

// ── ProjectorPayload ──────────────────────────────────────────────────────────

/// Opaque error payload from a [`Projector`] call.
///
/// Mirrors [`crate::SolverErrorPayload`] so future tasks that wire a real
/// OCCT-backed `Projector` can add structured kernel-error fields without
/// breaking existing `ProjectionFailure::Projector(payload)` match arms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectorPayload {
    message: String,
}

impl ProjectorPayload {
    /// Construct a payload from an error message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The error message text.
    pub fn message(&self) -> &str {
        &self.message
    }
}

// ── ProjectionFailure ─────────────────────────────────────────────────────────

/// Failure modes from [`compute_dirichlet_bcs`].
///
/// `MissingCorrespondence` is the load-bearing diagnostic — it surfaces:
/// - The v0.2 vertex-attached-node case (since
///   [`CorrespondenceMap::vertex_to_vertex`] is always empty in v0.2).
/// - Any future Stage-B-passes-but-CorrespondenceMap-incomplete edge case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionFailure {
    /// The correspondence map has no entry for `old_handle` of the given kind.
    ///
    /// In v0.2, `SubShapeKind::Vertex` always triggers this variant because
    /// [`CorrespondenceMap::vertex_to_vertex`] is structurally always-empty
    /// (see `reify_eval::morph_stage_b` doc-comment on `vertex_to_vertex`).
    MissingCorrespondence {
        kind: SubShapeKind,
        old_handle: GeometryHandleId,
    },
    /// The node index recorded in [`BoundaryAssociation`] is out of range for
    /// the provided `old_mesh.vertices` slice (i.e. `node_idx * 3 + 2 >=
    /// old_mesh.vertices.len()`).
    InvalidNodeIndex(u32),
    /// The [`Projector`] returned an error (e.g. OCCT BRepExtrema failure).
    Projector(ProjectorPayload),
}

// ── Projector trait ───────────────────────────────────────────────────────────

/// Dependency-injected closest-point projector.
///
/// Kept as a trait so `boundary.rs` is testable in pure Rust without an OCCT
/// build — mirrors the discipline of `reify_eval::morph_stage_b` where callers
/// pre-extract handle slices and pass them in. The real OCCT-backed
/// implementation is a follow-on task (accompanying PRD task #10 or PRD task
/// #7 elasticity-morph integration).
///
/// All methods take `&self` (no `&mut self`) so `&dyn Projector` is shareable
/// across the per-node loop without locking discipline.
pub trait Projector {
    /// Compute the closest point on the given face to `point`.
    fn project_onto_face(
        &self,
        face: GeometryHandleId,
        point: [f64; 3],
    ) -> Result<[f64; 3], ProjectorPayload>;

    /// Compute the closest point on the given edge to `point`.
    fn project_onto_edge(
        &self,
        edge: GeometryHandleId,
        point: [f64; 3],
    ) -> Result<[f64; 3], ProjectorPayload>;

    /// Return the exact position of the given vertex.
    ///
    /// The old node position is intentionally not passed — vertex projection is
    /// a snap to the new vertex's exact coordinates, not a closest-point
    /// computation.
    fn vertex_position(&self, vertex: GeometryHandleId) -> Result<[f64; 3], ProjectorPayload>;
}

// ── compute_dirichlet_bcs ─────────────────────────────────────────────────────

/// Translate surface-node B-rep attachments into Dirichlet boundary conditions.
///
/// For each `(node_index, attachment)` in `boundary` (iterated in ascending
/// node-index order per [`BoundaryAssociation`]'s `BTreeMap` discipline):
///
/// 1. Reads the node's current position from `old_mesh.vertices` (f32 → f64
///    widening at this single boundary — all downstream FEA arithmetic is f64).
/// 2. Looks up the mapped new B-rep entity via `correspondence`.
/// 3. Invokes `projector` to compute the prescribed position on the new shape.
/// 4. Accumulates `(node_index, prescribed_position)` into the result.
///
/// ## Failure
///
/// Returns the first [`ProjectionFailure`] encountered:
/// - `MissingCorrespondence` — no entry in `correspondence` for the attachment's
///   old handle. This is deterministic in v0.2 for `OnVertex` nodes because
///   [`CorrespondenceMap::vertex_to_vertex`] is structurally always-empty.
/// - `InvalidNodeIndex` — node index is out of range for `old_mesh.vertices`.
/// - `Projector` — the kernel's closest-point computation failed.
///
/// ## PRD invariant
///
/// The projection target is *always* the mapped counterpart of the node's
/// attached B-rep entity — never the globally-closest entity. See the critical
/// correctness anchor in this module's doc-comment and the regression-guard
/// test `compute_dirichlet_bcs_corner_node_projects_onto_attached_face_not_globally_closest_face`.
// G-allow: mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #2947 (mesh-morph VolumeMesh realization wiring) / CN-contract §8 task κ #3429
pub fn compute_dirichlet_bcs(
    old_mesh: &VolumeMesh,
    boundary: &BoundaryAssociation,
    correspondence: &CorrespondenceMap,
    projector: &dyn Projector,
) -> Result<Vec<(u32, [f64; 3])>, ProjectionFailure> {
    let mut result = Vec::with_capacity(boundary.len());

    for (node_idx, attachment) in boundary.iter() {
        // vertex_f64 widens f32→f64; downstream FEA arithmetic is f64.
        let old_position = old_mesh
            .vertex_f64(node_idx)
            .ok_or(ProjectionFailure::InvalidNodeIndex(node_idx))?;

        match attachment {
            // PRD invariant: see test
            // compute_dirichlet_bcs_corner_node_projects_onto_attached_face_not_globally_closest_face
            NodeAttachment::OnFace(old_handle) => {
                let new_handle = correspondence
                    .face_to_face
                    .get(&old_handle)
                    .copied()
                    .ok_or(ProjectionFailure::MissingCorrespondence {
                        kind: SubShapeKind::Face,
                        old_handle,
                    })?;
                let prescribed = projector
                    .project_onto_face(new_handle, old_position)
                    .map_err(ProjectionFailure::Projector)?;
                result.push((node_idx, prescribed));
            }
            NodeAttachment::OnEdge(old_handle) => {
                let new_handle = correspondence
                    .edge_to_edge
                    .get(&old_handle)
                    .copied()
                    .ok_or(ProjectionFailure::MissingCorrespondence {
                        kind: SubShapeKind::Edge,
                        old_handle,
                    })?;
                let prescribed = projector
                    .project_onto_edge(new_handle, old_position)
                    .map_err(ProjectionFailure::Projector)?;
                result.push((node_idx, prescribed));
            }
            NodeAttachment::OnVertex(old_handle) => {
                let new_handle = correspondence
                    .vertex_to_vertex
                    .get(&old_handle)
                    .copied()
                    .ok_or(ProjectionFailure::MissingCorrespondence {
                        kind: SubShapeKind::Vertex,
                        old_handle,
                    })?;
                // Old position intentionally not passed — vertex is a snap.
                let prescribed = projector
                    .vertex_position(new_handle)
                    .map_err(ProjectionFailure::Projector)?;
                result.push((node_idx, prescribed));
            }
        }
    }

    Ok(result)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use reify_eval::CorrespondenceMap;
    use reify_ir::{ElementOrderTag, GeometryHandleId, VolumeMesh};

    use super::*;

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Shorthand to create a `GeometryHandleId` from a `u64`.
    fn h(id: u64) -> GeometryHandleId {
        GeometryHandleId(id)
    }

    /// An empty `VolumeMesh` (no vertices, no tets).
    fn empty_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: Vec::new(),
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    /// A `VolumeMesh` with `n` nodes at given flat-XYZ vertices.
    fn mesh_with_vertices(flat_xyz: Vec<f32>) -> VolumeMesh {
        VolumeMesh {
            vertices: flat_xyz,
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    // ── RecordingProjector ────────────────────────────────────────────────────

    /// Recorded call to a `Projector` method.
    #[derive(Debug, Clone, PartialEq)]
    enum ProjectorCall {
        Face {
            face: GeometryHandleId,
            point: [f64; 3],
        },
        Edge {
            edge: GeometryHandleId,
            point: [f64; 3],
        },
        Vertex {
            vertex: GeometryHandleId,
        },
    }

    /// A test double for `Projector` that records calls and returns canned
    /// responses (or errors).
    struct RecordingProjector {
        calls: Mutex<Vec<ProjectorCall>>,
        face_responses: HashMap<GeometryHandleId, Result<[f64; 3], ProjectorPayload>>,
        edge_responses: HashMap<GeometryHandleId, Result<[f64; 3], ProjectorPayload>>,
        vertex_responses: HashMap<GeometryHandleId, Result<[f64; 3], ProjectorPayload>>,
    }

    impl RecordingProjector {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                face_responses: HashMap::new(),
                edge_responses: HashMap::new(),
                vertex_responses: HashMap::new(),
            }
        }

        fn add_face_response(
            &mut self,
            face: GeometryHandleId,
            result: Result<[f64; 3], ProjectorPayload>,
        ) {
            self.face_responses.insert(face, result);
        }

        fn add_edge_response(
            &mut self,
            edge: GeometryHandleId,
            result: Result<[f64; 3], ProjectorPayload>,
        ) {
            self.edge_responses.insert(edge, result);
        }

        fn add_vertex_response(
            &mut self,
            vertex: GeometryHandleId,
            result: Result<[f64; 3], ProjectorPayload>,
        ) {
            self.vertex_responses.insert(vertex, result);
        }

        fn captured_calls(&self) -> Vec<ProjectorCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Projector for RecordingProjector {
        fn project_onto_face(
            &self,
            face: GeometryHandleId,
            point: [f64; 3],
        ) -> Result<[f64; 3], ProjectorPayload> {
            self.calls
                .lock()
                .unwrap()
                .push(ProjectorCall::Face { face, point });
            self.face_responses
                .get(&face)
                .cloned()
                .unwrap_or_else(|| panic!("no canned response for face {:?}", face))
        }

        fn project_onto_edge(
            &self,
            edge: GeometryHandleId,
            point: [f64; 3],
        ) -> Result<[f64; 3], ProjectorPayload> {
            self.calls
                .lock()
                .unwrap()
                .push(ProjectorCall::Edge { edge, point });
            self.edge_responses
                .get(&edge)
                .cloned()
                .unwrap_or_else(|| panic!("no canned response for edge {:?}", edge))
        }

        fn vertex_position(&self, vertex: GeometryHandleId) -> Result<[f64; 3], ProjectorPayload> {
            self.calls
                .lock()
                .unwrap()
                .push(ProjectorCall::Vertex { vertex });
            self.vertex_responses
                .get(&vertex)
                .cloned()
                .unwrap_or_else(|| panic!("no canned response for vertex {:?}", vertex))
        }
    }

    // ── Step-3: BoundaryAssociation round-trip ────────────────────────────────

    #[test]
    fn boundary_association_default_is_empty_and_associate_round_trips_through_iter() {
        let mut ba = BoundaryAssociation::default();
        assert!(ba.is_empty());
        assert_eq!(ba.len(), 0);

        ba.associate(7, NodeAttachment::OnFace(h(10)));
        ba.associate(3, NodeAttachment::OnEdge(h(20)));

        // BTreeMap iteration yields ascending node-index order.
        let entries: Vec<_> = ba.iter().collect();
        assert_eq!(
            entries,
            vec![
                (3, NodeAttachment::OnEdge(h(20))),
                (7, NodeAttachment::OnFace(h(10))),
            ]
        );
        assert_eq!(ba.get(7), Some(NodeAttachment::OnFace(h(10))));
        assert_eq!(ba.len(), 2);
        assert!(!ba.is_empty());
    }

    // ── Step-7: Projector trait is object-safe ────────────────────────────────

    #[test]
    fn projector_trait_is_object_safe_and_dispatches_through_dyn_reference() {
        let mut proj = RecordingProjector::new();
        proj.add_face_response(h(1), Ok([10.0, 20.0, 30.0]));
        proj.add_edge_response(h(2), Ok([0.5, 0.5, 0.0]));
        proj.add_vertex_response(h(3), Ok([1.0, 0.0, 0.0]));

        // Exercise through &dyn Projector — compile-time object-safety check.
        let dyn_ref: &dyn Projector = &proj;
        let face_result = dyn_ref.project_onto_face(h(1), [9.0, 19.0, 29.0]);
        let edge_result = dyn_ref.project_onto_edge(h(2), [0.4, 0.4, 0.0]);
        let vertex_result = dyn_ref.vertex_position(h(3));

        assert_eq!(face_result, Ok([10.0, 20.0, 30.0]));
        assert_eq!(edge_result, Ok([0.5, 0.5, 0.0]));
        assert_eq!(vertex_result, Ok([1.0, 0.0, 0.0]));

        let calls = proj.captured_calls();
        assert_eq!(calls.len(), 3);
    }

    // ── Step-9: empty boundary returns empty vec ──────────────────────────────

    #[test]
    fn compute_dirichlet_bcs_with_empty_boundary_returns_empty_vec_and_does_not_invoke_projector() {
        let proj = RecordingProjector::new();
        let result = compute_dirichlet_bcs(
            &empty_mesh(),
            &BoundaryAssociation::default(),
            &CorrespondenceMap::default(),
            &proj,
        );
        assert_eq!(result, Ok(vec![]));
        assert_eq!(
            proj.captured_calls().len(),
            0,
            "projector must not be called"
        );
    }

    // ── Step-11: face-attached node projects onto mapped new face ─────────────

    #[test]
    fn compute_dirichlet_bcs_face_attached_node_projects_onto_mapped_new_face_with_old_position() {
        let mesh = mesh_with_vertices(vec![1.0_f32, 2.0, 3.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnFace(h(10)));

        let mut correspondence = CorrespondenceMap::default();
        correspondence.face_to_face.insert(h(10), h(20));

        let mut proj = RecordingProjector::new();
        proj.add_face_response(h(20), Ok([1.5, 2.5, 3.5]));

        let result = compute_dirichlet_bcs(&mesh, &ba, &correspondence, &proj);
        assert_eq!(result, Ok(vec![(0, [1.5, 2.5, 3.5])]));

        let calls = proj.captured_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            ProjectorCall::Face {
                face: h(20),
                point: [1.0, 2.0, 3.0]
            }
        );
    }

    // ── Step-13: face-attached with missing correspondence ────────────────────

    #[test]
    fn compute_dirichlet_bcs_face_attached_with_missing_correspondence_returns_missing_correspondence_face()
     {
        let mesh = mesh_with_vertices(vec![0.0_f32, 0.0, 0.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnFace(h(10)));

        let proj = RecordingProjector::new();
        let result = compute_dirichlet_bcs(&mesh, &ba, &CorrespondenceMap::default(), &proj);
        assert_eq!(
            result,
            Err(ProjectionFailure::MissingCorrespondence {
                kind: SubShapeKind::Face,
                old_handle: h(10),
            })
        );
        assert_eq!(
            proj.captured_calls().len(),
            0,
            "projector must not be called"
        );
    }

    // ── Step-15: edge-attached node projects onto mapped new edge ─────────────

    #[test]
    fn compute_dirichlet_bcs_edge_attached_node_projects_onto_mapped_new_edge() {
        let mesh = mesh_with_vertices(vec![0.0_f32, 0.5, 1.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnEdge(h(30)));

        let mut correspondence = CorrespondenceMap::default();
        correspondence.edge_to_edge.insert(h(30), h(40));

        let mut proj = RecordingProjector::new();
        proj.add_edge_response(h(40), Ok([0.0, 0.6, 1.0]));

        let result = compute_dirichlet_bcs(&mesh, &ba, &correspondence, &proj);
        assert_eq!(result, Ok(vec![(0, [0.0, 0.6, 1.0])]));

        let calls = proj.captured_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            ProjectorCall::Edge {
                edge: h(40),
                point: [0.0, 0.5, 1.0]
            }
        );
    }

    // ── Step-17: edge-attached with missing correspondence ────────────────────

    #[test]
    fn compute_dirichlet_bcs_edge_attached_with_missing_correspondence_returns_missing_correspondence_edge()
     {
        let mesh = mesh_with_vertices(vec![0.0_f32, 0.0, 0.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnEdge(h(30)));

        let proj = RecordingProjector::new();
        let result = compute_dirichlet_bcs(&mesh, &ba, &CorrespondenceMap::default(), &proj);
        assert_eq!(
            result,
            Err(ProjectionFailure::MissingCorrespondence {
                kind: SubShapeKind::Edge,
                old_handle: h(30),
            })
        );
        assert_eq!(
            proj.captured_calls().len(),
            0,
            "projector must not be called"
        );
    }

    // ── Step-19: vertex-attached node snaps to new vertex position ────────────

    #[test]
    fn compute_dirichlet_bcs_vertex_attached_node_snaps_to_new_vertex_position_via_projector() {
        let mesh = mesh_with_vertices(vec![2.0_f32, 0.0, 0.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnVertex(h(50)));

        // Manually populated even though Stage B never produces it in v0.2.
        let mut correspondence = CorrespondenceMap::default();
        correspondence.vertex_to_vertex.insert(h(50), h(60));

        let mut proj = RecordingProjector::new();
        proj.add_vertex_response(h(60), Ok([2.1, 0.0, 0.0]));

        let result = compute_dirichlet_bcs(&mesh, &ba, &correspondence, &proj);
        assert_eq!(result, Ok(vec![(0, [2.1, 0.0, 0.0])]));

        let calls = proj.captured_calls();
        assert_eq!(calls.len(), 1);
        // Must call vertex_position with new handle h(60), not old h(50).
        // No point argument — vertex is a snap.
        assert_eq!(calls[0], ProjectorCall::Vertex { vertex: h(60) });
    }

    // ── Step-21: vertex-attached with v0.2 empty vertex_to_vertex ────────────

    /// Pins the v0.2 behaviour: [`CorrespondenceMap::vertex_to_vertex`] is
    /// always empty because Stage B never populates it. Any future task that
    /// populates `vertex_to_vertex` will see this test fail and must update
    /// both the test and the doc-comment in lockstep.
    #[test]
    fn compute_dirichlet_bcs_vertex_attached_with_v0_2_empty_vertex_correspondence_returns_missing_correspondence_vertex()
     {
        let mesh = mesh_with_vertices(vec![0.0_f32, 0.0, 0.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnVertex(h(50)));

        // Default CorrespondenceMap: vertex_to_vertex is always empty in v0.2.
        let result = compute_dirichlet_bcs(
            &mesh,
            &ba,
            &CorrespondenceMap::default(),
            &RecordingProjector::new(),
        );
        assert_eq!(
            result,
            Err(ProjectionFailure::MissingCorrespondence {
                kind: SubShapeKind::Vertex,
                old_handle: h(50),
            })
        );
    }

    // ── Step-23: node index out of range ─────────────────────────────────────

    #[test]
    fn compute_dirichlet_bcs_node_index_out_of_mesh_vertices_range_returns_invalid_node_index() {
        // 2 nodes → vertices.len() == 6; node 5 → base = 15 >= 6
        let mesh = mesh_with_vertices(vec![0.0_f32, 0.0, 0.0, 1.0, 1.0, 1.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(5, NodeAttachment::OnFace(h(10)));

        let mut correspondence = CorrespondenceMap::default();
        correspondence.face_to_face.insert(h(10), h(20));

        let proj = RecordingProjector::new();
        let result = compute_dirichlet_bcs(&mesh, &ba, &correspondence, &proj);
        assert_eq!(result, Err(ProjectionFailure::InvalidNodeIndex(5)));
        assert_eq!(
            proj.captured_calls().len(),
            0,
            "projector must not be called"
        );
    }

    // ── Step-25: propagates projector face failure ────────────────────────────

    #[test]
    fn compute_dirichlet_bcs_propagates_projector_face_failure_as_projection_failure_projector_variant()
     {
        let mesh = mesh_with_vertices(vec![0.0_f32, 0.0, 0.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnFace(h(10)));

        let mut correspondence = CorrespondenceMap::default();
        correspondence.face_to_face.insert(h(10), h(20));

        let mut proj = RecordingProjector::new();
        proj.add_face_response(
            h(20),
            Err(ProjectorPayload::new("BRepExtrema_DistShapeShape failed")),
        );

        let result = compute_dirichlet_bcs(&mesh, &ba, &correspondence, &proj);
        match result {
            Err(ProjectionFailure::Projector(payload)) => {
                assert_eq!(payload.message(), "BRepExtrema_DistShapeShape failed");
            }
            other => panic!("expected Projector failure, got: {other:?}"),
        }
    }

    // ── Step-25b (parity): propagates projector edge failure ─────────────────

    #[test]
    fn compute_dirichlet_bcs_propagates_projector_edge_failure_as_projection_failure_projector_variant()
     {
        let mesh = mesh_with_vertices(vec![0.0_f32, 0.0, 0.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnEdge(h(30)));

        let mut correspondence = CorrespondenceMap::default();
        correspondence.edge_to_edge.insert(h(30), h(40));

        let mut proj = RecordingProjector::new();
        proj.add_edge_response(
            h(40),
            Err(ProjectorPayload::new("BRepExtrema edge projection failed")),
        );

        let result = compute_dirichlet_bcs(&mesh, &ba, &correspondence, &proj);
        match result {
            Err(ProjectionFailure::Projector(payload)) => {
                assert_eq!(payload.message(), "BRepExtrema edge projection failed");
            }
            other => panic!("expected Projector failure, got: {other:?}"),
        }
    }

    // ── Step-25c (parity): propagates projector vertex failure ───────────────

    #[test]
    fn compute_dirichlet_bcs_propagates_projector_vertex_failure_as_projection_failure_projector_variant()
     {
        let mesh = mesh_with_vertices(vec![0.0_f32, 0.0, 0.0]);
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnVertex(h(50)));

        // Manually populated even though Stage B never produces it in v0.2 —
        // same approach as the existing happy-path vertex test.
        let mut correspondence = CorrespondenceMap::default();
        correspondence.vertex_to_vertex.insert(h(50), h(60));

        let mut proj = RecordingProjector::new();
        proj.add_vertex_response(
            h(60),
            Err(ProjectorPayload::new("vertex_position lookup failed")),
        );

        let result = compute_dirichlet_bcs(&mesh, &ba, &correspondence, &proj);
        match result {
            Err(ProjectionFailure::Projector(payload)) => {
                assert_eq!(payload.message(), "vertex_position lookup failed");
            }
            other => panic!("expected Projector failure, got: {other:?}"),
        }
    }

    // ── Step-27: PRD critical-correctness regression guard ────────────────────

    /// PRD invariant: corner / edge nodes must project onto the *mapped
    /// counterpart* face, not the closest face globally.
    /// Without per-face correspondence, nodes near corners can 'jump' across
    /// to the wrong face when the geometry deforms — producing an invalid morph
    /// that no quality check would catch cleanly.
    #[test]
    fn compute_dirichlet_bcs_corner_node_projects_onto_attached_face_not_globally_closest_face() {
        let mesh = mesh_with_vertices(vec![1.0_f32, 1.0, 0.0]);
        let mut ba = BoundaryAssociation::default();
        // Node is attached to old_face h(11), NOT h(10).
        ba.associate(0, NodeAttachment::OnFace(h(11)));

        let mut correspondence = CorrespondenceMap::default();
        correspondence.face_to_face.insert(h(10), h(20));
        correspondence.face_to_face.insert(h(11), h(21));

        let mut proj = RecordingProjector::new();
        proj.add_face_response(h(21), Ok([1.05, 1.0, 0.0]));
        // Competing canned response for the globally-closest face h(20).
        // Wiring it means a regression that incorrectly dispatches to h(20)
        // would *silently succeed at the projector level* rather than
        // panicking with "no canned response for face …" — so the
        // captured_calls assertions below are the definitive failure signals.
        proj.add_face_response(h(20), Ok([0.0, 0.0, 0.0]));

        let result = compute_dirichlet_bcs(&mesh, &ba, &correspondence, &proj);
        assert_eq!(result, Ok(vec![(0, [1.05, 1.0, 0.0])]));

        // Structural guard: exactly one face dispatch, to the mapped counterpart h(21).
        // Asserting the only recorded call is to h(21) implies zero calls referencing h(20).
        let calls = proj.captured_calls();
        assert_eq!(calls.len(), 1, "expected exactly one projector call");
        assert_eq!(
            calls[0],
            ProjectorCall::Face {
                face: h(21),
                point: [1.0, 1.0, 0.0]
            },
            "must dispatch to mapped face h(21), never the globally-closest h(20)"
        );
    }

    // ── Step-29: multiple attachments in ascending node-index order ───────────

    #[test]
    fn compute_dirichlet_bcs_multiple_attachments_returns_results_in_node_index_ascending_order() {
        // 8 nodes → indices 0..7 all valid
        let mut flat = vec![0.0_f32; 8 * 3];
        // Set node 3 at some position
        flat[3 * 3] = 3.0;
        flat[3 * 3 + 1] = 0.3;
        flat[3 * 3 + 2] = 0.0;
        // Node 5
        flat[5 * 3] = 5.0;
        flat[5 * 3 + 1] = 0.5;
        flat[5 * 3 + 2] = 0.0;
        // Node 7
        flat[7 * 3] = 7.0;
        flat[7 * 3 + 1] = 0.7;
        flat[7 * 3 + 2] = 0.0;

        let mesh = mesh_with_vertices(flat);

        // Insert out-of-order to verify BTreeMap sorts by key.
        let mut ba = BoundaryAssociation::default();
        ba.associate(7, NodeAttachment::OnFace(h(10)));
        ba.associate(3, NodeAttachment::OnEdge(h(30)));
        ba.associate(5, NodeAttachment::OnFace(h(11)));

        let mut correspondence = CorrespondenceMap::default();
        correspondence.face_to_face.insert(h(10), h(20));
        correspondence.face_to_face.insert(h(11), h(21));
        correspondence.edge_to_edge.insert(h(30), h(40));

        let mut proj = RecordingProjector::new();
        proj.add_edge_response(h(40), Ok([3.0, 0.31, 0.0]));
        proj.add_face_response(h(21), Ok([5.0, 0.51, 0.0]));
        proj.add_face_response(h(20), Ok([7.0, 0.71, 0.0]));

        let result = compute_dirichlet_bcs(&mesh, &ba, &correspondence, &proj).unwrap();

        // Must be in ascending node-index order: 3, 5, 7.
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, 3, "first entry should be node 3");
        assert_eq!(result[1].0, 5, "second entry should be node 5");
        assert_eq!(result[2].0, 7, "third entry should be node 7");
    }

    // ── Step-31: lib re-exports public surface ────────────────────────────────

    // This test lives in lib.rs tests module (step-31 wires the re-exports).
    // Verified separately when step-32's impl lands.
}
