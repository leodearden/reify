use reify_types::{DimensionVector, Value};

use super::sanitize::sanitize_value;

/// Evaluate a complex-number method call.
///
/// Returns `Some(value)` for recognized complex methods (`magnitude`, `phase`,
/// `conjugate`, `re`, `im`), `None` otherwise — letting the caller fall through
/// to other method-dispatch logic.
pub(crate) fn eval_complex_method(obj: &Value, method: &str, args: &[Value]) -> Option<Value> {
    match method {
        "magnitude" => {
            if !args.is_empty() {
                return Some(Value::Undef);
            }
            match obj {
                Value::Complex { re, im, dimension } => {
                    let mag = re.hypot(*im);
                    Some(sanitize_value(Value::from_component(mag, *dimension)))
                }
                _ => Some(Value::Undef),
            }
        }
        "phase" => {
            if !args.is_empty() {
                return Some(Value::Undef);
            }
            match obj {
                Value::Complex { re, im, .. } => {
                    if !re.is_finite() || !im.is_finite() {
                        return Some(Value::Undef);
                    }
                    if *re == 0.0 && *im == 0.0 {
                        return Some(Value::Undef);
                    }
                    // The pre-guard above is essential: atan2(y, Inf) = 0.0 and
                    // atan2(y, -Inf) = ±π are both finite, so sanitize_value alone
                    // cannot detect Inf inputs — it would silently return a wrong result.
                    // After the guard, atan2(finite, finite) with at least one non-zero
                    // argument always returns a value in [-π, π], so no output
                    // sanitization is needed here.
                    let angle = im.atan2(*re);
                    Some(Value::Scalar {
                        si_value: angle,
                        dimension: DimensionVector::ANGLE,
                    })
                }
                _ => Some(Value::Undef),
            }
        }
        "conjugate" => {
            if !args.is_empty() {
                return Some(Value::Undef);
            }
            match obj {
                Value::Complex { re, im, dimension } => {
                    // Defense-in-depth: reject poisoned inputs before constructing
                    // the output Complex, mirroring the phase-method pattern.
                    // sanitize_value's Complex arm provides a secondary layer but a
                    // direct pre-guard is independent and more robust.
                    if !re.is_finite() || !im.is_finite() {
                        return Some(Value::Undef);
                    }
                    Some(Value::Complex {
                        re: *re,
                        im: -im,
                        dimension: *dimension,
                    })
                }
                _ => Some(Value::Undef),
            }
        }
        "re" | "im" => {
            if !args.is_empty() {
                return Some(Value::Undef);
            }
            match obj {
                Value::Complex { re, im, dimension } => {
                    let component = if method == "re" { *re } else { *im };
                    Some(sanitize_value(Value::from_component(component, *dimension)))
                }
                _ => Some(Value::Undef),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use reify_types::{CompiledExpr, DimensionVector, Type, Value, ValueMap};

    use crate::{eval_expr, EvalContext};

    // Helper to build a literal expression
    fn lit(v: Value, ty: Type) -> CompiledExpr {
        CompiledExpr::literal(v, ty)
    }

    // ── method: re ────────────────────────────────────────────────────────────

    #[test]
    fn re_nan_dimensionless_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.re → Undef
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "re".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.re with NaN real part should return Undef"
        );
    }

    #[test]
    fn re_inf_dimensionless_returns_undef() {
        // Complex{re:+Inf, im:1.0, DIMENSIONLESS}.re → Undef
        let complex_val = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "re".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.re with Inf real part should return Undef"
        );
    }

    #[test]
    fn re_nan_dimensioned_returns_undef() {
        // Complex{re:NaN, im:1.0, LENGTH}.re → Undef (dimensioned Scalar path)
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::length())),
            "re".to_string(),
            vec![],
            Type::length(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.re with NaN real part (dimensioned) should return Undef"
        );
    }

    // ── method: im ────────────────────────────────────────────────────────────

    #[test]
    fn im_nan_dimensionless_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.im → Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "im".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.im with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn im_inf_dimensionless_returns_undef() {
        // Complex{re:1.0, im:+Inf, DIMENSIONLESS}.im → Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "im".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.im with Inf imaginary part should return Undef"
        );
    }

    #[test]
    fn im_nan_dimensioned_returns_undef() {
        // Complex{re:1.0, im:NaN, LENGTH}.im → Undef (dimensioned Scalar path)
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::LENGTH,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::length())),
            "im".to_string(),
            vec![],
            Type::length(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.im with NaN imaginary part (dimensioned) should return Undef"
        );
    }

    // ── method: magnitude ─────────────────────────────────────────────────────

    #[test]
    fn magnitude_nan_dimensionless_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.magnitude → Undef
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "magnitude".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.magnitude with NaN should return Undef"
        );
    }

    #[test]
    fn magnitude_overflow_dimensionless_returns_undef() {
        // Complex{re:f64::MAX, im:f64::MAX, DIMENSIONLESS}.magnitude → Undef (overflow to +Inf)
        let complex_val = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "magnitude".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.magnitude overflowing to +Inf should return Undef"
        );
    }

    #[test]
    fn magnitude_nan_dimensioned_returns_undef() {
        // Complex{re:NaN, im:1.0, LENGTH}.magnitude → Undef (dimensioned path)
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::length())),
            "magnitude".to_string(),
            vec![],
            Type::length(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.magnitude with NaN (dimensioned) should return Undef"
        );
    }

    #[test]
    fn magnitude_inf_dimensionless_returns_undef() {
        // Complex{re:+Inf, im:0.0, DIMENSIONLESS}.magnitude → Undef (direct Inf input)
        let complex_val = Value::Complex {
            re: f64::INFINITY,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "magnitude".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.magnitude with +Inf input should return Undef"
        );
    }

    // ── method regressions: finite values still work ──────────────────────────

    #[test]
    fn magnitude_finite_dimensionless_correct() {
        // Complex{re:3.0, im:4.0, DIMENSIONLESS}.magnitude == Real(5.0)
        let complex_val = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "magnitude".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Real(v) => assert!((v - 5.0).abs() < 1e-12, "expected 5.0, got {}", v),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    #[test]
    fn re_finite_dimensionless_correct() {
        // Complex{re:3.0, im:4.0, DIMENSIONLESS}.re == Real(3.0)
        let complex_val = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "re".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12, "expected 3.0, got {}", v),
            other => panic!("expected Real(3.0), got {:?}", other),
        }
    }

    #[test]
    fn im_finite_dimensionless_correct() {
        // Complex{re:3.0, im:4.0, DIMENSIONLESS}.im == Real(4.0)
        let complex_val = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "im".to_string(),
            vec![],
            Type::Real,
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Real(v) => assert!((v - 4.0).abs() < 1e-12, "expected 4.0, got {}", v),
            other => panic!("expected Real(4.0), got {:?}", other),
        }
    }

    #[test]
    fn re_finite_dimensioned_correct() {
        // Complex{re:0.003, im:0.004, LENGTH}.re == Scalar{0.003, LENGTH}
        let complex_val = Value::Complex {
            re: 0.003,
            im: 0.004,
            dimension: DimensionVector::LENGTH,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::length())),
            "re".to_string(),
            vec![],
            Type::length(),
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (si_value - 0.003).abs() < 1e-12,
                    "expected 0.003, got {}",
                    si_value
                );
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar{{0.003, LENGTH}}, got {:?}", other),
        }
    }

    // ── method: phase (NaN/Inf sanitization) ─────────────────────────────────

    #[test]
    fn phase_nan_re_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.phase → Undef
        // atan2(1.0, NaN) = NaN; phase should return Undef
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with NaN real part should return Undef"
        );
    }

    #[test]
    fn phase_nan_im_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.phase → Undef
        // atan2(NaN, 1.0) = NaN; phase should return Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn phase_inf_re_returns_undef() {
        // Complex{re:+Inf, im:1.0, DIMENSIONLESS}.phase → Undef
        // Note: atan2(1.0, +Inf) = 0.0 which is finite — sanitize_value alone
        // would NOT catch this Inf input. The pre-guard is what correctly rejects it.
        let complex_val = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with +Inf real part should return Undef"
        );
    }

    #[test]
    fn phase_neg_inf_im_returns_undef() {
        // Complex{re:1.0, im:-Inf, DIMENSIONLESS}.phase → Undef
        // The Complex carries an Inf component, violating sanitization convention
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NEG_INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with -Inf imaginary part should return Undef"
        );
    }

    #[test]
    fn phase_neg_inf_re_returns_undef() {
        // Complex{re:-Inf, im:1.0, DIMENSIONLESS}.phase → Undef
        //
        // Note: atan2(1.0, -Inf) = π, which is finite — so sanitize_value alone
        // would NOT catch this -Inf input and would silently return a wrong result.
        // The pre-guard (!re.is_finite() || !im.is_finite()) is what correctly
        // rejects this case. This test locks that behaviour as a regression guard.
        let complex_val = Value::Complex {
            re: f64::NEG_INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.phase with -Inf real part should return Undef (atan2(1.0,-Inf)=π is finite, \
             so the pre-guard, not sanitize_value, is what catches this)"
        );
    }

    // ── method regressions: finite phase values still work ────────────────────

    #[test]
    fn phase_finite_45_degrees_correct() {
        // Complex{re:1.0, im:1.0, DIMENSIONLESS}.phase == Scalar{π/4, ANGLE}
        // atan2(1.0, 1.0) = π/4 ≈ 0.7853981633974483
        let complex_val = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                let expected = std::f64::consts::FRAC_PI_4;
                assert!(
                    (si_value - expected).abs() < 1e-12,
                    "expected π/4 ≈ {}, got {}",
                    expected,
                    si_value
                );
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Scalar{{π/4, ANGLE}}, got {:?}", other),
        }
    }

    #[test]
    fn phase_finite_180_degrees_correct() {
        // Complex{re:-1.0, im:0.0, DIMENSIONLESS}.phase == Scalar{π, ANGLE}
        // atan2(0.0, -1.0) = π
        let complex_val = Value::Complex {
            re: -1.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "phase".to_string(),
            vec![],
            Type::angle(),
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                let expected = std::f64::consts::PI;
                assert!(
                    (si_value - expected).abs() < 1e-12,
                    "expected π ≈ {}, got {}",
                    expected,
                    si_value
                );
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Scalar{{π, ANGLE}}, got {:?}", other),
        }
    }

    // ── method: conjugate (NaN/Inf sanitization) ─────────────────────────────

    #[test]
    fn conjugate_nan_re_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.conjugate → Undef
        // -1.0 (or -NaN) is still NaN; conjugate should return Undef
        let complex_val = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "conjugate".to_string(),
            vec![],
            Type::complex(Type::Real),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.conjugate with NaN real part should return Undef"
        );
    }

    #[test]
    fn conjugate_nan_im_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.conjugate → Undef
        // -(NaN) is still NaN; conjugate should return Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "conjugate".to_string(),
            vec![],
            Type::complex(Type::Real),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.conjugate with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn conjugate_inf_re_returns_undef() {
        // Complex{re:+Inf, im:1.0, DIMENSIONLESS}.conjugate → Undef
        // The output would carry +Inf in the re field; conjugate should return Undef
        let complex_val = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "conjugate".to_string(),
            vec![],
            Type::complex(Type::Real),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.conjugate with +Inf real part should return Undef"
        );
    }

    #[test]
    fn conjugate_neg_inf_im_returns_undef() {
        // Complex{re:1.0, im:-Inf, DIMENSIONLESS}.conjugate → Undef
        // The conjugate would flip -Inf → +Inf, still non-finite; should return Undef
        let complex_val = Value::Complex {
            re: 1.0,
            im: f64::NEG_INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "conjugate".to_string(),
            vec![],
            Type::complex(Type::Real),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.conjugate with -Inf imaginary part should return Undef"
        );
    }

    #[test]
    fn conjugate_neg_inf_re_returns_undef() {
        // Complex{re:-Inf, im:1.0, DIMENSIONLESS}.conjugate → Undef
        // The output would carry -Inf in the re field; conjugate should return Undef
        let complex_val = Value::Complex {
            re: f64::NEG_INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "conjugate".to_string(),
            vec![],
            Type::complex(Type::Real),
        );
        let values = ValueMap::new();
        assert!(
            eval_expr(&expr, &EvalContext::simple(&values)).is_undef(),
            "z.conjugate with -Inf real part should return Undef"
        );
    }

    // ── method regression: finite conjugate still works ──────────────────────

    #[test]
    fn conjugate_finite_dimensioned_correct() {
        // Complex{re:3.0, im:4.0, LENGTH}.conjugate == Complex{re:3.0, im:-4.0, LENGTH}
        // Guards against the pre-guard accidentally rejecting finite values.
        // Uses a dimensioned (LENGTH) Complex to add coverage beyond the dimensionless
        // path already tested in tests/complex_eval_tests.rs::method_conjugate.
        let complex_val = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::length())),
            "conjugate".to_string(),
            vec![],
            Type::complex(Type::length()),
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Complex { re, im, dimension } => {
                assert!(
                    (re - 3.0).abs() < 1e-12,
                    "expected re=3.0, got {}",
                    re
                );
                assert!(
                    (im - (-4.0)).abs() < 1e-12,
                    "expected im=-4.0, got {}",
                    im
                );
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{re:3.0, im:-4.0, LENGTH}}, got {:?}", other),
        }
    }
}
