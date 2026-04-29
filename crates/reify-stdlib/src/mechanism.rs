//! Mechanism builder stdlib (task 2528).
//!
//! Implements the v0.1 `mechanism().body(...)` builder per
//! `docs/prds/kinematic-constraints.md` task 3 and `docs/reify-stdlib-reference.md` §13.2.
//!
//! Mechanism state is encoded as a `Value::Map` with the shape:
//! `{ "kind": "mechanism", "bodies": List(body_record...), "joint_parents": Map(joint→parent), "next_id": Int(N) }`.
//! On error the Map additionally carries `error`, `error_path1`, `error_path2`,
//! and `error_message` fields. See plan §"Mechanism Map shape".
//!
//! Diagnostic emission via `EvalResult.diagnostics` is deferred to the
//! snapshot/eval-pipeline integration (`DiagnosticCode::KinematicClosedChain`
//! and `DiagnosticCode::MechanismDuplicateSolid` are reserved in
//! `reify-types/src/diagnostics.rs` for that future integration).

use std::collections::BTreeMap;

use reify_types::Value;

/// Evaluate a mechanism stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_mechanism(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "mechanism" => {
            if !args.is_empty() {
                return Some(Value::Undef);
            }
            make_empty_mechanism()
        }
        "world" => {
            if !args.is_empty() {
                return Some(Value::Undef);
            }
            make_world_sentinel()
        }
        "body" => {
            // Dispatch on arity. The 3-arg (default parent = world,
            // identity pose), 4-arg (explicit parent, identity pose),
            // and 5-arg (explicit parent + pose) forms all delegate to
            // the same `append_body` core after substituting defaults
            // for any omitted argument.
            //
            // Validation surface (each guard short-circuits to
            // Value::Undef BEFORE any state mutation; pinned by the
            // step-11 input-validation test block):
            //   args.len() ∈ {3, 4, 5}                  → arity guard
            //   args[0] is a Map with kind="mechanism"  → mechanism guard
            //   args[2] is a joint value                → at-arg guard
            //   args[3] is a joint value or world       → parent guard (4/5-arg)
            //   args[4] is a Value::Transform           → pose guard (5-arg)
            if !matches!(args.len(), 3 | 4 | 5) {
                return Some(Value::Undef);
            }

            // Validate args[0] is a Mechanism Map.
            let mech_map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            if mech_map.get(&Value::String("kind".to_string()))
                != Some(&Value::String("mechanism".to_string()))
            {
                return Some(Value::Undef);
            }

            // Validate args[2] is a joint value.
            if !is_joint_value(&args[2]) {
                return Some(Value::Undef);
            }

            // Resolve the parent argument: 3-arg form defaults to the
            // world sentinel; 4- and 5-arg forms take args[3] which
            // must be a joint value or the world sentinel.
            let parent = if args.len() >= 4 {
                if !is_joint_value(&args[3]) && !is_world(&args[3]) {
                    return Some(Value::Undef);
                }
                args[3].clone()
            } else {
                make_world_sentinel()
            };

            // Resolve the pose argument: 3- and 4-arg forms default to
            // the identity transform; the 5-arg form takes args[4]
            // which must be a Value::Transform.
            let pose = if args.len() == 5 {
                if !matches!(&args[4], Value::Transform { .. }) {
                    return Some(Value::Undef);
                }
                args[4].clone()
            } else {
                identity_transform()
            };

            append_body(mech_map, args[1].clone(), args[2].clone(), parent, pose)
        }
        _ => return None,
    })
}

/// Build the canonical empty Mechanism `Value::Map`.
///
/// Shape (alphabetical key order, matching `BTreeMap` iteration):
/// - `bodies` → `Value::List(vec![])`
/// - `joint_parents` → `Value::Map(BTreeMap::new())`
/// - `kind` → `Value::String("mechanism")`
/// - `next_id` → `Value::Int(0)`
///
/// Parallel to `make_joint`/`make_coupling` in `joints.rs`.
fn make_empty_mechanism() -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("bodies".to_string()), Value::List(vec![]));
    m.insert(
        Value::String("joint_parents".to_string()),
        Value::Map(BTreeMap::new()),
    );
    m.insert(
        Value::String("kind".to_string()),
        Value::String("mechanism".to_string()),
    );
    m.insert(Value::String("next_id".to_string()), Value::Int(0));
    Value::Map(m)
}

/// Build the world-frame sentinel `Value::Map` with the single key
/// `kind = "world"`. The sentinel is the implicit ground-frame root of
/// every Mechanism DAG and the default `parent` argument for `body()`
/// when omitted (`docs/reify-stdlib-reference.md` §13.2).
fn make_world_sentinel() -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("world".to_string()),
    );
    Value::Map(m)
}

/// Returns true when `v` is the world-frame sentinel — a `Value::Map`
/// whose `kind` field equals `"world"`. Used by `body()` parent-arg
/// validation (the world sentinel is an acceptable parent value).
fn is_world(v: &Value) -> bool {
    match v {
        Value::Map(m) => matches!(
            m.get(&Value::String("kind".to_string())),
            Some(Value::String(s)) if s == "world"
        ),
        _ => false,
    }
}

/// Returns true when `v` is a joint `Value::Map` — a Map whose `kind`
/// field is one of `"prismatic"`, `"revolute"`, or `"coupling"`. Used
/// by `body()` for `at`-arg validation and by the 4-arg form for
/// parent-arg validation (joint values OR the world sentinel are
/// acceptable parents).
///
/// Mirrors the kind-discriminator pattern in
/// `joints.rs::transform_at` and `joints.rs::joint_jacobian` (the
/// `kind in {"prismatic","revolute","coupling"}` guard). Kept private
/// to `mechanism.rs` for now; if a third call site emerges, it can be
/// promoted to a shared helpers module.
fn is_joint_value(v: &Value) -> bool {
    match v {
        Value::Map(m) => matches!(
            m.get(&Value::String("kind".to_string())),
            Some(Value::String(s))
                if matches!(s.as_str(), "prismatic" | "revolute" | "coupling")
        ),
        _ => false,
    }
}

/// Build the canonical identity `Value::Transform` (zero translation,
/// unit-quaternion rotation). Used as the default `pose` argument
/// when omitted from a `body()` call.
///
/// Mirrors the identity-rotation construction in
/// `joints.rs::transform_at_simple_joint` (the prismatic arm's
/// `Value::Orientation { w: 1.0, ... }` block).
fn identity_transform() -> Value {
    let rotation = Value::Orientation {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
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

/// Build a body record `Value::Map` with the standard five-key layout:
/// `at`, `id`, `parent`, `pose`, `solid` (alphabetical, matching `BTreeMap`
/// iteration). Parallel to `make_joint`/`make_coupling` in `joints.rs`.
fn make_body_record(id: i64, solid: Value, at: Value, parent: Value, pose: Value) -> Value {
    let mut b = BTreeMap::new();
    b.insert(Value::String("at".to_string()), at);
    b.insert(Value::String("id".to_string()), Value::Int(id));
    b.insert(Value::String("parent".to_string()), parent);
    b.insert(Value::String("pose".to_string()), pose);
    b.insert(Value::String("solid".to_string()), solid);
    Value::Map(b)
}

/// Append a body record to a Mechanism `Value::Map`, returning the new
/// (immutable) Mechanism Map. The 3-/4-/5-arg `body()` paths all
/// delegate here after substituting defaults for omitted arguments.
///
/// Side effects on the returned Map (vs. the input):
/// - `bodies` list grows by one record (with `id = m.next_id`).
/// - `joint_parents` records `at → parent` (existing entries are kept;
///   conflict detection lands in a later step).
/// - `next_id` increments by one.
///
/// Closed-chain conflict detection, cycle detection, duplicate-solid
/// detection, and errored-mechanism short-circuit are all layered on
/// in subsequent steps. This step is the unconditional happy-path
/// implementation.
fn append_body(
    mech_map: &BTreeMap<Value, Value>,
    solid: Value,
    at: Value,
    parent: Value,
    pose: Value,
) -> Value {
    // Extract current bodies / joint_parents / next_id with defense-
    // in-depth fallbacks (the caller validated `kind = "mechanism"`).
    let mut bodies = match mech_map.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b.clone(),
        _ => return Value::Undef,
    };
    let mut joint_parents = match mech_map.get(&Value::String("joint_parents".to_string())) {
        Some(Value::Map(jp)) => jp.clone(),
        _ => return Value::Undef,
    };
    let next_id = match mech_map.get(&Value::String("next_id".to_string())) {
        Some(Value::Int(n)) => *n,
        _ => return Value::Undef,
    };

    // Build and append the new body record.
    bodies.push(make_body_record(
        next_id,
        solid,
        at.clone(),
        parent.clone(),
        pose,
    ));

    // Record (at → parent) in joint_parents. If the entry already
    // exists with the same parent, this is a no-op overwrite. Conflict
    // detection (different parent) is added in a later step.
    joint_parents.insert(at, parent);

    // Build the new Mechanism Map. Preserve the input map's other
    // fields verbatim (e.g. an "error" key from a future short-circuit
    // path; today there are no other fields beyond the four canonical
    // ones).
    let mut new_map = mech_map.clone();
    new_map.insert(Value::String("bodies".to_string()), Value::List(bodies));
    new_map.insert(
        Value::String("joint_parents".to_string()),
        Value::Map(joint_parents),
    );
    new_map.insert(Value::String("next_id".to_string()), Value::Int(next_id + 1));
    Value::Map(new_map)
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_types::Value;
    use std::collections::BTreeMap;

    // ── mechanism() constructor: happy path ────────────────────────────────

    /// `mechanism()` returns a `Value::Map` with the four canonical fields
    /// (`kind = "mechanism"`, empty `bodies` list, empty `joint_parents` map,
    /// `next_id = 0`). Pins the empty-Mechanism shape so subsequent `body()`
    /// builders can rely on these fields existing.
    #[test]
    fn mechanism_returns_empty_map() {
        let result = eval_builtin("mechanism", &[]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("mechanism".to_string())),
            "kind field should be 'mechanism'"
        );
        assert_eq!(
            map.get(&Value::String("bodies".to_string())),
            Some(&Value::List(vec![])),
            "bodies field should be an empty List"
        );
        assert_eq!(
            map.get(&Value::String("joint_parents".to_string())),
            Some(&Value::Map(BTreeMap::new())),
            "joint_parents field should be an empty Map"
        );
        assert_eq!(
            map.get(&Value::String("next_id".to_string())),
            Some(&Value::Int(0)),
            "next_id field should be Int(0)"
        );
    }

    /// `mechanism(...)` with any non-zero arg count returns `Value::Undef`,
    /// matching the stdlib convention for wrong-arity constructors.
    #[test]
    fn mechanism_with_args_returns_undef() {
        assert!(eval_builtin("mechanism", &[Value::Int(0)]).is_undef());
        assert!(eval_builtin("mechanism", &[Value::Int(0), Value::Int(1)]).is_undef());
        assert!(eval_builtin("mechanism", &[Value::Real(1.0)]).is_undef());
    }

    // ── world() sentinel: happy path ───────────────────────────────────────

    /// `world()` returns the world-frame sentinel as a `Value::Map` with the
    /// single key `kind = "world"`. This singleton-shape Map is the implicit
    /// ground-frame root of every Mechanism DAG and the default `parent`
    /// argument when omitted from a `body()` call (see docs/reify-stdlib-
    /// reference.md §13.2 and the design-decisions block in plan.json).
    #[test]
    fn world_returns_singleton_shape_map() {
        let result = eval_builtin("world", &[]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("world".to_string())),
            "kind field should be 'world'"
        );
        assert_eq!(
            map.len(),
            1,
            "world sentinel should have exactly one key (kind), got {} keys",
            map.len()
        );
    }

    /// `world(...)` with any non-zero arg count returns `Value::Undef`.
    #[test]
    fn world_with_args_returns_undef() {
        assert!(eval_builtin("world", &[Value::Int(0)]).is_undef());
        assert!(eval_builtin("world", &[Value::Int(0), Value::Int(1)]).is_undef());
        assert!(eval_builtin("world", &[Value::Real(1.0)]).is_undef());
    }

    // ── body() 3-arg form (default parent = world, identity pose) ─────────

    /// Test fixtures (copies of the joint fixtures in joints.rs::tests). A
    /// follow-up could promote these to a shared internal helpers module;
    /// for v0.1 we duplicate to keep the cross-module wiring minimal.
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

    /// Build the canonical identity `Value::Transform` (zero translation,
    /// unit-quaternion rotation). Mirror of the default-pose helper used
    /// inside `mechanism.rs`'s `append_body`.
    fn identity_transform_value() -> Value {
        Value::Transform {
            rotation: Box::new(Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(Value::Vector(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
        }
    }

    /// `body(m, solid, j)` with the 3-arg form appends a body record with
    /// id=0, the supplied solid+at, parent defaulted to the world sentinel,
    /// and pose defaulted to the identity transform. The Mechanism map's
    /// `next_id` advances to 1 and `joint_parents` records `j → world`.
    #[test]
    fn body_three_args_appends_record_with_default_parent_and_pose() {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());

        let m1 = eval_builtin("body", &[m0, solid.clone(), j.clone()]);
        let map = match m1 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };

        // bodies list has one entry
        let bodies = match map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected bodies List, got {:?}", other),
        };
        assert_eq!(bodies.len(), 1, "bodies should have exactly one record");

        let body = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("expected body record Map, got {:?}", other),
        };
        assert_eq!(
            body.get(&Value::String("id".to_string())),
            Some(&Value::Int(0)),
            "body id should be Int(0) for the first appended body"
        );
        assert_eq!(
            body.get(&Value::String("solid".to_string())),
            Some(&solid),
            "body record's solid field should match"
        );
        assert_eq!(
            body.get(&Value::String("at".to_string())),
            Some(&j),
            "body record's at field should equal the supplied joint"
        );

        // Parent defaulted to world sentinel
        let world = eval_builtin("world", &[]);
        assert_eq!(
            body.get(&Value::String("parent".to_string())),
            Some(&world),
            "3-arg body() defaults parent to world sentinel"
        );

        // Pose defaulted to identity transform
        assert_eq!(
            body.get(&Value::String("pose".to_string())),
            Some(&identity_transform_value()),
            "3-arg body() defaults pose to identity"
        );

        // next_id is now Int(1)
        assert_eq!(
            map.get(&Value::String("next_id".to_string())),
            Some(&Value::Int(1)),
            "next_id should advance to 1 after appending the first body"
        );

        // joint_parents records j → world
        let jp = match map.get(&Value::String("joint_parents".to_string())) {
            Some(Value::Map(jp)) => jp,
            other => panic!("expected joint_parents Map, got {:?}", other),
        };
        assert_eq!(
            jp.get(&j),
            Some(&world),
            "joint_parents should record j → world for the 3-arg default"
        );
        assert_eq!(jp.len(), 1, "joint_parents should have exactly one entry");
    }

    // ── body() 4-arg form (explicit parent joint) ────────────────────────

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

    /// `body(m, solid, at, parent)` with the 4-arg form threads the
    /// explicit parent joint through to the body record and to
    /// `joint_parents`. Builds the chain `body(m0, solid_a, j_a)` →
    /// `body(m1, solid_b, j_b, j_a)` and asserts:
    ///   - the second body's `parent` field equals `j_a`
    ///   - `joint_parents` carries both `j_a → world` (from call 1)
    ///     and `j_b → j_a` (from call 2)
    ///   - poses for both bodies remain the identity transform
    #[test]
    fn body_four_args_records_explicit_parent_joint() {
        let m0 = eval_builtin("mechanism", &[]);
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        let m1 = eval_builtin("body", &[m0, solid_a.clone(), j_a.clone()]);
        let m2 = eval_builtin(
            "body",
            &[m1, solid_b.clone(), j_b.clone(), j_a.clone()],
        );

        let map = match m2 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };

        let bodies = match map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected bodies List, got {:?}", other),
        };
        assert_eq!(bodies.len(), 2, "bodies should have two records");

        // Second body record's parent equals j_a.
        let body1 = match &bodies[1] {
            Value::Map(b) => b,
            other => panic!("expected body record Map, got {:?}", other),
        };
        assert_eq!(
            body1.get(&Value::String("parent".to_string())),
            Some(&j_a),
            "4-arg body() records the supplied parent joint"
        );
        assert_eq!(
            body1.get(&Value::String("id".to_string())),
            Some(&Value::Int(1)),
            "second body's id should be Int(1)"
        );
        // Pose for both bodies remains identity.
        assert_eq!(
            body1.get(&Value::String("pose".to_string())),
            Some(&identity_transform_value()),
            "4-arg body() defaults pose to identity"
        );

        // joint_parents has both edges.
        let jp = match map.get(&Value::String("joint_parents".to_string())) {
            Some(Value::Map(jp)) => jp,
            other => panic!("expected joint_parents Map, got {:?}", other),
        };
        let world = eval_builtin("world", &[]);
        assert_eq!(
            jp.get(&j_a),
            Some(&world),
            "joint_parents preserves j_a → world from the first call"
        );
        assert_eq!(
            jp.get(&j_b),
            Some(&j_a),
            "joint_parents records j_b → j_a from the 4-arg call"
        );
        assert_eq!(jp.len(), 2, "joint_parents should have exactly two entries");
    }

    // ── body() 5-arg form (explicit pose) ────────────────────────────────

    /// Build a non-identity pose: zero rotation, +1mm x-translation. Used
    /// to verify the 5-arg form's pose argument is threaded verbatim.
    fn pose_translate_1mm_x() -> Value {
        Value::Transform {
            rotation: Box::new(Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(Value::Vector(vec![
                Value::length(0.001),
                Value::length(0.0),
                Value::length(0.0),
            ])),
        }
    }

    /// `body(m, solid, at, parent, pose)` with the 5-arg form threads
    /// the explicit pose through to the body record.
    #[test]
    fn body_five_args_records_explicit_pose() {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());
        let world = eval_builtin("world", &[]);
        let custom = pose_translate_1mm_x();

        let m1 = eval_builtin(
            "body",
            &[m0, solid.clone(), j.clone(), world, custom.clone()],
        );

        let map = match m1 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };
        let bodies = match map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected bodies List, got {:?}", other),
        };
        let body = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("expected body record Map, got {:?}", other),
        };
        assert_eq!(
            body.get(&Value::String("pose".to_string())),
            Some(&custom),
            "5-arg body() threads the supplied pose through to the body record"
        );
    }

    // ── body() input validation: full surface returns Undef ──────────────

    /// `body()` with an arity outside {3, 4, 5} returns Undef. Pins the
    /// arity allow-list so future maintainers don't accidentally accept
    /// a 2- or 6-arg form by extending the inner match.
    #[test]
    fn body_wrong_arity_returns_undef() {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());
        let world = eval_builtin("world", &[]);
        let pose = identity_transform_value();

        // 0 / 1 / 2 args
        assert!(eval_builtin("body", &[]).is_undef());
        assert!(eval_builtin("body", &[m0.clone()]).is_undef());
        assert!(eval_builtin("body", &[m0.clone(), solid.clone()]).is_undef());

        // 6 args
        let extra = Value::String("extra".to_string());
        assert!(eval_builtin(
            "body",
            &[
                m0.clone(),
                solid.clone(),
                j.clone(),
                world.clone(),
                pose.clone(),
                extra,
            ]
        )
        .is_undef());
    }

    /// `body(non_mechanism, ...)` returns Undef when args[0] is not a
    /// Mechanism Map (here: a bare Real).
    #[test]
    fn body_non_mechanism_arg_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());

        // Non-Map first arg.
        assert!(eval_builtin("body", &[Value::Real(1.0), solid.clone(), j.clone()]).is_undef());

        // Map but not a Mechanism Map (kind="world" instead of "mechanism").
        let world = eval_builtin("world", &[]);
        assert!(eval_builtin("body", &[world, solid, j]).is_undef());
    }

    /// `body(m, solid, non_joint)` returns Undef when args[2] is not a
    /// joint value (here: a bare String).
    #[test]
    fn body_non_joint_at_arg_returns_undef() {
        let m0 = eval_builtin("mechanism", &[]);
        let solid = Value::String("solidA".to_string());

        assert!(eval_builtin(
            "body",
            &[m0, solid, Value::String("foo".to_string())]
        )
        .is_undef());
    }

    /// 4-arg `body(m, solid, j, non_joint_non_world)` returns Undef when
    /// args[3] is neither a joint value nor the world sentinel (here: a
    /// bare Real).
    #[test]
    fn body_non_joint_non_world_parent_arg_returns_undef() {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());

        assert!(eval_builtin("body", &[m0, solid, j, Value::Real(1.0)]).is_undef());
    }

    /// 5-arg body() with a non-Transform pose argument returns Undef.
    #[test]
    fn body_five_args_non_transform_pose_returns_undef() {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());
        let world = eval_builtin("world", &[]);

        // Real, Int, String, List, Map all reject as poses.
        for bad_pose in [
            Value::Real(0.0),
            Value::Int(1),
            Value::String("not a transform".to_string()),
            Value::List(vec![]),
            Value::Map(BTreeMap::new()),
        ] {
            let result = eval_builtin(
                "body",
                &[
                    m0.clone(),
                    solid.clone(),
                    j.clone(),
                    world.clone(),
                    bad_pose.clone(),
                ],
            );
            assert!(
                result.is_undef(),
                "pose={:?} should produce Undef, got {:?}",
                bad_pose,
                result
            );
        }
    }
}
