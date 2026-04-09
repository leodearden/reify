use reify_types::{DimensionVector, Value};
use crate::common::*;

/// Validate args for a point/vector constructor and return `Value::Point` or `Value::Vector`.
///
/// Validates:
/// 1. `args.len() == expected_n`
/// 2. All args are numeric (Int, Real, or Scalar — `as_f64()` returns Some)
/// 3. All args share the same physical dimension
///
/// Returns `Value::Undef` on any validation failure.
/// When `is_point` is `true`, returns `Value::Point`; otherwise returns `Value::Vector`.
pub(crate) fn construct_point_or_vector(
    args: &[Value],
    expected_n: usize,
    is_point: bool,
) -> Value {
    if args.len() != expected_n {
        return Value::Undef;
    }
    // All args must be numeric
    if !args.iter().all(|a| a.as_f64().is_some()) {
        return Value::Undef;
    }
    // All args must share the same physical dimension
    let first_dim = match args.first() {
        Some(v) => v.dimension(),
        None => return Value::Undef,
    };
    if !args.iter().all(|a| a.dimension() == first_dim) {
        return Value::Undef;
    }
    if is_point {
        Value::Point(args.to_vec())
    } else {
        Value::Vector(args.to_vec())
    }
}

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let v = match name {
        // --- Linear algebra: dot, cross, magnitude, normalize ---
        "normalize" => unary(args, |v| {
            // Determine the output wrapper based on input variant.
            let wrap: fn(Vec<Value>) -> Value = match v {
                Value::Vector(_) => Value::Vector,
                Value::Point(_) => Value::Point,
                _ => Value::Tensor,
            };
            let (vals, _dim) = match tensor_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            // Reject non-finite inputs early — a partially-Undef Tensor is not
            // a meaningful unit vector, so we return a single Undef for the
            // whole result rather than per-component sanitization.
            if vals.iter().any(|x| !x.is_finite()) {
                return Value::Undef;
            }
            let sum_sq: f64 = vals.iter().map(|x| x * x).sum();
            let mag = sum_sq.sqrt();
            // mag is finite here, but squaring can still overflow to Inf.
            if !mag.is_finite() || mag == 0.0 {
                return Value::Undef;
            }
            wrap(vals.iter().map(|x| Value::Real(x / mag)).collect())
        }),

        "magnitude" => unary(args, |v| {
            // Handle Complex before the Tensor fallback.
            if let Value::Complex { re, im, dimension } = v {
                return crate::complex::complex_abs(*re, *im, *dimension);
            }
            let (vals, dim) = match tensor_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            let sum_sq: f64 = vals.iter().map(|x| x * x).sum();
            let mag = sum_sq.sqrt();
            if dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(mag))
            } else {
                sanitize_value(Value::Scalar {
                    si_value: mag,
                    dimension: dim,
                })
            }
        }),

        "cross" => binary(args, |a, b| {
            // Cross product of two vectors → vector; point inputs are
            // semantically invalid (cross is only defined for vectors).
            let wrap: fn(Vec<Value>) -> Value = match (a, b) {
                (Value::Point(_), _) | (_, Value::Point(_)) => return Value::Undef,
                (Value::Vector(_), Value::Vector(_)) => Value::Vector,
                _ => Value::Tensor,
            };
            let (a_vals, a_dim) = match tensor_components_f64(a) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let (b_vals, b_dim) = match tensor_components_f64(b) {
                Some(v) => v,
                None => return Value::Undef,
            };
            if a_vals.len() != 3 || b_vals.len() != 3 {
                return Value::Undef;
            }
            let (a0, a1, a2) = (a_vals[0], a_vals[1], a_vals[2]);
            let (b0, b1, b2) = (b_vals[0], b_vals[1], b_vals[2]);
            let cx = a1 * b2 - a2 * b1;
            let cy = a2 * b0 - a0 * b2;
            let cz = a0 * b1 - a1 * b0;
            let result_dim = a_dim.mul(&b_dim);
            let make_component = |v: f64| -> Value {
                if result_dim == DimensionVector::DIMENSIONLESS {
                    sanitize_value(Value::Real(v))
                } else {
                    sanitize_value(Value::Scalar {
                        si_value: v,
                        dimension: result_dim,
                    })
                }
            };
            wrap(vec![
                make_component(cx),
                make_component(cy),
                make_component(cz),
            ])
        }),

        "dot" => binary(args, |a, b| {
            let (a_vals, a_dim) = match tensor_components_f64(a) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let (b_vals, b_dim) = match tensor_components_f64(b) {
                Some(v) => v,
                None => return Value::Undef,
            };
            if a_vals.len() != b_vals.len() {
                return Value::Undef;
            }
            let sum: f64 = a_vals.iter().zip(b_vals.iter()).map(|(x, y)| x * y).sum();
            let result_dim = a_dim.mul(&b_dim);
            if result_dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(sum))
            } else {
                sanitize_value(Value::Scalar {
                    si_value: sum,
                    dimension: result_dim,
                })
            }
        }),

        // --- Point/Vector constructors ---
        "point2" => construct_point_or_vector(args, 2, true),
        "point3" => construct_point_or_vector(args, 3, true),
        "vec2" => construct_point_or_vector(args, 2, false),
        "vec3" => construct_point_or_vector(args, 3, false),

        _ => return None,
    };
    Some(v)
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn vector_dispatch_dot_orthogonal() {
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert_eq!(dispatch("dot", &[a, b]), Some(Value::Real(0.0)));
    }

    #[test]
    fn vector_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
