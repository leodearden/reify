use reify_types::{DimensionVector, Value};
use crate::common::*;

/// Compute the absolute value (modulus) of a complex number.
///
/// Uses [`f64::hypot`] for overflow-resistant magnitude computation,
/// avoiding premature overflow when components are large but the true
/// magnitude is still representable. Returns `Value::Real(mag)` when
/// `dimension` is dimensionless, or `Value::Scalar { si_value: mag,
/// dimension }` otherwise. Non-finite results are converted to `Undef`
/// by [`sanitize_value`].
pub(crate) fn complex_abs(re: f64, im: f64, dimension: DimensionVector) -> Value {
    let mag = re.hypot(im);
    sanitize_value(Value::from_component(mag, dimension))
}

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let v = match name {
        // complex(re, im) constructor: both args must be numeric with matching dimensions.
        // Returns Value::Complex { re, im, dimension }.
        // Returns Undef on: wrong arg count, non-numeric, mismatched dimensions, NaN/Inf.
        "complex" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let re = match args[0].as_f64() {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let im = match args[1].as_f64() {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let dim_re = args[0].dimension();
            let dim_im = args[1].dimension();
            if dim_re != dim_im {
                return Some(Value::Undef);
            }
            if !re.is_finite() || !im.is_finite() {
                return Some(Value::Undef);
            }
            Value::Complex {
                re,
                im,
                dimension: dim_re,
            }
        }

        // re(z) / real(z): extract real part. Returns Real if DIMENSIONLESS, Scalar otherwise.
        "re" | "real" => unary(args, |v| {
            sanitize_value(match v {
                Value::Complex { re, dimension, .. } => Value::from_component(*re, *dimension),
                _ => Value::Undef,
            })
        }),

        // im(z) / imag(z): extract imaginary part. Returns Real if DIMENSIONLESS, Scalar otherwise.
        "im" | "imag" => unary(args, |v| {
            sanitize_value(match v {
                Value::Complex { im, dimension, .. } => Value::from_component(*im, *dimension),
                _ => Value::Undef,
            })
        }),

        // conjugate(z): negate the imaginary part, preserve re and dimension.
        "conjugate" => unary(args, |v| match v {
            Value::Complex { re, im, dimension } => sanitize_value(Value::Complex {
                re: *re,
                im: -im,
                dimension: *dimension,
            }),
            _ => Value::Undef,
        }),

        // phase(z): compute atan2(im, re), return Scalar with ANGLE dimension.
        // phase(0+0i) is undefined — zero vector has no direction.
        "phase" => unary(args, |v| match v {
            Value::Complex { re, im, .. } => {
                if *re == 0.0 && *im == 0.0 {
                    return Value::Undef;
                }
                let angle = im.atan2(*re);
                sanitize_value(Value::Scalar {
                    si_value: angle,
                    dimension: DimensionVector::ANGLE,
                })
            }
            _ => Value::Undef,
        }),

        // complex_magnitude(z): compute sqrt(re²+im²) for Complex inputs only.
        // Returns Real if DIMENSIONLESS, Scalar otherwise.
        // Returns Undef for non-Complex inputs (unlike generic `magnitude` which handles Tensors).
        "complex_magnitude" => unary(args, |v| match v {
            Value::Complex { re, im, dimension } => complex_abs(*re, *im, *dimension),
            _ => Value::Undef,
        }),

        // complex_add(a, b): add two complex numbers with matching dimensions.
        "complex_add" => binary(args, |a, b| match (a, b) {
            (
                Value::Complex {
                    re: ar,
                    im: ai,
                    dimension: ad,
                },
                Value::Complex {
                    re: br,
                    im: bi,
                    dimension: bd,
                },
            ) => {
                if ad != bd {
                    return Value::Undef;
                }
                sanitize_value(Value::Complex {
                    re: ar + br,
                    im: ai + bi,
                    dimension: *ad,
                })
            }
            _ => Value::Undef,
        }),

        // complex_mul(a, b): multiply two complex numbers, combining dimensions via mul().
        // (a+bi)(c+di) = (ac-bd) + (ad+bc)i
        "complex_mul" => binary(args, |a, b| match (a, b) {
            (
                Value::Complex {
                    re: ar,
                    im: ai,
                    dimension: ad,
                },
                Value::Complex {
                    re: br,
                    im: bi,
                    dimension: bd,
                },
            ) => {
                let re = ar * br - ai * bi;
                let im = ar * bi + ai * br;
                let dimension = ad.mul(bd);
                sanitize_value(Value::Complex { re, im, dimension })
            }
            _ => Value::Undef,
        }),

        _ => return None,
    };
    Some(v)
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn complex_dispatch_constructor() {
        let result = dispatch("complex", &[Value::Real(3.0), Value::Real(4.0)]);
        assert_eq!(
            result,
            Some(Value::Complex {
                re: 3.0,
                im: 4.0,
                dimension: DimensionVector::DIMENSIONLESS,
            })
        );
    }

    #[test]
    fn complex_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
