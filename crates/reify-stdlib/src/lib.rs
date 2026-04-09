use reify_types::Value;

mod common;

mod complex;
mod frames;
mod geometry;
mod linalg;
mod numeric;
mod orientation;
mod stubs;
mod trig;
mod vector;

#[cfg(test)]
mod test_helpers;

// Re-export helpers needed by tests in this module until test migration steps
// (steps 19-27) move each test block to its submodule.
#[cfg(test)]
use crate::common::*;
#[cfg(test)]
pub(crate) use linalg::matrix_components_f64;

/// Evaluate a built-in stdlib function by name.
///
/// Returns `Value::Undef` for unknown functions or wrong argument types/counts.
pub fn eval_builtin(name: &str, args: &[Value]) -> Value {
    if let Some(v) = numeric::dispatch(name, args) {
        return v;
    }
    if let Some(v) = trig::dispatch(name, args) {
        return v;
    }
    if let Some(v) = vector::dispatch(name, args) {
        return v;
    }
    if let Some(v) = complex::dispatch(name, args) {
        return v;
    }
    if let Some(v) = orientation::dispatch(name, args) {
        return v;
    }
    if let Some(v) = frames::dispatch(name, args) {
        return v;
    }
    if let Some(v) = geometry::dispatch(name, args) {
        return v;
    }
    if let Some(v) = linalg::dispatch(name, args) {
        return v;
    }
    if let Some(v) = stubs::dispatch(name, args) {
        return v;
    }
    Value::Undef
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::DimensionVector;
    use crate::test_helpers::*;
    use crate::vector::construct_point_or_vector;

    // --- Determinacy predicate stubs (step-7) ---

    #[test]
    fn determined_stub_returns_undef() {
        // determined() is handled at the eval layer where DeterminacyState is available.
        // The stdlib stub returns Undef as a fallback.
        let result = eval_builtin("determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "determined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn undetermined_stub_returns_undef() {
        let result = eval_builtin("undetermined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "undetermined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn constrained_stub_returns_undef() {
        let result = eval_builtin("constrained", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "constrained stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn partially_determined_stub_returns_undef() {
        let result = eval_builtin("partially_determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "partially_determined stub should return Undef, got {:?}",
            result
        );
    }

    // --- Field operation stubs (step-25) ---

    #[test]
    fn gradient_scalar_field_returns_undef() {
        // gradient(field) on a scalar field should return Undef (stub).
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::length(),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("gradient", &[field]);
        assert!(
            result.is_undef(),
            "gradient stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn divergence_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::StructureRef("Vector3".into()),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("divergence", &[field]);
        assert!(
            result.is_undef(),
            "divergence stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn curl_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::StructureRef("Vector3".into()),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("curl", &[field]);
        assert!(
            result.is_undef(),
            "curl stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn sample_in_stdlib_returns_undef() {
        // sample() in stdlib returns Undef because lambda application
        // needs an EvalContext (handled in reify-expr instead).
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::length(),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("sample", &[field, Value::Int(42)]);
        assert!(
            result.is_undef(),
            "sample in stdlib should return Undef (handled in eval_expr), got {:?}",
            result
        );
    }

    // ── assert_orientation_approx diagnostic tests ──────────────────────────

    #[test]
    fn orient_identity_per_component_diagnostic() {
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                0.5, // wrong w
                0.0,
                0.0,
                0.0
            );
        });
        let err = result.expect_err("expected assert_orientation_approx to panic");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("w:"),
            "expected panic message to contain 'w:', got: {msg:?}"
        );
    }

    #[test]
    fn orient_per_component_diagnostic_x() {
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                1.0,
                0.5, // wrong x
                0.0,
                0.0
            );
        });
        let err = result.expect_err("expected assert_orientation_approx to panic");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("x:"),
            "expected panic message to contain 'x:', got: {msg:?}"
        );
    }

    #[test]
    fn orient_per_component_diagnostic_y() {
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                1.0,
                0.0,
                0.5, // wrong y
                0.0
            );
        });
        let err = result.expect_err("expected assert_orientation_approx to panic");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("y:"),
            "expected panic message to contain 'y:', got: {msg:?}"
        );
    }

    #[test]
    fn orient_per_component_diagnostic_z() {
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                1.0,
                0.0,
                0.0,
                0.5 // wrong z
            );
        });
        let err = result.expect_err("expected assert_orientation_approx to panic");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("z:"),
            "expected panic message to contain 'z:', got: {msg:?}"
        );
    }

    // ── assert_orientation_approx sign_insensitive = tests ──────────────────

    #[test]
    fn sign_insensitive_macro_positive() {
        // Positive-sign identity: should pass with positive-sign expected values.
        assert_orientation_approx!(
            Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            1.0,
            0.0,
            0.0,
            0.0,
            sign_insensitive = 1e-10
        );
    }

    #[test]
    fn sign_insensitive_macro_negative() {
        // Negated identity quaternion: w=-1,x=0,y=0,z=0 represents the same rotation as identity.
        // The sign-insensitive macro should accept it when expected values are (1,0,0,0).
        assert_orientation_approx!(
            Value::Orientation {
                w: -1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            1.0,
            0.0,
            0.0,
            0.0,
            sign_insensitive = 1e-10
        );
    }

    #[test]
    fn sign_insensitive_macro_non_trivial_quaternion() {
        // 90° rotation quaternion: (s, s, 0, 0) where s = FRAC_1_SQRT_2.
        // Tests that the sign-flip handles non-zero x component, not just the trivial
        // w-only identity case.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        // Positive form: actual (s, s, 0, 0) should match expected (s, s, 0, 0).
        assert_orientation_approx!(
            Value::Orientation {
                w: s,
                x: s,
                y: 0.0,
                z: 0.0
            },
            s,
            s,
            0.0,
            0.0,
            sign_insensitive = 1e-10
        );
        // Negated form: actual (-s, -s, 0, 0) should also match expected (s, s, 0, 0).
        assert_orientation_approx!(
            Value::Orientation {
                w: -s,
                x: -s,
                y: 0.0,
                z: 0.0
            },
            s,
            s,
            0.0,
            0.0,
            sign_insensitive = 1e-10
        );
    }

    #[test]
    fn sign_insensitive_macro_rejects_wrong_value() {
        // w=0.5,x=0.5,y=0.5,z=0.5 does not match ±(1,0,0,0) — macro should panic.
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 0.5,
                    x: 0.5,
                    y: 0.5,
                    z: 0.5
                },
                1.0,
                0.0,
                0.0,
                0.0,
                sign_insensitive = 1e-10
            );
        });
        let err = result.expect_err("expected assert_orientation_approx sign_insensitive to panic for wrong value");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("expected Orientation(\u{b1}"),
            "expected panic message to contain 'expected Orientation(\u{b1}', got: {msg:?}"
        );
        assert!(
            msg.contains("got"),
            "expected panic message to contain 'got', got: {msg:?}"
        );
    }

    // ── orient_identity tests (step-6) ──────────────────────────────────────

    #[test]
    fn orient_identity_no_args() {
        assert_orientation_approx!(eval_builtin("orient_identity", &[]), 1.0, 0.0, 0.0, 0.0);
    }

    #[test]
    fn orient_identity_with_args_returns_undef() {
        assert!(eval_builtin("orient_identity", &[Value::Real(1.0)]).is_undef());
    }

    // ── orient_quaternion tests (step-8) ────────────────────────────────────

    #[test]
    fn orient_quaternion_normalizes_unnormalized() {
        // (2,0,0,0) should normalize to (1,0,0,0)
        assert_orientation_approx!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(2.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            ),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_quaternion_preserves_normalized() {
        assert_orientation_approx!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(1.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            ),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_quaternion_arbitrary_normalizes() {
        // (1,1,1,1) norm = 2, normalized = (0.5, 0.5, 0.5, 0.5)
        assert_orientation_approx!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(1.0),
                    Value::Real(1.0),
                    Value::Real(1.0),
                    Value::Real(1.0)
                ]
            ),
            0.5,
            0.5,
            0.5,
            0.5
        );
    }

    #[test]
    fn orient_quaternion_zero_returns_undef() {
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_quaternion_nan_returns_undef() {
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(f64::NAN),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_quaternion_inf_returns_undef() {
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(f64::INFINITY),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_quaternion_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("orient_quaternion", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("orient_quaternion", &[]).is_undef());
    }

    // ── orient_axis_angle tests (step-10) ─────────────────────────────────

    #[test]
    fn orient_axis_angle_90deg_around_z() {
        // 90° around Z: q = (cos(π/4), 0, 0, sin(π/4))
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Real(std::f64::consts::FRAC_PI_2);
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            cos_pi_4,
            0.0,
            0.0,
            sin_pi_4
        );
    }

    #[test]
    fn orient_axis_angle_180deg_around_x() {
        // 180° around X: q = (cos(π/2), sin(π/2), 0, 0) = (0, 1, 0, 0)
        let axis = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let angle = Value::Real(std::f64::consts::PI);
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            0.0,
            1.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_axis_angle_accepts_angle_scalar() {
        // Same as 90° around Z but angle is an Angle Scalar
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Scalar {
            si_value: std::f64::consts::FRAC_PI_2,
            dimension: DimensionVector::ANGLE,
        };
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            cos_pi_4,
            0.0,
            0.0,
            sin_pi_4
        );
    }

    #[test]
    fn orient_axis_angle_zero_axis_returns_undef() {
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let angle = Value::Real(1.0);
        assert!(eval_builtin("orient_axis_angle", &[axis, angle]).is_undef());
    }

    #[test]
    fn orient_axis_angle_non_3d_axis_returns_undef() {
        let axis = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]);
        let angle = Value::Real(1.0);
        assert!(eval_builtin("orient_axis_angle", &[axis, angle]).is_undef());
    }

    #[test]
    fn orient_axis_angle_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("orient_axis_angle", &[]).is_undef());
        assert!(eval_builtin("orient_axis_angle", &[Value::Real(1.0)]).is_undef());
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin(
                "orient_axis_angle",
                &[axis.clone(), Value::Real(1.0), Value::Real(2.0)]
            )
            .is_undef()
        );
    }

    // ── orient_euler tests (step-12) ──────────────────────────────────────

    #[test]
    fn orient_euler_xyz_single_axis() {
        // Intrinsic xyz with (π/2, 0, 0): rotation of π/2 about X
        // = quaternion (cos(π/4), sin(π/4), 0, 0)
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            ),
            cos_pi_4,
            sin_pi_4,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_euler_zyx_single_axis() {
        // Intrinsic zyx with (π/2, 0, 0): rotation of π/2 about Z
        // = quaternion (cos(π/4), 0, 0, sin(π/4))
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("zyx".into()),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            ),
            cos_pi_4,
            0.0,
            0.0,
            sin_pi_4
        );
    }

    #[test]
    fn orient_euler_zero_angles_is_identity() {
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            ),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_euler_invalid_convention_returns_undef() {
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("abc".into()),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_euler_non_string_convention_returns_undef() {
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_euler_angle_scalar_accepted() {
        // Same as xyz (π/2, 0, 0) but with Angle Scalar
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Scalar {
                        si_value: std::f64::consts::FRAC_PI_2,
                        dimension: DimensionVector::ANGLE,
                    },
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            ),
            cos_pi_4,
            sin_pi_4,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_euler_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("orient_euler", &[]).is_undef());
        assert!(
            eval_builtin(
                "orient_euler",
                &[Value::String("xyz".into()), Value::Real(0.0),]
            )
            .is_undef()
        );
    }

    // ── orient_euler compound rotation tests (step-16) ───────────────────

    #[test]
    fn orient_euler_xyz_two_nonzero_angles() {
        // orient_euler('xyz', π/2, π/2, 0): q_x(π/2) * q_y(π/2) * q_z(0)
        // Two non-zero angles exercise quat_mul with non-identity operands.
        // Expected: (0.5, 0.5, 0.5, 0.5)
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(0.0),
                ]
            ),
            0.5,
            0.5,
            0.5,
            0.5
        );
    }

    #[test]
    fn orient_euler_zyx_three_nonzero_angles() {
        // orient_euler('zyx', π/3, π/4, π/6): q_z(π/3) * q_y(π/4) * q_x(π/6)
        // Three non-zero angles exercise full three-way quat_mul composition.
        // Analytically computed via Hamilton product of elementary rotations.
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("zyx".into()),
                    Value::Real(std::f64::consts::FRAC_PI_3),
                    Value::Real(std::f64::consts::FRAC_PI_4),
                    Value::Real(std::f64::consts::FRAC_PI_6),
                ]
            ),
            0.822_363_171_905_999_4,
            0.02226002671473384,
            0.43967973954090955,
            0.360_423_405_650_355_9
        );
    }

    #[test]
    fn orient_euler_xzx_proper_euler_compound() {
        // orient_euler('xzx', π/2, π/2, 0): q_x(π/2) * q_z(π/2) * q_x(0)
        // Proper Euler convention with compound rotation.
        // Expected: (0.5, 0.5, -0.5, 0.5)
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xzx".into()),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(0.0),
                ]
            ),
            0.5,
            0.5,
            -0.5,
            0.5
        );
    }

    // ── orient_basis tests (step-14) ──────────────────────────────────────

    #[test]
    fn orient_basis_identity_basis() {
        // Standard basis = identity rotation
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert_orientation_approx!(eval_builtin("orient_basis", &[x, y, z]), 1.0, 0.0, 0.0, 0.0);
    }

    #[test]
    fn orient_basis_90deg_rotated() {
        // 90° rotation around Z: X→Y, Y→-X, Z→Z
        // = quaternion (cos(π/4), 0, 0, sin(π/4))
        let x = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(-1.0), Value::Real(0.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_basis", &[x, y, z]),
            cos_pi_4,
            0.0,
            0.0,
            sin_pi_4
        );
    }

    #[test]
    fn orient_basis_non_orthogonal_returns_undef() {
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(1.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(eval_builtin("orient_basis", &[x, y, z]).is_undef());
    }

    #[test]
    fn orient_basis_non_3d_returns_undef() {
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0)]);
        assert!(eval_builtin("orient_basis", &[x, y, z]).is_undef());
    }

    #[test]
    fn orient_basis_zero_length_returns_undef() {
        let x = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(eval_builtin("orient_basis", &[x, y, z]).is_undef());
    }

    #[test]
    fn orient_basis_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("orient_basis", &[]).is_undef());
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(eval_builtin("orient_basis", &[x]).is_undef());
    }

    // ── orient_basis left-handed rejection tests (step-17) ───────────────

    #[test]
    fn orient_basis_left_handed_reflection_xy_plane_returns_undef() {
        // x=(1,0,0), y=(0,1,0), z=(0,0,-1): reflection through XY plane, det=-1
        // Orthonormal but left-handed — must return Undef (not in SO(3)).
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(-1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "left-handed basis (z-reflection) should be rejected"
        );
    }

    #[test]
    fn orient_basis_left_handed_swapped_yz_returns_undef() {
        // x=(1,0,0), y=(0,0,1), z=(0,1,0): another left-handed basis, det=-1
        // Y and Z swapped relative to right-handed standard.
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "left-handed basis (y-z swap) should be rejected"
        );
    }

    #[test]
    fn orient_basis_right_handed_near_tolerance_passes() {
        // A valid right-handed basis that's slightly off from exact (within tolerance).
        // Should still produce a valid orientation.
        let eps = 1e-8; // well within the 1e-6 tolerance
        let x = Value::Tensor(vec![
            Value::Real(1.0 - eps),
            Value::Real(eps),
            Value::Real(0.0),
        ]);
        let y = Value::Tensor(vec![
            Value::Real(-eps),
            Value::Real(1.0 - eps),
            Value::Real(0.0),
        ]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let result = eval_builtin("orient_basis", &[x, y, z]);
        assert!(
            !result.is_undef(),
            "right-handed basis near tolerance should produce valid orientation, got {:?}",
            result
        );
    }

    // ── orient NaN/Inf/edge-case tests (task-359) ─────────────────────────

    #[test]
    fn orient_euler_uppercase_convention_returns_undef() {
        // Convention matching is case-sensitive: 'XYZ' is not recognized, only 'xyz'.
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("XYZ".into()),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef(),
            "uppercase convention 'XYZ' should be rejected"
        );
    }

    #[test]
    fn orient_basis_nan_component_returns_undef() {
        // NaN in a basis vector must be rejected — NaN bypasses IEEE 754 comparisons.
        let x = Value::Tensor(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "NaN component should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_nan_angle_returns_undef() {
        // NaN angle must be rejected — trig_input should guard against non-finite values.
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Real(f64::NAN);
        assert!(
            eval_builtin("orient_axis_angle", &[axis, angle]).is_undef(),
            "NaN angle should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_inf_angle_returns_undef() {
        // Inf angle must be rejected — cos/sin of Inf produce NaN.
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Real(f64::INFINITY);
        assert!(
            eval_builtin("orient_axis_angle", &[axis, angle]).is_undef(),
            "Inf angle should be rejected"
        );
    }

    #[test]
    fn orient_euler_nan_angle_returns_undef() {
        // NaN angle must be rejected in orient_euler.
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(f64::NAN),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef(),
            "NaN euler angle should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_non_unit_axis_normalizes() {
        // orient_axis_angle normalizes the axis vector — [2,0,0] with π/2 should
        // produce the same rotation as [1,0,0] with π/2: q = (cos(π/4), sin(π/4), 0, 0)
        let axis_scaled = Value::Tensor(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let axis_unit = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let angle = Value::Real(std::f64::consts::FRAC_PI_2);
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis_scaled, angle.clone()]),
            cos_pi_4,
            sin_pi_4,
            0.0,
            0.0
        );
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis_unit, angle]),
            cos_pi_4,
            sin_pi_4,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_quaternion_dimensioned_scalar_returns_undef() {
        // Dimensioned Scalars (e.g. LENGTH) must be rejected — quaternion components
        // are pure numbers and should not carry physical dimensions.
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::LENGTH,
                    },
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_quaternion_accepts_dimensionless_scalar() {
        // Dimensionless Scalars should be accepted — they are pure numbers.
        assert_orientation_approx!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::DIMENSIONLESS,
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::DIMENSIONLESS,
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::DIMENSIONLESS,
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::DIMENSIONLESS,
                    },
                ]
            ),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_quaternion_rejects_angle_dimension() {
        // ANGLE-dimensioned Scalars must also be rejected — quaternion components
        // are dimensionless, not angles.
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::ANGLE,
                    },
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_euler_inf_angle_returns_undef() {
        // Inf angle must be rejected in orient_euler.
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(f64::INFINITY),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef(),
            "Inf euler angle should be rejected"
        );
    }

    #[test]
    fn orient_basis_inf_component_returns_undef() {
        // Inf in a basis vector must be rejected — magnitude would be Inf, not ≈1.
        let x = Value::Tensor(vec![
            Value::Real(f64::INFINITY),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "Inf component should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_nan_axis_returns_undef() {
        // NaN in axis must be rejected — vec3_norm(NaN, 0, 0) = sqrt(NaN) = NaN, not finite.
        let axis = Value::Tensor(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let angle = Value::Real(std::f64::consts::FRAC_PI_2);
        assert!(
            eval_builtin("orient_axis_angle", &[axis, angle]).is_undef(),
            "NaN axis component should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_inf_axis_returns_undef() {
        // Inf in axis must be rejected — vec3_norm(Inf, 0, 0) = sqrt(Inf) = Inf, not finite.
        let axis = Value::Tensor(vec![
            Value::Real(f64::INFINITY),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let angle = Value::Real(std::f64::consts::FRAC_PI_2);
        assert!(
            eval_builtin("orient_axis_angle", &[axis, angle]).is_undef(),
            "Inf axis component should be rejected"
        );
    }

    #[test]
    fn orient_basis_non_unit_vector_returns_undef() {
        // Orthogonal but non-unit x=[2,0,0] must be rejected — isolates the magnitude
        // check (|x|=2.0, |2.0-1.0|=1.0 > 1e-6) from the orthogonality check.
        let x = Value::Tensor(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "Non-unit basis vector should be rejected"
        );
    }

    #[test]
    fn orient_basis_non_unit_y_returns_undef() {
        // Orthogonal but non-unit y=[0,2,0] must be rejected — isolates the mag_y
        // branch of the unit-length guard (|y|=2.0, |2.0-1.0|=1.0 > 1e-6).
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "Non-unit y basis vector should be rejected"
        );
    }

    #[test]
    fn orient_basis_non_unit_z_returns_undef() {
        // Orthogonal but non-unit z=[0,0,2] must be rejected — isolates the mag_z
        // branch of the unit-length guard (|z|=2.0, |2.0-1.0|=1.0 > 1e-6).
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "Non-unit z basis vector should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_integer_angle_accepted() {
        // Value::Int(1) = 1 radian, exercises the Value::Int(i) => Some(*i as f64) arm
        // in trig_input. Expected: half=0.5, q=(cos(0.5), 0, 0, sin(0.5)).
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Int(1);
        let half = 0.5_f64;
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            half.cos(),
            0.0,
            0.0,
            half.sin()
        );
    }

    #[test]
    fn orient_axis_angle_integer_angle_zero_is_identity() {
        // Value::Int(0) = 0 radians, exercises the zero-angle boundary of
        // half-angle trig: cos(0)=1, sin(0)=0 → identity quaternion (1,0,0,0).
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Int(0);
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn dot_mixed_component_dimensions_returns_undef() {
        // A Tensor with mixed dimensions is not a valid physical vector
        let a = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::MASS,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert!(
            eval_builtin("dot", &[a, b]).is_undef(),
            "dot of vector with mixed component dimensions should be Undef"
        );
    }

    // ── complex() constructor tests (step-1) ──────────────────────────────────

    #[test]
    fn complex_real_real_returns_dimensionless() {
        // complex(Real, Real) → Complex with DIMENSIONLESS dimension
        let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(4.0)]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12, "expected re=3.0, got {}", re);
                assert!((im - 4.0).abs() < 1e-12, "expected im=4.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{3,4,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_int_int_returns_dimensionless() {
        // complex(Int, Int) → Complex with DIMENSIONLESS dimension
        let result = eval_builtin("complex", &[Value::Int(5), Value::Int(-2)]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 5.0).abs() < 1e-12, "expected re=5.0, got {}", re);
                assert!((im - (-2.0)).abs() < 1e-12, "expected im=-2.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{5,-2,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_int_real_mixed_coercion_dimensionless() {
        // complex(Int, Real) → Complex with DIMENSIONLESS dimension (both dimensionless)
        let result = eval_builtin("complex", &[Value::Int(1), Value::Real(2.5)]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 1.0).abs() < 1e-12, "expected re=1.0, got {}", re);
                assert!((im - 2.5).abs() < 1e-12, "expected im=2.5, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{1,2.5,DIMLESS}}, got {:?}", other),
        }
    }

    // ── complex() with Scalar args (step-3) ───────────────────────────────────

    #[test]
    fn complex_scalar_mm_preserves_length_dimension() {
        // complex(Scalar{5mm}, Scalar{3mm}) → Complex{0.005, 0.003, LENGTH}
        let result = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 0.005,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.003,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 0.005).abs() < 1e-15, "expected re=0.005, got {}", re);
                assert!((im - 0.003).abs() < 1e-15, "expected im=0.003, got {}", im);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{0.005,0.003,LENGTH}}, got {:?}", other),
        }
    }

    // ── complex() error cases (step-5) ────────────────────────────────────────

    #[test]
    fn complex_dimension_mismatch_returns_undef() {
        // complex(3mm, 4s) → Undef (LENGTH ≠ TIME)
        let result = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 0.003,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 4.0,
                    dimension: DimensionVector::TIME,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "expected Undef for dimension mismatch, got {:?}",
            result
        );
    }

    #[test]
    fn complex_real_with_scalar_dimension_mismatch_returns_undef() {
        // complex(Real(3.0), Scalar{4, LENGTH}) → Undef
        // Real is DIMENSIONLESS, Scalar{LENGTH} is not — mismatch
        let result = eval_builtin(
            "complex",
            &[
                Value::Real(3.0),
                Value::Scalar {
                    si_value: 4.0,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "expected Undef for Real+Scalar mismatch, got {:?}",
            result
        );
    }

    #[test]
    fn complex_zero_args_returns_undef() {
        let result = eval_builtin("complex", &[]);
        assert!(
            result.is_undef(),
            "expected Undef for 0 args, got {:?}",
            result
        );
    }

    #[test]
    fn complex_three_args_returns_undef() {
        let result = eval_builtin(
            "complex",
            &[Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        );
        assert!(
            result.is_undef(),
            "expected Undef for 3 args, got {:?}",
            result
        );
    }

    #[test]
    fn complex_non_numeric_re_returns_undef() {
        let result = eval_builtin("complex", &[Value::Bool(true), Value::Real(3.0)]);
        assert!(
            result.is_undef(),
            "expected Undef for non-numeric re, got {:?}",
            result
        );
    }

    #[test]
    fn complex_nan_arg_returns_undef() {
        let result = eval_builtin("complex", &[Value::Real(f64::NAN), Value::Real(3.0)]);
        assert!(
            result.is_undef(),
            "expected Undef for NaN re, got {:?}",
            result
        );
    }

    #[test]
    fn complex_inf_arg_returns_undef() {
        let result = eval_builtin("complex", &[Value::Real(f64::INFINITY), Value::Real(3.0)]);
        assert!(
            result.is_undef(),
            "expected Undef for Inf re, got {:?}",
            result
        );
    }

    #[test]
    fn complex_nan_im_arg_returns_undef() {
        // NaN in the imaginary (second) arg should also produce Undef
        let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(f64::NAN)]);
        assert!(
            result.is_undef(),
            "expected Undef for NaN im, got {:?}",
            result
        );
    }

    #[test]
    fn complex_inf_im_arg_returns_undef() {
        // Infinity in the imaginary (second) arg should also produce Undef
        let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(f64::INFINITY)]);
        assert!(
            result.is_undef(),
            "expected Undef for Inf im, got {:?}",
            result
        );
    }

    // ── re() and im() accessor tests (step-7) ────────────────────────────────

    #[test]
    fn re_dimensionless_returns_real() {
        // re(Complex{3,4,DIMLESS}) → Real(3.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("re", &[z]), 3.0);
    }

    #[test]
    fn im_dimensionless_returns_real() {
        // im(Complex{3,4,DIMLESS}) → Real(4.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("im", &[z]), 4.0);
    }

    #[test]
    fn re_dimensioned_returns_scalar() {
        // re(Complex{5,3,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("re", &[z]), 5.0, DimensionVector::LENGTH);
    }

    #[test]
    fn im_dimensioned_returns_scalar() {
        // im(Complex{5,3,LENGTH}) → Scalar{3.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("im", &[z]), 3.0, DimensionVector::LENGTH);
    }

    #[test]
    fn re_non_complex_returns_undef() {
        assert!(eval_builtin("re", &[Value::Real(3.0)]).is_undef());
    }

    #[test]
    fn im_non_complex_returns_undef() {
        assert!(eval_builtin("im", &[Value::Real(3.0)]).is_undef());
    }

    // ── conjugate() tests (step-9) ────────────────────────────────────────────

    #[test]
    fn conjugate_dimensionless_negates_im() {
        // conjugate(Complex{3,4,DIMLESS}) → Complex{3,-4,DIMLESS}
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("conjugate", &[z]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12);
                assert!((im - (-4.0)).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{3,-4,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn conjugate_dimensioned_preserves_dimension() {
        // conjugate(Complex{5,3,LENGTH}) → Complex{5,-3,LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        let result = eval_builtin("conjugate", &[z]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 5.0).abs() < 1e-12);
                assert!((im - (-3.0)).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{5,-3,LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn conjugate_non_complex_returns_undef() {
        assert!(eval_builtin("conjugate", &[Value::Real(3.0)]).is_undef());
    }

    #[test]
    fn conjugate_nan_re_returns_undef() {
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with NaN re must return Undef"
        );
    }

    #[test]
    fn conjugate_nan_im_returns_undef() {
        let z = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with NaN im must return Undef"
        );
    }

    #[test]
    fn conjugate_inf_re_returns_undef() {
        let z = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with Inf re must return Undef"
        );
    }

    #[test]
    fn conjugate_inf_im_returns_undef() {
        let z = Value::Complex {
            re: 1.0,
            im: f64::NEG_INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with -Inf im must return Undef"
        );
    }

    // ── magnitude on Complex tests (step-11) ─────────────────────────────────

    #[test]
    fn magnitude_complex_dimensionless_3_4_returns_5() {
        // magnitude(Complex{3,4,DIMLESS}) → Real(5.0) (3-4-5 Pythagorean triple)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("magnitude", &[z]), 5.0);
    }

    #[test]
    fn magnitude_complex_dimensioned_3_4_returns_scalar_5() {
        // magnitude(Complex{3,4,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("magnitude", &[z]),
            5.0,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn magnitude_large_representable_complex_no_overflow() {
        // magnitude(Complex{1e200, 0, DIMLESS}) must return Real(1e200), not Undef.
        // Covers the generic 'magnitude' builtin path to complex_abs.
        let z = Value::Complex {
            re: 1e200,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("magnitude", &[z]), 1e200);
    }

    #[test]
    fn magnitude_zero_complex_returns_zero() {
        // magnitude(0+0i) = 0.0 (zero vector has zero magnitude, unlike phase which is undef)
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("magnitude", &[z]), 0.0);
    }

    #[test]
    fn complex_magnitude_zero_complex_returns_zero() {
        // complex_magnitude(0+0i) = 0.0
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("complex_magnitude", &[z]), 0.0);
    }

    // ── phase() tests (step-13) ───────────────────────────────────────────────

    #[test]
    fn phase_complex_1_1_returns_pi_over_4() {
        // phase(1+1i) = π/4
        let z = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("phase", &[z]),
            std::f64::consts::FRAC_PI_4,
            DimensionVector::ANGLE
        );
    }

    #[test]
    fn phase_complex_1_0_returns_0() {
        // phase(1+0i) = 0
        let z = Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(eval_builtin("phase", &[z]), 0.0, DimensionVector::ANGLE);
    }

    #[test]
    fn phase_complex_0_1_returns_pi_over_2() {
        // phase(0+1i) = π/2
        let z = Value::Complex {
            re: 0.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("phase", &[z]),
            std::f64::consts::FRAC_PI_2,
            DimensionVector::ANGLE
        );
    }

    #[test]
    fn phase_non_complex_returns_undef() {
        assert!(eval_builtin("phase", &[Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn phase_zero_complex_returns_undef() {
        // phase(0+0i) is mathematically undefined (zero vector has no direction)
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("phase", &[z]).is_undef(),
            "phase(0+0i) should be Undef, not Scalar{{0.0, ANGLE}}"
        );
    }

    // ── complex_add() tests (step-15) ─────────────────────────────────────────

    #[test]
    fn complex_add_dimensionless() {
        // complex_add(1+2i, 3+4i) = 4+6i
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("complex_add", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 4.0).abs() < 1e-12);
                assert!((im - 6.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{4,6,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_add_dimensioned_preserves_dimension() {
        // complex_add(a+bi [LENGTH], c+di [LENGTH]) = (a+c)+(b+d)i [LENGTH]
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        let result = eval_builtin("complex_add", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 4.0).abs() < 1e-12);
                assert!((im - 6.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{4,6,LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_add_dimension_mismatch_returns_undef() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(eval_builtin("complex_add", &[a, b]).is_undef());
    }

    #[test]
    fn complex_add_non_complex_arg_returns_undef() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(eval_builtin("complex_add", &[a, Value::Real(3.0)]).is_undef());
    }

    // ── complex_mul() tests (step-17) ─────────────────────────────────────────

    #[test]
    fn complex_mul_dimensionless() {
        // (1+2i)(3+4i) = (3-8) + (4+6)i = -5 + 10i
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("complex_mul", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - (-5.0)).abs() < 1e-12, "expected re=-5.0, got {}", re);
                assert!((im - 10.0).abs() < 1e-12, "expected im=10.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{-5,10,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_mul_dimensioned_combines_dimensions() {
        // complex_mul(LENGTH, LENGTH) → result dimension is LENGTH^2 (AREA)
        let area_dim = DimensionVector::LENGTH.mul(&DimensionVector::LENGTH);
        let a = Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 2.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let result = eval_builtin("complex_mul", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 2.0).abs() < 1e-12, "expected re=2.0, got {}", re);
                assert!((im - 0.0).abs() < 1e-12, "expected im=0.0, got {}", im);
                assert_eq!(dimension, area_dim, "expected AREA dimension");
            }
            other => panic!("expected Complex{{2,0,AREA}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_mul_non_complex_returns_undef() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(eval_builtin("complex_mul", &[a, Value::Real(3.0)]).is_undef());
    }

    // ── Complex<Impedance> integration test (step-19) ─────────────────────────

    #[test]
    fn complex_impedance_integration() {
        // Impedance = kg·m²·s⁻³·A⁻² = MASS·LENGTH²·TIME⁻³·CURRENT⁻²
        // Build as MASS * LENGTH^2 * TIME^-3 * CURRENT^-2
        use reify_types::DimensionVector;
        let mass_dim = DimensionVector::MASS;
        let length_dim = DimensionVector::LENGTH;
        let area = length_dim.mul(&length_dim);
        let mass_area = mass_dim.mul(&area);
        let time3 = DimensionVector::TIME.pow(3);
        let current2 = DimensionVector::CURRENT.pow(2);
        let impedance = mass_area.div(&time3).div(&current2);

        // Create 50 Ω (real part) + -25j Ω (imaginary part)
        let z = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 50.0,
                    dimension: impedance,
                },
                Value::Scalar {
                    si_value: -25.0,
                    dimension: impedance,
                },
            ],
        );
        match &z {
            Value::Complex { re, im, dimension } => {
                assert!((re - 50.0).abs() < 1e-12, "re={}", re);
                assert!((im - (-25.0)).abs() < 1e-12, "im={}", im);
                assert_eq!(*dimension, impedance);
            }
            other => panic!("expected Complex (impedance), got {:?}", other),
        }

        // re accessor → Scalar{50, IMPEDANCE}
        assert_scalar_approx!(
            eval_builtin("re", std::slice::from_ref(&z)),
            50.0,
            impedance
        );

        // im accessor → Scalar{-25, IMPEDANCE}
        assert_scalar_approx!(
            eval_builtin("im", std::slice::from_ref(&z)),
            -25.0,
            impedance
        );

        // magnitude → Scalar{sqrt(50²+25²), IMPEDANCE} = Scalar{sqrt(3125), IMPEDANCE}
        let expected_mag = (50.0_f64 * 50.0 + 25.0 * 25.0).sqrt();
        assert_scalar_approx!(
            eval_builtin("magnitude", std::slice::from_ref(&z)),
            expected_mag,
            impedance
        );

        // conjugate → Complex{50, 25, IMPEDANCE}
        let conj = eval_builtin("conjugate", std::slice::from_ref(&z));
        match &conj {
            Value::Complex { re, im, dimension } => {
                assert!((re - 50.0).abs() < 1e-12);
                assert!((im - 25.0).abs() < 1e-12);
                assert_eq!(*dimension, impedance);
            }
            other => panic!("expected conjugate Complex, got {:?}", other),
        }

        // phase → Scalar{atan2(-25, 50), ANGLE}
        let expected_phase = (-25.0_f64).atan2(50.0);
        assert_scalar_approx!(
            eval_builtin("phase", std::slice::from_ref(&z)),
            expected_phase,
            DimensionVector::ANGLE
        );
    }

    // ── Voltage dimension spec tests (step-7) ────────────────────────────────

    /// Build Voltage dimension: V = kg·m²·s⁻³·A⁻¹
    fn voltage_dim() -> DimensionVector {
        let mass = DimensionVector::MASS;
        let length = DimensionVector::LENGTH;
        let area = length.mul(&length);
        let mass_area = mass.mul(&area);
        let time3 = DimensionVector::TIME.pow(3);
        let current1 = DimensionVector::CURRENT.pow(1);
        mass_area.div(&time3).div(&current1)
    }

    #[test]
    fn complex_voltage_preserves_dimension() {
        // complex(Scalar{3,V}, Scalar{4,V}) → Complex{3,4,V}
        let v = voltage_dim();
        let z = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 3.0,
                    dimension: v,
                },
                Value::Scalar {
                    si_value: 4.0,
                    dimension: v,
                },
            ],
        );
        match &z {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12, "re={}", re);
                assert!((im - 4.0).abs() < 1e-12, "im={}", im);
                assert_eq!(*dimension, v, "dimension should be Voltage");
            }
            other => panic!("expected Complex{{3,4,V}}, got {:?}", other),
        }
    }

    #[test]
    fn real_voltage_returns_scalar() {
        // real(complex_voltage) → Scalar{3, V}
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        assert_scalar_approx!(eval_builtin("real", &[z]), 3.0, v);
    }

    #[test]
    fn imag_voltage_returns_scalar() {
        // imag(complex_voltage) → Scalar{4, V}
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        assert_scalar_approx!(eval_builtin("imag", &[z]), 4.0, v);
    }

    #[test]
    fn complex_magnitude_voltage() {
        // complex_magnitude(Complex{3,4,V}) → Scalar{5.0, V}
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        assert_scalar_approx!(eval_builtin("complex_magnitude", &[z]), 5.0, v);
    }

    #[test]
    fn conjugate_voltage_preserves_dim() {
        // conjugate flips im sign, preserves voltage dimension
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        let result = eval_builtin("conjugate", &[z]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12, "re={}", re);
                assert!((im - (-4.0)).abs() < 1e-12, "im={}", im);
                assert_eq!(dimension, v, "dimension should be Voltage");
            }
            other => panic!("expected Complex{{3,-4,V}}, got {:?}", other),
        }
    }

    // ── Dimension mismatch spec test (step-8) ─────────────────────────────────

    #[test]
    fn complex_voltage_current_mismatch_returns_undef() {
        // complex(Scalar{3, Voltage}, Scalar{4, Current}) → Undef (mismatched dims)
        let voltage = voltage_dim();
        // Current dimension: A (SI base, exponent 1 in CURRENT slot)
        let current = DimensionVector::CURRENT;
        let result = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 3.0,
                    dimension: voltage,
                },
                Value::Scalar {
                    si_value: 4.0,
                    dimension: current,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "expected Undef for V/A mismatch, got {:?}",
            result
        );
    }

    // ── Phase degree-equivalent spec test (step-9) ───────────────────────────

    #[test]
    fn phase_1_plus_i_approx_45_deg() {
        // phase(1+i) = atan2(1,1) = π/4 ≈ 0.7854 rad (45°)
        let z = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("phase", &[z]),
            std::f64::consts::FRAC_PI_4, // π/4 ≈ 0.7854 rad ≈ 45°
            DimensionVector::ANGLE
        );
    }

    // ── sanitize_value Complex arm tests (step-20) ────────────────────────────

    #[test]
    fn sanitize_complex_nan_re_returns_undef() {
        let v = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with NaN re should become Undef"
        );
    }

    #[test]
    fn sanitize_complex_nan_im_returns_undef() {
        let v = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with NaN im should become Undef"
        );
    }

    #[test]
    fn sanitize_complex_inf_re_returns_undef() {
        let v = Value::Complex {
            re: f64::INFINITY,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with +Inf re should become Undef"
        );
    }

    #[test]
    fn sanitize_complex_neg_inf_im_returns_undef() {
        let v = Value::Complex {
            re: 0.0,
            im: f64::NEG_INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with -Inf im should become Undef"
        );
    }

    #[test]
    fn sanitize_complex_finite_passthrough() {
        let v = Value::Complex {
            re: 3.0,
            im: -4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        match sanitize_value(v) {
            Value::Complex { re, im, .. } => {
                assert!((re - 3.0).abs() < f64::EPSILON);
                assert!((im - (-4.0)).abs() < f64::EPSILON);
            }
            other => panic!("expected Complex{{re:3.0, im:-4.0}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_mul_overflow_returns_undef() {
        // (f64::MAX + f64::MAX*i) * (f64::MAX + f64::MAX*i)
        // re = MAX*MAX - MAX*MAX = 0 (actually NaN-ish), im = MAX*MAX + MAX*MAX = +Inf
        // Either component going Inf/NaN must produce Undef.
        let a = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("complex_mul", &[a, b]).is_undef(),
            "complex_mul with f64::MAX components must return Undef (Inf overflow)"
        );
    }

    #[test]
    fn complex_add_overflow_returns_undef() {
        // f64::MAX + f64::MAX = +Inf overflow
        let a = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("complex_add", &[a, b]).is_undef(),
            "complex_add with f64::MAX components must return Undef (Inf overflow)"
        );
    }

    // ── sanitize_value Orientation arm tests (task-904) ──────────────────────

    #[test]
    fn sanitize_orientation_nan_returns_undef() {
        let v = Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with NaN component should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: f64::INFINITY,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with Inf component should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_x_nan_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: f64::NAN,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with NaN x should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_w_inf_returns_undef() {
        let v = Value::Orientation {
            w: f64::INFINITY,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with +Inf w should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_all_components_nonfinite_returns_undef() {
        let v = Value::Orientation {
            w: f64::NAN,
            x: f64::INFINITY,
            y: f64::NEG_INFINITY,
            z: f64::NAN,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with all non-finite components should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_valid_passthrough() {
        let v = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_orientation_approx!(sanitize_value(v), 1.0, 0.0, 0.0, 0.0);
    }

    // ── re/real sanitize_value tests (task-358 step-1) ─────────────────────────

    #[test]
    fn re_nan_re_component_returns_undef() {
        // re(Complex{NaN, 1.0, DIMLESS}) → Undef (NaN must not propagate)
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("re", &[z]).is_undef(),
            "re() with NaN real component must return Undef"
        );
    }

    #[test]
    fn re_inf_re_component_returns_undef() {
        // re(Complex{+Inf, 1.0, DIMLESS}) → Undef (Inf must not propagate)
        let z = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("re", &[z]).is_undef(),
            "re() with Inf real component must return Undef"
        );
    }

    #[test]
    fn re_nan_dimensioned_returns_undef() {
        // re(Complex{NaN, 1.0, LENGTH}) → Undef (dimensioned Scalar path)
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            eval_builtin("re", &[z]).is_undef(),
            "re() with NaN dimensioned real component must return Undef"
        );
    }

    #[test]
    fn real_nan_re_component_returns_undef() {
        // real(Complex{NaN, 1.0, DIMLESS}) → Undef (alias coverage)
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("real", &[z]).is_undef(),
            "real() with NaN real component must return Undef"
        );
    }

    // ── real() alias tests (step-1) ───────────────────────────────────────────

    #[test]
    fn real_dimensionless_returns_real() {
        // real(Complex{3,4,DIMLESS}) → Real(3.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("real", &[z]), 3.0);
    }

    #[test]
    fn real_dimensioned_returns_scalar() {
        // real(Complex{5,3,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("real", &[z]), 5.0, DimensionVector::LENGTH);
    }

    #[test]
    fn real_non_complex_returns_undef() {
        assert!(eval_builtin("real", &[Value::Real(3.0)]).is_undef());
    }

    // ── im/imag sanitize_value tests (task-358 step-3) ─────────────────────────

    #[test]
    fn im_nan_im_component_returns_undef() {
        // im(Complex{1.0, NaN, DIMLESS}) → Undef (NaN must not propagate)
        let z = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("im", &[z]).is_undef(),
            "im() with NaN imaginary component must return Undef"
        );
    }

    #[test]
    fn im_inf_im_component_returns_undef() {
        // im(Complex{1.0, +Inf, DIMLESS}) → Undef (Inf must not propagate)
        let z = Value::Complex {
            re: 1.0,
            im: f64::INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("im", &[z]).is_undef(),
            "im() with Inf imaginary component must return Undef"
        );
    }

    #[test]
    fn im_inf_dimensioned_returns_undef() {
        // im(Complex{1.0, +Inf, LENGTH}) → Undef (dimensioned Scalar path)
        let z = Value::Complex {
            re: 1.0,
            im: f64::INFINITY,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            eval_builtin("im", &[z]).is_undef(),
            "im() with Inf dimensioned imaginary component must return Undef"
        );
    }

    #[test]
    fn imag_nan_im_component_returns_undef() {
        // imag(Complex{1.0, NaN, DIMLESS}) → Undef (alias coverage)
        let z = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("imag", &[z]).is_undef(),
            "imag() with NaN imaginary component must return Undef"
        );
    }

    // ── imag() alias tests (step-3) ───────────────────────────────────────────

    #[test]
    fn imag_dimensionless_returns_real() {
        // imag(Complex{3,4,DIMLESS}) → Real(4.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("imag", &[z]), 4.0);
    }

    #[test]
    fn imag_dimensioned_returns_scalar() {
        // imag(Complex{5,3,LENGTH}) → Scalar{3.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("imag", &[z]), 3.0, DimensionVector::LENGTH);
    }

    #[test]
    fn imag_non_complex_returns_undef() {
        assert!(eval_builtin("imag", &[Value::Real(3.0)]).is_undef());
    }

    // ── magnitude / complex_magnitude edge-case tests: overflow, NaN, dimensioned ──

    /// Assert that evaluating `builtin` with a single `Complex { re, im, dimension }` argument
    /// returns `Value::Undef`. Panics with a descriptive message including the builtin name.
    fn assert_complex_builtin_undef(builtin: &str, re: f64, im: f64, dimension: DimensionVector) {
        let z = Value::Complex { re, im, dimension };
        assert!(
            eval_builtin(builtin, &[z]).is_undef(),
            "{builtin} with Complex{{re={re}, im={im}, dimension={dimension:?}}} must return Undef"
        );
    }

    #[test]
    fn complex_overflow_returns_undef_both_builtins() {
        // Both `magnitude` and `complex_magnitude` delegate to complex_abs for Complex
        // inputs; f64::MAX² + f64::MAX² overflows to +Inf; sanitize_value must catch it.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::MAX,
                f64::MAX,
                DimensionVector::DIMENSIONLESS,
            );
        }
    }

    #[test]
    fn complex_overflow_dimensioned_returns_undef_both_builtins() {
        // Same overflow but through the Scalar branch (non-dimensionless).
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(builtin, f64::MAX, f64::MAX, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_nan_component_returns_undef_both_builtins() {
        // A NaN component propagates through re.hypot(im) and sanitize_value catches it.
        for builtin in ["magnitude", "complex_magnitude"] {
            // re=NaN
            assert_complex_builtin_undef(builtin, f64::NAN, 1.0, DimensionVector::DIMENSIONLESS);
            // im=NaN (symmetric case)
            assert_complex_builtin_undef(builtin, 1.0, f64::NAN, DimensionVector::DIMENSIONLESS);
        }
    }

    #[test]
    fn complex_nan_dimensioned_returns_undef_both_builtins() {
        // NaN component with non-dimensionless input exercises the Value::Scalar arm of
        // sanitize_value (rather than Value::Real). Ensures the Scalar path is covered.
        for builtin in ["magnitude", "complex_magnitude"] {
            // re=NaN, im=1.0, LENGTH dimension → hits Scalar arm
            assert_complex_builtin_undef(builtin, f64::NAN, 1.0, DimensionVector::LENGTH);
            // im=NaN, re=1.0, LENGTH dimension → symmetric case
            assert_complex_builtin_undef(builtin, 1.0, f64::NAN, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_both_nan_returns_undef_both_builtins() {
        // hypot(NaN, NaN) = NaN per IEEE 754; test both dimensionless and dimensioned paths.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::NAN,
                f64::NAN,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, f64::NAN, f64::NAN, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_direct_infinity_returns_undef_both_builtins() {
        // Direct ±Infinity inputs (not computed overflow) are also caught by sanitize_value.
        // hypot(±Inf, x) = +Inf for any finite x per IEEE 754.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::INFINITY,
                0.0,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, f64::INFINITY, 0.0, DimensionVector::LENGTH);
            assert_complex_builtin_undef(
                builtin,
                0.0,
                f64::NEG_INFINITY,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, 0.0, f64::NEG_INFINITY, DimensionVector::LENGTH);
            // im=+Inf (symmetric of re=+Inf)
            assert_complex_builtin_undef(
                builtin,
                0.0,
                f64::INFINITY,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, 0.0, f64::INFINITY, DimensionVector::LENGTH);
            // re=-Inf (symmetric of im=-Inf)
            assert_complex_builtin_undef(
                builtin,
                f64::NEG_INFINITY,
                0.0,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, f64::NEG_INFINITY, 0.0, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_both_infinite_returns_undef_both_builtins() {
        // hypot(Inf, Inf) = +Inf per IEEE 754; sanitize_value must catch it.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::INFINITY,
                f64::INFINITY,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(
                builtin,
                f64::INFINITY,
                f64::INFINITY,
                DimensionVector::LENGTH,
            );
        }
    }

    // ── complex_magnitude() tests ─────────────────────────────────────────────

    #[test]
    fn complex_magnitude_3_4_returns_5() {
        // complex_magnitude(Complex{3,4,DIMLESS}) → Real(5.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("complex_magnitude", &[z]), 5.0);
    }

    #[test]
    fn complex_magnitude_dimensioned_returns_scalar() {
        // complex_magnitude(Complex{3,4,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("complex_magnitude", &[z]),
            5.0,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn complex_magnitude_non_complex_returns_undef() {
        // unlike generic magnitude which handles Tensors, complex_magnitude rejects non-Complex
        assert!(eval_builtin("complex_magnitude", &[Value::Real(5.0)]).is_undef());
    }

    #[test]
    fn complex_magnitude_large_representable_no_overflow() {
        // 1e200 is representable as f64, so |1e200 + 0i| = 1e200 must NOT overflow.
        // The naive (re*re + im*im).sqrt() formula fails because 1e200² = Inf.
        let z = Value::Complex {
            re: 1e200,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("complex_magnitude", &[z]), 1e200);
    }

    #[test]
    fn complex_magnitude_large_dimensioned_no_overflow() {
        // |1e200 + 0i| with LENGTH dimension must return Scalar{1e200, LENGTH}, not Undef.
        // Covers the dimensioned (Scalar) branch of complex_abs with large values.
        let z = Value::Complex {
            re: 1e200,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("complex_magnitude", &[z]),
            1e200,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn complex_magnitude_large_both_components() {
        // |1e200 + 1e200i| = 1e200 * sqrt(2) ≈ 1.4142e200, fully representable.
        // The naive formula fails because 1e200² + 1e200² overflows.
        let z = Value::Complex {
            re: 1e200,
            im: 1e200,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("complex_magnitude", &[z]);
        let expected = 1e200 * std::f64::consts::SQRT_2;
        match result {
            Value::Real(v) => {
                let rel_err = ((v - expected) / expected).abs();
                assert!(
                    rel_err < 1e-14,
                    "expected Real({expected}) got Real({v}), relative error {rel_err}"
                );
            }
            other => panic!("expected Real({expected}), got {other:?}"),
        }
    }

    // ── frame3 tests (step-5) ────────────────────────────────────────────────

    #[test]
    fn frame3_valid_args_returns_frame() {
        let origin = make_point3_len();
        let basis = make_identity_orientation();
        let result = eval_builtin("frame3", &[origin.clone(), basis.clone()]);
        match result {
            Value::Frame {
                origin: o,
                basis: b,
            } => {
                assert_eq!(*o, origin);
                assert_eq!(*b, basis);
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_stores_origin_and_basis_correctly() {
        let origin = Value::Point(vec![
            Value::length(5.0),
            Value::length(6.0),
            Value::length(7.0),
        ]);
        let basis = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let result = eval_builtin("frame3", &[origin.clone(), basis.clone()]);
        match result {
            Value::Frame {
                origin: o,
                basis: b,
            } => {
                assert_eq!(*o, origin, "origin should be stored exactly");
                assert_eq!(*b, basis, "basis should be stored exactly");
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_no_args_returns_undef() {
        assert!(eval_builtin("frame3", &[]).is_undef());
    }

    #[test]
    fn frame3_one_arg_returns_undef() {
        assert!(eval_builtin("frame3", &[make_point3_len()]).is_undef());
    }

    #[test]
    fn frame3_three_args_returns_undef() {
        let o = make_point3_len();
        let b = make_identity_orientation();
        assert!(eval_builtin("frame3", &[o.clone(), b.clone(), Value::Real(0.0)]).is_undef());
    }

    #[test]
    fn frame3_non_point_first_arg_returns_undef() {
        let basis = make_identity_orientation();
        // First arg is Real, not Point
        assert!(eval_builtin("frame3", &[Value::Real(1.0), basis]).is_undef());
    }

    #[test]
    fn frame3_non_orientation_second_arg_returns_undef() {
        let origin = make_point3_len();
        // Second arg is Real, not Orientation
        assert!(eval_builtin("frame3", &[origin, Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn frame3_point2_origin_returns_undef() {
        // Point2 (wrong component count) should be rejected
        let origin_2d = Value::Point(vec![Value::length(1.0), Value::length(2.0)]);
        let basis = make_identity_orientation();
        assert!(eval_builtin("frame3", &[origin_2d, basis]).is_undef());
    }

    #[test]
    fn frame3_point4_origin_returns_undef() {
        // Point4 (wrong component count) should be rejected
        let origin_4d = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0),
        ]);
        let basis = make_identity_orientation();
        assert!(eval_builtin("frame3", &[origin_4d, basis]).is_undef());
    }

    #[test]
    fn frame3_dimensionless_point3_is_accepted() {
        // Point3 with dimensionless (Real) components is accepted
        let origin = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let basis = make_identity_orientation();
        let result = eval_builtin("frame3", &[origin.clone(), basis.clone()]);
        assert!(
            matches!(&result, Value::Frame { .. }),
            "expected Value::Frame for dimensionless Point3 origin, got {:?}",
            result
        );
    }

    // ── frame3_identity tests (step-7) ────────────────────────────────────────

    #[test]
    fn frame3_identity_no_args_returns_frame() {
        let result = eval_builtin("frame3_identity", &[]);
        assert!(
            matches!(&result, Value::Frame { .. }),
            "expected Value::Frame, got {:?}",
            result
        );
    }

    #[test]
    fn frame3_identity_origin_is_zero_length_point3() {
        let result = eval_builtin("frame3_identity", &[]);
        match result {
            Value::Frame { origin, .. } => {
                let expected_origin = Value::Point(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::length(0.0),
                ]);
                assert_eq!(
                    *origin, expected_origin,
                    "identity origin should be zero Point3<Length>"
                );
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_identity_basis_is_identity_quaternion() {
        let result = eval_builtin("frame3_identity", &[]);
        match result {
            Value::Frame { basis, .. } => {
                let expected_basis = Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                };
                assert_eq!(
                    *basis, expected_basis,
                    "identity basis should be (w:1,x:0,y:0,z:0)"
                );
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_identity_with_any_args_returns_undef() {
        assert!(eval_builtin("frame3_identity", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("frame3_identity", &[Value::Real(1.0), Value::Real(2.0)]).is_undef());
        assert!(
            eval_builtin(
                "frame3_identity",
                &[Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]
            )
            .is_undef()
        );
        assert!(
            eval_builtin(
                "frame3_identity",
                &[
                    Value::Real(1.0),
                    Value::Real(2.0),
                    Value::Real(3.0),
                    Value::Real(4.0)
                ]
            )
            .is_undef()
        );
    }

    // ── transform3 tests (step-5) ─────────────────────────────────────────────

    fn make_vec3_length() -> Value {
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    #[test]
    fn transform3_valid_args_returns_transform() {
        let rotation = make_identity_orientation();
        let translation = make_vec3_length();
        let result = eval_builtin("transform3", &[rotation.clone(), translation.clone()]);
        match result {
            Value::Transform {
                rotation: r,
                translation: t,
            } => {
                assert_eq!(*r, rotation);
                assert_eq!(*t, translation);
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_stores_rotation_and_translation_correctly() {
        let rotation = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let translation = Value::Vector(vec![
            Value::length(5.0),
            Value::length(6.0),
            Value::length(7.0),
        ]);
        let result = eval_builtin("transform3", &[rotation.clone(), translation.clone()]);
        match result {
            Value::Transform {
                rotation: r,
                translation: t,
            } => {
                assert_eq!(*r, rotation, "rotation should be stored exactly");
                assert_eq!(*t, translation, "translation should be stored exactly");
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_no_args_returns_undef() {
        assert!(eval_builtin("transform3", &[]).is_undef());
    }

    #[test]
    fn transform3_one_arg_returns_undef() {
        assert!(eval_builtin("transform3", &[make_identity_orientation()]).is_undef());
    }

    #[test]
    fn transform3_three_args_returns_undef() {
        let r = make_identity_orientation();
        let t = make_vec3_length();
        assert!(eval_builtin("transform3", &[r.clone(), t.clone(), Value::Real(0.0)]).is_undef());
    }

    #[test]
    fn transform3_non_orientation_first_arg_returns_undef() {
        // First arg is Real, not Orientation
        assert!(eval_builtin("transform3", &[Value::Real(1.0), make_vec3_length()]).is_undef());
    }

    #[test]
    fn transform3_non_vector_second_arg_returns_undef() {
        // Second arg is Real, not Vector
        assert!(
            eval_builtin(
                "transform3",
                &[make_identity_orientation(), Value::Real(1.0)]
            )
            .is_undef()
        );
    }

    #[test]
    fn transform3_point3_second_arg_returns_undef() {
        // Second arg is Point3, not Vector3
        let pt3 = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        assert!(eval_builtin("transform3", &[make_identity_orientation(), pt3]).is_undef());
    }

    #[test]
    fn transform3_orientation_second_arg_returns_undef() {
        // Second arg is Orientation, not Vector3
        assert!(
            eval_builtin(
                "transform3",
                &[make_identity_orientation(), make_identity_orientation()]
            )
            .is_undef()
        );
    }

    #[test]
    fn transform3_vector2_translation_returns_undef() {
        // Vector2 (wrong component count) should be rejected
        let vec2 = Value::Vector(vec![Value::length(1.0), Value::length(2.0)]);
        assert!(eval_builtin("transform3", &[make_identity_orientation(), vec2]).is_undef());
    }

    #[test]
    fn transform3_dimensionless_vector3_is_accepted() {
        // Vector3 with dimensionless (Real) components is accepted
        let translation = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let result = eval_builtin(
            "transform3",
            &[make_identity_orientation(), translation.clone()],
        );
        assert!(
            matches!(&result, Value::Transform { .. }),
            "expected Value::Transform for dimensionless Vector3 translation, got {:?}",
            result
        );
    }

    // ── transform3_identity tests (step-7) ────────────────────────────────────

    #[test]
    fn transform3_identity_no_args_returns_transform() {
        let result = eval_builtin("transform3_identity", &[]);
        assert!(
            matches!(&result, Value::Transform { .. }),
            "expected Value::Transform, got {:?}",
            result
        );
    }

    #[test]
    fn transform3_identity_rotation_is_identity_quaternion() {
        let result = eval_builtin("transform3_identity", &[]);
        match result {
            Value::Transform { rotation, .. } => {
                let expected = Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                };
                assert_eq!(
                    *rotation, expected,
                    "identity rotation should be (w:1,x:0,y:0,z:0)"
                );
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_identity_translation_is_zero_length_vector3() {
        let result = eval_builtin("transform3_identity", &[]);
        match result {
            Value::Transform { translation, .. } => {
                let expected = Value::Vector(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::length(0.0),
                ]);
                assert_eq!(
                    *translation, expected,
                    "identity translation should be zero Vector3<Length>"
                );
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_identity_with_any_args_returns_undef() {
        assert!(eval_builtin("transform3_identity", &[Value::Real(1.0)]).is_undef());
        assert!(
            eval_builtin("transform3_identity", &[Value::Real(1.0), Value::Real(2.0)]).is_undef()
        );
    }

    // ── axis_z tests (step-5) ────────────────────────────────────────────────

    #[test]
    fn axis_z_with_point3_returns_axis() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_z", std::slice::from_ref(&origin));
        assert!(
            matches!(result, Value::Axis { .. }),
            "expected Value::Axis, got {:?}",
            result
        );
    }

    #[test]
    fn axis_z_stores_origin_correctly() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_z", std::slice::from_ref(&origin));
        match result {
            Value::Axis { origin: o, .. } => assert_eq!(*o, origin),
            other => panic!("expected Value::Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_z_direction_is_z() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_z", &[origin]);
        match result {
            Value::Axis { direction, .. } => match *direction {
                Value::Vector(ref comps) => {
                    assert_eq!(comps.len(), 3);
                    assert_eq!(comps[0], Value::Real(0.0));
                    assert_eq!(comps[1], Value::Real(0.0));
                    assert_eq!(comps[2], Value::Real(1.0));
                }
                other => panic!("expected Vector, got {:?}", other),
            },
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_z_no_args_returns_undef() {
        assert!(eval_builtin("axis_z", &[]).is_undef());
    }

    #[test]
    fn axis_z_real_arg_returns_undef() {
        assert!(eval_builtin("axis_z", &[Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn axis_z_point2_returns_undef() {
        assert!(eval_builtin("axis_z", &[make_point2_length()]).is_undef());
    }

    #[test]
    fn axis_z_vector3_returns_undef() {
        let vec3 = Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        assert!(eval_builtin("axis_z", &[vec3]).is_undef());
    }

    // ── axis_x / axis_y tests (step-7) ───────────────────────────────────────

    #[test]
    fn axis_x_direction_is_x() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_x", &[origin]);
        match result {
            Value::Axis { direction, .. } => match *direction {
                Value::Vector(ref comps) => {
                    assert_eq!(comps[0], Value::Real(1.0));
                    assert_eq!(comps[1], Value::Real(0.0));
                    assert_eq!(comps[2], Value::Real(0.0));
                }
                other => panic!("expected Vector, got {:?}", other),
            },
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_y_direction_is_y() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_y", &[origin]);
        match result {
            Value::Axis { direction, .. } => match *direction {
                Value::Vector(ref comps) => {
                    assert_eq!(comps[0], Value::Real(0.0));
                    assert_eq!(comps[1], Value::Real(1.0));
                    assert_eq!(comps[2], Value::Real(0.0));
                }
                other => panic!("expected Vector, got {:?}", other),
            },
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_x_no_args_returns_undef() {
        assert!(eval_builtin("axis_x", &[]).is_undef());
    }

    #[test]
    fn axis_y_two_args_returns_undef() {
        assert!(eval_builtin("axis_y", &[make_point3_length(), make_point3_length()]).is_undef());
    }

    #[test]
    fn axis_x_with_dimensionless_point3() {
        let origin = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let result = eval_builtin("axis_x", std::slice::from_ref(&origin));
        match result {
            Value::Axis { origin: o, .. } => assert_eq!(*o, origin),
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    // ── bbox tests (step-9) ──────────────────────────────────────────────────

    fn make_point3_min() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_point3_max() -> Value {
        Value::Point(vec![
            Value::length(4.0),
            Value::length(6.0),
            Value::length(9.0),
        ])
    }

    #[test]
    fn bbox_with_two_point3_returns_bounding_box() {
        let result = eval_builtin("bbox", &[make_point3_min(), make_point3_max()]);
        assert!(
            matches!(result, Value::BoundingBox { .. }),
            "expected BoundingBox, got {:?}",
            result
        );
    }

    #[test]
    fn bbox_stores_min_and_max() {
        let min = make_point3_min();
        let max = make_point3_max();
        let result = eval_builtin("bbox", &[min.clone(), max.clone()]);
        match result {
            Value::BoundingBox { min: mn, max: mx } => {
                assert_eq!(*mn, min);
                assert_eq!(*mx, max);
            }
            other => panic!("expected BoundingBox, got {:?}", other),
        }
    }

    #[test]
    fn bbox_mismatched_dimensions_returns_undef() {
        let min = Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let max = Value::Point(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
        ]);
        assert!(eval_builtin("bbox", &[min, max]).is_undef());
    }

    #[test]
    fn bbox_non_point_arg_returns_undef() {
        let vec3 = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let pt3 = make_point3_min();
        assert!(eval_builtin("bbox", &[vec3, pt3]).is_undef());
    }

    #[test]
    fn bbox_point2_returns_undef() {
        let pt2 = make_point2_length();
        let pt3 = make_point3_min();
        assert!(eval_builtin("bbox", &[pt2, pt3]).is_undef());
    }

    #[test]
    fn bbox_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("bbox", &[]).is_undef());
        assert!(eval_builtin("bbox", &[make_point3_min()]).is_undef());
        assert!(
            eval_builtin(
                "bbox",
                &[make_point3_min(), make_point3_max(), make_point3_min()]
            )
            .is_undef()
        );
    }

    #[test]
    fn bbox_one_point_one_vector_returns_undef() {
        let pt3 = make_point3_min();
        let vec3 = Value::Vector(vec![
            Value::length(4.0),
            Value::length(6.0),
            Value::length(9.0),
        ]);
        assert!(eval_builtin("bbox", &[pt3, vec3]).is_undef());
    }

    // ── bbox_size / bbox_center tests (step-11) ──────────────────────────────

    fn make_bbox() -> Value {
        Value::BoundingBox {
            min: Box::new(Value::Point(vec![
                Value::length(1.0),
                Value::length(2.0),
                Value::length(3.0),
            ])),
            max: Box::new(Value::Point(vec![
                Value::length(4.0),
                Value::length(6.0),
                Value::length(9.0),
            ])),
        }
    }

    #[test]
    fn bbox_size_returns_correct_vector() {
        // min=(1m,2m,3m), max=(4m,6m,9m) → size=(3m,4m,6m)
        let result = eval_builtin("bbox_size", &[make_bbox()]);
        match result {
            Value::Vector(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(3.0));
                assert_eq!(comps[1], Value::length(4.0));
                assert_eq!(comps[2], Value::length(6.0));
            }
            other => panic!("expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn bbox_center_returns_correct_point() {
        // min=(1m,2m,3m), max=(4m,6m,9m) → center=(2.5m,4m,6m)
        let result = eval_builtin("bbox_center", &[make_bbox()]);
        match result {
            Value::Point(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(2.5));
                assert_eq!(comps[1], Value::length(4.0));
                assert_eq!(comps[2], Value::length(6.0));
            }
            other => panic!("expected Point, got {:?}", other),
        }
    }

    #[test]
    fn bbox_size_non_bounding_box_returns_undef() {
        assert!(eval_builtin("bbox_size", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("bbox_size", &[make_point3_min()]).is_undef());
    }

    #[test]
    fn bbox_center_non_bounding_box_returns_undef() {
        assert!(eval_builtin("bbox_center", &[Value::Undef]).is_undef());
        assert!(eval_builtin("bbox_center", &[make_point3_min()]).is_undef());
    }

    #[test]
    fn bbox_size_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("bbox_size", &[]).is_undef());
        assert!(eval_builtin("bbox_size", &[make_bbox(), make_bbox()]).is_undef());
    }

    #[test]
    fn bbox_center_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("bbox_center", &[]).is_undef());
        assert!(eval_builtin("bbox_center", &[make_bbox(), make_bbox()]).is_undef());
    }

    #[test]
    fn bbox_size_dimensionless_bbox() {
        let bbox = Value::BoundingBox {
            min: Box::new(Value::Point(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
            max: Box::new(Value::Point(vec![
                Value::Real(2.0),
                Value::Real(4.0),
                Value::Real(6.0),
            ])),
        };
        let result = eval_builtin("bbox_size", &[bbox]);
        match result {
            Value::Vector(ref comps) => {
                assert_eq!(comps[0], Value::Real(2.0));
                assert_eq!(comps[1], Value::Real(4.0));
                assert_eq!(comps[2], Value::Real(6.0));
            }
            other => panic!("expected Vector of Reals, got {:?}", other),
        }
    }

    // ── plane_xz / plane_yz tests (step-3) ───────────────────────────────────

    #[test]
    fn plane_xz_with_length_offset_returns_plane() {
        let result = eval_builtin("plane_xz", &[Value::length(0.003)]);
        assert!(
            matches!(result, Value::Plane { .. }),
            "expected Value::Plane, got {:?}",
            result
        );
    }

    #[test]
    fn plane_xz_correct_origin_and_normal() {
        // plane_xz(3mm) → origin=(0m, 3mm, 0m), normal=(0,1,0)
        let result = eval_builtin("plane_xz", &[Value::length(0.003)]);
        match result {
            Value::Plane { origin, normal } => {
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3);
                        assert_eq!(comps[0], Value::length(0.0), "x should be 0m");
                        assert_eq!(comps[1], Value::length(0.003), "y should be 3mm");
                        assert_eq!(comps[2], Value::length(0.0), "z should be 0m");
                    }
                    other => panic!("expected Point, got {:?}", other),
                }
                match *normal {
                    Value::Vector(ref comps) => {
                        assert_eq!(comps[0], Value::Real(0.0));
                        assert_eq!(comps[1], Value::Real(1.0));
                        assert_eq!(comps[2], Value::Real(0.0));
                    }
                    other => panic!("expected Vector, got {:?}", other),
                }
            }
            other => panic!("expected Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_yz_with_length_offset_returns_plane() {
        let result = eval_builtin("plane_yz", &[Value::length(0.007)]);
        assert!(
            matches!(result, Value::Plane { .. }),
            "expected Value::Plane, got {:?}",
            result
        );
    }

    #[test]
    fn plane_yz_correct_origin_and_normal() {
        // plane_yz(7mm) → origin=(7mm, 0m, 0m), normal=(1,0,0)
        let result = eval_builtin("plane_yz", &[Value::length(0.007)]);
        match result {
            Value::Plane { origin, normal } => {
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3);
                        assert_eq!(comps[0], Value::length(0.007), "x should be 7mm");
                        assert_eq!(comps[1], Value::length(0.0), "y should be 0m");
                        assert_eq!(comps[2], Value::length(0.0), "z should be 0m");
                    }
                    other => panic!("expected Point, got {:?}", other),
                }
                match *normal {
                    Value::Vector(ref comps) => {
                        assert_eq!(comps[0], Value::Real(1.0));
                        assert_eq!(comps[1], Value::Real(0.0));
                        assert_eq!(comps[2], Value::Real(0.0));
                    }
                    other => panic!("expected Vector, got {:?}", other),
                }
            }
            other => panic!("expected Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_xz_no_args_returns_undef() {
        assert!(eval_builtin("plane_xz", &[]).is_undef());
    }

    #[test]
    fn plane_yz_no_args_returns_undef() {
        assert!(eval_builtin("plane_yz", &[]).is_undef());
    }

    #[test]
    fn plane_xz_nan_returns_undef() {
        assert!(eval_builtin("plane_xz", &[Value::Real(f64::NAN)]).is_undef());
    }

    #[test]
    fn plane_yz_two_args_returns_undef() {
        assert!(eval_builtin("plane_yz", &[Value::length(0.0), Value::length(0.0)]).is_undef());
    }

    // ── plane_xy tests (step-1) ───────────────────────────────────────────────

    #[test]
    fn plane_xy_with_length_offset_returns_plane() {
        // plane_xy(5mm) → Plane with origin=(0m,0m,5mm) and normal=(0,0,1)
        let offset = Value::length(0.005); // 5mm in SI (meters)
        let result = eval_builtin("plane_xy", &[offset]);
        assert!(
            matches!(result, Value::Plane { .. }),
            "expected Value::Plane, got {:?}",
            result
        );
    }

    #[test]
    fn plane_xy_with_length_offset_correct_origin() {
        let offset = Value::length(0.005); // 5mm
        let result = eval_builtin("plane_xy", &[offset]);
        match result {
            Value::Plane { origin, .. } => {
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3, "origin should be 3D");
                        // x=0m, y=0m, z=5mm
                        assert_eq!(comps[0], Value::length(0.0), "origin.x should be 0m");
                        assert_eq!(comps[1], Value::length(0.0), "origin.y should be 0m");
                        assert_eq!(comps[2], Value::length(0.005), "origin.z should be 5mm");
                    }
                    other => panic!("origin should be Point, got {:?}", other),
                }
            }
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_xy_with_length_offset_correct_normal() {
        let offset = Value::length(0.005);
        let result = eval_builtin("plane_xy", &[offset]);
        match result {
            Value::Plane { normal, .. } => match *normal {
                Value::Vector(ref comps) => {
                    assert_eq!(comps.len(), 3, "normal should be 3D");
                    assert_eq!(comps[0], Value::Real(0.0), "normal.x should be 0");
                    assert_eq!(comps[1], Value::Real(0.0), "normal.y should be 0");
                    assert_eq!(comps[2], Value::Real(1.0), "normal.z should be 1");
                }
                other => panic!("normal should be Vector, got {:?}", other),
            },
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_xy_no_args_returns_undef() {
        assert!(eval_builtin("plane_xy", &[]).is_undef());
    }

    #[test]
    fn plane_xy_bool_arg_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::Bool(true)]).is_undef());
    }

    #[test]
    fn plane_xy_two_args_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::length(0.0), Value::length(0.0)]).is_undef());
    }

    #[test]
    fn plane_xy_nan_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::Real(f64::NAN)]).is_undef());
    }

    #[test]
    fn plane_xy_inf_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::Real(f64::INFINITY)]).is_undef());
    }

    #[test]
    fn plane_xy_real_zero_produces_dimensionless_origin() {
        // plane_xy(Real(0.0)) → dimensionless origin with Real(0.0) components
        let result = eval_builtin("plane_xy", &[Value::Real(0.0)]);
        match result {
            Value::Plane { origin, .. } => match *origin {
                Value::Point(ref comps) => {
                    assert_eq!(comps.len(), 3);
                    assert_eq!(comps[0], Value::Real(0.0));
                    assert_eq!(comps[1], Value::Real(0.0));
                    assert_eq!(comps[2], Value::Real(0.0));
                }
                other => panic!("expected Point, got {:?}", other),
            },
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    // ── step-7: frame_to_frame tests ─────────────────────────────────────────

    /// Helper: build a Frame with given origin (LENGTH) and orientation.
    fn make_frame(ox: f64, oy: f64, oz: f64, orientation: Value) -> Value {
        Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(ox),
                Value::length(oy),
                Value::length(oz),
            ])),
            basis: Box::new(orientation),
        }
    }

    /// Helper: 90-degree Z rotation quaternion.
    fn make_rot90z() -> Value {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        }
    }

    /// frame_to_frame(F, F) should return an identity transform.
    #[test]
    fn frame_to_frame_same_gives_identity() {
        let f = make_frame(5.0, 3.0, 1.0, make_identity_orientation());
        let result = eval_builtin("frame_to_frame", &[f.clone(), f]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                // Identity rotation
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-10);
                // Zero translation
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected ~0");
                        }
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// frame_to_frame(origin_frame, translated_frame) gives pure translation.
    #[test]
    fn frame_to_frame_translated() {
        let from = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        let to = make_frame(5.0, 0.0, 0.0, make_identity_orientation());
        let result = eval_builtin("frame_to_frame", &[from, to]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                // Identity rotation
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-10);
                // Translation = (5,0,0)
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 5.0).abs() < 1e-10, "tx = {tx}, expected 5");
                        assert!(ty.abs() < 1e-10, "ty = {ty}, expected 0");
                        assert!(tz.abs() < 1e-10, "tz = {tz}, expected 0");
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// frame_to_frame(identity_frame, rotated_frame) gives pure rotation.
    #[test]
    fn frame_to_frame_rotated() {
        let from = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        let to = make_frame(0.0, 0.0, 0.0, make_rot90z());
        let result = eval_builtin("frame_to_frame", &[from, to]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                // 90Z rotation
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-10);
                // Zero translation
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected ~0");
                        }
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// frame_to_frame with both rotation and translation.
    /// From: origin=(1,0,0), identity rotation
    /// To: origin=(0,0,0), 90Z rotation
    /// R = R_to * conj(R_from) = 90Z * identity = 90Z
    /// t = origin_to - R * origin_from = (0,0,0) - 90Z*(1,0,0) = (0,0,0) - (0,1,0) = (0,-1,0)
    #[test]
    fn frame_to_frame_general() {
        let from = make_frame(1.0, 0.0, 0.0, make_identity_orientation());
        let to = make_frame(0.0, 0.0, 0.0, make_rot90z());
        let result = eval_builtin("frame_to_frame", &[from, to]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-10);
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!(tx.abs() < 1e-10, "tx = {tx}, expected 0");
                        assert!((ty + 1.0).abs() < 1e-10, "ty = {ty}, expected -1");
                        assert!(tz.abs() < 1e-10, "tz = {tz}, expected 0");
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// Wrong argument count or non-Frame args return Undef.
    #[test]
    fn frame_to_frame_wrong_args_undef() {
        // No args
        assert!(eval_builtin("frame_to_frame", &[]).is_undef());
        // One arg
        let f = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        assert!(eval_builtin("frame_to_frame", std::slice::from_ref(&f)).is_undef());
        // Three args
        assert!(eval_builtin("frame_to_frame", &[f.clone(), f.clone(), f.clone()]).is_undef());
        // Non-Frame args
        assert!(eval_builtin("frame_to_frame", &[Value::Real(1.0), f.clone()]).is_undef());
        assert!(eval_builtin("frame_to_frame", &[f, Value::Real(1.0)]).is_undef());
    }

    /// frame_to_frame with NaN in origin_from x-component should return Undef.
    #[test]
    fn frame_to_frame_nan_origin_from_returns_undef() {
        let from = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::length(0.0),
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        let to = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        assert!(
            eval_builtin("frame_to_frame", &[from, to]).is_undef(),
            "NaN in origin_from should return Undef"
        );
    }

    /// frame_to_frame with NaN in origin_to y-component should return Undef.
    #[test]
    fn frame_to_frame_nan_origin_to_returns_undef() {
        let from = make_frame(1.0, 0.0, 0.0, make_identity_orientation());
        let to = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(0.0),
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        assert!(
            eval_builtin("frame_to_frame", &[from, to]).is_undef(),
            "NaN in origin_to should return Undef"
        );
    }

    /// frame_to_frame with mixed-dimension origin (length, angle, length) should return Undef.
    #[test]
    fn frame_to_frame_mixed_dimension_origin_returns_undef() {
        let from = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(1.0),
                Value::angle(0.0), // dimension mismatch within same origin
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        let to = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        assert!(
            eval_builtin("frame_to_frame", &[from, to]).is_undef(),
            "mixed-dimension origin should return Undef"
        );
    }

    /// frame_to_frame with mismatched origin dimensions (LENGTH vs ANGLE) returns Undef.
    #[test]
    fn frame_to_frame_mismatched_origin_dimensions_undef() {
        // from-frame: LENGTH-dimensioned origin
        let from = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(1.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        // to-frame: ANGLE-dimensioned origin
        let to = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::angle(1.0),
                Value::angle(0.0),
                Value::angle(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        assert!(eval_builtin("frame_to_frame", &[from, to]).is_undef());
    }

    // ── normalize_quaternion near-zero tests ────────────────────────────────

    /// normalize_quaternion with near-zero norm (1e-17 < f64::EPSILON) should return None.
    /// Currently passes because norm != 0.0 is true for 1e-17.
    #[test]
    fn normalize_quaternion_near_zero_returns_none() {
        assert!(
            normalize_quaternion(1e-17, 0.0, 0.0, 0.0).is_none(),
            "near-zero quaternion (norm=1e-17) should return None"
        );
    }

    /// normalize_quaternion with all near-zero components should return None.
    #[test]
    fn normalize_quaternion_all_near_zero_returns_none() {
        assert!(
            normalize_quaternion(1e-18, 1e-18, 1e-18, 1e-18).is_none(),
            "all near-zero components should return None"
        );
    }

    // ── Advanced linalg tests (task 337) ─────────────────────────────────────

    // --- determinant tests ---

    #[test]
    fn det_identity_2x2() {
        let m = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 1.0);
    }

    #[test]
    fn det_identity_3x3() {
        let m = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 1.0);
    }

    #[test]
    fn det_2_times_identity_3x3() {
        // det(2*I₃) = 2³ = 8
        let m = make_matrix(&[&[2.0, 0.0, 0.0], &[0.0, 2.0, 0.0], &[0.0, 0.0, 2.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 8.0);
    }

    #[test]
    fn det_singular_matrix() {
        // Singular: rows are linearly dependent
        let m = make_matrix(&[&[1.0, 2.0], &[2.0, 4.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 0.0);
    }

    #[test]
    fn det_dimensioned_3x3() {
        // det(Force_mat) has dimension Force³ for 3×3
        let force_dim = reify_types::dimension::FORCE;
        let m = make_dimensioned_matrix(
            &[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]],
            force_dim,
        );
        let result = eval_builtin("determinant", &[m]);
        let expected_dim = force_dim.pow(3);
        assert_scalar_approx!(result, 1.0, expected_dim);
    }

    #[test]
    fn det_1x1() {
        let m = make_matrix(&[&[42.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 42.0);
    }

    #[test]
    fn det_non_square_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        assert!(eval_builtin("determinant", &[m]).is_undef());
    }

    // --- inverse tests ---

    #[test]
    fn inverse_2x2_identity() {
        let m = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        let inv = eval_builtin("inverse", std::slice::from_ref(&m));
        // inv(I) = I — check all four elements
        if let Value::Tensor(rows) = &inv {
            assert_eq!(rows.len(), 2);
            for (i, row) in rows.iter().enumerate() {
                if let Value::Tensor(elems) = row {
                    assert_eq!(elems.len(), 2);
                    for (j, elem) in elems.iter().enumerate() {
                        let expected = if i == j { 1.0 } else { 0.0 };
                        let val = elem.as_f64().unwrap();
                        assert!(
                            (val - expected).abs() < 1e-12,
                            "inv[{i}][{j}]: expected {expected}, got {val}"
                        );
                    }
                } else {
                    panic!("expected Tensor row");
                }
            }
        } else {
            panic!("expected Tensor, got {:?}", inv);
        }
    }

    #[test]
    fn inverse_times_original_approx_identity() {
        // A = [[1,2],[3,4]], verify inv(A)*A ≈ I via manual multiply
        let a = make_matrix(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let inv = eval_builtin("inverse", std::slice::from_ref(&a));
        // Extract inv as flat
        let inv_data = matrix_components_f64(&inv).unwrap();
        let a_data = matrix_components_f64(&a).unwrap();
        // Manual 2×2 multiply: product = inv * a
        let (ai, ad) = (inv_data.2, a_data.2);
        let p00 = ai[0] * ad[0] + ai[1] * ad[2];
        let p01 = ai[0] * ad[1] + ai[1] * ad[3];
        let p10 = ai[2] * ad[0] + ai[3] * ad[2];
        let p11 = ai[2] * ad[1] + ai[3] * ad[3];
        assert!((p00 - 1.0).abs() < 1e-10, "p00={p00}");
        assert!((p01).abs() < 1e-10, "p01={p01}");
        assert!((p10).abs() < 1e-10, "p10={p10}");
        assert!((p11 - 1.0).abs() < 1e-10, "p11={p11}");
    }

    #[test]
    fn inverse_3x3() {
        let a = make_matrix(&[&[1.0, 2.0, 3.0], &[0.0, 1.0, 4.0], &[5.0, 6.0, 0.0]]);
        let inv = eval_builtin("inverse", std::slice::from_ref(&a));
        let inv_d = matrix_components_f64(&inv).unwrap();
        let a_d = matrix_components_f64(&a).unwrap();
        // 3×3 multiply to verify ≈ identity
        let (ai, ad) = (inv_d.2, a_d.2);
        for r in 0..3 {
            for c in 0..3 {
                let sum: f64 = (0..3).map(|k| ai[r * 3 + k] * ad[k * 3 + c]).sum();
                let expected = if r == c { 1.0 } else { 0.0 };
                assert!(
                    (sum - expected).abs() < 1e-10,
                    "product[{r}][{c}] = {sum}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn inverse_singular_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0], &[2.0, 4.0]]);
        assert!(
            eval_builtin("inverse", &[m]).is_undef(),
            "inverse of singular matrix should be Undef"
        );
    }

    // --- transpose tests ---

    #[test]
    fn transpose_symmetric_unchanged() {
        // Symmetric matrix: transpose should equal original
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[2.0, 5.0, 6.0], &[3.0, 6.0, 9.0]]);
        let t = eval_builtin("transpose", std::slice::from_ref(&m));
        let orig_d = matrix_components_f64(&m).unwrap();
        let t_d = matrix_components_f64(&t).unwrap();
        assert_eq!(orig_d.0, t_d.0);
        assert_eq!(orig_d.1, t_d.1);
        for (a, b) in orig_d.2.iter().zip(t_d.2.iter()) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn transpose_2x3() {
        // [[1,2,3],[4,5,6]] → [[1,4],[2,5],[3,6]]
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        let t = eval_builtin("transpose", &[m]);
        let t_d = matrix_components_f64(&t).unwrap();
        assert_eq!(t_d.0, 3); // rows
        assert_eq!(t_d.1, 2); // cols
        assert!((t_d.2[0] - 1.0).abs() < 1e-12);
        assert!((t_d.2[1] - 4.0).abs() < 1e-12);
        assert!((t_d.2[2] - 2.0).abs() < 1e-12);
        assert!((t_d.2[3] - 5.0).abs() < 1e-12);
        assert!((t_d.2[4] - 3.0).abs() < 1e-12);
        assert!((t_d.2[5] - 6.0).abs() < 1e-12);
    }

    // --- trace tests ---

    #[test]
    fn trace_identity_3x3() {
        let m = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        assert_real_approx!(eval_builtin("trace", &[m]), 3.0);
    }

    #[test]
    fn trace_general_2x2() {
        let m = make_matrix(&[&[5.0, 3.0], &[7.0, 2.0]]);
        assert_real_approx!(eval_builtin("trace", &[m]), 7.0);
    }

    #[test]
    fn trace_non_square_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        assert!(eval_builtin("trace", &[m]).is_undef());
    }

    // --- outer product tests ---

    #[test]
    fn outer_two_vectors() {
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]);
        let b = Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(5.0)]);
        let result = eval_builtin("outer", &[a, b]);
        let d = matrix_components_f64(&result).unwrap();
        assert_eq!(d.0, 2);
        assert_eq!(d.1, 3);
        // [[3,4,5],[6,8,10]]
        let expected = [3.0, 4.0, 5.0, 6.0, 8.0, 10.0];
        for (got, exp) in d.2.iter().zip(expected.iter()) {
            assert!((got - exp).abs() < 1e-12);
        }
    }

    #[test]
    fn outer_dimensioned_vectors() {
        let length_dim = DimensionVector::LENGTH;
        let force_dim = reify_types::dimension::FORCE;
        let a = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: length_dim,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: length_dim,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 3.0,
                dimension: force_dim,
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: force_dim,
            },
        ]);
        let result = eval_builtin("outer", &[a, b]);
        let d = matrix_components_f64(&result).unwrap();
        assert_eq!(d.3, length_dim.mul(&force_dim));
    }

    // --- eigenvalues tests ---

    #[test]
    fn eigenvalues_diagonal_2x2() {
        let m = make_matrix(&[&[3.0, 0.0], &[0.0, 7.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 2);
            // Sorted: [3, 7]
            assert!((items[0].as_f64().unwrap() - 3.0).abs() < 1e-10);
            assert!((items[1].as_f64().unwrap() - 7.0).abs() < 1e-10);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_diagonal_3x3() {
        let m = make_matrix(&[&[2.0, 0.0, 0.0], &[0.0, 5.0, 0.0], &[0.0, 0.0, 8.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            // Sorted: [2, 5, 8]
            assert!((items[0].as_f64().unwrap() - 2.0).abs() < 1e-10);
            assert!((items[1].as_f64().unwrap() - 5.0).abs() < 1e-10);
            assert!((items[2].as_f64().unwrap() - 8.0).abs() < 1e-10);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_symmetric_3x3() {
        // Symmetric matrix always has real eigenvalues
        let m = make_matrix(&[&[2.0, 1.0, 0.0], &[1.0, 3.0, 1.0], &[0.0, 1.0, 2.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            // Eigenvalues of this matrix: 1, 2, 4
            let eigs: Vec<f64> = items.iter().map(|v| v.as_f64().unwrap()).collect();
            assert!((eigs[0] - 1.0).abs() < 1e-10, "eig0={}", eigs[0]);
            assert!((eigs[1] - 2.0).abs() < 1e-10, "eig1={}", eigs[1]);
            assert!((eigs[2] - 4.0).abs() < 1e-10, "eig2={}", eigs[2]);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_1x1() {
        let m = make_matrix(&[&[42.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 1);
            assert!((items[0].as_f64().unwrap() - 42.0).abs() < 1e-12);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_identity_3x3() {
        let m = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            for item in &items {
                assert!((item.as_f64().unwrap() - 1.0).abs() < 1e-10);
            }
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn inverse_non_square_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        assert!(eval_builtin("inverse", &[m]).is_undef());
    }

    #[test]
    fn determinant_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("determinant", &[]).is_undef());
    }

    #[test]
    fn inverse_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("inverse", &[]).is_undef());
    }

    #[test]
    fn transpose_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("transpose", &[]).is_undef());
    }

    #[test]
    fn trace_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("trace", &[]).is_undef());
    }

    #[test]
    fn eigenvalues_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("eigenvalues", &[]).is_undef());
    }

    #[test]
    fn outer_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("outer", &[]).is_undef());
    }

    #[test]
    fn determinant_non_matrix_returns_undef() {
        assert!(eval_builtin("determinant", &[Value::Real(5.0)]).is_undef());
    }

    #[test]
    fn inverse_dimensioned_2x2() {
        // inverse of dimensioned matrix has inverse dimension
        let length_dim = DimensionVector::LENGTH;
        let m = make_dimensioned_matrix(&[&[1.0, 0.0], &[0.0, 2.0]], length_dim);
        let inv = eval_builtin("inverse", &[m]);
        let d = matrix_components_f64(&inv).unwrap();
        let expected_dim = DimensionVector::DIMENSIONLESS.div(&length_dim);
        assert_eq!(d.3, expected_dim);
        // Check values: inv of diag(1,2) = diag(1, 0.5)
        assert!((d.2[0] - 1.0).abs() < 1e-12);
        assert!((d.2[1]).abs() < 1e-12);
        assert!((d.2[2]).abs() < 1e-12);
        assert!((d.2[3] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn matrix_value_form_works() {
        // Test that Value::Matrix is also accepted
        let m = Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 1.0);
    }
}
