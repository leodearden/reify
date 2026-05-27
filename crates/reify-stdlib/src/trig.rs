use reify_core::DimensionVector;
use reify_ir::Value;

use crate::helpers::{binary_f64, trig_input, unary, unary_f64};

pub(crate) fn eval_trig(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        // --- Trig functions: accept Angle Scalar or bare Real (radians) ---
        "sin" => unary(args, |v| {
            trig_input(v).map_or(Value::Undef, |r| Value::Real(r.sin()))
        }),
        "cos" => unary(args, |v| {
            trig_input(v).map_or(Value::Undef, |r| Value::Real(r.cos()))
        }),
        "tan" => unary(args, |v| {
            trig_input(v).map_or(Value::Undef, |r| Value::Real(r.tan()))
        }),

        // --- Inverse trig: accept Real, return Angle Scalar ---
        "asin" => unary_f64(args, |x| Value::Scalar {
            si_value: x.asin(),
            dimension: DimensionVector::ANGLE,
        }),
        "acos" => unary_f64(args, |x| Value::Scalar {
            si_value: x.acos(),
            dimension: DimensionVector::ANGLE,
        }),
        "atan" => unary_f64(args, |x| Value::Scalar {
            si_value: x.atan(),
            dimension: DimensionVector::ANGLE,
        }),
        "atan2" => binary_f64(args, |y, x| Value::Scalar {
            si_value: y.atan2(x),
            dimension: DimensionVector::ANGLE,
        }),

        // --- Hyperbolic: accept Real, return Real ---
        "sinh" => unary_f64(args, |x| Value::Real(x.sinh())),
        "cosh" => unary_f64(args, |x| Value::Real(x.cosh())),
        "tanh" => unary_f64(args, |x| Value::Real(x.tanh())),

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    // --- Trig function tests ---

    #[test]
    fn sin_angle_scalar() {
        let result = eval_builtin(
            "sin",
            &[Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_4,
                dimension: DimensionVector::ANGLE,
            }],
        );
        match result {
            Value::Real(v) => assert!((v - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-10),
            other => panic!("expected Real(~0.7071), got {:?}", other),
        }
    }

    #[test]
    fn cos_angle_zero() {
        let result = eval_builtin(
            "cos",
            &[Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::ANGLE,
            }],
        );
        match result {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-12),
            other => panic!("expected Real(1.0), got {:?}", other),
        }
    }

    #[test]
    fn tan_angle_pi_over_4() {
        let result = eval_builtin(
            "tan",
            &[Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_4,
                dimension: DimensionVector::ANGLE,
            }],
        );
        match result {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-10),
            other => panic!("expected Real(~1.0), got {:?}", other),
        }
    }

    #[test]
    fn asin_returns_angle() {
        let result = eval_builtin("asin", &[Value::Real(1.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    #[test]
    fn acos_returns_angle() {
        let result = eval_builtin("acos", &[Value::Real(0.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    #[test]
    fn atan_returns_angle() {
        let result = eval_builtin("atan", &[Value::Real(1.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    #[test]
    fn atan2_returns_angle() {
        let result = eval_builtin("atan2", &[Value::Real(1.0), Value::Real(1.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    #[test]
    fn sinh_real() {
        let result = eval_builtin("sinh", &[Value::Real(0.0)]);
        match result {
            Value::Real(v) => assert!((v - 0.0).abs() < 1e-12),
            other => panic!("expected Real(0.0), got {:?}", other),
        }
    }

    #[test]
    fn sin_non_angle_scalar_returns_undef() {
        // A LENGTH scalar should not be accepted by sin
        let result = eval_builtin(
            "sin",
            &[Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            }],
        );
        assert!(result.is_undef(), "sin of LENGTH scalar should be Undef");
    }

    // --- Inverse-trig domain errors and hyperbolic overflow (step-23) ---

    #[test]
    fn asin_out_of_range_positive() {
        let result = eval_builtin("asin", &[Value::Real(2.0)]);
        assert!(
            result.is_undef(),
            "asin(2.0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn asin_out_of_range_negative() {
        let result = eval_builtin("asin", &[Value::Real(-2.0)]);
        assert!(
            result.is_undef(),
            "asin(-2.0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn acos_out_of_range_positive() {
        let result = eval_builtin("acos", &[Value::Real(2.0)]);
        assert!(
            result.is_undef(),
            "acos(2.0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn acos_out_of_range_negative() {
        let result = eval_builtin("acos", &[Value::Real(-2.0)]);
        assert!(
            result.is_undef(),
            "acos(-2.0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn sinh_overflow_returns_undef() {
        let result = eval_builtin("sinh", &[Value::Real(1000.0)]);
        assert!(
            result.is_undef(),
            "sinh(1000) should be Undef (inf), got {:?}",
            result
        );
    }

    #[test]
    fn cosh_overflow_returns_undef() {
        let result = eval_builtin("cosh", &[Value::Real(1000.0)]);
        assert!(
            result.is_undef(),
            "cosh(1000) should be Undef (inf), got {:?}",
            result
        );
    }

    // Boundary valid inputs: confirm no regressions on valid inputs

    #[test]
    fn asin_boundary_valid() {
        let result = eval_builtin("asin", &[Value::Real(1.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    #[test]
    fn acos_boundary_valid() {
        let result = eval_builtin("acos", &[Value::Real(-1.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::PI).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }
}
