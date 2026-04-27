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
        _ => return None,
    })
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
}
