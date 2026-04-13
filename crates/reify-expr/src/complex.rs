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
                    // the output Complex, mirroring the phase-method pattern above.
                    // Unlike phase (where atan2(y, Inf) = 0.0 is finite and sanitize_value
                    // alone cannot detect the poisoned input), conjugate does no numeric
                    // transformation — sanitize_value's Complex arm would also catch Inf/NaN
                    // here, making the two approaches functionally equivalent for conjugate.
                    // The pre-guard is still preferred for stylistic parity with the phase
                    // method and for forward-compatibility: if the Complex arm of
                    // sanitize_value is ever removed, conjugate stays safe.
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

    use crate::{EvalContext, eval_expr};

    // Helper to build a literal expression
    fn lit(v: Value, ty: Type) -> CompiledExpr {
        CompiledExpr::literal(v, ty)
    }

    // Helper: build and evaluate a Complex method call, returning the result Value.
    // `dim` / `elem_ty` describe the component type (DIMENSIONLESS + Real for
    // dimensionless tests; LENGTH + length() for dimensioned tests).
    // `ret_ty` is the declared return type of the method (Real, length(), angle(),
    // complex(Real), etc.).
    fn call_complex_method(
        re: f64,
        im: f64,
        dim: DimensionVector,
        elem_ty: Type,
        method: &str,
        ret_ty: Type,
    ) -> Value {
        let complex_val = Value::Complex { re, im, dimension: dim };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(elem_ty)),
            method.to_string(),
            vec![],
            ret_ty,
        );
        let values = ValueMap::new();
        eval_expr(&expr, &EvalContext::simple(&values))
    }

    // Thin wrapper for the common case: dimensionless Complex with Real-valued
    // components and Real return type.  Covers the majority of re/im/magnitude
    // edge-case tests, making the dimensioned and non-Real-return callsites
    // (which still use `call_complex_method` directly) stand out naturally.
    fn call_complex_method_real(re: f64, im: f64, method: &str) -> Value {
        call_complex_method(
            re,
            im,
            DimensionVector::DIMENSIONLESS,
            Type::Real,
            method,
            Type::Real,
        )
    }

    // ── method: re ────────────────────────────────────────────────────────────

    #[test]
    fn re_nan_dimensionless_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.re → Undef
        assert!(
            call_complex_method_real(f64::NAN, 1.0, "re").is_undef(),
            "z.re with NaN real part should return Undef"
        );
    }

    #[test]
    fn re_inf_dimensionless_returns_undef() {
        // Complex{re:+Inf, im:1.0, DIMENSIONLESS}.re → Undef
        assert!(
            call_complex_method_real(f64::INFINITY, 1.0, "re").is_undef(),
            "z.re with Inf real part should return Undef"
        );
    }

    #[test]
    fn re_nan_dimensioned_returns_undef() {
        // Complex{re:NaN, im:1.0, LENGTH}.re → Undef (dimensioned Scalar path)
        assert!(
            call_complex_method(f64::NAN, 1.0, DimensionVector::LENGTH, Type::length(), "re", Type::length()).is_undef(),
            "z.re with NaN real part (dimensioned) should return Undef"
        );
    }

    #[test]
    fn re_neg_inf_dimensionless_returns_undef() {
        // Complex{re:-Inf, im:1.0, DIMENSIONLESS}.re → Undef
        assert!(
            call_complex_method_real(f64::NEG_INFINITY, 1.0, "re").is_undef(),
            "z.re with NEG_INFINITY real part should return Undef"
        );
    }

    #[test]
    fn re_neg_inf_dimensioned_returns_undef() {
        // Complex{re:-Inf, im:1.0, LENGTH}.re → Undef (dimensioned Scalar path)
        // Mirrors re_nan_dimensioned_returns_undef but for NEG_INFINITY to lock
        // the Scalar{si_value: -Inf} → Undef path in sanitize_value.
        assert!(
            call_complex_method(f64::NEG_INFINITY, 1.0, DimensionVector::LENGTH, Type::length(), "re", Type::length()).is_undef(),
            "z.re with NEG_INFINITY real part (dimensioned) should return Undef"
        );
    }

    // ── method: im ────────────────────────────────────────────────────────────

    #[test]
    fn im_nan_dimensionless_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.im → Undef
        assert!(
            call_complex_method_real(1.0, f64::NAN, "im").is_undef(),
            "z.im with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn im_inf_dimensionless_returns_undef() {
        // Complex{re:1.0, im:+Inf, DIMENSIONLESS}.im → Undef
        assert!(
            call_complex_method_real(1.0, f64::INFINITY, "im").is_undef(),
            "z.im with Inf imaginary part should return Undef"
        );
    }

    #[test]
    fn im_nan_dimensioned_returns_undef() {
        // Complex{re:1.0, im:NaN, LENGTH}.im → Undef (dimensioned Scalar path)
        assert!(
            call_complex_method(1.0, f64::NAN, DimensionVector::LENGTH, Type::length(), "im", Type::length()).is_undef(),
            "z.im with NaN imaginary part (dimensioned) should return Undef"
        );
    }

    #[test]
    fn im_neg_inf_dimensionless_returns_undef() {
        // Complex{re:1.0, im:-Inf, DIMENSIONLESS}.im → Undef
        assert!(
            call_complex_method_real(1.0, f64::NEG_INFINITY, "im").is_undef(),
            "z.im with NEG_INFINITY imaginary part should return Undef"
        );
    }

    #[test]
    fn im_neg_inf_dimensioned_returns_undef() {
        // Complex{re:1.0, im:-Inf, LENGTH}.im → Undef (dimensioned Scalar path)
        // Mirrors im_nan_dimensioned_returns_undef but for NEG_INFINITY to lock
        // the Scalar{si_value: -Inf} → Undef path in sanitize_value.
        assert!(
            call_complex_method(1.0, f64::NEG_INFINITY, DimensionVector::LENGTH, Type::length(), "im", Type::length()).is_undef(),
            "z.im with NEG_INFINITY imaginary part (dimensioned) should return Undef"
        );
    }

    // ── method: magnitude ─────────────────────────────────────────────────────

    #[test]
    fn magnitude_nan_dimensionless_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.magnitude → Undef
        assert!(
            call_complex_method_real(f64::NAN, 1.0, "magnitude").is_undef(),
            "z.magnitude with NaN should return Undef"
        );
    }

    #[test]
    fn magnitude_overflow_dimensionless_returns_undef() {
        // Complex{re:f64::MAX, im:f64::MAX, DIMENSIONLESS}.magnitude → Undef (overflow to +Inf)
        assert!(
            call_complex_method_real(f64::MAX, f64::MAX, "magnitude").is_undef(),
            "z.magnitude overflowing to +Inf should return Undef"
        );
    }

    #[test]
    fn magnitude_nan_dimensioned_returns_undef() {
        // Complex{re:NaN, im:1.0, LENGTH}.magnitude → Undef (dimensioned path)
        assert!(
            call_complex_method(f64::NAN, 1.0, DimensionVector::LENGTH, Type::length(), "magnitude", Type::length()).is_undef(),
            "z.magnitude with NaN (dimensioned) should return Undef"
        );
    }

    #[test]
    fn magnitude_inf_dimensionless_returns_undef() {
        // Complex{re:+Inf, im:0.0, DIMENSIONLESS}.magnitude → Undef (direct Inf input)
        assert!(
            call_complex_method_real(f64::INFINITY, 0.0, "magnitude").is_undef(),
            "z.magnitude with +Inf input should return Undef"
        );
    }

    #[test]
    fn magnitude_nan_im_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.magnitude → Undef
        // hypot propagates NaN when neither argument is ±∞ (IEEE 754)
        assert!(
            call_complex_method_real(1.0, f64::NAN, "magnitude").is_undef(),
            "z.magnitude with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn magnitude_inf_im_returns_undef() {
        // Complex{re:1.0, im:+Inf, DIMENSIONLESS}.magnitude → Undef
        // hypot returns +Inf when any argument is ±∞; sanitize_value catches it
        assert!(
            call_complex_method_real(1.0, f64::INFINITY, "magnitude").is_undef(),
            "z.magnitude with +Inf imaginary part should return Undef"
        );
    }

    #[test]
    fn magnitude_nan_im_dimensioned_returns_undef() {
        // Complex{re:1.0, im:NaN, LENGTH}.magnitude → Undef (dimensioned im path)
        // hypot propagates NaN; sanitize_value Scalar arm catches non-finite si_value
        assert!(
            call_complex_method(1.0, f64::NAN, DimensionVector::LENGTH, Type::length(), "magnitude", Type::length()).is_undef(),
            "z.magnitude with NaN imaginary part (dimensioned) should return Undef"
        );
    }

    #[test]
    fn magnitude_inf_im_dimensioned_returns_undef() {
        // Complex{re:1.0, im:+Inf, LENGTH}.magnitude → Undef (dimensioned im path)
        // hypot(1.0, +Inf) = +Inf; sanitize_value Scalar arm catches non-finite si_value
        assert!(
            call_complex_method(1.0, f64::INFINITY, DimensionVector::LENGTH, Type::length(), "magnitude", Type::length()).is_undef(),
            "z.magnitude with +Inf imaginary part (dimensioned) should return Undef"
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
        assert!(
            call_complex_method(f64::NAN, 1.0, DimensionVector::DIMENSIONLESS, Type::Real, "phase", Type::angle()).is_undef(),
            "z.phase with NaN real part should return Undef"
        );
    }

    #[test]
    fn phase_nan_im_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.phase → Undef
        // atan2(NaN, 1.0) = NaN; phase should return Undef
        assert!(
            call_complex_method(1.0, f64::NAN, DimensionVector::DIMENSIONLESS, Type::Real, "phase", Type::angle()).is_undef(),
            "z.phase with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn phase_inf_re_returns_undef() {
        // Complex{re:+Inf, im:1.0, DIMENSIONLESS}.phase → Undef
        // Note: atan2(1.0, +Inf) = 0.0 which is finite — sanitize_value alone
        // would NOT catch this Inf input. The pre-guard is what correctly rejects it.
        assert!(
            call_complex_method(f64::INFINITY, 1.0, DimensionVector::DIMENSIONLESS, Type::Real, "phase", Type::angle()).is_undef(),
            "z.phase with +Inf real part should return Undef"
        );
    }

    #[test]
    fn phase_neg_inf_im_returns_undef() {
        // Complex{re:1.0, im:-Inf, DIMENSIONLESS}.phase → Undef
        // The Complex carries an Inf component, violating sanitization convention
        assert!(
            call_complex_method(1.0, f64::NEG_INFINITY, DimensionVector::DIMENSIONLESS, Type::Real, "phase", Type::angle()).is_undef(),
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
        assert!(
            call_complex_method(f64::NEG_INFINITY, 1.0, DimensionVector::DIMENSIONLESS, Type::Real, "phase", Type::angle()).is_undef(),
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
    // Coverage: NaN/±Inf × {re, im}, DIMENSIONLESS; signed-zero and both-non-finite deliberately omitted.

    #[test]
    fn conjugate_nan_re_returns_undef() {
        // Complex{re:NaN, im:1.0, DIMENSIONLESS}.conjugate → Undef
        // -1.0 (or -NaN) is still NaN; conjugate should return Undef
        assert!(
            call_complex_method(f64::NAN, 1.0, DimensionVector::DIMENSIONLESS, Type::Real, "conjugate", Type::complex(Type::Real)).is_undef(),
            "z.conjugate with NaN real part should return Undef"
        );
    }

    #[test]
    fn conjugate_nan_im_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.conjugate → Undef
        // -(NaN) is still NaN; conjugate should return Undef
        assert!(
            call_complex_method(1.0, f64::NAN, DimensionVector::DIMENSIONLESS, Type::Real, "conjugate", Type::complex(Type::Real)).is_undef(),
            "z.conjugate with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn conjugate_inf_re_returns_undef() {
        // Complex{re:+Inf, im:1.0, DIMENSIONLESS}.conjugate → Undef
        // The output would carry +Inf in the re field; conjugate should return Undef
        assert!(
            call_complex_method(f64::INFINITY, 1.0, DimensionVector::DIMENSIONLESS, Type::Real, "conjugate", Type::complex(Type::Real)).is_undef(),
            "z.conjugate with +Inf real part should return Undef"
        );
    }

    #[test]
    fn conjugate_neg_inf_im_returns_undef() {
        // Complex{re:1.0, im:-Inf, DIMENSIONLESS}.conjugate → Undef
        // The conjugate would flip -Inf → +Inf, still non-finite; should return Undef
        assert!(
            call_complex_method(1.0, f64::NEG_INFINITY, DimensionVector::DIMENSIONLESS, Type::Real, "conjugate", Type::complex(Type::Real)).is_undef(),
            "z.conjugate with -Inf imaginary part should return Undef"
        );
    }

    #[test]
    fn conjugate_neg_inf_re_returns_undef() {
        // Complex{re:-Inf, im:1.0, DIMENSIONLESS}.conjugate → Undef
        // The output would carry -Inf in the re field; conjugate should return Undef
        assert!(
            call_complex_method(f64::NEG_INFINITY, 1.0, DimensionVector::DIMENSIONLESS, Type::Real, "conjugate", Type::complex(Type::Real)).is_undef(),
            "z.conjugate with -Inf real part should return Undef"
        );
    }

    #[test]
    fn conjugate_pos_inf_im_returns_undef() {
        // Complex{re:1.0, im:+Inf, DIMENSIONLESS}.conjugate → Undef
        // The conjugate would flip +Inf → -Inf, still non-finite; should return Undef
        assert!(
            call_complex_method(1.0, f64::INFINITY, DimensionVector::DIMENSIONLESS, Type::Real, "conjugate", Type::complex(Type::Real)).is_undef(),
            "z.conjugate with +Inf imaginary part should return Undef"
        );
    }

    // ── method regression: finite conjugate still works ──────────────────────

    #[test]
    fn conjugate_pure_imaginary_correct() {
        // Complex{re:0.0, im:5.0, DIMENSIONLESS}.conjugate == Complex{re:0.0, im:-5.0, DIMENSIONLESS}
        // Guards against the pre-guard accidentally rejecting finite values.
        // Uses a pure-imaginary (re=0.0) DIMENSIONLESS input — orthogonal to
        // tests/complex_eval_tests.rs::method_conjugate which uses (3.0, 4.0, LENGTH).
        // Exercises the pure-imaginary degenerate case (re=0.0) and DIMENSIONLESS
        // dimension, complementing the LENGTH case in the sibling integration test.
        let complex_val = Value::Complex {
            re: 0.0,
            im: 5.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let expr = CompiledExpr::method_call(
            lit(complex_val, Type::complex(Type::Real)),
            "conjugate".to_string(),
            vec![],
            Type::complex(Type::Real),
        );
        let values = ValueMap::new();
        match eval_expr(&expr, &EvalContext::simple(&values)) {
            Value::Complex { re, im, dimension } => {
                assert!((re - 0.0).abs() < 1e-12, "expected re=0.0, got {}", re);
                assert!((im - (-5.0)).abs() < 1e-12, "expected im=-5.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!(
                "expected Complex{{re:0.0, im:-5.0, DIMENSIONLESS}}, got {:?}",
                other
            ),
        }
    }
}
