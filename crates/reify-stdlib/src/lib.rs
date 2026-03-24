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
        "sqrt" => unary(args, |v| match v {
            Value::Scalar { si_value, dimension } => sanitize_value(Value::Scalar {
                si_value: si_value.sqrt(),
                dimension: dimension.root(2),
            }),
            _ => match v.as_f64() {
                Some(x) => sanitize_value(Value::Real(x.sqrt())),
                None => Value::Undef,
            },
        }),
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
            (Value::Scalar { si_value: x, dimension: d1 }, Value::Scalar { si_value: y, dimension: d2 })
                if d1 == d2 => Value::Scalar { si_value: x.min(*y), dimension: *d1 },
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
            (Value::Scalar { si_value: x, dimension: d1 }, Value::Scalar { si_value: y, dimension: d2 })
                if d1 == d2 => Value::Scalar { si_value: x.max(*y), dimension: *d1 },
            _ => {
                match (a.as_f64(), b.as_f64()) {
                    (Some(x), Some(y)) => Value::Real(x.max(y)),
                    _ => Value::Undef,
                }
            }
        }),
        "pow" => binary_f64(args, |x, y| Value::Real(x.powf(y))),

        // --- Three-arg numeric functions ---
        "clamp" => ternary(args, |x, lo, hi| match (x, lo, hi) {
            (Value::Int(x), Value::Int(lo), Value::Int(hi)) => Value::Int(*x.clamp(lo, hi)),
            (Value::Real(x), Value::Real(lo), Value::Real(hi)) => {
                sanitize_value(Value::Real(x.clamp(*lo, *hi)))
            }
            (
                Value::Scalar { si_value: x, dimension: d1 },
                Value::Scalar { si_value: lo, dimension: d2 },
                Value::Scalar { si_value: hi, dimension: d3 },
            ) if d1 == d2 && d2 == d3 => {
                sanitize_value(Value::Scalar { si_value: x.clamp(*lo, *hi), dimension: *d1 })
            }
            _ => {
                // Fallback: require all three to have the same dimension
                let dx = x.dimension();
                let dlo = lo.dimension();
                let dhi = hi.dimension();
                if dx != dlo || dlo != dhi {
                    return Value::Undef;
                }
                match (x.as_f64(), lo.as_f64(), hi.as_f64()) {
                    (Some(xv), Some(lov), Some(hiv)) => {
                        sanitize_value(Value::Real(xv.clamp(lov, hiv)))
                    }
                    _ => Value::Undef,
                }
            }
        }),

        "lerp" => ternary(args, |a, b, t| {
            // t must be dimensionless (pure scalar or Int/Real)
            if t.dimension() != DimensionVector::DIMENSIONLESS {
                return Value::Undef;
            }
            let Some(tv) = t.as_f64() else {
                return Value::Undef;
            };
            match (a, b) {
                (Value::Real(a), Value::Real(b)) => {
                    sanitize_value(Value::Real(a + tv * (b - a)))
                }
                (
                    Value::Scalar { si_value: av, dimension: da },
                    Value::Scalar { si_value: bv, dimension: db },
                ) if da == db => sanitize_value(Value::Scalar {
                    si_value: av + tv * (bv - av),
                    dimension: *da,
                }),
                _ => {
                    // Fallback: require a and b to have the same dimension
                    if a.dimension() != b.dimension() {
                        return Value::Undef;
                    }
                    match (a.as_f64(), b.as_f64()) {
                        (Some(av), Some(bv)) => sanitize_value(Value::Real(av + tv * (bv - av))),
                        _ => Value::Undef,
                    }
                }
            }
        }),

        // --- Integer functions ---
        "mod" => binary(args, |x, y| match (x, y) {
            (Value::Int(x), Value::Int(y)) if *y != 0 => Value::Int(x % y),
            _ => Value::Undef,
        }),

        "remap" => {
            // remap(x, from_lo, from_hi, to_lo, to_hi)
            if args.len() != 5 {
                return Value::Undef;
            }
            match (
                args[0].as_f64(),
                args[1].as_f64(),
                args[2].as_f64(),
                args[3].as_f64(),
                args[4].as_f64(),
            ) {
                (Some(x), Some(from_lo), Some(from_hi), Some(to_lo), Some(to_hi)) => {
                    let result = to_lo + (x - from_lo) * (to_hi - to_lo) / (from_hi - from_lo);
                    sanitize_value(Value::Real(result))
                }
                _ => Value::Undef,
            }
        }

        // --- Trig functions: accept Angle Scalar or bare Real (radians) ---
        "sin" => unary(args, |v| trig_input(v).map_or(Value::Undef, |r| Value::Real(r.sin()))),
        "cos" => unary(args, |v| trig_input(v).map_or(Value::Undef, |r| Value::Real(r.cos()))),
        "tan" => unary(args, |v| trig_input(v).map_or(Value::Undef, |r| Value::Real(r.tan()))),

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

        // --- Determinacy predicates (stubs) ---
        // These predicates inspect DeterminacyState which is tracked in the Engine's
        // snapshot, not in Value itself. Like sample(), the actual behavior is
        // intercepted at the eval layer (reify-expr/reify-eval) where snapshot state
        // is available. These stubs serve as documentation and fallback.
        "determined" => Value::Undef,
        "undetermined" => Value::Undef,
        "constrained" => Value::Undef,
        "partially_determined" => Value::Undef,

        // --- Field operations (stubs) ---
        // These are handled by reify-expr's eval_expr FunctionCall interceptor
        // for actual lambda application; the stdlib entries serve as documentation
        // and fallback for direct stdlib calls.
        "sample" => Value::Undef,     // Requires EvalContext for lambda application
        "gradient" => Value::Undef,   // Numeric differentiation not yet implemented
        "divergence" => Value::Undef, // Numeric differentiation not yet implemented
        "curl" => Value::Undef,       // Numeric differentiation not yet implemented

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

/// Convert non-finite f64 values (NaN, inf) to Undef.
///
/// This is a defense-in-depth catch-all applied at the return point of
/// `unary_f64` and `binary_f64` to ensure domain errors (e.g., sqrt(-1),
/// log(0), exp(1000) overflow) produce Undef instead of silently propagating
/// NaN or infinity through the evaluation graph.
fn sanitize_value(v: Value) -> Value {
    match &v {
        Value::Real(x) if x.is_nan() || x.is_infinite() => Value::Undef,
        Value::Scalar { si_value, .. } if si_value.is_nan() || si_value.is_infinite() => {
            Value::Undef
        }
        _ => v,
    }
}

/// Apply a function to a single f64 argument (extracted from any numeric Value).
fn unary_f64(args: &[Value], f: impl FnOnce(f64) -> Value) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    match args[0].as_f64() {
        Some(x) => sanitize_value(f(x)),
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

/// Extract radians from a trig function argument.
/// Accepts: Angle Scalar (si_value is already radians) or bare Real (treated as radians).
/// Rejects: non-ANGLE Scalar (dimension error).
fn trig_input(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, dimension } => {
            if *dimension == DimensionVector::ANGLE {
                Some(*si_value)
            } else {
                None // dimension error: sin(5mm) is meaningless
            }
        }
        Value::Real(r) => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Apply a function to three arguments (by reference).
fn ternary(args: &[Value], f: impl FnOnce(&Value, &Value, &Value) -> Value) -> Value {
    if args.len() != 3 {
        return Value::Undef;
    }
    f(&args[0], &args[1], &args[2])
}

/// Apply a function to two f64 arguments.
fn binary_f64(args: &[Value], f: impl FnOnce(f64, f64) -> Value) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    match (args[0].as_f64(), args[1].as_f64()) {
        (Some(x), Some(y)) => sanitize_value(f(x, y)),
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

    // --- Domain-error NaN/inf hardening tests (step-21) ---

    #[test]
    fn sqrt_negative_returns_undef() {
        let result = eval_builtin("sqrt", &[Value::Real(-1.0)]);
        assert!(result.is_undef(), "sqrt(-1) should be Undef, got {:?}", result);
    }

    #[test]
    fn log_zero_returns_undef() {
        let result = eval_builtin("log", &[Value::Real(0.0)]);
        assert!(result.is_undef(), "log(0) should be Undef, got {:?}", result);
    }

    #[test]
    fn log_negative_returns_undef() {
        let result = eval_builtin("log", &[Value::Real(-1.0)]);
        assert!(result.is_undef(), "log(-1) should be Undef, got {:?}", result);
    }

    #[test]
    fn log10_zero_returns_undef() {
        let result = eval_builtin("log10", &[Value::Real(0.0)]);
        assert!(result.is_undef(), "log10(0) should be Undef, got {:?}", result);
    }

    #[test]
    fn log10_negative_returns_undef() {
        let result = eval_builtin("log10", &[Value::Real(-1.0)]);
        assert!(result.is_undef(), "log10(-1) should be Undef, got {:?}", result);
    }

    #[test]
    fn exp_overflow_returns_undef() {
        let result = eval_builtin("exp", &[Value::Real(1000.0)]);
        assert!(result.is_undef(), "exp(1000) should be Undef (inf), got {:?}", result);
    }

    #[test]
    fn pow_negative_base_fractional_exp_returns_undef() {
        let result = eval_builtin("pow", &[Value::Real(-2.0), Value::Real(0.5)]);
        assert!(result.is_undef(), "pow(-2, 0.5) should be Undef (NaN), got {:?}", result);
    }

    // --- Inverse-trig domain errors and hyperbolic overflow (step-23) ---

    #[test]
    fn asin_out_of_range_positive() {
        let result = eval_builtin("asin", &[Value::Real(2.0)]);
        assert!(result.is_undef(), "asin(2.0) should be Undef, got {:?}", result);
    }

    #[test]
    fn asin_out_of_range_negative() {
        let result = eval_builtin("asin", &[Value::Real(-2.0)]);
        assert!(result.is_undef(), "asin(-2.0) should be Undef, got {:?}", result);
    }

    #[test]
    fn acos_out_of_range_positive() {
        let result = eval_builtin("acos", &[Value::Real(2.0)]);
        assert!(result.is_undef(), "acos(2.0) should be Undef, got {:?}", result);
    }

    #[test]
    fn acos_out_of_range_negative() {
        let result = eval_builtin("acos", &[Value::Real(-2.0)]);
        assert!(result.is_undef(), "acos(-2.0) should be Undef, got {:?}", result);
    }

    #[test]
    fn sinh_overflow_returns_undef() {
        let result = eval_builtin("sinh", &[Value::Real(1000.0)]);
        assert!(result.is_undef(), "sinh(1000) should be Undef (inf), got {:?}", result);
    }

    #[test]
    fn cosh_overflow_returns_undef() {
        let result = eval_builtin("cosh", &[Value::Real(1000.0)]);
        assert!(result.is_undef(), "cosh(1000) should be Undef (inf), got {:?}", result);
    }

    // Boundary valid inputs: confirm no regressions on valid inputs

    #[test]
    fn asin_boundary_valid() {
        let result = eval_builtin("asin", &[Value::Real(1.0)]);
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    // --- sqrt dimension-awareness tests (step-3, task 39) ---

    #[test]
    fn sqrt_scalar_area_to_length() {
        // sqrt(Scalar{4.0, AREA}) must return Scalar{2.0, LENGTH}
        let result = eval_builtin(
            "sqrt",
            &[Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::AREA,
            }],
        );
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 2.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar{{2.0, LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_scalar_length4_to_length2() {
        // sqrt(Scalar{9.0, LENGTH^4}) must return Scalar{3.0, LENGTH^2}
        let len4 = DimensionVector::LENGTH.pow(4);
        let result = eval_builtin(
            "sqrt",
            &[Value::Scalar {
                si_value: 9.0,
                dimension: len4,
            }],
        );
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 3.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::AREA); // LENGTH^2 == AREA
            }
            other => panic!("expected Scalar{{3.0, AREA}}, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_scalar_length_to_fractional_exponent() {
        use reify_types::Rational;
        // sqrt(Scalar{4.0, LENGTH}) must return Scalar{2.0, LENGTH^(1/2)}
        let result = eval_builtin(
            "sqrt",
            &[Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            }],
        );
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 2.0).abs() < 1e-12);
                assert_eq!(dimension.0[0], Rational::new(1, 2));
                for i in 1..9 {
                    assert!(dimension.0[i].is_zero());
                }
            }
            other => panic!("expected Scalar{{2.0, LENGTH^(1/2)}}, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_negative_scalar_returns_undef() {
        // sqrt of negative Scalar must return Undef (via sanitize_value catching NaN)
        let result = eval_builtin(
            "sqrt",
            &[Value::Scalar {
                si_value: -4.0,
                dimension: DimensionVector::AREA,
            }],
        );
        assert!(result.is_undef(), "sqrt of negative Scalar should be Undef, got {:?}", result);
    }

    #[test]
    fn acos_boundary_valid() {
        let result = eval_builtin("acos", &[Value::Real(-1.0)]);
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - std::f64::consts::PI).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    // --- Determinacy predicate stubs (step-7) ---

    #[test]
    fn determined_stub_returns_undef() {
        // determined() is handled at the eval layer where DeterminacyState is available.
        // The stdlib stub returns Undef as a fallback.
        let result = eval_builtin("determined", &[Value::Real(42.0)]);
        assert!(result.is_undef(), "determined stub should return Undef, got {:?}", result);
    }

    #[test]
    fn undetermined_stub_returns_undef() {
        let result = eval_builtin("undetermined", &[Value::Real(42.0)]);
        assert!(result.is_undef(), "undetermined stub should return Undef, got {:?}", result);
    }

    #[test]
    fn constrained_stub_returns_undef() {
        let result = eval_builtin("constrained", &[Value::Real(42.0)]);
        assert!(result.is_undef(), "constrained stub should return Undef, got {:?}", result);
    }

    #[test]
    fn partially_determined_stub_returns_undef() {
        let result = eval_builtin("partially_determined", &[Value::Real(42.0)]);
        assert!(result.is_undef(), "partially_determined stub should return Undef, got {:?}", result);
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
        assert!(result.is_undef(), "gradient stub should return Undef, got {:?}", result);
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
        assert!(result.is_undef(), "divergence stub should return Undef, got {:?}", result);
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
        assert!(result.is_undef(), "curl stub should return Undef, got {:?}", result);
    }

    // --- clamp tests (step-1) ---

    #[test]
    fn clamp_real_within_range() {
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0)]);
        match result {
            Value::Real(v) => assert!((v - 5.0).abs() < 1e-12),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    #[test]
    fn clamp_real_below_lo() {
        let result = eval_builtin("clamp", &[Value::Real(-1.0), Value::Real(0.0), Value::Real(10.0)]);
        match result {
            Value::Real(v) => assert!((v - 0.0).abs() < 1e-12),
            other => panic!("expected Real(0.0), got {:?}", other),
        }
    }

    #[test]
    fn clamp_real_above_hi() {
        let result = eval_builtin("clamp", &[Value::Real(15.0), Value::Real(0.0), Value::Real(10.0)]);
        match result {
            Value::Real(v) => assert!((v - 10.0).abs() < 1e-12),
            other => panic!("expected Real(10.0), got {:?}", other),
        }
    }

    #[test]
    fn clamp_at_lo_boundary() {
        let result = eval_builtin("clamp", &[Value::Real(0.0), Value::Real(0.0), Value::Real(10.0)]);
        match result {
            Value::Real(v) => assert!((v - 0.0).abs() < 1e-12),
            other => panic!("expected Real(0.0), got {:?}", other),
        }
    }

    #[test]
    fn clamp_at_hi_boundary() {
        let result = eval_builtin("clamp", &[Value::Real(10.0), Value::Real(0.0), Value::Real(10.0)]);
        match result {
            Value::Real(v) => assert!((v - 10.0).abs() < 1e-12),
            other => panic!("expected Real(10.0), got {:?}", other),
        }
    }

    #[test]
    fn clamp_int_preserves_type() {
        let result = eval_builtin("clamp", &[Value::Int(3), Value::Int(1), Value::Int(7)]);
        match result {
            Value::Int(3) => {}
            other => panic!("expected Int(3), got {:?}", other),
        }
    }

    #[test]
    fn clamp_scalar_preserves_dimension() {
        // 5mm, 0mm, 10mm in SI (m): 0.005, 0.0, 0.01
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar { si_value: 0.005, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.0,   dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.01,  dimension: DimensionVector::LENGTH },
            ],
        );
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 0.005).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar{{LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn clamp_dimension_mismatch_returns_undef() {
        // x in LENGTH, bounds in TIME — should return Undef
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar { si_value: 0.005, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.0,   dimension: DimensionVector::TIME },
                Value::Scalar { si_value: 10.0,  dimension: DimensionVector::TIME },
            ],
        );
        assert!(result.is_undef(), "clamp with dimension mismatch should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_nan_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[Value::Real(f64::NAN), Value::Real(0.0), Value::Real(10.0)],
        );
        assert!(result.is_undef(), "clamp(NaN,...) should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_wrong_arg_count_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Real(5.0)]);
        assert!(result.is_undef(), "clamp with wrong arg count should be Undef, got {:?}", result);
    }

    // --- mod tests (step-7) ---

    #[test]
    fn mod_basic() {
        let result = eval_builtin("mod", &[Value::Int(7), Value::Int(3)]);
        match result {
            Value::Int(1) => {}
            other => panic!("expected Int(1), got {:?}", other),
        }
    }

    #[test]
    fn mod_exact_division() {
        let result = eval_builtin("mod", &[Value::Int(10), Value::Int(5)]);
        match result {
            Value::Int(0) => {}
            other => panic!("expected Int(0), got {:?}", other),
        }
    }

    #[test]
    fn mod_negative_dividend() {
        // Rust remainder semantics: -7 % 3 == -1 (sign of dividend)
        let result = eval_builtin("mod", &[Value::Int(-7), Value::Int(3)]);
        match result {
            Value::Int(-1) => {}
            other => panic!("expected Int(-1), got {:?}", other),
        }
    }

    #[test]
    fn mod_negative_divisor() {
        // 7 % -3 == 1 in Rust (sign of dividend)
        let result = eval_builtin("mod", &[Value::Int(7), Value::Int(-3)]);
        match result {
            Value::Int(1) => {}
            other => panic!("expected Int(1), got {:?}", other),
        }
    }

    #[test]
    fn mod_by_zero_returns_undef() {
        let result = eval_builtin("mod", &[Value::Int(7), Value::Int(0)]);
        assert!(result.is_undef(), "mod(7,0) should be Undef, got {:?}", result);
    }

    #[test]
    fn mod_non_int_returns_undef() {
        // mod is strictly Int — Real args return Undef
        let result = eval_builtin("mod", &[Value::Real(7.0), Value::Real(3.0)]);
        assert!(result.is_undef(), "mod(7.0, 3.0) should be Undef, got {:?}", result);
    }

    #[test]
    fn mod_wrong_arg_count_returns_undef() {
        let result = eval_builtin("mod", &[Value::Int(7)]);
        assert!(result.is_undef(), "mod with wrong arg count should be Undef, got {:?}", result);
    }

    // --- remap tests (step-5) ---

    #[test]
    fn remap_midpoint() {
        // remap(5, 0→10, 0→100) == 50
        let result = eval_builtin(
            "remap",
            &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)],
        );
        match result {
            Value::Real(v) => assert!((v - 50.0).abs() < 1e-12),
            other => panic!("expected Real(50.0), got {:?}", other),
        }
    }

    #[test]
    fn remap_at_from_lo() {
        let result = eval_builtin(
            "remap",
            &[Value::Real(0.0), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)],
        );
        match result {
            Value::Real(v) => assert!((v - 0.0).abs() < 1e-12),
            other => panic!("expected Real(0.0), got {:?}", other),
        }
    }

    #[test]
    fn remap_at_from_hi() {
        let result = eval_builtin(
            "remap",
            &[Value::Real(10.0), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)],
        );
        match result {
            Value::Real(v) => assert!((v - 100.0).abs() < 1e-12),
            other => panic!("expected Real(100.0), got {:?}", other),
        }
    }

    #[test]
    fn remap_extrapolation() {
        // x=15 beyond [0,10] → 150 (extrapolation allowed)
        let result = eval_builtin(
            "remap",
            &[Value::Real(15.0), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)],
        );
        match result {
            Value::Real(v) => assert!((v - 150.0).abs() < 1e-12),
            other => panic!("expected Real(150.0), got {:?}", other),
        }
    }

    #[test]
    fn remap_inverse() {
        // remap(50, 0→100, 0→10) == 5
        let result = eval_builtin(
            "remap",
            &[Value::Real(50.0), Value::Real(0.0), Value::Real(100.0), Value::Real(0.0), Value::Real(10.0)],
        );
        match result {
            Value::Real(v) => assert!((v - 5.0).abs() < 1e-12),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    #[test]
    fn remap_division_by_zero_returns_undef() {
        // from_lo == from_hi → division by zero
        let result = eval_builtin(
            "remap",
            &[Value::Real(5.0), Value::Real(5.0), Value::Real(5.0), Value::Real(0.0), Value::Real(100.0)],
        );
        assert!(result.is_undef(), "remap with from_lo==from_hi should be Undef, got {:?}", result);
    }

    #[test]
    fn remap_nan_returns_undef() {
        let result = eval_builtin(
            "remap",
            &[Value::Real(f64::NAN), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)],
        );
        assert!(result.is_undef(), "remap(NaN,...) should be Undef, got {:?}", result);
    }

    #[test]
    fn remap_wrong_arg_count_returns_undef() {
        let result = eval_builtin("remap", &[Value::Real(5.0), Value::Real(0.0)]);
        assert!(result.is_undef(), "remap with wrong arg count should be Undef, got {:?}", result);
    }

    // --- lerp tests (step-3) ---

    #[test]
    fn lerp_midpoint() {
        let result = eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(0.5)]);
        match result {
            Value::Real(v) => assert!((v - 5.0).abs() < 1e-12),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    #[test]
    fn lerp_t_zero() {
        let result = eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(0.0)]);
        match result {
            Value::Real(v) => assert!((v - 0.0).abs() < 1e-12),
            other => panic!("expected Real(0.0), got {:?}", other),
        }
    }

    #[test]
    fn lerp_t_one() {
        let result = eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(1.0)]);
        match result {
            Value::Real(v) => assert!((v - 10.0).abs() < 1e-12),
            other => panic!("expected Real(10.0), got {:?}", other),
        }
    }

    #[test]
    fn lerp_negative_t_extrapolation() {
        // lerp with t=-0.5 extrapolates below a
        let result = eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(-0.5)]);
        match result {
            Value::Real(v) => assert!((v - (-5.0)).abs() < 1e-12),
            other => panic!("expected Real(-5.0), got {:?}", other),
        }
    }

    #[test]
    fn lerp_scalar_preserves_dimension() {
        // lerp(2mm, 8mm, 0.5) → 5mm  (SI: 0.002, 0.008, 0.005)
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar { si_value: 0.002, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.008, dimension: DimensionVector::LENGTH },
                Value::Real(0.5),
            ],
        );
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 0.005).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar{{LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn lerp_dimension_mismatch_a_b_returns_undef() {
        // a=2mm, b=8s, t=0.5 — mismatched dimensions
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar { si_value: 0.002, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 8.0,   dimension: DimensionVector::TIME },
                Value::Real(0.5),
            ],
        );
        assert!(result.is_undef(), "lerp with mismatched a/b dimensions should be Undef, got {:?}", result);
    }

    #[test]
    fn lerp_t_dimensioned_returns_undef() {
        // t must be dimensionless; lerp(0.0, 10.0, 5mm) is semantically wrong
        let result = eval_builtin(
            "lerp",
            &[
                Value::Real(0.0),
                Value::Real(10.0),
                Value::Scalar { si_value: 0.005, dimension: DimensionVector::LENGTH },
            ],
        );
        assert!(result.is_undef(), "lerp with dimensioned t should be Undef, got {:?}", result);
    }

    #[test]
    fn lerp_nan_returns_undef() {
        let result = eval_builtin(
            "lerp",
            &[Value::Real(0.0), Value::Real(10.0), Value::Real(f64::NAN)],
        );
        assert!(result.is_undef(), "lerp with NaN t should be Undef, got {:?}", result);
    }

    #[test]
    fn lerp_wrong_arg_count_returns_undef() {
        let result = eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0)]);
        assert!(result.is_undef(), "lerp with wrong arg count should be Undef, got {:?}", result);
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
        assert!(result.is_undef(), "sample in stdlib should return Undef (handled in eval_expr), got {:?}", result);
    }
}
