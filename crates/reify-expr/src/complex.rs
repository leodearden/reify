use reify_ir::Value;

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
                    Some(sanitize_value(Value::from_real_scalar(mag, *dimension)))
                }
                _ => Some(Value::Undef),
            }
        }
        "phase" => {
            if !args.is_empty() {
                return Some(Value::Undef);
            }
            match obj {
                // Delegate to the shared helper in reify-stdlib so this method path
                // and the builtin path (stdlib::eval_complex "phase" arm) use the
                // exact same pre-guards (is_finite + zero-vector) and output shape.
                // See `reify_stdlib::complex_phase` for the guard rationale.
                Value::Complex { re, im, .. } => Some(reify_stdlib::complex_phase(*re, *im)),
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
                    Some(sanitize_value(Value::from_real_scalar(
                        component, *dimension,
                    )))
                }
                _ => Some(Value::Undef),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use reify_core::{DimensionVector, Type};
    use reify_ir::{CompiledExpr, Value, ValueMap};

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
        let complex_val = Value::Complex {
            re,
            im,
            dimension: dim,
        };
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
            call_complex_method(
                f64::NAN,
                1.0,
                DimensionVector::LENGTH,
                Type::length(),
                "re",
                Type::length()
            )
            .is_undef(),
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
            call_complex_method(
                f64::NEG_INFINITY,
                1.0,
                DimensionVector::LENGTH,
                Type::length(),
                "re",
                Type::length()
            )
            .is_undef(),
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
            call_complex_method(
                1.0,
                f64::NAN,
                DimensionVector::LENGTH,
                Type::length(),
                "im",
                Type::length()
            )
            .is_undef(),
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
            call_complex_method(
                1.0,
                f64::NEG_INFINITY,
                DimensionVector::LENGTH,
                Type::length(),
                "im",
                Type::length()
            )
            .is_undef(),
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
            call_complex_method(
                f64::NAN,
                1.0,
                DimensionVector::LENGTH,
                Type::length(),
                "magnitude",
                Type::length()
            )
            .is_undef(),
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
            call_complex_method(
                1.0,
                f64::NAN,
                DimensionVector::LENGTH,
                Type::length(),
                "magnitude",
                Type::length()
            )
            .is_undef(),
            "z.magnitude with NaN imaginary part (dimensioned) should return Undef"
        );
    }

    #[test]
    fn magnitude_inf_im_dimensioned_returns_undef() {
        // Complex{re:1.0, im:+Inf, LENGTH}.magnitude → Undef (dimensioned im path)
        // hypot(1.0, +Inf) = +Inf; sanitize_value Scalar arm catches non-finite si_value
        assert!(
            call_complex_method(
                1.0,
                f64::INFINITY,
                DimensionVector::LENGTH,
                Type::length(),
                "magnitude",
                Type::length()
            )
            .is_undef(),
            "z.magnitude with +Inf imaginary part (dimensioned) should return Undef"
        );
    }

    #[test]
    fn magnitude_zero_dimensioned_complex_returns_scalar_zero() {
        // Complex{re:0.0, im:0.0, LENGTH}.magnitude → Scalar{0.0, LENGTH}
        //
        // Unlike phase (which returns Undef for a zero vector), magnitude of a
        // zero complex is well-defined at zero. This test locks that zero
        // dimensioned complexes return a zero Scalar with the ORIGINAL dimension,
        // not Real(0.0) — mirrors the stdlib builtin-path test on the method path.
        match call_complex_method(
            0.0,
            0.0,
            DimensionVector::LENGTH,
            Type::length(),
            "magnitude",
            Type::length(),
        ) {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    si_value.abs() < 1e-15,
                    "expected si_value=0.0, got {}",
                    si_value
                );
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar{{0.0, LENGTH}}, got {:?}", other),
        }
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
            call_complex_method(
                f64::NAN,
                1.0,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with NaN real part should return Undef"
        );
    }

    #[test]
    fn phase_nan_im_returns_undef() {
        // Complex{re:1.0, im:NaN, DIMENSIONLESS}.phase → Undef
        // atan2(NaN, 1.0) = NaN; phase should return Undef
        assert!(
            call_complex_method(
                1.0,
                f64::NAN,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with NaN imaginary part should return Undef"
        );
    }

    #[test]
    fn phase_inf_re_returns_undef() {
        // Complex{re:+Inf, im:1.0, DIMENSIONLESS}.phase → Undef
        // Note: atan2(1.0, +Inf) = 0.0 which is finite — sanitize_value alone
        // would NOT catch this Inf input. The pre-guard is what correctly rejects it.
        assert!(
            call_complex_method(
                f64::INFINITY,
                1.0,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with +Inf real part should return Undef"
        );
    }

    #[test]
    fn phase_inf_im_returns_undef() {
        // Complex{re:1.0, im:+Inf, DIMENSIONLESS}.phase → Undef
        // Note: atan2(+Inf, 1.0) = π/2 which is finite — sanitize_value alone
        // would NOT catch this Inf input. The pre-guard is what correctly rejects it.
        assert!(
            call_complex_method(
                1.0,
                f64::INFINITY,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with +Inf imaginary part should return Undef"
        );
    }

    #[test]
    fn phase_neg_inf_im_returns_undef() {
        // Complex{re:1.0, im:-Inf, DIMENSIONLESS}.phase → Undef
        //
        // Note: atan2(-Inf, 1.0) = -π/2, which is finite — so sanitize_value alone
        // would NOT catch this -Inf input and would silently return a wrong result.
        // The pre-guard (!re.is_finite() || !im.is_finite()) is what correctly
        // rejects this case. This test locks that behaviour as a regression guard.
        assert!(
            call_complex_method(
                1.0,
                f64::NEG_INFINITY,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with -Inf imaginary part should return Undef (atan2(-Inf,1.0)=-π/2 is finite, \
             so the pre-guard, not sanitize_value, is what catches this)"
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
            call_complex_method(
                f64::NEG_INFINITY,
                1.0,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with -Inf real part should return Undef (atan2(1.0,-Inf)=π is finite, \
             so the pre-guard, not sanitize_value, is what catches this)"
        );
    }

    #[test]
    fn phase_nan_im_dimensioned_returns_undef() {
        // Complex{re:1.0, im:NaN, LENGTH}.phase → Undef
        //
        // phase() ignores the Complex's dimension field — Value::Complex { re, im, .. }
        // drops it before the is_finite pre-guard runs, so dimensionless and dimensioned
        // inputs take the same code path through phase(). This test does not exercise a
        // distinct branch; it locks the invariant that phase() ignores dimension: if a
        // future refactor introduced a dimensioned fast/slow split and accidentally
        // omitted the pre-guard on one branch, this test would catch the regression.
        // Mirrors re_nan_dimensioned_returns_undef for parity across complex methods.
        assert!(
            call_complex_method(
                1.0,
                f64::NAN,
                DimensionVector::LENGTH,
                Type::length(),
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with NaN imaginary part (dimensioned) should return Undef"
        );
    }

    #[test]
    fn phase_neg_inf_im_dimensioned_returns_undef() {
        // Complex{re:1.0, im:-Inf, LENGTH}.phase → Undef
        //
        // atan2(-Inf, 1.0) = -π/2 which is finite, so sanitize_value alone cannot
        // catch this -Inf input (and phase() doesn't wrap its output in sanitize_value
        // anyway). Like the NaN variant above, phase() ignores the Complex's dimension
        // field — dimensionless and dimensioned inputs take the same code path. This
        // test locks the invariant that phase() ignores dimension for NEG_INFINITY
        // inputs: not a distinct branch, but a guard against a future dimensioned
        // fast/slow split accidentally omitting the pre-guard. Mirrors
        // im_neg_inf_dimensioned_returns_undef and re_neg_inf_dimensioned_returns_undef.
        assert!(
            call_complex_method(
                1.0,
                f64::NEG_INFINITY,
                DimensionVector::LENGTH,
                Type::length(),
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with -Inf imaginary part (dimensioned) should return Undef"
        );
    }

    // ── method: phase (zero-vector edge case) ─────────────────────────────────

    #[test]
    fn phase_zero_complex_returns_undef() {
        // Complex{re:0.0, im:0.0, DIMENSIONLESS}.phase → Undef
        // atan2(0.0, 0.0) = 0.0 which is finite — neither sanitize_value nor the
        // is_finite pre-guard can detect this case. The zero-vector guard
        // (`*re == 0.0 && *im == 0.0`) correctly rejects the mathematically-
        // undefined phase of the zero vector. This test locks that guard as a
        // regression guard.
        assert!(
            call_complex_method(
                0.0,
                0.0,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with zero vector should return Undef (atan2(0.0,0.0)=0.0 is finite, \
             so the zero-vector guard, not sanitize_value, is what catches this)"
        );
    }

    #[test]
    fn phase_signed_zero_complex_returns_undef() {
        // Complex{re:-0.0, im:-0.0, DIMENSIONLESS}.phase → Undef
        // (plus both mixed-sign variants: (0.0,-0.0) and (-0.0,0.0))
        //
        // This is a separate #[test] from phase_zero_complex_returns_undef so that
        // the signed-zero path is independently asserted: if the first test fails the
        // test runner still executes this one, making signed-zero regressions visible.
        //
        // IEEE-754 guarantees -0.0 == 0.0, so the zero-vector guard (`*re == 0.0 &&
        // *im == 0.0`) in phase() catches all signed-zero variants today. This test
        // locks all three mixed/negative-sign combinations against a future refactor
        // that swapped `==` for a bit-pattern check (e.g. `to_bits() == 0`), which
        // would silently break mixed-sign cases.
        assert!(
            call_complex_method(
                -0.0,
                -0.0,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with signed-zero vector (-0.0,-0.0) should return Undef \
             (IEEE-754: -0.0 == 0.0, so the zero-vector guard catches this too)"
        );
        assert!(
            call_complex_method(
                0.0,
                -0.0,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with mixed-sign zero vector (0.0,-0.0) should return Undef \
             (IEEE-754: -0.0 == 0.0, so the zero-vector guard catches this too)"
        );
        assert!(
            call_complex_method(
                -0.0,
                0.0,
                DimensionVector::DIMENSIONLESS,
                Type::Real,
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with mixed-sign zero vector (-0.0,0.0) should return Undef \
             (IEEE-754: -0.0 == 0.0, so the zero-vector guard catches this too)"
        );
    }

    #[test]
    fn phase_zero_dimensioned_complex_returns_undef() {
        // Complex{re:0.0, im:0.0, LENGTH}.phase → Undef (dimensioned zero-vector)
        //
        // phase() is dimension-invariant by contract — the zero-vector guard fires
        // before dimension is ever consulted. This test mirrors
        // phase_zero_complex_returns_undef but with LENGTH dimension, locking the
        // invariant that a future refactor which added a dimension-aware fast path
        // cannot silently drop the zero-vector guard on one branch.
        assert!(
            call_complex_method(
                0.0,
                0.0,
                DimensionVector::LENGTH,
                Type::length(),
                "phase",
                Type::angle()
            )
            .is_undef(),
            "z.phase with dimensioned zero vector (LENGTH) should return Undef"
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

    #[test]
    fn phase_finite_dimensioned_returns_angle() {
        // Complex{re:1.0, im:1.0, LENGTH}.phase → Scalar{π/4, ANGLE}
        // The phase method ignores the dimension of the Complex components and always
        // returns a dimensionless angle — this test locks that contract.
        match call_complex_method(
            1.0,
            1.0,
            DimensionVector::LENGTH,
            Type::length(),
            "phase",
            Type::angle(),
        ) {
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
}
