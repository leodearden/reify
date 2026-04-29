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

use crate::joints::is_joint_value;

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
            if !matches!(args.len(), 3..=5) {
                return Some(Value::Undef);
            }

            // Validate args[0] is a Mechanism Map. This guard runs
            // BEFORE the errored-mechanism short-circuit so only Maps
            // that are actually Mechanisms get the propagation path —
            // an unrelated error-bearing Map (or a test-constructed
            // Map without `kind="mechanism"`) must surface as Undef
            // rather than propagating verbatim.
            let mech_map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            if mech_map.get(&Value::String("kind".to_string()))
                != Some(&Value::String("mechanism".to_string()))
            {
                return Some(Value::Undef);
            }

            // Errored-mechanism short-circuit: if the Mechanism Map
            // already carries an "error" key, return it verbatim. This
            // locks in idempotent error propagation so callers can
            // chain `.body(...)` calls without each link re-validating
            // in a way that could mask the original error (test step-21).
            if mech_map.contains_key(&Value::String("error".to_string())) {
                return Some(args[0].clone());
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
        "body_id_of" => {
            // 2 args: (mechanism, solid).
            if args.len() != 2 {
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
            // Errored Mechanism short-circuit: an errored mechanism's
            // bodies list may be incomplete or stale (the bodies
            // recorded before the error are preserved verbatim). A
            // user who chains `body_id_of()` onto an errored
            // mechanism would otherwise get a plausible-looking Int
            // back and never see the underlying closed_chain /
            // duplicate_solid. Return Undef instead so the caller is
            // forced to reckon with the error before relying on a
            // body id. The companion test
            // `body_id_of_on_errored_mechanism_returns_undef` pins
            // this behaviour so future refactors can't silently
            // change it.
            if mech_map.contains_key(&Value::String("error".to_string())) {
                return Some(Value::Undef);
            }
            // Iterate `bodies` and return the id of the first record
            // whose `solid` field equals args[1] by structural Value
            // equality. The PRD calls for "referential identity" but
            // the v0.1 Value model only exposes structural equality —
            // see the design-decision note in plan.json.
            let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
                Some(Value::List(b)) => b,
                _ => return Some(Value::Undef),
            };
            for body in bodies {
                let body_map = match body {
                    Value::Map(b) => b,
                    _ => continue,
                };
                if body_map.get(&Value::String("solid".to_string())) == Some(&args[1]) {
                    return Some(
                        body_map
                            .get(&Value::String("id".to_string()))
                            .cloned()
                            .unwrap_or(Value::Undef),
                    );
                }
            }
            Value::Undef
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
pub(crate) fn make_world_sentinel() -> Value {
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
pub(crate) fn is_world(v: &Value) -> bool {
    match v {
        Value::Map(m) => matches!(
            m.get(&Value::String("kind".to_string())),
            Some(Value::String(s)) if s == "world"
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

/// Walk the `joint_parents` map ancestor-ward starting from `start`,
/// returning the chain of joints in **top-down** order:
/// `[oldest_recorded_ancestor, ..., parent_of_start, start]`. The world
/// sentinel is NOT included in the returned vector — callers prepend it
/// to form the canonical `[world, ..., at]` error path.
///
/// Cycle-safe: the walk is capped at `joint_parents.len() + 1` so cyclic
/// edges produced before the cycle-detection pass cannot loop here.
fn walk_to_world(joint_parents: &BTreeMap<Value, Value>, start: &Value) -> Vec<Value> {
    let mut walk = Vec::new();
    let mut current = start.clone();
    let cap = joint_parents.len() + 1;
    while walk.len() < cap {
        if is_world(&current) {
            // The world sentinel is prepended by the caller, not stored
            // mid-walk; stop here.
            break;
        }
        walk.push(current.clone());
        match joint_parents.get(&current) {
            Some(parent) => current = parent.clone(),
            None => break, // No further recorded ancestor; implicit world.
        }
    }
    // Walk accumulated child→parent (bottom-up). Reverse for top-down.
    walk.reverse();
    walk
}

/// Returns `true` if adding the edge `(at → parent)` to `joint_parents`
/// would close a cycle. The check walks the pre-edge `joint_parents`
/// from `parent` ancestor-ward; if the walk encounters `at`, the new
/// edge closes a cycle. Returns `false` if the walk reaches the world
/// sentinel or a node with no recorded parent.
///
/// Cycle-safe: bounded at `joint_parents.len() + 1` so any pre-existing
/// cycle (which would only be present in defensive scenarios — the
/// builder eagerly rejects every cycle-creating edge) cannot loop here.
fn cycle_introduced(
    pre_edge: &BTreeMap<Value, Value>,
    at: &Value,
    parent: &Value,
) -> bool {
    let mut current = parent.clone();
    let cap = pre_edge.len() + 1;
    for _ in 0..cap {
        if is_world(&current) {
            return false;
        }
        if &current == at {
            return true;
        }
        match pre_edge.get(&current) {
            Some(p) => current = p.clone(),
            None => return false,
        }
    }
    // Bound exhausted without reaching world or `at` — defensive: the
    // pre-edge graph is itself cyclic, so adding any edge "closes a
    // cycle" in the loose sense. Conservative truthy answer.
    true
}

/// Decorate an existing Mechanism Map with closed-chain or duplicate-solid
/// error fields. Preserves the input's `bodies`, `joint_parents`,
/// `next_id`, and `kind` fields verbatim and appends `error`,
/// `error_path1`, `error_path2`, `error_message`. Used by both the
/// closed-chain conflict path and (in a later step) the duplicate-solid
/// path so the error-Map shape stays uniform.
fn make_error_mechanism(
    mech_map: &BTreeMap<Value, Value>,
    error_kind: &str,
    path1: Vec<Value>,
    path2: Vec<Value>,
    message: String,
) -> Value {
    let mut new_map = mech_map.clone();
    new_map.insert(
        Value::String("error".to_string()),
        Value::String(error_kind.to_string()),
    );
    new_map.insert(
        Value::String("error_message".to_string()),
        Value::String(message),
    );
    new_map.insert(
        Value::String("error_path1".to_string()),
        Value::List(path1),
    );
    new_map.insert(
        Value::String("error_path2".to_string()),
        Value::List(path2),
    );
    Value::Map(new_map)
}

/// Append a body record to a Mechanism `Value::Map`, returning the new
/// (immutable) Mechanism Map. The 3-/4-/5-arg `body()` paths all
/// delegate here after substituting defaults for omitted arguments.
///
/// Side effects on the returned Map (vs. the input):
/// - `bodies` list grows by one record (with `id = m.next_id`).
/// - `joint_parents` records `at → parent` (existing entries with the
///   same parent are no-ops; entries with a *different* parent surface
///   a `closed_chain` error).
/// - `next_id` increments by one.
///
/// Closed-chain conflict detection runs BEFORE any state mutation. On
/// conflict, the input mechanism's `bodies`/`joint_parents`/`next_id`
/// are preserved verbatim and the returned Map carries the `error*`
/// fields. Cycle detection, duplicate-solid detection, and errored-
/// mechanism short-circuit are layered on in subsequent steps.
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

    // Duplicate-solid detection: scan `bodies` for any existing record
    // whose `solid` field is structurally equal to the new solid. Runs
    // BEFORE the closed-chain checks so duplicate-solid takes precedence
    // when both errors would fire (per design-decisions in plan.json:
    // duplicate-solid is body-local and surfaces the smaller-scope
    // diagnostic first).
    //
    // v0.1 uses structural `Value` equality — the docs §13.2 spec says
    // "by referential identity" but Reify's Value model only exposes
    // structural equality. The follow-on docs task (#2538) reconciles
    // the spec wording with the v0.1 implementation.
    //
    // Performance: this is an O(n) linear scan, making a chain of n
    // body() calls O(n²). Deliberately accepted for v0.1 — mechanisms
    // are documented (`docs/prds/kinematic-constraints.md` task 3) as
    // a handful of bodies, and an immutable Map-shaped state already
    // forces O(n) clones per call. If mechanisms ever grow large, the
    // remediation is a `seen_solids: BTreeSet<Value>` field alongside
    // `bodies` (tracked as a follow-up if a real workload demands it).
    for existing in &bodies {
        if let Value::Map(b) = existing
            && b.get(&Value::String("solid".to_string())) == Some(&solid)
        {
            return make_error_mechanism(
                mech_map,
                "duplicate_solid",
                Vec::new(),
                Vec::new(),
                "duplicate solid: solid value already attached to a body in this mechanism"
                    .to_string(),
            );
        }
    }

    // Closed-chain conflict detection: if `at` is already mapped to a
    // *different* parent, surface a `closed_chain` error before any
    // mutation. (Same-parent re-registration is a no-op overwrite.)
    if let Some(existing_parent) = joint_parents.get(&at)
        && existing_parent != &parent
    {
        let world = make_world_sentinel();
        let mut path1 = vec![world.clone()];
        path1.extend(walk_to_world(&joint_parents, existing_parent));
        path1.push(at.clone());
        let mut path2 = vec![world];
        path2.extend(walk_to_world(&joint_parents, &parent));
        path2.push(at);
        return make_error_mechanism(
            mech_map,
            "closed_chain",
            path1,
            path2,
            "closed chain detected: joint already has a different parent recorded".to_string(),
        );
    }

    // Closed-chain cycle detection: if walking from `parent` upward in
    // the pre-edge `joint_parents` reaches `at`, then adding the edge
    // `(at → parent)` would close a cycle. Surface a `closed_chain`
    // error before any mutation.
    if cycle_introduced(&joint_parents, &at, &parent) {
        let world = make_world_sentinel();
        // path1: `at`'s pre-edge ancestor chain — best-effort
        // representation of where `at` was rooted before the new edge.
        let mut path1 = vec![world.clone()];
        path1.extend(walk_to_world(&joint_parents, &at));
        // path2: pre-edge ancestor walk from `parent`, top-down,
        // appended with `at` to make the closing edge visible. The
        // cycle manifests as `at` appearing twice (once as a pre-
        // existing ancestor and once as the new closing node).
        let mut path2 = vec![world];
        path2.extend(walk_to_world(&joint_parents, &parent));
        path2.push(at.clone());
        return make_error_mechanism(
            mech_map,
            "closed_chain",
            path1,
            path2,
            "closed chain detected: new edge would close a cycle through joint_parents"
                .to_string(),
        );
    }

    // Build and append the new body record.
    bodies.push(make_body_record(
        next_id,
        solid,
        at.clone(),
        parent.clone(),
        pose,
    ));

    // Record (at → parent) in joint_parents. Same-parent re-registration
    // is a no-op overwrite (the conflict guard above already returned
    // early on a different-parent attempt).
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
        assert!(eval_builtin("body", std::slice::from_ref(&m0)).is_undef());
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

    // ── closed-chain detection: parent conflict ──────────────────────────

    /// Build a third joint distinct from j_a and j_b for the conflict
    /// scenarios. Use a different axis so the joint Maps differ
    /// structurally (they would otherwise be Value::Eq).
    fn axis_y_unit() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
    }

    /// `body()` calls that try to give the same joint two different
    /// parents (`j_x` → `j_a` from call 1, `j_x` → `j_b` from call 2)
    /// produce an errored Mechanism Map with `error="closed_chain"`,
    /// non-empty `error_message`, and `error_path1` / `error_path2`
    /// both terminating at `j_x` (path1 walks `world → ... → j_a → j_x`,
    /// path2 walks `world → ... → j_b → j_x`).
    #[test]
    fn closed_chain_via_parent_conflict_emits_error_with_both_paths() {
        // j_a, j_b distinct; j_x distinct again.
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        // Call 1: body(m0, solid_a, j_x, j_a) records j_x → j_a.
        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_x.clone(), j_a.clone()]);
        // Call 2: body(m1, solid_b, j_x, j_b) tries j_x → j_b → conflict.
        let m2 = eval_builtin("body", &[m1, solid_b, j_x.clone(), j_b.clone()]);

        let map = match m2 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("error".to_string())),
            Some(&Value::String("closed_chain".to_string())),
            "error field should be 'closed_chain'"
        );
        match map.get(&Value::String("error_message".to_string())) {
            Some(Value::String(s)) => {
                assert!(!s.is_empty(), "error_message should be non-empty");
            }
            other => panic!("expected error_message String, got {:?}", other),
        }

        // Both paths terminate at j_x.
        let path1 = match map.get(&Value::String("error_path1".to_string())) {
            Some(Value::List(p)) => p,
            other => panic!("expected error_path1 List, got {:?}", other),
        };
        let path2 = match map.get(&Value::String("error_path2".to_string())) {
            Some(Value::List(p)) => p,
            other => panic!("expected error_path2 List, got {:?}", other),
        };
        let world = eval_builtin("world", &[]);
        assert!(!path1.is_empty(), "error_path1 should be non-empty");
        assert!(!path2.is_empty(), "error_path2 should be non-empty");
        assert_eq!(
            path1.last(),
            Some(&j_x),
            "error_path1 should terminate at j_x"
        );
        assert_eq!(
            path2.last(),
            Some(&j_x),
            "error_path2 should terminate at j_x"
        );
        // path1: world → ... → j_a → j_x. j_a was never recorded as an
        // `at` value, so its walk-to-world is just [j_a]; path1 is
        // [world, j_a, j_x].
        assert_eq!(
            path1,
            &vec![world.clone(), j_a, j_x.clone()],
            "path1 should walk world → j_a → j_x"
        );
        assert_eq!(
            path2,
            &vec![world, j_b, j_x],
            "path2 should walk world → j_b → j_x"
        );
    }

    // ── closed-chain detection: joint-graph cycle ────────────────────────

    /// `body()` calls whose recorded `(at → parent)` edges introduce a
    /// cycle that doesn't conflict on a single joint's parent still
    /// produce a `closed_chain` error.
    ///
    /// Scenario: `body(m, solid_a, j_a, j_b)` then
    /// `body(m', solid_b, j_b, j_a)`. After call 1, `joint_parents`
    /// records `j_a → j_b`. After call 2 *would* record `j_b → j_a`,
    /// producing the cycle `j_a → j_b → j_a`. The conflict-on-single-
    /// joint guard (step-16) does NOT fire here because `j_b` has no
    /// existing parent recorded — the cycle is detected by the DFS
    /// from `at` through `joint_parents` (step-18).
    ///
    /// Both paths are pinned exactly, mirroring the precision of the
    /// parent-conflict test. Canonical shapes (traced through
    /// `walk_to_world` + the `cycle_introduced` arm of `append_body`):
    ///   `path1 = [world, j_b]`         — `walk_to_world({j_a:j_b}, j_b)` yields
    ///                                    `[j_b]` because j_b has no recorded parent
    ///                                    in the pre-edge graph; world prepended.
    ///   `path2 = [world, j_b, j_a, j_b]` — `walk_to_world({j_a:j_b}, j_a)` yields
    ///                                      `[j_b, j_a]` top-down; world prepended;
    ///                                      closing edge `at = j_b` appended. j_b
    ///                                      appears twice — per the `path2` comment
    ///                                      in `append_body`'s `cycle_introduced`
    ///                                      arm ("the cycle manifests as `at`
    ///                                      appearing twice").
    #[test]
    fn closed_chain_via_joint_graph_cycle_emits_error_with_cycle_path() {
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        // Call 1: body(m0, solid_a, j_a, j_b) records j_a → j_b.
        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_a.clone(), j_b.clone()]);
        // Sanity-check: call 1 succeeds (no error key).
        match &m1 {
            Value::Map(m) => assert!(
                !m.contains_key(&Value::String("error".to_string())),
                "first body() call should succeed; got error: {:?}",
                m.get(&Value::String("error".to_string()))
            ),
            other => panic!("expected Mechanism Map after call 1, got {:?}", other),
        }
        // Call 2: body(m1, solid_b, j_b, j_a) records j_b → j_a, producing
        // the cycle j_a → j_b → j_a.
        let m2 = eval_builtin("body", &[m1, solid_b, j_b.clone(), j_a.clone()]);

        let map = match m2 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map after call 2, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("error".to_string())),
            Some(&Value::String("closed_chain".to_string())),
            "cycle should be reported as closed_chain"
        );
        match map.get(&Value::String("error_message".to_string())) {
            Some(Value::String(s)) => {
                assert!(!s.is_empty(), "error_message should be non-empty");
            }
            other => panic!("expected error_message String, got {:?}", other),
        }

        let path1 = match map.get(&Value::String("error_path1".to_string())) {
            Some(Value::List(p)) => p.clone(),
            other => panic!("expected error_path1 List, got {:?}", other),
        };
        let path2 = match map.get(&Value::String("error_path2".to_string())) {
            Some(Value::List(p)) => p.clone(),
            other => panic!("expected error_path2 List, got {:?}", other),
        };
        let world = eval_builtin("world", &[]);
        assert_eq!(
            path1,
            vec![world.clone(), j_b.clone()],
            "path1 should walk world → j_b (j_b has no recorded parent in the pre-edge graph)"
        );
        assert_eq!(
            path2,
            vec![world, j_b.clone(), j_a, j_b],
            "path2 should walk world → j_b → j_a → j_b: walk_to_world({{j_a:j_b}}, j_a) \
             yields [j_b, j_a] top-down, then `at=j_b` appended as closing edge \
             (j_b appears twice — per the path2 comment in append_body's cycle_introduced arm)"
        );
    }

    // ── closed-chain detection: self-loop ────────────────────────────────

    /// `body()` with the same joint as both `at` and `parent` produces a
    /// `closed_chain` error immediately.
    ///
    /// Regression-prevention intent: today the case is caught by
    /// `cycle_introduced` comparing `current` (initialised to `parent`) with
    /// `at` on the very first iteration — with `at == parent` this evaluates
    /// to `true` immediately. An unsuspecting refactor of `cycle_introduced`
    /// that started the comparison only *after* one ancestor hop would
    /// silently pass the self-loop through. This test pins that contract.
    ///
    /// Pin shapes (fresh mechanism, `joint_parents` empty):
    ///   `path1 = [world, j]`    — `walk_to_world({}, j)` yields `[j]`; world prepended.
    ///   `path2 = [world, j, j]` — same walk yields `[j]`; world prepended;
    ///                             closing edge `at = j` appended (j appears twice).
    #[test]
    fn body_self_loop_emits_closed_chain_error() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solid".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        // Pass j as both `at` (args[2]) and `parent` (args[3]).
        let result = eval_builtin("body", &[m0, solid, j.clone(), j.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("error".to_string())),
            Some(&Value::String("closed_chain".to_string())),
            "self-loop should be reported as closed_chain"
        );
        match map.get(&Value::String("error_message".to_string())) {
            Some(Value::String(s)) => {
                assert!(!s.is_empty(), "error_message should be non-empty");
            }
            other => panic!("expected error_message String, got {:?}", other),
        }
        let path1 = match map.get(&Value::String("error_path1".to_string())) {
            Some(Value::List(p)) => p,
            other => panic!("expected error_path1 List, got {:?}", other),
        };
        let path2 = match map.get(&Value::String("error_path2".to_string())) {
            Some(Value::List(p)) => p,
            other => panic!("expected error_path2 List, got {:?}", other),
        };
        let world = eval_builtin("world", &[]);
        assert_eq!(
            path1,
            &vec![world.clone(), j.clone()],
            "path1 should walk world → j (j has no recorded parent in fresh mechanism)"
        );
        assert_eq!(
            path2,
            &vec![world, j.clone(), j],
            "path2 should walk world → j → j: walk_to_world yields [j], world prepended, \
             then closing edge `at = j` appended (j appears twice)"
        );
    }

    // ── errored-mechanism propagation ────────────────────────────────────

    /// Once a Mechanism Map carries an `error` field, subsequent
    /// `body()` calls must short-circuit and return the errored Map
    /// unchanged. This locks in idempotent error propagation so
    /// callers can write the natural `mechanism().body(...).body(...)`
    /// chain without each link re-validating (which could otherwise
    /// mask the original error).
    #[test]
    fn errored_mechanism_propagates_through_subsequent_body_calls() {
        // Build an errored mechanism via parent-conflict (cheaper to
        // set up than the cycle case; both produce error="closed_chain"
        // so the propagation contract is identical).
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_x.clone(), j_a]);
        let errored = eval_builtin("body", &[m1, solid_b, j_x, j_b]);
        // Sanity: the setup actually produced an errored mechanism.
        match &errored {
            Value::Map(m) => {
                assert_eq!(
                    m.get(&Value::String("error".to_string())),
                    Some(&Value::String("closed_chain".to_string())),
                    "setup precondition: errored mechanism has error='closed_chain'"
                );
            }
            other => panic!("expected errored Mechanism Map, got {:?}", other),
        }

        // Now call body() on the errored mechanism with fresh inputs.
        let new_j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let new_solid = Value::String("solidC".to_string());
        let propagated = eval_builtin("body", &[errored.clone(), new_solid, new_j]);

        // The propagated mechanism must equal the input errored mechanism
        // field-by-field — no new body record appended, error fields
        // preserved.
        assert_eq!(
            propagated, errored,
            "subsequent body() call on errored mechanism must return the errored Map verbatim"
        );
    }

    // ── duplicate-solid detection ────────────────────────────────────────

    /// `body()` calls that try to insert the same solid value twice
    /// produce an errored Mechanism Map with `error="duplicate_solid"`
    /// and a non-empty `error_message`.
    ///
    /// v0.1 detects duplicates by **structural** `Value` equality —
    /// the docs §13.2 spec says "by referential identity" but Reify's
    /// Value model only exposes structural equality (a clone is
    /// `Value::Eq` to its source). Tracked in the design-decisions
    /// section of plan.json. The follow-on docs task (#2538) will
    /// reconcile the spec wording with the v0.1 implementation.
    #[test]
    fn duplicate_solid_emits_error() {
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid = Value::String("solidA".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid.clone(), j_a]);
        // Reuse `solid` (same Value::String, structurally equal) — the
        // builder must reject this as a duplicate.
        let m2 = eval_builtin("body", &[m1, solid, j_b]);

        let map = match m2 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("error".to_string())),
            Some(&Value::String("duplicate_solid".to_string())),
            "error field should be 'duplicate_solid'"
        );
        match map.get(&Value::String("error_message".to_string())) {
            Some(Value::String(s)) => {
                assert!(!s.is_empty(), "error_message should be non-empty");
            }
            other => panic!("expected error_message String, got {:?}", other),
        }
    }

    // ── body_id_of() lookup ──────────────────────────────────────────────

    /// `body_id_of(m, solid)` returns `Int(body.id)` for the first body
    /// whose stored solid value equals (Value::Eq) the supplied solid;
    /// `Value::Undef` for an absent solid, a non-mechanism Map, or wrong
    /// arg count.
    #[test]
    fn body_id_of_returns_int_for_present_solid_and_undef_for_unknown() {
        let m0 = eval_builtin("mechanism", &[]);
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        let m1 = eval_builtin("body", &[m0, solid_a.clone(), j_a.clone()]);
        let m2 = eval_builtin("body", &[m1, solid_b.clone(), j_b, j_a]);

        assert_eq!(
            eval_builtin("body_id_of", &[m2.clone(), solid_a]),
            Value::Int(0),
            "first body's id is 0"
        );
        assert_eq!(
            eval_builtin("body_id_of", &[m2.clone(), solid_b]),
            Value::Int(1),
            "second body's id is 1"
        );
        assert!(
            eval_builtin(
                "body_id_of",
                &[m2.clone(), Value::String("absent".to_string())]
            )
            .is_undef(),
            "unknown solid yields Undef"
        );

        // Non-mechanism Map → Undef.
        let world = eval_builtin("world", &[]);
        assert!(
            eval_builtin("body_id_of", &[world, Value::String("anything".to_string())]).is_undef(),
            "non-mechanism Map yields Undef"
        );

        // Wrong arity → Undef.
        assert!(eval_builtin("body_id_of", &[]).is_undef());
        assert!(eval_builtin("body_id_of", std::slice::from_ref(&m2)).is_undef());
        assert!(eval_builtin(
            "body_id_of",
            &[m2, Value::String("a".to_string()), Value::Int(1)]
        )
        .is_undef());
    }

    /// `body_id_of` on an errored Mechanism returns `Value::Undef` —
    /// not a body id from the (possibly stale) pre-error bodies list.
    /// Pins the design choice noted at the `body_id_of` arm in
    /// `eval_mechanism`: a user who chains `body_id_of()` onto an
    /// errored mechanism must reckon with the error before getting a
    /// plausible-looking Int back. Companion to suggestion #2 in the
    /// reviewer's amendment pass.
    #[test]
    fn body_id_of_on_errored_mechanism_returns_undef() {
        // Build an errored mechanism via parent-conflict (cheaper to
        // set up than the cycle / duplicate-solid cases; the contract
        // is uniform across every error kind).
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a.clone(), j_x.clone(), j_a]);
        let errored = eval_builtin("body", &[m1, solid_b, j_x, j_b]);
        // Sanity: setup actually produced an errored mechanism with
        // solid_a still in the pre-error bodies list.
        match &errored {
            Value::Map(m) => {
                assert_eq!(
                    m.get(&Value::String("error".to_string())),
                    Some(&Value::String("closed_chain".to_string())),
                    "setup precondition: errored mechanism has error='closed_chain'"
                );
            }
            other => panic!("expected errored Mechanism Map, got {:?}", other),
        }

        // body_id_of on the errored mechanism must yield Undef even
        // though solid_a IS present in the pre-error bodies list (the
        // "closed_chain" error decorates the mechanism but preserves
        // the bodies prefix from before the conflicting body() call).
        assert!(
            eval_builtin("body_id_of", &[errored, solid_a]).is_undef(),
            "body_id_of on errored mechanism must yield Undef, even for a \
             solid present in the pre-error bodies list"
        );
    }

    /// `body()` on a Map that carries an "error" key but is NOT a
    /// Mechanism (no `kind="mechanism"`) returns `Value::Undef` — the
    /// errored-mechanism short-circuit must NOT fire on unrelated
    /// error-bearing Maps. Pins the validation order fixed in
    /// suggestion #1 of the reviewer's amendment pass: kind validation
    /// runs BEFORE the error short-circuit so the validation contract
    /// survives a regression that produced an unrelated error-bearing
    /// Map (or a test-constructed Map without `kind="mechanism"`).
    #[test]
    fn body_error_map_without_mechanism_kind_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());

        // A Map carrying `error` but with `kind="other"` (or no kind).
        let mut bogus = BTreeMap::new();
        bogus.insert(
            Value::String("kind".to_string()),
            Value::String("other".to_string()),
        );
        bogus.insert(
            Value::String("error".to_string()),
            Value::String("synthetic_error".to_string()),
        );
        let bogus_map = Value::Map(bogus);

        assert!(
            eval_builtin("body", &[bogus_map, solid.clone(), j.clone()]).is_undef(),
            "body() on a non-Mechanism Map must surface Undef even when an \
             'error' key is present"
        );

        // Also pin the no-kind variant.
        let mut bogus_no_kind = BTreeMap::new();
        bogus_no_kind.insert(
            Value::String("error".to_string()),
            Value::String("synthetic_error".to_string()),
        );
        let bogus_no_kind_map = Value::Map(bogus_no_kind);
        assert!(
            eval_builtin("body", &[bogus_no_kind_map, solid, j]).is_undef(),
            "body() on a Map with no `kind` field must surface Undef even \
             when an 'error' key is present"
        );
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
