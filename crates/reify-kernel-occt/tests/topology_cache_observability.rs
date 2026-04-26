//! Integration tests asserting topology-map cache build-count invariants.
//!
//! These tests do NOT check behaviour (correctness is covered by
//! `topology_selectors_integration.rs`); they check that the lazy cache slots
//! are populated exactly once regardless of how many times the same query is
//! repeated on the same shape.

#![cfg(has_occt)]

use reify_kernel_occt::{OcctKernel, TopologyCacheBuildCounts};
use reify_types::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Helper: build a kernel containing one 10×10×10 box, return the kernel and
/// the handle id of the box.
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

/// Helper: issue `AdjacentFaces { face_index }` and unwrap the result.
fn query_adjacent_faces(kernel: &OcctKernel, shape: GeometryHandleId, face_index: usize) {
    kernel
        .query(&GeometryQuery::AdjacentFaces { shape, face_index })
        .expect("AdjacentFaces query should succeed");
}

/// Helper: issue `SharedEdges { face_a, face_b }` and unwrap the result.
fn query_shared_edges(
    kernel: &OcctKernel,
    shape: GeometryHandleId,
    face_a: usize,
    face_b: usize,
) {
    kernel
        .query(&GeometryQuery::SharedEdges {
            shape,
            face_a,
            face_b,
        })
        .expect("SharedEdges query should succeed");
}

/// A freshly constructed shape should have zero build counts for all three
/// cache slots — no topology map has been built yet.
#[test]
fn topology_cache_starts_empty_on_fresh_shape() {
    let (kernel, box_id) = box_kernel();

    let counts = kernel
        .topology_cache_build_counts(box_id)
        .expect("topology_cache_build_counts should succeed for a valid handle");

    assert_eq!(
        counts,
        TopologyCacheBuildCounts {
            face_map_builds: 0,
            edge_map_builds: 0,
            edge_face_map_builds: 0,
        },
        "fresh shape should have zero cache build counts, got {:?}",
        counts
    );

    // An unknown handle must return Err(GeometryError::InvalidReference(_)).
    let bad_id = GeometryHandleId(999);
    let result = kernel.topology_cache_build_counts(bad_id);
    match result {
        Err(GeometryError::InvalidReference(id)) => {
            assert_eq!(id, bad_id, "InvalidReference should carry the bad handle id");
        }
        Ok(c) => panic!(
            "expected Err(InvalidReference) for unknown handle, got Ok({:?})",
            c
        ),
        Err(other) => panic!(
            "expected Err(InvalidReference) for unknown handle, got Err({:?})",
            other
        ),
    }
}

/// After calling `SharedEdges` for several face pairs, the edge_map cache
/// should be built exactly once, and the face_map already populated by a
/// prior `AdjacentFaces` call should NOT be rebuilt (both methods share the
/// single face_map slot on OcctShape).
#[test]
fn shared_edges_caches_edge_map_and_reuses_face_map_built_by_adjacent_faces() {
    let (kernel, box_id) = box_kernel();

    // (a) Warm the face_map and edge_face_map via one AdjacentFaces call.
    query_adjacent_faces(&kernel, box_id, 0);

    let counts_after_adjacent = kernel
        .topology_cache_build_counts(box_id)
        .expect("topology_cache_build_counts should succeed");
    assert_eq!(
        counts_after_adjacent,
        TopologyCacheBuildCounts {
            face_map_builds: 1,
            edge_map_builds: 0,
            edge_face_map_builds: 1,
        },
        "after 1 adjacent_faces call (pre-condition), got {:?}",
        counts_after_adjacent
    );

    // (b) Issue five shared_edges calls on distinct face pairs.
    for (fa, fb) in [(0, 1), (0, 2), (0, 3), (1, 2), (2, 3)] {
        query_shared_edges(&kernel, box_id, fa, fb);
    }

    // (c) face_map must NOT be rebuilt (was already cached), edge_map must be
    //     built exactly once across all five calls, edge_face_map stays at 1.
    let counts_final = kernel
        .topology_cache_build_counts(box_id)
        .expect("topology_cache_build_counts should succeed");
    assert_eq!(
        counts_final,
        TopologyCacheBuildCounts {
            face_map_builds: 1,
            edge_map_builds: 1,
            edge_face_map_builds: 1,
        },
        "after 5 shared_edges calls: face_map unchanged (1), edge_map built once (1), \
         edge_face_map unchanged (1). Got {:?}",
        counts_final
    );
}

/// After calling `AdjacentFaces` for every face of a 10×10×10 box (6 calls
/// total), the face_map and edge_face_map caches should each have been built
/// exactly once, and the edge_map should remain untouched (adjacent_faces
/// does not use the global edge map).
#[test]
fn adjacent_faces_repeated_calls_build_face_and_edge_face_map_exactly_once() {
    let (kernel, box_id) = box_kernel();

    // Six calls — one per face of the box.
    for i in 0..6 {
        query_adjacent_faces(&kernel, box_id, i);
    }

    let counts = kernel
        .topology_cache_build_counts(box_id)
        .expect("topology_cache_build_counts should succeed");

    assert_eq!(
        counts,
        TopologyCacheBuildCounts {
            face_map_builds: 1,
            edge_map_builds: 0,
            edge_face_map_builds: 1,
        },
        "after 6 adjacent_faces calls: face_map and edge_face_map should each \
         be built once; edge_map should be untouched. Got {:?}",
        counts
    );
}
