//! Integration tests for raw topology extractors `extract_edges` /
//! `extract_faces` on the public OcctKernel API (task 318).
//!
//! These selectors materialize each unique sub-shape (deduplicated by
//! `IsSame`) into a fresh kernel handle whose ReprKind reflects the
//! sub-shape kind (`Edge` or `Face`).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, ReprKind, Value};

/// Helper: build a kernel containing one box of the given mm dimensions
/// (converted to SI metres at the kernel boundary so geometric queries
/// like `SurfaceArea`/`EdgeLength` return values in m² / m) and return
/// the kernel + its handle id.
fn box_kernel(width_mm: f64, height_mm: f64, depth_mm: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(width_mm * 1e-3),
            height: Value::Real(height_mm * 1e-3),
            depth: Value::Real(depth_mm * 1e-3),
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
fn extract_faces_face_handles_have_correct_surface_area() {
    // 10x20x30 mm box → 6 faces in 3 axis-aligned pairs:
    //   - 2 × (10mm × 20mm) = 2 × 200e-6 m²
    //   - 2 × (10mm × 30mm) = 2 × 300e-6 m²
    //   - 2 × (20mm × 30mm) = 2 × 600e-6 m²
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces on a valid box should succeed");

    let mut areas: Vec<f64> = faces
        .iter()
        .map(|id| match kernel.query(&GeometryQuery::SurfaceArea(*id)) {
            Ok(Value::Real(a)) => a,
            other => panic!("SurfaceArea({:?}) returned unexpected value: {:?}", id, other),
        })
        .collect();
    areas.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let expected = [
        200e-6, 200e-6, // 10 × 20
        300e-6, 300e-6, // 10 × 30
        600e-6, 600e-6, // 20 × 30
    ];
    let tol = 1e-9;
    assert_eq!(areas.len(), expected.len());
    for (got, want) in areas.iter().zip(expected.iter()) {
        assert!(
            (got - want).abs() < tol,
            "extracted-face area mismatch: got {got}, want {want} (tol={tol}). \
             full sorted areas: {:?}",
            areas
        );
    }
}

#[test]
fn query_edge_length_returns_correct_value_for_extracted_box_edge() {
    // 10x20x30 mm box → 12 edges in 3 axis-aligned groups of 4:
    //   - 4 × 10 mm = 4 × 0.010 m
    //   - 4 × 20 mm = 4 × 0.020 m
    //   - 4 × 30 mm = 4 × 0.030 m
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);
    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges on a valid box should succeed");

    let mut lengths: Vec<f64> = edges
        .iter()
        .map(|id| match kernel.query(&GeometryQuery::EdgeLength(*id)) {
            Ok(Value::Real(l)) => l,
            other => panic!("EdgeLength({:?}) returned unexpected value: {:?}", id, other),
        })
        .collect();
    lengths.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let expected = [
        0.010, 0.010, 0.010, 0.010, // x-axis edges
        0.020, 0.020, 0.020, 0.020, // y-axis edges
        0.030, 0.030, 0.030, 0.030, // z-axis edges
    ];
    let tol = 1e-9;
    assert_eq!(lengths.len(), expected.len());
    for (got, want) in lengths.iter().zip(expected.iter()) {
        assert!(
            (got - want).abs() < tol,
            "edge length mismatch: got {got}, want {want} (tol={tol}). \
             full sorted lengths: {:?}",
            lengths
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
