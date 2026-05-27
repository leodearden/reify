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
use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

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
                (0..12).contains(&edge_idx),
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

    for (face, face_neighbors) in neighbors.iter().enumerate().take(6) {
        // The opposite face is the one in 0..6 that is not `face` and not a neighbor.
        let opposite_candidates: Vec<usize> = (0..6usize)
            .filter(|&i| i != face && !face_neighbors.contains(&(i as i64)))
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

#[test]
fn shared_edges_with_out_of_range_face_index_returns_query_failed() {
    let (kernel, box_id) = box_kernel();

    // Sub-assert (a): face_a out of range.
    let result_a = kernel.query(&GeometryQuery::SharedEdges {
        shape: box_id,
        face_a: 99,
        face_b: 0,
    });
    match result_a {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("out of range"),
                "expected error containing 'out of range' (face_a=99), got: {msg}"
            );
            assert!(
                msg.contains("99"),
                "expected error containing the offending index '99' (face_a=99), got: {msg}"
            );
        }
        Ok(v) => panic!("expected Err(QueryFailed) for face_a=99, got Ok({:?})", v),
        Err(other) => panic!("expected QueryFailed for face_a=99, got {:?}", other),
    }

    // Sub-assert (b): face_b out of range.
    let result_b = kernel.query(&GeometryQuery::SharedEdges {
        shape: box_id,
        face_a: 0,
        face_b: 99,
    });
    match result_b {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("out of range"),
                "expected error containing 'out of range' (face_b=99), got: {msg}"
            );
            assert!(
                msg.contains("99"),
                "expected error containing the offending index '99' (face_b=99), got: {msg}"
            );
        }
        Ok(v) => panic!("expected Err(QueryFailed) for face_b=99, got Ok({:?})", v),
        Err(other) => panic!("expected QueryFailed for face_b=99, got {:?}", other),
    }
}

#[test]
fn topology_selectors_on_fused_two_box_solid_match_known_geometry() {
    // Build two 10x10x10 boxes; translate the second by +10 along X so it
    // abuts the first; union them. The resulting solid has a deterministic
    // topology: 10 outer faces (each of top/bottom/front/back is split along
    // X=10 into two sub-faces; plus the two end faces at X=0 and X=20). The
    // shared interior face at X=10 collapses, contributing only the seam
    // edges to the outer topology.
    let mut kernel = OcctKernel::new();
    let box_a = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("Box A creation should succeed");
    let box_b_raw = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("Box B creation should succeed");
    let box_b = kernel
        .execute(&GeometryOp::Translate {
            target: box_b_raw.id,
            dx: 10.0,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("Box B translate should succeed");
    let fused = kernel
        .execute(&GeometryOp::Union {
            left: box_a.id,
            right: box_b.id,
        })
        .expect("Union should succeed");
    let fused_id = fused.id;

    // Verified empirically: two abutting 10x10x10 boxes fused yield exactly
    // 10 outer faces. We probe the boundary (index 10 must be out-of-range,
    // index 9 must be in-range) instead of using a brittle string-match
    // helper.
    const EXPECTED_FACES: usize = 10;
    for face in 0..EXPECTED_FACES {
        let r = kernel.query(&GeometryQuery::AdjacentFaces {
            shape: fused_id,
            face_index: face,
        });
        assert!(
            matches!(r, Ok(Value::List(_))),
            "face {} should be in range and return a list, got {:?}",
            face,
            r
        );
    }
    match kernel.query(&GeometryQuery::AdjacentFaces {
        shape: fused_id,
        face_index: EXPECTED_FACES,
    }) {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("out of range"),
                "expected 'out of range' for face {}, got: {msg}",
                EXPECTED_FACES
            );
        }
        other => panic!(
            "expected QueryFailed for index {}, got {:?}",
            EXPECTED_FACES, other
        ),
    }

    // Each face of this solid touches at least 3 other faces: the two end
    // faces (X=0 and X=20) are bordered by 4 sub-faces each; every split
    // sub-face is bordered by an end face, two perpendicular sub-faces, and
    // its co-planar partner across the X=10 seam — also 4 neighbors. So
    // every face should have >=3 neighbors as a generous lower bound.
    let neighbors: Vec<std::collections::HashSet<i64>> = (0..EXPECTED_FACES)
        .map(|i| neighbors_of(&kernel, fused_id, i))
        .collect();
    for (i, set) in neighbors.iter().enumerate() {
        assert!(
            set.len() >= 3,
            "face {} of fused solid should have ≥3 neighbors, got {} ({:?})",
            i,
            set.len(),
            set
        );
        // All neighbor indices must be in [0, EXPECTED_FACES) and distinct
        // from `i` itself.
        for &n in set {
            assert!(
                (0..EXPECTED_FACES as i64).contains(&n) && n != i as i64,
                "face {} has invalid neighbor {} (expected in [0, {}) and != {})",
                i,
                n,
                EXPECTED_FACES,
                i
            );
        }
    }

    // Adjacency is symmetric.
    for a in 0..EXPECTED_FACES {
        for b in 0..EXPECTED_FACES {
            let a_in_b = neighbors[b].contains(&(a as i64));
            let b_in_a = neighbors[a].contains(&(b as i64));
            assert_eq!(
                a_in_b, b_in_a,
                "adjacency asymmetric at ({}, {}): a_in_b={} b_in_a={}",
                a, b, a_in_b, b_in_a
            );
        }
    }

    // For every (face, neighbor) pair, SharedEdges should return a nonempty
    // list — adjacency by definition means they share at least one edge.
    // This is a meaningful correctness check on fused topology, not just
    // a "no panic" smoke probe.
    for (face, face_neighbors) in neighbors.iter().enumerate() {
        for &n in face_neighbors {
            let edges = shared_edges_of(&kernel, fused_id, face, n as usize);
            assert!(
                !edges.is_empty(),
                "adjacent faces ({}, {}) should share ≥1 edge, got empty",
                face,
                n
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// extract_faces ↔ AdjacentFaces round-trip — pinning the v0.1 contract that
// `extract_faces(box)` returns face handles in the same canonical
// TopExp_Explorer / `face_map.FindKey(i+1)` order that the 0-based
// `face_index` of `AdjacentFaces { face_index }` expects. Task 2658's v2
// selector `adjacent_to_face` (in reify-eval) layers on this exact mapping
// — if the v0.1 ordering ever changes silently, the v2 selector would
// silently return the wrong handles. This round-trip test guards that
// invariant from the kernel side.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn extract_faces_then_adjacent_faces_round_trip_box_face_zero() {
    let (mut kernel, box_id) = box_kernel();

    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    assert_eq!(
        face_handles.len(),
        6,
        "a 10×10×10 box must have exactly 6 faces"
    );

    // AdjacentFaces { face_index: 0 } must address the same face that
    // appears at index 0 in the extract_faces output. The neighbours
    // returned (as global indices into the canonical face_map order)
    // must therefore be in [1, 5] and map back to handles in the
    // extract_faces output.
    let result = kernel
        .query(&GeometryQuery::AdjacentFaces {
            shape: box_id,
            face_index: 0,
        })
        .expect("AdjacentFaces { face_index: 0 } should succeed on a box");
    let items = match result {
        Value::List(items) => items,
        other => panic!("expected Value::List, got {:?}", other),
    };
    assert_eq!(items.len(), 4, "box face 0 has exactly 4 neighbours");

    for item in items {
        let idx = match item {
            Value::Int(i) => i,
            other => panic!("expected Value::Int, got {:?}", other),
        };
        assert!(
            (0..6).contains(&idx),
            "neighbour index {} must be a valid box face index [0, 6)",
            idx
        );
        let usize_idx = usize::try_from(idx).expect("non-negative");
        let _neighbour_handle = face_handles.get(usize_idx).copied().unwrap_or_else(|| {
            panic!(
                "neighbour index {} must map to a handle in extract_faces output (len={})",
                idx,
                face_handles.len()
            )
        });
    }
}
