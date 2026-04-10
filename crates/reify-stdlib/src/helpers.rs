use reify_types::{DimensionVector, Value};

/// Apply a function to a single argument (by reference, for pattern matching).
pub(crate) fn unary(args: &[Value], f: impl FnOnce(&Value) -> Value) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    f(&args[0])
}

/// Apply a function to two arguments (by reference).
pub(crate) fn binary(args: &[Value], f: impl FnOnce(&Value, &Value) -> Value) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    f(&args[0], &args[1])
}

/// Apply a function to three arguments (by reference).
pub(crate) fn ternary(args: &[Value], f: impl FnOnce(&Value, &Value, &Value) -> Value) -> Value {
    if args.len() != 3 {
        return Value::Undef;
    }
    f(&args[0], &args[1], &args[2])
}

/// Apply a function to a single f64 argument (extracted from any numeric Value).
pub(crate) fn unary_f64(args: &[Value], f: impl FnOnce(f64) -> Value) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    match args[0].as_f64() {
        Some(x) => sanitize_value(f(x)),
        None => Value::Undef,
    }
}

/// Apply a function to two f64 arguments.
pub(crate) fn binary_f64(args: &[Value], f: impl FnOnce(f64, f64) -> Value) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    match (args[0].as_f64(), args[1].as_f64()) {
        (Some(x), Some(y)) => sanitize_value(f(x, y)),
        _ => Value::Undef,
    }
}

/// Apply a function to five f64 arguments (extracted via `as_f64()`).
///
/// Returns `Undef` on wrong argument count or extraction failure.
/// Applies `sanitize_value` to the result.
pub(crate) fn quinary_f64(
    args: &[Value],
    f: impl FnOnce(f64, f64, f64, f64, f64) -> Value,
) -> Value {
    if args.len() != 5 {
        return Value::Undef;
    }
    match (
        args[0].as_f64(),
        args[1].as_f64(),
        args[2].as_f64(),
        args[3].as_f64(),
        args[4].as_f64(),
    ) {
        (Some(a), Some(b), Some(c), Some(d), Some(e)) => sanitize_value(f(a, b, c, d, e)),
        _ => Value::Undef,
    }
}

/// Convert non-finite f64 values (NaN, inf) to Undef.
///
/// This is a defense-in-depth catch-all applied at the return point of
/// `unary_f64` and `binary_f64` to ensure domain errors (e.g., sqrt(-1),
/// log(0), exp(1000) overflow) produce Undef instead of silently propagating
/// NaN or infinity through the evaluation graph.
// SYNC: mirror of reify-expr::sanitize_value — keep in sync
pub(crate) fn sanitize_value(v: Value) -> Value {
    match &v {
        Value::Real(x) if !x.is_finite() => Value::Undef,
        Value::Scalar { si_value, .. } if !si_value.is_finite() => Value::Undef,
        Value::Complex { re, im, .. } if !re.is_finite() || !im.is_finite() => Value::Undef,
        Value::Orientation { w, x, y, z }
            if !w.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite() =>
        {
            Value::Undef
        }
        _ => v,
    }
}

/// Extract radians from a trig function argument.
/// Accepts: Angle Scalar (si_value is already radians) or bare Real (treated as radians).
/// Rejects: non-ANGLE Scalar (dimension error).
pub(crate) fn trig_input(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            if *dimension == DimensionVector::ANGLE && si_value.is_finite() {
                Some(*si_value)
            } else {
                None // dimension error or non-finite value
            }
        }
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

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

/// Extract numeric components and consistent dimension from a Tensor value.
///
/// Returns `Some((values, dimension))` if:
/// - `v` is a `Value::Tensor`, `Value::Point`, or `Value::Vector` with at least one element.
/// - All components support `as_f64()`.
/// - All components share the same dimension (or all are dimensionless).
///
/// Returns `None` for non-Tensor/Point/Vector values, empty containers, non-numeric
/// components, or containers with mixed dimensions.
pub(crate) fn tensor_components_f64(v: &Value) -> Option<(Vec<f64>, DimensionVector)> {
    let items = match v {
        Value::Tensor(items) | Value::Point(items) | Value::Vector(items) if !items.is_empty() => {
            items
        }
        _ => return None,
    };
    let first_dim = items[0].dimension();
    let mut vals = Vec::with_capacity(items.len());
    for item in items {
        if item.dimension() != first_dim {
            return None; // mixed dimensions
        }
        match item.as_f64() {
            Some(x) => vals.push(x),
            None => return None, // non-numeric component
        }
    }
    Some((vals, first_dim))
}

// SYNC: mirror of reify-expr::sanitize.rs tests — keep in sync
#[cfg(test)]
mod tests {
    use reify_types::DimensionVector;

    use super::*;

    fn assert_extraction(input: Value, expected_vals: &[f64], expected_dim: DimensionVector, label: &str) {
        let (vals, dim) = tensor_components_f64(&input)
            .unwrap_or_else(|| panic!("{}: expected Some but got None", label));
        assert_eq!(
            vals.len(),
            expected_vals.len(),
            "{}: expected {} components but got {}",
            label,
            expected_vals.len(),
            vals.len()
        );
        for (i, (&actual, &expected)) in vals.iter().zip(expected_vals.iter()).enumerate() {
            assert!(
                (actual - expected).abs() < f64::EPSILON,
                "{}: vals[{}] expected {} but got {}",
                label,
                i,
                expected,
                actual
            );
        }
        assert_eq!(dim, expected_dim, "{}: dimension mismatch", label);
    }

    // ── tensor_components_f64 rejection: non-container types ─────────────────

    #[test]
    fn tensor_components_f64_real_returns_none() {
        assert!(
            tensor_components_f64(&Value::Real(1.0)).is_none(),
            "Real value should return None"
        );
    }

    #[test]
    fn tensor_components_f64_int_returns_none() {
        assert!(
            tensor_components_f64(&Value::Int(42)).is_none(),
            "Int value should return None"
        );
    }

    #[test]
    fn tensor_components_f64_undef_returns_none() {
        assert!(
            tensor_components_f64(&Value::Undef).is_none(),
            "Undef value should return None"
        );
    }

    #[test]
    fn tensor_components_f64_bool_returns_none() {
        assert!(
            tensor_components_f64(&Value::Bool(true)).is_none(),
            "Bool value should return None"
        );
    }

    #[test]
    fn tensor_components_f64_string_returns_none() {
        assert!(
            tensor_components_f64(&Value::String("hello".to_string())).is_none(),
            "String value should return None"
        );
    }

    #[test]
    fn tensor_components_f64_list_returns_none() {
        assert!(
            tensor_components_f64(&Value::List(vec![Value::Real(1.0)])).is_none(),
            "List value should return None"
        );
    }

    // ── tensor_components_f64 rejection: empty containers ────────────────────

    #[test]
    fn tensor_components_f64_empty_tensor_returns_none() {
        assert!(
            tensor_components_f64(&Value::Tensor(vec![])).is_none(),
            "Empty Tensor should return None"
        );
    }

    #[test]
    fn tensor_components_f64_empty_point_returns_none() {
        assert!(
            tensor_components_f64(&Value::Point(vec![])).is_none(),
            "Empty Point should return None"
        );
    }

    #[test]
    fn tensor_components_f64_empty_vector_returns_none() {
        assert!(
            tensor_components_f64(&Value::Vector(vec![])).is_none(),
            "Empty Vector should return None"
        );
    }

    // ── tensor_components_f64 rejection: non-numeric components ──────────────

    #[test]
    fn tensor_components_f64_vector_with_string_component_returns_none() {
        let v = Value::Vector(vec![Value::String("x".to_string())]);
        assert!(
            tensor_components_f64(&v).is_none(),
            "Vector containing a String component should return None"
        );
    }

    #[test]
    fn tensor_components_f64_tensor_with_bool_component_returns_none() {
        let v = Value::Tensor(vec![Value::Bool(true)]);
        assert!(
            tensor_components_f64(&v).is_none(),
            "Tensor containing a Bool component should return None"
        );
    }

    #[test]
    fn tensor_components_f64_vector_with_complex_component_returns_none() {
        let v = Value::Vector(vec![Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        }]);
        assert!(
            tensor_components_f64(&v).is_none(),
            "Vector containing a Complex component should return None"
        );
    }

    // ── tensor_components_f64 rejection: mixed dimensions ────────────────────

    #[test]
    fn tensor_components_f64_vector_mixed_dimensionless_and_length_returns_none() {
        // First element is dimensionless (Real), second is LENGTH (Scalar).
        let v = Value::Vector(vec![
            Value::Real(1.0),
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert!(
            tensor_components_f64(&v).is_none(),
            "Vector mixing DIMENSIONLESS and LENGTH should return None"
        );
    }

    #[test]
    fn tensor_components_f64_tensor_mixed_length_and_mass_returns_none() {
        let v = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::MASS,
            },
        ]);
        assert!(
            tensor_components_f64(&v).is_none(),
            "Tensor mixing LENGTH and MASS should return None"
        );
    }

    // ── tensor_components_f64 success: valid extraction paths ────────────────

    #[test]
    fn tensor_components_f64_vector_of_reals_returns_values_and_dimensionless() {
        let v = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let (vals, dim) =
            tensor_components_f64(&v).expect("expected Some for Vector of Reals");
        assert_eq!(vals.len(), 3, "should extract 3 components");
        assert!((vals[0] - 1.0).abs() < f64::EPSILON);
        assert!((vals[1] - 2.0).abs() < f64::EPSILON);
        assert!((vals[2] - 3.0).abs() < f64::EPSILON);
        assert_eq!(dim, DimensionVector::DIMENSIONLESS, "Real elements are dimensionless");
    }

    #[test]
    fn tensor_components_f64_point_of_length_scalars_returns_values_and_length() {
        let v = Value::Point(vec![
            Value::Scalar {
                si_value: 0.5,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 1.5,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let (vals, dim) =
            tensor_components_f64(&v).expect("expected Some for Point of LENGTH Scalars");
        assert_eq!(vals.len(), 2, "should extract 2 components");
        assert!((vals[0] - 0.5).abs() < f64::EPSILON);
        assert!((vals[1] - 1.5).abs() < f64::EPSILON);
        assert_eq!(dim, DimensionVector::LENGTH, "Scalar{{LENGTH}} elements have LENGTH dimension");
    }

    #[test]
    fn tensor_components_f64_single_element_tensor_of_int_returns_value_and_dimensionless() {
        let v = Value::Tensor(vec![Value::Int(7)]);
        let (vals, dim) =
            tensor_components_f64(&v).expect("expected Some for single-element Tensor of Int");
        assert_eq!(vals.len(), 1, "should extract 1 component");
        assert!((vals[0] - 7.0).abs() < f64::EPSILON, "Int(7) should become 7.0_f64");
        assert_eq!(dim, DimensionVector::DIMENSIONLESS, "Int elements are dimensionless");
    }

    #[test]
    fn tensor_components_f64_vector_of_mass_scalars_returns_values_and_mass() {
        let v = Value::Vector(vec![
            Value::Scalar {
                si_value: 1.5,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 2.5,
                dimension: DimensionVector::MASS,
            },
        ]);
        let (vals, dim) =
            tensor_components_f64(&v).expect("expected Some for Vector of MASS Scalars");
        assert_eq!(vals.len(), 2, "should extract 2 components");
        assert!(
            (vals[0] - 1.5).abs() < f64::EPSILON,
            "first component should be 1.5"
        );
        assert!(
            (vals[1] - 2.5).abs() < f64::EPSILON,
            "second component should be 2.5"
        );
        assert_eq!(
            dim,
            DimensionVector::MASS,
            "Scalar{{MASS}} elements have MASS dimension"
        );
    }

    #[test]
    fn tensor_components_f64_tensor_of_reals_returns_values_and_dimensionless() {
        let v = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let (vals, dim) =
            tensor_components_f64(&v).expect("expected Some for Tensor of Reals");
        assert_eq!(vals.len(), 3, "should extract 3 components");
        assert!((vals[0] - 1.0).abs() < f64::EPSILON);
        assert!((vals[1] - 2.0).abs() < f64::EPSILON);
        assert!((vals[2] - 3.0).abs() < f64::EPSILON);
        assert_eq!(
            dim,
            DimensionVector::DIMENSIONLESS,
            "Real elements are dimensionless"
        );
    }

    #[test]
    fn tensor_components_f64_vector_of_length_scalars_returns_values_and_length() {
        let v = Value::Vector(vec![
            Value::Scalar {
                si_value: 0.1,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.2,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.3,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let (vals, dim) =
            tensor_components_f64(&v).expect("expected Some for Vector of LENGTH Scalars");
        assert_eq!(vals.len(), 3, "should extract 3 components");
        assert!((vals[0] - 0.1).abs() < f64::EPSILON);
        assert!((vals[1] - 0.2).abs() < f64::EPSILON);
        assert!((vals[2] - 0.3).abs() < f64::EPSILON);
        assert_eq!(
            dim,
            DimensionVector::LENGTH,
            "Scalar{{LENGTH}} elements have LENGTH dimension"
        );
    }

    // SYNC: sanitize_value Real/Scalar tests mirrored in reify-expr::sanitize tests; Complex/Orientation arms in crate::complex tests — keep in sync

    // ── sanitize_value Real arm characterization tests ───────────────────────

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

    // ── sanitize_value Scalar arm characterization tests ─────────────────────

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

    // ── sanitize_value Complex arm characterization tests ─────────────────────

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

    // ── sanitize_value Orientation arm characterization tests ─────────────────

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
