use reify_ir::Value;

use crate::helpers::{binary, complex_abs, complex_phase, sanitize_value, unary};

pub(crate) fn eval_complex(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
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
                Value::Complex { re, dimension, .. } => Value::from_real_scalar(*re, *dimension),
                _ => Value::Undef,
            })
        }),

        // im(z) / imag(z): extract imaginary part. Returns Real if DIMENSIONLESS, Scalar otherwise.
        "im" | "imag" => unary(args, |v| {
            sanitize_value(match v {
                Value::Complex { im, dimension, .. } => Value::from_real_scalar(*im, *dimension),
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
        // Delegates to the shared helper so the method path (reify-expr) and
        // builtin path share identical pre-guards and output construction.
        "phase" => unary(args, |v| match v {
            Value::Complex { re, im, .. } => complex_phase(*re, *im),
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
    })
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    // ── complex() constructor tests (step-1) ──────────────────────────────────

    #[test]
    fn complex_real_real_returns_dimensionless() {
        // complex(Real, Real) → Complex with DIMENSIONLESS dimension
        let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(4.0)]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12, "expected re=3.0, got {}", re);
                assert!((im - 4.0).abs() < 1e-12, "expected im=4.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{3,4,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_int_int_returns_dimensionless() {
        // complex(Int, Int) → Complex with DIMENSIONLESS dimension
        let result = eval_builtin("complex", &[Value::Int(5), Value::Int(-2)]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 5.0).abs() < 1e-12, "expected re=5.0, got {}", re);
                assert!((im - (-2.0)).abs() < 1e-12, "expected im=-2.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{5,-2,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_int_real_mixed_coercion_dimensionless() {
        // complex(Int, Real) → Complex with DIMENSIONLESS dimension (both dimensionless)
        let result = eval_builtin("complex", &[Value::Int(1), Value::Real(2.5)]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 1.0).abs() < 1e-12, "expected re=1.0, got {}", re);
                assert!((im - 2.5).abs() < 1e-12, "expected im=2.5, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{1,2.5,DIMLESS}}, got {:?}", other),
        }
    }

    // ── complex() with Scalar args (step-3) ───────────────────────────────────

    #[test]
    fn complex_scalar_mm_preserves_length_dimension() {
        // complex(Scalar{5mm}, Scalar{3mm}) → Complex{0.005, 0.003, LENGTH}
        let result = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 0.005,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.003,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 0.005).abs() < 1e-15, "expected re=0.005, got {}", re);
                assert!((im - 0.003).abs() < 1e-15, "expected im=0.003, got {}", im);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{0.005,0.003,LENGTH}}, got {:?}", other),
        }
    }

    // ── complex() error cases (step-5) ────────────────────────────────────────

    #[test]
    fn complex_dimension_mismatch_returns_undef() {
        // complex(3mm, 4s) → Undef (LENGTH ≠ TIME)
        let result = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 0.003,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 4.0,
                    dimension: DimensionVector::TIME,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "expected Undef for dimension mismatch, got {:?}",
            result
        );
    }

    #[test]
    fn complex_real_with_scalar_dimension_mismatch_returns_undef() {
        // complex(Real(3.0), Scalar{4, LENGTH}) → Undef
        // Real is DIMENSIONLESS, Scalar{LENGTH} is not — mismatch
        let result = eval_builtin(
            "complex",
            &[
                Value::Real(3.0),
                Value::Scalar {
                    si_value: 4.0,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "expected Undef for Real+Scalar mismatch, got {:?}",
            result
        );
    }

    #[test]
    fn complex_zero_args_returns_undef() {
        let result = eval_builtin("complex", &[]);
        assert!(
            result.is_undef(),
            "expected Undef for 0 args, got {:?}",
            result
        );
    }

    #[test]
    fn complex_three_args_returns_undef() {
        let result = eval_builtin(
            "complex",
            &[Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        );
        assert!(
            result.is_undef(),
            "expected Undef for 3 args, got {:?}",
            result
        );
    }

    #[test]
    fn complex_non_numeric_re_returns_undef() {
        let result = eval_builtin("complex", &[Value::Bool(true), Value::Real(3.0)]);
        assert!(
            result.is_undef(),
            "expected Undef for non-numeric re, got {:?}",
            result
        );
    }

    #[test]
    fn complex_nan_arg_returns_undef() {
        let result = eval_builtin("complex", &[Value::Real(f64::NAN), Value::Real(3.0)]);
        assert!(
            result.is_undef(),
            "expected Undef for NaN re, got {:?}",
            result
        );
    }

    #[test]
    fn complex_inf_arg_returns_undef() {
        let result = eval_builtin("complex", &[Value::Real(f64::INFINITY), Value::Real(3.0)]);
        assert!(
            result.is_undef(),
            "expected Undef for Inf re, got {:?}",
            result
        );
    }

    #[test]
    fn complex_nan_im_arg_returns_undef() {
        // NaN in the imaginary (second) arg should also produce Undef
        let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(f64::NAN)]);
        assert!(
            result.is_undef(),
            "expected Undef for NaN im, got {:?}",
            result
        );
    }

    #[test]
    fn complex_inf_im_arg_returns_undef() {
        // Infinity in the imaginary (second) arg should also produce Undef
        let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(f64::INFINITY)]);
        assert!(
            result.is_undef(),
            "expected Undef for Inf im, got {:?}",
            result
        );
    }

    // ── re() and im() accessor tests (step-7) ────────────────────────────────

    #[test]
    fn re_dimensionless_returns_real() {
        // re(Complex{3,4,DIMLESS}) → Real(3.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("re", &[z]), 3.0);
    }

    #[test]
    fn im_dimensionless_returns_real() {
        // im(Complex{3,4,DIMLESS}) → Real(4.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("im", &[z]), 4.0);
    }

    #[test]
    fn re_dimensioned_returns_scalar() {
        // re(Complex{5,3,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("re", &[z]), 5.0, DimensionVector::LENGTH);
    }

    #[test]
    fn im_dimensioned_returns_scalar() {
        // im(Complex{5,3,LENGTH}) → Scalar{3.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("im", &[z]), 3.0, DimensionVector::LENGTH);
    }

    #[test]
    fn re_non_complex_returns_undef() {
        assert!(eval_builtin("re", &[Value::Real(3.0)]).is_undef());
    }

    #[test]
    fn im_non_complex_returns_undef() {
        assert!(eval_builtin("im", &[Value::Real(3.0)]).is_undef());
    }

    // ── conjugate() tests (step-9) ────────────────────────────────────────────

    #[test]
    fn conjugate_dimensionless_negates_im() {
        // conjugate(Complex{3,4,DIMLESS}) → Complex{3,-4,DIMLESS}
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("conjugate", &[z]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12);
                assert!((im - (-4.0)).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{3,-4,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn conjugate_dimensioned_preserves_dimension() {
        // conjugate(Complex{5,3,LENGTH}) → Complex{5,-3,LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        let result = eval_builtin("conjugate", &[z]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 5.0).abs() < 1e-12);
                assert!((im - (-3.0)).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{5,-3,LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn conjugate_non_complex_returns_undef() {
        assert!(eval_builtin("conjugate", &[Value::Real(3.0)]).is_undef());
    }

    #[test]
    fn conjugate_nan_re_returns_undef() {
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with NaN re must return Undef"
        );
    }

    #[test]
    fn conjugate_nan_im_returns_undef() {
        let z = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with NaN im must return Undef"
        );
    }

    #[test]
    fn conjugate_inf_re_returns_undef() {
        let z = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with Inf re must return Undef"
        );
    }

    #[test]
    fn conjugate_inf_im_returns_undef() {
        let z = Value::Complex {
            re: 1.0,
            im: f64::NEG_INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with -Inf im must return Undef"
        );
    }

    // ── magnitude on Complex tests (step-11) ─────────────────────────────────

    #[test]
    fn magnitude_complex_dimensionless_3_4_returns_5() {
        // magnitude(Complex{3,4,DIMLESS}) → Real(5.0) (3-4-5 Pythagorean triple)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("magnitude", &[z]), 5.0);
    }

    #[test]
    fn magnitude_complex_dimensioned_3_4_returns_scalar_5() {
        // magnitude(Complex{3,4,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("magnitude", &[z]),
            5.0,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn magnitude_large_representable_complex_no_overflow() {
        // magnitude(Complex{1e200, 0, DIMLESS}) must return Real(1e200), not Undef.
        // Covers the generic 'magnitude' builtin path to complex_abs.
        let z = Value::Complex {
            re: 1e200,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("magnitude", &[z]), 1e200);
    }

    #[test]
    fn magnitude_zero_complex_returns_zero() {
        // magnitude(0+0i) = 0.0 (zero vector has zero magnitude, unlike phase which is undef)
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("magnitude", &[z]), 0.0);
    }

    #[test]
    fn magnitude_zero_dimensioned_complex_returns_scalar_zero() {
        // magnitude(Complex{0,0,LENGTH}) → Scalar{0.0, LENGTH}.
        //
        // Unlike phase (which returns Undef for a zero vector since direction is
        // mathematically undefined), magnitude of a zero complex is well-defined at
        // zero. This test locks the contract that a zero dimensioned complex returns
        // a zero Scalar carrying the original dimension — the builtin path through
        // complex_abs → sanitize_value → from_real_scalar dispatches on LENGTH and
        // produces Scalar{0.0, LENGTH}, NOT Real(0.0).
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("complex_magnitude", &[z]),
            0.0,
            DimensionVector::LENGTH
        );
    }

    // ── phase() tests (step-13) ───────────────────────────────────────────────

    #[test]
    fn phase_complex_1_1_returns_pi_over_4() {
        // phase(1+1i) = π/4
        let z = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("phase", &[z]),
            std::f64::consts::FRAC_PI_4,
            DimensionVector::ANGLE
        );
    }

    #[test]
    fn phase_complex_1_0_returns_0() {
        // phase(1+0i) = 0
        let z = Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(eval_builtin("phase", &[z]), 0.0, DimensionVector::ANGLE);
    }

    #[test]
    fn phase_complex_0_1_returns_pi_over_2() {
        // phase(0+1i) = π/2
        let z = Value::Complex {
            re: 0.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("phase", &[z]),
            std::f64::consts::FRAC_PI_2,
            DimensionVector::ANGLE
        );
    }

    #[test]
    fn phase_non_complex_returns_undef() {
        assert!(eval_builtin("phase", &[Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn phase_zero_complex_returns_undef() {
        // phase(0+0i) is mathematically undefined (zero vector has no direction)
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("phase", &[z]).is_undef(),
            "phase(0+0i) should be Undef, not Scalar{{0.0, ANGLE}}"
        );
    }

    #[test]
    fn phase_zero_dimensioned_complex_returns_undef() {
        // phase(Complex{0,0,LENGTH}) → Undef (dimensioned zero-vector).
        //
        // phase() is dimension-invariant by contract — the zero-vector guard fires
        // before dimension is ever consulted. Mirrors phase_zero_complex_returns_undef
        // but for the dimensioned (Scalar) branch, locking the invariant that a future
        // refactor which added a dimension-aware fast path cannot silently drop the
        // zero-vector guard on one branch.
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            eval_builtin("phase", &[z]).is_undef(),
            "phase(Complex{{0,0,LENGTH}}) should be Undef regardless of dimension"
        );
    }

    // ── arg() alias tests (step-3, task-3952) ────────────────────────────────

    #[test]
    fn arg_matches_phase_dimensionless() {
        // arg(z) must equal phase(z) exactly (same code path) for dimensionless z
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(
            eval_builtin("arg", &[z.clone()]),
            eval_builtin("phase", &[z]),
            "arg(z) must equal phase(z) for dimensionless Complex"
        );
    }

    #[test]
    fn arg_matches_phase_dimensioned() {
        // arg(z) must equal phase(z) for dimensioned Complex (phase is dimension-invariant)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(
            eval_builtin("arg", &[z.clone()]),
            eval_builtin("phase", &[z]),
            "arg(z) must equal phase(z) for dimensioned Complex"
        );
    }

    #[test]
    fn arg_complex_1_1_returns_pi_over_4() {
        // arg(1+1i) = atan2(1,1) = π/4, with ANGLE dimension
        let z = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("arg", &[z]),
            std::f64::consts::FRAC_PI_4,
            DimensionVector::ANGLE
        );
    }

    #[test]
    fn arg_zero_complex_returns_undef() {
        // arg(0+0i) is undefined — zero vector has no direction
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("arg", &[z]).is_undef(),
            "arg(0+0i) should be Undef, not Scalar{{0.0, ANGLE}}"
        );
    }

    #[test]
    fn arg_non_complex_returns_undef() {
        // arg on a non-Complex value must return Undef
        assert!(
            eval_builtin("arg", &[Value::Real(1.0)]).is_undef(),
            "arg(Real) should be Undef"
        );
    }

    // ── complex_add() tests (step-15) ─────────────────────────────────────────

    #[test]
    fn complex_add_dimensionless() {
        // complex_add(1+2i, 3+4i) = 4+6i
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("complex_add", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 4.0).abs() < 1e-12);
                assert!((im - 6.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{4,6,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_add_dimensioned_preserves_dimension() {
        // complex_add(a+bi [LENGTH], c+di [LENGTH]) = (a+c)+(b+d)i [LENGTH]
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        let result = eval_builtin("complex_add", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 4.0).abs() < 1e-12);
                assert!((im - 6.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{4,6,LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_add_dimension_mismatch_returns_undef() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(eval_builtin("complex_add", &[a, b]).is_undef());
    }

    #[test]
    fn complex_add_non_complex_arg_returns_undef() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(eval_builtin("complex_add", &[a, Value::Real(3.0)]).is_undef());
    }

    // ── complex_mul() tests (step-17) ─────────────────────────────────────────

    #[test]
    fn complex_mul_dimensionless() {
        // (1+2i)(3+4i) = (3-8) + (4+6)i = -5 + 10i
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("complex_mul", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - (-5.0)).abs() < 1e-12, "expected re=-5.0, got {}", re);
                assert!((im - 10.0).abs() < 1e-12, "expected im=10.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{-5,10,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_mul_dimensioned_combines_dimensions() {
        // complex_mul(LENGTH, LENGTH) → result dimension is LENGTH^2 (AREA)
        let area_dim = DimensionVector::LENGTH.mul(&DimensionVector::LENGTH);
        let a = Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 2.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let result = eval_builtin("complex_mul", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 2.0).abs() < 1e-12, "expected re=2.0, got {}", re);
                assert!((im - 0.0).abs() < 1e-12, "expected im=0.0, got {}", im);
                assert_eq!(dimension, area_dim, "expected AREA dimension");
            }
            other => panic!("expected Complex{{2,0,AREA}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_mul_non_complex_returns_undef() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(eval_builtin("complex_mul", &[a, Value::Real(3.0)]).is_undef());
    }

    // ── Complex<Impedance> integration test (step-19) ─────────────────────────

    #[test]
    fn complex_impedance_integration() {
        // Impedance = kg·m²·s⁻³·A⁻² = MASS·LENGTH²·TIME⁻³·CURRENT⁻²
        // Build as MASS * LENGTH^2 * TIME^-3 * CURRENT^-2
        use reify_core::DimensionVector;
        let mass_dim = DimensionVector::MASS;
        let length_dim = DimensionVector::LENGTH;
        let area = length_dim.mul(&length_dim);
        let mass_area = mass_dim.mul(&area);
        let time3 = DimensionVector::TIME.pow(3);
        let current2 = DimensionVector::CURRENT.pow(2);
        let impedance = mass_area.div(&time3).div(&current2);

        // Create 50 Ω (real part) + -25j Ω (imaginary part)
        let z = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 50.0,
                    dimension: impedance,
                },
                Value::Scalar {
                    si_value: -25.0,
                    dimension: impedance,
                },
            ],
        );
        match &z {
            Value::Complex { re, im, dimension } => {
                assert!((re - 50.0).abs() < 1e-12, "re={}", re);
                assert!((im - (-25.0)).abs() < 1e-12, "im={}", im);
                assert_eq!(*dimension, impedance);
            }
            other => panic!("expected Complex (impedance), got {:?}", other),
        }

        // re accessor → Scalar{50, IMPEDANCE}
        assert_scalar_approx!(
            eval_builtin("re", std::slice::from_ref(&z)),
            50.0,
            impedance
        );

        // im accessor → Scalar{-25, IMPEDANCE}
        assert_scalar_approx!(
            eval_builtin("im", std::slice::from_ref(&z)),
            -25.0,
            impedance
        );

        // magnitude → Scalar{sqrt(50²+25²), IMPEDANCE} = Scalar{sqrt(3125), IMPEDANCE}
        let expected_mag = (50.0_f64 * 50.0 + 25.0 * 25.0).sqrt();
        assert_scalar_approx!(
            eval_builtin("magnitude", std::slice::from_ref(&z)),
            expected_mag,
            impedance
        );

        // conjugate → Complex{50, 25, IMPEDANCE}
        let conj = eval_builtin("conjugate", std::slice::from_ref(&z));
        match &conj {
            Value::Complex { re, im, dimension } => {
                assert!((re - 50.0).abs() < 1e-12);
                assert!((im - 25.0).abs() < 1e-12);
                assert_eq!(*dimension, impedance);
            }
            other => panic!("expected conjugate Complex, got {:?}", other),
        }

        // phase → Scalar{atan2(-25, 50), ANGLE}
        let expected_phase = (-25.0_f64).atan2(50.0);
        assert_scalar_approx!(
            eval_builtin("phase", std::slice::from_ref(&z)),
            expected_phase,
            DimensionVector::ANGLE
        );
    }

    // ── Voltage dimension spec tests (step-7) ────────────────────────────────

    /// Build Voltage dimension: V = kg·m²·s⁻³·A⁻¹
    fn voltage_dim() -> DimensionVector {
        let mass = DimensionVector::MASS;
        let length = DimensionVector::LENGTH;
        let area = length.mul(&length);
        let mass_area = mass.mul(&area);
        let time3 = DimensionVector::TIME.pow(3);
        let current1 = DimensionVector::CURRENT.pow(1);
        mass_area.div(&time3).div(&current1)
    }

    #[test]
    fn complex_voltage_preserves_dimension() {
        // complex(Scalar{3,V}, Scalar{4,V}) → Complex{3,4,V}
        let v = voltage_dim();
        let z = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 3.0,
                    dimension: v,
                },
                Value::Scalar {
                    si_value: 4.0,
                    dimension: v,
                },
            ],
        );
        match &z {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12, "re={}", re);
                assert!((im - 4.0).abs() < 1e-12, "im={}", im);
                assert_eq!(*dimension, v, "dimension should be Voltage");
            }
            other => panic!("expected Complex{{3,4,V}}, got {:?}", other),
        }
    }

    #[test]
    fn real_voltage_returns_scalar() {
        // real(complex_voltage) → Scalar{3, V}
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        assert_scalar_approx!(eval_builtin("real", &[z]), 3.0, v);
    }

    #[test]
    fn imag_voltage_returns_scalar() {
        // imag(complex_voltage) → Scalar{4, V}
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        assert_scalar_approx!(eval_builtin("imag", &[z]), 4.0, v);
    }

    #[test]
    fn complex_magnitude_voltage() {
        // complex_magnitude(Complex{3,4,V}) → Scalar{5.0, V}
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        assert_scalar_approx!(eval_builtin("complex_magnitude", &[z]), 5.0, v);
    }

    #[test]
    fn conjugate_voltage_preserves_dim() {
        // conjugate flips im sign, preserves voltage dimension
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        let result = eval_builtin("conjugate", &[z]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12, "re={}", re);
                assert!((im - (-4.0)).abs() < 1e-12, "im={}", im);
                assert_eq!(dimension, v, "dimension should be Voltage");
            }
            other => panic!("expected Complex{{3,-4,V}}, got {:?}", other),
        }
    }

    // ── Dimension mismatch spec test (step-8) ─────────────────────────────────

    #[test]
    fn complex_voltage_current_mismatch_returns_undef() {
        // complex(Scalar{3, Voltage}, Scalar{4, Current}) → Undef (mismatched dims)
        let voltage = voltage_dim();
        // Current dimension: A (SI base, exponent 1 in CURRENT slot)
        let current = DimensionVector::CURRENT;
        let result = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 3.0,
                    dimension: voltage,
                },
                Value::Scalar {
                    si_value: 4.0,
                    dimension: current,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "expected Undef for V/A mismatch, got {:?}",
            result
        );
    }

    // ── Phase degree-equivalent spec test (step-9) ───────────────────────────

    #[test]
    fn phase_1_plus_i_approx_45_deg() {
        // phase(1+i) = atan2(1,1) = π/4 ≈ 0.7854 rad (45°)
        let z = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("phase", &[z]),
            std::f64::consts::FRAC_PI_4, // π/4 ≈ 0.7854 rad ≈ 45°
            DimensionVector::ANGLE
        );
    }

    #[test]
    fn complex_mul_overflow_returns_undef() {
        // (f64::MAX + f64::MAX*i) * (f64::MAX + f64::MAX*i)
        // re = MAX*MAX - MAX*MAX = 0 (actually NaN-ish), im = MAX*MAX + MAX*MAX = +Inf
        // Either component going Inf/NaN must produce Undef.
        let a = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("complex_mul", &[a, b]).is_undef(),
            "complex_mul with f64::MAX components must return Undef (Inf overflow)"
        );
    }

    #[test]
    fn complex_add_overflow_returns_undef() {
        // f64::MAX + f64::MAX = +Inf overflow
        let a = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("complex_add", &[a, b]).is_undef(),
            "complex_add with f64::MAX components must return Undef (Inf overflow)"
        );
    }

    // ── re/real sanitize_value tests (task-358 step-1) ─────────────────────────

    #[test]
    fn re_nan_re_component_returns_undef() {
        // re(Complex{NaN, 1.0, DIMLESS}) → Undef (NaN must not propagate)
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("re", &[z]).is_undef(),
            "re() with NaN real component must return Undef"
        );
    }

    #[test]
    fn re_inf_re_component_returns_undef() {
        // re(Complex{+Inf, 1.0, DIMLESS}) → Undef (Inf must not propagate)
        let z = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("re", &[z]).is_undef(),
            "re() with Inf real component must return Undef"
        );
    }

    #[test]
    fn re_nan_dimensioned_returns_undef() {
        // re(Complex{NaN, 1.0, LENGTH}) → Undef (dimensioned Scalar path)
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            eval_builtin("re", &[z]).is_undef(),
            "re() with NaN dimensioned real component must return Undef"
        );
    }

    #[test]
    fn real_nan_re_component_returns_undef() {
        // real(Complex{NaN, 1.0, DIMLESS}) → Undef (alias coverage)
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("real", &[z]).is_undef(),
            "real() with NaN real component must return Undef"
        );
    }

    // ── real() alias tests (step-1) ───────────────────────────────────────────

    #[test]
    fn real_dimensionless_returns_real() {
        // real(Complex{3,4,DIMLESS}) → Real(3.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("real", &[z]), 3.0);
    }

    #[test]
    fn real_dimensioned_returns_scalar() {
        // real(Complex{5,3,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("real", &[z]), 5.0, DimensionVector::LENGTH);
    }

    #[test]
    fn real_non_complex_returns_undef() {
        assert!(eval_builtin("real", &[Value::Real(3.0)]).is_undef());
    }

    // ── im/imag sanitize_value tests (task-358 step-3) ─────────────────────────

    #[test]
    fn im_nan_im_component_returns_undef() {
        // im(Complex{1.0, NaN, DIMLESS}) → Undef (NaN must not propagate)
        let z = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("im", &[z]).is_undef(),
            "im() with NaN imaginary component must return Undef"
        );
    }

    #[test]
    fn im_inf_im_component_returns_undef() {
        // im(Complex{1.0, +Inf, DIMLESS}) → Undef (Inf must not propagate)
        let z = Value::Complex {
            re: 1.0,
            im: f64::INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("im", &[z]).is_undef(),
            "im() with Inf imaginary component must return Undef"
        );
    }

    #[test]
    fn im_inf_dimensioned_returns_undef() {
        // im(Complex{1.0, +Inf, LENGTH}) → Undef (dimensioned Scalar path)
        let z = Value::Complex {
            re: 1.0,
            im: f64::INFINITY,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            eval_builtin("im", &[z]).is_undef(),
            "im() with Inf dimensioned imaginary component must return Undef"
        );
    }

    #[test]
    fn imag_nan_im_component_returns_undef() {
        // imag(Complex{1.0, NaN, DIMLESS}) → Undef (alias coverage)
        let z = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("imag", &[z]).is_undef(),
            "imag() with NaN imaginary component must return Undef"
        );
    }

    // ── imag() alias tests (step-3) ───────────────────────────────────────────

    #[test]
    fn imag_dimensionless_returns_real() {
        // imag(Complex{3,4,DIMLESS}) → Real(4.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("imag", &[z]), 4.0);
    }

    #[test]
    fn imag_dimensioned_returns_scalar() {
        // imag(Complex{5,3,LENGTH}) → Scalar{3.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("imag", &[z]), 3.0, DimensionVector::LENGTH);
    }

    #[test]
    fn imag_non_complex_returns_undef() {
        assert!(eval_builtin("imag", &[Value::Real(3.0)]).is_undef());
    }

    // ── magnitude / complex_magnitude edge-case tests: overflow, NaN, dimensioned ──

    /// Assert that evaluating `builtin` with a single `Complex { re, im, dimension }` argument
    /// returns `Value::Undef`. Panics with a descriptive message including the builtin name.
    fn assert_complex_builtin_undef(builtin: &str, re: f64, im: f64, dimension: DimensionVector) {
        let z = Value::Complex { re, im, dimension };
        assert!(
            eval_builtin(builtin, &[z]).is_undef(),
            "{builtin} with Complex{{re={re}, im={im}, dimension={dimension:?}}} must return Undef"
        );
    }

    #[test]
    fn complex_overflow_returns_undef_both_builtins() {
        // Both `magnitude` and `complex_magnitude` delegate to complex_abs for Complex
        // inputs; f64::MAX² + f64::MAX² overflows to +Inf; sanitize_value must catch it.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::MAX,
                f64::MAX,
                DimensionVector::DIMENSIONLESS,
            );
        }
    }

    #[test]
    fn complex_overflow_dimensioned_returns_undef_both_builtins() {
        // Same overflow but through the Scalar branch (non-dimensionless).
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(builtin, f64::MAX, f64::MAX, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_nan_component_returns_undef_both_builtins() {
        // A NaN component propagates through re.hypot(im) and sanitize_value catches it.
        for builtin in ["magnitude", "complex_magnitude"] {
            // re=NaN
            assert_complex_builtin_undef(builtin, f64::NAN, 1.0, DimensionVector::DIMENSIONLESS);
            // im=NaN (symmetric case)
            assert_complex_builtin_undef(builtin, 1.0, f64::NAN, DimensionVector::DIMENSIONLESS);
        }
    }

    #[test]
    fn complex_nan_dimensioned_returns_undef_both_builtins() {
        // NaN component with non-dimensionless input exercises the Value::Scalar arm of
        // sanitize_value (rather than Value::Real). Ensures the Scalar path is covered.
        for builtin in ["magnitude", "complex_magnitude"] {
            // re=NaN, im=1.0, LENGTH dimension → hits Scalar arm
            assert_complex_builtin_undef(builtin, f64::NAN, 1.0, DimensionVector::LENGTH);
            // im=NaN, re=1.0, LENGTH dimension → symmetric case
            assert_complex_builtin_undef(builtin, 1.0, f64::NAN, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_both_nan_returns_undef_both_builtins() {
        // hypot(NaN, NaN) = NaN per IEEE 754; test both dimensionless and dimensioned paths.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::NAN,
                f64::NAN,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, f64::NAN, f64::NAN, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_direct_infinity_returns_undef_both_builtins() {
        // Direct ±Infinity inputs (not computed overflow) are also caught by sanitize_value.
        // hypot(±Inf, x) = +Inf for any finite x per IEEE 754.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::INFINITY,
                0.0,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, f64::INFINITY, 0.0, DimensionVector::LENGTH);
            assert_complex_builtin_undef(
                builtin,
                0.0,
                f64::NEG_INFINITY,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, 0.0, f64::NEG_INFINITY, DimensionVector::LENGTH);
            // im=+Inf (symmetric of re=+Inf)
            assert_complex_builtin_undef(
                builtin,
                0.0,
                f64::INFINITY,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, 0.0, f64::INFINITY, DimensionVector::LENGTH);
            // re=-Inf (symmetric of im=-Inf)
            assert_complex_builtin_undef(
                builtin,
                f64::NEG_INFINITY,
                0.0,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, f64::NEG_INFINITY, 0.0, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_both_infinite_returns_undef_both_builtins() {
        // hypot(Inf, Inf) = +Inf per IEEE 754; sanitize_value must catch it.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::INFINITY,
                f64::INFINITY,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(
                builtin,
                f64::INFINITY,
                f64::INFINITY,
                DimensionVector::LENGTH,
            );
        }
    }

    // ── complex_magnitude() tests ─────────────────────────────────────────────

    #[test]
    fn complex_magnitude_3_4_returns_5() {
        // complex_magnitude(Complex{3,4,DIMLESS}) → Real(5.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("complex_magnitude", &[z]), 5.0);
    }

    #[test]
    fn complex_magnitude_dimensioned_returns_scalar() {
        // complex_magnitude(Complex{3,4,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("complex_magnitude", &[z]),
            5.0,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn complex_magnitude_non_complex_returns_undef() {
        // unlike generic magnitude which handles Tensors, complex_magnitude rejects non-Complex
        assert!(eval_builtin("complex_magnitude", &[Value::Real(5.0)]).is_undef());
    }

    #[test]
    fn complex_magnitude_large_representable_no_overflow() {
        // 1e200 is representable as f64, so |1e200 + 0i| = 1e200 must NOT overflow.
        // The naive (re*re + im*im).sqrt() formula fails because 1e200² = Inf.
        let z = Value::Complex {
            re: 1e200,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("complex_magnitude", &[z]), 1e200);
    }

    #[test]
    fn complex_magnitude_large_dimensioned_no_overflow() {
        // |1e200 + 0i| with LENGTH dimension must return Scalar{1e200, LENGTH}, not Undef.
        // Covers the dimensioned (Scalar) branch of complex_abs with large values.
        let z = Value::Complex {
            re: 1e200,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("complex_magnitude", &[z]),
            1e200,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn complex_magnitude_large_both_components() {
        // |1e200 + 1e200i| = 1e200 * sqrt(2) ≈ 1.4142e200, fully representable.
        // The naive formula fails because 1e200² + 1e200² overflows.
        let z = Value::Complex {
            re: 1e200,
            im: 1e200,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("complex_magnitude", &[z]);
        let expected = 1e200 * std::f64::consts::SQRT_2;
        match result {
            Value::Real(v) => {
                let rel_err = ((v - expected) / expected).abs();
                assert!(
                    rel_err < 1e-14,
                    "expected Real({expected}) got Real({v}), relative error {rel_err}"
                );
            }
            other => panic!("expected Real({expected}), got {other:?}"),
        }
    }
}
