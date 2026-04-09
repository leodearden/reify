use reify_types::Value;

/// Convert a Value that carries NaN or Inf to Undef.
///
/// All callers pass either a `Value::from_component(...)` result — which
/// returns only `Value::Real` (dimensionless) or `Value::Scalar` (dimensioned)
/// — or a directly-constructed `Value::Scalar`.  Consequently only the
/// `Value::Real` and `Value::Scalar` arms are reachable from current call
/// sites; the `Value::Orientation` arm is unreachable today but is included
/// for structural parity with `reify-stdlib::sanitize_value` (defense-in-depth:
/// if callers evolve to pass orientation values, sanitization is already in
/// place).
///
/// This helper mirrors the private `sanitize_value` in `reify-stdlib` — the
/// duplication is intentional (making stdlib's version public would widen its
/// API surface; moving it to reify-types would add evaluation semantics to a
/// type crate).
// SYNC: mirror of reify-stdlib::sanitize_value — keep in sync
pub(crate) fn sanitize_value(v: Value) -> Value {
    match &v {
        Value::Real(x) if x.is_nan() || x.is_infinite() => Value::Undef,
        Value::Scalar { si_value, .. } if si_value.is_nan() || si_value.is_infinite() => {
            Value::Undef
        }
        Value::Complex { re, im, .. } if !re.is_finite() || !im.is_finite() => Value::Undef,
        Value::Orientation { w, x, y, z }
            if !w.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite() =>
        {
            Value::Undef
        }
        _ => v,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::DimensionVector;

    // ── sanitize_value direct unit tests ─────────────────────────────────────

    #[test]
    fn sanitize_real_nan_returns_undef() {
        assert!(
            sanitize_value(Value::Real(f64::NAN)).is_undef(),
            "Real(NaN) should become Undef"
        );
    }

    #[test]
    fn sanitize_real_inf_returns_undef() {
        assert!(
            sanitize_value(Value::Real(f64::INFINITY)).is_undef(),
            "Real(+Inf) should become Undef"
        );
    }

    #[test]
    fn sanitize_real_neg_inf_returns_undef() {
        assert!(
            sanitize_value(Value::Real(f64::NEG_INFINITY)).is_undef(),
            "Real(-Inf) should become Undef"
        );
    }

    #[test]
    fn sanitize_real_finite_passthrough() {
        let v = Value::Real(2.72);
        match sanitize_value(v) {
            Value::Real(x) => assert!((x - 2.72).abs() < 1e-12),
            other => panic!("expected Real(2.72), got {:?}", other),
        }
    }

    #[test]
    fn sanitize_scalar_nan_returns_undef() {
        let v = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Scalar with NaN si_value should become Undef"
        );
    }

    #[test]
    fn sanitize_scalar_inf_returns_undef() {
        let v = Value::Scalar {
            si_value: f64::INFINITY,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Scalar with +Inf si_value should become Undef"
        );
    }

    #[test]
    fn sanitize_scalar_neg_inf_returns_undef() {
        let v = Value::Scalar {
            si_value: f64::NEG_INFINITY,
            dimension: DimensionVector::MASS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Scalar with -Inf si_value should become Undef"
        );
    }

    #[test]
    fn sanitize_scalar_finite_passthrough() {
        let v = Value::Scalar {
            si_value: 0.001,
            dimension: DimensionVector::LENGTH,
        };
        match sanitize_value(v) {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 0.001).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar{{0.001, LENGTH}}, got {:?}", other),
        }
    }

    // ── sanitize_value Complex arm tests ─────────────────────────────────────

    #[test]
    fn sanitize_complex_nan_re_returns_undef() {
        let v = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with NaN re should become Undef"
        );
    }

    #[test]
    fn sanitize_complex_inf_im_returns_undef() {
        let v = Value::Complex {
            re: 0.0,
            im: f64::INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with +Inf im should become Undef"
        );
    }

    #[test]
    fn sanitize_complex_neg_inf_re_returns_undef() {
        let v = Value::Complex {
            re: f64::NEG_INFINITY,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with -Inf re should become Undef"
        );
    }

    #[test]
    fn sanitize_complex_finite_passthrough() {
        let v = Value::Complex {
            re: 3.0,
            im: -4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        match sanitize_value(v) {
            Value::Complex { re, im, .. } => {
                assert!((re - 3.0).abs() < f64::EPSILON);
                assert!((im - (-4.0)).abs() < f64::EPSILON);
            }
            other => panic!("expected Complex{{re:3.0, im:-4.0}}, got {:?}", other),
        }
    }

    // ── sanitize_value Orientation arm tests (task-914) ──────────────────────

    #[test]
    fn sanitize_orientation_nan_returns_undef() {
        let v = Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with NaN w should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: f64::INFINITY,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with +Inf x should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_neg_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: f64::NEG_INFINITY,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with -Inf z should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_nan_y_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: f64::NAN,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with NaN y should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_x_nan_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: f64::NAN,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with NaN x should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_w_inf_returns_undef() {
        let v = Value::Orientation {
            w: f64::INFINITY,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with +Inf w should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_all_components_nonfinite_returns_undef() {
        let v = Value::Orientation {
            w: f64::NAN,
            x: f64::INFINITY,
            y: f64::NEG_INFINITY,
            z: f64::NAN,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with all non-finite components should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_valid_passthrough() {
        let v = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        match sanitize_value(v) {
            Value::Orientation { w, x, y, z } => {
                assert!((w - 1.0).abs() < f64::EPSILON);
                assert!((x - 0.0).abs() < f64::EPSILON);
                assert!((y - 0.0).abs() < f64::EPSILON);
                assert!((z - 0.0).abs() < f64::EPSILON);
            }
            other => panic!("expected Orientation{{1,0,0,0}}, got {:?}", other),
        }
    }
}
