//! Integration tests for orientation stdlib functions.
//!
//! Tests the public API `reify_stdlib::eval_builtin()` for:
//! - orient_look_at: Gram-Schmidt constructor
//! - orient_euler / orient_to_euler: EulerConvention enum-value path
//!
//! These lock the G2 boundary signals over the completed impl; they complement
//! the in-module RED→GREEN tests in src/orientation.rs by exercising the same
//! functions through the public eval_builtin surface.

use reify_core::DimensionVector;
use reify_ir::Value;
use reify_stdlib::eval_builtin;

const TOLERANCE: f64 = 1e-10;

// ── Local assert helpers ──────────────────────────────────────────────────────

/// Build a 3-component Tensor value.
fn t3(x: f64, y: f64, z: f64) -> Value {
    Value::Tensor(vec![Value::Real(x), Value::Real(y), Value::Real(z)])
}

/// Assert that a Value is an Orientation with components near expected, accepting sign flip.
fn assert_orientation_approx(actual: &Value, ew: f64, ex: f64, ey: f64, ez: f64) {
    match actual {
        Value::Orientation { w, x, y, z } => {
            // Quaternions q and -q represent the same rotation; accept either sign.
            let direct = (w - ew).abs() < TOLERANCE
                && (x - ex).abs() < TOLERANCE
                && (y - ey).abs() < TOLERANCE
                && (z - ez).abs() < TOLERANCE;
            let flipped = (w + ew).abs() < TOLERANCE
                && (x + ex).abs() < TOLERANCE
                && (y + ey).abs() < TOLERANCE
                && (z + ez).abs() < TOLERANCE;
            assert!(
                direct || flipped,
                "expected Orientation {{w≈{ew}, x≈{ex}, y≈{ey}, z≈{ez}}} (or its negative), got \
                 Orientation {{w={w}, x={x}, y={y}, z={z}}}"
            );
        }
        other => panic!(
            "expected Orientation {{w≈{ew}, x≈{ex}, y≈{ey}, z≈{ez}}}, got {other:?}"
        ),
    }
}

/// Extract three Angle Scalars from a `List<Angle>` result. Returns None if wrong shape.
fn euler_extract(v: &Value) -> Option<[f64; 3]> {
    let items = match v {
        Value::List(items) if items.len() == 3 => items,
        _ => return None,
    };
    let mut out = [0.0; 3];
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::Scalar { si_value, dimension } if *dimension == DimensionVector::ANGLE => {
                out[i] = *si_value;
            }
            _ => return None,
        }
    }
    Some(out)
}

// ── orient_look_at G2 signals ─────────────────────────────────────────────────

#[test]
fn integration_orient_look_at_forward_x_up_z() {
    // forward=(1,0,0), up=(0,0,1) → Shepperd r22-branch → (0.5, 0.5, 0.5, 0.5)
    let result = eval_builtin("orient_look_at", &[t3(1.0, 0.0, 0.0), t3(0.0, 0.0, 1.0)]);
    assert_orientation_approx(&result, 0.5, 0.5, 0.5, 0.5);
}

#[test]
fn integration_orient_look_at_forward_z_up_y_is_identity() {
    // forward=(0,0,1), up=(0,1,0) → identity basis → (1, 0, 0, 0)
    let result = eval_builtin("orient_look_at", &[t3(0.0, 0.0, 1.0), t3(0.0, 1.0, 0.0)]);
    assert_orientation_approx(&result, 1.0, 0.0, 0.0, 0.0);
}

#[test]
fn integration_orient_look_at_parallel_forward_up_returns_undef() {
    // forward ∥ up → cross product = 0 → Undef
    let result = eval_builtin("orient_look_at", &[t3(0.0, 0.0, 1.0), t3(0.0, 0.0, 1.0)]);
    assert!(result.is_undef(), "parallel forward and up should return Undef, got {result:?}");
}

// ── orient_euler EulerConvention enum G2 signals ──────────────────────────────

#[test]
fn integration_orient_euler_enum_xyz_matches_string_xyz() {
    let angles = [Value::Real(0.2_f64), Value::Real(0.3_f64), Value::Real(-0.1_f64)];
    let by_enum = eval_builtin(
        "orient_euler",
        &[
            Value::Enum {
                type_name: "EulerConvention".to_string(),
                variant: "XYZ".to_string(),
                payload: vec![],
            },
            angles[0].clone(),
            angles[1].clone(),
            angles[2].clone(),
        ],
    );
    let by_str = eval_builtin(
        "orient_euler",
        &[
            Value::String("xyz".to_string()),
            angles[0].clone(),
            angles[1].clone(),
            angles[2].clone(),
        ],
    );
    assert!(!by_enum.is_undef(), "EulerConvention.XYZ orient_euler should not return Undef");
    assert_eq!(
        by_enum, by_str,
        "EulerConvention.XYZ orient_euler should equal string 'xyz'"
    );
}

// ── orient_to_euler EulerConvention enum G2 signals ───────────────────────────

#[test]
fn integration_orient_to_euler_enum_zyx_matches_string_zyx() {
    // Build a known quaternion, then decode with enum and string paths.
    let q = eval_builtin(
        "orient_euler",
        &[
            Value::String("zyx".to_string()),
            Value::Real(0.3_f64),
            Value::Real(0.5_f64),
            Value::Real(-0.7_f64),
        ],
    );
    let by_enum = eval_builtin(
        "orient_to_euler",
        &[
            Value::Enum {
                type_name: "EulerConvention".to_string(),
                variant: "ZYX".to_string(),
                payload: vec![],
            },
            q.clone(),
        ],
    );
    let by_str = eval_builtin(
        "orient_to_euler",
        &[Value::String("zyx".to_string()), q.clone()],
    );
    assert!(
        euler_extract(&by_enum).is_some(),
        "EulerConvention.ZYX orient_to_euler should return a 3-element Angle list, got {by_enum:?}"
    );
    assert_eq!(
        by_enum, by_str,
        "EulerConvention.ZYX orient_to_euler should equal string 'zyx'"
    );
}

// ── orient_to_euler subject-first signals (task #6082, F3) ────────────────────
//
// `orient_to_euler` is a DECOMPOSER and must take its subject at argument 0, to
// match every sibling — `orient_log(q)`, `orient_to_axis_angle(q)`,
// `orient_inverse(q)`, `transform_log(t)`. It was the lone exception, taking
// `(convention, q)`.
//
// (`orient_euler` is deliberately NOT flipped: it is a CONSTRUCTOR whose
// convention selects the meaning of the three angle arguments that follow,
// matching standard R_xyz(a, b, c) notation. The asymmetry is intentional.)

/// Round-trip tolerance. A quaternion compose→decompose round trip is exact to
/// ~1e-15; 1e-12 leaves headroom without hiding a real convention mismatch.
const ROUNDTRIP_TOLERANCE: f64 = 1e-12;

/// Build a qualified `EulerConvention` enum value, as the compiler lowers
/// `EulerConvention.<VARIANT>` (`Value::enum_unit` with an uppercase variant).
fn convention(variant: &str) -> Value {
    Value::Enum {
        type_name: "EulerConvention".to_string(),
        variant: variant.to_string(),
        payload: vec![],
    }
}

/// Compose three angles under `variant`, then decompose subject-first, and
/// assert the angles come back. Exercises the enum path on BOTH builtins.
fn assert_subject_first_roundtrip(variant: &str, a: f64, b: f64, c: f64) {
    let q = eval_builtin(
        "orient_euler",
        &[convention(variant), Value::Real(a), Value::Real(b), Value::Real(c)],
    );
    assert!(
        matches!(q, Value::Orientation { .. }),
        "orient_euler(EulerConvention.{variant}, …) should build an Orientation, got {q:?}"
    );

    // Subject FIRST: (q, convention).
    let back = eval_builtin("orient_to_euler", &[q.clone(), convention(variant)]);
    let angles = euler_extract(&back).unwrap_or_else(|| {
        panic!(
            "orient_to_euler(q, EulerConvention.{variant}) should return a 3-element \
             Angle list, got {back:?}"
        )
    });

    for (i, (got, want)) in angles.iter().zip([a, b, c].iter()).enumerate() {
        assert!(
            (got - want).abs() < ROUNDTRIP_TOLERANCE,
            "EulerConvention.{variant} round-trip angle[{i}]: expected {want}, got {got}"
        );
    }
}

/// A Tait-Bryan convention (three distinct axes). Middle angle is kept well
/// away from ±90°, where this family is singular.
#[test]
fn integration_orient_to_euler_subject_first_roundtrips_xyz() {
    assert_subject_first_roundtrip("XYZ", 0.2, 0.3, -0.1);
}

/// The reverse Tait-Bryan sequence.
#[test]
fn integration_orient_to_euler_subject_first_roundtrips_zyx() {
    assert_subject_first_roundtrip("ZYX", 0.3, 0.5, -0.7);
}

/// A proper/classic Euler convention (first axis repeated as third), reachable
/// as a qualified enum value only since the declaration was widened to twelve.
/// Middle angle is kept away from 0 and π, where THIS family is singular — a
/// different locus than Tait-Bryan's.
#[test]
fn integration_orient_to_euler_subject_first_roundtrips_zxz() {
    assert_subject_first_roundtrip("ZXZ", 0.3, 0.7, -0.2);
}

/// The returned list must carry the ANGLE dimension on all three elements, not
/// bare Reals — this is what lets the result feed a `List<Angle>` slot.
#[test]
fn integration_orient_to_euler_subject_first_returns_angle_dimensioned_list() {
    let q = eval_builtin(
        "orient_euler",
        &[convention("ZXZ"), Value::Real(0.3), Value::Real(0.7), Value::Real(-0.2)],
    );
    let back = eval_builtin("orient_to_euler", &[q, convention("ZXZ")]);

    let items = match &back {
        Value::List(items) => items,
        other => panic!("orient_to_euler should return a Value::List, got {other:?}"),
    };
    assert_eq!(items.len(), 3, "expected exactly 3 angles, got {}", items.len());
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::Scalar { dimension, .. } => assert_eq!(
                *dimension,
                DimensionVector::ANGLE,
                "angle[{i}] should carry the ANGLE dimension"
            ),
            other => panic!("angle[{i}] should be a Scalar, got {other:?}"),
        }
    }
}
