//! Transform operation evaluation tests: Transform * Vector, Transform * Point,
//! Transform * Transform composition.

use reify_expr::{eval_expr, EvalContext};
use reify_types::{BinOp, CompiledExpr, DimensionVector, Type, Value, ValueMap};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Identity quaternion (no rotation).
fn identity_orientation() -> Value {
    Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }
}

/// 90-degree rotation about the Z axis: cos(45deg) = sin(45deg) = sqrt(2)/2.
fn rotation_90z() -> Value {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    Value::Orientation { w: s, x: 0.0, y: 0.0, z: s }
}

/// Identity transform (no rotation, zero LENGTH translation).
fn identity_transform() -> Value {
    Value::Transform {
        rotation: Box::new(identity_orientation()),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    }
}

/// Build a Transform with given rotation and translation vector.
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

// ── step-1: Transform * Vector tests ─────────────────────────────────────────

/// Identity transform * vector returns the same vector.
#[test]
fn identity_transform_mul_vector() {
    let v = Value::Vector(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        v.clone(),
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert_eq!(result, v);
}

/// 90-degree Z rotation applied to (1,0,0) yields (0,1,0).
#[test]
fn transform_90z_mul_vector() {
    let v = Value::Vector(vec![Value::length(1.0), Value::length(0.0), Value::length(0.0)]);
    let result = eval_mul_expr(
        make_transform(rotation_90z(), 0.0, 0.0, 0.0),
        Type::Transform(3),
        v,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    // After 90deg Z rotation: (1,0,0) -> (0,1,0)
    match result {
        Value::Vector(ref items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!((x - 0.0).abs() < 1e-10, "x = {x}, expected ~0");
            assert!((y - 1.0).abs() < 1e-10, "y = {y}, expected ~1");
            assert!((z - 0.0).abs() < 1e-10, "z = {z}, expected ~0");
        }
        other => panic!("expected Vector, got {:?}", other),
    }
}

/// Translation component is ignored when multiplying Transform * Vector.
#[test]
fn transform_translation_ignored_for_vector() {
    let v = Value::Vector(vec![Value::length(1.0), Value::length(0.0), Value::length(0.0)]);
    // Identity rotation, but large translation -- should NOT affect vector
    let t = make_transform(identity_orientation(), 100.0, 200.0, 300.0);
    let result = eval_mul_expr(
        t,
        Type::Transform(3),
        v.clone(),
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert_eq!(result, v);
}

/// Transform * Vector preserves the vector's dimension.
#[test]
fn transform_mul_vector_preserves_dimension() {
    let v = Value::Vector(vec![Value::length(5.0), Value::length(0.0), Value::length(0.0)]);
    let result = eval_mul_expr(
        make_transform(rotation_90z(), 0.0, 0.0, 0.0),
        Type::Transform(3),
        v,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    match result {
        Value::Vector(ref items) if items.len() == 3 => {
            // All components should have LENGTH dimension
            for item in items {
                assert_eq!(item.dimension(), DimensionVector::LENGTH);
            }
        }
        other => panic!("expected Vector with LENGTH dimension, got {:?}", other),
    }
}

// ── step-3: Transform * Point tests ──────────────────────────────────────────

/// Identity transform * point returns the same point.
#[test]
fn identity_transform_mul_point() {
    let p = Value::Point(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        p.clone(),
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert_eq!(result, p);
}

/// Rotation-only transform (zero translation) applied to point.
#[test]
fn rotation_only_transform_mul_point() {
    let p = Value::Point(vec![Value::length(1.0), Value::length(0.0), Value::length(0.0)]);
    let result = eval_mul_expr(
        make_transform(rotation_90z(), 0.0, 0.0, 0.0),
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    // 90deg Z rotation of (1,0,0) -> (0,1,0)
    match result {
        Value::Point(ref items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!((x - 0.0).abs() < 1e-10, "x = {x}, expected ~0");
            assert!((y - 1.0).abs() < 1e-10, "y = {y}, expected ~1");
            assert!((z - 0.0).abs() < 1e-10, "z = {z}, expected ~0");
        }
        other => panic!("expected Point, got {:?}", other),
    }
}

/// Full transform: 90-degree Z rotation + (10,20,30) translation applied to point (1,0,0).
/// Result = rotate(1,0,0) + (10,20,30) = (0,1,0) + (10,20,30) = (10,21,30).
#[test]
fn full_transform_mul_point() {
    let p = Value::Point(vec![Value::length(1.0), Value::length(0.0), Value::length(0.0)]);
    let result = eval_mul_expr(
        make_transform(rotation_90z(), 10.0, 20.0, 30.0),
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    match result {
        Value::Point(ref items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!((x - 10.0).abs() < 1e-10, "x = {x}, expected ~10");
            assert!((y - 21.0).abs() < 1e-10, "y = {y}, expected ~21");
            assert!((z - 30.0).abs() < 1e-10, "z = {z}, expected ~30");
        }
        other => panic!("expected Point, got {:?}", other),
    }
}

/// Transform * Point with dimension mismatch (point has LENGTH, translation has ANGLE) returns Undef.
#[test]
fn transform_mul_point_dimension_mismatch_undef() {
    let p = Value::Point(vec![Value::length(1.0), Value::length(0.0), Value::length(0.0)]);
    let t = Value::Transform {
        rotation: Box::new(identity_orientation()),
        translation: Box::new(Value::Vector(vec![
            Value::angle(0.0),
            Value::angle(0.0),
            Value::angle(0.0),
        ])),
    };
    let result = eval_mul_expr(
        t,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert!(result.is_undef(), "dimension mismatch should return Undef, got {:?}", result);
}

// ── step-5: Transform * Transform (composition) tests ────────────────────────

/// Helper to check a transform's rotation quaternion components.
fn assert_orientation_approx(val: &Value, ew: f64, ex: f64, ey: f64, ez: f64, label: &str) {
    match val {
        Value::Orientation { w, x, y, z } => {
            // Quaternion sign ambiguity: q and -q represent the same rotation.
            // Check both signs.
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

/// Helper to check a transform's translation vector components.
fn assert_vector_approx(val: &Value, ex: f64, ey: f64, ez: f64, label: &str) {
    match val {
        Value::Vector(items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!(
                (x - ex).abs() < 1e-10 && (y - ey).abs() < 1e-10 && (z - ez).abs() < 1e-10,
                "{label}: translation ({x},{y},{z}) != expected ({ex},{ey},{ez})"
            );
        }
        other => panic!("{label}: expected Vector3, got {:?}", other),
    }
}

/// Identity * Transform = Transform.
#[test]
fn identity_mul_transform() {
    let t = make_transform(rotation_90z(), 10.0, 20.0, 30.0);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        t.clone(),
        Type::Transform(3),
        Type::Transform(3),
    );
    match result {
        Value::Transform { rotation, translation } => {
            let s = std::f64::consts::FRAC_1_SQRT_2;
            assert_orientation_approx(&rotation, s, 0.0, 0.0, s, "I*T rotation");
            assert_vector_approx(&translation, 10.0, 20.0, 30.0, "I*T translation");
        }
        other => panic!("expected Transform, got {:?}", other),
    }
}

/// Transform * Identity = Transform.
#[test]
fn transform_mul_identity() {
    let t = make_transform(rotation_90z(), 10.0, 20.0, 30.0);
    let result = eval_mul_expr(
        t.clone(),
        Type::Transform(3),
        identity_transform(),
        Type::Transform(3),
        Type::Transform(3),
    );
    match result {
        Value::Transform { rotation, translation } => {
            let s = std::f64::consts::FRAC_1_SQRT_2;
            assert_orientation_approx(&rotation, s, 0.0, 0.0, s, "T*I rotation");
            assert_vector_approx(&translation, 10.0, 20.0, 30.0, "T*I translation");
        }
        other => panic!("expected Transform, got {:?}", other),
    }
}

/// Two 90-degree Z rotations compose to 180-degree Z rotation.
/// 90Z quat = (cos45, 0, 0, sin45); composed = (0, 0, 0, 1) (180Z).
#[test]
fn compose_two_rotations() {
    let t1 = make_transform(rotation_90z(), 0.0, 0.0, 0.0);
    let t2 = make_transform(rotation_90z(), 0.0, 0.0, 0.0);
    let result = eval_mul_expr(
        t1,
        Type::Transform(3),
        t2,
        Type::Transform(3),
        Type::Transform(3),
    );
    match result {
        Value::Transform { rotation, translation } => {
            // 180-degree Z rotation: quat = (0, 0, 0, 1)
            assert_orientation_approx(&rotation, 0.0, 0.0, 0.0, 1.0, "90Z*90Z rotation");
            assert_vector_approx(&translation, 0.0, 0.0, 0.0, "90Z*90Z translation");
        }
        other => panic!("expected Transform, got {:?}", other),
    }
}

/// Compose (R1,t1)*(R2,t2) = (R1*R2, R1*t2+t1).
/// R1 = 90Z, t1 = (10,0,0), R2 = identity, t2 = (1,0,0).
/// Result rotation = 90Z, result translation = 90Z*(1,0,0) + (10,0,0) = (0,1,0) + (10,0,0) = (10,1,0).
#[test]
fn compose_rotation_and_translation() {
    let t1 = make_transform(rotation_90z(), 10.0, 0.0, 0.0);
    let t2 = make_transform(identity_orientation(), 1.0, 0.0, 0.0);
    let result = eval_mul_expr(
        t1,
        Type::Transform(3),
        t2,
        Type::Transform(3),
        Type::Transform(3),
    );
    match result {
        Value::Transform { rotation, translation } => {
            let s = std::f64::consts::FRAC_1_SQRT_2;
            assert_orientation_approx(&rotation, s, 0.0, 0.0, s, "compose rot");
            assert_vector_approx(&translation, 10.0, 1.0, 0.0, "compose trans");
        }
        other => panic!("expected Transform, got {:?}", other),
    }
}

/// Translations with different dimensions returns Undef.
#[test]
fn compose_dimension_mismatch_undef() {
    let t1 = make_transform(identity_orientation(), 1.0, 0.0, 0.0); // LENGTH translation
    let t2 = Value::Transform {
        rotation: Box::new(identity_orientation()),
        translation: Box::new(Value::Vector(vec![
            Value::angle(0.0),
            Value::angle(0.0),
            Value::angle(0.0),
        ])),
    };
    let result = eval_mul_expr(
        t1,
        Type::Transform(3),
        t2,
        Type::Transform(3),
        Type::Transform(3),
    );
    assert!(result.is_undef(), "mismatched translation dimensions should return Undef, got {:?}", result);
}

// ── step-11: Transform * Transform NaN quaternion tests ──────────────────────

/// Transform with NaN in one rotation component * identity should return Undef,
/// not silently substitute identity quaternion (1,0,0,0).
#[test]
fn compose_nan_rotation_returns_undef() {
    let nan_transform = Value::Transform {
        rotation: Box::new(Value::Orientation { w: f64::NAN, x: 0.0, y: 0.0, z: 0.0 }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let result = eval_mul_expr(
        nan_transform,
        Type::Transform(3),
        identity_transform(),
        Type::Transform(3),
        Type::Transform(3),
    );
    assert!(result.is_undef(), "NaN rotation component should return Undef, got {:?}", result);
}

/// Transform with all-NaN rotation components * identity should return Undef.
#[test]
fn compose_all_nan_rotation_returns_undef() {
    let nan_transform = Value::Transform {
        rotation: Box::new(Value::Orientation { w: f64::NAN, x: f64::NAN, y: f64::NAN, z: f64::NAN }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let result = eval_mul_expr(
        nan_transform,
        Type::Transform(3),
        identity_transform(),
        Type::Transform(3),
        Type::Transform(3),
    );
    assert!(result.is_undef(), "all-NaN rotation should return Undef, got {:?}", result);
}
