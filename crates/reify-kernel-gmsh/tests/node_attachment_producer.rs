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
// Step-1 (RED): compute_boundary_association — no-snap path
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
