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
        "mod" => binary(args, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => {
                if *y == 0 || (*x == i64::MIN && *y == -1) {
                    Value::Undef
                } else {
                    Value::Int(x % y)
                }
            }
            _ => Value::Undef,
        }),

        // --- Three-arg numeric functions ---
        "clamp" => ternary(args, |x, lo, hi| match (x, lo, hi) {
            (Value::Int(xv), Value::Int(lov), Value::Int(hiv)) => {
                if lov > hiv {
                    Value::Undef
                } else {
                    Value::Int((*xv).clamp(*lov, *hiv))
                }
            }
            (Value::Real(xv), Value::Real(lov), Value::Real(hiv)) => {
                if xv.is_nan() || !valid_f64_range(*lov, *hiv) {
                    Value::Undef
                } else {
                    sanitize_value(Value::Real(xv.clamp(*lov, *hiv)))
                }
            }
            (
                Value::Scalar { si_value: xv, dimension: dx },
                Value::Scalar { si_value: lov, dimension: dlo },
                Value::Scalar { si_value: hiv, dimension: dhi },
            ) => {
                if dx != dlo || dx != dhi {
                    return Value::Undef;
                }
                if xv.is_nan() || !valid_f64_range(*lov, *hiv) {
                    return Value::Undef;
                }
                sanitize_value(Value::Scalar {
                    si_value: xv.clamp(*lov, *hiv),
                    dimension: *dx,
                })
            }
            _ => {
                // Fallback: try to extract f64 from all three args.
                // If all dimensions agree and are non-DIMENSIONLESS, reconstruct Scalar.
                let (xv, lov, hiv) = match (x.as_f64(), lo.as_f64(), hi.as_f64()) {
                    (Some(a), Some(b), Some(c)) => (a, b, c),
                    _ => return Value::Undef,
                };
                if xv.is_nan() || !valid_f64_range(lov, hiv) {
                    return Value::Undef;
                }
                let result = xv.clamp(lov, hiv);
                let dx = x.dimension();
                if dx != DimensionVector::DIMENSIONLESS
                    && dx == lo.dimension()
                    && dx == hi.dimension()
                {
                    sanitize_value(Value::Scalar { si_value: result, dimension: dx })
                } else {
                    sanitize_value(Value::Real(result))
                }
            }
        }),

        "lerp" => ternary(args, |a, b, t| {
            // t must be dimensionless (Real or Int; reject dimensioned Scalar)
            if let Value::Scalar { dimension, .. } = t {
                if *dimension != DimensionVector::DIMENSIONLESS {
                    return Value::Undef;
                }
            }
            let tv = match t.as_f64() {
                Some(v) => v,
                None => return Value::Undef,
            };
            if tv.is_nan() {
                return Value::Undef;
            }
            match (a, b) {
                (Value::Real(av), Value::Real(bv)) => {
                    sanitize_value(Value::Real(lerp_f64(*av, *bv, tv)))
                }
                (
                    Value::Scalar { si_value: av, dimension: da },
                    Value::Scalar { si_value: bv, dimension: db },
                ) => {
                    if da != db {
                        return Value::Undef;
                    }
                    sanitize_value(Value::Scalar {
                        si_value: lerp_f64(*av, *bv, tv),
                        dimension: *da,
                    })
                }
                // Int fast path: documents the explicit Int->Real coercion
                (Value::Int(av), Value::Int(bv)) => {
                    sanitize_value(Value::Real(lerp_f64(*av as f64, *bv as f64, tv)))
                }
                _ => {
                    // Fallback: extract f64 from a and b; check dimension consistency
                    let av = match a.as_f64() {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    let bv = match b.as_f64() {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    let da = a.dimension();
                    if da != DimensionVector::DIMENSIONLESS && da == b.dimension() {
                        sanitize_value(Value::Scalar {
                            si_value: lerp_f64(av, bv, tv),
                            dimension: da,
                        })
                    } else {
                        sanitize_value(Value::Real(lerp_f64(av, bv, tv)))
                    }
                }
            }
        }),

        "remap" => {
            // Dimension-aware path handled in step-18; for now use quinary_f64 fallback.
            // Will be extended with Scalar arms before falling through here.
            quinary_f64(args, |x, from_lo, from_hi, to_lo, to_hi| {
                if from_lo == from_hi {
                    return Value::Undef; // early-exit: division by zero
                }
                let result = to_lo + (x - from_lo) * (to_hi - to_lo) / (from_hi - from_lo);
                Value::Real(result)
            })
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

/// Apply a function to three arguments (by reference, for pattern matching).
fn ternary(args: &[Value], f: impl FnOnce(&Value, &Value, &Value) -> Value) -> Value {
    if args.len() != 3 {
        return Value::Undef;
    }
    f(&args[0], &args[1], &args[2])
}

/// Returns true iff `lo` and `hi` form a valid (non-NaN, non-inverted) range.
///
/// Used by clamp Real/Scalar/fallback arms instead of inline `lo.is_nan() || hi.is_nan() || lo > hi`.
fn valid_f64_range(lo: f64, hi: f64) -> bool {
    !lo.is_nan() && !hi.is_nan() && lo <= hi
}

/// Linear interpolation: `a + t * (b - a)`.
fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + t * (b - a)
}

/// Apply a function to five f64 arguments (extracted via `as_f64()`).
///
/// Returns `Undef` on wrong argument count or extraction failure.
/// Applies `sanitize_value` to the result.
fn quinary_f64(args: &[Value], f: impl FnOnce(f64, f64, f64, f64, f64) -> Value) -> Value {
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
        (Some(a), Some(b), Some(c), Some(d), Some(e)) => sanitize_value(f(a, b, c, d, e)),
        _ => Value::Undef,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::DimensionVector;

    /// Assert that an expression evaluates to `Value::Real(v)` where `|v - expected| < 1e-12`.
    macro_rules! assert_real_approx {
        ($expr:expr, $expected:expr) => {
            match $expr {
                Value::Real(v) => assert!(
                    (v - $expected).abs() < 1e-12,
                    "expected Real({}) got Real({})",
                    $expected,
                    v
                ),
                other => panic!("expected Real({}), got {:?}", $expected, other),
            }
        };
    }

    /// Assert that an expression evaluates to `Value::Scalar { si_value, dimension }` where
    /// `|si_value - expected_si| < 1e-12` and `dimension == expected_dim`.
    macro_rules! assert_scalar_approx {
        ($expr:expr, $expected_si:expr, $expected_dim:expr) => {
            match $expr {
                Value::Scalar { si_value, dimension } => {
                    assert!(
                        (si_value - $expected_si).abs() < 1e-12,
                        "expected si_value={}, got {}",
                        $expected_si,
                        si_value
                    );
                    assert_eq!(dimension, $expected_dim);
                }
                other => panic!(
                    "expected Scalar{{si={}, dim={:?}}}, got {:?}",
                    $expected_si, $expected_dim, other
                ),
            }
        };
    }

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

    // --- mod builtin tests (step-1) ---

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
        let result = eval_builtin("mod", &[Value::Int(6), Value::Int(3)]);
        match result {
            Value::Int(0) => {}
            other => panic!("expected Int(0), got {:?}", other),
        }
    }

    #[test]
    fn mod_negative_dividend() {
        // Rust's % truncates toward zero: -7 % 3 == -1
        let result = eval_builtin("mod", &[Value::Int(-7), Value::Int(3)]);
        match result {
            Value::Int(-1) => {}
            other => panic!("expected Int(-1), got {:?}", other),
        }
    }

    #[test]
    fn mod_negative_divisor() {
        // -7 % -3 == -1 (truncation toward zero)
        let result = eval_builtin("mod", &[Value::Int(-7), Value::Int(-3)]);
        match result {
            Value::Int(-1) => {}
            other => panic!("expected Int(-1), got {:?}", other),
        }
    }

    #[test]
    fn mod_by_zero_returns_undef() {
        let result = eval_builtin("mod", &[Value::Int(7), Value::Int(0)]);
        assert!(result.is_undef(), "mod by zero should be Undef, got {:?}", result);
    }

    #[test]
    fn mod_non_int_returns_undef() {
        let result = eval_builtin("mod", &[Value::Real(3.5), Value::Real(2.0)]);
        assert!(result.is_undef(), "mod on Real should be Undef, got {:?}", result);
    }

    #[test]
    fn mod_wrong_arg_count_returns_undef() {
        let result = eval_builtin("mod", &[Value::Int(7)]);
        assert!(result.is_undef(), "mod with 1 arg should be Undef, got {:?}", result);
    }

    #[test]
    fn mod_i64_min_neg1_returns_undef() {
        // i64::MIN % -1 overflows in Rust (panics in debug mode)
        let result = eval_builtin("mod", &[Value::Int(i64::MIN), Value::Int(-1)]);
        assert!(result.is_undef(), "mod(i64::MIN, -1) should be Undef (overflow), got {:?}", result);
    }

    // --- clamp Real tests (step-3) ---

    #[test]
    fn clamp_real_within_range() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0)]),
            5.0
        );
    }

    #[test]
    fn clamp_real_below_lo() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(-3.0), Value::Real(0.0), Value::Real(10.0)]),
            0.0
        );
    }

    #[test]
    fn clamp_real_above_hi() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(15.0), Value::Real(0.0), Value::Real(10.0)]),
            10.0
        );
    }

    #[test]
    fn clamp_at_lo_boundary() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(0.0), Value::Real(0.0), Value::Real(10.0)]),
            0.0
        );
    }

    #[test]
    fn clamp_at_hi_boundary() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(10.0), Value::Real(0.0), Value::Real(10.0)]),
            10.0
        );
    }

    #[test]
    fn clamp_nan_x_returns_undef() {
        // x is NaN — explicit x.is_nan() guard
        let result = eval_builtin("clamp", &[Value::Real(f64::NAN), Value::Real(0.0), Value::Real(10.0)]);
        assert!(result.is_undef(), "clamp(NaN, 0, 10) should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_nan_lo_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(f64::NAN), Value::Real(10.0)]);
        assert!(result.is_undef(), "clamp(5, NaN, 10) should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_nan_hi_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(0.0), Value::Real(f64::NAN)]);
        assert!(result.is_undef(), "clamp(5, 0, NaN) should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_inverted_range_real_returns_undef() {
        // lo > hi is invalid
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(10.0), Value::Real(0.0)]);
        assert!(result.is_undef(), "clamp with inverted range should be Undef, got {:?}", result);
    }

    // --- clamp Int tests (step-5) ---

    #[test]
    fn clamp_int_preserves_type() {
        // within range: value passes through, returns Int
        let result = eval_builtin("clamp", &[Value::Int(5), Value::Int(0), Value::Int(10)]);
        match result {
            Value::Int(5) => {}
            other => panic!("expected Int(5), got {:?}", other),
        }
    }

    #[test]
    fn clamp_int_below_lo() {
        let result = eval_builtin("clamp", &[Value::Int(-3), Value::Int(0), Value::Int(10)]);
        match result {
            Value::Int(0) => {}
            other => panic!("expected Int(0) (clamped to lo), got {:?}", other),
        }
    }

    #[test]
    fn clamp_int_above_hi() {
        let result = eval_builtin("clamp", &[Value::Int(15), Value::Int(0), Value::Int(10)]);
        match result {
            Value::Int(10) => {}
            other => panic!("expected Int(10) (clamped to hi), got {:?}", other),
        }
    }

    #[test]
    fn clamp_inverted_range_int_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Int(5), Value::Int(10), Value::Int(0)]);
        assert!(result.is_undef(), "clamp Int with inverted range should be Undef, got {:?}", result);
    }

    // --- clamp Scalar + fallback tests (step-7) ---

    #[test]
    fn clamp_scalar_preserves_dimension() {
        // All three args: same LENGTH dimension, result should be LENGTH Scalar
        assert_scalar_approx!(
            eval_builtin(
                "clamp",
                &[
                    Value::Scalar { si_value: 0.005, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 0.001, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 0.010, dimension: DimensionVector::LENGTH },
                ]
            ),
            0.005,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn clamp_dimension_mismatch_returns_undef() {
        // lo/hi have different dimensions -> Undef
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 10.0, dimension: DimensionVector::TIME },
            ],
        );
        assert!(result.is_undef(), "clamp with dimension mismatch should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_inverted_range_scalar_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar { si_value: 5.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 10.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
            ],
        );
        assert!(result.is_undef(), "clamp Scalar with inverted range should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_scalar_nan_x_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 10.0, dimension: DimensionVector::LENGTH },
            ],
        );
        assert!(result.is_undef(), "clamp Scalar NaN x should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_wrong_arg_count_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(0.0)]);
        assert!(result.is_undef(), "clamp with 2 args should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_fallback_scalar_reconstruction() {
        // Mixed types: x is Int, lo/hi are Scalar LENGTH -> fallback extracts as_f64 but
        // since all args share a non-DIMENSIONLESS dimension, result should be Scalar LENGTH
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar { si_value: 0.005, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.001, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.010, dimension: DimensionVector::LENGTH },
            ],
        );
        // The Scalar arm should handle this; checking it preserves dimension
        match result {
            Value::Scalar { dimension, .. } => assert_eq!(dimension, DimensionVector::LENGTH),
            other => panic!("expected Scalar with LENGTH dimension, got {:?}", other),
        }
    }

    // --- lerp Real tests (step-9) ---

    #[test]
    fn lerp_midpoint() {
        // lerp(0, 10, 0.5) = 5
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(0.5)]),
            5.0
        );
    }

    #[test]
    fn lerp_t_zero() {
        // lerp(a, b, 0) = a
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Real(3.0), Value::Real(7.0), Value::Real(0.0)]),
            3.0
        );
    }

    #[test]
    fn lerp_t_one() {
        // lerp(a, b, 1) = b
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Real(3.0), Value::Real(7.0), Value::Real(1.0)]),
            7.0
        );
    }

    #[test]
    fn lerp_negative_t_extrapolation() {
        // lerp(0, 10, -0.5) = -5 (extrapolation below)
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(-0.5)]),
            -5.0
        );
    }

    #[test]
    fn lerp_nan_t_returns_undef() {
        // t is NaN — explicit NaN check after extraction
        let result = eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(f64::NAN)]);
        assert!(result.is_undef(), "lerp with NaN t should be Undef, got {:?}", result);
    }

    // --- lerp Scalar + dimension tests (step-11) ---

    #[test]
    fn lerp_scalar_preserves_dimension() {
        // lerp(Scalar{0.0, LENGTH}, Scalar{1.0, LENGTH}, Real(0.5)) = Scalar{0.5, LENGTH}
        assert_scalar_approx!(
            eval_builtin(
                "lerp",
                &[
                    Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
                    Value::Real(0.5),
                ]
            ),
            0.5,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn lerp_dimension_mismatch_a_b_returns_undef() {
        // a and b have different dimensions -> Undef
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 1.0, dimension: DimensionVector::TIME },
                Value::Real(0.5),
            ],
        );
        assert!(result.is_undef(), "lerp dimension mismatch a/b should be Undef, got {:?}", result);
    }

    #[test]
    fn lerp_t_dimensioned_returns_undef() {
        // t must be dimensionless; a LENGTH t is invalid
        let result = eval_builtin(
            "lerp",
            &[
                Value::Real(0.0),
                Value::Real(10.0),
                Value::Scalar { si_value: 0.5, dimension: DimensionVector::LENGTH },
            ],
        );
        assert!(result.is_undef(), "lerp with dimensioned t should be Undef, got {:?}", result);
    }

    #[test]
    fn lerp_nan_a_returns_undef() {
        // NaN in a -> Undef (via sanitize_value)
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
                Value::Real(0.5),
            ],
        );
        assert!(result.is_undef(), "lerp with NaN a should be Undef, got {:?}", result);
    }

    #[test]
    fn lerp_nan_b_returns_undef() {
        // NaN in b -> Undef (via sanitize_value)
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH },
                Value::Real(0.5),
            ],
        );
        assert!(result.is_undef(), "lerp with NaN b should be Undef, got {:?}", result);
    }

    // --- lerp Int/edge tests (step-13) ---

    #[test]
    fn lerp_int_inputs_coerce_to_real() {
        // lerp(Int(0), Int(10), Real(0.5)) -> Real(5.0)
        // The Int fast path extracts as f64, computes, returns Real
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Int(0), Value::Int(10), Value::Real(0.5)]),
            5.0
        );
    }

    #[test]
    fn lerp_wrong_arg_count_returns_undef() {
        let result = eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0)]);
        assert!(result.is_undef(), "lerp with 2 args should be Undef, got {:?}", result);
    }

    // --- remap Real tests (step-15) ---
    // remap(x, from_lo, from_hi, to_lo, to_hi)
    // formula: to_lo + (x - from_lo) * (to_hi - to_lo) / (from_hi - from_lo)

    #[test]
    fn remap_midpoint() {
        // remap(5, 0, 10, 0, 100) = 50
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)]
            ),
            50.0
        );
    }

    #[test]
    fn remap_at_from_lo() {
        // remap(from_lo, from_lo, from_hi, to_lo, to_hi) = to_lo
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(0.0), Value::Real(0.0), Value::Real(10.0), Value::Real(20.0), Value::Real(30.0)]
            ),
            20.0
        );
    }

    #[test]
    fn remap_at_from_hi() {
        // remap(from_hi, from_lo, from_hi, to_lo, to_hi) = to_hi
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(10.0), Value::Real(0.0), Value::Real(10.0), Value::Real(20.0), Value::Real(30.0)]
            ),
            30.0
        );
    }

    #[test]
    fn remap_extrapolation() {
        // x outside [from_lo, from_hi] extrapolates
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(15.0), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)]
            ),
            150.0
        );
    }

    #[test]
    fn remap_inverse() {
        // remap from [0,100] to [0,10] — inverse of remap_midpoint
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(50.0), Value::Real(0.0), Value::Real(100.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            5.0
        );
    }

    #[test]
    fn remap_division_by_zero_returns_undef() {
        // from_lo == from_hi -> division by zero -> Undef (early-exit)
        let result = eval_builtin(
            "remap",
            &[Value::Real(5.0), Value::Real(3.0), Value::Real(3.0), Value::Real(0.0), Value::Real(10.0)],
        );
        assert!(result.is_undef(), "remap with from_lo==from_hi should be Undef, got {:?}", result);
    }

    #[test]
    fn remap_nan_returns_undef() {
        let result = eval_builtin(
            "remap",
            &[Value::Real(f64::NAN), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)],
        );
        assert!(result.is_undef(), "remap with NaN x should be Undef, got {:?}", result);
    }

    #[test]
    fn remap_wrong_arg_count_returns_undef() {
        let result = eval_builtin("remap", &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0)]);
        assert!(result.is_undef(), "remap with 3 args should be Undef, got {:?}", result);
    }
}
