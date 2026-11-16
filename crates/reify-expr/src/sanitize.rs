use reify_ir::{Value, quaternion_is_finite};

/// Convert a Value that carries NaN or Inf to Undef.
///
/// All callers pass either a `Value::from_real_scalar(...)` result — which
/// returns only `Value::Real` (dimensionless) or `Value::Scalar` (dimensioned)
/// — or a directly-constructed `Value::Scalar`.  Consequently only the
/// `Value::Real` and `Value::Scalar` arms are reachable from current call
/// sites; the `Value::Complex` and `Value::Orientation` arms are unreachable
/// today but are included for structural parity with `reify-stdlib::sanitize_value`
/// (defense-in-depth: if callers evolve to pass complex or orientation values,
/// sanitization is already in place).
///
/// This helper mirrors the private `sanitize_value` in `reify-stdlib` — the
/// duplication is intentional (making stdlib's version public would widen its
/// API surface; moving it to reify-types would add evaluation semantics to a
/// type crate). The `Real`, `Scalar`, and `Complex` arms remain local mirrors
/// of reify-stdlib; the `Orientation` arm delegates to the shared
/// `reify_types::quaternion_is_finite` predicate.
// SYNC: mirror of reify-stdlib::sanitize_value — keep function AND tests in sync
// NOTE: Orientation arm uses reify_types::quaternion_is_finite (shared predicate)
pub(crate) fn sanitize_value(v: Value) -> Value {
    match &v {
        Value::Real(x) if !x.is_finite() => Value::Undef,
        Value::Scalar { si_value, .. } if !si_value.is_finite() => Value::Undef,
        Value::Complex { re, im, .. } if !re.is_finite() || !im.is_finite() => Value::Undef,
        Value::Orientation { w, x, y, z } if !quaternion_is_finite(*w, *x, *y, *z) => Value::Undef,
        _ => v,
    }
}

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;

    use super::*;

    // SYNC: sanitize_value tests mirrored in reify-stdlib::helpers tests — keep in sync

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
        assert_eq!(
            sanitize_value(Value::Real(2.72)),
            Value::Real(2.72),
            "Real(2.72) must pass through bit-identical"
        );
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
        assert_eq!(
            sanitize_value(Value::Scalar {
                si_value: 0.001,
                dimension: DimensionVector::LENGTH,
            }),
            Value::Scalar {
                si_value: 0.001,
                dimension: DimensionVector::LENGTH,
            },
            "Scalar(0.001, LENGTH) must pass through bit-identical"
        );
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
        assert_eq!(
            sanitize_value(Value::Complex {
                re: 3.0,
                im: -4.0,
                dimension: DimensionVector::DIMENSIONLESS,
            }),
            Value::Complex {
                re: 3.0,
                im: -4.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            "Complex(3.0, -4.0) must pass through bit-identical"
        );
    }

    #[test]
    fn sanitize_complex_nan_im_returns_undef() {
        let v = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with NaN im should become Undef"
        );
    }

    #[test]
    fn sanitize_complex_inf_re_returns_undef() {
        let v = Value::Complex {
            re: f64::INFINITY,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with +Inf re should become Undef"
        );
    }

    #[test]
    fn sanitize_complex_neg_inf_im_returns_undef() {
        let v = Value::Complex {
            re: 0.0,
            im: f64::NEG_INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Complex with -Inf im should become Undef"
        );
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
    fn sanitize_orientation_z_nan_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: f64::NAN,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with NaN z should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_w_neg_inf_returns_undef() {
        let v = Value::Orientation {
            w: f64::NEG_INFINITY,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with -Inf w should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_x_neg_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: f64::NEG_INFINITY,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with -Inf x should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_y_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: f64::INFINITY,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with +Inf y should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_y_neg_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: f64::NEG_INFINITY,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with -Inf y should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_z_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: f64::INFINITY,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with +Inf z should become Undef"
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
        assert_eq!(
            sanitize_value(Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            "Identity orientation must pass through bit-identical"
        );
    }

    #[test]
    fn sanitize_orientation_non_identity_passthrough() {
        // Unit quaternion (0.5, 0.5, 0.5, 0.5) — 120° rotation about (1,1,1)/√3.
        // All components are exact f64 (0.5 = 2^-1), so assert_eq! is safe.
        let v = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        assert_eq!(
            sanitize_value(v),
            Value::Orientation {
                w: 0.5,
                x: 0.5,
                y: 0.5,
                z: 0.5
            },
            "Finite non-identity orientation must pass through unchanged"
        );
    }

    // ── sanitize_value wildcard arm (`_ => v`) characterization tests ─────────
    // Note: *_finite_passthrough tests in the per-variant sections above also
    // exercise this arm — finite values skip all guarded arms and reach `_ => v`.

    #[test]
    fn sanitize_undef_returns_undef() {
        assert_eq!(
            sanitize_value(Value::Undef),
            Value::Undef,
            "Undef is idempotent: sanitize_value(Undef) must return Undef"
        );
    }

    #[test]
    fn sanitize_wildcard_variants_passthrough() {
        // Smoke test: representative `_ => v` variants pass through bit-identical.
        // Bool(true/false), Int, String, Vector, Frame, List, and Transform sample seven of ~25
        // variants that all hit the wildcard arm. Container/struct payloads intentionally carry
        // NaN components as a non-recursion tripwire: if sanitize_value were changed to
        // recurse into children, the inner NaN would become Undef and the assert_eq!
        // below would fail. (Value::PartialEq uses to_bits(), so NaN == NaN here.)
        // The *_finite_passthrough tests above cover the guarded arms.
        let cases = [
            Value::Bool(true),
            Value::Bool(false),
            Value::Int(0),
            Value::String("x".to_string()),
            Value::Vector(vec![Value::Real(f64::NAN)]),
            Value::Frame {
                origin: Box::new(Value::Point(vec![
                    Value::Real(f64::NAN),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ])),
                basis: Box::new(Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
            },
            Value::List(vec![Value::Real(f64::NAN)]),
            Value::Transform {
                rotation: Box::new(Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                translation: Box::new(Value::Vector(vec![
                    Value::Real(f64::NAN),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ])),
            },
        ];
        for v in &cases {
            assert_eq!(
                sanitize_value(v.clone()),
                *v,
                "wildcard variant {:?} must pass through unchanged",
                v
            );
        }
    }
}
