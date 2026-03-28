//! Orientation, Frame, and Transform integration tests (task 237).
//!
//! Tests cover: quaternion normalization, axis-angle construction, euler-to-quaternion,
//! transform composition, identity no-op, frame construction, point transformation,
//! vector rotation vs translation, and numerical accuracy.

use reify_expr::{eval_expr, EvalContext};
use reify_stdlib::eval_builtin;
use reify_types::{BinOp, CompiledExpr, Type, Value, ValueMap};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Identity quaternion (no rotation).
fn identity_orientation() -> Value {
    Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }
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
fn eval_mul_expr(left_val: Value, left_ty: Type, right_val: Value, right_ty: Type, result_ty: Type) -> Value {
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
    let q = eval_builtin("orient_quaternion", &[
        Value::Real(3.0), Value::Real(4.0), Value::Real(0.0), Value::Real(0.0),
    ]);
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
    let q_pos = eval_builtin("orient_quaternion", &[
        Value::Real(1.0), Value::Real(1.0), Value::Real(1.0), Value::Real(1.0),
    ]);
    let q_neg = eval_builtin("orient_quaternion", &[
        Value::Real(-1.0), Value::Real(-1.0), Value::Real(-1.0), Value::Real(-1.0),
    ]);
    // Build transforms with each orientation and apply to a point
    let t_pos = make_transform(q_pos, 0.0, 0.0, 0.0);
    let t_neg = make_transform(q_neg, 0.0, 0.0, 0.0);
    let p = Value::Point(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]);
    let result_pos = eval_mul_expr(
        t_pos, Type::Transform(3),
        p.clone(), Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    let result_neg = eval_mul_expr(
        t_neg, Type::Transform(3),
        p, Type::point3(Type::length()),
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
        _ => panic!("expected Point results, got pos={:?}, neg={:?}", result_pos, result_neg),
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
    let q_pos = eval_builtin("orient_axis_angle", &[axis_z.clone(), Value::Real(std::f64::consts::FRAC_PI_4)]);
    let q_neg = eval_builtin("orient_axis_angle", &[axis_z, Value::Real(-std::f64::consts::FRAC_PI_4)]);
    // Compose the two transforms: apply pos then neg → should be identity
    let t_pos = make_transform(q_pos, 0.0, 0.0, 0.0);
    let t_neg = make_transform(q_neg, 0.0, 0.0, 0.0);
    let composed = eval_mul_expr(
        t_neg, Type::Transform(3),
        t_pos, Type::Transform(3),
        Type::Transform(3),
    );
    // Apply composed transform to point (7, -3, 42)
    let p = Value::Point(vec![Value::length(7.0), Value::length(-3.0), Value::length(42.0)]);
    let result = eval_mul_expr(
        composed, Type::Transform(3),
        p, Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_point_approx(&result, 7.0, -3.0, 42.0, "pos+neg rotation should be identity");
}

// ── Test 5-6: Euler-to-quaternion ──────────────────────────────────────────

/// orient_euler('xyz', a, b, c) should match composing axis_angle rotations X*Y*Z.
#[test]
fn euler_xyz_matches_sequential_axis_angle() {
    let a = std::f64::consts::FRAC_PI_3;  // pi/3
    let b = std::f64::consts::FRAC_PI_4;  // pi/4
    let c = std::f64::consts::FRAC_PI_6;  // pi/6

    // Euler intrinsic xyz: q = q_x(a) * q_y(b) * q_z(c)
    let q_euler = eval_builtin("orient_euler", &[
        Value::String("xyz".into()),
        Value::Real(a),
        Value::Real(b),
        Value::Real(c),
    ]);

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
        t_x, Type::Transform(3),
        t_y, Type::Transform(3),
        Type::Transform(3),
    );
    let t_xyz = eval_mul_expr(
        t_xy, Type::Transform(3),
        t_z, Type::Transform(3),
        Type::Transform(3),
    );

    // Extract composed rotation and compare with euler result
    match (q_euler, t_xyz) {
        (Value::Orientation { w: ew, x: ex, y: ey, z: ez },
         Value::Transform { rotation, .. }) => {
            assert_orientation_approx(&rotation, ew, ex, ey, ez,
                "euler xyz should match axis-angle composition");
        }
        (e, t) => panic!("unexpected types: euler={:?}, composed={:?}", e, t),
    }
}

/// orient_euler with different conventions but same angles produces different results.
#[test]
fn euler_different_conventions_produce_different_results() {
    let a = std::f64::consts::FRAC_PI_3;  // pi/3
    let b = std::f64::consts::FRAC_PI_4;  // pi/4
    let c = std::f64::consts::FRAC_PI_6;  // pi/6

    let q_xyz = eval_builtin("orient_euler", &[
        Value::String("xyz".into()),
        Value::Real(a), Value::Real(b), Value::Real(c),
    ]);
    let q_zyx = eval_builtin("orient_euler", &[
        Value::String("zyx".into()),
        Value::Real(a), Value::Real(b), Value::Real(c),
    ]);

    match (&q_xyz, &q_zyx) {
        (Value::Orientation { w: w1, x: x1, y: y1, z: z1 },
         Value::Orientation { w: w2, x: x2, y: y2, z: z2 }) => {
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
