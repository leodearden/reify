//! Transform operation evaluation tests: Transform * Vector, Transform * Point,
//! Transform * Transform composition.

use reify_expr::{EvalContext, eval_expr};
use reify_core::{DimensionVector, Type};
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

/// 90-degree rotation about the Z axis: cos(45deg) = sin(45deg) = sqrt(2)/2.
fn rotation_90z() -> Value {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    Value::Orientation {
        w: s,
        x: 0.0,
        y: 0.0,
        z: s,
    }
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

// ── step-1: Transform * Vector tests ─────────────────────────────────────────

/// Identity transform * vector returns the same vector.
///
/// `assert_eq!` is valid here (not `assert_approx`) because the identity
/// quaternion `(1,0,0,0)` rotation is exact under IEEE 754: `quat_rotate`
/// multiplies by 1.0 and 0.0, which produce exact results with no
/// floating-point rounding.
#[test]
fn identity_transform_mul_vector() {
    let v = Value::Vector(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::length(3.0),
    ]);
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
    let v = Value::Vector(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
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
///
/// `assert_eq!` is valid here (not `assert_approx`) because the identity
/// quaternion `(1,0,0,0)` rotation is exact under IEEE 754: `quat_rotate`
/// multiplies by 1.0 and 0.0, which produce exact results with no
/// floating-point rounding.
#[test]
fn transform_translation_ignored_for_vector() {
    let v = Value::Vector(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
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
    let v = Value::Vector(vec![
        Value::length(5.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
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

// ── Wrong-dimension vector tests ────────────────────────────────────────────

/// Transform * 2-element Vector should return Undef because vec3_components
/// rejects vectors with len != 3.
#[test]
fn transform_mul_vector_2d_returns_undef() {
    let v2 = Value::Vector(vec![Value::length(1.0), Value::length(2.0)]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        v2,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "2-element vector should return Undef, got {:?}",
        result
    );
}

/// Transform * 4-element Vector should return Undef because vec3_components
/// rejects vectors with len != 3.
#[test]
fn transform_mul_vector_4d_returns_undef() {
    let v4 = Value::Vector(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::length(3.0),
        Value::length(4.0),
    ]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        v4,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "4-element vector should return Undef, got {:?}",
        result
    );
}

// ── Dimensionless vector tests ──────────────────────────────────────────────

/// Identity transform applied to a dimensionless vector (Value::Real components)
/// should return a Vector with Value::Real components, not Value::Scalar.
/// This exercises the make_components_3 DIMENSIONLESS branch.
#[test]
fn transform_mul_dimensionless_vector_preserves_real() {
    let v = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        v.clone(),
        Type::vec3(Type::Real),
        Type::vec3(Type::Real),
    );
    // Verify the result is a Vector (not Undef)
    match &result {
        Value::Vector(items) => {
            assert_eq!(items.len(), 3, "result should have 3 components");
            // Verify each component is Value::Real (not Value::Scalar)
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::Real(_) => {} // correct variant
                    other => panic!("component {i} should be Value::Real, got {:?}", other),
                }
            }
            // Verify values match input
            assert_eq!(
                result, v,
                "identity transform should preserve dimensionless vector exactly"
            );
        }
        other => panic!("expected Vector, got {:?}", other),
    }
}

// ── step-3: Transform * Point tests ──────────────────────────────────────────

/// Identity transform * point returns the same point.
///
/// `assert_eq!` is valid here (not `assert_approx`) because the identity
/// quaternion `(1,0,0,0)` rotation is exact under IEEE 754: `quat_rotate`
/// multiplies by 1.0 and 0.0, which produce exact results with no
/// floating-point rounding.
#[test]
fn identity_transform_mul_point() {
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::length(3.0),
    ]);
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
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
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
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
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
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
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
    assert!(
        result.is_undef(),
        "dimension mismatch should return Undef, got {:?}",
        result
    );
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
        Value::Transform {
            rotation,
            translation,
        } => {
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
        Value::Transform {
            rotation,
            translation,
        } => {
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
        Value::Transform {
            rotation,
            translation,
        } => {
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
        Value::Transform {
            rotation,
            translation,
        } => {
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
    assert!(
        result.is_undef(),
        "mismatched translation dimensions should return Undef, got {:?}",
        result
    );
}

// ── Mixed-dimension vector components tests ─────────────────────────────────

/// Transform * Vector with mixed-dimension components (length, angle, length)
/// should return Undef. Currently vec3_components only checks items[0].dimension(),
/// so it silently adopts the first component's dimension.
#[test]
fn transform_mul_mixed_dimension_vector_returns_undef() {
    let mixed_vec = Value::Vector(vec![
        Value::length(1.0),
        Value::angle(2.0),
        Value::length(3.0),
    ]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        mixed_vec,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "mixed-dimension vector should return Undef, got {:?}",
        result
    );
}

/// Transform * Point with mixed-dimension point components should return Undef.
#[test]
fn transform_mul_mixed_dimension_point_returns_undef() {
    let mixed_point = Value::Point(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::angle(3.0),
    ]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        mixed_point,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "mixed-dimension point should return Undef, got {:?}",
        result
    );
}

/// Transform * Transform with mixed-dimension translation should return Undef.
#[test]
fn transform_compose_mixed_dimension_translation_returns_undef() {
    let t1 = Value::Transform {
        rotation: Box::new(identity_orientation()),
        translation: Box::new(Value::Vector(vec![
            Value::length(1.0),
            Value::angle(2.0), // dimension mismatch within same vector
            Value::length(3.0),
        ])),
    };
    let t2 = identity_transform();
    let result = eval_mul_expr(
        t1,
        Type::Transform(3),
        t2,
        Type::Transform(3),
        Type::Transform(3),
    );
    assert!(
        result.is_undef(),
        "mixed-dimension translation should return Undef, got {:?}",
        result
    );
}

// ── Unnormalized quaternion tests ────────────────────────────────────────────

/// Transform*Vector with unnormalized quaternion (w=2,x=0,y=0,z=0 — norm=2)
/// rotating vector (1,0,0). Should produce (1,0,0) since the rotation is identity,
/// but without normalization quat_rotate scales by norm²=4 giving (4,0,0).
#[test]
fn unnormalized_quat_transform_mul_vector() {
    let unnorm_transform = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: 2.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let v = Value::Vector(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
    let result = eval_mul_expr(
        unnorm_transform,
        Type::Transform(3),
        v,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    match result {
        Value::Vector(ref items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!(
                (x - 1.0).abs() < 1e-10,
                "x = {x}, expected 1.0 (not 4.0 from norm² scaling)"
            );
            assert!(y.abs() < 1e-10, "y = {y}, expected 0");
            assert!(z.abs() < 1e-10, "z = {z}, expected 0");
        }
        other => panic!("expected Vector, got {:?}", other),
    }
}

/// Transform*Point with unnormalized quaternion should normalize before rotation.
#[test]
fn unnormalized_quat_transform_mul_point() {
    let unnorm_transform = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: 2.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(10.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
    let result = eval_mul_expr(
        unnorm_transform,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    // rotate(1,0,0) + (10,0,0) = (1,0,0) + (10,0,0) = (11,0,0)
    // Without normalization: (4,0,0) + (10,0,0) = (14,0,0)
    match result {
        Value::Point(ref items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!(
                (x - 11.0).abs() < 1e-10,
                "x = {x}, expected 11.0 (not 14.0 from norm² scaling)"
            );
            assert!(y.abs() < 1e-10, "y = {y}, expected 0");
            assert!(z.abs() < 1e-10, "z = {z}, expected 0");
        }
        other => panic!("expected Point, got {:?}", other),
    }
}

// ── NaN/Infinity in vector component tests ──────────────────────────────────

/// Transform * Vector with NaN in a component should return Undef.
#[test]
fn transform_mul_vector_nan_component_returns_undef() {
    let nan_vec = Value::Vector(vec![
        Value::length(1.0),
        Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::LENGTH,
        },
        Value::length(3.0),
    ]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        nan_vec,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "NaN vector component should return Undef, got {:?}",
        result
    );
}

/// Transform * Vector with Infinity in a component should return Undef.
#[test]
fn transform_mul_vector_infinity_component_returns_undef() {
    let inf_vec = Value::Vector(vec![
        Value::Scalar {
            si_value: f64::INFINITY,
            dimension: DimensionVector::LENGTH,
        },
        Value::length(2.0),
        Value::length(3.0),
    ]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        inf_vec,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "Infinity vector component should return Undef, got {:?}",
        result
    );
}

/// Transform * Point with NaN in a component should return Undef.
#[test]
fn transform_mul_point_nan_component_returns_undef() {
    let nan_point = Value::Point(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        nan_point,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "NaN point component should return Undef, got {:?}",
        result
    );
}

// ── Unnormalized quaternion in Transform*Transform tests ─────────────────────

/// Transform*Transform with unnormalized q1 (w1=2) composing with transform
/// that has translation t2=(1,0,0). The translation rotation quat_rotate(q1, t2)
/// should use normalized q1, producing (1,0,0) not (4,0,0).
/// Compose: (R1=2*identity,t1=(10,0,0)) * (R2=identity,t2=(1,0,0))
///   rotation = R1*R2 = 2*identity (normalized to identity)
///   translation = R1*t2 + t1 = normalized_rotate(1,0,0) + (10,0,0) = (11,0,0)
/// Without q1 normalization: translation = 4*(1,0,0) + (10,0,0) = (14,0,0)
#[test]
fn unnormalized_q1_transform_compose_translation() {
    let t1 = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: 2.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(10.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let t2 = make_transform(identity_orientation(), 1.0, 0.0, 0.0);
    let result = eval_mul_expr(
        t1,
        Type::Transform(3),
        t2,
        Type::Transform(3),
        Type::Transform(3),
    );
    match result {
        Value::Transform {
            rotation,
            translation,
        } => {
            // Composed rotation should be normalized identity
            assert_orientation_approx(&rotation, 1.0, 0.0, 0.0, 0.0, "unnorm q1 rotation");
            // Translation should be (11,0,0), not (14,0,0)
            assert_vector_approx(&translation, 11.0, 0.0, 0.0, "unnorm q1 translation");
        }
        other => panic!("expected Transform, got {:?}", other),
    }
}

// ── step-11: Transform * Transform NaN quaternion tests ──────────────────────

/// Transform with NaN in one rotation component * identity should return Undef,
/// not silently substitute identity quaternion (1,0,0,0).
#[test]
fn compose_nan_rotation_returns_undef() {
    let nan_transform = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
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
    assert!(
        result.is_undef(),
        "NaN rotation component should return Undef, got {:?}",
        result
    );
}

/// Transform with all-NaN rotation components * identity should return Undef.
#[test]
fn compose_all_nan_rotation_returns_undef() {
    let nan_transform = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: f64::NAN,
            x: f64::NAN,
            y: f64::NAN,
            z: f64::NAN,
        }),
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
    assert!(
        result.is_undef(),
        "all-NaN rotation should return Undef, got {:?}",
        result
    );
}

/// Identity * Transform-with-NaN-rotation should return Undef.
/// Complements compose_nan_rotation_returns_undef which tests NaN on the LHS;
/// this ensures the RHS NaN propagates through quat_mul_t and is caught by
/// the post-multiply finiteness check.
#[test]
fn compose_rhs_nan_rotation_returns_undef() {
    let nan_transform = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let result = eval_mul_expr(
        identity_transform(),
        Type::Transform(3),
        nan_transform,
        Type::Transform(3),
        Type::Transform(3),
    );
    assert!(
        result.is_undef(),
        "RHS NaN rotation should return Undef, got {:?}",
        result
    );
}

// ── Near-zero quaternion tests ───────────────────────────────────────────────

/// Transform*Transform with near-zero quaternion (w=1e-17, rest=0 — norm=1e-17
/// < f64::EPSILON≈2.22e-16) should return Undef. The quaternion is too small
/// to normalize meaningfully.
#[test]
fn near_zero_quat_transform_compose_returns_undef() {
    let near_zero_t = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: 1e-17,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let result = eval_mul_expr(
        near_zero_t,
        Type::Transform(3),
        identity_transform(),
        Type::Transform(3),
        Type::Transform(3),
    );
    assert!(
        result.is_undef(),
        "near-zero quaternion should return Undef, got {:?}",
        result
    );
}

/// Transform*Transform with all-zero quaternion (w=0,x=0,y=0,z=0) should return
/// Undef. The zero quaternion has norm=0.0 < EPSILON, so it cannot be normalized.
/// This is the exact-zero boundary case complementing near_zero_quat (w=1e-17).
#[test]
fn compose_zero_norm_quaternion_returns_undef() {
    let zero_quat_transform = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let result = eval_mul_expr(
        zero_quat_transform,
        Type::Transform(3),
        identity_transform(),
        Type::Transform(3),
        Type::Transform(3),
    );
    assert!(
        result.is_undef(),
        "zero-norm quaternion should return Undef, got {:?}",
        result
    );
}

/// Transform*Vector with near-zero quaternion should return Undef.
#[test]
fn near_zero_quat_transform_mul_vector_returns_undef() {
    let near_zero_t = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: 1e-17,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let v = Value::Vector(vec![
        Value::length(1.0),
        Value::length(0.0),
        Value::length(0.0),
    ]);
    let result = eval_mul_expr(
        near_zero_t,
        Type::Transform(3),
        v,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "near-zero quaternion * vector should return Undef, got {:?}",
        result
    );
}

// ── step-13: Transform * Vector / Point NaN quaternion tests ─────────────────

/// Transform with NaN rotation * vector should return Undef.
#[test]
fn nan_rotation_mul_vector_returns_undef() {
    let nan_transform = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let v = Value::Vector(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::length(3.0),
    ]);
    let result = eval_mul_expr(
        nan_transform,
        Type::Transform(3),
        v,
        Type::vec3(Type::length()),
        Type::vec3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "NaN rotation * vector should return Undef, got {:?}",
        result
    );
}

/// Transform with NaN rotation * point should return Undef.
#[test]
fn nan_rotation_mul_point_returns_undef() {
    let nan_transform = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    };
    let p = Value::Point(vec![
        Value::length(1.0),
        Value::length(2.0),
        Value::length(3.0),
    ]);
    let result = eval_mul_expr(
        nan_transform,
        Type::Transform(3),
        p,
        Type::point3(Type::length()),
        Type::point3(Type::length()),
    );
    assert!(
        result.is_undef(),
        "NaN rotation * point should return Undef, got {:?}",
        result
    );
}
