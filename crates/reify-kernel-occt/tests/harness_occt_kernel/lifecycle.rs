//! `OcctKernel` state lifecycle: the warm-start trait no-op, `reset`, the
//! resident shape count, and the vertex-extraction cache.

#![cfg(has_occt)]

use reify_ir::{BRepKind, GeometryOp, GeometryQuery, OpaqueState, Value, WarmStartable};
use reify_kernel_occt::OcctKernel;

/// The OCCT kernel carries no warm state: `warm_state` is `None` on a fresh
/// kernel and on one holding shapes, and `with_warm_state` — even handed a
/// foreign payload — leaves the shape table, the repr table, extraction
/// provenance, the extraction cache and id minting exactly as they were.
#[test]
fn warm_state_is_none_and_with_warm_state_leaves_the_kernel_untouched() {
    let mut kernel = OcctKernel::new();
    assert!(
        kernel.warm_state().is_none(),
        "a fresh kernel has no warm state"
    );

    let box_id = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap()
        .id;
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces on a valid box should succeed");
    assert!(!faces.is_empty(), "a box has faces to extract");

    let count = kernel.shape_count();
    let repr = kernel.repr_of(box_id);
    let owner = kernel
        .query(&GeometryQuery::OwnerBody(faces[0]))
        .expect("the extracted face has an owner");
    assert!(count > 0, "the box and its faces are resident");
    assert_eq!(repr, Some(BRepKind::Solid), "the box is a solid");
    assert_eq!(
        owner,
        Value::Int(1),
        "the extracted face resolves to its owning box"
    );

    assert!(
        kernel.warm_state().is_none(),
        "a kernel holding shapes still has no warm state"
    );

    kernel.with_warm_state(OpaqueState::new(String::from("foreign"), 8));

    assert_eq!(
        kernel.shape_count(),
        count,
        "with_warm_state must not touch the shape table"
    );
    assert_eq!(
        kernel.repr_of(box_id),
        repr,
        "with_warm_state must not touch the repr table"
    );
    assert_eq!(
        kernel
            .query(&GeometryQuery::OwnerBody(faces[0]))
            .expect("the extracted face still has an owner"),
        owner,
        "with_warm_state must not touch extraction provenance"
    );
    assert_eq!(
        kernel
            .extract_faces(box_id)
            .expect("re-extraction should succeed"),
        faces,
        "with_warm_state must not clear the extraction cache: re-extraction \
         returns the same ids"
    );
    match kernel.query(&GeometryQuery::Volume(box_id)) {
        Ok(Value::Real(v)) => assert!((v - 6000.0).abs() < 1.0, "box volume: {v}"),
        other => panic!("expected Ok(Value::Real), got {other:?}"),
    }

    let later = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(20.0),
        })
        .unwrap()
        .id;
    let highest_earlier = faces
        .iter()
        .chain(std::iter::once(&box_id))
        .map(|id| id.0)
        .max()
        .unwrap();
    assert!(
        later.0 > highest_earlier,
        "id minting continues past every earlier id: minted {} after {}",
        later.0,
        highest_earlier
    );
}

/// `shape_count()` reports the number of resident native shapes. A fresh
/// kernel holds zero; after executing two primitives it holds exactly two.
/// It is the observable the `reset()` boundedness test below reads.
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
/// id.
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
