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
