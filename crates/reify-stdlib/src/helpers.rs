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
