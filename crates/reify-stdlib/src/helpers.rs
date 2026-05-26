use std::collections::BTreeMap;

use reify_types::{DimensionVector, Value, quaternion_is_finite};

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
// SYNC: mirror of reify-expr::sanitize_value — keep function AND tests in sync
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
    sanitize_value(Value::from_real_scalar(mag, dimension))
}

/// Compute the phase angle of a complex number: `atan2(im, re)`.
///
/// Returns `Value::Scalar { si_value: angle, dimension: ANGLE }` for ordinary
/// inputs, or `Value::Undef` for two edge cases handled by pre-guards:
///
/// 1. **Non-finite inputs (`NaN` or `±Inf`):** `atan2` applied to such inputs
///    often returns a *finite* value (e.g. `atan2(1.0, +Inf) = 0.0`), which
///    [`sanitize_value`] cannot detect as a poisoned input. The `is_finite`
///    pre-guard rejects these cases explicitly.
/// 2. **Zero vector `(0.0, 0.0)`:** `atan2(0.0, 0.0) = 0.0` is also finite,
///    so [`sanitize_value`] cannot distinguish this mathematically-undefined
///    case from a legitimate zero angle. The zero-vector pre-guard catches it.
///
/// After both pre-guards, `atan2(finite, finite)` with at least one non-zero
/// argument always yields a value in `[-π, π]`, so no output sanitization is
/// required.
///
/// Phase is dimension-invariant by contract — `atan2` on dimensioned components
/// still produces a dimensionless angle — so this helper takes only `re`/`im`
/// (not a dimension parameter) and always returns an `ANGLE`-dimensioned Scalar.
pub fn complex_phase(re: f64, im: f64) -> Value {
    if !re.is_finite() || !im.is_finite() {
        return Value::Undef;
    }
    if re == 0.0 && im == 0.0 {
        return Value::Undef;
    }
    let angle = im.atan2(re);
    Value::Scalar {
        si_value: angle,
        dimension: DimensionVector::ANGLE,
    }
}

/// Build a `Value::Map` with a `kind` discriminator field plus the given
/// extra fields.
///
/// Fields are inserted into a `BTreeMap`, which sorts them alphabetically.
/// The `kind` key is always included. Callers pass extra `(name, value)`
/// pairs in any order — alphabetical order is guaranteed by `BTreeMap`.
///
/// Takes `Vec<(&str, Value)>` so values are moved into the map (not cloned).
///
/// Hoisted from `loads::make_load_map` and `supports::make_support_map`,
/// which were byte-for-byte identical.
pub(crate) fn make_kind_map(kind: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String(kind.to_string()),
    );
    for (k, v) in fields {
        m.insert(Value::String(k.to_string()), v);
    }
    Value::Map(m)
}

/// Validate that `v` is a usable topology-selector target.
///
/// The topology-selector stdlib bindings (PRD `topology-selectors.md` task 5)
/// have not yet landed — there is no `Value::Face` / `Value::Edge` / `Value::Body`
/// variant today. Until those land, only two placeholder shapes are accepted:
///
/// - `Value::Map` — the canonical opaque-selector shape used by the existing
///   stub fixtures (e.g. a Map with `kind: "face_stub"`).
/// - `Value::String` — reserved for future named-selector sentinels, analogous
///   to `PressureLoad`'s `"normal"` direction sentinel.
///
/// Every other variant is rejected, including numeric primitives
/// (`Real`/`Int`/`Bool`/`Undef`) and dimensioned containers
/// (`Scalar`/`Complex`/`Vector`/`Tensor`/`Point`/`List`). The rejection of
/// dimensioned containers in particular catches a real misuse class — e.g.
/// the typo `point_load(force_vec, force_vec)` no longer silently embeds a
/// force-dimensioned Vector under the `point` field.
///
/// Full topology-kind validation (face-vs-edge-vs-body distinction with
/// source-span diagnostics) belongs in the FEA evaluation pipeline (PRD
/// task 16) once the engine resolves selectors against the kernel.
///
/// Returns `Some(())` when the value is an acceptable selector placeholder,
/// `None` otherwise.
pub(crate) fn validate_selector_target(v: &Value) -> Option<()> {
    match v {
        Value::Map(_) | Value::String(_) => Some(()),
        _ => None,
    }
}

/// Validate that `v` is a `Value::Scalar` with dimension matching `expected_dim`
/// and a finite SI value.
///
/// Returns `Some(si_value)` on success, `None` on any failure.
pub(crate) fn validate_dimensioned_scalar(v: &Value, expected_dim: DimensionVector) -> Option<f64> {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            if *dimension != expected_dim {
                return None;
            }
            if !si_value.is_finite() {
                return None;
            }
            Some(*si_value)
        }
        _ => None,
    }
}

/// Validate that `v` is a `Value::Vector` (or Tensor/Point) of exactly 3
/// numeric components with a consistent dimension matching `expected_dim`,
/// all finite.
///
/// Returns `Some(())` on success, `None` on any failure.
pub(crate) fn validate_dimensioned_vec3(v: &Value, expected_dim: DimensionVector) -> Option<()> {
    let (vals, dim) = tensor_components_f64(v)?;
    if vals.len() != 3 {
        return None;
    }
    if dim != expected_dim {
        return None;
    }
    if vals.iter().any(|x| !x.is_finite()) {
        return None;
    }
    Some(())
}

/// Validate that `v` is a `Value::Vector` (or Tensor/Point) of exactly 3
/// dimensionless components, all finite, with a non-zero, finite squared
/// magnitude — and return the raw (un-normalized) `[x, y, z]` components.
///
/// Returns `None` for any of:
/// - Non-Tensor/Vector/Point input (Real, Int, Bool, Undef, String, …) or empty container.
/// - Wrong arity (length ≠ 3).
/// - Non-DIMENSIONLESS dimension (e.g. LENGTH-dimensioned vector).
/// - Any non-finite component (NaN, ±Inf).
/// - Zero-magnitude vector `[0, 0, 0]`.
/// - Squared magnitude overflows to `+inf` (e.g. `[f64::MAX, 0, 0]`).
///
/// The `mag_sq.is_finite()` guard is required to catch overflow inputs like
/// `[f64::MAX, 0, 0]` whose squared magnitude is `+inf`. Without this guard
/// such inputs would be silently accepted as valid axes — the very
/// consistency hole this unified helper closes.
///
/// Used by `supports::eval_supports` (RollerSupport), `loads::validate_pressure_direction`
/// (non-sentinel branch), and `joints::validate_axis` (one-line wrapper preserved
/// for rustdoc cross-references and the 8 existing call sites).
pub(crate) fn validate_dimensionless_unit_axis_vec3(v: &Value) -> Option<[f64; 3]> {
    let (comps, dim) = tensor_components_f64(v)?;
    if comps.len() != 3 {
        return None;
    }
    if dim != DimensionVector::DIMENSIONLESS {
        return None;
    }
    let [x, y, z] = [comps[0], comps[1], comps[2]];
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    let mag_sq = x * x + y * y + z * z;
    if mag_sq == 0.0 || !mag_sq.is_finite() {
        return None;
    }
    Some([x, y, z])
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

    fn assert_extraction(
        input: &Value,
        expected_vals: &[f64],
        expected_dim: DimensionVector,
        label: &str,
    ) {
        let (vals, dim) = tensor_components_f64(input)
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
        assert_extraction(
            &v,
            &[1.0, 2.0, 3.0],
            DimensionVector::DIMENSIONLESS,
            "Vector of Reals",
        );
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
        assert_extraction(
            &v,
            &[0.5, 1.5],
            DimensionVector::LENGTH,
            "Point of LENGTH Scalars",
        );
    }

    #[test]
    fn tensor_components_f64_single_element_tensor_of_int_returns_value_and_dimensionless() {
        let v = Value::Tensor(vec![Value::Int(7)]);
        assert_extraction(
            &v,
            &[7.0],
            DimensionVector::DIMENSIONLESS,
            "single-element Tensor of Int",
        );
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
        assert_extraction(
            &v,
            &[1.5, 2.5],
            DimensionVector::MASS,
            "Vector of MASS Scalars",
        );
    }

    #[test]
    fn tensor_components_f64_tensor_of_reals_returns_values_and_dimensionless() {
        let v = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        assert_extraction(
            &v,
            &[1.0, 2.0, 3.0],
            DimensionVector::DIMENSIONLESS,
            "Tensor of Reals",
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
        assert_extraction(
            &v,
            &[0.1, 0.2, 0.3],
            DimensionVector::LENGTH,
            "Vector of LENGTH Scalars",
        );
    }

    #[test]
    fn assert_extraction_borrows_input_allowing_reuse() {
        // Passing &v must compile; after the call v must still be accessible.
        let v = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0)]);
        assert_extraction(
            &v,
            &[1.0, 2.0],
            DimensionVector::DIMENSIONLESS,
            "reuse test",
        );
        // v is still owned — reuse it here to prove it was not moved.
        assert!(format!("{:?}", v).contains("Real"));
    }

    #[test]
    fn assert_extraction_borrow_allows_reuse() {
        // Construct v as an owned binding so we can pass &v and still use v afterward.
        let v = Value::Vector(vec![Value::Real(1.0)]);
        assert_extraction(&v, &[1.0], DimensionVector::DIMENSIONLESS, "borrow-reuse");
        // Reusing v after the call is the whole point of the &Value signature.
        assert!(matches!(v, Value::Vector(_)));
    }

    // SYNC: sanitize_value tests mirrored in reify-expr::sanitize tests — keep in sync

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
        assert_eq!(
            sanitize_value(Value::Real(2.72)),
            Value::Real(2.72),
            "Real(2.72) must pass through bit-identical"
        );
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

    // ── validate_dimensionless_unit_axis_vec3 ─────────────────────────────────
    //
    // Unified helper hoisted from supports::validate_unit_axis_vec3,
    // joints::validate_axis, and the non-sentinel branch of
    // loads::validate_pressure_direction. Returns the raw (un-normalized)
    // [x, y, z] components on success; rejects wrong arity, wrong dimension,
    // non-finite components, zero magnitude, and squared-magnitude overflow
    // (e.g. `[f64::MAX, 0, 0]` whose `mag_sq` is `+inf`).

    #[test]
    fn validate_dimensionless_unit_axis_vec3_real_vector_returns_components() {
        let v = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let result = validate_dimensionless_unit_axis_vec3(&v);
        assert_eq!(
            result,
            Some([1.0, 2.0, 3.0]),
            "Vector of dimensionless Reals should return raw (un-normalized) components"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_dimensionless_tensor_returns_components() {
        let v = Value::Tensor(vec![Value::Real(0.5), Value::Real(-0.25), Value::Real(1.5)]);
        assert_eq!(
            validate_dimensionless_unit_axis_vec3(&v),
            Some([0.5, -0.25, 1.5]),
            "Tensor of dimensionless Reals should return raw components"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_dimensionless_point_returns_components() {
        let v = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert_eq!(
            validate_dimensionless_unit_axis_vec3(&v),
            Some([0.0, 0.0, 1.0]),
            "Point of dimensionless Reals should return raw components"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_vec2_returns_none() {
        let v = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0)]);
        assert!(
            validate_dimensionless_unit_axis_vec3(&v).is_none(),
            "Vec2 (wrong arity) should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_vec4_returns_none() {
        let v = Value::Vector(vec![
            Value::Real(1.0),
            Value::Real(2.0),
            Value::Real(3.0),
            Value::Real(4.0),
        ]);
        assert!(
            validate_dimensionless_unit_axis_vec3(&v).is_none(),
            "Vec4 (wrong arity) should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_nan_component_returns_none() {
        let v = Value::Vector(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);
        assert!(
            validate_dimensionless_unit_axis_vec3(&v).is_none(),
            "NaN component should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_pos_inf_component_returns_none() {
        let v = Value::Vector(vec![
            Value::Real(f64::INFINITY),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            validate_dimensionless_unit_axis_vec3(&v).is_none(),
            "+Inf component should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_neg_inf_component_returns_none() {
        let v = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(f64::NEG_INFINITY),
            Value::Real(0.0),
        ]);
        assert!(
            validate_dimensionless_unit_axis_vec3(&v).is_none(),
            "-Inf component should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_zero_vector_returns_none() {
        let v = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            validate_dimensionless_unit_axis_vec3(&v).is_none(),
            "Zero-magnitude vector should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_overflow_magnitude_returns_none() {
        // Regression: `[f64::MAX, 0.0, 0.0]` has `mag_sq` = f64::MAX^2 → +inf.
        // This is the consistency-hole assertion: the loads.rs
        // validate_pressure_direction copy on main does not guard against
        // `mag_sq.is_finite()` and silently accepts this input.
        let v = Value::Vector(vec![
            Value::Real(f64::MAX),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            validate_dimensionless_unit_axis_vec3(&v).is_none(),
            "[f64::MAX, 0, 0] (mag_sq overflow to +inf) should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_length_dimensioned_returns_none() {
        let v = Value::Vector(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert!(
            validate_dimensionless_unit_axis_vec3(&v).is_none(),
            "LENGTH-dimensioned vector should return None (not DIMENSIONLESS)"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_real_returns_none() {
        assert!(
            validate_dimensionless_unit_axis_vec3(&Value::Real(1.0)).is_none(),
            "Bare Real should return None (not a Tensor/Vector/Point)"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_int_returns_none() {
        assert!(
            validate_dimensionless_unit_axis_vec3(&Value::Int(1)).is_none(),
            "Int should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_bool_returns_none() {
        assert!(
            validate_dimensionless_unit_axis_vec3(&Value::Bool(true)).is_none(),
            "Bool should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_undef_returns_none() {
        assert!(
            validate_dimensionless_unit_axis_vec3(&Value::Undef).is_none(),
            "Undef should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_string_returns_none() {
        assert!(
            validate_dimensionless_unit_axis_vec3(&Value::String("normal".to_string())).is_none(),
            "String should return None (no sentinel handling at this layer)"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_empty_vector_returns_none() {
        assert!(
            validate_dimensionless_unit_axis_vec3(&Value::Vector(vec![])).is_none(),
            "Empty Vector should return None"
        );
    }

    #[test]
    fn validate_dimensionless_unit_axis_vec3_mixed_dimensions_returns_none() {
        // Locks the contract at the helper boundary: mixed dimensions
        // (here `[Real, Scalar<LENGTH>, Real]`) are rejected. The underlying
        // `tensor_components_f64` enforces dimension consistency, but we
        // assert it here too so the contract cannot drift if the upstream
        // helper changes its rejection policy.
        let v = Value::Vector(vec![
            Value::Real(1.0),
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Real(0.0),
        ]);
        assert!(
            validate_dimensionless_unit_axis_vec3(&v).is_none(),
            "Mixed-dimension Vector should return None"
        );
    }

    // ── validate_selector_target ──────────────────────────────────────────────
    //
    // Hoisted from supports.rs and loads.rs (byte-for-byte identical bodies).
    // Narrowed to accept only Value::Map and Value::String as placeholder
    // selector shapes; all other variants (including dimensioned containers
    // like Vector/Tensor/Scalar/Point/Complex) are rejected.

    #[test]
    fn validate_selector_target_real_returns_none() {
        assert!(
            validate_selector_target(&Value::Real(0.0)).is_none(),
            "Value::Real should be rejected as a selector target"
        );
    }

    #[test]
    fn validate_selector_target_int_returns_none() {
        assert!(
            validate_selector_target(&Value::Int(0)).is_none(),
            "Value::Int should be rejected as a selector target"
        );
    }

    #[test]
    fn validate_selector_target_bool_returns_none() {
        assert!(
            validate_selector_target(&Value::Bool(true)).is_none(),
            "Value::Bool should be rejected as a selector target"
        );
    }

    #[test]
    fn validate_selector_target_undef_returns_none() {
        assert!(
            validate_selector_target(&Value::Undef).is_none(),
            "Value::Undef should be rejected as a selector target"
        );
    }

    #[test]
    fn validate_selector_target_empty_map_accepted() {
        use std::collections::BTreeMap;
        assert_eq!(
            validate_selector_target(&Value::Map(BTreeMap::new())),
            Some(()),
            "Empty Value::Map should be accepted as opaque selector"
        );
    }

    #[test]
    fn validate_selector_target_empty_list_rejected() {
        assert!(
            validate_selector_target(&Value::List(vec![])).is_none(),
            "Value::List is no longer accepted — placeholder selectors must be Map or String"
        );
    }

    #[test]
    fn validate_selector_target_string_accepted() {
        assert_eq!(
            validate_selector_target(&Value::String("x".to_string())),
            Some(()),
            "Value::String should be accepted as opaque selector"
        );
    }

    #[test]
    fn validate_selector_target_arbitrary_string_accepted() {
        // Pins the documented intent that *any* String is accepted — not just
        // a known whitelist — as a placeholder shape until the topology-selector
        // PRD task 5 introduces named-selector sentinels (e.g. "face1").
        // The breadth is intentional: unlike dimensioned containers (Scalar,
        // Vector, …), a String cannot be confused with a numeric typo, so
        // accepting an arbitrary sentinel string imposes no safety risk.
        // If a whitelist is introduced later, these tests must be tightened.
        assert_eq!(
            validate_selector_target(&Value::String("face1".to_string())),
            Some(()),
            "Arbitrary face-selector String should be accepted as placeholder"
        );
        assert_eq!(
            validate_selector_target(&Value::String("body_all".to_string())),
            Some(()),
            "Arbitrary body-selector String should be accepted as placeholder"
        );
    }

    #[test]
    fn validate_selector_target_empty_vector_rejected() {
        assert!(
            validate_selector_target(&Value::Vector(vec![])).is_none(),
            "Value::Vector is no longer accepted — placeholder selectors must be Map or String"
        );
    }

    #[test]
    fn validate_selector_target_empty_tensor_rejected() {
        assert!(
            validate_selector_target(&Value::Tensor(vec![])).is_none(),
            "Value::Tensor is no longer accepted — placeholder selectors must be Map or String"
        );
    }

    #[test]
    fn validate_selector_target_scalar_rejected() {
        // Lock the typo class at helper level: a force-dimensioned Scalar fed as
        // a selector returns None (narrowed contract).
        assert!(
            validate_selector_target(&Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::FORCE,
            })
            .is_none(),
            "Value::Scalar (force-dimensioned) should be rejected as selector"
        );
    }

    #[test]
    fn validate_selector_target_point_rejected() {
        assert!(
            validate_selector_target(&Value::Point(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ]))
            .is_none(),
            "Value::Point should be rejected as selector"
        );
    }

    #[test]
    fn validate_selector_target_complex_rejected() {
        assert!(
            validate_selector_target(&Value::Complex {
                re: 0.0,
                im: 0.0,
                dimension: DimensionVector::DIMENSIONLESS,
            })
            .is_none(),
            "Value::Complex should be rejected as selector"
        );
    }

    #[test]
    fn validate_selector_target_vector_with_content_rejected() {
        // Helper-level analog of the user-visible `point_load(force_vec, force_vec)`
        // typo case: a FORCE-dimensioned 3-vector fed as a selector is rejected.
        let v = Value::Vector(vec![
            Value::Scalar {
                si_value: 5000.0,
                dimension: DimensionVector::FORCE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::FORCE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::FORCE,
            },
        ]);
        assert!(
            validate_selector_target(&v).is_none(),
            "FORCE-dimensioned Vector should be rejected as selector"
        );
    }

    // ── validate_dimensioned_vec3 ─────────────────────────────────────────────
    //
    // Hoisted from supports.rs and loads.rs (byte-for-byte identical bodies).
    // Validates a 3-component vector with a specified expected dimension.

    fn make_scalar_vec3_local(vals: [f64; 3], dim: DimensionVector) -> Value {
        Value::Vector(
            vals.iter()
                .map(|&v| Value::Scalar {
                    si_value: v,
                    dimension: dim,
                })
                .collect(),
        )
    }

    #[test]
    fn validate_dimensioned_vec3_length_happy_path_returns_some() {
        let v = make_scalar_vec3_local([1.0, 2.0, 3.0], DimensionVector::LENGTH);
        assert_eq!(
            validate_dimensioned_vec3(&v, DimensionVector::LENGTH),
            Some(()),
            "3-component LENGTH vector with expected_dim=LENGTH should accept"
        );
    }

    #[test]
    fn validate_dimensioned_vec3_force_happy_path_returns_some() {
        let v = make_scalar_vec3_local([10.0, 20.0, 30.0], DimensionVector::FORCE);
        assert_eq!(
            validate_dimensioned_vec3(&v, DimensionVector::FORCE),
            Some(()),
            "3-component FORCE vector with expected_dim=FORCE should accept"
        );
    }

    #[test]
    fn validate_dimensioned_vec3_wrong_dimension_returns_none() {
        let v = make_scalar_vec3_local([1.0, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            validate_dimensioned_vec3(&v, DimensionVector::LENGTH).is_none(),
            "FORCE vector with expected_dim=LENGTH should reject"
        );
    }

    #[test]
    fn validate_dimensioned_vec3_vec2_returns_none() {
        let v = Value::Vector(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert!(
            validate_dimensioned_vec3(&v, DimensionVector::LENGTH).is_none(),
            "Vec2 (wrong arity) should return None"
        );
    }

    #[test]
    fn validate_dimensioned_vec3_vec4_returns_none() {
        let v = Value::Vector(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert!(
            validate_dimensioned_vec3(&v, DimensionVector::LENGTH).is_none(),
            "Vec4 (wrong arity) should return None"
        );
    }

    #[test]
    fn validate_dimensioned_vec3_nan_component_returns_none() {
        let v = make_scalar_vec3_local([f64::NAN, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(
            validate_dimensioned_vec3(&v, DimensionVector::LENGTH).is_none(),
            "NaN component should return None"
        );
    }

    #[test]
    fn validate_dimensioned_vec3_inf_component_returns_none() {
        let v = make_scalar_vec3_local([f64::INFINITY, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(
            validate_dimensioned_vec3(&v, DimensionVector::LENGTH).is_none(),
            "+Inf component should return None"
        );
    }

    #[test]
    fn validate_dimensioned_vec3_dimensionless_vs_length_returns_none() {
        let v = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            validate_dimensioned_vec3(&v, DimensionVector::LENGTH).is_none(),
            "DIMENSIONLESS vector with expected_dim=LENGTH should reject"
        );
    }

    #[test]
    fn validate_dimensioned_vec3_real_returns_none() {
        assert!(
            validate_dimensioned_vec3(&Value::Real(1.0), DimensionVector::LENGTH).is_none(),
            "Bare Real should return None"
        );
    }

    #[test]
    fn validate_dimensioned_vec3_undef_returns_none() {
        assert!(
            validate_dimensioned_vec3(&Value::Undef, DimensionVector::LENGTH).is_none(),
            "Undef should return None"
        );
    }

    // ── validate_dimensioned_scalar ───────────────────────────────────────────
    //
    // Hoisted from loads.rs. Validates a single Scalar value with a specified
    // expected dimension and a finite SI value.

    #[test]
    fn validate_dimensioned_scalar_force_happy_path_returns_si_value() {
        let v = Value::Scalar {
            si_value: 5000.0,
            dimension: DimensionVector::FORCE,
        };
        assert_eq!(
            validate_dimensioned_scalar(&v, DimensionVector::FORCE),
            Some(5000.0),
            "FORCE scalar with expected_dim=FORCE should return Some(si_value)"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_pressure_happy_path_returns_si_value() {
        let v = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        assert_eq!(
            validate_dimensioned_scalar(&v, DimensionVector::PRESSURE),
            Some(5e6),
            "PRESSURE scalar with expected_dim=PRESSURE should return Some(si_value)"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_wrong_dimension_returns_none() {
        let v = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::FORCE,
        };
        assert!(
            validate_dimensioned_scalar(&v, DimensionVector::LENGTH).is_none(),
            "FORCE scalar with expected_dim=LENGTH should return None"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_dimensionless_vs_length_returns_none() {
        let v = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            validate_dimensioned_scalar(&v, DimensionVector::LENGTH).is_none(),
            "DIMENSIONLESS scalar with expected_dim=LENGTH should return None"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_nan_si_value_returns_none() {
        let v = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::ACCELERATION,
        };
        assert!(
            validate_dimensioned_scalar(&v, DimensionVector::ACCELERATION).is_none(),
            "NaN si_value should return None"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_pos_inf_si_value_returns_none() {
        let v = Value::Scalar {
            si_value: f64::INFINITY,
            dimension: DimensionVector::FORCE,
        };
        assert!(
            validate_dimensioned_scalar(&v, DimensionVector::FORCE).is_none(),
            "+Inf si_value should return None"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_neg_inf_si_value_returns_none() {
        let v = Value::Scalar {
            si_value: f64::NEG_INFINITY,
            dimension: DimensionVector::FORCE,
        };
        assert!(
            validate_dimensioned_scalar(&v, DimensionVector::FORCE).is_none(),
            "-Inf si_value should return None"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_real_returns_none() {
        assert!(
            validate_dimensioned_scalar(&Value::Real(1.0), DimensionVector::LENGTH).is_none(),
            "Bare Real should return None"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_int_returns_none() {
        assert!(
            validate_dimensioned_scalar(&Value::Int(7), DimensionVector::LENGTH).is_none(),
            "Int should return None"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_bool_returns_none() {
        assert!(
            validate_dimensioned_scalar(&Value::Bool(true), DimensionVector::LENGTH).is_none(),
            "Bool should return None"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_undef_returns_none() {
        assert!(
            validate_dimensioned_scalar(&Value::Undef, DimensionVector::LENGTH).is_none(),
            "Undef should return None"
        );
    }

    #[test]
    fn validate_dimensioned_scalar_vector_returns_none() {
        let v = Value::Vector(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::FORCE,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::FORCE,
            },
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::FORCE,
            },
        ]);
        assert!(
            validate_dimensioned_scalar(&v, DimensionVector::FORCE).is_none(),
            "Vector (non-Scalar container) should return None"
        );
    }

    // ── make_kind_map ─────────────────────────────────────────────────────────
    //
    // Hoisted from supports.rs (make_support_map) and loads.rs (make_load_map),
    // which were byte-for-byte identical. Builds a Value::Map with a `kind`
    // discriminator key plus extra fields, all sorted alphabetically by BTreeMap.

    #[test]
    fn make_kind_map_kind_field_matches_input_string() {
        let result = make_kind_map("my_kind", vec![]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("my_kind".to_string())),
            "kind field should equal the input string"
        );
    }

    #[test]
    fn make_kind_map_extra_fields_appear_under_expected_keys() {
        let result = make_kind_map(
            "test",
            vec![("alpha", Value::Real(1.0)), ("beta", Value::Int(42))],
        );
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("alpha".to_string())),
            Some(&Value::Real(1.0)),
            "alpha field should round-trip"
        );
        assert_eq!(
            map.get(&Value::String("beta".to_string())),
            Some(&Value::Int(42)),
            "beta field should round-trip"
        );
    }

    #[test]
    fn make_kind_map_btreemap_orders_keys_alphabetically() {
        // Insert in non-alpha order: zulu, alpha, mike. BTreeMap sorts.
        let result = make_kind_map(
            "test",
            vec![
                ("zulu", Value::Int(3)),
                ("alpha", Value::Int(1)),
                ("mike", Value::Int(2)),
            ],
        );
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        // Iteration order on BTreeMap<Value, Value> follows Value's Ord impl.
        // For Value::String("alpha") < Value::String("kind") < Value::String("mike")
        // < Value::String("test") < Value::String("zulu"), the iteration order
        // is alpha, kind, mike, zulu.
        let keys: Vec<&Value> = map.keys().collect();
        let expected = [
            Value::String("alpha".to_string()),
            Value::String("kind".to_string()),
            Value::String("mike".to_string()),
            Value::String("zulu".to_string()),
        ];
        let expected_refs: Vec<&Value> = expected.iter().collect();
        assert_eq!(
            keys, expected_refs,
            "BTreeMap should iterate keys in alphabetical order"
        );
    }

    #[test]
    fn make_kind_map_empty_fields_produces_only_kind_key() {
        let result = make_kind_map("only_kind", vec![]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.len(),
            1,
            "Empty fields should produce a Map with only the kind key"
        );
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("only_kind".to_string())),
        );
    }
}
