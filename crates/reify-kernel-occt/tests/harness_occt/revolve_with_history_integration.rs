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

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle, revolve_synthesis_post_sort_for_test};
use reify_ir::GeometryQuery;

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
fn make_offset_rect_profile(kernel: &mut OcctKernelHandle) -> reify_ir::GeometryHandleId {
    kernel
        .make_rect_profile_at_for_test(RECT_WIDTH_M, RECT_HEIGHT_M, RECT_CX_M, 0.0, 0.0)
        .expect("offset rect profile should build")
}

/// Synthesis-helper face-normal-match tolerance. Mirrors `DIR_TOL` in
/// `crates/reify-kernel-occt/cpp/occt_wrapper.cpp:568`. The strengthened
/// assertions in the full-revolve regression tests pin `|n·axis|` against
/// this same bound so the test fails if the synthesiser ever loosens its
/// face-normal filter.
const DIR_TOL: f64 = 1e-6;

/// |dot(n, axis)| for normal-vs-axis alignment checks.
fn axis_dot_abs(n: [f64; 3], axis: [f64; 3]) -> f64 {
    (n[0] * axis[0] + n[1] * axis[1] + n[2] * axis[2]).abs()
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

    // (h) Silent-drop counter must be zero for a well-formed partial revolve.
    assert_eq!(
        history.silent_drop_count, 0,
        "partial revolve should not silently drop any Modified/Generated child — got {}",
        history.silent_drop_count
    );
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
    assert_eq!(
        history.face_generated.len(),
        4,
        "expected exactly 4 generated-face records (one per rect profile edge: \
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

    // (e) Normal-axis orientation, mirroring the triangle regression test
    //     (full_revolve_triangle_profile_synthesis_regression) — task 2707.
    //  - Profile edges 0 and 2 (bottom and top of the rect, Δz=0) are radial
    //    and reach face_generated via the synthesis post-pass; their result
    //    faces are flat annular disks with |n·axis| >= 1 - DIR_TOL. This pins
    //    the synthesis matcher's face-normal filter (occt_wrapper.cpp:737-740).
    //  - Profile edges 1 and 3 (right and left, parallel to Z) are axial and
    //    reach face_generated via OCCT's Generated() as cylindrical sweeps.
    //    Cylindrical face normals are exactly perpendicular to the revolution
    //    axis, so |n·axis| = 0 exactly (up to floating-point). We assert
    //    dot < DIR_TOL (= 1e-6) rather than the looser < 1 - DIR_TOL; the
    //    tighter bound makes a degenerate face-normal regression fail loudly
    //    instead of only catching synthesiser-threshold drift.
    let axis_dir = [0.0_f64, 0.0, 1.0];

    for radial_edge in [0u32, 2u32] {
        let rec = history
            .face_generated
            .iter()
            .find(|r| r.parent_subshape_index == radial_edge)
            .unwrap_or_else(|| {
                panic!("rect radial edge e{radial_edge} missing from face_generated")
            });
        let n = kernel
            .face_outward_unit_normal_for_test(result_faces[rec.result_subshape_index as usize])
            .expect("face_outward_unit_normal_for_test should succeed");
        let dot = axis_dot_abs(n, axis_dir);
        assert!(
            dot > 1.0 - DIR_TOL,
            "synthesised annular-disk face for radial edge e{radial_edge} must \
             have |face_normal · axis| > 1 - DIR_TOL ({}), got {} (record {:?})",
            1.0 - DIR_TOL,
            dot,
            rec
        );
    }

    for axial_edge in [1u32, 3u32] {
        let rec = history
            .face_generated
            .iter()
            .find(|r| r.parent_subshape_index == axial_edge)
            .unwrap_or_else(|| panic!("rect axial edge e{axial_edge} missing from face_generated"));
        let n = kernel
            .face_outward_unit_normal_for_test(result_faces[rec.result_subshape_index as usize])
            .expect("face_outward_unit_normal_for_test should succeed");
        let dot = axis_dot_abs(n, axis_dir);
        assert!(
            dot < DIR_TOL,
            "OCCT-reported cylindrical face for axial edge e{axial_edge} must \
             have |face_normal · axis| < DIR_TOL ({DIR_TOL}), got {} (record {:?}); \
             cylindrical face normals are exactly perpendicular to the axis \
             (|n·axis| ≈ 0), so a value >= DIR_TOL would indicate a degenerate \
             normal or the wrong face was selected",
            dot,
            rec
        );
    }

    // (f) Diagnostic counter: for a well-formed rect profile every profile
    //     edge must produce a face_generated record — no unsynthesized profile
    //     edges expected.
    assert_eq!(
        history.unsynthesized_profile_edge_count, 0,
        "full revolve of rect profile must report 0 unsynthesized profile edges, \
         got {} (face_generated = {:?})",
        history.unsynthesized_profile_edge_count, history.face_generated
    );

    // (g) No duplicate parent_subshape_index values after the post-sort/dedup
    //     pass — expected 0 for a well-formed rect profile.
    assert_eq!(
        history.duplicate_parent_subshape_index_count, 0,
        "full revolve of rect profile must report 0 duplicate parent_subshape_index, \
         got {} (face_generated = {:?})",
        history.duplicate_parent_subshape_index_count, history.face_generated
    );
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

    let kernel = OcctKernelHandle::spawn();

    // Triangle in XZ plane: (15mm,0mm), (25mm,0mm), (20mm,10mm).
    // Bottom edge (e0) is radial; the two slanted edges (e1, e2) are covered
    // by OCCT's Generated().
    let profile_id = kernel
        .make_triangle_profile_at_for_test(
            0.015, 0.0, // p1: x=15mm, z=0mm
            0.025, 0.0, // p2: x=25mm, z=0mm
            0.020, 0.010, // p3: x=20mm, z=10mm
            0.0,   // cy=0 (XZ plane)
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

    // (vi) Normal-axis orientation:
    //  - The radial edge (parent_subshape_index == 0) sweeps to a flat
    //    annular-disk face whose normal is (anti-)parallel to the rotation
    //    axis: |n·axis| >= 1.0 - DIR_TOL. This is exactly the condition the
    //    synthesis matcher in occt_wrapper.cpp:737-740 enforces; the
    //    assertion fails if a future regression makes the matcher accept a
    //    non-disk face.
    //  - The two slanted edges (parent_subshape_index in {1, 2}) sweep to
    //    conical frustum faces. The bound is kept as < 1 - DIR_TOL rather
    //    than being tightened to < DIR_TOL because the half-angle of the cone
    //    determines the achievable |n·axis| value: e1 goes from (25mm,0,0) to
    //    (20mm,0,10mm) (Δr=−5mm, Δz=+10mm), giving a slant angle of
    //    atan(5/10) ≈ 26.6° from the Z-axis and a surface-normal component
    //    |n·axis| ≈ sin(26.6°) ≈ 0.447. Tightening to DIR_TOL would
    //    over-constrain to the specific triangle geometry; the synthesiser
    //    exclusion threshold (1 - DIR_TOL) is sufficient to pin that these
    //    records came from OCCT's Generated() and not the synthesis path. If
    //    the exact cone geometry matters, a dedicated geometry test should
    //    assert it.
    let axis_dir = [0.0_f64, 0.0, 1.0];

    let radial = history
        .face_generated
        .iter()
        .find(|r| r.parent_subshape_index == 0)
        .expect("triangle e0 (radial) must produce a face_generated record");
    let n = kernel
        .face_outward_unit_normal_for_test(result_faces[radial.result_subshape_index as usize])
        .expect("face_outward_unit_normal_for_test should succeed");
    let radial_dot = axis_dot_abs(n, axis_dir);
    assert!(
        radial_dot > 1.0 - DIR_TOL,
        "synthesised annular-disk face for radial edge e0 must have \
         |face_normal · axis| > 1 - DIR_TOL ({}), got {} (record {:?})",
        1.0 - DIR_TOL,
        radial_dot,
        radial
    );

    for slanted_idx in [1u32, 2u32] {
        let rec = history
            .face_generated
            .iter()
            .find(|r| r.parent_subshape_index == slanted_idx)
            .unwrap_or_else(|| {
                panic!("triangle slanted edge e{slanted_idx} missing from face_generated")
            });
        let n = kernel
            .face_outward_unit_normal_for_test(result_faces[rec.result_subshape_index as usize])
            .expect("face_outward_unit_normal_for_test should succeed");
        let slanted_dot = axis_dot_abs(n, axis_dir);
        assert!(
            slanted_dot < 1.0 - DIR_TOL,
            "OCCT-reported conical face for slanted edge e{slanted_idx} must have \
             |face_normal · axis| < 1 - DIR_TOL ({}), got {} (record {:?}); \
             value >= 1 - DIR_TOL would indicate this record came from the \
             synthesis path rather than OCCT's Generated()",
            1.0 - DIR_TOL,
            slanted_dot,
            rec
        );
    }

    // (vii) Diagnostic counter: for a well-formed triangle profile every
    //       profile edge must produce a face_generated record — no unsynthesized
    //       profile edges expected.
    assert_eq!(
        history.unsynthesized_profile_edge_count, 0,
        "full revolve of triangle profile must report 0 unsynthesized profile edges, \
         got {} (face_generated = {:?})",
        history.unsynthesized_profile_edge_count, history.face_generated
    );

    // (viii) No duplicate parent_subshape_index values after post-sort/dedup.
    assert_eq!(
        history.duplicate_parent_subshape_index_count, 0,
        "full revolve of triangle profile must report 0 duplicate parent_subshape_index, \
         got {} (face_generated = {:?})",
        history.duplicate_parent_subshape_index_count, history.face_generated
    );
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

/// Misclassified-radial-edge counter test: a triangle profile where the
/// bottom edge has a tiny axial component (Δz ≈ 2nm over 10mm run) that
/// makes `|dot(edge_dir, +Z)| ≈ 2e-6` — just over `DIR_TOL = 1e-6`.
///
/// Profile vertices:
///   p1 = (15mm, 0, 0)      — origin of bottom edge
///   p2 = (25mm, 0, 2e-8m)  — bottom edge end; Δz = 2e-8, Δx = 10mm
///   p3 = (20mm, 0, 10mm)   — apex
///
/// The bottom edge p1→p2 has `dot(edge_dir, +Z) ≈ 2e-6 / 0.01 = 2e-6`
/// — just above `DIR_TOL = 1e-6`, so the synthesis post-pass classifies
/// it as "slanted" (path 5) and won't synthesise a record.
///
/// Assertion: `unsynthesized_profile_edge_count == 3 - face_generated.len()`
/// (every profile edge without a synthesised record must bump the counter).
/// This is OCCT-version-agnostic: if OCCT covers the edge via Generated()
/// both sides are 0; if it doesn't, both sides equal the gap. Either way
/// the constraint holds once the increment logic is in place (step-4).
///
/// Before step-4, counter stays 0; if OCCT doesn't cover the edge the
/// assertion fires (`0 != 1`), confirming the failing state. After step-4
/// the assertion is fully meaningful.
#[test]
fn full_revolve_misclassified_radial_edge_counter_best_effort() {
    if !OCCT_AVAILABLE {
        return;
    }

    let kernel = OcctKernelHandle::spawn();

    // Bottom edge: p1=(15mm,0,0) → p2=(25mm,0,2e-8). Δx=10mm, Δz=2e-8m.
    // |dot(edge_dir, +Z)| ≈ 2e-8 / 0.01 = 2e-6 (just over DIR_TOL=1e-6).
    let profile_id = kernel
        .make_triangle_profile_at_for_test(
            0.015, 0.0, // p1: x=15mm, z=0mm
            0.025, 2e-8, // p2: x=25mm, z=2nm
            0.020, 0.010, // p3: x=20mm, z=10mm
            0.0,   // cy=0 (XZ plane)
        )
        .expect("slightly-slanted triangle profile should build");

    let (_result_handle, history) = kernel
        .revolve_with_history(
            profile_id,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0 * std::f64::consts::PI,
        )
        .expect("revolve_with_history should succeed for slightly-slanted triangle");

    eprintln!(
        "misclassified-radial-edge test: unsynthesized_profile_edge_count={}, \
         face_generated.len()={}, face_generated={:?}",
        history.unsynthesized_profile_edge_count,
        history.face_generated.len(),
        history.face_generated
    );

    // Sanity: profile has 3 edges, result can have at most 3 face_generated records.
    assert!(
        history.face_generated.len() <= 3,
        "triangle profile has 3 edges, cannot produce more than 3 face_generated records, \
         got {}",
        history.face_generated.len()
    );

    // Self-consistency: every edge without a synthesised record must have
    // bumped the counter.
    //
    // Two possible OCCT behaviours — both handled by the assertion:
    //
    //  A. OCCT 7.5.x does NOT track the nearly-radial edge in Generated()
    //     (the expected behaviour given the full-2π silent-gap; this is the
    //     regime the synthesis helper was written for).  The edge is not in
    //     `tracked_parent_edges`, hits path 5 (slanted: dot ≈ 2e-6 > DIR_TOL),
    //     increments the counter, and no record is synthesised.
    //     → face_generated.len()==2, counter==1 → 1 == 3−2 = 1  ✓ (meaningful)
    //
    //  B. A future OCCT version *does* track the nearly-radial edge. The edge
    //     lands in `tracked_parent_edges` before the synthesis loop, so the
    //     loop skips it without touching the counter, and face_generated.len()
    //     remains 3 (OCCT covered all three edges).
    //     → face_generated.len()==3, counter==0 → 0 == 3−3 = 0  ✓ (tautological
    //       but still a valid regression guard: any incorrect increment would
    //       make both sides non-zero and unequal, catching a double-count bug).
    //
    // If OCCT's behaviour changes and the test degrades to case B, the
    // eprintln! above will show counter=0 in CI output, signalling that the
    // increment path is no longer exercised here.  A dedicated synthesis-loop
    // fixture (not yet implemented) would provide a fully deterministic path.
    // Pin the formula's precondition: `3 - face_generated.len()` only equals
    // the unsynthesized count when no records were dropped by the post-sort
    // dedup pass. Any non-zero duplicate count would silently drag
    // `face_generated.len()` below the OCCT-reported edge total and make the
    // self-consistency assertion drift.
    assert_eq!(
        history.duplicate_parent_subshape_index_count, 0,
        "duplicate_parent_subshape_index_count must be 0 for this fixture so \
         the (3 - face_generated.len()) self-consistency formula below holds; \
         got {}",
        history.duplicate_parent_subshape_index_count
    );
    assert_eq!(
        history.unsynthesized_profile_edge_count as usize,
        3 - history.face_generated.len(),
        "every profile edge without a synthesised face_generated record must \
         increment unsynthesized_profile_edge_count; counter={}, face_generated.len()={}",
        history.unsynthesized_profile_edge_count,
        history.face_generated.len()
    );
}

/// Post-sort dedup fixture test: verifies that `revolve_synthesis_post_sort_for_test`
/// (a) drops duplicate `parent_subshape_index` records (keeping the first under
/// stable sort), (b) increments `duplicate_count` for each drop, and (c) leaves
/// well-formed (no-duplicate) input unchanged.
///
/// The fixture exercises the dedup logic extracted into
/// `revolve_synthesis_post_sort_and_dedup` (step-6) without needing real OCCT
/// geometry — input is a synthetic flat `Vec<u32>` in the same
/// `(parent_index, parent_subshape_index, result_subshape_index)` layout used
/// by `face_generated`.
///
/// Two sub-tests:
///  1. Input with one deliberate duplicate: sorted, dedup'd, count == 1.
///  2. Unsorted no-duplicate input: stable-sorted by parent_subshape_index,
///     count == 0.
#[test]
fn revolve_synthesis_post_sort_drops_duplicate_parent_subshape_index() {
    if !OCCT_AVAILABLE {
        return;
    }

    // --- Sub-test 1: one deliberate duplicate ---
    // Input: three records, two of which share parent_subshape_index=1.
    // After stable-sort by parent_subshape_index the order is already correct
    // (0, 1, 100), (0, 1, 200), (0, 2, 300). The dedup pass drops the second
    // parent_subshape_index=1 record (result=200, the later one under stable
    // sort) and keeps (result=100, the earlier one).
    let input_dup: Vec<u32> = vec![
        /* rec0: parent=0, subshape=1, result=100 */ 0, 1, 100,
        /* rec1: parent=0, subshape=1, result=200 */ 0, 1, 200, // duplicate
        /* rec2: parent=0, subshape=2, result=300 */ 0, 2, 300,
    ];
    let result_dup = revolve_synthesis_post_sort_for_test(&input_dup);
    assert_eq!(
        result_dup.duplicate_count, 1,
        "exactly one duplicate must be dropped; got duplicate_count={}",
        result_dup.duplicate_count
    );
    assert_eq!(
        result_dup.output.as_slice(),
        &[0_u32, 1, 100, 0, 2, 300],
        "dedup must keep the first occurrence (result=100), drop the second (result=200), \
         and preserve the non-duplicate (parent_subshape_index=2, result=300)"
    );

    // --- Sub-test 2: well-formed (no-duplicate) unsorted input ---
    // Input: three records in reverse parent_subshape_index order.
    // After stable-sort: order becomes parent_subshape_index 0 < 1 < 2.
    // No duplicates → dedup drops nothing, count == 0.
    let input_ok: Vec<u32> = vec![
        /* rec0: parent=0, subshape=2, result=300 */ 0, 2, 300,
        /* rec1: parent=0, subshape=0, result=100 */ 0, 0, 100,
        /* rec2: parent=0, subshape=1, result=200 */ 0, 1, 200,
    ];
    let result_ok = revolve_synthesis_post_sort_for_test(&input_ok);
    assert_eq!(
        result_ok.duplicate_count, 0,
        "no-duplicate input must report duplicate_count=0; got {}",
        result_ok.duplicate_count
    );
    assert_eq!(
        result_ok.output.as_slice(),
        &[0_u32, 0, 100, 0, 1, 200, 0, 2, 300],
        "stable-sort must reorder by parent_subshape_index (0 < 1 < 2)"
    );
}

/// Pins the `.abs()` branch: anti-parallel input (dot = -1.0) → 1.0.
#[test]
fn axis_dot_abs_returns_absolute_dot_product() {
    assert_eq!(axis_dot_abs([0.0, 0.0, -1.0], [0.0, 0.0, 1.0]), 1.0);
}
