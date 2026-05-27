//! Orientation, Frame, and Transform integration tests (task 237).
//!
//! Tests cover: quaternion normalization, axis-angle construction, euler-to-quaternion,
//! transform composition, identity no-op, frame construction, point transformation,
//! vector rotation vs translation, and numerical accuracy.

use reify_expr::{EvalContext, eval_expr};
use reify_stdlib::eval_builtin;
use reify_core::Type;
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Identity quaternion (no rotation).
fn identity_orientation() -> Value {
    Value::Orientation {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    }
}

/// Build a Transform with given rotation and translation vector (LENGTH dimension).
fn make_transform(rotation: Value, tx: f64, ty: f64, tz: f64) -> Value {
    Value::Transform {
        rotation: Box::new(rotation),
        translation: Box::new(Value::Vector(vec![
            Value::length(tx),
            Value::length(ty),
            Value::length(tz),
        ])),
    }
}

/// Evaluate a binary multiplication expression.
fn eval_mul_expr(
    left_val: Value,
    left_ty: Type,
    right_val: Value,
    right_ty: Type,
    result_ty: Type,
) -> Value {
    let left = CompiledExpr::literal(left_val, left_ty);
    let right = CompiledExpr::literal(right_val, right_ty);
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, result_ty);
    let values = ValueMap::new();
    eval_expr(&expr, &EvalContext::simple(&values))
}

/// Assert an Orientation value has the expected components (handles sign ambiguity).
fn assert_orientation_approx(val: &Value, ew: f64, ex: f64, ey: f64, ez: f64, label: &str) {
    match val {
        Value::Orientation { w, x, y, z } => {
            let pos_ok = (w - ew).abs() < 1e-10
                && (x - ex).abs() < 1e-10
                && (y - ey).abs() < 1e-10
                && (z - ez).abs() < 1e-10;
            let neg_ok = (w + ew).abs() < 1e-10
                && (x + ex).abs() < 1e-10
                && (y + ey).abs() < 1e-10
                && (z + ez).abs() < 1e-10;
            assert!(
                pos_ok || neg_ok,
                "{label}: orientation ({w},{x},{y},{z}) != expected ({ew},{ex},{ey},{ez})"
            );
        }
        other => panic!("{label}: expected Orientation, got {:?}", other),
    }
}

/// Assert a Vector value has the expected components within tolerance.
fn assert_vector_approx(val: &Value, ex: f64, ey: f64, ez: f64, label: &str) {
    match val {
        Value::Vector(items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!(
                (x - ex).abs() < 1e-10 && (y - ey).abs() < 1e-10 && (z - ez).abs() < 1e-10,
                "{label}: vector ({x},{y},{z}) != expected ({ex},{ey},{ez})"
            );
        }
        other => panic!("{label}: expected Vector3, got {:?}", other),
    }
}

/// Assert a Point value has the expected components within tolerance.
fn assert_point_approx(val: &Value, ex: f64, ey: f64, ez: f64, label: &str) {
    match val {
        Value::Point(items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!(
                (x - ex).abs() < 1e-10 && (y - ey).abs() < 1e-10 && (z - ez).abs() < 1e-10,
                "{label}: point ({x},{y},{z}) != expected ({ex},{ey},{ez})"
            );
        }
        other => panic!("{label}: expected Point3, got {:?}", other),
    }
}

// ── Test 1-2: Quaternion normalization ───────────────────────────────────────

/// orient_quaternion(3,4,0,0) should produce a unit quaternion (|q| = 1).
#[test]
fn quat_normalization_produces_unit_length() {
    let q = eval_builtin(
        "orient_quaternion",
        &[
            Value::Real(3.0),
            Value::Real(4.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ],
    );
    match q {
        Value::Orientation { w, x, y, z } => {
            let norm = (w * w + x * x + y * y + z * z).sqrt();
            assert!(
                (norm - 1.0).abs() < 1e-12,
                "expected unit quaternion, got |q| = {norm}"
            );
            // (3,4,0,0) normalized = (3/5, 4/5, 0, 0)
            assert_orientation_approx(&q, 0.6, 0.8, 0.0, 0.0, "quat(3,4,0,0)");
        }
        other => panic!("expected Orientation, got {:?}", other),
    }
}

/// Negating all quaternion components (-w,-x,-y,-z) represents the same rotation.
/// Applying both to a point should give the same result.
#[test]
fn quat_negative_equivalent_same_rotation() {
    // Use (1,1,1,1) → normalizes to (0.5, 0.5, 0.5, 0.5)
    let q_pos = eval_builtin(
        "orient_quaternion",
        &[
            Value::Real(1.0),
            Value::Real(1.0),
            Value::Real(1.0),
            Value::Real(1.0),
        ],
    );
    let q_neg = eval_builtin(
        "orient_quaternion",
        &[
            Value::Real(-1.0),
            Value::Real(-1.0),
            Value::Real(-1.0),
            Value::Real(-1.0),
        ],
    );
    // Build transforms with each orientation and apply to a point
    let t_pos = make_transform(q_pos, 0.0, 0.0, 0.0);
    let t_neg = make_transform(q_neg, 0.0, 0.0, 0.0);
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::length(3.0),
    ]);
    let result_pos = eval_mul_expr(
        t_pos,
        Type::Transform(3),
        p.clone(),
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    let result_neg = eval_mul_expr(
        t_neg,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    // Both should produce the same point
    match (&result_pos, &result_neg) {
        (Value::Point(a), Value::Point(b)) if a.len() == 3 && b.len() == 3 => {
            for i in 0..3 {
                let va = a[i].as_f64().unwrap();
                let vb = b[i].as_f64().unwrap();
                assert!(
                    (va - vb).abs() < 1e-10,
                    "component {i}: pos={va}, neg={vb} differ"
                );
            }
        }
        _ => panic!(
            "expected Point results, got pos={:?}, neg={:?}",
            result_pos, result_neg
        ),
    }
}

// ── Test 3-4: Axis-angle construction ────────────────────────────────────────

/// A very small angle (1e-8) should produce a near-identity quaternion (w≈1, xyz≈0).
#[test]
fn axis_angle_small_angle_near_identity() {
    let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
    let angle = Value::Real(1e-8);
    let q = eval_builtin("orient_axis_angle", &[axis, angle]);
    match q {
        Value::Orientation { w, x, y, z } => {
            assert!((w - 1.0).abs() < 1e-6, "w should be ≈1, got {w}");
            assert!(x.abs() < 1e-6, "x should be ≈0, got {x}");
            assert!(y.abs() < 1e-6, "y should be ≈0, got {y}");
            assert!(z.abs() < 1e-6, "z should be ≈0, got {z}");
        }
        other => panic!("expected Orientation, got {:?}", other),
    }
}

/// Negative angle reverses rotation: applying angle then -angle to a point returns original.
#[test]
fn axis_angle_negative_angle_reverses_rotation() {
    let axis_z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
    let q_pos = eval_builtin(
        "orient_axis_angle",
        &[axis_z.clone(), Value::Real(std::f64::consts::FRAC_PI_4)],
    );
    let q_neg = eval_builtin(
        "orient_axis_angle",
        &[axis_z, Value::Real(-std::f64::consts::FRAC_PI_4)],
    );
    // Compose the two transforms: apply pos then neg → should be identity
    let t_pos = make_transform(q_pos, 0.0, 0.0, 0.0);
    let t_neg = make_transform(q_neg, 0.0, 0.0, 0.0);
    let composed = eval_mul_expr(
        t_neg,
        Type::Transform(3),
        t_pos,
        Type::Transform(3),
        Type::Transform(3),
    );
    // Apply composed transform to point (7, -3, 42)
    let p = Value::Point(vec![
        Value::length(7.0),
        Value::length(-3.0),
        Value::length(42.0),
    ]);
    let result = eval_mul_expr(
        composed,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(
        &result,
        7.0,
        -3.0,
        42.0,
        "pos+neg rotation should be identity",
    );
}

// ── Test 5-6: Euler-to-quaternion ──────────────────────────────────────────

/// orient_euler('xyz', a, b, c) should match composing axis_angle rotations X*Y*Z.
#[test]
fn euler_xyz_matches_sequential_axis_angle() {
    let a = std::f64::consts::FRAC_PI_3; // pi/3
    let b = std::f64::consts::FRAC_PI_4; // pi/4
    let c = std::f64::consts::FRAC_PI_6; // pi/6

    // Euler intrinsic xyz: q = q_x(a) * q_y(b) * q_z(c)
    let q_euler = eval_builtin(
        "orient_euler",
        &[
            Value::String("xyz".into()),
            Value::Real(a),
            Value::Real(b),
            Value::Real(c),
        ],
    );

    // Build sequential axis-angle rotations and compose via Transform * Transform
    let axis_x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
    let axis_y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
    let axis_z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
    let q_x = eval_builtin("orient_axis_angle", &[axis_x, Value::Real(a)]);
    let q_y = eval_builtin("orient_axis_angle", &[axis_y, Value::Real(b)]);
    let q_z = eval_builtin("orient_axis_angle", &[axis_z, Value::Real(c)]);

    // Compose: (q_x * q_y) * q_z via transform multiplication
    let t_x = make_transform(q_x, 0.0, 0.0, 0.0);
    let t_y = make_transform(q_y, 0.0, 0.0, 0.0);
    let t_z = make_transform(q_z, 0.0, 0.0, 0.0);
    let t_xy = eval_mul_expr(
        t_x,
        Type::Transform(3),
        t_y,
        Type::Transform(3),
        Type::Transform(3),
    );
    let t_xyz = eval_mul_expr(
        t_xy,
        Type::Transform(3),
        t_z,
        Type::Transform(3),
        Type::Transform(3),
    );

    // Extract composed rotation and compare with euler result
    match (q_euler, t_xyz) {
        (
            Value::Orientation {
                w: ew,
                x: ex,
                y: ey,
                z: ez,
            },
            Value::Transform { rotation, .. },
        ) => {
            assert_orientation_approx(
                &rotation,
                ew,
                ex,
                ey,
                ez,
                "euler xyz should match axis-angle composition",
            );
        }
        (e, t) => panic!("unexpected types: euler={:?}, composed={:?}", e, t),
    }
}

/// orient_euler with different conventions but same angles produces different results.
#[test]
fn euler_different_conventions_produce_different_results() {
    let a = std::f64::consts::FRAC_PI_3; // pi/3
    let b = std::f64::consts::FRAC_PI_4; // pi/4
    let c = std::f64::consts::FRAC_PI_6; // pi/6

    let q_xyz = eval_builtin(
        "orient_euler",
        &[
            Value::String("xyz".into()),
            Value::Real(a),
            Value::Real(b),
            Value::Real(c),
        ],
    );
    let q_zyx = eval_builtin(
        "orient_euler",
        &[
            Value::String("zyx".into()),
            Value::Real(a),
            Value::Real(b),
            Value::Real(c),
        ],
    );

    match (&q_xyz, &q_zyx) {
        (
            Value::Orientation {
                w: w1,
                x: x1,
                y: y1,
                z: z1,
            },
            Value::Orientation {
                w: w2,
                x: x2,
                y: y2,
                z: z2,
            },
        ) => {
            // Check that they are NOT the same rotation (neither positive nor negative match)
            let same_pos = (w1 - w2).abs() < 1e-10
                && (x1 - x2).abs() < 1e-10
                && (y1 - y2).abs() < 1e-10
                && (z1 - z2).abs() < 1e-10;
            let same_neg = (w1 + w2).abs() < 1e-10
                && (x1 + x2).abs() < 1e-10
                && (y1 + y2).abs() < 1e-10
                && (z1 + z2).abs() < 1e-10;
            assert!(
                !same_pos && !same_neg,
                "xyz and zyx with same angles should produce different rotations"
            );
        }
        _ => panic!("expected Orientation results"),
    }
}

// ── Test 7-9: Transform composition ────────────────────────────────────────

/// (A*B)*p equals A*(B*p) — composition-then-apply matches sequential application.
#[test]
fn compose_then_apply_equals_sequential() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    // A: 90° Z rotation + (5, 0, 0) translation
    let rot_z90 = Value::Orientation {
        w: s,
        x: 0.0,
        y: 0.0,
        z: s,
    };
    let t_a = make_transform(rot_z90, 5.0, 0.0, 0.0);
    // B: 90° X rotation + (0, 3, 0) translation
    let rot_x90 = Value::Orientation {
        w: s,
        x: s,
        y: 0.0,
        z: 0.0,
    };
    let t_b = make_transform(rot_x90, 0.0, 3.0, 0.0);
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::length(3.0),
    ]);

    // Path 1: compose (A*B), then apply to p
    let t_ab = eval_mul_expr(
        t_a.clone(),
        Type::Transform(3),
        t_b.clone(),
        Type::Transform(3),
        Type::Transform(3),
    );
    let result_composed = eval_mul_expr(
        t_ab,
        Type::Transform(3),
        p.clone(),
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );

    // Path 2: apply B*p, then A*(B*p)
    let bp = eval_mul_expr(
        t_b,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    let result_sequential = eval_mul_expr(
        t_a,
        Type::Transform(3),
        bp,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );

    // Both paths should give the same point
    match (&result_composed, &result_sequential) {
        (Value::Point(a), Value::Point(b)) if a.len() == 3 && b.len() == 3 => {
            for i in 0..3 {
                let va = a[i].as_f64().unwrap();
                let vb = b[i].as_f64().unwrap();
                assert!(
                    (va - vb).abs() < 1e-10,
                    "component {i}: composed={va}, sequential={vb}"
                );
            }
        }
        _ => panic!("expected Point results"),
    }
}

/// ((C*B)*A)*p matches C*(B*(A*p)) — three-way composition is associative.
#[test]
fn three_way_composition_correctness() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    // A: 90° Z rotation + (1, 0, 0)
    let t_a = make_transform(
        Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        },
        1.0,
        0.0,
        0.0,
    );
    // B: 90° X rotation + (0, 2, 0)
    let t_b = make_transform(
        Value::Orientation {
            w: s,
            x: s,
            y: 0.0,
            z: 0.0,
        },
        0.0,
        2.0,
        0.0,
    );
    // C: 90° Y rotation + (0, 0, 3)
    let t_c = make_transform(
        Value::Orientation {
            w: s,
            x: 0.0,
            y: s,
            z: 0.0,
        },
        0.0,
        0.0,
        3.0,
    );
    let p = Value::Point(vec![
        Value::length(2.0),
        Value::length(-1.0),
        Value::length(4.0),
    ]);

    // Path 1: ((C*B)*A)*p
    let cb = eval_mul_expr(
        t_c.clone(),
        Type::Transform(3),
        t_b.clone(),
        Type::Transform(3),
        Type::Transform(3),
    );
    let cba = eval_mul_expr(
        cb,
        Type::Transform(3),
        t_a.clone(),
        Type::Transform(3),
        Type::Transform(3),
    );
    let result_composed = eval_mul_expr(
        cba,
        Type::Transform(3),
        p.clone(),
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );

    // Path 2: C*(B*(A*p))
    let ap = eval_mul_expr(
        t_a,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    let bap = eval_mul_expr(
        t_b,
        Type::Transform(3),
        ap,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    let result_sequential = eval_mul_expr(
        t_c,
        Type::Transform(3),
        bap,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );

    match (&result_composed, &result_sequential) {
        (Value::Point(a), Value::Point(b)) if a.len() == 3 && b.len() == 3 => {
            for i in 0..3 {
                let va = a[i].as_f64().unwrap();
                let vb = b[i].as_f64().unwrap();
                assert!(
                    (va - vb).abs() < 1e-10,
                    "component {i}: composed={va}, sequential={vb}"
                );
            }
        }
        _ => panic!("expected Point results"),
    }
}

/// A*B ≠ B*A — transform composition is non-commutative.
#[test]
fn composition_is_non_commutative() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    // A: 90° Z rotation + (10, 0, 0)
    let t_a = make_transform(
        Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        },
        10.0,
        0.0,
        0.0,
    );
    // B: 90° X rotation + (0, 5, 0)
    let t_b = make_transform(
        Value::Orientation {
            w: s,
            x: s,
            y: 0.0,
            z: 0.0,
        },
        0.0,
        5.0,
        0.0,
    );

    let ab = eval_mul_expr(
        t_a.clone(),
        Type::Transform(3),
        t_b.clone(),
        Type::Transform(3),
        Type::Transform(3),
    );
    let ba = eval_mul_expr(
        t_b,
        Type::Transform(3),
        t_a,
        Type::Transform(3),
        Type::Transform(3),
    );

    // Apply both to a test point and verify they differ
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(1.0),
        Value::length(1.0),
    ]);
    let result_ab = eval_mul_expr(
        ab,
        Type::Transform(3),
        p.clone(),
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    let result_ba = eval_mul_expr(
        ba,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );

    match (&result_ab, &result_ba) {
        (Value::Point(a), Value::Point(b)) if a.len() == 3 && b.len() == 3 => {
            let mut any_differ = false;
            for i in 0..3 {
                let va = a[i].as_f64().unwrap();
                let vb = b[i].as_f64().unwrap();
                if (va - vb).abs() > 1e-10 {
                    any_differ = true;
                }
            }
            assert!(any_differ, "A*B and B*A should produce different results");
        }
        _ => panic!("expected Point results"),
    }
}

// ── Test 10-11: Identity transform no-op ───────────────────────────────────

/// Identity transform on an arbitrary point returns the exact same point.
#[test]
fn identity_noop_arbitrary_point() {
    let t_id = make_transform(identity_orientation(), 0.0, 0.0, 0.0);
    let p = Value::Point(vec![
        Value::length(7.0),
        Value::length(-3.0),
        Value::length(42.0),
    ]);
    let result = eval_mul_expr(
        t_id,
        Type::Transform(3),
        p.clone(),
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(&result, 7.0, -3.0, 42.0, "identity should not change point");
}

/// Identity transform on an arbitrary vector returns the exact same vector.
#[test]
fn identity_noop_arbitrary_vector() {
    let t_id = make_transform(identity_orientation(), 0.0, 0.0, 0.0);
    let v = Value::Vector(vec![
        Value::length(5.0),
        Value::length(-2.0),
        Value::length(11.0),
    ]);
    let result = eval_mul_expr(
        t_id,
        Type::Transform(3),
        v.clone(),
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert_vector_approx(
        &result,
        5.0,
        -2.0,
        11.0,
        "identity should not change vector",
    );
}

// ── Test 12-13: Frame construction ─────────────────────────────────────────

/// Frame stores origin and basis correctly; they can be recovered.
#[test]
fn frame_round_trip_construction() {
    let axis_z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
    let orient = eval_builtin(
        "orient_axis_angle",
        &[axis_z, Value::Real(std::f64::consts::FRAC_PI_4)],
    );
    let origin = Value::Point(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::length(3.0),
    ]);
    let frame = eval_builtin("frame3", &[origin.clone(), orient.clone()]);
    match frame {
        Value::Frame {
            origin: o,
            basis: b,
        } => {
            assert_point_approx(&o, 1.0, 2.0, 3.0, "frame origin");
            match (&orient, &*b) {
                (
                    Value::Orientation {
                        w: ew,
                        x: ex,
                        y: ey,
                        z: ez,
                    },
                    Value::Orientation { w, x, y, z },
                ) => {
                    assert!(
                        (w - ew).abs() < 1e-12
                            && (x - ex).abs() < 1e-12
                            && (y - ey).abs() < 1e-12
                            && (z - ez).abs() < 1e-12,
                        "frame basis should match input orientation"
                    );
                }
                _ => panic!("expected Orientation basis"),
            }
        }
        other => panic!("expected Frame, got {:?}", other),
    }
}

/// frame_to_frame(A,B) composed with frame_to_frame(B,A) is identity (round-trip).
#[test]
fn frame_to_frame_inverse_is_identity() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let orient_a = Value::Orientation {
        w: s,
        x: 0.0,
        y: 0.0,
        z: s,
    }; // 90° Z
    let frame_a = eval_builtin(
        "frame3",
        &[
            Value::Point(vec![
                Value::length(5.0),
                Value::length(3.0),
                Value::length(1.0),
            ]),
            orient_a,
        ],
    );
    let orient_b = Value::Orientation {
        w: s,
        x: s,
        y: 0.0,
        z: 0.0,
    }; // 90° X
    let frame_b = eval_builtin(
        "frame3",
        &[
            Value::Point(vec![
                Value::length(-2.0),
                Value::length(7.0),
                Value::length(4.0),
            ]),
            orient_b,
        ],
    );

    let t_ab = eval_builtin("frame_to_frame", &[frame_a.clone(), frame_b.clone()]);
    let t_ba = eval_builtin("frame_to_frame", &[frame_b, frame_a]);

    // Compose t_ba * t_ab → should be identity
    let composed = eval_mul_expr(
        t_ba,
        Type::Transform(3),
        t_ab,
        Type::Transform(3),
        Type::Transform(3),
    );

    // Apply to arbitrary point — should return the same point
    let p = Value::Point(vec![
        Value::length(11.0),
        Value::length(-7.0),
        Value::length(3.5),
    ]);
    let result = eval_mul_expr(
        composed,
        Type::Transform(3),
        p.clone(),
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(
        &result,
        11.0,
        -7.0,
        3.5,
        "frame_to_frame round-trip should be identity",
    );
}

// ── Test 14-15: Point transformation correctness ───────────────────────────

/// 90° about each principal axis sends basis vectors to correct locations.
#[test]
fn rotation_90_about_each_principal_axis() {
    let s = std::f64::consts::FRAC_1_SQRT_2;

    // 90° about X: (0,1,0) → (0,0,1)
    let rot_x = Value::Orientation {
        w: s,
        x: s,
        y: 0.0,
        z: 0.0,
    };
    let p_y = Value::Point(vec![
        Value::length(0.0),
        Value::length(1.0),
        Value::length(0.0),
    ]);
    let result_x = eval_mul_expr(
        make_transform(rot_x, 0.0, 0.0, 0.0),
        Type::Transform(3),
        p_y,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(&result_x, 0.0, 0.0, 1.0, "90° X: (0,1,0) → (0,0,1)");

    // 90° about Y: (1,0,0) → (0,0,-1)
    let rot_y = Value::Orientation {
        w: s,
        x: 0.0,
        y: s,
        z: 0.0,
    };
    let p_x = Value::Point(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
    let result_y = eval_mul_expr(
        make_transform(rot_y, 0.0, 0.0, 0.0),
        Type::Transform(3),
        p_x,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(&result_y, 0.0, 0.0, -1.0, "90° Y: (1,0,0) → (0,0,-1)");

    // 90° about Z: (1,0,0) → (0,1,0)
    let rot_z = Value::Orientation {
        w: s,
        x: 0.0,
        y: 0.0,
        z: s,
    };
    let p_x2 = Value::Point(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
    let result_z = eval_mul_expr(
        make_transform(rot_z, 0.0, 0.0, 0.0),
        Type::Transform(3),
        p_x2,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(&result_z, 0.0, 1.0, 0.0, "90° Z: (1,0,0) → (0,1,0)");
}

/// Translation-only transform (identity rotation) shifts point by translation vector.
#[test]
fn translation_only_shifts_point() {
    let t = make_transform(identity_orientation(), 5.0, 10.0, 15.0);
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::length(3.0),
    ]);
    let result = eval_mul_expr(
        t,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(&result, 6.0, 12.0, 18.0, "translation shifts point");
}

// ── Test 16-17: Vector rotation vs translation ─────────────────────────────

/// Same transform applied to Vector vs Point: translation only affects Point.
#[test]
fn same_transform_point_vs_vector_contrast() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    // 90° Z + (10, 20, 30)
    let t = make_transform(
        Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        },
        10.0,
        20.0,
        30.0,
    );
    let coords = || (Value::length(1.0), Value::length(0.0), Value::length(0.0));

    // Vector: rotation only → (1,0,0) rotated 90° Z → (0,1,0), NO translation
    let (vx, vy, vz) = coords();
    let v = Value::Vector(vec![vx, vy, vz]);
    let v_result = eval_mul_expr(
        t.clone(),
        Type::Transform(3),
        v,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert_vector_approx(
        &v_result,
        0.0,
        1.0,
        0.0,
        "vector: rotation only, no translation",
    );

    // Point: rotation + translation → (0,1,0) + (10,20,30) = (10,21,30)
    let (px, py, pz) = coords();
    let p = Value::Point(vec![px, py, pz]);
    let p_result = eval_mul_expr(
        t,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(&p_result, 10.0, 21.0, 30.0, "point: rotation + translation");
}

/// Rotating a vector preserves its magnitude.
#[test]
fn vector_rotation_preserves_magnitude() {
    // Vector (3, 4, 0) has magnitude 5
    let v = Value::Vector(vec![
        Value::length(3.0),
        Value::length(4.0),
        Value::length(0.0),
    ]);
    // 45° Z rotation
    let cos_22_5 = (std::f64::consts::FRAC_PI_4 / 2.0).cos();
    let sin_22_5 = (std::f64::consts::FRAC_PI_4 / 2.0).sin();
    let rot = Value::Orientation {
        w: cos_22_5,
        x: 0.0,
        y: 0.0,
        z: sin_22_5,
    };
    let t = make_transform(rot, 0.0, 0.0, 0.0);
    let result = eval_mul_expr(
        t,
        Type::Transform(3),
        v,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    match result {
        Value::Vector(items) if items.len() == 3 => {
            let rx = items[0].as_f64().unwrap();
            let ry = items[1].as_f64().unwrap();
            let rz = items[2].as_f64().unwrap();
            let mag = (rx * rx + ry * ry + rz * rz).sqrt();
            assert!(
                (mag - 5.0).abs() < 1e-10,
                "magnitude should be preserved: got {mag}, expected 5"
            );
        }
        other => panic!("expected Vector, got {:?}", other),
    }
}

// ── Test 18-19: Numerical accuracy ─────────────────────────────────────────

/// orient_quaternion with very large inputs still normalizes to unit length.
#[test]
fn quaternion_normalization_unit_length_after_large_input() {
    let q = eval_builtin(
        "orient_quaternion",
        &[
            Value::Real(1e8),
            Value::Real(1e8),
            Value::Real(1e8),
            Value::Real(1e8),
        ],
    );
    match q {
        Value::Orientation { w, x, y, z } => {
            let norm = (w * w + x * x + y * y + z * z).sqrt();
            assert!(
                (norm - 1.0).abs() < 1e-12,
                "expected unit quaternion after large input, got |q| = {norm}"
            );
        }
        other => panic!("expected Orientation, got {:?}", other),
    }
}

/// Composing 90° Z rotation 4 times (= 360° = identity), verify result matches original.
#[test]
fn accumulated_composition_error_bounded() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let rot_z90 = Value::Orientation {
        w: s,
        x: 0.0,
        y: 0.0,
        z: s,
    };
    let t_90 = make_transform(rot_z90, 0.0, 0.0, 0.0);

    // Compose 4 times: 90° * 4 = 360°
    let t_180 = eval_mul_expr(
        t_90.clone(),
        Type::Transform(3),
        t_90.clone(),
        Type::Transform(3),
        Type::Transform(3),
    );
    let t_360 = eval_mul_expr(
        t_180.clone(),
        Type::Transform(3),
        t_180,
        Type::Transform(3),
        Type::Transform(3),
    );

    // Apply to arbitrary point
    let p = Value::Point(vec![
        Value::length(3.7),
        Value::length(-2.1),
        Value::length(8.5),
    ]);
    let result = eval_mul_expr(
        t_360,
        Type::Transform(3),
        p.clone(),
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(
        &result,
        3.7,
        -2.1,
        8.5,
        "360° rotation should return original point within 1e-10",
    );
}
