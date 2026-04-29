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

use crate::eval_builtin;
use crate::joints::is_joint_value;
use crate::loop_closure::joint_range_midpoint;
use crate::mechanism::is_world;

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
            // Errored-mechanism short-circuit (mirrors `body_id_of`'s
            // arm in mechanism.rs): a user who chains `snapshot()`
            // onto an errored mechanism must reckon with the error
            // before getting a plausible-looking Snapshot back.
            // Layered AFTER the kind validation so a non-mechanism
            // Map carrying an unrelated `error` key still hits the
            // mechanism-kind guard above, not this short-circuit.
            if mech_map.contains_key(&Value::String("error".to_string())) {
                return Some(Value::Undef);
            }
            // Bindings argument: must be a List, and every entry
            // must be a binding Map (kind="binding" with present
            // `joint`/`value` fields).  Whole-call rejection on any
            // malformed entry — silent skipping would paper over a
            // bug at the call site (e.g., a swapped arg order).
            let bindings_entries = match &args[1] {
                Value::List(b) => b,
                _ => return Some(Value::Undef),
            };
            for entry in bindings_entries {
                let map = match entry {
                    Value::Map(m) => m,
                    _ => return Some(Value::Undef),
                };
                if map.get(&Value::String("kind".to_string()))
                    != Some(&Value::String("binding".to_string()))
                {
                    return Some(Value::Undef);
                }
                if !map.contains_key(&Value::String("joint".to_string())) {
                    return Some(Value::Undef);
                }
                if !map.contains_key(&Value::String("value".to_string())) {
                    return Some(Value::Undef);
                }
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

            // Read the mechanism's `joint_parents` map for parent-chain
            // walking.
            let joint_parents = match mech_map.get(&Value::String("joint_parents".to_string())) {
                Some(Value::Map(jp)) => jp,
                _ => return Some(Value::Undef),
            };

            let bindings_list = match &args[1] {
                Value::List(b) => b.as_slice(),
                _ => return Some(Value::Undef), // unreachable: validated above.
            };

            // Per-snapshot memoization cache for joint world transforms.
            // Keys are joint Map values themselves — equal joints share
            // an entry by `Value::Eq`. The cache is local to this
            // `snapshot()` call so it doesn't leak state across calls
            // and is invalidated naturally when bindings change.
            let mut cache: BTreeMap<Value, Value> = BTreeMap::new();

            let mut snapshot_bodies = Vec::with_capacity(bodies.len());
            for body_value in bodies {
                let body_map = match body_value {
                    Value::Map(b) => b,
                    _ => return Some(Value::Undef),
                };
                let id = match body_map.get(&Value::String("id".to_string())) {
                    Some(v) => v.clone(),
                    None => return Some(Value::Undef),
                };
                let solid = match body_map.get(&Value::String("solid".to_string())) {
                    Some(v) => v.clone(),
                    None => return Some(Value::Undef),
                };
                let at = match body_map.get(&Value::String("at".to_string())) {
                    Some(v) => v.clone(),
                    None => return Some(Value::Undef),
                };
                let pose = match body_map.get(&Value::String("pose".to_string())) {
                    Some(v) => v.clone(),
                    None => return Some(Value::Undef),
                };

                // Walk the parent chain ancestor-ward to compute the
                // body's `at`-joint frame in world coordinates.
                let t_at_world =
                    match joint_world_transform(&at, joint_parents, bindings_list, &mut cache) {
                        Some(t) => t,
                        None => return Some(Value::Undef),
                    };

                // body's world_transform = T_at_world ∘ pose.
                let world_transform = eval_builtin("transform_compose", &[t_at_world, pose.clone()]);
                if world_transform.is_undef() {
                    return Some(Value::Undef);
                }

                snapshot_bodies.push(make_snapshot_body_record(id, solid, pose, world_transform));
            }

            make_snapshot(snapshot_bodies)
        }
        "bodies" => {
            // Validation surface (each guard short-circuits to Undef):
            //   args.len() == 1                       → arity guard
            //   args[0] is Map with kind="snapshot"   → snapshot guard
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let snap_bodies = match snapshot_bodies(&args[0]) {
                Some(b) => b,
                None => return Some(Value::Undef),
            };
            // Project each body record's `id` field into a flat List.
            // A missing `id` (defense-in-depth — well-formed snapshots
            // always carry one) collapses the whole call to Undef.
            let mut ids = Vec::with_capacity(snap_bodies.len());
            for body in snap_bodies {
                let body_map = match body {
                    Value::Map(b) => b,
                    _ => return Some(Value::Undef),
                };
                match body_map.get(&Value::String("id".to_string())) {
                    Some(v) => ids.push(v.clone()),
                    None => return Some(Value::Undef),
                }
            }
            Value::List(ids)
        }
        "transform_of" => {
            // Validation surface (each guard short-circuits to Undef):
            //   args.len() == 2                       → arity guard
            //   args[0] is Map with kind="snapshot"   → snapshot guard
            //   args[1] is Value::Int                 → id-type guard
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let snap_bodies = match snapshot_bodies(&args[0]) {
                Some(b) => b,
                None => return Some(Value::Undef),
            };
            if !matches!(&args[1], Value::Int(_)) {
                return Some(Value::Undef);
            }
            // Linear scan: first body whose `id` field equals args[1]
            // by structural Value::Eq wins.  Returns Undef when no
            // body matches (caller's responsibility to gate on the
            // bodies() list when an unknown id is a programming
            // error rather than a queryable miss).
            for body in snap_bodies {
                let body_map = match body {
                    Value::Map(b) => b,
                    _ => return Some(Value::Undef),
                };
                if body_map.get(&Value::String("id".to_string())) == Some(&args[1]) {
                    return Some(
                        body_map
                            .get(&Value::String("world_transform".to_string()))
                            .cloned()
                            .unwrap_or(Value::Undef),
                    );
                }
            }
            Value::Undef
        }
        "center_of_mass" => {
            // Validation surface (each guard short-circuits to Undef):
            //   args.len() in {1, 2}                     → arity guard
            //   args[0] is Map with kind="snapshot"      → snapshot guard
            // v0.1 semantic: density-weighted mean of per-body world-frame
            // ORIGINS (translation of each body's `world_transform`).
            // Point-mass approximation — real volumetric centroid needs
            // OCCT (`BRepGProp::VolumeProperties`), scope of FFI task #2530.
            // Empty Snapshot → Undef (zero-mass divide-by-zero).
            //
            // Density resolution (uniform fallback for partial maps):
            //   - args.len() == 1 OR args[1] is Undef OR args[1] is empty Map
            //     → all bodies get density 1.0 (uniform).
            //   - args[1] is non-empty Map → per-body lookup with 1.0
            //     fallback for absent ids (wired in step 26).
            if args.len() != 1 && args.len() != 2 {
                return Some(Value::Undef);
            }
            let snap_bodies = match snapshot_bodies(&args[0]) {
                Some(b) => b,
                None => return Some(Value::Undef),
            };
            if snap_bodies.is_empty() {
                return Some(Value::Undef);
            }

            // Density resolution.  When args[1] is absent OR Undef OR an
            // empty Map → uniform 1.0 per body.  When args[1] is a non-
            // empty Map → per-body lookup with 1.0 fallback for absent
            // ids.  Any other shape (Real, List, String, …) is rejected
            // up-front; non-numeric density values inside a populated Map
            // collapse the whole call later (per-body loop below).
            let densities_map: Option<&BTreeMap<Value, Value>> = if args.len() == 1 {
                None
            } else {
                match &args[1] {
                    Value::Undef => None,
                    Value::Map(m) if m.is_empty() => None,
                    Value::Map(m) => Some(m),
                    _ => return Some(Value::Undef),
                }
            };

            let mut weighted_xyz = [0.0_f64; 3];
            let mut total_density = 0.0_f64;
            for body in snap_bodies {
                let body_map = match body {
                    Value::Map(b) => b,
                    _ => return Some(Value::Undef),
                };
                let wt = match body_map.get(&Value::String("world_transform".to_string())) {
                    Some(v) => v,
                    None => return Some(Value::Undef),
                };
                let xyz = match world_transform_translation(wt) {
                    Some(t) => t,
                    None => return Some(Value::Undef),
                };

                // Resolve this body's density.  Uniform fallback when
                // densities_map is None (no arg / Undef / empty Map) OR
                // when this body's id isn't a key in the populated Map
                // (partial map → 1.0 for absent ids per spec §13.3).
                // Non-numeric density values reject the whole call.
                let id = match body_map.get(&Value::String("id".to_string())) {
                    Some(v) => v,
                    None => return Some(Value::Undef),
                };
                let density = match densities_map.and_then(|m| m.get(id)) {
                    None => 1.0_f64,
                    Some(Value::Real(r)) => *r,
                    Some(Value::Int(i)) => *i as f64,
                    Some(Value::Scalar { si_value, .. }) => *si_value,
                    Some(_) => return Some(Value::Undef),
                };
                if !density.is_finite() {
                    return Some(Value::Undef);
                }
                for i in 0..3 {
                    weighted_xyz[i] += density * xyz[i];
                }
                total_density += density;
            }
            // Total density of zero (e.g., user supplies all-zero
            // densities) is a divide-by-zero — return Undef.  The
            // uniform default never hits this branch (every body
            // contributes 1.0).
            if total_density == 0.0 {
                return Some(Value::Undef);
            }
            // total_density > 0 is guaranteed because snap_bodies is non-empty
            // and each body contributes 1.0 (or, post-step-26, a positive
            // density — zero/negative densities can be caught there).
            let com = [
                weighted_xyz[0] / total_density,
                weighted_xyz[1] / total_density,
                weighted_xyz[2] / total_density,
            ];
            make_length_point3(com)
        }
        "bounding_box" => {
            // Validation surface (each guard short-circuits to Undef):
            //   args.len() == 1                       → arity guard
            //   args[0] is Map with kind="snapshot"   → snapshot guard
            // v0.1 semantic: AABB of per-body world-frame ORIGINS
            // (translation of each body's `world_transform`). This is
            // a point-mass approximation — the real volumetric AABB
            // requires OCCT (`BRepBndLib::Add`), scope of FFI task
            // #2530. Empty Snapshot → Undef (no points to envelope).
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let snap_bodies = match snapshot_bodies(&args[0]) {
                Some(b) => b,
                None => return Some(Value::Undef),
            };
            if snap_bodies.is_empty() {
                return Some(Value::Undef);
            }

            let mut min_xyz = [f64::INFINITY; 3];
            let mut max_xyz = [f64::NEG_INFINITY; 3];
            for body in snap_bodies {
                let body_map = match body {
                    Value::Map(b) => b,
                    _ => return Some(Value::Undef),
                };
                let wt = match body_map.get(&Value::String("world_transform".to_string())) {
                    Some(v) => v,
                    None => return Some(Value::Undef),
                };
                let xyz = match world_transform_translation(wt) {
                    Some(t) => t,
                    None => return Some(Value::Undef),
                };
                for i in 0..3 {
                    if xyz[i] < min_xyz[i] {
                        min_xyz[i] = xyz[i];
                    }
                    if xyz[i] > max_xyz[i] {
                        max_xyz[i] = xyz[i];
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(
                Value::String("max".to_string()),
                make_length_point3(max_xyz),
            );
            m.insert(
                Value::String("min".to_string()),
                make_length_point3(min_xyz),
            );
            Value::Map(m)
        }
        _ => return None,
    })
}

/// Extract the three SI-unit translation components from a `Value::Transform`.
///
/// Returns `None` (which callers map to `Value::Undef`) when:
/// - `t` is not a `Value::Transform`,
/// - `translation` is not a `Value::Vector` of exactly three components, or
/// - any component is non-numeric (e.g., a String) or non-finite.
///
/// The components' carried dimensions are NOT validated here — the FK
/// walk produces world transforms via `transform_compose`, which
/// preserves the LENGTH dimension on the translation vector.  This
/// helper is local to snapshot.rs because `geometry.rs::decompose_transform`
/// is private to that module; duplicating the destructure-and-validate
/// pattern keeps the FK accessors decoupled from geometry's internals.
fn world_transform_translation(t: &Value) -> Option<[f64; 3]> {
    let translation = match t {
        Value::Transform { translation, .. } => translation.as_ref(),
        _ => return None,
    };
    let comps = match translation {
        Value::Vector(c) if c.len() == 3 => c,
        _ => return None,
    };
    let read = |v: &Value| -> Option<f64> {
        let f = match v {
            Value::Real(r) => *r,
            Value::Scalar { si_value, .. } => *si_value,
            _ => return None,
        };
        if f.is_finite() { Some(f) } else { None }
    };
    Some([read(&comps[0])?, read(&comps[1])?, read(&comps[2])?])
}

/// Build a `Value::Point` of three LENGTH-dimensioned scalars from
/// raw f64 SI components (metres).  Mirrors the Point3<Length>
/// shape produced by `Value::length` / `point3(...)`.
fn make_length_point3(xyz: [f64; 3]) -> Value {
    Value::Point(vec![
        Value::length(xyz[0]),
        Value::length(xyz[1]),
        Value::length(xyz[2]),
    ])
}

/// Extract the `bodies` list from a Snapshot Map, validating the
/// `kind="snapshot"` discriminant.  Returns `None` for any non-Map,
/// non-Snapshot, or malformed Snapshot (missing/non-List `bodies`
/// field).  Used by the accessor arms of `eval_snapshot` to share
/// the same validation predicate.
fn snapshot_bodies(snap: &Value) -> Option<&[Value]> {
    let map = match snap {
        Value::Map(m) => m,
        _ => return None,
    };
    if map.get(&Value::String("kind".to_string()))
        != Some(&Value::String("snapshot".to_string()))
    {
        return None;
    }
    match map.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => Some(b.as_slice()),
        _ => None,
    }
}

/// Build the canonical empty Snapshot `Value::Map`.
///
/// Shape (alphabetical key order, matching `BTreeMap` iteration):
/// - `bodies` → `Value::List(vec![])`
/// - `kind` → `Value::String("snapshot")`
fn make_empty_snapshot() -> Value {
    make_snapshot(Vec::new())
}

/// Build a Snapshot `Value::Map` carrying the supplied list of
/// per-body world-transform records.
fn make_snapshot(bodies: Vec<Value>) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("bodies".to_string()), Value::List(bodies));
    m.insert(
        Value::String("kind".to_string()),
        Value::String("snapshot".to_string()),
    );
    Value::Map(m)
}

/// Build a snapshot body record `Value::Map` with the four-key layout
/// `id`, `pose`, `solid`, `world_transform` (alphabetical, matching
/// `BTreeMap` iteration). Mirrors `make_body_record` in mechanism.rs
/// but adds the FK-derived `world_transform` and drops `at`/`parent`
/// (those belong to the source mechanism, not the snapshot).
fn make_snapshot_body_record(id: Value, solid: Value, pose: Value, world_transform: Value) -> Value {
    let mut b = BTreeMap::new();
    b.insert(Value::String("id".to_string()), id);
    b.insert(Value::String("pose".to_string()), pose);
    b.insert(Value::String("solid".to_string()), solid);
    b.insert(
        Value::String("world_transform".to_string()),
        world_transform,
    );
    Value::Map(b)
}

/// Compute the world-frame transform of a joint by walking the
/// `joint_parents` chain ancestor-ward to the world sentinel and
/// folding `transform_at(joint_k, value_for(joint_k))` into a running
/// composition.
///
/// Returns `None` on any of:
///   - a joint along the chain has no resolvable motion value (no
///     binding entry and the midpoint fallback is not yet wired in
///     this step — added in step 12),
///   - `transform_at` or `transform_compose` returns Undef,
///   - the chain length exceeds `joint_parents.len() + 1` (cycle-
///     safe, defence-in-depth — `mechanism::body()` already rejects
///     cycle-creating edges).
///
/// Memoizes per-joint results in `cache` so a chain shared by many
/// bodies is O(D + N) instead of O(D·N). Keys are joint Map values
/// themselves — equal joints share a cache entry by `Value::Eq`.
fn joint_world_transform(
    joint: &Value,
    joint_parents: &BTreeMap<Value, Value>,
    bindings: &[Value],
    cache: &mut BTreeMap<Value, Value>,
) -> Option<Value> {
    // Cache hit: return the memoized world transform directly.
    if let Some(cached) = cache.get(joint) {
        return Some(cached.clone());
    }

    // Resolve this joint's parent in the recorded `joint_parents` map.
    // A missing entry means the joint was never registered as an `at`
    // value — defence-in-depth, not reachable for joints in
    // mechanism's `bodies` list (every body's `at` joint has its
    // parent recorded by `append_body`).
    let parent = joint_parents.get(joint)?;

    // Compute the parent's world transform first (recursive walk).
    // The world sentinel is the chain's terminator: its world frame
    // is the SE(3) identity.
    let parent_world = if is_world(parent) {
        eval_builtin("transform3_identity", &[])
    } else {
        joint_world_transform(parent, joint_parents, bindings, cache)?
    };
    if parent_world.is_undef() {
        return None;
    }

    // Compose: T_joint_world = T_parent_world ∘ T_joint_local
    // where T_joint_local = transform_at(joint, value_for(joint)).
    let motion_value = value_for(joint, bindings)?;
    let t_local = eval_builtin("transform_at", &[joint.clone(), motion_value]);
    if t_local.is_undef() {
        return None;
    }
    let t_world = eval_builtin("transform_compose", &[parent_world, t_local]);
    if t_world.is_undef() {
        return None;
    }

    cache.insert(joint.clone(), t_world.clone());
    Some(t_world)
}

/// Look up the motion value for `joint` in a bindings list.
///
/// Linear scan; first match by structural `Value::Eq` on the binding's
/// `joint` field wins.  When no entry matches, fall back to the
/// joint's range midpoint via [`joint_range_midpoint`] (per spec
/// §13.3): the resulting f64 (in SI units — metres for prismatic,
/// radians for revolute, parent-frame midpoint for coupling) is
/// wrapped back into a dimensioned `Value::length` / `Value::angle`
/// via [`wrap_midpoint_for_joint`] so it round-trips through
/// `transform_at`'s `length_input` / `trig_input` checks.
///
/// Returns `None` only when the joint is non-Map, lacks a range, or
/// has an unbounded range — `joint_range_midpoint` already filters
/// those.  The FK walk's caller maps `None` to `Value::Undef`.
///
/// Defensive against malformed binding entries (non-Map, missing
/// `joint`/`value` keys): such entries are skipped, not failed-on.
fn value_for(joint: &Value, bindings: &[Value]) -> Option<Value> {
    for entry in bindings {
        let map = match entry {
            Value::Map(m) => m,
            _ => continue,
        };
        if map.get(&Value::String("joint".to_string())) == Some(joint) {
            if let Some(v) = map.get(&Value::String("value".to_string())) {
                return Some(v.clone());
            }
        }
    }
    // No binding matched — fall back to the joint's range midpoint
    // (spec §13.3).  For coupling joints, `joint_range_midpoint`
    // already recurses to the parent's range; we still wrap with the
    // parent's dimension here so `transform_at(coupling, v)` receives
    // a properly-typed value for its `length_input`/`trig_input`.
    let mid_si = joint_range_midpoint(joint)?;
    wrap_midpoint_for_joint(joint, mid_si)
}

/// Wrap a midpoint f64 (in SI units) back into a dimensioned `Value`
/// based on the joint's underlying kind.
///
/// - `prismatic` → `Value::length(mid_si)` (metres)
/// - `revolute`  → `Value::angle(mid_si)`  (radians)
/// - `coupling`  → recurse on the coupling's `parent` (the coupling's
///   own free-variable dimension is the parent's; `transform_at`
///   applies `ratio`/`offset` downstream).
///
/// Returns `None` for non-Map joints, missing `kind`, unknown kinds,
/// or a coupling whose `parent` field is missing/non-Map — symmetric
/// with `joint_range_midpoint`'s defensive failure modes.
fn wrap_midpoint_for_joint(joint: &Value, mid_si: f64) -> Option<Value> {
    let map = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    match kind {
        "prismatic" => Some(Value::length(mid_si)),
        "revolute" => Some(Value::angle(mid_si)),
        "coupling" => {
            let parent = map.get(&Value::String("parent".to_string()))?;
            wrap_midpoint_for_joint(parent, mid_si)
        }
        _ => None,
    }
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

    // ── Analytic two-link chain (multi-level parent walk) ─────────────────

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

    fn length_range_0_to_2m() -> Value {
        Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(2.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    /// Headline acceptance test (PRD task 4): analytic two-link chain.
    ///
    /// `j_rev = revolute(+Z, 0..π)` at world; `j_pris = prismatic(+X, 0..2m)`
    /// parented to `j_rev`. Body A at `j_rev` (parent=world), body B at
    /// `j_pris` (parent=`j_rev`). Bindings: bind(j_rev, π/4), bind(j_pris, 2 m).
    ///
    /// Analytic answer for body B:
    ///   T_rev_world = R_z(π/4)
    ///   T_pris_world = R_z(π/4) ∘ T_x(2m)
    ///   body B's world_transform = T_pris_world ∘ identity_pose = T_pris_world
    ///   Translation: R_z(π/4) ⋅ (2, 0, 0) = (2·cos(π/4), 2·sin(π/4), 0) = (√2, √2, 0)
    ///   Rotation:    quaternion(angle=π/4, axis=+Z) = (cos(π/8), 0, 0, sin(π/8))
    ///                up to sign.
    #[test]
    fn snapshot_analytic_two_link_chain_world_transform() {
        let j_rev = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let j_pris = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_2m()]);
        let solid_a = Value::String("a".to_string());
        let solid_b = Value::String("b".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        // Body A: at j_rev, parent defaulted to world.
        let m1 = eval_builtin("body", &[m0, solid_a, j_rev.clone()]);
        // Body B: at j_pris, parent j_rev.
        let m2 = eval_builtin(
            "body",
            &[m1, solid_b, j_pris.clone(), j_rev.clone()],
        );

        let bind_rev = eval_builtin(
            "bind",
            &[j_rev, Value::angle(std::f64::consts::FRAC_PI_4)],
        );
        let bind_pris = eval_builtin("bind", &[j_pris, Value::length(2.0)]);

        let s = eval_builtin("snapshot", &[m2, Value::List(vec![bind_rev, bind_pris])]);
        let smap = match s {
            Value::Map(m) => m,
            other => panic!("expected Snapshot Map, got {:?}", other),
        };
        let bodies = match smap.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected snapshot bodies List, got {:?}", other),
        };
        assert_eq!(bodies.len(), 2, "snapshot should have 2 bodies (A and B)");

        // Body B is the second record (id=1).
        let body_b = match &bodies[1] {
            Value::Map(b) => b,
            other => panic!("expected snapshot body record Map, got {:?}", other),
        };
        let wt = body_b
            .get(&Value::String("world_transform".to_string()))
            .expect("body B must carry a world_transform field");
        let ((rw, rx, ry, rz), [tx, ty, tz]) = decompose_transform_for_assert(wt);

        // Expected translation: (√2, √2, 0).
        let sqrt2 = std::f64::consts::SQRT_2;
        assert!(
            (tx - sqrt2).abs() < 1e-9,
            "body B tx should be √2 ≈ {}, got {}",
            sqrt2,
            tx
        );
        assert!(
            (ty - sqrt2).abs() < 1e-9,
            "body B ty should be √2 ≈ {}, got {}",
            sqrt2,
            ty
        );
        assert!(tz.abs() < 1e-9, "body B tz should be 0, got {}", tz);

        // Expected rotation: quaternion for R_z(π/4) = (cos(π/8), 0, 0, sin(π/8))
        // up to sign — check both the quaternion and its negation.
        let half = std::f64::consts::FRAC_PI_8;
        let qw = half.cos();
        let qz = half.sin();
        let matches_pos = (rw - qw).abs() < 1e-9
            && rx.abs() < 1e-9
            && ry.abs() < 1e-9
            && (rz - qz).abs() < 1e-9;
        let matches_neg = (rw + qw).abs() < 1e-9
            && rx.abs() < 1e-9
            && ry.abs() < 1e-9
            && (rz + qz).abs() < 1e-9;
        assert!(
            matches_pos || matches_neg,
            "body B rotation should be quaternion(R_z(π/4)) = ({}, 0, 0, {}) up to sign, \
             got ({}, {}, {}, {})",
            qw,
            qz,
            rw,
            rx,
            ry,
            rz
        );
    }

    // ── Midpoint fallback for unbound joints ──────────────────────────────

    /// Joints absent from the bindings list default to their range
    /// midpoint per spec §13.3. With the same single-body mechanism as
    /// the world-parent test (prismatic +X, range 0..1m), passing `[]`
    /// as bindings should produce a body world translation of
    /// (0.5, 0, 0) — the midpoint of 0..1m.
    #[test]
    fn snapshot_unbound_joint_uses_range_midpoint() {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());
        let m1 = eval_builtin("body", &[m0, solid, j]);

        let s = eval_builtin("snapshot", &[m1, Value::List(vec![])]);
        let smap = match s {
            Value::Map(m) => m,
            other => panic!("expected Snapshot Map, got {:?}", other),
        };
        let bodies = match smap.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected snapshot bodies List, got {:?}", other),
        };
        let body = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("expected snapshot body record Map, got {:?}", other),
        };
        let wt = body
            .get(&Value::String("world_transform".to_string()))
            .expect("body record must carry a world_transform field");
        let (_, [tx, ty, tz]) = decompose_transform_for_assert(wt);

        assert!(
            (tx - 0.5).abs() < 1e-12,
            "tx should be 0.5 m (midpoint of 0..1m), got {}",
            tx
        );
        assert!(ty.abs() < 1e-12, "ty should be 0, got {}", ty);
        assert!(tz.abs() < 1e-12, "tz should be 0, got {}", tz);
    }

    // ── snapshot() input validation: full surface returns Undef ───────────
    //
    // Validation allow-list (matches `eval_snapshot::snapshot` arm):
    //   args.len() == 2                                 → arity guard
    //   args[0] is Map with kind="mechanism"            → mechanism guard
    //   args[1] is Value::List                          → bindings guard
    //   each entry of args[1] is Map with kind="binding"
    //   AND a present `joint`/`value` field             → per-entry guard
    // Any guard failure returns `Value::Undef` BEFORE any FK work.

    /// `snapshot()` with an arity outside {2} returns Undef.
    #[test]
    fn snapshot_wrong_arity_returns_undef() {
        let m0 = eval_builtin("mechanism", &[]);
        // 0, 1, 3 args
        assert!(eval_builtin("snapshot", &[]).is_undef());
        assert!(eval_builtin("snapshot", std::slice::from_ref(&m0)).is_undef());
        assert!(eval_builtin(
            "snapshot",
            &[m0.clone(), Value::List(vec![]), Value::List(vec![])]
        )
        .is_undef());
    }

    /// `snapshot(non_mechanism, [])` returns Undef when args[0] is not
    /// a Mechanism Map.  Covers `Value::Real` (non-Map), the world
    /// sentinel (Map with kind="world"), and an error-bearing non-
    /// mechanism Map (Map with kind="error" — unrelated to the
    /// mechanism's internal `error` field, which lives under
    /// kind="mechanism").
    #[test]
    fn snapshot_non_mechanism_first_arg_returns_undef() {
        // Real (not a Map at all)
        assert!(eval_builtin("snapshot", &[Value::Real(1.0), Value::List(vec![])]).is_undef());

        // World sentinel (Map with kind="world", not "mechanism")
        let world = eval_builtin("world", &[]);
        assert!(eval_builtin("snapshot", &[world, Value::List(vec![])]).is_undef());

        // Map with kind="error" (a non-mechanism kind)
        let mut error_map = std::collections::BTreeMap::new();
        error_map.insert(
            Value::String("kind".to_string()),
            Value::String("error".to_string()),
        );
        assert!(eval_builtin(
            "snapshot",
            &[Value::Map(error_map), Value::List(vec![])]
        )
        .is_undef());
    }

    /// `snapshot(m, non_list)` returns Undef when args[1] is not a
    /// `Value::List`.  Covers `Value::Real`, `Value::Map(empty)`, and
    /// `Value::Undef` — all three are non-List shapes that must be
    /// rejected before any FK walk.
    #[test]
    fn snapshot_non_list_bindings_returns_undef() {
        let m0 = eval_builtin("mechanism", &[]);
        assert!(eval_builtin("snapshot", &[m0.clone(), Value::Real(1.0)]).is_undef());
        assert!(eval_builtin(
            "snapshot",
            &[m0.clone(), Value::Map(std::collections::BTreeMap::new())]
        )
        .is_undef());
        assert!(eval_builtin("snapshot", &[m0, Value::Undef]).is_undef());
    }

    /// A bindings list containing any non-binding entry causes
    /// `snapshot()` to return `Value::Undef` — even when other
    /// entries are valid binding Maps.  This is whole-call rejection,
    /// not silent skipping: a malformed entry signals a bug at the
    /// call site that should not be papered over by midpoint
    /// fallback.
    #[test]
    fn snapshot_invalid_binding_entry_returns_undef() {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solidA".to_string());
        let m1 = eval_builtin("body", &[m0, solid, j.clone()]);
        let valid_binding = eval_builtin("bind", &[j.clone(), Value::length(0.002)]);

        // Non-Map entry (Real)
        assert!(eval_builtin(
            "snapshot",
            &[
                m1.clone(),
                Value::List(vec![valid_binding.clone(), Value::Real(0.5)])
            ]
        )
        .is_undef());

        // Map with kind != "binding"
        let mut wrong_kind = std::collections::BTreeMap::new();
        wrong_kind.insert(
            Value::String("kind".to_string()),
            Value::String("not_a_binding".to_string()),
        );
        wrong_kind.insert(Value::String("joint".to_string()), j.clone());
        wrong_kind.insert(Value::String("value".to_string()), Value::length(0.001));
        assert!(eval_builtin(
            "snapshot",
            &[m1.clone(), Value::List(vec![Value::Map(wrong_kind)])]
        )
        .is_undef());

        // Map with kind="binding" but missing `joint` field
        let mut missing_joint = std::collections::BTreeMap::new();
        missing_joint.insert(
            Value::String("kind".to_string()),
            Value::String("binding".to_string()),
        );
        missing_joint.insert(Value::String("value".to_string()), Value::length(0.001));
        assert!(eval_builtin(
            "snapshot",
            &[m1.clone(), Value::List(vec![Value::Map(missing_joint)])]
        )
        .is_undef());

        // Map with kind="binding" but missing `value` field
        let mut missing_value = std::collections::BTreeMap::new();
        missing_value.insert(
            Value::String("kind".to_string()),
            Value::String("binding".to_string()),
        );
        missing_value.insert(Value::String("joint".to_string()), j);
        assert!(eval_builtin(
            "snapshot",
            &[m1, Value::List(vec![Value::Map(missing_value)])]
        )
        .is_undef());
    }

    // ── Errored-mechanism short-circuit ────────────────────────────────────

    fn axis_y_unit() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
    }

    /// `snapshot()` on an errored Mechanism returns `Value::Undef` —
    /// not a partial Snapshot of the pre-error bodies list.  Mirrors
    /// `body_id_of_on_errored_mechanism_returns_undef` in mechanism.rs:
    /// a user who chains `snapshot()` onto an errored mechanism must
    /// reckon with the error before getting a plausible-looking
    /// Snapshot back.
    #[test]
    fn snapshot_on_errored_mechanism_returns_undef() {
        // Build an errored mechanism via parent-conflict — same recipe as
        // mechanism.rs::body_id_of_on_errored_mechanism_returns_undef.
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

        // snapshot() on the errored mechanism must yield Undef even
        // though the pre-error bodies list contains a fully-formed
        // body record.
        assert!(
            eval_builtin("snapshot", &[errored, Value::List(vec![])]).is_undef(),
            "snapshot() on errored mechanism must yield Undef"
        );
    }

    // ── bodies(snapshot) accessor ─────────────────────────────────────────

    /// Helper: build a 2-body Snapshot whose bodies sit at distinct
    /// world positions.
    ///
    /// Layout:
    /// - body 0: solid="a", at j_neg (prismatic +X with offset bound to -0.5m
    ///   via binding) — world translation (-0.5, 0, 0)
    /// - body 1: solid="b", at j_pos (prismatic +X bound to +0.5m) —
    ///   world translation (+0.5, 0, 0)
    ///
    /// Each body has parent=world and identity pose, so the body's
    /// `world_transform` equals its `at` joint's transform.  Returns
    /// the Snapshot and the two joints (so accessor tests can build
    /// expected-transform fixtures via `transform_at`).
    fn make_two_body_snapshot() -> (Value, Value, Value) {
        let j_neg = eval_builtin(
            "prismatic",
            &[
                axis_x_unit(),
                Value::Range {
                    lower: Some(Box::new(Value::length(-1.0))),
                    upper: Some(Box::new(Value::length(0.0))),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
            ],
        );
        let j_pos = eval_builtin(
            "prismatic",
            &[
                axis_x_unit(),
                Value::Range {
                    lower: Some(Box::new(Value::length(0.0))),
                    upper: Some(Box::new(Value::length(1.0))),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
            ],
        );

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin(
            "body",
            &[m0, Value::String("a".to_string()), j_neg.clone()],
        );
        let m2 = eval_builtin(
            "body",
            &[m1, Value::String("b".to_string()), j_pos.clone()],
        );

        let bind_neg = eval_builtin("bind", &[j_neg.clone(), Value::length(-0.5)]);
        let bind_pos = eval_builtin("bind", &[j_pos.clone(), Value::length(0.5)]);

        let s = eval_builtin("snapshot", &[m2, Value::List(vec![bind_neg, bind_pos])]);
        (s, j_neg, j_pos)
    }

    /// `bodies(s)` returns a `Value::List` of body ids in insertion
    /// order (matching the source mechanism's bodies list).
    #[test]
    fn bodies_returns_id_list_in_insertion_order() {
        let (s, _, _) = make_two_body_snapshot();
        let result = eval_builtin("bodies", &[s]);
        assert_eq!(
            result,
            Value::List(vec![Value::Int(0), Value::Int(1)]),
            "bodies(s) should return [Int(0), Int(1)]"
        );
    }

    /// `bodies(empty_snapshot)` returns the empty `Value::List`.
    #[test]
    fn bodies_on_empty_snapshot_returns_empty_list() {
        let m0 = eval_builtin("mechanism", &[]);
        let s = eval_builtin("snapshot", &[m0, Value::List(vec![])]);
        let result = eval_builtin("bodies", &[s]);
        assert_eq!(
            result,
            Value::List(vec![]),
            "bodies on an empty snapshot should be the empty List"
        );
    }

    /// `bodies()` validation surface: arity != 1 and non-snapshot
    /// args[0] both return `Value::Undef`.
    #[test]
    fn bodies_validation_returns_undef() {
        let (s, _, _) = make_two_body_snapshot();
        // Wrong arity (0, 2 args)
        assert!(eval_builtin("bodies", &[]).is_undef());
        assert!(eval_builtin("bodies", &[s.clone(), Value::Int(0)]).is_undef());
        // Non-snapshot first arg: Real, world sentinel, mechanism
        assert!(eval_builtin("bodies", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("bodies", &[eval_builtin("world", &[])]).is_undef());
        let m0 = eval_builtin("mechanism", &[]);
        assert!(eval_builtin("bodies", &[m0]).is_undef());
    }

    // ── transform_of(snapshot, id) accessor ───────────────────────────────

    /// `transform_of(s, id)` returns the body's recorded
    /// `world_transform` for each id present in the snapshot's
    /// bodies list.  Verified by decomposing the result and
    /// comparing the analytic per-body world translation
    /// component-wise: byte-equal comparison against a fresh
    /// `transform_at(joint, v)` is brittle to signed-zero
    /// normalization in `transform_compose(t_at_world, identity_pose)`
    /// (`Value::Scalar`'s `PartialEq` uses `to_bits()`, which
    /// distinguishes +0.0 from -0.0).
    #[test]
    fn transform_of_returns_body_world_transform() {
        let (s, _j_neg, _j_pos) = make_two_body_snapshot();

        // Body 0: at j_neg with value=-0.5m, identity pose, parent=world
        // → world translation (-0.5, 0, 0) and identity rotation.
        let result_0 = eval_builtin("transform_of", &[s.clone(), Value::Int(0)]);
        let ((rw0, rx0, ry0, rz0), [tx0, ty0, tz0]) = decompose_transform_for_assert(&result_0);
        assert!((rw0 - 1.0).abs() < 1e-12, "body 0 rotation w should be 1, got {}", rw0);
        assert!(rx0.abs() < 1e-12, "body 0 rotation x should be 0, got {}", rx0);
        assert!(ry0.abs() < 1e-12, "body 0 rotation y should be 0, got {}", ry0);
        assert!(rz0.abs() < 1e-12, "body 0 rotation z should be 0, got {}", rz0);
        assert!((tx0 - (-0.5)).abs() < 1e-12, "body 0 tx should be -0.5, got {}", tx0);
        assert!(ty0.abs() < 1e-12, "body 0 ty should be 0, got {}", ty0);
        assert!(tz0.abs() < 1e-12, "body 0 tz should be 0, got {}", tz0);

        // Body 1: at j_pos with value=+0.5m, identity pose, parent=world
        // → world translation (+0.5, 0, 0) and identity rotation.
        let result_1 = eval_builtin("transform_of", &[s, Value::Int(1)]);
        let ((rw1, rx1, ry1, rz1), [tx1, ty1, tz1]) = decompose_transform_for_assert(&result_1);
        assert!((rw1 - 1.0).abs() < 1e-12, "body 1 rotation w should be 1, got {}", rw1);
        assert!(rx1.abs() < 1e-12, "body 1 rotation x should be 0, got {}", rx1);
        assert!(ry1.abs() < 1e-12, "body 1 rotation y should be 0, got {}", ry1);
        assert!(rz1.abs() < 1e-12, "body 1 rotation z should be 0, got {}", rz1);
        assert!((tx1 - 0.5).abs() < 1e-12, "body 1 tx should be 0.5, got {}", tx1);
        assert!(ty1.abs() < 1e-12, "body 1 ty should be 0, got {}", ty1);
        assert!(tz1.abs() < 1e-12, "body 1 tz should be 0, got {}", tz1);
    }

    /// `transform_of(s, unknown_id)` returns `Value::Undef`.
    #[test]
    fn transform_of_unknown_id_returns_undef() {
        let (s, _, _) = make_two_body_snapshot();
        assert!(eval_builtin("transform_of", &[s, Value::Int(99)]).is_undef());
    }

    /// `transform_of()` validation surface: arity, non-snapshot
    /// first arg, and non-Int second arg all return `Value::Undef`.
    #[test]
    fn transform_of_validation_returns_undef() {
        let (s, _, _) = make_two_body_snapshot();
        // Wrong arity (0, 1, 3 args)
        assert!(eval_builtin("transform_of", &[]).is_undef());
        assert!(eval_builtin("transform_of", std::slice::from_ref(&s)).is_undef());
        assert!(eval_builtin(
            "transform_of",
            &[s.clone(), Value::Int(0), Value::Int(1)]
        )
        .is_undef());
        // Non-snapshot first arg
        assert!(eval_builtin("transform_of", &[Value::Real(1.0), Value::Int(0)]).is_undef());
        let m0 = eval_builtin("mechanism", &[]);
        assert!(eval_builtin("transform_of", &[m0, Value::Int(0)]).is_undef());
        // Non-Int second arg: String, Real, world sentinel
        assert!(eval_builtin(
            "transform_of",
            &[s.clone(), Value::String("0".to_string())]
        )
        .is_undef());
        assert!(eval_builtin("transform_of", &[s.clone(), Value::Real(0.0)]).is_undef());
        assert!(eval_builtin("transform_of", &[s, eval_builtin("world", &[])]).is_undef());
    }

    // ── bounding_box(snapshot) accessor ───────────────────────────────────
    //
    // v0.1 semantic: `bounding_box(s)` returns the AABB of the per-body
    // world-frame ORIGINS (translation of each body's `world_transform`).
    // This is a point-mass approximation — the real volumetric AABB
    // requires OCCT (`BRepBndLib::Add`), which is the scope of FFI task
    // #2530.  Empty Snapshot → Undef (no points to envelope).
    //
    // Result shape: `Value::Map { min: Point3<Length>, max: Point3<Length> }`
    // where each Point3 is `Value::Point(vec![length(x), length(y), length(z)])`.

    /// Decompose a `Value::Point` of three Length-dimensioned scalars
    /// into `[f64; 3]` SI values for assertion purposes.
    fn decompose_point3_length_for_assert(v: &Value) -> [f64; 3] {
        let comps = match v {
            Value::Point(c) if c.len() == 3 => c,
            other => panic!("expected Value::Point len=3, got {:?}", other),
        };
        let read = |comp: &Value| -> f64 {
            match comp {
                Value::Real(r) => *r,
                Value::Scalar { si_value, .. } => *si_value,
                other => panic!("expected numeric component, got {:?}", other),
            }
        };
        [read(&comps[0]), read(&comps[1]), read(&comps[2])]
    }

    /// `bounding_box(s)` on a 2-body Snapshot whose bodies sit at
    /// world translations (-0.5, 0, 0) and (+0.5, 0, 0).  Result must
    /// be a Map with `min` = Point3(-0.5, 0, 0), `max` = Point3(+0.5, 0, 0),
    /// all components carrying LENGTH dimension.
    #[test]
    fn bounding_box_two_body_envelopes_origins() {
        let (s, _, _) = make_two_body_snapshot();
        let result = eval_builtin("bounding_box", &[s]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected bounding_box Map, got {:?}", other),
        };

        let min_v = map
            .get(&Value::String("min".to_string()))
            .expect("bounding_box result must carry a `min` field");
        let max_v = map
            .get(&Value::String("max".to_string()))
            .expect("bounding_box result must carry a `max` field");

        let [minx, miny, minz] = decompose_point3_length_for_assert(min_v);
        let [maxx, maxy, maxz] = decompose_point3_length_for_assert(max_v);

        assert!((minx - (-0.5)).abs() < 1e-12, "min.x should be -0.5, got {}", minx);
        assert!(miny.abs() < 1e-12, "min.y should be 0, got {}", miny);
        assert!(minz.abs() < 1e-12, "min.z should be 0, got {}", minz);
        assert!((maxx - 0.5).abs() < 1e-12, "max.x should be 0.5, got {}", maxx);
        assert!(maxy.abs() < 1e-12, "max.y should be 0, got {}", maxy);
        assert!(maxz.abs() < 1e-12, "max.z should be 0, got {}", maxz);

        // All components must carry LENGTH dimension (not bare Real).
        let assert_length = |p: &Value, label: &str| {
            let comps = match p {
                Value::Point(c) => c,
                other => panic!("{}: expected Value::Point, got {:?}", label, other),
            };
            for (i, comp) in comps.iter().enumerate() {
                match comp {
                    Value::Scalar { dimension, .. } => {
                        assert_eq!(
                            *dimension,
                            reify_types::DimensionVector::LENGTH,
                            "{}: component[{}] should carry LENGTH dimension",
                            label,
                            i
                        );
                    }
                    other => panic!("{}: component[{}] should be Value::Scalar, got {:?}", label, i, other),
                }
            }
        };
        assert_length(min_v, "min");
        assert_length(max_v, "max");
    }

    /// `bounding_box(empty_snapshot)` returns `Value::Undef` — no points
    /// to envelope, so the AABB is undefined.
    #[test]
    fn bounding_box_on_empty_snapshot_returns_undef() {
        let m0 = eval_builtin("mechanism", &[]);
        let s = eval_builtin("snapshot", &[m0, Value::List(vec![])]);
        assert!(eval_builtin("bounding_box", &[s]).is_undef());
    }

    /// `bounding_box()` validation surface: arity != 1 and non-snapshot
    /// args[0] both return `Value::Undef`.
    #[test]
    fn bounding_box_validation_returns_undef() {
        let (s, _, _) = make_two_body_snapshot();
        // Wrong arity (0, 2 args)
        assert!(eval_builtin("bounding_box", &[]).is_undef());
        assert!(eval_builtin("bounding_box", &[s.clone(), Value::Int(0)]).is_undef());
        // Non-snapshot first arg: Real, world sentinel, mechanism
        assert!(eval_builtin("bounding_box", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("bounding_box", &[eval_builtin("world", &[])]).is_undef());
        let m0 = eval_builtin("mechanism", &[]);
        assert!(eval_builtin("bounding_box", &[m0]).is_undef());
    }

    // ── center_of_mass(snapshot, [densities]) accessor (uniform default) ──
    //
    // v0.1 semantic: `center_of_mass(s)` returns the density-weighted mean
    // of the per-body world-frame ORIGINS (translation of each body's
    // `world_transform`).  This is a point-mass approximation — the real
    // volumetric centroid requires OCCT (`BRepGProp::VolumeProperties`),
    // scope of FFI task #2530.  Empty Snapshot → Undef (zero-mass system,
    // divide-by-zero).
    //
    // Default density (no `densities` arg, or arg is Undef, or arg is an
    // empty Map) is uniform 1.0 per body.  Per-body density Map is wired
    // in step 26.
    //
    // Result shape: `Value::Point` of three LENGTH-dimensioned scalars.

    /// `center_of_mass(s)` on a 2-body Snapshot whose bodies sit
    /// symmetrically at (-0.5, 0, 0) and (+0.5, 0, 0): uniform-density
    /// COM = (0, 0, 0).  All result components carry LENGTH dimension.
    #[test]
    fn center_of_mass_uniform_two_body_returns_origin() {
        let (s, _, _) = make_two_body_snapshot();
        let result = eval_builtin("center_of_mass", &[s]);
        let [cx, cy, cz] = decompose_point3_length_for_assert(&result);
        assert!(cx.abs() < 1e-12, "COM.x should be 0, got {}", cx);
        assert!(cy.abs() < 1e-12, "COM.y should be 0, got {}", cy);
        assert!(cz.abs() < 1e-12, "COM.z should be 0, got {}", cz);

        // All components must carry LENGTH dimension (not bare Real).
        let comps = match &result {
            Value::Point(c) => c,
            other => panic!("expected Value::Point, got {:?}", other),
        };
        for (i, comp) in comps.iter().enumerate() {
            match comp {
                Value::Scalar { dimension, .. } => {
                    assert_eq!(
                        *dimension,
                        reify_types::DimensionVector::LENGTH,
                        "COM component[{}] should carry LENGTH dimension",
                        i
                    );
                }
                other => panic!("COM component[{}] should be Value::Scalar, got {:?}", i, other),
            }
        }
    }

    /// `center_of_mass(s)` on a single-body Snapshot returns that body's
    /// world-frame origin.  Bind a prismatic joint to 0.7 m so the body
    /// sits at (0.7, 0, 0).
    #[test]
    fn center_of_mass_uniform_single_body_returns_body_origin() {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solid".to_string());
        let m1 = eval_builtin("body", &[m0, solid, j.clone()]);
        let binding = eval_builtin("bind", &[j, Value::length(0.7)]);
        let s = eval_builtin("snapshot", &[m1, Value::List(vec![binding])]);

        let result = eval_builtin("center_of_mass", &[s]);
        let [cx, cy, cz] = decompose_point3_length_for_assert(&result);
        assert!((cx - 0.7).abs() < 1e-12, "COM.x should be 0.7, got {}", cx);
        assert!(cy.abs() < 1e-12, "COM.y should be 0, got {}", cy);
        assert!(cz.abs() < 1e-12, "COM.z should be 0, got {}", cz);
    }

    /// `center_of_mass(empty_snapshot)` returns `Value::Undef` — zero-mass
    /// system has no canonical COM (would be a divide-by-zero).
    #[test]
    fn center_of_mass_on_empty_snapshot_returns_undef() {
        let m0 = eval_builtin("mechanism", &[]);
        let s = eval_builtin("snapshot", &[m0, Value::List(vec![])]);
        assert!(eval_builtin("center_of_mass", &[s]).is_undef());
    }

    /// `center_of_mass(s, Value::Map(empty))` is treated as uniform
    /// density (per spec §13.3 — an empty densities Map has no effect on
    /// the partial-map fallback to 1.0 per body).  Result must equal the
    /// no-densities-arg call.
    #[test]
    fn center_of_mass_with_empty_densities_map_uses_uniform() {
        let (s, _, _) = make_two_body_snapshot();
        let result = eval_builtin(
            "center_of_mass",
            &[s, Value::Map(std::collections::BTreeMap::new())],
        );
        let [cx, cy, cz] = decompose_point3_length_for_assert(&result);
        assert!(cx.abs() < 1e-12, "COM.x with empty densities should be 0, got {}", cx);
        assert!(cy.abs() < 1e-12, "COM.y with empty densities should be 0, got {}", cy);
        assert!(cz.abs() < 1e-12, "COM.z with empty densities should be 0, got {}", cz);
    }

    /// `center_of_mass(s, Value::Undef)` is treated as the uniform
    /// default (no densities arg).  Useful for callers that pass an
    /// optional value that may be Undef (e.g., a let-binding that
    /// failed validation upstream).
    #[test]
    fn center_of_mass_with_undef_densities_uses_uniform() {
        let (s, _, _) = make_two_body_snapshot();
        let result = eval_builtin("center_of_mass", &[s, Value::Undef]);
        let [cx, cy, cz] = decompose_point3_length_for_assert(&result);
        assert!(cx.abs() < 1e-12, "COM.x with Undef densities should be 0, got {}", cx);
        assert!(cy.abs() < 1e-12, "COM.y with Undef densities should be 0, got {}", cy);
        assert!(cz.abs() < 1e-12, "COM.z with Undef densities should be 0, got {}", cz);
    }

    // ── center_of_mass with per-body density Map ──────────────────────────
    //
    // densities is a `Value::Map { id → density }`.  Bodies absent from
    // the map fall back to density 1.0 (uniform fallback for partial
    // maps).  Non-numeric density entries (or a non-Map densities arg)
    // collapse the whole call to Undef.

    /// Helper: build a 2-body Snapshot with bodies at world translations
    /// (-1.0, 0, 0) and (+1.0, 0, 0).  Uses a wider prismatic range so
    /// the binding values land cleanly inside it.
    fn make_two_body_snapshot_unit_separation() -> Value {
        let j_neg = eval_builtin(
            "prismatic",
            &[
                axis_x_unit(),
                Value::Range {
                    lower: Some(Box::new(Value::length(-2.0))),
                    upper: Some(Box::new(Value::length(0.0))),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
            ],
        );
        let j_pos = eval_builtin(
            "prismatic",
            &[
                axis_x_unit(),
                Value::Range {
                    lower: Some(Box::new(Value::length(0.0))),
                    upper: Some(Box::new(Value::length(2.0))),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
            ],
        );

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin(
            "body",
            &[m0, Value::String("a".to_string()), j_neg.clone()],
        );
        let m2 = eval_builtin(
            "body",
            &[m1, Value::String("b".to_string()), j_pos.clone()],
        );

        let bind_neg = eval_builtin("bind", &[j_neg, Value::length(-1.0)]);
        let bind_pos = eval_builtin("bind", &[j_pos, Value::length(1.0)]);
        eval_builtin("snapshot", &[m2, Value::List(vec![bind_neg, bind_pos])])
    }

    /// `center_of_mass(s, {Int(0): Real(3.0), Int(1): Real(1.0)})` on a
    /// 2-body Snapshot with bodies at (-1, 0, 0) and (+1, 0, 0):
    /// COM = (3·(-1) + 1·(+1)) / (3 + 1) = -0.5.
    #[test]
    fn center_of_mass_per_body_densities_weighted_mean() {
        let s = make_two_body_snapshot_unit_separation();
        let mut densities = std::collections::BTreeMap::new();
        densities.insert(Value::Int(0), Value::Real(3.0));
        densities.insert(Value::Int(1), Value::Real(1.0));
        let result = eval_builtin("center_of_mass", &[s, Value::Map(densities)]);
        let [cx, cy, cz] = decompose_point3_length_for_assert(&result);
        assert!(
            (cx - (-0.5)).abs() < 1e-12,
            "COM.x with densities {{0:3, 1:1}} should be -0.5, got {}",
            cx
        );
        assert!(cy.abs() < 1e-12, "COM.y should be 0, got {}", cy);
        assert!(cz.abs() < 1e-12, "COM.z should be 0, got {}", cz);
    }

    /// Partial densities Map: only body 0 is listed (density 3.0); body 1
    /// is absent and falls back to the uniform default of 1.0.
    /// COM = (3·(-1) + 1·(+1)) / (3 + 1) = -0.5 (same as the explicit
    /// fully-specified case).
    #[test]
    fn center_of_mass_partial_densities_map_falls_back_to_one() {
        let s = make_two_body_snapshot_unit_separation();
        let mut densities = std::collections::BTreeMap::new();
        densities.insert(Value::Int(0), Value::Real(3.0));
        // Body 1 absent — should fall back to 1.0.
        let result = eval_builtin("center_of_mass", &[s, Value::Map(densities)]);
        let [cx, _, _] = decompose_point3_length_for_assert(&result);
        assert!(
            (cx - (-0.5)).abs() < 1e-12,
            "COM.x with partial densities {{0:3}} (1 absent → 1.0) should be -0.5, got {}",
            cx
        );
    }

    /// `center_of_mass(s, non_map_densities)` returns Undef when args[1]
    /// is neither Undef, an empty Map, nor a populated Map.  Covers
    /// `Value::Real`, `Value::List`, and `Value::String` — all three are
    /// non-Map shapes that must be rejected before any FK arithmetic.
    #[test]
    fn center_of_mass_non_map_densities_returns_undef() {
        let s = make_two_body_snapshot_unit_separation();
        // Real
        assert!(eval_builtin("center_of_mass", &[s.clone(), Value::Real(1.0)]).is_undef());
        // List
        assert!(eval_builtin("center_of_mass", &[s.clone(), Value::List(vec![])]).is_undef());
        // String
        assert!(eval_builtin(
            "center_of_mass",
            &[s, Value::String("uniform".to_string())]
        )
        .is_undef());
    }

    /// `center_of_mass(s, {Int(0): String(...)})`: a non-numeric density
    /// value collapses the whole call to Undef.  Mirrors the strict
    /// rejection in `bind`/`snapshot` — silent fallback to 1.0 here would
    /// paper over a bug at the call site.
    #[test]
    fn center_of_mass_non_numeric_density_value_returns_undef() {
        let s = make_two_body_snapshot_unit_separation();
        let mut densities = std::collections::BTreeMap::new();
        densities.insert(Value::Int(0), Value::String("heavy".to_string()));
        densities.insert(Value::Int(1), Value::Real(1.0));
        assert!(eval_builtin("center_of_mass", &[s, Value::Map(densities)]).is_undef());
    }

    /// `center_of_mass()` validation surface: arity outside {1, 2} and
    /// non-snapshot args[0] both return `Value::Undef`.
    #[test]
    fn center_of_mass_validation_returns_undef() {
        let (s, _, _) = make_two_body_snapshot();
        // Wrong arity (0, 3 args)
        assert!(eval_builtin("center_of_mass", &[]).is_undef());
        assert!(eval_builtin(
            "center_of_mass",
            &[s.clone(), Value::Undef, Value::Undef]
        )
        .is_undef());
        // Non-snapshot first arg: Real, world sentinel, mechanism
        assert!(eval_builtin("center_of_mass", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("center_of_mass", &[eval_builtin("world", &[])]).is_undef());
        let m0 = eval_builtin("mechanism", &[]);
        assert!(eval_builtin("center_of_mass", &[m0]).is_undef());
    }
}
