//! Integration tests for topology-relational selectors via the public
//! OcctKernel API.
//!
//! These tests exercise `GeometryQuery::AdjacentFaces` and
//! `GeometryQuery::SharedEdges` against a 10×10×10 unit box (where
//! every face has exactly 4 adjacent faces and every adjacent pair
//! shares exactly 1 edge), plus a fused two-box solid for non-manifold
//! / complex-topology robustness.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Helper: build a kernel containing one 10×10×10 box, return the kernel
/// and the handle id of the box.
fn box_kernel() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("Box creation should succeed");
    (kernel, box_h.id)
}

#[test]
fn box_face_zero_has_four_adjacent_faces() {
    let (kernel, box_id) = box_kernel();

    let result = kernel.query(&GeometryQuery::AdjacentFaces {
        shape: box_id,
        face_index: 0,
    });

    let items = match result {
        Ok(Value::List(items)) => items,
        Ok(other) => panic!("expected Value::List, got {:?}", other),
        Err(e) => panic!("expected Ok(Value::List), got Err({:?})", e),
    };

    assert_eq!(
        items.len(),
        4,
        "a box face should have exactly 4 adjacent faces, got {}",
        items.len()
    );

    let mut seen = std::collections::HashSet::new();
    for item in &items {
        match item {
            Value::Int(idx) => {
                assert!(
                    *idx >= 0 && *idx < 6,
                    "face index {} out of expected box face range [0, 6)",
                    idx
                );
                assert!(
                    *idx != 0,
                    "adjacent_faces should not include the queried face itself"
                );
                assert!(
                    seen.insert(*idx),
                    "duplicate face index {} in adjacent_faces result",
                    idx
                );
            }
            other => panic!("expected Value::Int, got {:?}", other),
        }
    }
}
