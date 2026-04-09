use reify_types::{DimensionVector, Value};
use crate::common::*;

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let v = match name {
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
    };
    Some(v)
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn numeric_dispatch_abs_int() {
        assert_eq!(dispatch("abs", &[Value::Int(-5)]), Some(Value::Int(5)));
    }

    #[test]
    fn numeric_dispatch_unknown_returns_none() {
        assert!(dispatch("unknown_fn", &[]).is_none());
    }
}
