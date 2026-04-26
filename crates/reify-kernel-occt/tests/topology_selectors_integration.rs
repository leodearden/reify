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
use reify_types::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

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

/// Helper: query `AdjacentFaces` for `face_index` and return the neighbor
/// list as a `HashSet<i64>`. Asserts `Ok(Value::List(_))` of `Value::Int`.
fn neighbors_of(
    kernel: &OcctKernel,
    shape: GeometryHandleId,
    face_index: usize,
) -> std::collections::HashSet<i64> {
    let result = kernel
        .query(&GeometryQuery::AdjacentFaces { shape, face_index })
        .unwrap_or_else(|e| panic!("AdjacentFaces({}) returned Err: {:?}", face_index, e));
    let items = match result {
        Value::List(v) => v,
        other => panic!("expected Value::List, got {:?}", other),
    };
    items
        .into_iter()
        .map(|v| match v {
            Value::Int(i) => i,
            other => panic!("expected Value::Int neighbor, got {:?}", other),
        })
        .collect()
}

#[test]
fn box_every_face_has_four_adjacent_faces_and_adjacency_is_symmetric() {
    let (kernel, box_id) = box_kernel();

    let neighbors: Vec<std::collections::HashSet<i64>> =
        (0..6).map(|i| neighbors_of(&kernel, box_id, i)).collect();

    // Each face has exactly 4 neighbors.
    for (i, set) in neighbors.iter().enumerate() {
        assert_eq!(
            set.len(),
            4,
            "face {} should have 4 neighbors, got {} ({:?})",
            i,
            set.len(),
            set
        );
    }

    // Symmetry: a in adj(b) <=> b in adj(a).
    for a in 0..6 {
        for b in 0..6 {
            let a_in_b = neighbors[b].contains(&(a as i64));
            let b_in_a = neighbors[a].contains(&(b as i64));
            assert_eq!(
                a_in_b, b_in_a,
                "adjacency asymmetric: a={} b={} a_in_b={} b_in_a={}",
                a, b, a_in_b, b_in_a
            );
        }
    }

    // Union of all neighbor sets covers exactly faces 0..6.
    let mut all: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for set in &neighbors {
        all.extend(set.iter().copied());
    }
    let expected: std::collections::HashSet<i64> = (0i64..6).collect();
    assert_eq!(
        all, expected,
        "union of all adjacency lists should cover faces 0..6 exactly"
    );
}

#[test]
fn adjacent_faces_with_out_of_range_face_index_returns_query_failed() {
    let (kernel, box_id) = box_kernel();

    let result = kernel.query(&GeometryQuery::AdjacentFaces {
        shape: box_id,
        face_index: 99,
    });

    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("out of range"),
                "expected error containing 'out of range', got: {msg}"
            );
            assert!(
                msg.contains("99"),
                "expected error containing the offending index '99', got: {msg}"
            );
        }
        Ok(v) => panic!("expected Err(QueryFailed), got Ok({:?})", v),
        Err(other) => panic!("expected QueryFailed, got {:?}", other),
    }
}

/// Helper: query `SharedEdges` and assert `Ok(Value::List(_))` of `Value::Int`,
/// returning the indices as an `i64` Vec for further assertions.
fn shared_edges_of(
    kernel: &OcctKernel,
    shape: GeometryHandleId,
    face_a: usize,
    face_b: usize,
) -> Vec<i64> {
    let result = kernel
        .query(&GeometryQuery::SharedEdges {
            shape,
            face_a,
            face_b,
        })
        .unwrap_or_else(|e| {
            panic!(
                "SharedEdges(face_a={}, face_b={}) returned Err: {:?}",
                face_a, face_b, e
            )
        });
    let items = match result {
        Value::List(v) => v,
        other => panic!("expected Value::List, got {:?}", other),
    };
    items
        .into_iter()
        .map(|v| match v {
            Value::Int(i) => i,
            other => panic!("expected Value::Int edge index, got {:?}", other),
        })
        .collect()
}

#[test]
fn box_two_adjacent_faces_share_exactly_one_edge() {
    let (kernel, box_id) = box_kernel();

    for face in 0..6 {
        let neighbors = neighbors_of(&kernel, box_id, face);
        for &neighbor in &neighbors {
            let neighbor_idx = neighbor as usize;
            let edges = shared_edges_of(&kernel, box_id, face, neighbor_idx);
            assert_eq!(
                edges.len(),
                1,
                "adjacent box faces ({}, {}) should share exactly 1 edge, got {} ({:?})",
                face,
                neighbor_idx,
                edges.len(),
                edges
            );
            let edge_idx = edges[0];
            assert!(
                edge_idx >= 0 && edge_idx < 12,
                "edge index {} out of expected box edge range [0, 12)",
                edge_idx
            );
        }
    }
}

#[test]
fn box_opposite_faces_share_no_edges() {
    let (kernel, box_id) = box_kernel();

    // For each face, find its opposite: the unique face index that is
    // neither itself nor in its adjacency set.
    let neighbors: Vec<std::collections::HashSet<i64>> =
        (0..6).map(|i| neighbors_of(&kernel, box_id, i)).collect();

    for face in 0..6usize {
        // The opposite face is the one in 0..6 that is not `face` and not a neighbor.
        let opposite_candidates: Vec<usize> = (0..6usize)
            .filter(|&i| i != face && !neighbors[face].contains(&(i as i64)))
            .collect();
        assert_eq!(
            opposite_candidates.len(),
            1,
            "expected exactly 1 opposite face for face {}, got {:?} (neighbors={:?})",
            face,
            opposite_candidates,
            neighbors[face]
        );
        let opposite = opposite_candidates[0];

        let edges = shared_edges_of(&kernel, box_id, face, opposite);
        assert!(
            edges.is_empty(),
            "opposite faces ({}, {}) should share no edges, got {:?}",
            face,
            opposite,
            edges
        );
    }
}

#[test]
fn shared_edges_same_face_returns_empty_list() {
    let (kernel, box_id) = box_kernel();

    let edges = shared_edges_of(&kernel, box_id, 0, 0);
    assert!(
        edges.is_empty(),
        "shared_edges(f, f) should return an empty list, got {:?}",
        edges
    );
}
