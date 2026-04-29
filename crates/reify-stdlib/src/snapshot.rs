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
        "snapshot" => {
            // Validation surface (each guard short-circuits to
            // Value::Undef BEFORE the FK walk):
            //   args.len() == 2                                → arity guard
            //   args[0] is Map with kind="mechanism"           → mechanism guard
            //   args[1] is Value::List                         → bindings guard
            // Errored-mechanism short-circuit and per-binding
            // shape validation are layered on in subsequent steps.
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let mech_map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            if mech_map.get(&Value::String("kind".to_string()))
                != Some(&Value::String("mechanism".to_string()))
            {
                return Some(Value::Undef);
            }
            // Bindings argument: must be a List. Per-entry shape
            // validation lands in a later step.
            if !matches!(&args[1], Value::List(_)) {
                return Some(Value::Undef);
            }
            // Read the mechanism's `bodies` list — defense-in-depth
            // (the `kind` guard above ensures this is well-formed
            // for any value produced by `mechanism()` / `body()`).
            let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
                Some(Value::List(b)) => b,
                _ => return Some(Value::Undef),
            };
            if bodies.is_empty() {
                return Some(make_empty_snapshot());
            }
            // Non-empty FK walk lands in subsequent steps.
            make_empty_snapshot()
        }
        _ => return None,
    })
}

/// Build the canonical empty Snapshot `Value::Map`.
///
/// Shape (alphabetical key order, matching `BTreeMap` iteration):
/// - `bodies` → `Value::List(vec![])`
/// - `kind` → `Value::String("snapshot")`
fn make_empty_snapshot() -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("bodies".to_string()), Value::List(vec![]));
    m.insert(
        Value::String("kind".to_string()),
        Value::String("snapshot".to_string()),
    );
    Value::Map(m)
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

    // ── snapshot() empty case: pin canonical Snapshot Map shape ───────────

    /// `snapshot(empty_mechanism, [])` returns the canonical empty
    /// Snapshot: a `Value::Map` with `kind="snapshot"` and `bodies` =
    /// empty `Value::List`. Pins the shape so subsequent FK + accessor
    /// steps can rely on these fields existing.
    #[test]
    fn snapshot_on_empty_mechanism_returns_empty_snapshot_map() {
        let m0 = eval_builtin("mechanism", &[]);
        let result = eval_builtin("snapshot", &[m0, Value::List(vec![])]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Snapshot Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("snapshot".to_string())),
            "kind field should be 'snapshot'"
        );
        assert_eq!(
            map.get(&Value::String("bodies".to_string())),
            Some(&Value::List(vec![])),
            "bodies field should be an empty List"
        );
    }

    // ── Single-body FK (parent = world) ───────────────────────────────────

    /// Decompose a `Value::Transform` into (rotation_quaternion, translation_si)
    /// for assertion purposes. Mirrors the test-side decompose pattern in
    /// `geometry.rs::tests`.
    fn decompose_transform_for_assert(t: &Value) -> ((f64, f64, f64, f64), [f64; 3]) {
        let (rot, trans) = match t {
            Value::Transform {
                rotation,
                translation,
            } => (rotation.as_ref(), translation.as_ref()),
            other => panic!("expected Value::Transform, got {:?}", other),
        };
        let (rw, rx, ry, rz) = match rot {
            Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
            other => panic!("expected Value::Orientation, got {:?}", other),
        };
        let comps = match trans {
            Value::Vector(c) if c.len() == 3 => c,
            other => panic!("expected Value::Vector len=3, got {:?}", other),
        };
        let read = |v: &Value| -> f64 {
            match v {
                Value::Real(r) => *r,
                Value::Scalar { si_value, .. } => *si_value,
                other => panic!("expected numeric component, got {:?}", other),
            }
        };
        ((rw, rx, ry, rz), [read(&comps[0]), read(&comps[1]), read(&comps[2])])
    }

    /// Simplest non-empty FK case: one body at a prismatic joint along +X
    /// (range 0..1m), parent=world, identity pose. Bind the joint to 2 mm.
    /// The body's `world_transform` should have translation (0.002, 0, 0)
    /// and identity rotation.
    #[test]
    fn snapshot_single_body_world_parent_records_bound_joint_transform() {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());
        let m1 = eval_builtin("body", &[m0, solid, j.clone()]);

        let v = Value::length(0.002);
        let binding = eval_builtin("bind", &[j, v]);

        let s = eval_builtin("snapshot", &[m1, Value::List(vec![binding])]);
        let smap = match s {
            Value::Map(m) => m,
            other => panic!("expected Snapshot Map, got {:?}", other),
        };

        // bodies list has one record
        let bodies = match smap.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected snapshot bodies List, got {:?}", other),
        };
        assert_eq!(bodies.len(), 1, "snapshot bodies should have exactly one record");

        // Body record's world_transform: translation (0.002, 0, 0), identity rotation.
        let body = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("expected snapshot body record Map, got {:?}", other),
        };
        let wt = body
            .get(&Value::String("world_transform".to_string()))
            .expect("body record must carry a world_transform field");
        let ((rw, rx, ry, rz), [tx, ty, tz]) = decompose_transform_for_assert(wt);
        assert!((rw - 1.0).abs() < 1e-12, "rotation w should be 1.0, got {}", rw);
        assert!(rx.abs() < 1e-12, "rotation x should be 0, got {}", rx);
        assert!(ry.abs() < 1e-12, "rotation y should be 0, got {}", ry);
        assert!(rz.abs() < 1e-12, "rotation z should be 0, got {}", rz);
        assert!((tx - 0.002).abs() < 1e-12, "tx should be 0.002 m, got {}", tx);
        assert!(ty.abs() < 1e-12, "ty should be 0, got {}", ty);
        assert!(tz.abs() < 1e-12, "tz should be 0, got {}", tz);
    }
}
