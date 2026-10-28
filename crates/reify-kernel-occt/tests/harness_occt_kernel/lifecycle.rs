//! `OcctKernel` state lifecycle: warm-state capture and restore, `reset`, the resident
//! shape count, and the vertex-extraction cache across a warm start.

#![cfg(has_occt)]

use reify_ir::{
    BRepKind, GeometryHandleId, GeometryOp, GeometryQuery, OpaqueState, QueryError, Value,
    WarmStartable,
};
use reify_kernel_occt::OcctKernel;

/// Capture `from`'s warm state and restore it into `into` — which may be
/// a freshly constructed kernel or one that already has its own state
/// (a "dirty" consumer) — returning the restored kernel. Shared by the
/// warm-start round-trip tests so each test body leads with its distinct
/// scenario setup and assertions instead of repeating the
/// capture/restore boilerplate.
fn warm_restore(from: &OcctKernel, mut into: OcctKernel) -> OcctKernel {
    let state = from.warm_state().expect("kernel should have warm state");
    into.with_warm_state(state);
    into
}

#[test]
fn warm_state_returns_none_on_fresh_kernel() {
    let kernel = OcctKernel::new();
    assert!(
        kernel.warm_state().is_none(),
        "fresh kernel should have no warm state"
    );
}

#[test]
fn warm_state_returns_some_after_ops() {
    let mut kernel = OcctKernel::new();
    // Execute a box and a cylinder
    kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();
    kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(20.0),
        })
        .unwrap();

    let state = kernel.warm_state();
    assert!(state.is_some(), "kernel with shapes should have warm state");
    let state = state.unwrap();
    assert!(
        state.estimated_size_bytes() > 0,
        "estimated size should be positive"
    );
}

#[test]
fn warm_start_roundtrip_single_shape() {
    // Create kernel A with a box
    let mut kernel_a = OcctKernel::new();
    kernel_a
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();

    // Extract warm state
    let state = kernel_a.warm_state().expect("should have warm state");

    // Create fresh kernel B and restore warm state
    let mut kernel_b = OcctKernel::new();
    kernel_b.with_warm_state(state);

    // Query volume on kernel B using handle ID 1 (the box)
    let vol = kernel_b
        .query(&GeometryQuery::Volume(GeometryHandleId(1)))
        .unwrap();
    match vol {
        Value::Real(v) => {
            assert!((v - 6000.0).abs() < 1.0, "expected volume ~6000, got {v}");
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn warm_start_roundtrip_multi_shape() {
    let mut kernel_a = OcctKernel::new();

    // Create box (10x20x30), handle ID 1
    kernel_a
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();

    // Create cylinder (r=5, h=20), handle ID 2
    kernel_a
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(20.0),
        })
        .unwrap();

    // Boolean union, handle ID 3
    kernel_a
        .execute(&GeometryOp::Union {
            left: GeometryHandleId(1),
            right: GeometryHandleId(2),
        })
        .unwrap();

    // Extract warm state
    let state = kernel_a.warm_state().expect("should have warm state");

    // Create fresh kernel B and restore
    let mut kernel_b = OcctKernel::new();
    kernel_b.with_warm_state(state);

    // Verify box volume (~6000)
    let vol_box = kernel_b
        .query(&GeometryQuery::Volume(GeometryHandleId(1)))
        .unwrap();
    match vol_box {
        Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "box vol: {v}"),
        other => panic!("expected Real, got {:?}", other),
    }

    // Verify cylinder volume (~pi*25*20 ≈ 1570.8)
    let vol_cyl = kernel_b
        .query(&GeometryQuery::Volume(GeometryHandleId(2)))
        .unwrap();
    match vol_cyl {
        Value::Real(v) => assert!((v - 1570.8).abs() < 1.0, "cyl vol: {v}"),
        other => panic!("expected Real, got {:?}", other),
    }

    // Verify union volume (positive)
    let vol_union = kernel_b
        .query(&GeometryQuery::Volume(GeometryHandleId(3)))
        .unwrap();
    match vol_union {
        Value::Real(v) => assert!(v > 0.0, "union vol should be positive: {v}"),
        other => panic!("expected Real, got {:?}", other),
    }

    // Verify next_id restored: new sphere should get handle ID 4
    let sphere_h = kernel_b
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(5.0),
        })
        .unwrap();
    assert_eq!(
        sphere_h.id,
        GeometryHandleId(4),
        "next_id should be restored"
    );
}

#[test]
fn repr_of_survives_warm_start_round_trip() {
    // Pins the invariant that `repr_of` returns the correct BRepKind after a
    // warm_state()/with_warm_state() round-trip. Prior to the original fix,
    // OcctWarmState did not persist the `reprs` map, so every restored handle
    // returned None.
    //
    // Scope note (#5162): repr survival is scoped to root/persistent handles
    // (those minted by `execute`: Box, Cylinder, Union, ...). Derived
    // sub-shape handles minted by extract_* are `parent_handle` keys on the
    // producer and are deliberately filtered out of warm_state() to bound
    // multi-cycle payload growth (they round-trip as orphaned, unreachable
    // entries otherwise — see the "Bounded payload" note in warm_state()).
    // So a previously-extracted face id correctly returns None post-restore;
    // its repr is no longer persisted. The regression this test guards —
    // reprs are persisted at all, for persistent handles — is asserted via
    // the root box handle below.

    // 1. Build kernel A with a box → BRepKind::Solid (id 1).
    let mut kernel_a = OcctKernel::new();
    let box_h = kernel_a
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.010),
            size_y: Value::Real(0.010),
            size_z: Value::Real(0.010),
        })
        .expect("Box should succeed");
    let box_id = box_h.id;

    // 2. Extract faces → each is BRepKind::Face. Capture the first face id.
    let faces = kernel_a
        .extract_faces(box_id)
        .expect("extract_faces on a valid box should succeed");
    assert_eq!(faces.len(), 6, "a box has 6 faces");
    let face_id = faces[0];

    // 3. Pre-condition: assert reprs are correct before warm-start.
    assert_eq!(
        kernel_a.repr_of(face_id),
        Some(BRepKind::Face),
        "pre-warm: face handle should have BRepKind::Face"
    );
    assert_eq!(
        kernel_a.repr_of(box_id),
        Some(BRepKind::Solid),
        "pre-warm: box handle should have BRepKind::Solid"
    );

    // 4. Round-trip.
    let kernel_b = warm_restore(&kernel_a, OcctKernel::new());

    // 5. Post-warm: the extracted-face handle is a derived sub-shape id and is
    //    deliberately filtered out of warm_state() (#5162), so its repr is not
    //    persisted and repr_of correctly returns None post-restore. This is the
    //    same orphaned-but-queryable state #5162 eliminates: pre-#5162 it would
    //    round-trip as Some(Face) but remain unreachable via OwnerBody and never
    //    be reused by a later extract_* call.
    assert_eq!(
        kernel_b.repr_of(face_id),
        None,
        "post-warm: derived sub-shape (face) repr is filtered out of warm-start (#5162)"
    );

    // 6. Post-warm: box handle (a persistent root shape) must still report
    //    BRepKind::Solid — this is the actual regression this test guards.
    assert_eq!(
        kernel_b.repr_of(box_id),
        Some(BRepKind::Solid),
        "post-warm: box handle should have BRepKind::Solid"
    );

    // 7. Truly-unknown id must still report None.
    assert_eq!(
        kernel_b.repr_of(GeometryHandleId(99999)),
        None,
        "repr_of for unknown id should return None"
    );
}

#[test]
fn owner_body_survives_warm_start() {
    // Kernel A: a lone box; its warm state will be restored into a
    // *dirty* kernel B below.
    let mut kernel_a = OcctKernel::new();
    kernel_a
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();

    // Kernel B: dirty consumer. It already has extraction provenance
    // (parent_handle entries) from its own cylinder before the restore.
    let mut kernel_b = OcctKernel::new();
    kernel_b
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(20.0),
        })
        .unwrap();
    let cyl_faces = kernel_b
        .extract_faces(GeometryHandleId(1))
        .expect("extract_faces on the cylinder should succeed");
    assert!(
        !cyl_faces.is_empty(),
        "cylinder extraction should yield at least one face to seed a stale parent_handle entry"
    );
    // Secondary sanity check, not load-bearing for the regression this
    // test targets below: a cylinder has 3 faces (two caps + the
    // lateral surface). An OCCT-version/topology change would fail
    // this without indicating a provenance-clearing regression.
    assert_eq!(
        cyl_faces.len(),
        3,
        "sanity check: a cylinder has 3 faces (two caps + the lateral surface)"
    );

    // Also seed extracted_edges/extracted_vertices provenance on the same
    // dirty kernel_b — not just extracted_faces. with_warm_state clears
    // all three extraction caches independently (see its exhaustive
    // destructure), so a hypothetical regression that cleared
    // extracted_faces on restore but forgot one of its sister caches
    // would not be caught behaviorally without this. See the
    // `edges != cyl_edges` / `verts != cyl_verts` re-extraction checks
    // below, mirroring `faces != cyl_faces`.
    let cyl_edges = kernel_b
        .extract_edges(GeometryHandleId(1))
        .expect("extract_edges on the cylinder should succeed");
    assert!(
        !cyl_edges.is_empty(),
        "cylinder extraction should yield at least one edge to seed a stale extracted_edges cache entry"
    );
    let cyl_verts = kernel_b
        .extract_vertices(GeometryHandleId(1))
        .expect("extract_vertices on the cylinder should succeed");
    assert!(
        !cyl_verts.is_empty(),
        "cylinder extraction should yield at least one vertex to seed a stale extracted_vertices cache entry"
    );
    let stale_child = cyl_faces[0];

    // Precondition: before the restore, OwnerBody correctly resolves the
    // cylinder face back to its parent (handle 1).
    assert_eq!(
        kernel_b
            .query(&GeometryQuery::OwnerBody(stale_child))
            .unwrap(),
        Value::Int(1),
        "precondition: stale_child should resolve to its cylinder parent before warm-start"
    );

    // Warm-start kernel B with kernel A's box-only state (shapes={1},
    // next_id=2). This wholesale-swaps kernel B's shape table out from
    // under its stale parent_handle entries.
    let mut kernel_b = warm_restore(&kernel_a, kernel_b);

    // RED on current main: parent_handle is never cleared by
    // with_warm_state, so OwnerBody(stale_child) still resolves to
    // Value::Int(1) — WRONG, because stale_child no longer corresponds
    // to any sub-shape of the freshly-restored box. After the fix, the
    // stale entry is cleared and this query correctly reports
    // QueryFailed.
    let post_restore = kernel_b.query(&GeometryQuery::OwnerBody(stale_child));
    assert!(
        matches!(post_restore, Err(QueryError::QueryFailed(_))),
        "stale parent_handle must be cleared on warm-start; got {:?}",
        post_restore
    );

    // PERSIST path: reprs must be restored from kernel_a's state. Handle
    // 1 now names kernel_a's box (kernel_b's own cylinder used the same
    // id before the wholesale swap), so repr_of(1) must report the
    // restored box's BRepKind, not a stale leftover.
    assert_eq!(
        kernel_b.repr_of(GeometryHandleId(1)),
        Some(BRepKind::Solid),
        "post-restore: handle 1 should report the restored box's BRepKind::Solid"
    );

    // PERSIST path: next_id must come from kernel_a's state (2, after
    // minting one handle) — not kernel_b's own dirty next_id, which the
    // cylinder + extract_faces above already advanced past kernel_a's.
    // This dirty-consumer setup is the only round-trip test where
    // kernel_b's pre-restore next_id exceeds kernel_a's, so it is the
    // one place a "keep the larger of the two" merge regression (instead
    // of an overwrite) would actually surface.
    let minted = kernel_b
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(1.0),
        })
        .unwrap();
    assert_eq!(
        minted.id,
        GeometryHandleId(2),
        "next_id should be restored from kernel_a's warm state, not kernel_b's dirty pre-restore value"
    );

    // Re-extraction on the restored (box) shape correctly rebuilds
    // provenance. If extracted_faces had not been cleared on restore,
    // this call would hit the idempotency cache under parent id 1 and
    // return the stale 3-entry cylinder-face list verbatim instead of
    // re-extracting — so the assert_ne! below (faces != cyl_faces)
    // already proves the cache was cleared, independent of the box's
    // exact face count (which OCCT-version/topology drift could
    // otherwise change without indicating this regression).
    //
    // Note: the returned ids are deliberately *not* asserted disjoint
    // from `cyl_faces`. next_id is restored wholesale from kernel_a's
    // state (asserted above) rather than merged/maxed against kernel_b's
    // dirty pre-restore counter, and this test mints a sphere in
    // between — so numeric id reuse across the warm-start boundary
    // (e.g. a new box face landing on the same integer as an old
    // cylinder face) is an expected consequence of that design, not a
    // staleness bug. The correctness guarantee under test is that every
    // *current* handle resolves correctly (see the OwnerBody loop
    // below), not that ids stay globally unique across a kernel's
    // lifetime.
    let faces = kernel_b
        .extract_faces(GeometryHandleId(1))
        .expect("extract_faces on the restored box should succeed");
    assert_ne!(
        faces, cyl_faces,
        "re-extraction after warm-start restore returned the stale cached \
         cylinder-face list verbatim — extracted_faces was not cleared on restore"
    );
    assert!(
        !faces.is_empty(),
        "re-extraction after warm-start restore should yield at least one face"
    );

    for face in &faces {
        assert_eq!(
            kernel_b.query(&GeometryQuery::OwnerBody(*face)).unwrap(),
            Value::Int(1),
            "post-restore re-extraction should rebuild provenance correctly for {face:?}"
        );
    }

    // Secondary sanity check, not the regression this test targets: a
    // box has 6 faces. If this fails while the assertions above pass,
    // it points to an OCCT-version/topology change, not a cache-clear
    // regression.
    assert_eq!(faces.len(), 6, "sanity check: a box has 6 faces");

    // Mirror the extracted_faces regression check above for
    // extracted_edges: this is a defense-in-depth gap the exhaustive-
    // destructure type guard alone does not close, since it enforces
    // that every field is *classified* but not that a CLEAR-on-restore
    // field is cleared at the *correct* call site. (Same id-reuse
    // caveat as `faces` above applies: `edges` is not asserted disjoint
    // from `cyl_edges`, only unequal as a whole — see the note above.)
    let edges = kernel_b
        .extract_edges(GeometryHandleId(1))
        .expect("extract_edges on the restored box should succeed");
    assert_ne!(
        edges, cyl_edges,
        "re-extraction after warm-start restore returned the stale cached \
         cylinder-edge list verbatim — extracted_edges was not cleared on restore"
    );
    assert!(
        !edges.is_empty(),
        "re-extraction after warm-start restore should yield at least one edge"
    );
    for edge in &edges {
        assert_eq!(
            kernel_b.query(&GeometryQuery::OwnerBody(*edge)).unwrap(),
            Value::Int(1),
            "post-restore re-extraction should rebuild edge provenance correctly for {edge:?}"
        );
    }
    // Secondary sanity check, not the regression this test targets: a
    // (rectangular) box has 12 edges.
    assert_eq!(edges.len(), 12, "sanity check: a box has 12 edges");

    // Mirror the same check for extracted_vertices.
    let verts = kernel_b
        .extract_vertices(GeometryHandleId(1))
        .expect("extract_vertices on the restored box should succeed");
    assert_ne!(
        verts, cyl_verts,
        "re-extraction after warm-start restore returned the stale cached \
         cylinder-vertex list verbatim — extracted_vertices was not cleared on restore"
    );
    assert!(
        !verts.is_empty(),
        "re-extraction after warm-start restore should yield at least one vertex"
    );
    for vertex in &verts {
        assert_eq!(
            kernel_b.query(&GeometryQuery::OwnerBody(*vertex)).unwrap(),
            Value::Int(1),
            "post-restore re-extraction should rebuild vertex provenance correctly for {vertex:?}"
        );
    }
    // Secondary sanity check, not the regression this test targets: a
    // box has 8 vertices.
    assert_eq!(verts.len(), 8, "sanity check: a box has 8 vertices");
}

#[test]
fn warm_state_producer_does_not_serialize_extraction_provenance() {
    // Kernel A extracts its own box's faces, populating parent_handle
    // (child face id -> parent box id) before any warm-state round-trip.
    let mut kernel_a = OcctKernel::new();
    kernel_a
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();
    let faces_a = kernel_a
        .extract_faces(GeometryHandleId(1))
        .expect("extract_faces on the box should succeed");
    assert!(
        !faces_a.is_empty(),
        "box extraction should yield at least one face"
    );

    // Precondition: kernel_a resolves its own freshly-extracted face's
    // owner correctly, pre-restore.
    assert_eq!(
        kernel_a
            .query(&GeometryQuery::OwnerBody(faces_a[0]))
            .unwrap(),
        Value::Int(1),
        "precondition: kernel_a should resolve its own extracted face's owner"
    );

    // Restore kernel_a's warm state into a FRESH kernel_b — no dirty
    // pre-existing provenance of its own, so a `parent_handle.clear()`
    // on the consumer side would be a no-op regardless. This isolates
    // the *producer* half of the persist/clear classification: it
    // proves `warm_state()` never serialized kernel_a's parent_handle
    // entries into `OcctWarmState` at all, independent of whether the
    // consumer clears on restore (that consumer-side behavior is
    // covered separately by `owner_body_survives_warm_start`, which
    // uses a *dirty* consumer to get a RED-on-main signal).
    let kernel_b = warm_restore(&kernel_a, OcctKernel::new());

    // faces_a[0]'s id is a `parent_handle` key, so (#5162) `warm_state()`
    // filters it out entirely — its shape blob is absent from kernel_b's
    // `shapes`/`reprs` tables, not merely disconnected from its owner.
    // The assertion below still holds regardless of that filtering: it
    // depends only on kernel_b's (always-empty-here) `parent_handle`, not
    // on whether the blob itself round-tripped. If `warm_state()` ever
    // starts serializing `parent_handle`, this fresh kernel_b would
    // silently resolve the owner "correctly" (there is no stale state
    // here to expose the bug the way a dirty consumer would), so this
    // assertion is the only thing pinning the "derived, not persisted"
    // half of the classification.
    let result = kernel_b.query(&GeometryQuery::OwnerBody(faces_a[0]));
    assert!(
        matches!(result, Err(QueryError::QueryFailed(_))),
        "warm_state() must not serialize extraction provenance \
         (parent_handle); got {:?}",
        result
    );
}

/// `shape_count()` reports the number of resident native shapes in the
/// kernel's `shapes` table. A fresh kernel holds zero; after executing two
/// primitives it holds exactly two. This accessor is the observability
/// primitive for the reload native-memory-bound signal (task 5212) — it
/// mirrors the existing `warm_start_failures()` diagnostic accessor — and
/// is the hook the `reset()` boundedness test below reads.
#[test]
fn shape_count_reports_resident_shape_table_size() {
    let mut kernel = OcctKernel::new();
    assert_eq!(
        kernel.shape_count(),
        0,
        "a fresh kernel holds no resident shapes"
    );

    kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();
    kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(20.0),
        })
        .unwrap();

    assert_eq!(
        kernel.shape_count(),
        2,
        "executing two primitives leaves exactly two resident shapes"
    );
}

/// `reset()` part (b): `next_id` stays MONOTONIC across a reset — it is
/// deliberately NOT rewound to 1. A stale handle that outlives a reload
/// (e.g. a lingering realization-cache entry) must resolve to a clean
/// InvalidReference via the 5211 `get_shape` guard, never alias a
/// freshly-minted shape. See `reset()`'s doc comment and INV-BUILD-1.
#[test]
fn reset_keeps_next_id_monotonic() {
    let mut kernel = OcctKernel::new();
    let pre = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();
    let pre_id = pre.id.0;

    kernel.reset();
    assert_eq!(
        kernel.shape_count(),
        0,
        "reset must free the resident shape"
    );

    let post = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(20.0),
        })
        .unwrap();
    assert!(
        post.id.0 > pre_id,
        "next_id must stay monotonic across reset: post-reset id {} must \
         strictly exceed pre-reset id {} (never rewound to 1)",
        post.id.0,
        pre_id
    );
}

/// `reset()` part (c): the reload native-memory bound. Across N
/// execute-batch→reset cycles the resident shape count stays CONSTANT
/// (bounded, not monotonic) while `next_id` strictly increases — proving
/// reset frees native memory every cycle without ever reusing a handle
/// id. Direct analog of
/// `warm_state_persisted_shape_count_stays_bounded_across_cycles`.
#[test]
fn reset_bounds_shape_count_across_execute_reset_cycles() {
    let mut kernel = OcctKernel::new();
    let mut counts = Vec::new();
    let mut first_ids = Vec::new();
    const CYCLES: usize = 5;
    for _ in 0..CYCLES {
        let box_h = kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(10.0),
                size_y: Value::Real(20.0),
                size_z: Value::Real(30.0),
            })
            .unwrap();
        first_ids.push(box_h.id.0);
        let cyl_h = kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();
        kernel
            .execute(&GeometryOp::Union {
                left: box_h.id,
                right: cyl_h.id,
            })
            .unwrap();
        counts.push(kernel.shape_count());
        kernel.reset();
    }

    // The core bound: every batch leaves the same resident count. Without
    // reset this would climb monotonically (3, 6, 9, ...).
    assert!(
        counts.iter().all(|&c| c == counts[0]),
        "resident shape count must stay bounded (constant) across \
         execute→reset cycles: {counts:?}"
    );
    assert!(
        counts[0] > 0,
        "each batch must leave resident shapes so the bound is \
         non-vacuous: {counts:?}"
    );
    // next_id is monotonic: each cycle's first (box) id strictly exceeds
    // the previous cycle's, so reset never rewinds the counter.
    assert!(
        first_ids.windows(2).all(|w| w[1] > w[0]),
        "next_id must strictly increase across reset cycles (handle ids \
         never reused): {first_ids:?}"
    );
}

#[test]
fn extract_vertices_box_returns_eight_handles_with_brepkind_vertex() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.010),
            size_y: Value::Real(0.010),
            size_z: Value::Real(0.010),
        })
        .expect("Box should succeed");
    let box_id = box_h.id;
    let vertices = kernel
        .extract_vertices(box_id)
        .expect("extract_vertices on a valid box should succeed");
    assert_eq!(
        vertices.len(),
        8,
        "a box should have 8 vertices, got {}",
        vertices.len()
    );
    for v in &vertices {
        assert_eq!(
            kernel.repr_of(*v),
            Some(BRepKind::Vertex),
            "each extracted vertex should have BRepKind::Vertex, got {:?} for {:?}",
            kernel.repr_of(*v),
            v
        );
    }
}

#[test]
fn extract_vertices_idempotency_returns_same_handles() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.010),
            size_y: Value::Real(0.010),
            size_z: Value::Real(0.010),
        })
        .expect("Box should succeed");
    let box_id = box_h.id;
    let v1 = kernel
        .extract_vertices(box_id)
        .expect("first extract_vertices should succeed");
    let v2 = kernel
        .extract_vertices(box_id)
        .expect("second extract_vertices should succeed");
    assert_eq!(
        v1, v2,
        "extract_vertices must be idempotent: second call must return same ids in same order"
    );
}

#[test]
fn extract_vertices_invalidates_cache_on_warm_state() {
    // Build a box, extract vertices (populates cache), then apply warm-state
    // to the *same* kernel so the cache is non-empty when `with_warm_state`
    // runs.  After the restore, re-extracting must mint fresh ids — proving
    // that `extracted_vertices.clear()` inside `with_warm_state` is the
    // load-bearing line (a version without the clear would return the stale
    // cached vec rather than new ids).
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.010),
            size_y: Value::Real(0.010),
            size_z: Value::Real(0.010),
        })
        .expect("Box should succeed");
    let box_id = box_h.id;

    // Populate the cache.
    let vec1 = kernel
        .extract_vertices(box_id)
        .expect("extract_vertices before warm-state should succeed");
    assert_eq!(vec1.len(), 8, "should yield 8 vertices before warm restore");

    // Snapshot, then restore onto the *same* kernel (cache is now non-empty).
    let state = kernel
        .warm_state()
        .expect("kernel should produce warm state");
    kernel.with_warm_state(state);

    // Re-extract: the clear() in with_warm_state must have evicted the cache,
    // so this call mints 8 fresh ids instead of returning the cached vec1.
    let vec2 = kernel
        .extract_vertices(box_id)
        .expect("extract_vertices after warm-state restore should succeed");
    assert_eq!(vec2.len(), 8, "should yield 8 vertices after warm restore");

    // Ids must be disjoint: warm-state restore clears extracted_vertices,
    // so the re-extract mints fresh ids starting from warm.next_id (strictly
    // greater than any id ever minted before the snapshot).
    let set1: std::collections::HashSet<GeometryHandleId> = vec1.iter().copied().collect();
    let set2: std::collections::HashSet<GeometryHandleId> = vec2.iter().copied().collect();
    let intersection: std::collections::HashSet<_> = set1.intersection(&set2).collect();
    assert!(
        intersection.is_empty(),
        "warm-state restore should clear the extracted_vertices cache so fresh ids are minted; \
         found shared ids: {intersection:?}"
    );
}

#[test]
fn with_warm_state_ignores_wrong_type() {
    let mut kernel = OcctKernel::new();
    // Pass a String instead of OcctWarmState — should be silently ignored
    kernel.with_warm_state(OpaqueState::new(String::from("garbage"), 8));
    // Kernel should still function normally
    let gh = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();
    let vol = kernel.query(&GeometryQuery::Volume(gh.id)).unwrap();
    match vol {
        Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "vol: {v}"),
        other => panic!("expected Real, got {:?}", other),
    }
}

#[test]
fn with_warm_state_all_valid_zero_failures() {
    // Create a kernel with a box and extract its warm state
    let mut source = OcctKernel::new();
    source
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();
    let state = source.warm_state().expect("should have warm state");

    // Apply that fully-valid warm state to a fresh kernel
    let mut kernel = OcctKernel::new();
    kernel.with_warm_state(state);

    // All shapes were valid, so failure count should be 0
    assert_eq!(
        kernel.warm_start_failures(),
        0,
        "fully valid warm state should report 0 failures"
    );

    // Verify the shape was actually restored
    let vol = kernel
        .query(&GeometryQuery::Volume(GeometryHandleId(1)))
        .unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 6000.0).abs() < 1.0,
                "box volume should be ~6000, got {v}"
            );
        }
        other => panic!("expected Real, got {:?}", other),
    }
}
