//! Integration test for `OcctKernelHandle::boolean_{fuse,cut,common}_with_history` —
//! the v0.2 persistent-naming-v2 BRepAlgoAPI history-tracking primitives
//! (task 2590, step-13 and task 2656, steps 2 and 4).
//!
//! Exercises the FFI primitives that wrap `BRepAlgoAPI_Fuse/Cut/Common::Modified()`,
//! `Generated()`, and `IsDeleted()` and expose the per-parent / per-result
//! sub-shape correspondence for face and edge topology. This is the foundation
//! the propagation helper in `reify-eval/src/topology_attribute_propagation.rs`
//! consumes to copy parent topology attributes onto result handles after a
//! constructive boolean.
//!
//! Shared helpers:
//! - `setup_two_overlapping_boxes()` — spawn a kernel + two 10mm cubes with
//!   +5mm X offset for a non-trivial 5mm-overlap geometry.
//! - `assert_boolean_history_well_formed()` — common well-formedness checks
//!   (parent_index range, subshape index range, silent_drop_count == 0, etc.).
//!   Op-specific assertions (both-parents vs left-only) remain in each test.
//!
//! Gated on `OCCT_AVAILABLE`: the test bails out early in builds without OCCT,
//! mirroring the pattern used by the other `crates/reify-kernel-occt/tests/*`
//! integration files.

#![cfg(has_occt)]

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_ir::{
    BooleanOpHistoryRecords, GeometryHandleId, GeometryOp, GeometryQuery, Value,
};

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

/// Build two overlapping 10mm boxes on a freshly spawned kernel.
///
/// Box A is a 10mm cube at the origin.  Box B is the same shape translated
/// +5mm in X, so the two boxes share a [0, 5mm]×[-5mm, +5mm]×[-5mm, +5mm]
/// half-box overlap.  Fuse (A∪B), cut (A−B), and common (A∩B) all yield
/// non-trivial results with this setup: volume > 0 and faces from at least
/// the left parent in the history.
///
/// Returns `(kernel, left_id, right_id)`.  The kernel must stay bound for the
/// duration of any subsequent kernel calls that use the returned handle IDs.
fn setup_two_overlapping_boxes() -> (OcctKernelHandle, GeometryHandleId, GeometryHandleId) {
    let kernel = OcctKernelHandle::spawn();
    let left = kernel
        .execute(&ten_mm_box_op())
        .expect("left box should build");
    let right_origin = kernel
        .execute(&ten_mm_box_op())
        .expect("right box origin should build");
    let right = kernel
        .execute(&GeometryOp::Translate {
            target: right_origin.id,
            dx: 5.0e-3,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("right translate should build");
    (kernel, left.id, right.id)
}

/// Assert that `history` is well-formed for a boolean operation whose result
/// solid has `result_face_count` faces and `result_edge_count` edges.
///
/// Checks shared by all three boolean ops (fuse / cut / common):
/// - face and edge Modified+Generated records are non-empty;
/// - `parent_index ∈ {0, 1}` for every record;
/// - parent face `subshape_index < 6` (a box has 6 faces);
/// - parent edge `subshape_index < 12` (a box has 12 edges);
/// - result subshape indices are in range;
/// - deleted records carry valid parent indices;
/// - `silent_drop_count == 0`.
///
/// Op-specific assertions — e.g. fuse requires both parents present whereas
/// cut/common only requires the left parent — are left to the individual tests.
fn assert_boolean_history_well_formed(
    history: &BooleanOpHistoryRecords,
    result_face_count: u32,
    result_edge_count: u32,
) {
    // Face Modified/Generated must be non-empty.
    assert!(
        !history.face_modified.is_empty() || !history.face_generated.is_empty(),
        "history should have non-empty Modified or Generated face records; \
         got {} modified, {} generated",
        history.face_modified.len(),
        history.face_generated.len()
    );

    // Each face record must be well-formed.
    for r in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        assert!(
            r.parent_index < 2,
            "face parent_index must be 0 or 1, got {}",
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

    // Face deleted records (if any) must carry valid parent indices.
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

    // Edge Modified/Generated must be non-empty.
    assert!(
        !history.edge_modified.is_empty() || !history.edge_generated.is_empty(),
        "history should have non-empty Modified or Generated edge records; \
         got {} modified, {} generated",
        history.edge_modified.len(),
        history.edge_generated.len()
    );

    // Each edge record must be well-formed (a box has 12 edges).
    for r in history
        .edge_modified
        .iter()
        .chain(history.edge_generated.iter())
    {
        assert!(
            r.parent_index < 2,
            "edge parent_index must be 0 or 1, got {}",
            r.parent_index
        );
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

    // Edge deleted records (if any) must carry valid parent indices.
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

    // Silent-drop counter must be zero.
    assert_eq!(
        history.silent_drop_count,
        0,
        "should not silently drop any Modified/Generated child; got {}",
        history.silent_drop_count
    );

    // The Modified+Generated vectors must be non-empty so the
    // silent_drop_count==0 check above is meaningful (i.e. it would catch a
    // wholesale-drop regression where the counter is zero because no records
    // were emitted at all).
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

// ---------------------------------------------------------------------------
// Test: BRepAlgoAPI_Fuse
// ---------------------------------------------------------------------------

/// `BRepAlgoAPI_Fuse` history exposes Modified/Generated/Deleted for each
/// parent face and edge.  Uses `setup_two_overlapping_boxes` (two 10mm cubes,
/// right offset +5mm in X) and `assert_boolean_history_well_formed` for the
/// shared well-formedness checks, then adds fuse-specific assertions:
/// - `face_modified` must be specifically non-empty (both parents' surviving
///   outer faces appear via Modified in `BRepAlgoAPI_Fuse`);
/// - at least one face record from EACH parent (both parents must appear so
///   propagation can correctly route per-parent attributes).
///
/// Part of v0.2 persistent-naming-v2 (task 2590, step-14).
#[test]
fn boolean_fuse_with_history_reports_per_parent_face_and_edge_records() {
    if !OCCT_AVAILABLE {
        return;
    }
    let (kernel, left_id, right_id) = setup_two_overlapping_boxes();

    let (result_handle, history) = kernel
        .boolean_fuse_with_history(left_id, right_id)
        .expect("boolean_fuse_with_history should succeed for two overlapping boxes");

    // Result is a non-empty solid.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the fused result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "fused result must have positive volume, got {vol_si}"
    );

    // Fuse-specific: face_modified must be non-empty (not just the disjunction),
    // because BRepAlgoAPI_Fuse tracks both parents' surviving faces via Modified.
    assert!(
        !history.face_modified.is_empty(),
        "BRepAlgoAPI_Fuse history.face_modified should be non-empty for two \
         overlapping boxes — got {} records",
        history.face_modified.len()
    );

    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the fused result should succeed");
    let result_edges = kernel
        .extract_edges(result_handle)
        .expect("extract_edges on the fused result should succeed");
    assert_boolean_history_well_formed(
        &history,
        result_faces.len() as u32,
        result_edges.len() as u32,
    );

    // Fuse-specific: at least one face from EACH parent must appear.
    // If only the left parent appeared, propagation would mis-attribute every
    // result face to the left operand only.
    let face_records: Vec<_> = history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
        .collect();
    assert!(
        face_records.iter().any(|r| r.parent_index == 0),
        "at least one face history record should originate from the left parent; \
         got {face_records:?}"
    );
    assert!(
        face_records.iter().any(|r| r.parent_index == 1),
        "at least one face history record should originate from the right parent; \
         got {face_records:?}"
    );
}

// ---------------------------------------------------------------------------
// Test: BRepAlgoAPI_Cut
// ---------------------------------------------------------------------------

/// `BRepAlgoAPI_Cut` history exposes Modified/Generated/Deleted for each
/// parent face and edge.  Uses `setup_two_overlapping_boxes` and
/// `assert_boolean_history_well_formed`; adds cut-specific assertion:
/// the left parent (object A) must appear in the face history.  OCCT Cut does
/// not track cut-boundary faces from the tool (B) via Generated() — only the
/// object's (A's) surviving faces appear in Modified — so only
/// `parent_index == 0` is asserted.
///
/// Part of v0.2 persistent-naming-v2 (task 2656, step-2).
#[test]
fn boolean_cut_with_history_reports_per_parent_face_and_edge_records() {
    if !OCCT_AVAILABLE {
        return;
    }
    let (kernel, left_id, right_id) = setup_two_overlapping_boxes();

    // left − right: A with a corner notch cut out (volume ≈ 500 mm³ > 0).
    let (result_handle, history) = kernel
        .boolean_cut_with_history(left_id, right_id)
        .expect("boolean_cut_with_history should succeed for two overlapping boxes");

    // Result is a non-empty solid.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the cut result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "cut result must have positive volume, got {vol_si}"
    );

    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the cut result should succeed");
    let result_edges = kernel
        .extract_edges(result_handle)
        .expect("extract_edges on the cut result should succeed");
    assert_boolean_history_well_formed(
        &history,
        result_faces.len() as u32,
        result_edges.len() as u32,
    );

    // Cut-specific: left parent (A, the object) must appear in the history.
    let face_records: Vec<_> = history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
        .collect();
    assert!(
        face_records.iter().any(|r| r.parent_index == 0),
        "at least one face history record should originate from the left parent (A); \
         got {face_records:?}"
    );
}

// ---------------------------------------------------------------------------
// Test: BRepAlgoAPI_Common
// ---------------------------------------------------------------------------

/// `BRepAlgoAPI_Common` history exposes Modified/Generated/Deleted for each
/// parent face and edge.  Uses `setup_two_overlapping_boxes` and
/// `assert_boolean_history_well_formed`; adds common-specific assertion:
/// the left parent (A) must appear in the face history.  Similar to Cut, OCCT
/// Common may not report Generated records from the right parent (B); only
/// `parent_index == 0` is asserted.
///
/// Part of v0.2 persistent-naming-v2 (task 2656, step-4).
#[test]
fn boolean_common_with_history_reports_per_parent_face_and_edge_records() {
    if !OCCT_AVAILABLE {
        return;
    }
    let (kernel, left_id, right_id) = setup_two_overlapping_boxes();

    // A ∩ B = overlap half-box [0, 5mm]×[-5mm, +5mm]×[-5mm, +5mm] (vol ≈ 500 mm³).
    let (result_handle, history) = kernel
        .boolean_common_with_history(left_id, right_id)
        .expect("boolean_common_with_history should succeed for two overlapping boxes");

    // Result is a non-empty solid.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the common result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "common result must have positive volume, got {vol_si}"
    );

    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the common result should succeed");
    let result_edges = kernel
        .extract_edges(result_handle)
        .expect("extract_edges on the common result should succeed");
    assert_boolean_history_well_formed(
        &history,
        result_faces.len() as u32,
        result_edges.len() as u32,
    );

    // Common-specific: like Cut, OCCT may not report Generated records from
    // the right parent (B). Assert only that the left parent (A) appears.
    let face_records: Vec<_> = history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
        .collect();
    assert!(
        face_records.iter().any(|r| r.parent_index == 0),
        "at least one face history record should originate from the left parent (A); \
         got {face_records:?}"
    );
}
