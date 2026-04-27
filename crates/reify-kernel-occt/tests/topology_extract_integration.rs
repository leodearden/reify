//! Integration tests for raw topology extractors `extract_edges` /
//! `extract_faces` on the public OcctKernel API (task 318).
//!
//! These selectors materialize each unique sub-shape (deduplicated by
//! `IsSame`) into a fresh kernel handle whose ReprKind reflects the
//! sub-shape kind (`Edge` or `Face`).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, Value};

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
