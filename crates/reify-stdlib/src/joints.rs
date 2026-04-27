use std::collections::BTreeMap;

use reify_types::{DimensionVector, Value};

use crate::helpers::tensor_components_f64;

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
            let axis_val = match map.get(&Value::String("axis".to_string())) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let comps = match validate_axis(axis_val) {
                Some(c) => c,
                None => return Some(Value::Undef),
            };
            // Normalize axis to unit length
            let mag = (comps[0] * comps[0] + comps[1] * comps[1] + comps[2] * comps[2]).sqrt();
            let [nax, nay, naz] = [comps[0] / mag, comps[1] / mag, comps[2] / mag];

            match kind {
                "prismatic" => {
                    // Accept Length Scalar or bare Real/Int as metres
                    let dist = match length_input(&args[1]) {
                        Some(d) => d,
                        None => return Some(Value::Undef),
                    };
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
                _ => Value::Undef,
            }
        }
        _ => return None,
    })
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

/// Build a joint `Value::Map` with the standard three-key layout:
/// `"kind"`, `"axis"`, `"range"`.
fn make_joint(kind: &str, axis: Value, range: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("kind".to_string()), Value::String(kind.to_string()));
    m.insert(Value::String("axis".to_string()), axis);
    m.insert(Value::String("range".to_string()), range);
    Value::Map(m)
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
}
