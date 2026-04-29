//! Forward-kinematics snapshot stdlib (task 2535).
//!
//! Implements task 4 (FK tree-walk evaluator) and task 6 (Snapshot
//! value-type accessors) of the kinematic-constraints PRD.
//!
//! The Snapshot is encoded as a `Value::Map` paralleling the Mechanism
//! Map: `{ "kind": "snapshot", "bodies": List<body_record> }` where each
//! body record carries `{ id, solid, pose, world_transform }` (alphabetical
//! key order, matching `BTreeMap` iteration).
//!
//! Surface:
//!   - `bind(joint, value)`             → binding Map
//!   - `snapshot(mechanism, bindings)`  → Snapshot Map
//!   - `bodies(snapshot)`               → List<Int>
//!   - `transform_of(snapshot, id)`     → Transform | Undef
//!   - `center_of_mass(s, [densities])` → Point3<Length> | Undef
//!   - `bounding_box(snapshot)`         → Map { min, max } | Undef

use std::collections::BTreeMap;

use reify_types::Value;

use crate::joints::is_joint_value;

/// Evaluate a snapshot/FK stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_snapshot(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "bind" => {
            // Validation surface (each guard short-circuits to
            // Value::Undef BEFORE constructing the binding Map):
            //   args.len() == 2          → arity guard
            //   is_joint_value(args[0])  → joint-arg guard
            // The motion value (args[1]) is stored verbatim — downstream
            // `transform_at` handles dimension-checking when consumed.
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if !is_joint_value(&args[0]) {
                return Some(Value::Undef);
            }
            make_binding(args[0].clone(), args[1].clone())
        }
        _ => return None,
    })
}

/// Build a binding `Value::Map` with the standard three-key layout:
/// `kind`, `joint`, `value` (alphabetical, matching `BTreeMap` iteration).
/// Mirrors `make_joint`/`make_coupling` in `joints.rs` and the kind-
/// discriminated Map convention used across the stdlib value types.
fn make_binding(joint: Value, value: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("binding".to_string()),
    );
    m.insert(Value::String("joint".to_string()), joint);
    m.insert(Value::String("value".to_string()), value);
    Value::Map(m)
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_types::Value;

    // ── Joint fixtures (mirror the joints.rs test fixtures) ───────────────

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

    // ── bind(joint, value): happy path ────────────────────────────────────

    /// `bind(joint, value)` returns a `Value::Map` with shape
    /// `{kind="binding", joint=<input joint>, value=<input value>}`. The
    /// `joint` and `value` fields are stored verbatim from the call —
    /// downstream `transform_at` handles dimension-checking when consumed.
    #[test]
    fn bind_returns_binding_map_with_kind_joint_and_value() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let v = Value::length(0.002);
        let result = eval_builtin("bind", &[j.clone(), v.clone()]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map binding record, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("binding".to_string())),
            "kind field should be 'binding'"
        );
        assert_eq!(
            map.get(&Value::String("joint".to_string())),
            Some(&j),
            "joint field should be the input joint verbatim"
        );
        assert_eq!(
            map.get(&Value::String("value".to_string())),
            Some(&v),
            "value field should be the input motion value verbatim"
        );
    }

    // ── bind() input validation: full surface returns Undef ───────────────
    //
    // Validation allow-list (matches `eval_snapshot::bind` arm):
    //   args.len() == 2          → arity guard
    //   is_joint_value(args[0])  → joint-arg guard
    // Motion value (args[1]) is stored verbatim — downstream
    // `transform_at` is the single point of dimension/finite-value
    // validation. These tests pin every other input shape as Undef.

    /// `bind()` with an arity outside {2} returns Undef.
    #[test]
    fn bind_wrong_arity_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let v = Value::length(0.002);

        // 0, 1, 3 args
        assert!(eval_builtin("bind", &[]).is_undef());
        assert!(eval_builtin("bind", std::slice::from_ref(&j)).is_undef());
        assert!(eval_builtin("bind", &[j, v.clone(), v]).is_undef());
    }

    /// `bind(non_joint, value)` returns Undef when args[0] is not a
    /// joint value. Covers `Value::Real`, `Value::String`, and the
    /// world sentinel — all three are non-joint Map/non-Map shapes
    /// that must be rejected before binding-Map construction.
    #[test]
    fn bind_non_joint_arg_returns_undef() {
        let v = Value::length(0.002);

        // Real (not a Map at all)
        assert!(eval_builtin("bind", &[Value::Real(1.0), v.clone()]).is_undef());

        // String (not a Map at all)
        assert!(eval_builtin(
            "bind",
            &[Value::String("not a joint".to_string()), v.clone()]
        )
        .is_undef());

        // World sentinel — a Map with kind="world", but NOT one of the
        // joint kinds in JOINT_KINDS, so it must be rejected.
        let world = eval_builtin("world", &[]);
        assert!(eval_builtin("bind", &[world, v]).is_undef());
    }
}
