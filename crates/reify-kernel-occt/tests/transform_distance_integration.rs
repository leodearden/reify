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
use reify_ir::{GeometryHandleId, GeometryOp, QueryError, Value};

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
// distance_with_transform — rigid-invariance pin (rotation-only)
// ---------------------------------------------------------------------------

/// Rotation-rigid-invariance and quaternion-convention check.
///
/// **Fixture** (asymmetric — defeats the cube-invariance blind spot):
/// - `box_a`: a 12×8×6 brick centered at origin (X∈[-6,6], Y∈[-4,4], Z∈[-3,3]).
///   The non-cube shape is critical: a 90°-Z rotation shrinks its X-extent to ±4
///   (the old Y-extent), yielding a different distance to `box_b` than any other axis.
/// - `box_b`: a 10×10×10 cube translated to (30, 0, 0) (X∈[25,35]).
///
/// **Expected distances** under a 90°-Z rotation of `box_a`:
/// - Correct Z-rotation: rotated brick has X∈[-4,4] → distance to X∈[25,35] is **21 mm**.
/// - Wrong-axis result (xyzw/wxyz swap → interpreted as 90°-X): brick keeps X∈[-6,6] →
///   distance is **19 mm**. The 2 mm delta is ~2×10⁶× the assertion tolerance, so any
///   quaternion-swap bug definitively fails this test.
///
/// **Contract**: `distance_with_transform(a, b, t)` pre-composes `t` into the
/// cheaper-by-topology side. When |a| ≤ |b| (both rectangular solids have 18
/// topo entities → equal → the a-side branch applies), this equals
/// `min_clearance(T·box_a, box_b)`. The baseline is computed by baking the
/// rotation into `box_a` via `GeometryOp::Rotate` and calling `min_clearance`.
///
/// The 90°-Z rotation quaternion: `qw = cos(π/4)`, `qz = sin(π/4)`, `qx = qy = 0`.
#[test]
fn distance_with_transform_rotation_only_matches_rotated_shape() {
    // Build an asymmetric fixture inline (avoids two_box_kernel's cube symmetry).
    let mut kernel = OcctKernel::new();

    // box_a: 12×8×6 brick centered at origin. Width→X, Height→Y, Depth→Z.
    let box_a = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(12.0),
            height: Value::Real(8.0),
            depth: Value::Real(6.0),
        })
        .expect("box_a (12×8×6 brick) creation should succeed");
    let box_a_id = box_a.id;

    // box_b: 10×10×10 cube translated to (30, 0, 0) → X∈[25,35].
    let box_b_raw = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box_b_raw creation should succeed");
    let box_b = kernel
        .execute(&GeometryOp::Translate {
            target: box_b_raw.id,
            dx: 30.0,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("box_b translate to (30,0,0) should succeed");
    let box_b_id = box_b.id;

    // Baseline: bake a 90°-Z rotation into box_a, then measure min_clearance.
    // After rotation: box_a's X-extent is ±4 (old Y-extent), distance to box_b = 21mm.
    let rotated_a = kernel
        .execute(&GeometryOp::Rotate {
            target: box_a_id,
            axis: [0.0, 0.0, 1.0],
            angle_rad: PI / 2.0,
        })
        .expect("rotate box_a 90° Z should succeed");
    let baseline = kernel
        .min_clearance(rotated_a.id, box_b_id)
        .expect("min_clearance(rotated_a, box_b) should succeed");

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

    // Under-transform: distance_with_transform(a, b, t) applies t to a (the first arg)
    // per the contract at lib.rs:855 — equivalent to min_clearance(T·box_a, box_b).
    let under_transform = kernel
        .distance_with_transform(box_a_id, box_b_id, &t_rel)
        .expect("distance_with_transform should succeed");

    assert!(
        (under_transform - baseline).abs() < 1e-6,
        "rotation rigid-invariance failed: \
         distance_with_transform = {under_transform}, \
         min_clearance(rotated_a) = {baseline}, \
         delta = {} (expected ≈21mm; a wrong-axis quaternion would give ≈19mm)",
        (under_transform - baseline).abs()
    );
}

// ---------------------------------------------------------------------------
// distance_with_transform — rigid-invariance pin (rotation + translation combined)
// ---------------------------------------------------------------------------

/// Combined-transform rigid-invariance: verifies that `distance_with_transform`
/// handles the full SE(3) case (both rotation and translation nonzero), catching
/// bugs where `SetRotation` / `SetTranslationPart` compose in the wrong order.
///
/// **Fixture** (same asymmetric brick as the rotation-only test):
/// - `box_a`: 12×8×6 brick centered at origin (X∈[-6,6], Y∈[-4,4], Z∈[-3,3]).
/// - `box_b`: 10×10×10 cube translated to (30, 0, 0) (X∈[25,35]).
///
/// **Transform**: 90°-Z rotation (`qw=cos(π/4)`, `qz=sin(π/4)`) **and** +5mm X translation.
///
/// `gp_Trsf` applies as `p' = R·p + t` (rotate-then-translate). After the combined transform:
/// - 90°-Z rotates box_a: X-extent shrinks to ±4 (old Y-extent).
/// - +5mm X translation shifts it: X∈[1, 9].
/// - Distance to box_b (X∈[25,35]) = **16 mm**.
///
/// A translate-before-rotate bug would give ≈21 mm, a ~5 mm delta well above 1e-6.
#[test]
fn distance_with_transform_rotation_and_translation_matches_composed_shape() {
    let mut kernel = OcctKernel::new();

    // box_a: 12×8×6 brick centered at origin.
    let box_a = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(12.0),
            height: Value::Real(8.0),
            depth: Value::Real(6.0),
        })
        .expect("box_a (12×8×6 brick) creation should succeed");
    let box_a_id = box_a.id;

    // box_b: 10×10×10 cube translated to (30, 0, 0) → X∈[25,35].
    let box_b_raw = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box_b_raw creation should succeed");
    let box_b = kernel
        .execute(&GeometryOp::Translate {
            target: box_b_raw.id,
            dx: 30.0,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("box_b translate to (30,0,0) should succeed");
    let box_b_id = box_b.id;

    // Baseline: bake Rotate(box_a, 90°-Z) then Translate(+5mm X), then min_clearance.
    // GeometryOps follow the same compose-order as gp_Trsf (rotate first, translate second).
    let rotated_a = kernel
        .execute(&GeometryOp::Rotate {
            target: box_a_id,
            axis: [0.0, 0.0, 1.0],
            angle_rad: PI / 2.0,
        })
        .expect("rotate box_a 90°-Z should succeed");
    let composed_a = kernel
        .execute(&GeometryOp::Translate {
            target: rotated_a.id,
            dx: 5.0,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("translate rotated_a by +5mm X should succeed");
    let baseline = kernel
        .min_clearance(composed_a.id, box_b_id)
        .expect("min_clearance(composed_a, box_b) should succeed");

    // Combined Transform3: 90°-Z rotation + 5mm X translation.
    let t_rel = Transform3 {
        qw: (PI / 4.0).cos(),
        qx: 0.0,
        qy: 0.0,
        qz: (PI / 4.0).sin(),
        tx: 5.0,
        ty: 0.0,
        tz: 0.0,
    };
    let under_transform = kernel
        .distance_with_transform(box_a_id, box_b_id, &t_rel)
        .expect("distance_with_transform should succeed");

    assert!(
        (under_transform - baseline).abs() < 1e-6,
        "combined rotation+translation rigid-invariance failed: \
         distance_with_transform = {under_transform}, \
         min_clearance(rotate_then_translate(box_a)) = {baseline}, \
         delta = {} (expected ≈16mm; translate-before-rotate bug would give ≈21mm)",
        (under_transform - baseline).abs()
    );
}

// ---------------------------------------------------------------------------
// distance_with_transform — identity-transform sanity check
// ---------------------------------------------------------------------------

/// Identity transform: `distance_with_transform(a, b, Identity)` must match
/// `min_clearance(a, b)` within 1e-9 (tighter than the translation test's 1e-6
/// because no floating-point math is done on the transform itself).
#[test]
fn distance_with_transform_identity_equals_min_clearance() {
    let (kernel, box_a_id, box_b_id) = two_box_kernel(50.0);

    let baseline = kernel
        .min_clearance(box_a_id, box_b_id)
        .expect("min_clearance should succeed");

    let identity = Transform3 {
        qw: 1.0,
        qx: 0.0,
        qy: 0.0,
        qz: 0.0,
        tx: 0.0,
        ty: 0.0,
        tz: 0.0,
    };
    let under_transform = kernel
        .distance_with_transform(box_a_id, box_b_id, &identity)
        .expect("distance_with_transform(identity) should succeed");

    assert!(
        (under_transform - baseline).abs() < 1e-9,
        "identity-transform distance {under_transform} != min_clearance {baseline}, \
         delta = {}",
        (under_transform - baseline).abs()
    );
}

// ---------------------------------------------------------------------------
// Error paths — invalid handles for both methods
// ---------------------------------------------------------------------------

/// Unknown first handle in `distance_with_transform` returns `InvalidHandle(a)`.
#[test]
fn distance_with_transform_unknown_first_handle_returns_invalid_handle() {
    let (kernel, box_id, _) = two_box_kernel(50.0);
    let unknown = GeometryHandleId(998);
    let t_rel = Transform3 { qw: 1.0, qx: 0.0, qy: 0.0, qz: 0.0, tx: 0.0, ty: 0.0, tz: 0.0 };
    match kernel.distance_with_transform(unknown, box_id, &t_rel) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})"
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

/// Unknown second handle in `distance_with_transform` returns `InvalidHandle(b)`.
#[test]
fn distance_with_transform_unknown_second_handle_returns_invalid_handle() {
    let (kernel, box_id, _) = two_box_kernel(50.0);
    let unknown = GeometryHandleId(999);
    let t_rel = Transform3 { qw: 1.0, qx: 0.0, qy: 0.0, qz: 0.0, tx: 0.0, ty: 0.0, tz: 0.0 };
    match kernel.distance_with_transform(box_id, unknown, &t_rel) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})"
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

/// Unknown first handle in `interferes_with_transform` returns `InvalidHandle(a)`.
#[test]
fn interferes_with_transform_unknown_first_handle_returns_invalid_handle() {
    let (kernel, box_id, _) = two_box_kernel(50.0);
    let unknown = GeometryHandleId(998);
    let t_rel = Transform3 { qw: 1.0, qx: 0.0, qy: 0.0, qz: 0.0, tx: 0.0, ty: 0.0, tz: 0.0 };
    match kernel.interferes_with_transform(unknown, box_id, &t_rel) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})"
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

/// Unknown second handle in `interferes_with_transform` returns `InvalidHandle(b)`.
#[test]
fn interferes_with_transform_unknown_second_handle_returns_invalid_handle() {
    let (kernel, box_id, _) = two_box_kernel(50.0);
    let unknown = GeometryHandleId(999);
    let t_rel = Transform3 { qw: 1.0, qx: 0.0, qy: 0.0, qz: 0.0, tx: 0.0, ty: 0.0, tz: 0.0 };
    match kernel.interferes_with_transform(box_id, unknown, &t_rel) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})"
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// interferes_with_transform — overlap / disjoint probes
// ---------------------------------------------------------------------------

/// A +45mm X translation moves box_a into box_b (dx=50 → gap of 40; +45 means
/// the boxes now overlap by 5mm). `interferes_with_transform` must return true.
#[test]
fn interferes_with_transform_returns_true_for_overlap_after_transform() {
    let (kernel, box_a_id, box_b_id) = two_box_kernel(50.0);
    // t_rel shifts box_a by +45mm X: centre moves to (45,0,0), overlapping box_b at (50,0,0).
    let t_rel = Transform3 {
        qw: 1.0,
        qx: 0.0,
        qy: 0.0,
        qz: 0.0,
        tx: 45.0,
        ty: 0.0,
        tz: 0.0,
    };
    match kernel.interferes_with_transform(box_a_id, box_b_id, &t_rel) {
        Ok(true) => {}
        Ok(false) => panic!(
            "overlapping configuration (dx+45 into 40-unit gap) should interfere, got Ok(false)"
        ),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

/// Identity transform on the disjoint (dx=50, 40-unit gap) fixture: boxes stay
/// disjoint, `interferes_with_transform` must return false.
#[test]
fn interferes_with_transform_returns_false_for_disjoint_after_transform() {
    let (kernel, box_a_id, box_b_id) = two_box_kernel(50.0);
    let t_rel = Transform3 {
        qw: 1.0,
        qx: 0.0,
        qy: 0.0,
        qz: 0.0,
        tx: 0.0,
        ty: 0.0,
        tz: 0.0,
    };
    match kernel.interferes_with_transform(box_a_id, box_b_id, &t_rel) {
        Ok(false) => {}
        Ok(true) => panic!(
            "disjoint configuration (identity transform, 40-unit gap) should not interfere, got Ok(true)"
        ),
        Err(e) => panic!("expected Ok(false), got Err({e:?})"),
    }
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
