use reify_core::DimensionVector;
use reify_ir::Value;

use crate::helpers::{binary, quinary_f64, sanitize_value, ternary, unary, unary_f64};

pub(crate) fn eval_numeric(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        // --- Single-arg numeric functions ---
        "abs" => unary(args, |v| match v {
            Value::Int(i) => Value::Int(i.abs()),
            Value::Real(r) => Value::Real(r.abs()),
            Value::Scalar {
                si_value,
                dimension,
            } => Value::Scalar {
                si_value: si_value.abs(),
                dimension: *dimension,
            },
            _ => Value::Undef,
        }),
        "sqrt" => unary(args, |v| match v {
            Value::Scalar {
                si_value,
                dimension,
            } => sanitize_value(Value::Scalar {
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
            (
                Value::Scalar {
                    si_value: x,
                    dimension: d1,
                },
                Value::Scalar {
                    si_value: y,
                    dimension: d2,
                },
            ) if d1 == d2 => Value::Scalar {
                si_value: x.min(*y),
                dimension: *d1,
            },
            _ => match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => Value::Real(x.min(y)),
                _ => Value::Undef,
            },
        }),
        "max" => binary(args, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(*x.max(y)),
            (Value::Real(x), Value::Real(y)) => Value::Real(x.max(*y)),
            (
                Value::Scalar {
                    si_value: x,
                    dimension: d1,
                },
                Value::Scalar {
                    si_value: y,
                    dimension: d2,
                },
            ) if d1 == d2 => Value::Scalar {
                si_value: x.max(*y),
                dimension: *d1,
            },
            _ => match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => Value::Real(x.max(y)),
                _ => Value::Undef,
            },
        }),
        "pow" => crate::helpers::binary_f64(args, |x, y| Value::Real(x.powf(y))),
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
                Value::Scalar {
                    si_value: xv,
                    dimension: dx,
                },
                Value::Scalar {
                    si_value: lov,
                    dimension: dlo,
                },
                Value::Scalar {
                    si_value: hiv,
                    dimension: dhi,
                },
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
                if x.dimension() != DimensionVector::DIMENSIONLESS
                    || lo.dimension() != DimensionVector::DIMENSIONLESS
                    || hi.dimension() != DimensionVector::DIMENSIONLESS
                {
                    return Value::Undef;
                }
                let (xv, lov, hiv) = match (x.as_f64(), lo.as_f64(), hi.as_f64()) {
                    (Some(a), Some(b), Some(c)) => (a, b, c),
                    _ => return Value::Undef,
                };
                if xv.is_nan() || !valid_f64_range(lov, hiv) {
                    return Value::Undef;
                }
                sanitize_value(Value::Real(xv.clamp(lov, hiv)))
            }
        }),

        "lerp" => ternary(args, |a, b, t| {
            // t must be dimensionless (Real or Int; reject dimensioned Scalar)
            if let Value::Scalar { dimension, .. } = t
                && *dimension != DimensionVector::DIMENSIONLESS
            {
                return Value::Undef;
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
                    Value::Scalar {
                        si_value: av,
                        dimension: da,
                    },
                    Value::Scalar {
                        si_value: bv,
                        dimension: db,
                    },
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
                    if a.dimension() != DimensionVector::DIMENSIONLESS
                        || b.dimension() != DimensionVector::DIMENSIONLESS
                    {
                        return Value::Undef;
                    }
                    let av = match a.as_f64() {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    let bv = match b.as_f64() {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    sanitize_value(Value::Real(lerp_f64(av, bv, tv)))
                }
            }
        }),

        "remap" => {
            if args.len() != 5 {
                return Some(Value::Undef);
            }
            let (x, from_lo, from_hi, to_lo, to_hi) =
                (&args[0], &args[1], &args[2], &args[3], &args[4]);

            // Dimension-aware path: activate when any arg is a Scalar
            let any_scalar = args.iter().any(|a| matches!(a, Value::Scalar { .. }));
            if any_scalar {
                // x/from_lo/from_hi must share a dimension (input space)
                let from_dim = from_lo.dimension();
                if from_hi.dimension() != from_dim || x.dimension() != from_dim {
                    return Some(Value::Undef);
                }
                // to_lo/to_hi must share a dimension (output space)
                let to_dim = to_lo.dimension();
                if to_hi.dimension() != to_dim {
                    return Some(Value::Undef);
                }
                // Extract si_values via as_f64()
                let (xv, flov, fhiv, tlov, thiv) = match (
                    x.as_f64(),
                    from_lo.as_f64(),
                    from_hi.as_f64(),
                    to_lo.as_f64(),
                    to_hi.as_f64(),
                ) {
                    (Some(a), Some(b), Some(c), Some(d), Some(e)) => (a, b, c, d, e),
                    _ => return Some(Value::Undef),
                };
                if flov == fhiv {
                    return Some(Value::Undef); // early-exit: division by zero
                }
                let result = tlov + (xv - flov) * (thiv - tlov) / (fhiv - flov);
                return Some(sanitize_value(Value::Scalar {
                    si_value: result,
                    dimension: to_dim,
                }));
            }

            // Non-Scalar path: use quinary_f64 helper
            quinary_f64(args, |x, from_lo, from_hi, to_lo, to_hi| {
                if from_lo == from_hi {
                    return Value::Undef; // early-exit: division by zero
                }
                let result = to_lo + (x - from_lo) * (to_hi - to_lo) / (from_hi - from_lo);
                Value::Real(result)
            })
        }

        _ => return None,
    })
}

/// Returns true iff `lo` and `hi` form a valid (non-NaN, non-inverted) range.
fn valid_f64_range(lo: f64, hi: f64) -> bool {
    !lo.is_nan() && !hi.is_nan() && lo <= hi
}

/// Linear interpolation: `a + t * (b - a)`.
fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + t * (b - a)
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_core::DimensionVector;
    use reify_ir::Value;

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
            Value::Scalar {
                si_value,
                dimension,
            } => {
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
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 3.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::AREA); // LENGTH^2 == AREA
            }
            other => panic!("expected Scalar{{3.0, AREA}}, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_scalar_length_to_fractional_exponent() {
        use reify_core::Rational;
        // sqrt(Scalar{4.0, LENGTH}) must return Scalar{2.0, LENGTH^(1/2)}
        let result = eval_builtin(
            "sqrt",
            &[Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            }],
        );
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
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
        assert!(
            result.is_undef(),
            "sqrt of negative Scalar should be Undef, got {:?}",
            result
        );
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
        assert!(
            result.is_undef(),
            "mod by zero should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn mod_non_int_returns_undef() {
        let result = eval_builtin("mod", &[Value::Real(3.5), Value::Real(2.0)]);
        assert!(
            result.is_undef(),
            "mod on Real should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn mod_wrong_arg_count_returns_undef() {
        let result = eval_builtin("mod", &[Value::Int(7)]);
        assert!(
            result.is_undef(),
            "mod with 1 arg should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn mod_i64_min_neg1_returns_undef() {
        // i64::MIN % -1 overflows in Rust (panics in debug mode)
        let result = eval_builtin("mod", &[Value::Int(i64::MIN), Value::Int(-1)]);
        assert!(
            result.is_undef(),
            "mod(i64::MIN, -1) should be Undef (overflow), got {:?}",
            result
        );
    }

    // --- clamp Real tests (step-3) ---

    #[test]
    fn clamp_real_within_range() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            5.0
        );
    }

    #[test]
    fn clamp_real_below_lo() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(-3.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            0.0
        );
    }

    #[test]
    fn clamp_real_above_hi() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(15.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            10.0
        );
    }

    #[test]
    fn clamp_at_lo_boundary() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(0.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            0.0
        );
    }

    #[test]
    fn clamp_at_hi_boundary() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(10.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            10.0
        );
    }

    #[test]
    fn clamp_nan_x_returns_undef() {
        // x is NaN — explicit x.is_nan() guard
        let result = eval_builtin(
            "clamp",
            &[Value::Real(f64::NAN), Value::Real(0.0), Value::Real(10.0)],
        );
        assert!(
            result.is_undef(),
            "clamp(NaN, 0, 10) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_nan_lo_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[Value::Real(5.0), Value::Real(f64::NAN), Value::Real(10.0)],
        );
        assert!(
            result.is_undef(),
            "clamp(5, NaN, 10) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_nan_hi_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[Value::Real(5.0), Value::Real(0.0), Value::Real(f64::NAN)],
        );
        assert!(
            result.is_undef(),
            "clamp(5, 0, NaN) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_inverted_range_real_returns_undef() {
        // lo > hi is invalid
        let result = eval_builtin(
            "clamp",
            &[Value::Real(5.0), Value::Real(10.0), Value::Real(0.0)],
        );
        assert!(
            result.is_undef(),
            "clamp with inverted range should be Undef, got {:?}",
            result
        );
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
        assert!(
            result.is_undef(),
            "clamp Int with inverted range should be Undef, got {:?}",
            result
        );
    }

    // --- clamp Scalar + fallback tests (step-7) ---

    #[test]
    fn clamp_scalar_preserves_dimension() {
        // All three args: same LENGTH dimension, result should be LENGTH Scalar
        assert_scalar_approx!(
            eval_builtin(
                "clamp",
                &[
                    Value::Scalar {
                        si_value: 0.005,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.001,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.010,
                        dimension: DimensionVector::LENGTH
                    },
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
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::TIME,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "clamp with dimension mismatch should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_inverted_range_scalar_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "clamp Scalar with inverted range should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_scalar_nan_x_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "clamp Scalar NaN x should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_wrong_arg_count_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(0.0)]);
        assert!(
            result.is_undef(),
            "clamp with 2 args should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_fallback_dimension_mismatch_returns_undef() {
        // Fallback arm: x is Real (DIMENSIONLESS) but lo/hi are Scalar LENGTH.
        // The fallback cannot silently drop LENGTH -> must return Undef.
        let result = eval_builtin(
            "clamp",
            &[
                Value::Real(5.0),
                Value::Scalar {
                    si_value: 0.001,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.010,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "clamp with mismatched dimensions should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_fallback_all_dimensionless_returns_real() {
        // Fallback arm: x is Int, lo/hi are Real -> all DIMENSIONLESS -> clamp coerces to Real.
        let result = eval_builtin(
            "clamp",
            &[Value::Int(5), Value::Real(0.0), Value::Real(10.0)],
        );
        assert_real_approx!(result, 5.0);
    }

    // --- lerp Real tests (step-9) ---

    #[test]
    fn lerp_midpoint() {
        // lerp(0, 10, 0.5) = 5
        assert_real_approx!(
            eval_builtin(
                "lerp",
                &[Value::Real(0.0), Value::Real(10.0), Value::Real(0.5)]
            ),
            5.0
        );
    }

    #[test]
    fn lerp_t_zero() {
        // lerp(a, b, 0) = a
        assert_real_approx!(
            eval_builtin(
                "lerp",
                &[Value::Real(3.0), Value::Real(7.0), Value::Real(0.0)]
            ),
            3.0
        );
    }

    #[test]
    fn lerp_t_one() {
        // lerp(a, b, 1) = b
        assert_real_approx!(
            eval_builtin(
                "lerp",
                &[Value::Real(3.0), Value::Real(7.0), Value::Real(1.0)]
            ),
            7.0
        );
    }

    #[test]
    fn lerp_negative_t_extrapolation() {
        // lerp(0, 10, -0.5) = -5 (extrapolation below)
        assert_real_approx!(
            eval_builtin(
                "lerp",
                &[Value::Real(0.0), Value::Real(10.0), Value::Real(-0.5)]
            ),
            -5.0
        );
    }

    #[test]
    fn lerp_nan_t_returns_undef() {
        // t is NaN — explicit NaN check after extraction
        let result = eval_builtin(
            "lerp",
            &[Value::Real(0.0), Value::Real(10.0), Value::Real(f64::NAN)],
        );
        assert!(
            result.is_undef(),
            "lerp with NaN t should be Undef, got {:?}",
            result
        );
    }

    // --- lerp Scalar + dimension tests (step-11) ---

    #[test]
    fn lerp_scalar_preserves_dimension() {
        // lerp(Scalar{0.0, LENGTH}, Scalar{1.0, LENGTH}, Real(0.5)) = Scalar{0.5, LENGTH}
        assert_scalar_approx!(
            eval_builtin(
                "lerp",
                &[
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::LENGTH
                    },
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
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::TIME,
                },
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp dimension mismatch a/b should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_t_dimensioned_returns_undef() {
        // t must be dimensionless; a LENGTH t is invalid
        let result = eval_builtin(
            "lerp",
            &[
                Value::Real(0.0),
                Value::Real(10.0),
                Value::Scalar {
                    si_value: 0.5,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with dimensioned t should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_nan_a_returns_undef() {
        // NaN in a -> Undef (via sanitize_value)
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with NaN a should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_nan_b_returns_undef() {
        // NaN in b -> Undef (via sanitize_value)
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with NaN b should be Undef, got {:?}",
            result
        );
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
        assert!(
            result.is_undef(),
            "lerp with 2 args should be Undef, got {:?}",
            result
        );
    }

    // --- lerp fallback tests (step-21) ---

    #[test]
    fn lerp_fallback_scalar_a_real_b_returns_undef() {
        // Fallback arm: a is Scalar LENGTH, b is Real -> a's dimension would be silently
        // dropped if we returned Real. Per feedback_silent_defaults_pattern, must return Undef.
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Real(3.0),
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with Scalar a and Real b should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_fallback_real_a_scalar_b_returns_undef() {
        // Fallback arm: a is Real, b is Scalar LENGTH -> symmetric case, also must be Undef.
        let result = eval_builtin(
            "lerp",
            &[
                Value::Real(3.0),
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with Real a and Scalar b should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_fallback_all_dimensionless_returns_real() {
        // Fallback arm: a is Int, b is Real -> both DIMENSIONLESS -> valid coercion to Real.
        assert_real_approx!(
            eval_builtin(
                "lerp",
                &[Value::Int(0), Value::Real(10.0), Value::Real(0.5)],
            ),
            5.0
        );
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
                &[
                    Value::Real(5.0),
                    Value::Real(0.0),
                    Value::Real(10.0),
                    Value::Real(0.0),
                    Value::Real(100.0)
                ]
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
                &[
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(10.0),
                    Value::Real(20.0),
                    Value::Real(30.0)
                ]
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
                &[
                    Value::Real(10.0),
                    Value::Real(0.0),
                    Value::Real(10.0),
                    Value::Real(20.0),
                    Value::Real(30.0)
                ]
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
                &[
                    Value::Real(15.0),
                    Value::Real(0.0),
                    Value::Real(10.0),
                    Value::Real(0.0),
                    Value::Real(100.0)
                ]
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
                &[
                    Value::Real(50.0),
                    Value::Real(0.0),
                    Value::Real(100.0),
                    Value::Real(0.0),
                    Value::Real(10.0)
                ]
            ),
            5.0
        );
    }

    #[test]
    fn remap_division_by_zero_returns_undef() {
        // from_lo == from_hi -> division by zero -> Undef (early-exit)
        let result = eval_builtin(
            "remap",
            &[
                Value::Real(5.0),
                Value::Real(3.0),
                Value::Real(3.0),
                Value::Real(0.0),
                Value::Real(10.0),
            ],
        );
        assert!(
            result.is_undef(),
            "remap with from_lo==from_hi should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn remap_nan_returns_undef() {
        let result = eval_builtin(
            "remap",
            &[
                Value::Real(f64::NAN),
                Value::Real(0.0),
                Value::Real(10.0),
                Value::Real(0.0),
                Value::Real(100.0),
            ],
        );
        assert!(
            result.is_undef(),
            "remap with NaN x should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn remap_wrong_arg_count_returns_undef() {
        let result = eval_builtin(
            "remap",
            &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0)],
        );
        assert!(
            result.is_undef(),
            "remap with 3 args should be Undef, got {:?}",
            result
        );
    }

    // --- remap Scalar tests (step-17) ---
    // remap(x, from_lo, from_hi, to_lo, to_hi)

    #[test]
    fn remap_scalar_preserves_dimension() {
        // All 5 args LENGTH -> result is LENGTH
        // remap(Scalar{5m}, Scalar{0m}, Scalar{10m}, Scalar{0m}, Scalar{100m}) = Scalar{50m}
        assert_scalar_approx!(
            eval_builtin(
                "remap",
                &[
                    Value::Scalar {
                        si_value: 5.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 10.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 100.0,
                        dimension: DimensionVector::LENGTH
                    },
                ]
            ),
            50.0,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn remap_scalar_cross_dimension() {
        // x in LENGTH, from in LENGTH, to in TIME -> result is TIME
        // remap(Scalar{5m, LENGTH}, Scalar{0m}, Scalar{10m}, Scalar{0s, TIME}, Scalar{100s, TIME}) = Scalar{50s, TIME}
        assert_scalar_approx!(
            eval_builtin(
                "remap",
                &[
                    Value::Scalar {
                        si_value: 5.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 10.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::TIME
                    },
                    Value::Scalar {
                        si_value: 100.0,
                        dimension: DimensionVector::TIME
                    },
                ]
            ),
            50.0,
            DimensionVector::TIME
        );
    }

    #[test]
    fn remap_scalar_dimension_mismatch_x_from_returns_undef() {
        // x has TIME dimension but from_lo/from_hi are LENGTH -> Undef
        let result = eval_builtin(
            "remap",
            &[
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::TIME,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 100.0,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "remap with x dim != from dim should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn remap_scalar_to_range_mismatch_returns_undef() {
        // to_lo and to_hi have different dimensions -> Undef
        let result = eval_builtin(
            "remap",
            &[
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::TIME,
                },
                Value::Scalar {
                    si_value: 100.0,
                    dimension: DimensionVector::LENGTH,
                }, // mismatch
            ],
        );
        assert!(
            result.is_undef(),
            "remap with to_lo/to_hi dim mismatch should be Undef, got {:?}",
            result
        );
    }
}
