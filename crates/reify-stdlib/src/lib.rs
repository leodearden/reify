use reify_types::Value;

/// Evaluate a built-in stdlib function by name.
///
/// Returns `Value::Undef` for unknown functions or wrong argument types/counts.
pub fn eval_builtin(_name: &str, _args: &[Value]) -> Value {
    // Stub — implementation in step-2
    Value::Undef
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::DimensionVector;

    #[test]
    fn abs_real_negative() {
        let result = eval_builtin("abs", &[Value::Real(-5.0)]);
        match result {
            Value::Real(v) => assert!((v - 5.0).abs() < 1e-12),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    #[test]
    fn abs_int_negative() {
        let result = eval_builtin("abs", &[Value::Int(-3)]);
        match result {
            Value::Int(3) => {}
            other => panic!("expected Int(3), got {:?}", other),
        }
    }

    #[test]
    fn abs_scalar_preserves_dimension() {
        let result = eval_builtin(
            "abs",
            &[Value::Scalar {
                si_value: -0.005,
                dimension: DimensionVector::LENGTH,
            }],
        );
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 0.005).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_real() {
        let result = eval_builtin("sqrt", &[Value::Real(9.0)]);
        match result {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12),
            other => panic!("expected Real(3.0), got {:?}", other),
        }
    }

    #[test]
    fn min_real() {
        let result = eval_builtin("min", &[Value::Real(3.0), Value::Real(7.0)]);
        match result {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12),
            other => panic!("expected Real(3.0), got {:?}", other),
        }
    }

    #[test]
    fn max_int() {
        let result = eval_builtin("max", &[Value::Int(3), Value::Int(7)]);
        match result {
            Value::Int(7) => {}
            other => panic!("expected Int(7), got {:?}", other),
        }
    }

    #[test]
    fn floor_real() {
        let result = eval_builtin("floor", &[Value::Real(3.7)]);
        match result {
            Value::Int(3) => {}
            other => panic!("expected Int(3), got {:?}", other),
        }
    }

    #[test]
    fn ceil_real() {
        let result = eval_builtin("ceil", &[Value::Real(3.2)]);
        match result {
            Value::Int(4) => {}
            other => panic!("expected Int(4), got {:?}", other),
        }
    }

    #[test]
    fn round_real() {
        let result = eval_builtin("round", &[Value::Real(3.5)]);
        match result {
            Value::Int(4) => {}
            other => panic!("expected Int(4), got {:?}", other),
        }
    }

    #[test]
    fn log_e() {
        let result = eval_builtin("log", &[Value::Real(std::f64::consts::E)]);
        match result {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-12),
            other => panic!("expected Real(~1.0), got {:?}", other),
        }
    }

    #[test]
    fn exp_zero() {
        let result = eval_builtin("exp", &[Value::Real(0.0)]);
        match result {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-12),
            other => panic!("expected Real(1.0), got {:?}", other),
        }
    }

    #[test]
    fn sign_negative() {
        let result = eval_builtin("sign", &[Value::Real(-5.0)]);
        match result {
            Value::Real(v) => assert!((v - (-1.0)).abs() < 1e-12),
            other => panic!("expected Real(-1.0), got {:?}", other),
        }
    }

    #[test]
    fn unknown_function_returns_undef() {
        let result = eval_builtin("foo", &[Value::Real(1.0)]);
        assert!(result.is_undef());
    }
}
