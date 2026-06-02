//! Integration test for `OcctKernelHandle::boolean_fuse_with_history` —
//! the v0.2 persistent-naming-v2 BRepAlgoAPI history-tracking primitive
//! (task 2590, step-13).
//!
//! Exercises the FFI primitive that wraps `BRepAlgoAPI_Fuse::Modified()`,
//! `Generated()`, and `IsDeleted()` and exposes the per-parent / per-result
//! sub-shape correspondence for face and edge topology. This is the foundation
//! the propagation helper in `reify-eval/src/topology_attribute_propagation.rs`
//! consumes to copy parent topology attributes onto result handles after a
//! constructive boolean.
//!
//! Gated on `OCCT_AVAILABLE`: the test bails out early in builds without OCCT,
//! mirroring the pattern used by the other `crates/reify-kernel-occt/tests/*`
//! integration files.

#![cfg(has_occt)]

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_ir::{GeometryOp, GeometryQuery, Value};

/// 10×10×10 mm box, expressed in SI metres at the kernel boundary.
const BOX_SIDE_M: f64 = 10.0e-3;

/// Build the `GeometryOp::Box` for a 10mm cube.
fn ten_mm_box_op() -> GeometryOp {
    GeometryOp::Box {
        width: Value::Real(BOX_SIDE_M),
        height: Value::Real(BOX_SIDE_M),
        depth: Value::Real(BOX_SIDE_M),
    }
}

/// `BRepAlgoAPI_Fuse` history exposes Modified/Generated/Deleted for each
/// parent face and edge. The test:
/// - builds two overlapping 10mm cubes (right offset by 5mm in +X so the
///   fuse has both shared and outer faces);
/// - calls `boolean_fuse_with_history` on the actor handle;
/// - asserts the result is a non-empty solid (volume > 0);
/// - asserts at least one face from EACH parent appears in the history
///   (Modified ∪ Generated), pinning that propagation can correctly route
///   per-parent attributes;
/// - asserts every record carries an in-range parent_index (∈ {0, 1}) and
///   a parent_subshape_index < 6 (a box has 6 faces);
/// - asserts edges history is similarly non-empty.
///
/// Compilation/linkage of this test pins step-14: it will fail to build
/// until the FFI primitive + Rust handle method ship.
#[test]
fn boolean_fuse_with_history_reports_per_parent_face_and_edge_records() {
    if !OCCT_AVAILABLE {
        return;
    }

    let kernel = OcctKernelHandle::spawn();

    // Box A: 10mm cube centered at origin.
    let left = kernel
        .execute(&ten_mm_box_op())
        .expect("left box should build");
    // Box B: same shape, translated by +5mm in X so the two boxes overlap
    // by exactly half. Centered-at-origin make_box puts the original at
    // [-5mm, +5mm]^3 in metres; the translated copy lives at
    // [0, +10mm]×[-5mm,+5mm]×[-5mm,+5mm], so the overlap is the half-box
    // [0, 5mm]×[-5mm,+5mm]×[-5mm,+5mm] and the fuse is non-trivial.
    let right_origin = kernel
        .execute(&ten_mm_box_op())
        .expect("right box should build");
    let right = kernel
        .execute(&GeometryOp::Translate {
            target: right_origin.id,
            dx: 5.0e-3,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("right translate should build");

    let (result_handle, history) = kernel
        .boolean_fuse_with_history(left.id, right.id)
        .expect("boolean_fuse_with_history should succeed for two overlapping boxes");

    // (c) Result is a non-empty solid.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the fused result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "fused result must have positive volume, got {vol_si}"
    );

    // (d) Modified history is non-empty (parents' un-cut faces map to
    //     fused-result faces).
    assert!(
        !history.face_modified.is_empty(),
        "BRepAlgoAPI_Fuse history.face_modified should be non-empty for two \
         overlapping boxes — got {} records",
        history.face_modified.len()
    );

    // (e) Each face record is well-formed: parent_index ∈ {0, 1},
    //     parent_subshape_index < 6 (a box has 6 faces).
    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the fused result should succeed");
    let result_face_count = result_faces.len() as u32;
    for r in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        assert!(
            r.parent_index < 2,
            "parent_index must be 0 or 1, got {}",
            r.parent_index
        );
        assert!(
            r.parent_subshape_index < 6,
            "parent face index must be < 6 for a box, got {}",
            r.parent_subshape_index
        );
        assert!(
            r.result_subshape_index < result_face_count,
            "result face index {} out of range; result has {} faces",
            r.result_subshape_index,
            result_face_count
        );
    }

    // (f) Asymmetric assertion: at least one face from each parent appears
    //     in the history. (If we accidentally pointed Modified at only the
    //     left, the propagation helper would mis-attribute every result
    //     face.)
    let face_records: Vec<_> = history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
        .collect();
    assert!(
        face_records.iter().any(|r| r.parent_index == 0),
        "at least one face history record should originate from the left parent, got {face_records:?}"
    );
    assert!(
        face_records.iter().any(|r| r.parent_index == 1),
        "at least one face history record should originate from the right parent, got {face_records:?}"
    );

    // (g) Edges history is similarly non-empty and well-formed.
    let result_edges = kernel
        .extract_edges(result_handle)
        .expect("extract_edges on the fused result should succeed");
    let result_edge_count = result_edges.len() as u32;
    assert!(
        !history.edge_modified.is_empty(),
        "BRepAlgoAPI_Fuse history.edge_modified should be non-empty for two \
         overlapping boxes — got {} records",
        history.edge_modified.len()
    );
    for r in history
        .edge_modified
        .iter()
        .chain(history.edge_generated.iter())
    {
        assert!(
            r.parent_index < 2,
            "edge record parent_index must be 0 or 1, got {}",
            r.parent_index
        );
        // A box has 12 edges.
        assert!(
            r.parent_subshape_index < 12,
            "parent edge index must be < 12 for a box, got {}",
            r.parent_subshape_index
        );
        assert!(
            r.result_subshape_index < result_edge_count,
            "result edge index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }

    // Deleted records (if any) must reference valid parent faces / edges.
    for d in &history.face_deleted {
        assert!(
            d.parent_index < 2,
            "face deleted parent_index must be 0 or 1, got {}",
            d.parent_index
        );
        assert!(
            d.parent_subshape_index < 6,
            "face deleted parent index must be < 6 for a box, got {}",
            d.parent_subshape_index
        );
    }
    for d in &history.edge_deleted {
        assert!(
            d.parent_index < 2,
            "edge deleted parent_index must be 0 or 1, got {}",
            d.parent_index
        );
        assert!(
            d.parent_subshape_index < 12,
            "edge deleted parent index must be < 12 for a box, got {}",
            d.parent_subshape_index
        );
    }

    // (h) Silent-drop counter must be zero for a well-formed 5mm-overlap fuse:
    //     every Modified/Generated child must be resolvable in the result map.
    assert_eq!(
        history.silent_drop_count, 0,
        "vanilla overlap fuse should not silently drop any Modified/Generated child \
         — got {}",
        history.silent_drop_count
    );

    // (i) The Modified+Generated record vectors must be non-empty so that the
    //     silent_drop_count==0 assertion above is meaningful (i.e. the counter
    //     would have caught a wholesale-drop regression).
    assert!(
        history.face_modified.len()
            + history.face_generated.len()
            + history.edge_modified.len()
            + history.edge_generated.len()
            > 0,
        "Modified+Generated record vectors should be non-empty so the \
         silent_drop_count==0 assertion is meaningful"
    );
}

/// `BRepAlgoAPI_Cut` history exposes Modified/Generated/Deleted for each
/// parent face and edge. The test:
/// - builds two overlapping 10mm cubes (right offset by +5mm in X so the cut
///   A−B yields A with a corner notch);
/// - calls `boolean_cut_with_history` on the actor handle (left − right);
/// - asserts the result is a non-empty solid (volume > 0);
/// - asserts at least one face from EACH parent appears in the history
///   (Modified ∪ Generated), pinning that propagation can correctly route
///   per-parent attributes;
/// - asserts every record carries an in-range parent_index (∈ {0, 1}) and
///   a parent_subshape_index < 6 (a box has 6 faces) or < 12 (a box has 12
///   edges);
/// - asserts edges history is similarly non-empty and well-formed;
/// - asserts silent_drop_count == 0.
///
/// Part of v0.2 persistent-naming-v2 (task 2656, step-2).
#[test]
fn boolean_cut_with_history_reports_per_parent_face_and_edge_records() {
    if !OCCT_AVAILABLE {
        return;
    }

    let kernel = OcctKernelHandle::spawn();

    // Box A: 10mm cube centered at origin.
    let left = kernel
        .execute(&ten_mm_box_op())
        .expect("left box should build");
    // Box B: same shape, translated by +5mm in X so the two boxes overlap
    // by exactly half. A−B yields the left half of A with a corner notch cut
    // out (volume ≈ 500 mm³ > 0).
    let right_origin = kernel
        .execute(&ten_mm_box_op())
        .expect("right box should build");
    let right = kernel
        .execute(&GeometryOp::Translate {
            target: right_origin.id,
            dx: 5.0e-3,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("right translate should build");

    let (result_handle, history) = kernel
        .boolean_cut_with_history(left.id, right.id)
        .expect("boolean_cut_with_history should succeed for two overlapping boxes");

    // (c) Result is a non-empty solid.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the cut result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "cut result must have positive volume, got {vol_si}"
    );

    // (d) Modified history is non-empty (A's un-cut faces remain; B's
    //     intersection faces become new boundary walls in the result).
    assert!(
        !history.face_modified.is_empty() || !history.face_generated.is_empty(),
        "BRepAlgoAPI_Cut history should have non-empty Modified or Generated face records \
         for two overlapping boxes — got {} modified, {} generated",
        history.face_modified.len(),
        history.face_generated.len()
    );

    // (e) Each face record is well-formed: parent_index ∈ {0, 1},
    //     parent_subshape_index < 6 (a box has 6 faces).
    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the cut result should succeed");
    let result_face_count = result_faces.len() as u32;
    for r in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        assert!(
            r.parent_index < 2,
            "parent_index must be 0 or 1, got {}",
            r.parent_index
        );
        assert!(
            r.parent_subshape_index < 6,
            "parent face index must be < 6 for a box, got {}",
            r.parent_subshape_index
        );
        assert!(
            r.result_subshape_index < result_face_count,
            "result face index {} out of range; result has {} faces",
            r.result_subshape_index,
            result_face_count
        );
    }

    // (f) At least one face from each parent appears in the history.
    let face_records: Vec<_> = history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
        .collect();
    assert!(
        face_records.iter().any(|r| r.parent_index == 0),
        "at least one face history record should originate from the left parent (A), \
         got {face_records:?}"
    );
    assert!(
        face_records.iter().any(|r| r.parent_index == 1),
        "at least one face history record should originate from the right parent (B), \
         got {face_records:?}"
    );

    // (g) Edges history is non-empty and well-formed.
    let result_edges = kernel
        .extract_edges(result_handle)
        .expect("extract_edges on the cut result should succeed");
    let result_edge_count = result_edges.len() as u32;
    assert!(
        !history.edge_modified.is_empty() || !history.edge_generated.is_empty(),
        "BRepAlgoAPI_Cut history should have non-empty Modified or Generated edge records \
         — got {} modified, {} generated",
        history.edge_modified.len(),
        history.edge_generated.len()
    );
    for r in history
        .edge_modified
        .iter()
        .chain(history.edge_generated.iter())
    {
        assert!(
            r.parent_index < 2,
            "edge record parent_index must be 0 or 1, got {}",
            r.parent_index
        );
        // A box has 12 edges.
        assert!(
            r.parent_subshape_index < 12,
            "parent edge index must be < 12 for a box, got {}",
            r.parent_subshape_index
        );
        assert!(
            r.result_subshape_index < result_edge_count,
            "result edge index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }

    // Deleted records (if any) must reference valid parent faces / edges.
    for d in &history.face_deleted {
        assert!(
            d.parent_index < 2,
            "face deleted parent_index must be 0 or 1, got {}",
            d.parent_index
        );
        assert!(
            d.parent_subshape_index < 6,
            "face deleted parent index must be < 6 for a box, got {}",
            d.parent_subshape_index
        );
    }
    for d in &history.edge_deleted {
        assert!(
            d.parent_index < 2,
            "edge deleted parent_index must be 0 or 1, got {}",
            d.parent_index
        );
        assert!(
            d.parent_subshape_index < 12,
            "edge deleted parent index must be < 12 for a box, got {}",
            d.parent_subshape_index
        );
    }

    // (h) Silent-drop counter must be zero for a well-formed 5mm-overlap cut.
    assert_eq!(
        history.silent_drop_count, 0,
        "vanilla overlap cut should not silently drop any Modified/Generated child \
         — got {}",
        history.silent_drop_count
    );

    // (i) The Modified+Generated record vectors must be non-empty so that the
    //     silent_drop_count==0 assertion above is meaningful.
    assert!(
        history.face_modified.len()
            + history.face_generated.len()
            + history.edge_modified.len()
            + history.edge_generated.len()
            > 0,
        "Modified+Generated record vectors should be non-empty so the \
         silent_drop_count==0 assertion is meaningful"
    );
}
