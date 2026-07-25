//! Integration test for `OcctKernelHandle::loft_with_history` —
//! the v0.2 persistent-naming-v2 loft history-tracking primitive
//! for `BRepOffsetAPI_ThruSections` (task 5b / #2619, step-5).
//!
//! Loft is a **multi-parent** operation: each profile section is a
//! distinct parent, indexed by `parent_index ∈ [0, sections.len())`.
//! Unlike sweep / extrude / revolve, the result lateral faces come
//! from `BRepOffsetAPI_ThruSections::GeneratedFace(edge)` (per
//! profile-section edge) rather than the generic `Modified()` /
//! `Generated()` interface, so a separate FFI primitive is required.
//!
//! Mirrors the structure of `sweep_with_history_integration.rs` but
//! exercises:
//! - The multi-parent `parent_index` semantics on every record.
//! - The 2-profile validation error path (loft requires ≥2 profiles).
//! - Cap-index lists populated from `FirstShape()` / `LastShape()` under
//!   `is_solid=true` (the GeometryOp::Loft contract).
//!
//! Gated on `OCCT_AVAILABLE` and `#![cfg(has_occt)]` so non-OCCT builds
//! skip without linker errors.

#![cfg(has_occt)]

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Build a closed circular wire profile of the given radius at the given
/// z height (centred on the Z-axis) via `GeometryOp::Arc` (full 2π).
fn make_circle_profile(kernel: &mut OcctKernelHandle, radius: f64, z: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, z],
            radius,
            start_angle: 0.0,
            end_angle: 2.0 * std::f64::consts::PI,
            axis: [0.0, 0.0, 1.0],
        })
        .expect("Arc (full circle) creation should succeed")
        .id
}

/// Parse a JSON-encoded bounding box string
/// `{"xmin":…,"ymin":…,"zmin":…,"xmax":…,"ymax":…,"zmax":…}` into
/// (zmin, zmax). We only need Z for the span assertion.
fn parse_bbox_z(s: &str) -> (f64, f64) {
    let mut zmin = f64::NAN;
    let mut zmax = f64::NAN;
    let trimmed = s.trim_start_matches('{').trim_end_matches('}');
    for pair in trimmed.split(',') {
        let mut parts = pair.splitn(2, ':');
        let key = parts.next().unwrap().trim().trim_matches('"');
        let val: f64 = parts.next().unwrap().trim().parse().unwrap();
        match key {
            "zmin" => zmin = val,
            "zmax" => zmax = val,
            _ => {}
        }
    }
    (zmin, zmax)
}

/// `BRepOffsetAPI_ThruSections` history exposes per-profile-section
/// `GeneratedFace(edge)` for lateral faces and `FirstShape() /
/// LastShape()` for caps under `is_solid=true`. The test:
///
/// - builds two closed circular profiles at z=0 and z=0.1m;
/// - calls `loft_with_history(vec![p1, p2])`;
/// - asserts (a) result has positive volume + ≥0.09m Z-bbox span;
/// - asserts (b) `start_cap_face_indices` is non-empty (first profile cap
///   under is_solid=true);
/// - asserts (c) `end_cap_face_indices` is non-empty (last profile cap);
/// - asserts (d) `face_generated.len() ≥ 1` (at least one lateral
///   face per pair of two-circle sections — OCCT may produce a single
///   face for two coaxial circles);
/// - asserts (e) `parent_index < profiles.len()` and `result_subshape_index
///   < result_face_count` for each `face_generated` record;
/// - exercises the validation error path: `loft_with_history(vec![p1])` →
///   `Err(GeometryError::OperationFailed)` mentioning "profile".
///
/// Compilation/linkage of this test pins step-6: it will fail to build
/// until the FFI primitive + Rust handle method ship.
#[test]
fn loft_with_history_reports_caps_and_lofted_face_records() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    // Two coplanar (XY-plane, at different z) circular profiles.
    let p1 = make_circle_profile(&mut kernel, 0.02, 0.0);
    let p2 = make_circle_profile(&mut kernel, 0.02, 0.1);

    let (result_handle, history) = kernel
        .loft_with_history(&[p1, p2])
        .expect("loft_with_history should succeed for two coaxial circles");

    // (a) Result has positive volume.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the lofted result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "lofted solid must have positive volume, got {vol_si}"
    );
    // Bounding box spans the two profile z heights (0 → 0.1m).
    let bbox = kernel
        .query(&GeometryQuery::BoundingBox(result_handle))
        .expect("bbox query should succeed");
    let (zmin, zmax) = match bbox {
        Value::String(s) => parse_bbox_z(&s),
        other => panic!("expected bbox String, got {:?}", other),
    };
    assert!(
        zmin.is_finite() && zmax.is_finite(),
        "loft bbox z-extent must be finite, got [{zmin}, {zmax}]"
    );
    let z_span = zmax - zmin;
    assert!(
        z_span >= 0.09,
        "lofted shape must span ≥0.09m in Z (profiles at z=0 and z=0.1), got {z_span}"
    );

    // (b) Start cap: first profile section under is_solid=true.
    assert!(
        !history.start_cap_face_indices.is_empty(),
        "expected non-empty start_cap_face_indices for is_solid=true loft, got {:?}",
        history.start_cap_face_indices
    );

    // (c) End cap: last profile section under is_solid=true.
    assert!(
        !history.end_cap_face_indices.is_empty(),
        "expected non-empty end_cap_face_indices for is_solid=true loft, got {:?}",
        history.end_cap_face_indices
    );

    // (d) face_generated: at least one record for two coaxial-circle
    //     sections (OCCT may emit a single side face).
    assert!(
        !history.face_generated.is_empty(),
        "expected ≥1 face_generated record for a 2-profile loft, got {:?}",
        history.face_generated
    );

    // (e) For each face_generated record: parent_index < profiles.len() (=2)
    //     AND result_subshape_index < result_face_count.
    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the lofted result should succeed");
    let result_face_count = result_faces.len() as u32;
    for r in &history.face_generated {
        assert!(
            (r.parent_index as usize) < 2,
            "loft face_generated parent_index {} must be < profiles.len()=2",
            r.parent_index
        );
        assert!(
            r.result_subshape_index < result_face_count,
            "loft face_generated result_subshape_index {} out of range; result has {} faces",
            r.result_subshape_index,
            result_face_count
        );
    }
    // Cap indices must also be in-range.
    for &cap_idx in history
        .start_cap_face_indices
        .iter()
        .chain(history.end_cap_face_indices.iter())
    {
        assert!(
            cap_idx < result_face_count,
            "loft cap face index {} out of range; result has {} faces",
            cap_idx,
            result_face_count
        );
    }
}

/// Validation: loft_with_history rejects a 1-profile input. Mirrors the
/// `loft_profiles` C++-level validation ("requires at least 2 profiles")
/// surfaced as a Rust-layer GeometryError::OperationFailed before the
/// FFI call.
#[test]
fn loft_with_history_rejects_single_profile() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let p1 = make_circle_profile(&mut kernel, 0.02, 0.0);

    let err = kernel
        .loft_with_history(&[p1])
        .expect_err("loft_with_history with 1 profile must error");
    match err {
        GeometryError::OperationFailed(msg) => {
            assert!(
                msg.to_lowercase().contains("profile"),
                "error message should mention 'profile', got: {msg}"
            );
        }
        other => panic!("expected GeometryError::OperationFailed, got {:?}", other),
    }
}
