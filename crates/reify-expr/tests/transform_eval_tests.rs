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
