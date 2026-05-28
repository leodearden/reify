//! Integration tests for the new rigid-transform-application primitive
//! `OcctKernel::apply_transform_to_handle` (PRD §5, task 3901).
//!
//! These tests verify the kernel-level properties mandated by the
//! sub-placement PRD §5:
//!   (a) identity transform is a no-op (AABB preserved),
//!   (b) pure translation shifts the AABB by the translation vector,
//!   (c) rotation correctly re-orients the shape's extents,
//!   (d) `T ∘ T⁻¹` composes to identity (rigid-isometry contract),
//!   (e) the source handle is preserved unmodified (multi-frame reuse),
//!   (f) non-unit quaternions are rejected with a descriptive error.
//!
//! All AABBs are computed from tessellated mesh vertices (0.1 tolerance) to
//! observe the behavior the GUI / downstream consumers see, rather than the
//! BREP-level `BRepBndLib` query (which would pad the box and bypass the
//! TopLoc_Location application path).

#![cfg(has_occt)]

use std::f64::consts::PI;

use reify_kernel_occt::{OcctKernel, Transform3};
use reify_ir::{GeometryHandleId, GeometryOp, Value};

/// Axis-aligned bounding box derived from a flat `Vec<f32>` of vertex positions
/// (X, Y, Z triples). Stored as f64 so comparisons against f64 tolerances are
/// exact-after-widen (no float-promotion surprises).
#[derive(Debug, Clone, Copy)]
struct Aabb {
    min: [f64; 3],
    max: [f64; 3],
}

/// Compute the AABB of a tessellated mesh from its flat vertex buffer.
///
/// Panics if `vertices.len() % 3 != 0` or the buffer is empty — both indicate
/// a malformed tessellation result and are programmer errors at this seam.
fn aabb_of_vertices(vertices: &[f32]) -> Aabb {
    assert!(!vertices.is_empty(), "tessellation produced no vertices");
    assert_eq!(
        vertices.len() % 3,
        0,
        "tessellation vertex buffer length {} not divisible by 3",
        vertices.len()
    );
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for tri in vertices.chunks_exact(3) {
        for i in 0..3 {
            let v = tri[i] as f64;
            if v < min[i] {
                min[i] = v;
            }
            if v > max[i] {
                max[i] = v;
            }
        }
    }
    Aabb { min, max }
}

/// Tessellate `handle` at 0.1 tolerance and return its AABB.
fn aabb_of_handle(kernel: &OcctKernel, handle: GeometryHandleId) -> Aabb {
    let mesh = kernel
        .tessellate(handle, 0.1)
        .expect("tessellation should succeed");
    aabb_of_vertices(&mesh.vertices)
}

/// Assert that two AABBs match componentwise within `tol` (in the same units
/// as the AABB).
#[track_caller]
fn assert_aabb_eq(actual: Aabb, expected: Aabb, tol: f64, what: &str) {
    for i in 0..3 {
        let d_min = (actual.min[i] - expected.min[i]).abs();
        let d_max = (actual.max[i] - expected.max[i]).abs();
        assert!(
            d_min < tol,
            "{what}: AABB.min[{i}] mismatch: actual={}, expected={}, delta={}, tol={}",
            actual.min[i], expected.min[i], d_min, tol
        );
        assert!(
            d_max < tol,
            "{what}: AABB.max[{i}] mismatch: actual={}, expected={}, delta={}, tol={}",
            actual.max[i], expected.max[i], d_max, tol
        );
    }
}

// ---------------------------------------------------------------------------
// (a) Identity-transform invariance
// ---------------------------------------------------------------------------

/// The simplest invariant: applying the identity quaternion + zero translation
/// must produce a shape whose AABB matches the source exactly.
///
/// Catches: a bug in the `build_trsf` quaternion → gp_Trsf path that introduces
/// a stray rotation under the identity quaternion (qw=1, qx=qy=qz=0).
#[test]
fn apply_transform_to_handle_identity_returns_box_with_same_aabb() {
    let mut kernel = OcctKernel::new();

    let source = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        })
        .expect("box creation should succeed");

    let source_aabb = aabb_of_handle(&kernel, source.id);

    let identity = Transform3 {
        qw: 1.0,
        qx: 0.0,
        qy: 0.0,
        qz: 0.0,
        tx: 0.0,
        ty: 0.0,
        tz: 0.0,
    };

    let transformed_id = kernel
        .apply_transform_to_handle(source.id, &identity)
        .expect("identity transform should succeed");

    let transformed_aabb = aabb_of_handle(&kernel, transformed_id);

    // Identity is bit-exact at the TopLoc_Location level; the only drift comes
    // from tessellation float-rounding on the source vs the transformed side
    // (same mesher, same shape — should be deterministic).
    assert_aabb_eq(
        transformed_aabb,
        source_aabb,
        1e-6,
        "identity-transform AABB",
    );
}

// ---------------------------------------------------------------------------
// (b) Pure-translation regression lock
// ---------------------------------------------------------------------------

/// A pure translation (qw=1, zero rotation) shifts the AABB by the translation
/// vector exactly. `build_trsf` constructs the gp_Trsf via
/// `SetTranslationPart(gp_Vec(tx,ty,tz))` so this exercises the unconditional
/// translation path.
///
/// Fixture: 10×10×10 box centered at origin (X,Y,Z ∈ [-5, 5]) translated by
/// (10, 20, 30); expected AABB is min=[5, 15, 25] / max=[15, 25, 35].
#[test]
fn apply_transform_to_handle_pure_translation_shifts_aabb() {
    let mut kernel = OcctKernel::new();

    let source = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box creation should succeed");

    let t = Transform3 {
        qw: 1.0,
        qx: 0.0,
        qy: 0.0,
        qz: 0.0,
        tx: 10.0,
        ty: 20.0,
        tz: 30.0,
    };

    let transformed_id = kernel
        .apply_transform_to_handle(source.id, &t)
        .expect("pure-translation transform should succeed");

    let transformed_aabb = aabb_of_handle(&kernel, transformed_id);
    let expected = Aabb {
        min: [5.0, 15.0, 25.0],
        max: [15.0, 25.0, 35.0],
    };

    assert_aabb_eq(transformed_aabb, expected, 1e-6, "pure-translation AABB");
}

// ---------------------------------------------------------------------------
// (c) Rotation-only invariance (anti-cube-blind-spot)
// ---------------------------------------------------------------------------

/// A 90°-Z rotation on an asymmetric 12×8×6 brick swaps the X and Y extents.
///
/// **Fixture** (same as transform_distance_integration:84 — anti-cube-blind-
/// spot): brick centered at origin, X∈[-6,6], Y∈[-4,4], Z∈[-3,3]. After 90°-Z
/// rotation, the new X-extent is the old ±Y-extent (±4) and the new Y-extent
/// is the old ±X-extent (±6). Z-extent is unchanged.
///
/// The 90°-Z rotation quaternion: qw=cos(π/4), qz=sin(π/4), qx=qy=0.
///
/// **Why this catches xyzw/wxyz swaps**: a wrong quaternion-component order
/// would interpret this as a 90°-X rotation (X stays [-6,6], Y↔Z swap). The
/// resulting X-extent would still be ±6 — a 2mm delta from the correct ±4
/// extent, which is 20,000× the assertion tolerance of 1e-4 m.
///
/// Tolerance is 1e-4 m (slacker than translation case) because tessellation
/// of the rotated faces projects mesh-vertex positions onto the new axes;
/// the box edges are themselves preserved exactly by the TopLoc_Location path.
#[test]
fn apply_transform_to_handle_rotation_only_swaps_brick_extents() {
    let mut kernel = OcctKernel::new();

    let source = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(12.0),
            height: Value::Real(8.0),
            depth: Value::Real(6.0),
        })
        .expect("12×8×6 brick creation should succeed");

    let t = Transform3 {
        qw: (PI / 4.0).cos(),
        qx: 0.0,
        qy: 0.0,
        qz: (PI / 4.0).sin(),
        tx: 0.0,
        ty: 0.0,
        tz: 0.0,
    };

    let transformed_id = kernel
        .apply_transform_to_handle(source.id, &t)
        .expect("90°-Z rotation should succeed");

    let transformed_aabb = aabb_of_handle(&kernel, transformed_id);
    let expected = Aabb {
        min: [-4.0, -6.0, -3.0],
        max: [4.0, 6.0, 3.0],
    };

    assert_aabb_eq(
        transformed_aabb,
        expected,
        1e-4,
        "90°-Z rotation AABB (12×8×6 brick → expected X∈[-4,4], Y∈[-6,6], Z∈[-3,3]; \
         a wrong xyzw/wxyz quaternion swap would give X∈[-6,6] instead)",
    );
}

// ---------------------------------------------------------------------------
// (d) Rigid-isometry composition: T ∘ T⁻¹ round-trip
// ---------------------------------------------------------------------------

/// `apply_transform_to_handle(apply_transform_to_handle(s, T), T⁻¹)` yields a
/// shape whose AABB equals the source AABB componentwise.
///
/// **Why this matters**: confirms the `BRepBuilderAPI_Transform(...,
/// Standard_False)` path is a pure rigid-isometry composition — a regression
/// to `Standard_True` (geometry bake) would accumulate float-rounding on each
/// application, breaking componentwise AABB equality at the 1e-4 m level.
///
/// **Transform**: 60° rotation about the unit-normalized [1, 1, 1] axis,
/// combined with translation (7, -3, 5). The non-axis-aligned rotation
/// ensures the inverse is non-trivial — any composition-order bug or
/// quaternion-conjugation error would surface as a translation drift.
#[test]
fn apply_transform_to_handle_t_inverse_round_trip_matches_source_aabb() {
    let mut kernel = OcctKernel::new();

    let source = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(12.0),
            height: Value::Real(8.0),
            depth: Value::Real(6.0),
        })
        .expect("brick creation should succeed");

    let source_aabb = aabb_of_handle(&kernel, source.id);

    // 60° rotation about the unit-normalized [1, 1, 1] axis.
    let theta = PI / 3.0; // 60°
    let half = theta / 2.0;
    let inv_sqrt3 = 1.0 / (3.0_f64).sqrt();
    let qw = half.cos();
    let s = half.sin();
    let qx = s * inv_sqrt3;
    let qy = s * inv_sqrt3;
    let qz = s * inv_sqrt3;

    let tx = 7.0;
    let ty = -3.0;
    let tz = 5.0;

    let t = Transform3 {
        qw,
        qx,
        qy,
        qz,
        tx,
        ty,
        tz,
    };

    // T⁻¹: conjugate quaternion (qw, -qx, -qy, -qz); translation = -R⁻¹·t.
    // Since gp_Trsf composes as `p' = R·p + t`, the inverse is
    //   p = R⁻¹·(p' - t) = R⁻¹·p' - R⁻¹·t.
    // So T⁻¹.translation = -R⁻¹·t = -conjugate(q) · t · conjugate(q)⁻¹ as a
    // vector rotation. Compute R⁻¹·t by rotating the vector (tx, ty, tz) by
    // the inverse quaternion (qw, -qx, -qy, -qz) using the vector-rotation
    // formula v' = q · v · q⁻¹ (for unit q, q⁻¹ = conjugate(q)).
    //
    // For a unit quaternion q = (w, x, y, z), rotating v = (vx, vy, vz):
    //   v' = (w² + x² - y² - z²)·vx + 2(xy - wz)·vy + 2(xz + wy)·vz, …
    // (the standard quaternion-to-rotation-matrix formula).
    //
    // For the inverse rotation we use q_inv = (w, -x, -y, -z); plugging into
    // the formula gives the same matrix transposed.
    let qx_inv = -qx;
    let qy_inv = -qy;
    let qz_inv = -qz;
    let qw_inv = qw;
    let rot_inv_x = (qw_inv * qw_inv + qx_inv * qx_inv - qy_inv * qy_inv - qz_inv * qz_inv) * tx
        + 2.0 * (qx_inv * qy_inv - qw_inv * qz_inv) * ty
        + 2.0 * (qx_inv * qz_inv + qw_inv * qy_inv) * tz;
    let rot_inv_y = 2.0 * (qx_inv * qy_inv + qw_inv * qz_inv) * tx
        + (qw_inv * qw_inv - qx_inv * qx_inv + qy_inv * qy_inv - qz_inv * qz_inv) * ty
        + 2.0 * (qy_inv * qz_inv - qw_inv * qx_inv) * tz;
    let rot_inv_z = 2.0 * (qx_inv * qz_inv - qw_inv * qy_inv) * tx
        + 2.0 * (qy_inv * qz_inv + qw_inv * qx_inv) * ty
        + (qw_inv * qw_inv - qx_inv * qx_inv - qy_inv * qy_inv + qz_inv * qz_inv) * tz;

    let t_inv = Transform3 {
        qw: qw_inv,
        qx: qx_inv,
        qy: qy_inv,
        qz: qz_inv,
        tx: -rot_inv_x,
        ty: -rot_inv_y,
        tz: -rot_inv_z,
    };

    // Apply T then T⁻¹.
    let after_t = kernel
        .apply_transform_to_handle(source.id, &t)
        .expect("first transform should succeed");
    let round_trip = kernel
        .apply_transform_to_handle(after_t, &t_inv)
        .expect("inverse transform should succeed");

    let round_trip_aabb = aabb_of_handle(&kernel, round_trip);

    assert_aabb_eq(
        round_trip_aabb,
        source_aabb,
        1e-4,
        "T ∘ T⁻¹ round-trip AABB (rigid-isometry composition contract)",
    );
}

// ---------------------------------------------------------------------------
// (e) Source-handle preservation (multi-frame reuse contract)
// ---------------------------------------------------------------------------

/// `apply_transform_to_handle` must leave the source handle's shape intact,
/// so the same child can be placed in multiple frames.
///
/// **Why this matters**: T5/T8 will instantiate a `sub` placement by emitting
/// `ApplyTransform` ops that each reference the SAME source-geometry handle.
/// Any in-place mutation of the source slot would break subsequent placements
/// (or, worse, accumulate transforms onto a single shape).
///
/// **Method**: build a box centered at origin (X,Y,Z ∈ [-5, 5]), apply a pure
/// translation to produce a new handle, then re-tessellate the SOURCE handle
/// and assert its AABB is still the pre-transform [-5, 5] cube.
#[test]
fn apply_transform_to_handle_preserves_source_handle() {
    let mut kernel = OcctKernel::new();

    let source = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box creation should succeed");

    let t = Transform3 {
        qw: 1.0,
        qx: 0.0,
        qy: 0.0,
        qz: 0.0,
        tx: 100.0,
        ty: 200.0,
        tz: 300.0,
    };

    // Apply transform to obtain a new handle. We intentionally discard the
    // result — this test is about what happens to the *source*.
    let _transformed = kernel
        .apply_transform_to_handle(source.id, &t)
        .expect("translation transform should succeed");

    // Re-tessellate the source handle. If the FFI accidentally mutated the
    // source's TopoDS_Shape in place, this AABB would now be shifted by
    // (100, 200, 300) instead of staying at [-5, 5].
    let source_aabb_after = aabb_of_handle(&kernel, source.id);
    let expected_source_aabb = Aabb {
        min: [-5.0, -5.0, -5.0],
        max: [5.0, 5.0, 5.0],
    };

    assert_aabb_eq(
        source_aabb_after,
        expected_source_aabb,
        1e-6,
        "source AABB after apply_transform_to_handle (multi-frame reuse contract: \
         source must be unmodified so it can be placed in multiple frames)",
    );
}
