use reify_types::{DimensionVector, Value};
use crate::common::*;

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let v = match name {
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
    };
    Some(v)
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn trig_dispatch_sin_zero() {
        let angle = Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::ANGLE,
        };
        assert_eq!(dispatch("sin", &[angle]), Some(Value::Real(0.0)));
    }

    #[test]
    fn trig_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
