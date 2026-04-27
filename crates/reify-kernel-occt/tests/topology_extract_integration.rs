//! Integration tests for raw topology extractors `extract_edges` /
//! `extract_faces` on the public OcctKernel API (task 318).
//!
//! These selectors materialize each unique sub-shape (deduplicated by
//! `IsSame`) into a fresh kernel handle whose ReprKind reflects the
//! sub-shape kind (`Edge` or `Face`).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, QueryError, ReprKind, Value};

/// Helper: build a kernel containing one box of the given mm dimensions
/// and return the kernel + its handle id.
fn box_kernel(width_mm: f64, height_mm: f64, depth_mm: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(width_mm),
            height: Value::Real(height_mm),
            depth: Value::Real(depth_mm),
        })
        .expect("Box creation should succeed");
    (kernel, h.id)
}

#[test]
fn extract_edges_box_returns_twelve_distinct_handles() {
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);

    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges on a valid box should succeed");

    assert_eq!(
        edges.len(),
        12,
        "a 10x20x30 box has exactly 12 unique edges, got {}",
        edges.len()
    );

    let mut seen = std::collections::HashSet::new();
    for id in &edges {
        assert_ne!(
            *id, box_id,
            "extracted edge handle must differ from the source box handle"
        );
        assert_ne!(
            *id,
            GeometryHandleId::INVALID,
            "extracted edge handle must not be the INVALID sentinel"
        );
        assert!(
            seen.insert(*id),
            "duplicate edge handle id {:?} in extract_edges result",
            id
        );
    }
}

#[test]
fn extract_edges_handles_have_edge_repr_kind() {
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);

    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges on a valid box should succeed");

    for id in &edges {
        let repr = kernel
            .repr_of(*id)
            .unwrap_or_else(|| panic!("repr_of({:?}) returned None for an extracted edge", id));
        assert_eq!(
            repr,
            ReprKind::Edge,
            "extracted edge handle {:?} should have ReprKind::Edge, got {:?}",
            id,
            repr
        );
    }
}

#[test]
fn extract_faces_box_returns_six_distinct_face_handles() {
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);

    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces on a valid box should succeed");

    assert_eq!(
        faces.len(),
        6,
        "a 10x20x30 box has exactly 6 unique faces, got {}",
        faces.len()
    );

    let mut seen = std::collections::HashSet::new();
    for id in &faces {
        assert_ne!(
            *id, box_id,
            "extracted face handle must differ from the source box handle"
        );
        assert_ne!(
            *id,
            GeometryHandleId::INVALID,
            "extracted face handle must not be the INVALID sentinel"
        );
        assert!(
            seen.insert(*id),
            "duplicate face handle id {:?} in extract_faces result",
            id
        );
        let repr = kernel
            .repr_of(*id)
            .unwrap_or_else(|| panic!("repr_of({:?}) returned None for an extracted face", id));
        assert_eq!(
            repr,
            ReprKind::Face,
            "extracted face handle {:?} should have ReprKind::Face, got {:?}",
            id,
            repr
        );
    }
}

#[test]
fn extract_edges_invalid_handle_returns_invalid_reference() {
    // Fresh kernel — no shapes registered, so handle id 999 is unknown.
    let mut kernel = OcctKernel::new();
    let bad = GeometryHandleId(999);

    let result = kernel.extract_edges(bad);

    match result {
        Err(QueryError::InvalidHandle(id)) => {
            assert_eq!(
                id, bad,
                "InvalidHandle should carry the bad handle id verbatim"
            );
        }
        Ok(v) => panic!("expected Err(InvalidHandle), got Ok({:?})", v),
        Err(other) => panic!("expected Err(InvalidHandle), got Err({:?})", other),
    }
}
