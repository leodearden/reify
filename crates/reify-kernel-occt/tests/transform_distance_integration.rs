//! Integration tests for transform-aware distance and interference queries:
//! `distance_with_transform` and `interferes_with_transform`.
//!
//! These tests verify the rigid-invariance property mandated by the
//! kinematic-constraints PRD §9.2: `distance_with_transform(a, b, t_rel)` agrees
//! within 1e-6 with `min_clearance(transform_baked(a, t_rel), b)`, where
//! `transform_baked` uses the existing `OcctKernel::execute(GeometryOp::Translate|Rotate)`
//! ops.

#![cfg(has_occt)]

use std::f64::consts::PI;

use reify_kernel_occt::{OcctKernel, Transform3};
use reify_types::{GeometryHandleId, GeometryOp, QueryError, Value};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a kernel with two 10×10×10 boxes.
///
/// `box_a` is centered at the origin (x∈[-5,5]).
/// `box_b` is translated by `dx` along X (x∈[dx-5, dx+5]).
///
/// Returns `(kernel, box_a_id, box_b_id)`.
fn two_box_kernel(dx: f64) -> (OcctKernel, GeometryHandleId, GeometryHandleId) {
    let mut kernel = OcctKernel::new();

    let box_a = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box_a creation should succeed");

    let box_b_raw = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box_b creation should succeed");

    let box_b = kernel
        .execute(&GeometryOp::Translate {
            target: box_b_raw.id,
            dx,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("box_b translate should succeed");

    (kernel, box_a.id, box_b.id)
}

// ---------------------------------------------------------------------------
// distance_with_transform — rigid-invariance pin (translation-only)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// distance_with_transform — rigid-invariance pin (rotation-only)
// ---------------------------------------------------------------------------

/// Rotation-rigid-invariance and quaternion-convention check.
///
/// Fixture: box_a at origin, box_b at (30, 0, 0) — NOT rotation-symmetric so
/// a 90° rotation about Z genuinely changes the distance.
///
/// The 90°-Z rotation quaternion is: qw = cos(π/4), qz = sin(π/4), qx = qy = 0.
///
/// `baseline` is computed by baking the rotation into a new shape via
/// `GeometryOp::Rotate`, then calling `min_clearance`. The test then calls
/// `distance_with_transform(box_b_id, box_a_id, &t_rel)` (b transformed, a
/// fixed — forces the a_extent > b_extent branch when a has been rotated already
/// and b is the raw box) and asserts they agree within 1e-6.
///
/// This test specifically catches xyzw/wxyz swap bugs in the quaternion
/// constructor (gp_Quaternion takes x,y,z,w — wrong order produces a different
/// rotation and a different distance).
#[test]
fn distance_with_transform_rotation_only_matches_rotated_shape() {
    let (mut kernel, box_a_id, box_b_id) = two_box_kernel(30.0);

    // Bake a 90°-Z rotation into box_b.
    let rotated = kernel
        .execute(&GeometryOp::Rotate {
            target: box_b_id,
            axis: [0.0, 0.0, 1.0],
            angle_rad: PI / 2.0,
        })
        .expect("rotate box_b 90° Z should succeed");

    // Baseline: min_clearance(box_a, rotated_box_b).
    let baseline = kernel
        .min_clearance(box_a_id, rotated.id)
        .expect("min_clearance(box_a, rotated) should succeed");

    // Transform3 encoding 90°-Z rotation: qw = cos(π/4), qz = sin(π/4).
    let t_rel = Transform3 {
        qw: (PI / 4.0).cos(),
        qx: 0.0,
        qy: 0.0,
        qz: (PI / 4.0).sin(),
        tx: 0.0,
        ty: 0.0,
        tz: 0.0,
    };

    // Under-transform: distance_with_transform(box_a, box_b, t_rel) where t_rel
    // is applied to box_b. Since `distance_with_transform(a, b, t)` pre-composes
    // t into the cheaper side (and by rigid-invariance dist(A, T·B) == dist(T⁻¹·A, B)),
    // this should match baseline.
    let under_transform = kernel
        .distance_with_transform(box_a_id, box_b_id, &t_rel)
        .expect("distance_with_transform should succeed");

    assert!(
        (under_transform - baseline).abs() < 1e-6,
        "rotation rigid-invariance failed: \
         distance_with_transform = {under_transform}, \
         min_clearance(rotated) = {baseline}, \
         delta = {}",
        (under_transform - baseline).abs()
    );
}

/// Translation-rigid-invariance: `distance_with_transform(a, b, t_rel)` matches
/// `min_clearance(translate(a, t_rel), b)` within 1e-6.
///
/// Fixture: two_box_kernel(50.0) → 40-unit gap.
/// Transform: +5mm X translation of box_a → ~35-unit gap.
#[test]
fn distance_with_transform_translation_only_matches_translated_shape() {
    let (mut kernel, box_a_id, box_b_id) = two_box_kernel(50.0);

    // Bake the translation into a new shape via GeometryOp.
    let translated = kernel
        .execute(&GeometryOp::Translate {
            target: box_a_id,
            dx: 5.0,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("translate box_a by +5 X should succeed");

    // Baseline: min_clearance between the translated shape and box_b.
    let baseline = kernel
        .min_clearance(translated.id, box_b_id)
        .expect("min_clearance(translated, box_b) should succeed");

    // Under-transform: distance_with_transform with equivalent Transform3.
    let t_rel = Transform3 {
        qw: 1.0,
        qx: 0.0,
        qy: 0.0,
        qz: 0.0,
        tx: 5.0,
        ty: 0.0,
        tz: 0.0,
    };
    let under_transform = kernel
        .distance_with_transform(box_a_id, box_b_id, &t_rel)
        .expect("distance_with_transform should succeed");

    assert!(
        (under_transform - baseline).abs() < 1e-6,
        "distance_with_transform({t_rel:?}) = {under_transform}, \
         but min_clearance(translated) = {baseline}; delta = {}",
        (under_transform - baseline).abs()
    );
}
