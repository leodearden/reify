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
