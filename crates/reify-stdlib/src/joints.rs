use std::collections::BTreeMap;

use reify_types::Value;

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
            make_joint("prismatic", args[0].clone(), args[1].clone())
        }
        _ => return None,
    })
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
}
