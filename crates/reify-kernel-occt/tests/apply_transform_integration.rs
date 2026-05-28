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
