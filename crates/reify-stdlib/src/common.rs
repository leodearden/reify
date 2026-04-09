//! Shared production helpers used across all domain submodules.
//!
//! All functions are `pub(crate)` so domain modules can import them via
//! `use crate::common::*;`.

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

/// Returns true iff `lo` and `hi` form a valid (non-NaN, non-inverted) range.
///
/// Used by clamp Real/Scalar/fallback arms instead of inline `lo.is_nan() || hi.is_nan() || lo > hi`.
pub(crate) fn valid_f64_range(lo: f64, hi: f64) -> bool {
    !lo.is_nan() && !hi.is_nan() && lo <= hi
}

/// Linear interpolation: `a + t * (b - a)`.
pub(crate) fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + t * (b - a)
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

/// Extract numeric components and consistent dimension from a Tensor/Point/Vector value.
///
/// Returns `Some((values, dimension))` if:
/// - `v` is a `Value::Tensor`, `Value::Point`, or `Value::Vector` with at least one element.
/// - All components support `as_f64()`.
/// - All components share the same dimension (or all are dimensionless).
///
/// Returns `None` for other variants, empty containers, non-numeric
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

/// Compute the Euclidean norm (magnitude) of a 3D vector.
///
/// Pure mathematical function — callers are responsible for checking finiteness
/// of the result if needed.
#[inline]
pub(crate) fn vec3_norm(x: f64, y: f64, z: f64) -> f64 {
    (x * x + y * y + z * z).sqrt()
}

/// Normalize a quaternion (w, x, y, z) to unit length.
///
/// Returns `None` if any component is non-finite or the quaternion has zero length.
pub(crate) fn normalize_quaternion(w: f64, x: f64, y: f64, z: f64) -> Option<Value> {
    if !w.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    let norm = (w * w + x * x + y * y + z * z).sqrt();
    if norm < f64::EPSILON {
        return None;
    }
    Some(Value::Orientation {
        w: w / norm,
        x: x / norm,
        y: y / norm,
        z: z / norm,
    })
}

/// Create an elementary rotation quaternion for a single axis.
///
/// `axis`: 0=X, 1=Y, 2=Z. `angle`: rotation in radians.
/// Returns (w, x, y, z) quaternion.
pub(crate) fn elementary_rotation_quat(axis: usize, angle: f64) -> (f64, f64, f64, f64) {
    let half = angle / 2.0;
    let c = half.cos();
    let s = half.sin();
    match axis {
        0 => (c, s, 0.0, 0.0),
        1 => (c, 0.0, s, 0.0),
        2 => (c, 0.0, 0.0, s),
        _ => (1.0, 0.0, 0.0, 0.0), // identity fallback
    }
}

/// Hamilton product of two quaternions.
pub(crate) fn quat_mul(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    (
        a.0 * b.0 - a.1 * b.1 - a.2 * b.2 - a.3 * b.3,
        a.0 * b.1 + a.1 * b.0 + a.2 * b.3 - a.3 * b.2,
        a.0 * b.2 - a.1 * b.3 + a.2 * b.0 + a.3 * b.1,
        a.0 * b.3 + a.1 * b.2 - a.2 * b.1 + a.3 * b.0,
    )
}

/// Conjugate of a unit quaternion (equivalent to inverse for unit quaternions).
pub(crate) fn quat_conj(q: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    (q.0, -q.1, -q.2, -q.3)
}

/// Rotate a 3D vector by a unit quaternion: q * (0,v) * conj(q).
pub(crate) fn quat_rotate(q: (f64, f64, f64, f64), vx: f64, vy: f64, vz: f64) -> (f64, f64, f64) {
    let v_quat = (0.0, vx, vy, vz);
    let tmp = quat_mul(q, v_quat);
    let result = quat_mul(tmp, quat_conj(q));
    (result.1, result.2, result.3)
}
