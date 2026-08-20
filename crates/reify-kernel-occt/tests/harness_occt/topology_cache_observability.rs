//! Integration tests asserting topology-map cache build-count invariants.
//!
//! These tests do NOT check behaviour (correctness is covered by
//! `topology_selectors_integration.rs`); they check that the lazy cache slots
//! are populated exactly once regardless of how many times the same query is
//! repeated on the same shape.

#![cfg(has_occt)]

use reify_kernel_occt::{OcctKernel, TopologyCacheBuildCounts};
use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

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
fn query_shared_edges(kernel: &OcctKernel, shape: GeometryHandleId, face_a: usize, face_b: usize) {
    kernel
        .query(&GeometryQuery::SharedEdges {
            shape,
            face_a,
            face_b,
        })
        .expect("SharedEdges query should succeed");
}

/// Helper: assert that issuing `query_fn` twice from the same initial state
/// does not rebuild any cache slot beyond the first call. Checks (1) counts
/// after the first call match `expected_first`, and (2) counts after the second
/// identical call are unchanged — the lazy-once invariant holds.
fn assert_second_call_does_not_rebuild<F>(
    kernel: &OcctKernel,
    shape: GeometryHandleId,
    query_fn: F,
    expected_first: TopologyCacheBuildCounts,
) where
    F: Fn(&OcctKernel, GeometryHandleId),
{
    query_fn(kernel, shape);
    let counts1 = kernel
        .topology_cache_build_counts(shape)
        .expect("topology_cache_build_counts should succeed");
    assert_eq!(
        counts1, expected_first,
        "after first call: expected {:?}, got {:?}",
        expected_first, counts1
    );

    query_fn(kernel, shape);
    let counts2 = kernel
        .topology_cache_build_counts(shape)
        .expect("topology_cache_build_counts should succeed");
    assert_eq!(
        counts2, counts1,
        "second identical call must not rebuild any cache slot; \
         counts1={:?}, counts2={:?}",
        counts1, counts2
    );
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
}

/// `topology_cache_build_counts` with an unknown handle must propagate the
/// `get_shape` contract: `Err(GeometryError::InvalidReference(id))`.
#[test]
fn topology_cache_build_counts_returns_invalid_reference_for_unknown_handle() {
    let (kernel, _) = box_kernel();
    let bad_id = GeometryHandleId(999);
    match kernel.topology_cache_build_counts(bad_id) {
        Err(GeometryError::InvalidReference(id)) => {
            assert_eq!(
                id, bad_id,
                "InvalidReference should carry the bad handle id"
            );
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

// STRONG-EXCEPTION-GUARANTEE:
//
// The cache slot and build counter MUST advance atomically — either both update
// on a successful MapShapes / MapShapesAndAncestors call, or neither updates on
// a throw.  The lazy accessors must build into a local `unique_ptr` and only
// move it into the slot after MapShapes returns successfully; otherwise a throw
// would leave the slot non-null with an empty map while the counter stays 0,
// masking the failure on every subsequent call (the non-null slot is treated as
// "already built" and the empty map is returned forever).
//
// This test pins the observable half of that contract: counters advance
// atomically with slot population on the happy path.  The throw path cannot be
// exercised from the public FFI surface (OCCT only throws on OOM or in-memory
// topology corruption, neither of which is reproducible here), so the invariant
// for the throw path is enforced by code review and the inline
// STRONG-EXCEPTION-GUARANTEE comments in occt_wrapper.cpp / occt_wrapper.h.

/// All three lazy cache slots — face_map, edge_map, and edge_face_map — must
/// advance their paired build counters atomically: each counter increments from
/// 0 to 1 on the first call that populates the slot, then stays at 1 forever.
/// Re-invoking the same queries many times must not increment any counter above 1.
///
/// This is the observable part of the strong-exception-guarantee contract:
/// slot and counter are coupled — neither side advances unless MapShapes
/// completes successfully.
#[test]
fn lazy_slots_advance_atomically_with_counters() {
    let (kernel, box_id) = box_kernel();

    // (1) Fresh shape — no slot has been built yet.
    let counts = kernel
        .topology_cache_build_counts(box_id)
        .expect("topology_cache_build_counts should succeed");
    assert_eq!(
        counts,
        TopologyCacheBuildCounts {
            face_map_builds: 0,
            edge_map_builds: 0,
            edge_face_map_builds: 0,
        },
        "step 1: fresh shape — expected (0,0,0), got {:?}",
        counts
    );

    // (2) One AdjacentFaces call — populates face_map and edge_face_map only.
    query_adjacent_faces(&kernel, box_id, 0);

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
        "step 2: after AdjacentFaces — expected (1,0,1), got {:?}",
        counts
    );

    // (3) One SharedEdges call — populates edge_map; face_map already cached.
    query_shared_edges(&kernel, box_id, 0, 1);

    let counts = kernel
        .topology_cache_build_counts(box_id)
        .expect("topology_cache_build_counts should succeed");
    assert_eq!(
        counts,
        TopologyCacheBuildCounts {
            face_map_builds: 1,
            edge_map_builds: 1,
            edge_face_map_builds: 1,
        },
        "step 3: after SharedEdges — expected (1,1,1), got {:?}",
        counts
    );

    // (4) All three slots are now warm.  Repeating queries must NOT increment
    //     any counter — the caches must be reused, not rebuilt.
    for _ in 0..5 {
        query_adjacent_faces(&kernel, box_id, 0);
        query_shared_edges(&kernel, box_id, 0, 1);
    }

    let counts = kernel
        .topology_cache_build_counts(box_id)
        .expect("topology_cache_build_counts should succeed");
    assert_eq!(
        counts,
        TopologyCacheBuildCounts {
            face_map_builds: 1,
            edge_map_builds: 1,
            edge_face_map_builds: 1,
        },
        "step 4: after 5 repeated rounds — counters must remain (1,1,1), got {:?}",
        counts
    );
}

/// Two adjacent calls to AdjacentFaces with the SAME face_index must not
/// rebuild any cache slot — the lazy slots are non-null after the first call,
/// so the second call must short-circuit. Locks down a regression where the
/// face_map or edge_face_map could be rebuilt despite the slot being non-null.
#[test]
fn adjacent_faces_same_query_does_not_rebuild_cache() {
    let (kernel, box_id) = box_kernel();
    assert_second_call_does_not_rebuild(
        &kernel,
        box_id,
        |k, s| query_adjacent_faces(k, s, 0),
        TopologyCacheBuildCounts {
            face_map_builds: 1,
            edge_map_builds: 0,
            edge_face_map_builds: 1,
        },
    );
}

/// Two adjacent calls to SharedEdges with the SAME (face_a, face_b) must not
/// rebuild any cache slot — the face_map and edge_map slots are non-null after
/// the first call, so the second call must short-circuit. Locks down the lazy-
/// once invariant for the SharedEdges code path (parallel to
/// `adjacent_faces_same_query_does_not_rebuild_cache`).
#[test]
fn shared_edges_same_query_does_not_rebuild_cache() {
    let (kernel, box_id) = box_kernel();
    assert_second_call_does_not_rebuild(
        &kernel,
        box_id,
        |k, s| query_shared_edges(k, s, 0, 1),
        TopologyCacheBuildCounts {
            face_map_builds: 1,
            edge_map_builds: 1,
            edge_face_map_builds: 0,
        },
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
