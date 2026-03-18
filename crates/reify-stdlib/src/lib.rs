use reify_types::{DimensionVector, Value};

/// Evaluate a built-in stdlib function by name.
///
/// Returns `Value::Undef` for unknown functions or wrong argument types/counts.
pub fn eval_builtin(name: &str, args: &[Value]) -> Value {
    match name {
        // --- Single-arg numeric functions ---
        "abs" => unary(args, |v| match v {
            Value::Int(i) => Value::Int(i.abs()),
            Value::Real(r) => Value::Real(r.abs()),
            Value::Scalar { si_value, dimension } => Value::Scalar {
                si_value: si_value.abs(),
                dimension: *dimension,
            },
            _ => Value::Undef,
        }),
        "sqrt" => unary_f64(args, |x| Value::Real(x.sqrt())),
        "floor" => unary_f64(args, |x| Value::Int(x.floor() as i64)),
        "ceil" => unary_f64(args, |x| Value::Int(x.ceil() as i64)),
        "round" => unary_f64(args, |x| Value::Int(x.round() as i64)),
        "sign" => unary_f64(args, |x| Value::Real(x.signum())),
        "log" => unary_f64(args, |x| Value::Real(x.ln())),
        "log10" => unary_f64(args, |x| Value::Real(x.log10())),
        "exp" => unary_f64(args, |x| Value::Real(x.exp())),

        // --- Two-arg numeric functions ---
        "min" => binary(args, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(*x.min(y)),
            (Value::Real(x), Value::Real(y)) => Value::Real(x.min(*y)),
            _ => {
                match (a.as_f64(), b.as_f64()) {
                    (Some(x), Some(y)) => Value::Real(x.min(y)),
                    _ => Value::Undef,
                }
            }
        }),
        "max" => binary(args, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(*x.max(y)),
            (Value::Real(x), Value::Real(y)) => Value::Real(x.max(*y)),
            _ => {
                match (a.as_f64(), b.as_f64()) {
                    (Some(x), Some(y)) => Value::Real(x.max(y)),
                    _ => Value::Undef,
                }
            }
        }),
        "pow" => binary_f64(args, |x, y| Value::Real(x.powf(y))),

        // --- Trig functions (implemented in step-4) ---
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "atan2"
        | "sinh" | "cosh" | "tanh" => Value::Undef, // placeholder for step-4

        _ => Value::Undef,
    }
}

/// Apply a function to a single argument (by reference, for pattern matching).
fn unary(args: &[Value], f: impl FnOnce(&Value) -> Value) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    f(&args[0])
}

/// Apply a function to a single f64 argument (extracted from any numeric Value).
fn unary_f64(args: &[Value], f: impl FnOnce(f64) -> Value) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    match args[0].as_f64() {
        Some(x) => f(x),
        None => Value::Undef,
    }
}

/// Apply a function to two arguments (by reference).
fn binary(args: &[Value], f: impl FnOnce(&Value, &Value) -> Value) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    f(&args[0], &args[1])
}

/// Apply a function to two f64 arguments.
fn binary_f64(args: &[Value], f: impl FnOnce(f64, f64) -> Value) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    match (args[0].as_f64(), args[1].as_f64()) {
        (Some(x), Some(y)) => f(x, y),
        _ => Value::Undef,
    }
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
