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

use crate::common::{self, BBox};
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
    let BBox { zmin, zmax, .. } =
        common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(result_handle)));
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

// ─── task #6188: FACE profiles (the kind the compiler emits) ────────────────

/// A `CircleProfile` FACE (XY plane, +Z normal) of the given radius, lifted to
/// height `z` — the shape `translate(circle(r), 0, 0, z)` realizes.
fn circle_face_at(kernel: &mut OcctKernelHandle, radius: f64, z: f64) -> GeometryHandleId {
    let face = kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(radius),
        })
        .expect("CircleProfile should build")
        .id;
    if z == 0.0 {
        return face;
    }
    kernel
        .execute(&GeometryOp::Translate {
            target: face,
            dx: 0.0,
            dy: 0.0,
            dz: z,
        })
        .expect("Translate of the circle face should succeed")
        .id
}

/// A small box solid lifted to height `z`.
fn box_solid_at(kernel: &mut OcctKernelHandle, z: f64) -> GeometryHandleId {
    let solid = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.002),
            height: Value::Real(0.002),
            depth: Value::Real(0.002),
        })
        .expect("Box should build")
        .id;
    kernel
        .execute(&GeometryOp::Translate {
            target: solid,
            dx: 0.0,
            dy: 0.0,
            dz: z,
        })
        .expect("Translate of the box should succeed")
        .id
}

/// Conical frustum volume `π·h·(r1² + r1·r2 + r2²)/3`.
fn frustum_volume(r1: f64, r2: f64, h: f64) -> f64 {
    std::f64::consts::PI * h * (r1 * r1 + r1 * r2 + r2 * r2) / 3.0
}

fn volume_of(kernel: &OcctKernelHandle, handle: GeometryHandleId) -> f64 {
    kernel
        .query(&GeometryQuery::Volume(handle))
        .expect("volume query should succeed")
        .as_f64()
        .expect("volume value should be numeric")
}

/// Assert `actual` is within 1% of the frustum between two coaxial parallel
/// circles. Basis: a two-section ThruSections between such circles is exactly
/// a frustum's lateral surface, and ThruSections / BRepGProp error is orders
/// below 1e-3 relative.
fn assert_frustum_volume(actual: f64, r1: f64, r2: f64, h: f64) {
    let expected = frustum_volume(r1, r2, h);
    let rel = (actual - expected).abs() / expected;
    assert!(
        rel < 0.01,
        "lofted volume {actual} m³ deviates {:.4}% from the frustum {expected} m³",
        rel * 100.0
    );
}

/// Plain `execute(Loft)` (the `loft_profiles` path) over two circle FACES.
#[test]
fn loft_of_face_profiles_realizes_a_frustum() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let p1 = circle_face_at(&mut kernel, 0.02, 0.0);
    let p2 = circle_face_at(&mut kernel, 0.01, 0.1);

    let lofted = kernel
        .execute(&GeometryOp::Loft {
            profiles: vec![p1, p2],
        })
        .expect("loft of two circle faces should succeed")
        .id;

    assert_frustum_volume(volume_of(&kernel, lofted), 0.02, 0.01, 0.1);
    let BBox { zmin, zmax, .. } =
        common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(lofted)));
    assert!(
        ((zmax - zmin) - 0.1).abs() < 1e-6,
        "loft z-span must equal the 0.1 m section spacing, got [{zmin}, {zmax}]"
    );
}

/// `loft_with_history` (the `make_loft_with_history` path the engine drives)
/// over two circle FACES, with the same history contract as the wire case.
#[test]
fn loft_with_history_accepts_face_profiles() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let p1 = circle_face_at(&mut kernel, 0.02, 0.0);
    let p2 = circle_face_at(&mut kernel, 0.01, 0.1);

    let (lofted, history) = kernel
        .loft_with_history(&[p1, p2])
        .expect("loft_with_history of two circle faces should succeed");

    assert_frustum_volume(volume_of(&kernel, lofted), 0.02, 0.01, 0.1);
    assert!(
        !history.start_cap_face_indices.is_empty(),
        "expected a start cap, got {:?}",
        history.start_cap_face_indices
    );
    assert!(
        !history.end_cap_face_indices.is_empty(),
        "expected an end cap, got {:?}",
        history.end_cap_face_indices
    );
    assert!(
        !history.face_generated.is_empty(),
        "expected ≥1 face_generated record, got {:?}",
        history.face_generated
    );
    let result_face_count = kernel
        .extract_faces(lofted)
        .expect("extract_faces on the lofted result should succeed")
        .len() as u32;
    for r in &history.face_generated {
        assert!(
            (r.parent_index as usize) < 2,
            "face_generated parent_index {} must be < profiles.len()=2",
            r.parent_index
        );
        assert!(
            r.result_subshape_index < result_face_count,
            "face_generated result_subshape_index {} out of range; result has {} faces",
            r.result_subshape_index,
            result_face_count
        );
    }
}

/// A circle FACE lofted to a single VERTEX apex realizes a cone.
#[test]
fn loft_of_face_profile_to_vertex_apex_realizes_a_cone() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let base = circle_face_at(&mut kernel, 0.02, 0.0);
    let lifted_box = box_solid_at(&mut kernel, 0.1);
    let apex = *kernel
        .extract_vertices(lifted_box)
        .expect("extract_vertices on the lifted box should succeed")
        .first()
        .expect("a box has vertices");

    let cone = kernel
        .execute(&GeometryOp::Loft {
            profiles: vec![base, apex],
        })
        .expect("loft of a circle face to a vertex apex should succeed")
        .id;

    let vol = volume_of(&kernel, cone);
    assert!(vol > 0.0, "cone must have positive volume, got {vol}");
    let BBox { zmin, zmax, .. } = common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(cone)));
    assert!(
        zmax - zmin >= 0.09,
        "cone must span ≥0.09 m in Z (base at 0, apex at ≥0.1), got [{zmin}, {zmax}]"
    );
}

/// A SOLID section is refused with a diagnostic naming its shape type.
#[test]
fn loft_rejects_a_solid_profile_naming_its_shape_type() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let p1 = circle_face_at(&mut kernel, 0.02, 0.0);
    let solid = box_solid_at(&mut kernel, 0.1);

    let err = kernel
        .execute(&GeometryOp::Loft {
            profiles: vec![p1, solid],
        })
        .expect_err("loft with a solid section must error");
    match err {
        GeometryError::OperationFailed(msg) => assert!(
            msg.contains("unsupported profile shape type 'Solid'"),
            "error must name the unsupported 'Solid' section type, got: {msg}"
        ),
        other => panic!("expected GeometryError::OperationFailed, got {other:?}"),
    }
}
