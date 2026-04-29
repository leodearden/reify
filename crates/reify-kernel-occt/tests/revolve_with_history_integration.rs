//! Integration test for `OcctKernelHandle::revolve_with_history` —
//! the v0.2 persistent-naming-v2 sweep history-tracking primitive
//! for `BRepPrimAPI_MakeRevol` (task 5a / #2573, step-9).
//!
//! Exercises the FFI primitive that wraps `BRepPrimAPI_MakeRevol::Modified()`,
//! `Generated()`, `IsDeleted()`, `FirstShape()`, and `LastShape()` and
//! exposes the per-parent face/edge correspondence plus the cap-face
//! identification (start/end caps from FirstShape/LastShape).
//!
//! Two scenarios are pinned:
//! - PARTIAL revolve (180°) — both `FirstShape()` and `LastShape()` produce
//!   distinct cap faces, so `start_cap_face_indices` and
//!   `end_cap_face_indices` each contain exactly one entry.
//! - FULL revolve (360°) — `FirstShape()` and `LastShape()` reference the
//!   same closed surface, so BOTH cap-index lists are empty (no caps).
//!
//! Mirrors the structure of `extrude_with_history_integration.rs`: gated on
//! `OCCT_AVAILABLE` and `#![cfg(has_occt)]` so non-OCCT builds skip without
//! linker errors.

#![cfg(has_occt)]

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::GeometryQuery;

/// 5×10mm rectangular face profile, expressed in SI metres. Centered at
/// `(17.5mm, 0, 0)` so its left edge sits at x=15mm — clear of the z-axis,
/// which would otherwise produce a degenerate revolved solid.
const RECT_WIDTH_M: f64 = 5.0e-3;
const RECT_HEIGHT_M: f64 = 10.0e-3;
const RECT_CX_M: f64 = 17.5e-3;

/// Build a 5×10mm rect_face profile at `cx=17.5mm` (left edge at x=15mm).
/// The kernel-thread fixture method `make_rect_profile_at_for_test` is
/// gated on the `test-fixtures` cargo feature; this is the canonical way
/// to construct a planar profile for sweep history integration tests.
fn make_offset_rect_profile(kernel: &mut OcctKernelHandle) -> reify_types::GeometryHandleId {
    kernel
        .make_rect_profile_at_for_test(RECT_WIDTH_M, RECT_HEIGHT_M, RECT_CX_M, 0.0, 0.0)
        .expect("offset rect profile should build")
}

/// `BRepPrimAPI_MakeRevol` (PARTIAL — 180°): the test:
/// - builds a 5×10mm rect profile offset to x=17.5mm (left edge at 15mm);
/// - calls `revolve_with_history(profile, axis_origin=[0,0,0],
///   axis_dir=[0,0,1], angle_rad=π)`;
/// - asserts the result is a positive-volume solid;
/// - asserts the cap-index lists each contain exactly one entry (one face
///   per cap for a rect profile under partial revolution);
/// - asserts at least 4 generated face records (4 profile edges → 4 lateral
///   revolved faces);
/// - asserts every record references in-range profile and result indices.
///
/// Compilation/linkage of this test pins step-10: it will fail to build
/// until the FFI primitive + Rust handle method ship.
#[test]
fn partial_revolve_with_history_reports_caps_and_revolved_face_records() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let profile_id = make_offset_rect_profile(&mut kernel);

    // 180° partial revolve about the +Z axis at the origin.
    let (result_handle, history) = kernel
        .revolve_with_history(
            profile_id,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            std::f64::consts::PI,
        )
        .expect("revolve_with_history should succeed for a partial revolve");

    // (a) Result is a positive-volume solid.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the partial revolve result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "partial revolved solid must have positive volume, got {vol_si}"
    );

    // (b) Start cap: profile face → exactly one start cap face under partial revolution.
    assert_eq!(
        history.start_cap_face_indices.len(),
        1,
        "expected 1 start cap face for a partial revolution of a rect profile, got {} ({:?})",
        history.start_cap_face_indices.len(),
        history.start_cap_face_indices
    );

    // (c) End cap: same — exactly one end cap face under partial revolution.
    assert_eq!(
        history.end_cap_face_indices.len(),
        1,
        "expected 1 end cap face for a partial revolution of a rect profile, got {} ({:?})",
        history.end_cap_face_indices.len(),
        history.end_cap_face_indices
    );

    // (d) face_generated: at least 4 lateral faces (one per profile edge).
    assert!(
        history.face_generated.len() >= 4,
        "expected ≥4 generated faces (4 profile edges → ≥4 lateral revolved faces), \
         got {} ({:?})",
        history.face_generated.len(),
        history.face_generated
    );

    // (e) Every generated record has parent_index=0 and an in-range
    //     parent_subshape_index for the profile (rect has 4 edges, 4 vertices).
    for r in &history.face_generated {
        assert_eq!(
            r.parent_index, 0,
            "sweep records always have parent_index=0, got {}",
            r.parent_index
        );
        assert!(
            r.parent_subshape_index < 8, // 4 edges + 4 vertices, cap-loose bound
            "parent_subshape_index {} out of range for rect profile (≥8)",
            r.parent_subshape_index
        );
    }

    // (f) Result-side indices must be within the result face/edge maps.
    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the partial revolve result should succeed");
    let result_face_count = result_faces.len() as u32;
    for r in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        assert!(
            r.result_subshape_index < result_face_count,
            "face record result_subshape_index {} out of range; result has {} faces",
            r.result_subshape_index,
            result_face_count
        );
    }
    for &cap_idx in history
        .start_cap_face_indices
        .iter()
        .chain(history.end_cap_face_indices.iter())
    {
        assert!(
            cap_idx < result_face_count,
            "cap face index {} out of range; result has {} faces",
            cap_idx,
            result_face_count
        );
    }

    let result_edges = kernel
        .extract_edges(result_handle)
        .expect("extract_edges on the partial revolve result should succeed");
    let result_edge_count = result_edges.len() as u32;
    for r in history
        .edge_modified
        .iter()
        .chain(history.edge_generated.iter())
    {
        assert_eq!(
            r.parent_index, 0,
            "sweep edge record parent_index must be 0, got {}",
            r.parent_index
        );
        assert!(
            r.result_subshape_index < result_edge_count,
            "edge record result_subshape_index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }
}

/// `BRepPrimAPI_MakeRevol` (FULL — 360°): under full revolution
/// `FirstShape()` and `LastShape()` reference the same closed surface,
/// so the kernel reports BOTH cap-index lists as empty. The test:
/// - builds the same 5×10mm offset rect profile;
/// - calls `revolve_with_history(profile, axis_origin=[0,0,0],
///   axis_dir=[0,0,1], angle_rad=2π)`;
/// - asserts the result is a positive-volume torus-like solid;
/// - asserts BOTH `start_cap_face_indices` AND `end_cap_face_indices` are empty;
/// - asserts at least 4 generated face records (4 profile edges → 4 revolved
///   faces, stable across partial vs full).
#[test]
fn full_revolve_with_history_reports_no_caps() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let profile_id = make_offset_rect_profile(&mut kernel);

    // 360° full revolve about the +Z axis at the origin.
    let (result_handle, history) = kernel
        .revolve_with_history(
            profile_id,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0 * std::f64::consts::PI,
        )
        .expect("revolve_with_history should succeed for a full revolve");

    // (a) Result is a positive-volume solid (torus-like, since the offset
    //     rect doesn't touch the axis).
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the full revolve result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "full revolved solid must have positive volume, got {vol_si}"
    );

    // (b) Full revolution: BOTH cap-index lists are empty because
    //     FirstShape == LastShape (one closed surface).
    assert!(
        history.start_cap_face_indices.is_empty(),
        "full revolution should produce no start caps, got {:?}",
        history.start_cap_face_indices
    );
    assert!(
        history.end_cap_face_indices.is_empty(),
        "full revolution should produce no end caps, got {:?}",
        history.end_cap_face_indices
    );

    // (c) face_generated: the rect profile has 4 edges — 2 axial (parallel
    // to Z, reported by OCCT's Generated() for cylindrical surfaces) and
    // 2 radial (perpendicular to Z, synthesized by the C++ post-pass in
    // make_revolve_with_history for annular-disk surfaces, task 2636).
    // Every profile edge produces exactly one face_generated record.
    assert!(
        history.face_generated.len() >= 4,
        "expected ≥4 generated faces (4 rect profile edges → 4 lateral faces: \
         2 cylindrical from axial edges via OCCT Generated(), \
         2 annular-disk from radial edges via C++ synthesis post-pass), \
         got {} ({:?})",
        history.face_generated.len(),
        history.face_generated
    );

    // Edge-coverage: every profile edge index {0, 1, 2, 3} must appear as
    // a parent_subshape_index in at least one face_generated record.
    {
        use std::collections::HashSet;
        let covered: HashSet<u32> = history
            .face_generated
            .iter()
            .map(|r| r.parent_subshape_index)
            .collect();
        for expected_edge in 0u32..4 {
            assert!(
                covered.contains(&expected_edge),
                "profile edge {} has no face_generated record; covered = {:?}",
                expected_edge,
                covered
            );
        }
    }

    // (d) Result-side indices must be within the result face/edge maps.
    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the full revolve result should succeed");
    let result_face_count = result_faces.len() as u32;
    for r in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        assert!(
            r.result_subshape_index < result_face_count,
            "face record result_subshape_index {} out of range; result has {} faces",
            r.result_subshape_index,
            result_face_count
        );
    }

    let result_edges = kernel
        .extract_edges(result_handle)
        .expect("extract_edges on the full revolve result should succeed");
    let result_edge_count = result_edges.len() as u32;
    for r in history
        .edge_modified
        .iter()
        .chain(history.edge_generated.iter())
    {
        assert!(
            r.result_subshape_index < result_edge_count,
            "edge record result_subshape_index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }
}

/// `BRepPrimAPI_MakeRevol` (FULL — 360°, triangular profile): exercises the
/// synthesis post-pass (task 2636) beyond the rectangular profile to verify
/// it generalises correctly.
///
/// Triangle vertices in the XZ plane (Y=0):
///   p1 = (15mm, 0, 0mm) — bottom-left
///   p2 = (25mm, 0, 0mm) — bottom-right
///   p3 = (20mm, 0, 10mm) — apex
///
/// Profile edges:
///   e0 = p1→p2 — purely radial (Δz=0, perpendicular to Z)  → 1 synthesized record
///   e1 = p2→p3 — slanted (OCCT Generated() reports)         → 1 OCCT record
///   e2 = p3→p1 — slanted (OCCT Generated() reports)         → 1 OCCT record
///
/// Expected results:
///   - positive-volume solid (cone frustum with annular base)
///   - no caps (full revolution)
///   - face_generated.len() == 3 (one record per profile edge)
///   - parent_subshape_index covers {0, 1, 2} exactly once
///   - all result_subshape_index values in range
#[test]
fn full_revolve_triangle_profile_synthesis_regression() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();

    // Triangle in XZ plane: (15mm,0mm), (25mm,0mm), (20mm,10mm).
    // Bottom edge (e0) is radial; the two slanted edges (e1, e2) are covered
    // by OCCT's Generated().
    let profile_id = kernel
        .make_triangle_profile_at_for_test(
            0.015, 0.0,   // p1: x=15mm, z=0mm
            0.025, 0.0,   // p2: x=25mm, z=0mm
            0.020, 0.010, // p3: x=20mm, z=10mm
            0.0,          // cy=0 (XZ plane)
        )
        .expect("triangle profile should build");

    // Full 360° revolve about +Z at origin.
    let (result_handle, history) = kernel
        .revolve_with_history(
            profile_id,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0 * std::f64::consts::PI,
        )
        .expect("revolve_with_history should succeed for triangle profile");

    // (i) Result is a positive-volume solid.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query should succeed");
    let vol_si = vol.as_f64().expect("volume should be numeric");
    assert!(
        vol_si > 0.0,
        "triangle full-revolve solid must have positive volume, got {vol_si}"
    );

    // (ii) No caps (full-revolution invariant).
    assert!(
        history.start_cap_face_indices.is_empty(),
        "full revolution should produce no start caps, got {:?}",
        history.start_cap_face_indices
    );
    assert!(
        history.end_cap_face_indices.is_empty(),
        "full revolution should produce no end caps, got {:?}",
        history.end_cap_face_indices
    );

    // (iii) Exactly 3 face_generated records (one per profile edge).
    assert_eq!(
        history.face_generated.len(),
        3,
        "triangle profile (1 radial + 2 slanted) should produce exactly 3 \
         face_generated records, got {} ({:?})",
        history.face_generated.len(),
        history.face_generated
    );

    // (iv) parent_subshape_index covers {0, 1, 2} exactly once each.
    {
        use std::collections::HashSet;
        let covered: HashSet<u32> = history
            .face_generated
            .iter()
            .map(|r| r.parent_subshape_index)
            .collect();
        assert_eq!(
            covered,
            [0u32, 1, 2].into_iter().collect::<HashSet<_>>(),
            "triangle profile edges {{0,1,2}} must each appear exactly once \
             in face_generated, got covered = {:?}",
            covered
        );
    }

    // (v) All result_subshape_index values in range.
    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on triangle revolve result should succeed");
    let result_face_count = result_faces.len() as u32;
    for r in &history.face_generated {
        assert!(
            r.result_subshape_index < result_face_count,
            "face_generated result_subshape_index {} out of range; result has {} faces",
            r.result_subshape_index,
            result_face_count
        );
    }
}

/// Selector-stability: the stable-sort in `make_revolve_with_history` guarantees
/// that `face_generated` records appear in profile-edge order (record position i
/// ↔ `parent_subshape_index == i`) regardless of geometric dimensions.
///
/// This property is what `populate_revolve_attributes` relies on to assign
/// `local_index = parent_subshape_index` for full-revolution results — the same
/// invariant that holds naturally for partial revolutions via OCCT's own ordering.
///
/// The test runs two full-revolution revolves on different rect dimensions and
/// asserts that both produce identical `(parent_subshape_index, record_position)`
/// orderings: `[(0,0), (1,1), (2,2), (3,3)]`.
#[test]
fn full_revolve_synthesis_keeps_per_edge_record_ordering_stable_across_dimension_edits() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();

    // Helper: revolve a rect profile and return the (parent_subshape_index,
    // record_position) ordering vec, asserting len == 4 along the way.
    let revolve_rect = |kernel: &mut OcctKernelHandle, width: f64, height: f64, cx: f64| {
        let profile_id = kernel
            .make_rect_profile_at_for_test(width, height, cx, 0.0, 0.0)
            .expect("rect profile should build");
        let (_result_handle, history) = kernel
            .revolve_with_history(
                profile_id,
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                2.0 * std::f64::consts::PI,
            )
            .expect("revolve_with_history should succeed");
        assert_eq!(
            history.face_generated.len(),
            4,
            "rect full-revolve must produce exactly 4 face_generated records \
             (2 axial via OCCT Generated() + 2 radial via synthesis), \
             got {} ({:?}) for {}×{} rect at cx={}",
            history.face_generated.len(),
            history.face_generated,
            width * 1000.0,
            height * 1000.0,
            cx * 1000.0,
        );
        // Build the (parent_subshape_index, sequential_record_position) vec.
        history
            .face_generated
            .iter()
            .enumerate()
            .map(|(pos, r)| (r.parent_subshape_index, pos as u32))
            .collect::<Vec<_>>()
    };

    // Run 1: 5×10mm rect at cx=17.5mm (same profile as the other tests).
    let ordering_5x10 = revolve_rect(&mut kernel, 5.0e-3, 10.0e-3, 17.5e-3);

    // Run 2: 8×6mm rect at cx=20mm (different dimensions, same profile topology).
    let ordering_8x6 = revolve_rect(&mut kernel, 8.0e-3, 6.0e-3, 20.0e-3);

    // Both orderings must be [(0,0), (1,1), (2,2), (3,3)] — profile-edge order.
    let expected: Vec<(u32, u32)> = vec![(0, 0), (1, 1), (2, 2), (3, 3)];
    assert_eq!(
        ordering_5x10, expected,
        "5×10mm rect: face_generated must be in profile-edge order, got {:?}",
        ordering_5x10
    );
    assert_eq!(
        ordering_8x6, expected,
        "8×6mm rect: face_generated must be in profile-edge order, got {:?}",
        ordering_8x6
    );
    assert_eq!(
        ordering_5x10, ordering_8x6,
        "per-edge record ordering must be identical across dimension changes"
    );
}
