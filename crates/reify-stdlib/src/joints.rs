use std::collections::BTreeMap;

use reify_types::{DimensionVector, Value};

use crate::helpers::{tensor_components_f64, trig_input};
use crate::orientation::normalize_quaternion;

/// Evaluate a joints stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_joints(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "prismatic" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_axis(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_range(&args[1], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            // The axis is stored as the raw (potentially unnormalized) input.
            // `transform_at` normalizes it to unit length at evaluation time.
            // `joint_axis` returns this raw value — see its doc-comment.
            make_joint("prismatic", args[0].clone(), args[1].clone())
        }
        "revolute" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_axis(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_range(&args[1], DimensionVector::ANGLE).is_none() {
                return Some(Value::Undef);
            }
            // The axis is stored as the raw (potentially unnormalized) input.
            // `transform_at` normalizes it to unit length at evaluation time.
            // `joint_axis` returns this raw value — see its doc-comment.
            make_joint("revolute", args[0].clone(), args[1].clone())
        }
        "transform_at" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            let kind = match map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s)) => s.as_str(),
                _ => return Some(Value::Undef),
            };
            // The axis lookup is now inside each arm: Coupling maps have no axis
            // field, so moving the lookup prevents a spurious Undef for coupling.
            match kind {
                "prismatic" => {
                    let axis_val = match map.get(&Value::String("axis".to_string())) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let comps = match validate_axis(axis_val) {
                        Some(c) => c,
                        None => return Some(Value::Undef),
                    };
                    let mag = (comps[0] * comps[0] + comps[1] * comps[1] + comps[2] * comps[2]).sqrt();
                    let [nax, nay, naz] = [comps[0] / mag, comps[1] / mag, comps[2] / mag];
                    // Accept Length Scalar or bare Real/Int as metres
                    let dist = match length_input(&args[1]) {
                        Some(d) => d,
                        None => return Some(Value::Undef),
                    };
                    // length_input already enforces finiteness for the Scalar/Real
                    // branches; the Int branch yields finite f64 by construction.
                    // This guard is defense-in-depth against future changes to
                    // length_input.
                    if !dist.is_finite() {
                        return Some(Value::Undef);
                    }
                    let translation = Value::Vector(vec![
                        Value::length(dist * nax),
                        Value::length(dist * nay),
                        Value::length(dist * naz),
                    ]);
                    let rotation = Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    };
                    Value::Transform {
                        rotation: Box::new(rotation),
                        translation: Box::new(translation),
                    }
                }
                "revolute" => {
                    let axis_val = match map.get(&Value::String("axis".to_string())) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let comps = match validate_axis(axis_val) {
                        Some(c) => c,
                        None => return Some(Value::Undef),
                    };
                    let mag = (comps[0] * comps[0] + comps[1] * comps[1] + comps[2] * comps[2]).sqrt();
                    let [nax, nay, naz] = [comps[0] / mag, comps[1] / mag, comps[2] / mag];
                    // Accept Angle Scalar or bare Real/Int as radians
                    let theta = match trig_input(&args[1]) {
                        Some(t) => t,
                        None => return Some(Value::Undef),
                    };
                    // trig_input already enforces finiteness for the Scalar/Real
                    // branches; the Int branch yields finite f64 by construction.
                    // This guard is defense-in-depth against future changes to
                    // trig_input (parallel to the same guard in the prismatic arm).
                    if !theta.is_finite() {
                        return Some(Value::Undef);
                    }
                    let rotation = axis_angle_quaternion(nax, nay, naz, theta);
                    let translation = Value::Vector(vec![
                        Value::length(0.0),
                        Value::length(0.0),
                        Value::length(0.0),
                    ]);
                    Value::Transform {
                        rotation: Box::new(rotation),
                        translation: Box::new(translation),
                    }
                }
                "coupling" => {
                    // Extract the four coupling-map fields with explicit guards.
                    let parent = match map.get(&Value::String("parent".to_string())) {
                        Some(v) => v.clone(),
                        None => return Some(Value::Undef),
                    };
                    let ratio_f64 = match map.get(&Value::String("ratio".to_string())) {
                        Some(Value::Real(r)) => *r,
                        _ => return Some(Value::Undef),
                    };
                    let offset_si = match map.get(&Value::String("offset".to_string())) {
                        Some(Value::Scalar { si_value, .. }) => *si_value,
                        _ => return Some(Value::Undef),
                    };
                    // Validate the stored parent kind — defense-in-depth against
                    // hand-built Map fixtures with invalid parent kinds.
                    let parent_kind = match &parent {
                        Value::Map(pm) => match pm.get(&Value::String("kind".to_string())) {
                            Some(Value::String(s)) => s.clone(),
                            _ => return Some(Value::Undef),
                        },
                        _ => return Some(Value::Undef),
                    };
                    let parent_is_prismatic = match parent_kind.as_str() {
                        "prismatic" => true,
                        "revolute" => false,
                        _ => return Some(Value::Undef),
                    };
                    // Extract v_si from args[1] via dimension-appropriate helper;
                    // both helpers reject wrong-dim Scalars and non-finite values.
                    let v_si = if parent_is_prismatic {
                        match length_input(&args[1]) {
                            Some(d) => d,
                            None => return Some(Value::Undef),
                        }
                    } else {
                        match trig_input(&args[1]) {
                            Some(t) => t,
                            None => return Some(Value::Undef),
                        }
                    };
                    // Defense-in-depth: length_input/trig_input already reject
                    // non-finite v; this guard mirrors the prismatic/revolute arms.
                    if !v_si.is_finite() {
                        return Some(Value::Undef);
                    }
                    // Derive the coupled motion variable: ratio * v + offset
                    let coupled_si = ratio_f64 * v_si + offset_si;
                    if !coupled_si.is_finite() {
                        return Some(Value::Undef);
                    }
                    let coupled_value = if parent_is_prismatic {
                        Value::length(coupled_si)
                    } else {
                        Value::angle(coupled_si)
                    };
                    // Recursively delegate to the parent joint arm.  Termination is
                    // guaranteed: `couple` rejects coupling parents at construction,
                    // so the recursion always reaches a prismatic/revolute arm at depth 1.
                    eval_joints("transform_at", &[parent, coupled_value])
                        .unwrap_or(Value::Undef)
                }
                _ => Value::Undef,
            }
        }
        "couple" => {
            // Validate arg count: 2 or 3
            if args.len() != 2 && args.len() != 3 {
                return Some(Value::Undef);
            }
            // Validate parent: must be a Map with kind in {"prismatic", "revolute"}
            let parent_map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            let parent_kind = match parent_map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s)) => s.as_str(),
                _ => return Some(Value::Undef),
            };
            let is_prismatic = match parent_kind {
                "prismatic" => true,
                "revolute" => false,
                // Rejects "coupling" and any other kind
                _ => return Some(Value::Undef),
            };
            // Extract ratio: finite, dimensionless numeric (Real/Int/DIMENSIONLESS Scalar).
            // ratio_input rejects NaN, Inf, and dimensioned Scalars.
            let ratio_f64 = match ratio_input(&args[1]) {
                Some(r) => r,
                None => return Some(Value::Undef),
            };
            // Extract offset: use parent-dimension-keyed helper (length_input / trig_input)
            // so bare Real/Int is accepted in addition to correctly-dimensioned Scalar.
            let offset_si = if args.len() == 3 {
                if is_prismatic {
                    match length_input(&args[2]) {
                        Some(d) => d,
                        None => return Some(Value::Undef),
                    }
                } else {
                    match trig_input(&args[2]) {
                        Some(r) => r,
                        None => return Some(Value::Undef),
                    }
                }
            } else {
                0.0
            };
            let offset = if is_prismatic {
                Value::length(offset_si)
            } else {
                Value::angle(offset_si)
            };
            make_coupling(args[0].clone(), Value::Real(ratio_f64), offset)
        }
        "joint_axis" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            // Returns the axis as stored at construction — the raw, potentially
            // unnormalized input vector.  `transform_at` normalizes to unit
            // length when computing the resulting Transform; this accessor
            // preserves the original value so callers can inspect what was
            // passed to `prismatic`/`revolute`.
            match &args[0] {
                Value::Map(m) => {
                    m.get(&Value::String("axis".to_string()))
                        .cloned()
                        .unwrap_or(Value::Undef)
                }
                _ => Value::Undef,
            }
        }
        "joint_range" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::Map(m) => {
                    m.get(&Value::String("range".to_string()))
                        .cloned()
                        .unwrap_or(Value::Undef)
                }
                _ => Value::Undef,
            }
        }
        _ => return None,
    })
}

/// Extract a dimensionless ratio from a `couple` ratio argument.
///
/// Accepts:
/// - `Value::Scalar { dimension: DIMENSIONLESS, .. }` with finite si_value.
/// - `Value::Real(r)` (finite) — treated as dimensionless ratio directly.
/// - `Value::Int(i)` — treated as dimensionless ratio directly.
///
/// Returns `None` for any other variant (wrong dimension, non-finite, NaN, Inf).
fn ratio_input(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, dimension } => {
            if *dimension == DimensionVector::DIMENSIONLESS && si_value.is_finite() {
                Some(*si_value)
            } else {
                None
            }
        }
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Extract metres from a `transform_at` value argument for a Prismatic joint.
///
/// Accepts:
/// - `Value::Scalar { dimension: LENGTH, .. }` — si_value is metres.
/// - `Value::Real(r)` / `Value::Int(i)` — treated as metres directly.
///
/// Returns `None` for any other variant (wrong dimension, non-finite).
fn length_input(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, dimension } => {
            if *dimension == DimensionVector::LENGTH && si_value.is_finite() {
                Some(*si_value)
            } else {
                None
            }
        }
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Validate an axis value: must be a Vector3 of dimensionless components,
/// all finite, with non-zero magnitude.
///
/// Returns `Some([x, y, z])` (the raw components, not normalized) on success,
/// `None` on any failure.
fn validate_axis(value: &Value) -> Option<[f64; 3]> {
    let (comps, dim) = tensor_components_f64(value)?;
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

/// Validate a range value: must be `Value::Range` with both lower and upper
/// bounds present, both sharing `expected_dim`.
///
/// Returns `Some(())` on success, `None` on any failure.
fn validate_range(value: &Value, expected_dim: DimensionVector) -> Option<()> {
    match value {
        Value::Range {
            lower: Some(lo),
            upper: Some(up),
            ..
        } => {
            if lo.dimension() == expected_dim && up.dimension() == expected_dim {
                Some(())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Build a coupling `Value::Map` with the four-key layout:
/// `"kind"`, `"offset"`, `"parent"`, `"ratio"`.
///
/// Keys are in alphabetical order as `BTreeMap` sorts them, matching the
/// pattern of `make_joint`.  `ratio` is stored as `Value::Real` (already
/// extracted to f64 by the caller).
fn make_coupling(parent: Value, ratio: Value, offset: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
    m.insert(Value::String("offset".to_string()), offset);
    m.insert(Value::String("parent".to_string()), parent);
    m.insert(Value::String("ratio".to_string()), ratio);
    Value::Map(m)
}

/// Build a joint `Value::Map` with the standard three-key layout:
/// `"kind"`, `"axis"`, `"range"`.
fn make_joint(kind: &str, axis: Value, range: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("kind".to_string()), Value::String(kind.to_string()));
    m.insert(Value::String("axis".to_string()), axis);
    m.insert(Value::String("range".to_string()), range);
    Value::Map(m)
}

/// Build a quaternion `Value::Orientation` from a **pre-normalized** unit axis
/// `(nax, nay, naz)` and a rotation angle `theta` in radians.
///
/// Delegates to [`normalize_quaternion`] for a final unit-norm check to absorb
/// floating-point drift from the sin/cos computation.  Returns `Value::Undef`
/// only if inputs are non-finite or the computed norm underflows — both
/// unreachable in practice for finite, unit-magnitude axis inputs.
///
/// This mirrors the axis-angle path in `orientation::eval_orientation`
/// (`orient_axis_angle`).  A future scope expansion to `orientation.rs` can
/// lift this to `orientation::axis_angle_quaternion` and share it from both
/// call sites, eliminating the remaining duplication.
fn axis_angle_quaternion(nax: f64, nay: f64, naz: f64, theta: f64) -> Value {
    let half = theta / 2.0;
    let c = half.cos();
    let s = half.sin();
    normalize_quaternion(c, s * nax, s * nay, s * naz).unwrap_or(Value::Undef)
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_types::Value;

    fn axis_x_unit() -> Value {
        Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
    }

    fn length_range_0_to_1m() -> Value {
        Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(1.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    // ── prismatic constructor: happy path ────────────────────────────────────

    #[test]
    fn prismatic_returns_map_with_correct_fields() {
        let axis = axis_x_unit();
        let range = length_range_0_to_1m();
        let result = eval_builtin("prismatic", &[axis.clone(), range.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("prismatic".to_string())),
            "kind field should be 'prismatic'"
        );
        assert_eq!(
            map.get(&Value::String("axis".to_string())),
            Some(&axis),
            "axis field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range".to_string())),
            Some(&range),
            "range field should match input"
        );
    }

    // ── prismatic constructor: wrong arg counts ──────────────────────────────

    #[test]
    fn prismatic_zero_args_returns_undef() {
        assert!(
            eval_builtin("prismatic", &[]).is_undef(),
            "zero args should return Undef"
        );
    }

    #[test]
    fn prismatic_one_arg_returns_undef() {
        assert!(
            eval_builtin("prismatic", &[axis_x_unit()]).is_undef(),
            "one arg should return Undef"
        );
    }

    // ── revolute constructor helpers ─────────────────────────────────────────

    fn axis_z_unit() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
    }

    fn angle_range_0_to_pi() -> Value {
        Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    // ── revolute constructor: happy path ─────────────────────────────────────

    #[test]
    fn revolute_returns_map_with_correct_fields() {
        let axis = axis_z_unit();
        let range = angle_range_0_to_pi();
        let result = eval_builtin("revolute", &[axis.clone(), range.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("revolute".to_string())),
            "kind field should be 'revolute'"
        );
        assert_eq!(
            map.get(&Value::String("axis".to_string())),
            Some(&axis),
            "axis field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range".to_string())),
            Some(&range),
            "range field should match input"
        );
    }

    // ── revolute constructor: wrong arg counts ───────────────────────────────

    #[test]
    fn revolute_zero_args_returns_undef() {
        assert!(
            eval_builtin("revolute", &[]).is_undef(),
            "zero args should return Undef"
        );
    }

    #[test]
    fn revolute_one_arg_returns_undef() {
        assert!(
            eval_builtin("revolute", &[axis_z_unit()]).is_undef(),
            "one arg should return Undef"
        );
    }

    // ── prismatic validation: axis ───────────────────────────────────────────

    #[test]
    fn prismatic_non_vector_axis_returns_undef() {
        // axis is a bare Real, not a Vector3
        assert!(
            eval_builtin("prismatic", &[Value::Real(1.0), length_range_0_to_1m()]).is_undef(),
            "non-vector axis should return Undef"
        );
    }

    #[test]
    fn prismatic_vec2_axis_returns_undef() {
        // axis has 2 components, not 3
        let axis2 = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("prismatic", &[axis2, length_range_0_to_1m()]).is_undef(),
            "2-component axis should return Undef"
        );
    }

    #[test]
    fn prismatic_length_dimensioned_axis_returns_undef() {
        // axis components are LENGTH-dimensioned, not dimensionless
        let axis_len = Value::Vector(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        assert!(
            eval_builtin("prismatic", &[axis_len, length_range_0_to_1m()]).is_undef(),
            "Length-dimensioned axis should return Undef"
        );
    }

    #[test]
    fn prismatic_zero_axis_returns_undef() {
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("prismatic", &[zero_axis, length_range_0_to_1m()]).is_undef(),
            "zero-magnitude axis should return Undef"
        );
    }

    #[test]
    fn prismatic_nan_axis_returns_undef() {
        let nan_axis = Value::Vector(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("prismatic", &[nan_axis, length_range_0_to_1m()]).is_undef(),
            "NaN axis should return Undef"
        );
    }

    // ── prismatic validation: range ──────────────────────────────────────────

    #[test]
    fn prismatic_non_range_arg_returns_undef() {
        // range arg is a bare Real
        assert!(
            eval_builtin("prismatic", &[axis_x_unit(), Value::Real(1.0)]).is_undef(),
            "non-Range second arg should return Undef"
        );
    }

    #[test]
    fn prismatic_unbounded_range_returns_undef() {
        // range is missing upper bound
        let unbounded = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };
        assert!(
            eval_builtin("prismatic", &[axis_x_unit(), unbounded]).is_undef(),
            "unbounded range should return Undef"
        );
    }

    #[test]
    fn prismatic_angle_range_returns_undef() {
        // range dimension is Angle, not Length — dimension mismatch
        assert!(
            eval_builtin("prismatic", &[axis_x_unit(), angle_range_0_to_pi()]).is_undef(),
            "Angle-dimensioned range for Prismatic should return Undef"
        );
    }

    // ── revolute validation: axis ────────────────────────────────────────────

    #[test]
    fn revolute_non_vector_axis_returns_undef() {
        assert!(
            eval_builtin("revolute", &[Value::Real(1.0), angle_range_0_to_pi()]).is_undef(),
            "non-vector axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_vec2_axis_returns_undef() {
        let axis2 = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("revolute", &[axis2, angle_range_0_to_pi()]).is_undef(),
            "2-component axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_length_dimensioned_axis_returns_undef() {
        let axis_len = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(1.0),
        ]);
        assert!(
            eval_builtin("revolute", &[axis_len, angle_range_0_to_pi()]).is_undef(),
            "Length-dimensioned axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_zero_axis_returns_undef() {
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("revolute", &[zero_axis, angle_range_0_to_pi()]).is_undef(),
            "zero-magnitude axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_nan_axis_returns_undef() {
        let nan_axis = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(f64::NAN),
        ]);
        assert!(
            eval_builtin("revolute", &[nan_axis, angle_range_0_to_pi()]).is_undef(),
            "NaN axis for revolute should return Undef"
        );
    }

    // ── revolute validation: range ───────────────────────────────────────────

    #[test]
    fn revolute_non_range_arg_returns_undef() {
        assert!(
            eval_builtin("revolute", &[axis_z_unit(), Value::Real(1.0)]).is_undef(),
            "non-Range second arg for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_unbounded_range_returns_undef() {
        let unbounded = Value::Range {
            lower: None,
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: false,
            upper_inclusive: true,
        };
        assert!(
            eval_builtin("revolute", &[axis_z_unit(), unbounded]).is_undef(),
            "unbounded range for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_length_range_returns_undef() {
        // range dimension is Length, not Angle — dimension mismatch
        assert!(
            eval_builtin("revolute", &[axis_z_unit(), length_range_0_to_1m()]).is_undef(),
            "Length-dimensioned range for revolute should return Undef"
        );
    }

    // ── validate_range: inverted range is intentionally permissive ───────────

    #[test]
    fn prismatic_with_inverted_range_constructs_ok() {
        // validate_range only checks that both bounds are present and
        // dimensionally consistent; ordering (lo > up) is intentionally
        // permissive.  The range field is informational metadata used by
        // callers (e.g. a sweep step), not validated for geometric sense at
        // construction time.  This test pins that permissive behaviour so
        // any future tightening is a deliberate, visible change.
        let inverted = Value::Range {
            lower: Some(Box::new(Value::length(5.0))),
            upper: Some(Box::new(Value::length(0.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let result = eval_builtin("prismatic", &[axis_x_unit(), inverted]);
        assert!(
            matches!(result, Value::Map(_)),
            "inverted-range prismatic should construct successfully, got {:?}", result
        );
    }

    // ── transform_at on Prismatic: helpers ───────────────────────────────────

    fn prismatic_x_joint() -> Value {
        eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()])
    }

    fn prismatic_y_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(5.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("prismatic", &[axis, range])
    }

    fn prismatic_z_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(-5.0))),
            upper: Some(Box::new(Value::length(5.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("prismatic", &[axis, range])
    }

    /// Assert two `Value::Transform` are component-wise within tolerance.
    fn assert_transform_approx(result: &Value, exp_rot: (f64, f64, f64, f64), exp_trans: [f64; 3], tol: f64, label: &str) {
        let (rot, trans) = match result {
            Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
            other => panic!("{}: expected Transform, got {:?}", label, other),
        };
        let (w, x, y, z) = match rot {
            Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
            other => panic!("{}: expected Orientation, got {:?}", label, other),
        };
        assert!((w - exp_rot.0).abs() < tol, "{}: rotation.w expected {} got {}", label, exp_rot.0, w);
        assert!((x - exp_rot.1).abs() < tol, "{}: rotation.x expected {} got {}", label, exp_rot.1, x);
        assert!((y - exp_rot.2).abs() < tol, "{}: rotation.y expected {} got {}", label, exp_rot.2, y);
        assert!((z - exp_rot.3).abs() < tol, "{}: rotation.z expected {} got {}", label, exp_rot.3, z);

        let comps = match trans {
            Value::Vector(v) if v.len() == 3 => v,
            other => panic!("{}: expected Vector(3), got {:?}", label, other),
        };
        for (i, (comp, &exp)) in comps.iter().zip(exp_trans.iter()).enumerate() {
            let val = comp.as_f64().unwrap_or_else(|| panic!("{}: translation[{}] not numeric", label, i));
            assert!((val - exp).abs() < tol, "{}: translation[{}] expected {} got {}", label, i, exp, val);
        }
    }

    // ── transform_at on Prismatic: analytic tests ────────────────────────────

    #[test]
    fn prismatic_transform_at_x_axis_5m() {
        let joint = prismatic_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(5.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [5.0, 0.0, 0.0], 1e-12,
            "prismatic X, 5m");
    }

    #[test]
    fn prismatic_transform_at_y_axis_3m() {
        let joint = prismatic_y_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(3.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.0, 3.0, 0.0], 1e-12,
            "prismatic Y, 3m");
    }

    #[test]
    fn prismatic_transform_at_z_axis_neg2m() {
        let joint = prismatic_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(-2.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.0, 0.0, -2.0], 1e-12,
            "prismatic Z, -2m");
    }

    #[test]
    fn prismatic_transform_at_zero_value() {
        let joint = prismatic_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(0.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.0, 0.0, 0.0], 1e-12,
            "prismatic X, 0m");
    }

    #[test]
    fn prismatic_transform_at_diagonal_axis() {
        // axis = [1,1,0]/√2, value = √2 m → translation = [1m, 1m, 0m]
        let sq2 = std::f64::consts::SQRT_2;
        let axis = Value::Vector(vec![
            Value::Real(1.0 / sq2),
            Value::Real(1.0 / sq2),
            Value::Real(0.0),
        ]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(10.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("prismatic", &[axis, range]);
        let result = eval_builtin("transform_at", &[joint, Value::length(sq2)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [1.0, 1.0, 0.0], 1e-12,
            "prismatic diagonal [1,1,0]/√2, √2 m");
    }

    #[test]
    fn prismatic_transform_at_unnormalized_axis() {
        // axis = [2, 0, 0] (magnitude 2), value = 1m → normalized axis [1,0,0] → translation = [1m, 0, 0]
        let axis = Value::Vector(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(5.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("prismatic", &[axis, range]);
        let result = eval_builtin("transform_at", &[joint, Value::length(1.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [1.0, 0.0, 0.0], 1e-12,
            "prismatic unnormalized [2,0,0], 1m");
    }

    #[test]
    fn prismatic_transform_at_bare_real_value() {
        // bare Value::Real(0.5) accepted as 0.5 metres
        let joint = prismatic_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::Real(0.5)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.5, 0.0, 0.0], 1e-12,
            "prismatic X, bare Real(0.5)");
    }

    // ── transform_at on Revolute: helpers ────────────────────────────────────

    fn revolute_z_joint() -> Value {
        eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()])
    }

    fn revolute_x_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("revolute", &[axis, range])
    }

    fn revolute_y_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("revolute", &[axis, range])
    }

    // ── transform_at on Revolute: analytic tests ─────────────────────────────

    #[test]
    fn revolute_transform_at_z_axis_half_pi() {
        // Z axis, π/2 → rotation = (cos(π/4), 0, 0, sin(π/4))
        let pi = std::f64::consts::PI;
        let joint = revolute_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(&result, (cos, 0.0, 0.0, sin), [0.0, 0.0, 0.0], 1e-12,
            "revolute Z, π/2");
    }

    #[test]
    fn revolute_transform_at_x_axis_pi() {
        // X axis, π → rotation = (0, 1, 0, 0)
        let pi = std::f64::consts::PI;
        let joint = revolute_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi)]);
        assert_transform_approx(&result, (0.0, 1.0, 0.0, 0.0), [0.0, 0.0, 0.0], 1e-12,
            "revolute X, π");
    }

    #[test]
    fn revolute_transform_at_y_axis_half_pi() {
        // Y axis, π/2 → rotation = (cos(π/4), 0, sin(π/4), 0)
        let pi = std::f64::consts::PI;
        let joint = revolute_y_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(&result, (cos, 0.0, sin, 0.0), [0.0, 0.0, 0.0], 1e-12,
            "revolute Y, π/2");
    }

    #[test]
    fn revolute_transform_at_zero_angle() {
        // angle = 0 → identity rotation
        let joint = revolute_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(0.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.0, 0.0, 0.0], 1e-12,
            "revolute Z, 0");
    }

    #[test]
    fn revolute_transform_at_bare_real_value() {
        // bare Real(π/2) accepted as radians
        let pi = std::f64::consts::PI;
        let joint = revolute_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::Real(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(&result, (cos, 0.0, 0.0, sin), [0.0, 0.0, 0.0], 1e-12,
            "revolute Z, bare Real(π/2)");
    }

    #[test]
    fn revolute_transform_at_unnormalized_axis() {
        // axis [0, 0, 2] (magnitude 2) with π/2 → same rotation as [0,0,1] with π/2
        let pi = std::f64::consts::PI;
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(pi))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("revolute", &[axis, range]);
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(&result, (cos, 0.0, 0.0, sin), [0.0, 0.0, 0.0], 1e-12,
            "revolute unnormalized [0,0,2], π/2");
    }

    #[test]
    fn revolute_transform_at_translation_always_zero() {
        // translation should always be [0m, 0m, 0m] regardless of angle
        let pi = std::f64::consts::PI;
        let joint = revolute_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 3.0)]);
        // Check only translation components
        let trans = match &result {
            Value::Transform { translation, .. } => translation.as_ref(),
            other => panic!("expected Transform, got {:?}", other),
        };
        let comps = match trans {
            Value::Vector(v) if v.len() == 3 => v,
            other => panic!("expected Vector(3), got {:?}", other),
        };
        for (i, comp) in comps.iter().enumerate() {
            let val = comp.as_f64().expect("translation component should be numeric");
            assert!((val - 0.0).abs() < 1e-12,
                "revolute translation[{}] should be 0, got {}", i, val);
        }
    }

    // ── transform_at validation ──────────────────────────────────────────────

    #[test]
    fn transform_at_prismatic_with_angle_value_returns_undef() {
        // Angle Scalar passed to a Prismatic joint
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::angle(1.0)]).is_undef(),
            "Angle Scalar to Prismatic should return Undef"
        );
    }

    #[test]
    fn transform_at_revolute_with_length_value_returns_undef() {
        // Length Scalar passed to a Revolute joint
        let joint = revolute_z_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::length(1.0)]).is_undef(),
            "Length Scalar to Revolute should return Undef"
        );
    }

    #[test]
    fn transform_at_revolute_with_mass_value_returns_undef() {
        use reify_types::DimensionVector;
        let joint = revolute_z_joint();
        let mass = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        assert!(
            eval_builtin("transform_at", &[joint, mass]).is_undef(),
            "Mass Scalar to Revolute should return Undef"
        );
    }

    #[test]
    fn transform_at_non_map_returns_undef() {
        assert!(
            eval_builtin("transform_at", &[Value::Real(1.0), Value::length(1.0)]).is_undef(),
            "non-Map first arg should return Undef"
        );
    }

    #[test]
    fn transform_at_map_without_kind_key_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "Map without kind key should return Undef"
        );
    }

    #[test]
    fn transform_at_map_with_unknown_kind_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("sliding".to_string()));
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        m.insert(Value::String("range".to_string()), length_range_0_to_1m());
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "Map with unknown kind should return Undef"
        );
    }

    #[test]
    fn transform_at_prismatic_nan_value_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::Real(f64::NAN)]).is_undef(),
            "NaN value for prismatic should return Undef"
        );
    }

    #[test]
    fn transform_at_revolute_inf_value_returns_undef() {
        let joint = revolute_z_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::Real(f64::INFINITY)]).is_undef(),
            "Inf value for revolute should return Undef"
        );
    }

    #[test]
    fn transform_at_zero_args_returns_undef() {
        assert!(
            eval_builtin("transform_at", &[]).is_undef(),
            "0 args should return Undef"
        );
    }

    #[test]
    fn transform_at_one_arg_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint]).is_undef(),
            "1 arg should return Undef"
        );
    }

    #[test]
    fn transform_at_three_args_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::length(1.0), Value::Real(0.0)]).is_undef(),
            "3 args should return Undef"
        );
    }

    // ── joint_axis accessor ──────────────────────────────────────────────────

    #[test]
    fn joint_axis_prismatic_returns_stored_axis() {
        let axis = axis_x_unit();
        let joint = eval_builtin("prismatic", &[axis.clone(), length_range_0_to_1m()]);
        assert_eq!(
            eval_builtin("joint_axis", &[joint]),
            axis,
            "joint_axis(prismatic) should return stored axis"
        );
    }

    #[test]
    fn joint_axis_revolute_returns_stored_axis() {
        let axis = axis_z_unit();
        let joint = eval_builtin("revolute", &[axis.clone(), angle_range_0_to_pi()]);
        assert_eq!(
            eval_builtin("joint_axis", &[joint]),
            axis,
            "joint_axis(revolute) should return stored axis"
        );
    }

    #[test]
    fn joint_axis_non_joint_returns_undef() {
        assert!(
            eval_builtin("joint_axis", &[Value::Real(1.0)]).is_undef(),
            "joint_axis of non-Map should return Undef"
        );
    }

    #[test]
    fn joint_axis_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_axis", &[]).is_undef(),
            "joint_axis with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_axis_two_args_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("joint_axis", &[joint, Value::Real(0.0)]).is_undef(),
            "joint_axis with 2 args should return Undef"
        );
    }

    // ── couple constructor: happy paths ─────────────────────────────────────

    #[test]
    fn couple_prismatic_2arg_returns_coupling_map() {
        // 2-arg form: couple(prismatic, ratio) → Map with kind="coupling",
        // parent=<prismatic Map>, ratio=Value::Real(-1.0),
        // offset=Value::length(0.0) (default zero in LENGTH dimension)
        let parent = prismatic_x_joint();
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(-1.0)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the prismatic joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(-1.0)),
            "ratio should be Value::Real(-1.0)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::length(0.0)),
            "default offset for prismatic should be Value::length(0.0)"
        );
    }

    #[test]
    fn couple_prismatic_3arg_stores_explicit_offset() {
        // 3-arg form: explicit offset stored as provided
        let parent = prismatic_x_joint();
        let offset = Value::length(0.5);
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(2.0), offset.clone()]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the prismatic joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(2.0)),
            "ratio should be Value::Real(2.0)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&offset),
            "offset should be Value::length(0.5)"
        );
    }

    #[test]
    fn couple_revolute_2arg_defaults_angle_offset() {
        // 2-arg revolute parent → default offset is Value::angle(0.0)
        let parent = revolute_z_joint();
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(0.5)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the revolute joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(0.5)),
            "ratio should be Value::Real(0.5)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::angle(0.0)),
            "default offset for revolute should be Value::angle(0.0)"
        );
    }

    #[test]
    fn couple_revolute_3arg_stores_explicit_angle_offset() {
        // 3-arg revolute form: explicit angle offset stored
        let pi = std::f64::consts::PI;
        let parent = revolute_z_joint();
        let offset = Value::angle(pi / 4.0);
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(0.5), offset.clone()]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the revolute joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(0.5)),
            "ratio should be Value::Real(0.5)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&offset),
            "explicit angle offset should be stored"
        );
    }

    // ── couple constructor: validation rejections ────────────────────────────

    #[test]
    fn couple_zero_args_returns_undef() {
        assert!(eval_builtin("couple", &[]).is_undef(), "0 args should return Undef");
    }

    #[test]
    fn couple_one_arg_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint()]).is_undef(),
            "1 arg should return Undef"
        );
    }

    #[test]
    fn couple_four_args_returns_undef() {
        assert!(
            eval_builtin("couple", &[
                prismatic_x_joint(),
                Value::Real(1.0),
                Value::length(0.0),
                Value::Real(0.0),
            ]).is_undef(),
            "4 args should return Undef"
        );
    }

    #[test]
    fn couple_non_map_parent_returns_undef() {
        assert!(
            eval_builtin("couple", &[Value::Real(1.0), Value::Real(1.0)]).is_undef(),
            "non-Map parent should return Undef"
        );
    }

    #[test]
    fn couple_map_missing_kind_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("couple", &[Value::Map(m), Value::Real(1.0)]).is_undef(),
            "Map parent missing kind key should return Undef"
        );
    }

    #[test]
    fn couple_coupling_parent_returns_undef() {
        // nested coupling is rejected — kind="coupling" is not a DrivingJoint
        let inner = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("couple", &[inner, Value::Real(1.0)]).is_undef(),
            "coupling parent (kind='coupling') should return Undef"
        );
    }

    #[test]
    fn couple_unknown_parent_kind_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("sliding".to_string()));
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("couple", &[Value::Map(m), Value::Real(1.0)]).is_undef(),
            "parent kind='sliding' should return Undef"
        );
    }

    #[test]
    fn couple_string_ratio_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::String("bad".to_string())]).is_undef(),
            "String ratio should return Undef"
        );
    }

    #[test]
    fn couple_nan_ratio_returns_undef() {
        // NaN ratio must be rejected — only finites are valid
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(f64::NAN)]).is_undef(),
            "NaN ratio should return Undef"
        );
    }

    #[test]
    fn couple_inf_ratio_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(f64::INFINITY)]).is_undef(),
            "Infinite ratio should return Undef"
        );
    }

    #[test]
    fn couple_dimensioned_ratio_returns_undef() {
        // a Length Scalar as ratio is not dimensionless — must be rejected
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::length(1.0)]).is_undef(),
            "dimensioned ratio should return Undef"
        );
    }

    #[test]
    fn couple_prismatic_wrong_offset_dim_returns_undef() {
        use reify_types::DimensionVector;
        let mass_offset = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0), mass_offset]).is_undef(),
            "MASS offset for prismatic parent should return Undef"
        );
    }

    #[test]
    fn couple_revolute_wrong_offset_dim_returns_undef() {
        // Length offset for a revolute parent (needs Angle or bare Real)
        assert!(
            eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0), Value::length(1.0)]).is_undef(),
            "Length offset for revolute parent should return Undef"
        );
    }

    #[test]
    fn couple_prismatic_nan_offset_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0), Value::Real(f64::NAN)]).is_undef(),
            "NaN offset should return Undef"
        );
    }

    // ── transform_at on Coupling: validation rejections ─────────────────────

    /// Build a minimal coupling Map by hand for testing defense-in-depth guards.
    fn make_coupling_fixture(
        parent: Value,
        ratio: Value,
        offset: Value,
    ) -> Value {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("offset".to_string()), offset);
        m.insert(Value::String("parent".to_string()), parent);
        m.insert(Value::String("ratio".to_string()), ratio);
        Value::Map(m)
    }

    #[test]
    fn transform_at_coupling_angle_to_prismatic_parent_returns_undef() {
        // Angle Scalar passed to a coupling whose parent is prismatic
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::angle(1.0)]).is_undef(),
            "Angle to prismatic coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_length_to_revolute_parent_returns_undef() {
        // Length Scalar passed to a coupling whose parent is revolute
        let c = eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "Length to revolute coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_mass_value_returns_undef() {
        use reify_types::DimensionVector;
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        let mass = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        assert!(
            eval_builtin("transform_at", &[c, mass]).is_undef(),
            "MASS Scalar to coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_nan_value_returns_undef() {
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::Real(f64::NAN)]).is_undef(),
            "NaN value to coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_inf_value_returns_undef() {
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::Real(f64::INFINITY)]).is_undef(),
            "Inf value to coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_sliding_parent_kind_returns_undef() {
        // Defense-in-depth: hand-built coupling Map with parent kind="sliding"
        use std::collections::BTreeMap;
        let mut sliding = BTreeMap::new();
        sliding.insert(Value::String("kind".to_string()), Value::String("sliding".to_string()));
        sliding.insert(Value::String("axis".to_string()), axis_x_unit());
        let c = make_coupling_fixture(
            Value::Map(sliding),
            Value::Real(1.0),
            Value::length(0.0),
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with sliding parent should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_missing_parent_key_returns_undef() {
        // Defense-in-depth: hand-built coupling Map without parent key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "parent" key
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "coupling missing parent key should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_missing_ratio_key_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("parent".to_string()), prismatic_x_joint());
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "ratio" key
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "coupling missing ratio key should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_missing_offset_key_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("parent".to_string()), prismatic_x_joint());
        m.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        // no "offset" key
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "coupling missing offset key should return Undef"
        );
    }

    // ── transform_at on Coupling: analytic tests ────────────────────────────

    #[test]
    fn coupling_prismatic_sign_reversal() {
        // Counter-mass idiom: ratio=-1 on X-prismatic → translation negated
        // coupled_value = -1.0 * 5.0 + 0.0 = -5.0 m
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(-1.0)]);
        let result = eval_builtin("transform_at", &[c, Value::length(5.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [-5.0, 0.0, 0.0],
            1e-12,
            "coupling prismatic sign reversal, ratio=-1, v=5m → [-5,0,0]",
        );
    }

    #[test]
    fn coupling_prismatic_with_offset() {
        // ratio=2.0, offset=1.0m, v=3.0m → coupled = 2*3+1 = 7m → [7,0,0]
        let c = eval_builtin("couple", &[
            prismatic_x_joint(),
            Value::Real(2.0),
            Value::length(1.0),
        ]);
        let result = eval_builtin("transform_at", &[c, Value::length(3.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [7.0, 0.0, 0.0],
            1e-12,
            "coupling prismatic ratio=2, offset=1m, v=3m → [7,0,0]",
        );
    }

    #[test]
    fn coupling_prismatic_bare_real_value() {
        // bare Real(0.5) accepted as 0.5 metres; ratio=1, offset=0 → [0.5,0,0]
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        let result = eval_builtin("transform_at", &[c, Value::Real(0.5)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.5, 0.0, 0.0],
            1e-12,
            "coupling prismatic bare Real(0.5), ratio=1 → [0.5,0,0]",
        );
    }

    #[test]
    fn coupling_revolute_sign_reversal() {
        // ratio=-1 on Z-revolute → rotation reversed: angle = -π/2
        // coupled_value = -1.0 * (π/2) + 0 = -π/2
        // rotation = (cos(-π/4), 0, 0, sin(-π/4))
        let pi = std::f64::consts::PI;
        let c = eval_builtin("couple", &[revolute_z_joint(), Value::Real(-1.0)]);
        let result = eval_builtin("transform_at", &[c, Value::angle(pi / 2.0)]);
        let exp_w = (-pi / 4.0).cos();
        let exp_z = (-pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (exp_w, 0.0, 0.0, exp_z),
            [0.0, 0.0, 0.0],
            1e-12,
            "coupling revolute sign reversal, ratio=-1, v=π/2 → -π/2",
        );
    }

    #[test]
    fn coupling_revolute_with_offset() {
        // ratio=1.0, offset=π/4, v=π/4 → coupled = 1*(π/4) + π/4 = π/2
        // rotation about Z by π/2 = (cos(π/4), 0, 0, sin(π/4))
        let pi = std::f64::consts::PI;
        let c = eval_builtin("couple", &[
            revolute_z_joint(),
            Value::Real(1.0),
            Value::angle(pi / 4.0),
        ]);
        let result = eval_builtin("transform_at", &[c, Value::angle(pi / 4.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.0, 0.0, 0.0],
            1e-12,
            "coupling revolute ratio=1, offset=π/4, v=π/4 → rotation π/2",
        );
    }

    #[test]
    fn coupling_zero_ratio_gives_identity_transform() {
        // ratio=0 → coupled_value = 0*v + 0 = 0m regardless of v
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(0.0)]);
        let result = eval_builtin("transform_at", &[c, Value::length(99.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "coupling zero ratio → identity transform",
        );
    }

    // ── joint_range accessor ─────────────────────────────────────────────────

    #[test]
    fn joint_range_prismatic_returns_stored_range() {
        let range = length_range_0_to_1m();
        let joint = eval_builtin("prismatic", &[axis_x_unit(), range.clone()]);
        assert_eq!(
            eval_builtin("joint_range", &[joint]),
            range,
            "joint_range(prismatic) should return stored range"
        );
    }

    #[test]
    fn joint_range_revolute_returns_stored_range() {
        let range = angle_range_0_to_pi();
        let joint = eval_builtin("revolute", &[axis_z_unit(), range.clone()]);
        assert_eq!(
            eval_builtin("joint_range", &[joint]),
            range,
            "joint_range(revolute) should return stored range"
        );
    }

    #[test]
    fn joint_range_non_joint_returns_undef() {
        assert!(
            eval_builtin("joint_range", &[Value::String("foo".to_string())]).is_undef(),
            "joint_range of non-Map should return Undef"
        );
    }

    #[test]
    fn joint_range_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_range", &[]).is_undef(),
            "joint_range with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_range_two_args_returns_undef() {
        let joint = revolute_z_joint();
        assert!(
            eval_builtin("joint_range", &[joint, Value::Real(0.0)]).is_undef(),
            "joint_range with 2 args should return Undef"
        );
    }
}
