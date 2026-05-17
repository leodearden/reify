//! Tests for the NodeAttachment producer: `compute_boundary_association`,
//! `validate_attribution_length`, and (when `has_gmsh`)
//! `mesh_surface_to_volume_with_attribution`.
//!
//! File-level gate: requires the `mesh-morph` feature.  The self-dev-dep in
//! `Cargo.toml` activates it for all integration test binaries automatically.
#![cfg(feature = "mesh-morph")]

use reify_kernel_gmsh::mesh_boundary::{BoundaryAttributionInput, compute_boundary_association};
use reify_types::{GeometryHandleId, Mesh, NodeAttachment};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn h(n: u64) -> GeometryHandleId {
    GeometryHandleId(n)
}

fn mesh_with_n_vertices(n: usize) -> Mesh {
    let vertices: Vec<f32> = (0..n)
        .flat_map(|i| {
            let f = i as f32;
            [f, f * 2.0, f * 3.0]
        })
        .collect();
    // Indices don't matter for these tests.
    Mesh { vertices, indices: vec![], normals: None }
}

// ---------------------------------------------------------------------------
// Step-1: compute_boundary_association — no-snap path
// ---------------------------------------------------------------------------

#[test]
fn compute_boundary_association_empty_per_vertex_returns_empty_association() {
    let surface = mesh_with_n_vertices(0);
    let attribution = BoundaryAttributionInput {
        per_vertex: vec![],
        vertex_candidates: vec![],
        snap_tolerance: 0.0,
    };
    let result = compute_boundary_association(&attribution, &surface);
    assert!(result.is_empty(), "expected empty association for 0-vertex mesh");
    assert_eq!(result.len(), 0);
}

#[test]
fn compute_boundary_association_passes_per_vertex_attachments_through_when_no_snap_candidates() {
    let surface = mesh_with_n_vertices(3);
    let attribution = BoundaryAttributionInput {
        per_vertex: vec![
            NodeAttachment::OnFace(h(10)),
            NodeAttachment::OnEdge(h(20)),
            NodeAttachment::OnVertex(h(30)),
        ],
        vertex_candidates: vec![],
        snap_tolerance: 0.0,
    };
    let result = compute_boundary_association(&attribution, &surface);
    assert_eq!(result.len(), 3);
    assert_eq!(result.get(0), Some(NodeAttachment::OnFace(h(10))));
    assert_eq!(result.get(1), Some(NodeAttachment::OnEdge(h(20))));
    assert_eq!(result.get(2), Some(NodeAttachment::OnVertex(h(30))));
}

#[test]
fn compute_boundary_association_iter_yields_ascending_node_index_order() {
    let surface = mesh_with_n_vertices(3);
    let attribution = BoundaryAttributionInput {
        per_vertex: vec![
            NodeAttachment::OnFace(h(10)),
            NodeAttachment::OnEdge(h(20)),
            NodeAttachment::OnVertex(h(30)),
        ],
        vertex_candidates: vec![],
        snap_tolerance: 0.0,
    };
    let result = compute_boundary_association(&attribution, &surface);
    let keys: Vec<u32> = result.iter().map(|(k, _)| k).collect();
    assert_eq!(keys, vec![0, 1, 2], "iter must yield ascending node-index order (BTreeMap discipline)");
}

// ---------------------------------------------------------------------------
// Step-3 (RED): compute_boundary_association — snap-to-vertex override path
// ---------------------------------------------------------------------------

/// Helper: build a Mesh where vertex `i` is at position `[i as f32, 0.0, 0.0]`.
fn mesh_with_vertices_at(positions: &[[f32; 3]]) -> Mesh {
    let vertices: Vec<f32> = positions.iter().flat_map(|p| p.iter().copied()).collect();
    Mesh { vertices, indices: vec![], normals: None }
}

#[test]
fn compute_boundary_association_overrides_per_vertex_with_on_vertex_when_input_coincides_with_candidate()
{
    let surface = mesh_with_vertices_at(&[[1.0, 2.0, 3.0]]);
    let attribution = BoundaryAttributionInput {
        per_vertex: vec![NodeAttachment::OnFace(h(100))],
        vertex_candidates: vec![(h(7), [1.0, 2.0, 3.0])],
        snap_tolerance: 1e-6,
    };
    let result = compute_boundary_association(&attribution, &surface);
    assert_eq!(
        result.get(0),
        Some(NodeAttachment::OnVertex(h(7))),
        "coincident vertex should be overridden to OnVertex"
    );
}

#[test]
fn compute_boundary_association_does_not_override_when_input_is_outside_snap_tolerance() {
    let surface = mesh_with_vertices_at(&[[1.0, 2.0, 3.0]]);
    let attribution = BoundaryAttributionInput {
        per_vertex: vec![NodeAttachment::OnFace(h(100))],
        // distance from [1.0,2.0,3.0] to [1.5,2.0,3.0] is 0.5 — far outside 1e-6
        vertex_candidates: vec![(h(7), [1.5, 2.0, 3.0])],
        snap_tolerance: 1e-6,
    };
    let result = compute_boundary_association(&attribution, &surface);
    assert_eq!(
        result.get(0),
        Some(NodeAttachment::OnFace(h(100))),
        "vertex outside tolerance should not be overridden"
    );
}

#[test]
fn compute_boundary_association_with_zero_snap_tolerance_disables_override() {
    // Even exact coincidence must not snap when snap_tolerance == 0.0 (strict <).
    let surface = mesh_with_vertices_at(&[[1.0, 2.0, 3.0]]);
    let attribution = BoundaryAttributionInput {
        per_vertex: vec![NodeAttachment::OnFace(h(100))],
        vertex_candidates: vec![(h(7), [1.0, 2.0, 3.0])],
        snap_tolerance: 0.0,
    };
    let result = compute_boundary_association(&attribution, &surface);
    assert_eq!(
        result.get(0),
        Some(NodeAttachment::OnFace(h(100))),
        "snap_tolerance=0.0 must disable all snap overrides, even for exact coincidence"
    );
}
