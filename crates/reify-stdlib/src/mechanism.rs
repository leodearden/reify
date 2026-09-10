//! Mechanism builder stdlib (task 2528).
//!
//! Implements the v0.1 `mechanism().body(...)` builder per
//! `docs/prds/kinematic-constraints.md` task 3 and `docs/reify-stdlib-reference.md` §13.2.
//!
//! Mechanism state is encoded as a `Value::Map` with the shape:
//! `{ "kind": "mechanism", "bodies": List(body_record...), "joint_parents": Map(joint→parent), "loop_closures": List(loop_closure_record...), "next_id": Int(N) }`.
//!
//! `loop_closures` (v0.2) records closed-chain edges as constraint records
//! rather than rejecting them — see `make_loop_closure_record` for the
//! per-entry shape. Open-chain mechanisms carry an empty list.
//!
//! On a `duplicate_solid` or `world_parented_closure` error the Map
//! additionally carries `error`, `error_path1`, `error_path2`, and
//! `error_message` fields (`error_path1` and `error_path2` are empty Lists
//! for both — they were used by the v0.1 `closed_chain` error which is no
//! longer emitted). See plan §"Mechanism Map shape".
//!
//! `world_parented_closure` (task 7186) rejects a CLOSING edge whose
//! `parent` is the world sentinel: such a closure has no joint on the
//! closing side, so the loop-closure solver has no free variable to satisfy
//! it. A plain OPEN edge parented to `world()` is the common case and is
//! unaffected.
//!
//! Diagnostic emission via `EvalResult.diagnostics` is deferred to the
//! snapshot/eval-pipeline integration (`DiagnosticCode::KinematicClosedChain`
//! and `DiagnosticCode::MechanismDuplicateSolid` are reserved in
//! `reify-types/src/diagnostics.rs` for that future integration).

use std::collections::BTreeMap;

use reify_ir::Value;

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
/// - `loop_closures` → `Value::List(vec![])` — records loop-closure
///   constraints (one entry per closing body() call; see v0.2 migration).
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
    m.insert(
        Value::String("loop_closures".to_string()),
        Value::List(vec![]),
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

/// Encode a body `pose` as a synthetic 0-DOF rigid link:
/// `Value::Map { "kind": "fixed", "origin": <pose> }`.
///
/// **Why this shape composes correctly through the unmodified chain
/// machinery.** `joints.rs::transform_at` computes the per-kind motion
/// first and then applies `origin ∘ motion` UNIFORMLY, once, outside every
/// per-kind arm (PRD §7.2). The `fixed` arm's motion is the identity, so
/// the link evaluates to exactly `origin ∘ I = pose`. Every existing chain
/// consumer — `chain_transform`, `chain_jacobian_fd`,
/// `loop_residual_jacobian_by_joint` — therefore consumes it with no
/// signature change, and `extract_loop_closure_chains` resolves it to the
/// 0-DOF sentinel without making it a solver free variable (task 7186
/// step-4).
pub(crate) fn pose_link(pose: &Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("fixed".to_string()),
    );
    m.insert(Value::String("origin".to_string()), pose.clone());
    Value::Map(m)
}

/// Structural equality against [`identity_transform`].
///
/// This is a SIZE OPTIMISATION, not a correctness gate: a semantically-
/// identity pose that fails this structural test simply yields an identity
/// rigid link, which composes to a no-op in the residual. Its only job is
/// to keep the closure paths of the 3-/4-arg `body()` forms (whose pose
/// defaults to `identity_transform()`) byte-identical to their pre-task-7186
/// shapes.
fn is_identity_pose(pose: &Value) -> bool {
    pose == &identity_transform()
}

/// Build a body record `Value::Map` with the standard five-key layout:
/// `at`, `id`, `parent`, `pose`, `solid` (alphabetical, matching `BTreeMap`
/// iteration). Parallel to `make_joint`/`make_coupling` in `joints.rs`.
///
/// **Subtle: `parent` vs spanning-tree on closing edges.** The `parent`
/// field stored in the body record reflects the user-supplied `parent`
/// joint from the `body()` call — i.e. user intent. For closing edges
/// (parent-conflict, joint-graph cycle, or self-loop), the spanning-tree
/// edge that forward-kinematics uses is recorded in the mechanism's
/// `joint_parents` map (which retains the first-recorded `at → parent`
/// edge under v0.2's "first-recorded wins" policy), NOT in the body
/// record. So for closing-edge bodies, `body.parent` may disagree with
/// `joint_parents.get(body.at)`. Consumers that compute FK or walk the
/// spanning tree must read `joint_parents`, not `body.parent`. The
/// closing edge itself is captured separately in the mechanism's
/// `loop_closures` list.
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
fn cycle_introduced(pre_edge: &BTreeMap<Value, Value>, at: &Value, parent: &Value) -> bool {
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

/// Decorate an existing Mechanism Map with `duplicate_solid` error fields.
/// Preserves the input's `bodies`, `joint_parents`, `loop_closures`,
/// `next_id`, and `kind` fields verbatim and appends `error`,
/// `error_path1`, `error_path2`, `error_message`.
///
/// The sole caller is the duplicate-solid branch of `append_body`.
/// `error_path1` and `error_path2` are emitted as empty `Value::List`s
/// for v0.1 error-Map shape uniformity (see module-level doc lines 13-16);
/// `duplicate_solid` has no path-shaped diagnostic context.
fn make_duplicate_solid_error(mech_map: &BTreeMap<Value, Value>, message: String) -> Value {
    let mut new_map = mech_map.clone();
    new_map.insert(
        Value::String("error".to_string()),
        Value::String("duplicate_solid".to_string()),
    );
    new_map.insert(
        Value::String("error_message".to_string()),
        Value::String(message),
    );
    new_map.insert(
        Value::String("error_path1".to_string()),
        Value::List(Vec::new()),
    );
    new_map.insert(
        Value::String("error_path2".to_string()),
        Value::List(Vec::new()),
    );
    Value::Map(new_map)
}

/// Decorate an existing Mechanism Map with `world_parented_closure` error
/// fields. Direct analogy of [`make_duplicate_solid_error`] — same four-key
/// decoration (`error`, `error_message`, empty-List `error_path1` /
/// `error_path2`), same preserve-everything-else contract — stamped with this
/// error's own discriminator rather than a second error vocabulary.
///
/// The sole caller is the world-parent guard in `append_body`'s
/// parent-conflict branch (task 7186 review fix 1); the WHY lives at that
/// call site.
fn make_world_parented_closure_error(mech_map: &BTreeMap<Value, Value>, message: String) -> Value {
    let mut new_map = mech_map.clone();
    new_map.insert(
        Value::String("error".to_string()),
        Value::String("world_parented_closure".to_string()),
    );
    new_map.insert(
        Value::String("error_message".to_string()),
        Value::String(message),
    );
    new_map.insert(
        Value::String("error_path1".to_string()),
        Value::List(Vec::new()),
    );
    new_map.insert(
        Value::String("error_path2".to_string()),
        Value::List(Vec::new()),
    );
    Value::Map(new_map)
}

/// Build a single loop-closure record `Value::Map` with the five-key shape:
/// `{ body_id, closing_joint, kind="loop_closure", path_a, path_b }`.
///
/// Both paths carry the world sentinel at the head, but they do **not**
/// share a terminator — the two shapes differ, and differ again by branch
/// (task 7186 defect A):
///
/// * `path_a` is always `[world, joint_0, ..., closing_joint]`: the
///   spanning-tree walk down to the shared pivot, terminating AT the
///   closing joint.
/// * `path_b`, on the **parent-conflict** branch, is
///   `[world, joint_0, ..., parent]`: the walk that reaches the same pivot
///   through the closing edge's `parent`. It terminates at `parent` and
///   does **not** contain the closing joint at all — composing the closing
///   joint on both sides conjugates the residual instead of cancelling
///   (see the comment at the push site in `append_body`).
/// * `path_b`, on the **cycle / self-loop** branch, retains its
///   `[world, ..., at]` marker shape: `at` is appended so the closing node
///   is visible twice (once mid-walk as an ancestor of `parent`, once at
///   the tail). Its PRESENCE in `path_b` — not the duplication — is the
///   classification signal `mechanism_loop_closure_chains` reads to emit
///   `LoopClosureChain::Cycle`, and those chains are not solver-feedable in
///   the first place. The same signal catches the parent-conflict ANCESTOR
///   case, where `walk_to_world(parent)` passes through `at` and so puts the
///   closing joint mid-walk with no trailing marker.
///
/// On the parent-conflict branch `path_b` — and ONLY `path_b` — may
/// additionally carry ONE trailing synthetic 0-DOF rigid link
/// `{ kind: "fixed", origin: <pose> }` (task 7186 defect B, as amended by
/// review fix 2). It encodes the single meaning of `pose` on a closing
/// call: the closing edge is a rigid 0-DOF TIE from `parent` to `at`, so
/// the residual is `T_tree(at) == T(parent) ∘ pose` and the offset belongs
/// to the closing side alone. `path_a` is always joint-only. An identity
/// pose contributes no link, so the 3-/4-arg `body()` forms leave both
/// paths joint-only.
///
/// The closing joint is always available from the record's explicit
/// `closing_joint` field, on every branch.
fn make_loop_closure_record(
    body_id: i64,
    closing_joint: Value,
    path_a: Vec<Value>,
    path_b: Vec<Value>,
) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("body_id".to_string()), Value::Int(body_id));
    m.insert(Value::String("closing_joint".to_string()), closing_joint);
    m.insert(
        Value::String("kind".to_string()),
        Value::String("loop_closure".to_string()),
    );
    m.insert(Value::String("path_a".to_string()), Value::List(path_a));
    m.insert(Value::String("path_b".to_string()), Value::List(path_b));
    Value::Map(m)
}

/// Append a body record to a Mechanism `Value::Map`, returning the new
/// (immutable) Mechanism Map. The 3-/4-/5-arg `body()` paths all
/// delegate here after substituting defaults for omitted arguments.
///
/// Side effects on the returned Map (vs. the input):
/// - `bodies` list grows by one record (with `id = m.next_id`).
/// - `joint_parents` records `at → parent` for open-chain edges.
///   On a closing edge (parent conflict or cycle), the spanning tree
///   is left untouched and a loop-closure record is appended to
///   `loop_closures` instead.
/// - `loop_closures` grows by one entry when a closing edge is detected.
/// - `next_id` increments by one.
///
/// Duplicate-solid detection still produces an error Map (unchanged
/// from v0.1). Closed-chain edges are now recorded as loop closures
/// (v0.2 behaviour — no error emitted), with ONE exception: a closing
/// edge whose `parent` is the world sentinel produces a
/// `world_parented_closure` error Map (task 7186 — see the guard's own
/// comment in the parent-conflict branch for why rejection beats every
/// softer remedy).
fn append_body(
    mech_map: &BTreeMap<Value, Value>,
    solid: Value,
    at: Value,
    parent: Value,
    pose: Value,
) -> Value {
    // Extract current bodies / joint_parents / next_id / loop_closures
    // with defense-in-depth fallbacks (the caller validated `kind = "mechanism"`).
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
    let mut loop_closures = match mech_map.get(&Value::String("loop_closures".to_string())) {
        Some(Value::List(lc)) => lc.clone(),
        // Defense-in-depth for hand-built Mechanism Maps (e.g. test
        // fixtures) that omit the field. `make_empty_mechanism` always
        // emits `loop_closures`, so no Mechanism Map produced by the
        // v0.2 builder reaches this branch.
        None => Vec::new(),
        // A present-but-wrong-typed value indicates a corrupt mechanism —
        // reject with Undef, matching the sibling-field guards at lines
        // 435-446 (bodies, joint_parents, next_id).
        Some(_) => return Value::Undef,
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
    // `bodies` (deferred if a real workload demands it).
    for existing in &bodies {
        if let Value::Map(b) = existing
            && b.get(&Value::String("solid".to_string())) == Some(&solid)
        {
            return make_duplicate_solid_error(
                mech_map,
                "duplicate solid: solid value already attached to a body in this mechanism"
                    .to_string(),
            );
        }
    }

    // Closed-chain conflict detection (v0.2): if `at` is already mapped to
    // a *different* parent, record a loop-closure constraint and continue
    // (do NOT error). The spanning-tree edge is left untouched — the
    // first-recorded `at → existing_parent` edge wins.
    // (Same-parent re-registration is a no-op overwrite handled below.)
    let skip_jp_insert = if let Some(existing_parent) = joint_parents.get(&at)
        && existing_parent != &parent
    {
        // Task 7186 review fix 1: reject a CLOSING edge parented to the world
        // sentinel at build time, before any loop-closure record is built.
        //
        // Why this shape and no other: `path_b` below is
        // `[world] ++ walk_to_world(joint_parents, parent)`, and
        // `walk_to_world` returns an EMPTY vec iff `is_world(parent)` — it
        // breaks before pushing only for the world sentinel, whereas a
        // non-world parent with no recorded ancestor still yields `[parent]`.
        // So `path_b == [world]` (len 1) IFF `is_world(&parent)`; the guard is
        // exact and cannot over-reject (pinned by the negative control
        // `non_world_parented_closing_edge_still_records`).
        //
        // Why a loud rejection and not a softer repair. Both softer remedies —
        // relaxing `strip_world_sentinel` to admit `[world]`, or pushing an
        // identity anchor link so `path_b` reaches len 2 — produce a
        // WELL-FORMED but structurally UNSOLVABLE record: with `parent ==
        // world`, chain_b holds no joints; `is_zero_dof_joint`
        // (loop_closure.rs) correctly keeps the 0-DOF link out of `free_b`, so
        // `free_b == []` for ANY bindings; `validate_loop_closure_inputs`
        // iterates `free_b` only, so an empty one validates; and `newton_solve`
        // at n = 0 returns `NotConverged { x: [] }`, which snapshot.rs accepts
        // on the same arm as `Converged`. Measured on the 5-arg form (which
        // already reaches that state today via the pose link): bodies at
        // 0.5 / 1.5 / 1.7 m carrying a residual twist of [0,0,0,-1.3,0,0] — a
        // 1.3 m unsatisfied closure returned as a normal Snapshot Map with no
        // diagnostic. Both remedies would therefore trade a loud
        // whole-mechanism `Undef` for a SILENT wrong answer. The solver varies
        // only the CLOSING side's joints; a world-parented closing edge is a
        // GROUNDING constraint whose only candidate free variables live on
        // chain_a, so making it solvable is a solver redesign, not a fix here.
        //
        // Scope: only this world-parent subcase is closed, because it is
        // decidable from the path shape alone and step-2 (dropping `at` from
        // `path_b`) made it newly reachable. The GENERAL `free_b.is_empty()`
        // case — every chain_b joint directly bound — has the same structural
        // unsolvability but is not statically decidable here; it is the
        // verdict-integrity surface owned by #7185.
        //
        // The guard is inside the parent-conflict arm on purpose: a plain OPEN
        // edge `body(m, s, j, world)` is the common case and is untouched.
        if is_world(&parent) {
            return make_world_parented_closure_error(
                mech_map,
                "closing edge parented to world(): a loop closure whose closing edge attaches \
                 to world() has no joint on the closing side, so the loop-closure solver has \
                 no free variable to satisfy it. Parent the closing edge to a joint on the \
                 other branch of the loop instead."
                    .to_string(),
            );
        }
        let world = make_world_sentinel();
        let mut path_a = vec![world.clone()];
        path_a.extend(walk_to_world(&joint_parents, existing_parent));
        path_a.push(at.clone());
        // Task 7186 review fix 2: `path_a` carries NO pose link — it is
        // joint-only, terminating at the closing joint's OUTPUT frame.
        //
        // Step-6 pushed `first_body_pose(&bodies, &at)` here. That was
        // wrong twice over. The residual constrains JOINT frames, and
        // `first_body_pose` returns the pose of the FIRST-RECORDED body at
        // `at` — in general a DIFFERENT body from the closing one (in the
        // `p4_platform` fixture, "post2" rather than "closing"). That pose
        // places THAT body's own solid; where a third body's solid sits has
        // no bearing on where the two joint frames must coincide. Composing
        // it made chain_a's terminal a body-solid frame while chain_b's is a
        // joint frame plus an edge offset — two different things equated. It
        // was inert in-tree only because every fixture's first-recorded body
        // carries the default identity pose. Pinned by
        // `first_recorded_body_pose_stays_out_of_path_a`.
        let mut path_b = vec![world];
        path_b.extend(walk_to_world(&joint_parents, &parent));
        // Task 7186 defect A: `at` is deliberately NOT appended here.
        //
        // The closing joint's transform belongs to exactly ONE side of the
        // loop: chain_a reaches the shared pivot through the spanning tree
        // (…→ existing_parent → at), and chain_b reaches that SAME pivot
        // through the closing edge's `parent`. Appending `at` to both sides
        // makes the residual `log(inv(T_a) · T_b)` with `T_a = X·A` and
        // `T_b = Y·A` — whose zero set is `inv(A)·inv(X)·Y·A = I ⟺ X = Y`.
        // So `A` does not cancel harmlessly; it CONJUGATES the residual and
        // relocates the closure from the closing joint's OUTPUT frame to its
        // BASE frame. On the Grashof 4-bar
        // (examples/kinematic/relate_mounted_fourbar.ri) that relocation
        // moves the closure from pivot C to pivot B and makes the system
        // infeasible by 1.045 mm, so Newton returns a least-squares point
        // ~1.09 rad away from the analytic assembly.
        //
        // The asymmetric shape produced here is the one the in-tree
        // hand-built reference chains already use:
        // reify-eval-fea-tests/tests/closed_chain_idyn_e2e.rs (B4) and
        // reify-eval/tests/relate_mounted_joint_sweep_e2e.rs (B7).
        //
        // Task 7186 defect B, as amended by review fix 2. THE SINGLE MEANING
        // of `pose` on a closing call: the closing edge is a rigid 0-DOF TIE
        // from `parent` to `at` whose transform is `pose`. So the residual is
        //
        //     T_tree(at)  ==  T(parent) ∘ pose
        //
        // — chain_a walks the spanning tree to `at`, chain_b walks to
        // `parent` and then applies the tie. `pose` therefore appears exactly
        // ONCE, at chain_b's tail, and never decorates a joint mid-walk.
        //
        // (Step-6 justified the terminal placement with "that is what
        // `walk_fk` does". That was measurably wrong for this side: `walk_fk`
        // read `pose` as an offset from the body's own `at` frame, so the
        // closing body got its pose applied a SECOND time and landed 206.2 mm
        // off the frame the solve had just enforced. `walk_fk` now composes a
        // parent-conflict closing body from `body.parent` to match the rule
        // above — see its comment in snapshot.rs.)
        if !is_identity_pose(&pose) {
            path_b.push(pose_link(&pose));
        }
        let lc = make_loop_closure_record(next_id, at.clone(), path_a, path_b);
        loop_closures.push(lc);
        true // skip joint_parents.insert below
    } else if cycle_introduced(&joint_parents, &at, &parent) {
        // Closed-chain cycle detection (v0.2): if walking from `parent`
        // upward in the pre-edge `joint_parents` reaches `at`, the new
        // edge would close a cycle. Record a loop-closure constraint;
        // do NOT add the closing edge to the spanning tree.
        //
        // Self-loops (at == parent) are subsumed here because
        // `cycle_introduced` returns true on the first iteration when
        // current == at.
        let world = make_world_sentinel();
        // path_a: `at`'s pre-edge ancestor chain.
        let mut path_a = vec![world.clone()];
        path_a.extend(walk_to_world(&joint_parents, &at));
        // path_b: pre-edge ancestor walk from `parent` top-down, with
        // `at` appended as the closing node (appears twice when at is
        // already an ancestor — the canonical "cycle visible" shape).
        let mut path_b = vec![world];
        path_b.extend(walk_to_world(&joint_parents, &parent));
        path_b.push(at.clone());
        let lc = make_loop_closure_record(next_id, at.clone(), path_a, path_b);
        loop_closures.push(lc);
        true // skip joint_parents.insert below
    } else {
        false
    };

    // Build and append the new body record. Always done even for closing
    // edges — the body's `parent` field preserves user intent; the
    // spanning-tree FK reads from `joint_parents`, not `body.parent`.
    bodies.push(make_body_record(
        next_id,
        solid,
        at.clone(),
        parent.clone(),
        pose,
    ));

    // Record (at → parent) in joint_parents only for open-chain edges.
    // Closing edges are recorded in `loop_closures` above; inserting
    // them into joint_parents would break the spanning-tree acyclicity
    // invariant or silently overwrite the first-recorded edge.
    if !skip_jp_insert {
        joint_parents.insert(at, parent);
    }

    // Build the new Mechanism Map. Preserve the input map's other
    // fields verbatim.
    let mut new_map = mech_map.clone();
    new_map.insert(Value::String("bodies".to_string()), Value::List(bodies));
    new_map.insert(
        Value::String("joint_parents".to_string()),
        Value::Map(joint_parents),
    );
    new_map.insert(
        Value::String("loop_closures".to_string()),
        Value::List(loop_closures),
    );
    new_map.insert(
        Value::String("next_id".to_string()),
        Value::Int(next_id + 1),
    );
    Value::Map(new_map)
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::test_fixtures::{
        angle_range_0_to_pi, axis_x_unit, axis_y_unit, axis_z_unit, identity_transform_value,
        length_range_0_to_1m,
    };
    use reify_ir::Value;
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
        assert_eq!(
            map.get(&Value::String("loop_closures".to_string())),
            Some(&Value::List(vec![])),
            "loop_closures field should be an empty List"
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
        let m2 = eval_builtin("body", &[m1, solid_b.clone(), j_b.clone(), j_a.clone()]);

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
        assert!(
            eval_builtin(
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
            .is_undef()
        );
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

        assert!(eval_builtin("body", &[m0, solid, Value::String("foo".to_string())]).is_undef());
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

    /// v0.2: `body()` calls that try to give the same joint two different
    /// parents (`j_x` → `j_a` from call 1, `j_x` → `j_b` from call 2)
    /// now record a loop-closure constraint instead of erroring.
    ///
    /// Assertions:
    /// - returned Map has NO `error` key
    /// - `bodies` has length 2 (closing body IS appended)
    /// - bodies[1].at == j_x, bodies[1].parent == j_b, bodies[1].id == Int(1)
    /// - `joint_parents.get(j_x) == Some(j_a)` (first-recorded edge wins)
    /// - `loop_closures` is a List with exactly one Map entry:
    ///   `kind="loop_closure"`, `body_id=Int(1)`, `closing_joint=j_x`,
    ///   path_a=[world, j_a, j_x], path_b=[world, j_b]
    ///
    /// Task 7186 defect A: `path_b` used to be `[world, j_b, j_x]`. The
    /// closing joint belongs to exactly one side of the loop — appending it
    /// to both conjugates the residual rather than cancelling out of it (see
    /// `parent_conflict_path_b_omits_closing_joint` and the push-site comment
    /// in `append_body`). This expectation encoded the double count.
    #[test]
    fn parent_conflict_records_loop_closure_constraint() {
        // j_a, j_b distinct; j_x distinct again.
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        // Call 1: body(m0, solid_a, j_x, j_a) records j_x → j_a.
        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_x.clone(), j_a.clone()]);
        // Call 2: body(m1, solid_b, j_x, j_b) — j_x already → j_a, this is a
        // closing edge. v0.2: must record a loop-closure, NOT error.
        let m2 = eval_builtin("body", &[m1, solid_b, j_x.clone(), j_b.clone()]);

        let map = match m2 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };

        // No error key.
        assert!(
            !map.contains_key(&Value::String("error".to_string())),
            "parent-conflict in v0.2 must NOT produce an error key; got error={:?}",
            map.get(&Value::String("error".to_string()))
        );

        // Both bodies are present.
        let bodies = match map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected bodies List, got {:?}", other),
        };
        assert_eq!(
            bodies.len(),
            2,
            "closing body must be appended (bodies.len()==2)"
        );

        // Second body record: at=j_x, parent=j_b, id=1.
        let body1 = match &bodies[1] {
            Value::Map(b) => b,
            other => panic!("expected body record Map, got {:?}", other),
        };
        assert_eq!(
            body1.get(&Value::String("at".to_string())),
            Some(&j_x),
            "bodies[1].at should be j_x"
        );
        assert_eq!(
            body1.get(&Value::String("parent".to_string())),
            Some(&j_b),
            "bodies[1].parent should be j_b (user intent preserved)"
        );
        assert_eq!(
            body1.get(&Value::String("id".to_string())),
            Some(&Value::Int(1)),
            "bodies[1].id should be Int(1)"
        );

        // First-recorded edge wins: j_x → j_a (NOT overwritten by j_b).
        let jp = match map.get(&Value::String("joint_parents".to_string())) {
            Some(Value::Map(jp)) => jp,
            other => panic!("expected joint_parents Map, got {:?}", other),
        };
        assert_eq!(
            jp.get(&j_x),
            Some(&j_a),
            "joint_parents[j_x] should still be j_a (first-recorded wins)"
        );

        // loop_closures has exactly one entry.
        let loop_closures = match map.get(&Value::String("loop_closures".to_string())) {
            Some(Value::List(lc)) => lc,
            other => panic!("expected loop_closures List, got {:?}", other),
        };
        assert_eq!(
            loop_closures.len(),
            1,
            "exactly one loop-closure entry expected"
        );

        let lc = match &loop_closures[0] {
            Value::Map(m) => m,
            other => panic!("expected loop_closure Map, got {:?}", other),
        };
        assert_eq!(
            lc.get(&Value::String("kind".to_string())),
            Some(&Value::String("loop_closure".to_string())),
            "loop_closure record kind should be 'loop_closure'"
        );
        assert_eq!(
            lc.get(&Value::String("body_id".to_string())),
            Some(&Value::Int(1)),
            "loop_closure body_id should be Int(1)"
        );
        assert_eq!(
            lc.get(&Value::String("closing_joint".to_string())),
            Some(&j_x),
            "loop_closure closing_joint should be j_x"
        );
        let world = eval_builtin("world", &[]);
        assert_eq!(
            lc.get(&Value::String("path_a".to_string())),
            Some(&Value::List(vec![world.clone(), j_a.clone(), j_x.clone()])),
            "path_a should be [world, j_a, j_x]"
        );
        assert_eq!(
            lc.get(&Value::String("path_b".to_string())),
            Some(&Value::List(vec![world.clone(), j_b.clone()])),
            "path_b should be [world, j_b] — the closing joint j_x is composed \
             on path_a only (task 7186 defect A)"
        );
    }

    // ── closing-joint composition: chain_b must NOT re-append it ─────────

    /// **Task 7186 defect A.** The parent-conflict branch must append the
    /// closing joint to `path_a` only — never to `path_b`.
    ///
    /// The residual the solver drives to zero is `log(inv(T_a) · T_b)`.
    /// With the closing joint `A` appended to BOTH sides we get `T_a = X·A`
    /// and `T_b = Y·A`, whose zero set is `inv(A)·inv(X)·Y·A = I ⟺ X = Y` —
    /// so `A` does not cancel harmlessly, it CONJUGATES the residual and
    /// relocates the closure from the closing joint's OUTPUT frame to its
    /// BASE frame. For the Grashof 4-bar that relocation makes the loop
    /// infeasible by 1.045 mm (see
    /// `snapshot_grashof_fourbar_converges_to_analytic_closure`).
    ///
    /// The correct, asymmetric shape is already the one the in-tree
    /// hand-built reference chains use —
    /// `reify-eval/tests/relate_mounted_joint_sweep_e2e.rs` (B7) and
    /// `reify-eval-fea-tests/tests/closed_chain_idyn_e2e.rs` (B4) both feed
    /// `chain_a = [.., closing_joint]` against a `chain_b` that stops at the
    /// closing edge's `parent`.
    ///
    /// Fixture is the parent-conflict shape of
    /// `parent_conflict_records_loop_closure_constraint`: `j_x → j_a` from
    /// call 1, then the closing `body(m, solidB, j_x, j_b)`.
    #[test]
    fn parent_conflict_path_b_omits_closing_joint() {
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin(
            "body",
            &[
                m0,
                Value::String("solidA".to_string()),
                j_x.clone(),
                j_a.clone(),
            ],
        );
        let m2 = eval_builtin(
            "body",
            &[
                m1,
                Value::String("solidB".to_string()),
                j_x.clone(),
                j_b.clone(),
            ],
        );

        let map = match m2 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };
        let loop_closures = match map.get(&Value::String("loop_closures".to_string())) {
            Some(Value::List(lc)) => lc,
            other => panic!("expected loop_closures List, got {:?}", other),
        };
        assert_eq!(
            loop_closures.len(),
            1,
            "exactly one loop-closure entry expected"
        );
        let lc = match &loop_closures[0] {
            Value::Map(m) => m,
            other => panic!("expected loop_closure Map, got {:?}", other),
        };

        let world = eval_builtin("world", &[]);
        // chain_a still terminates at the closing joint — it reaches the
        // shared pivot through the spanning tree.
        assert_eq!(
            lc.get(&Value::String("path_a".to_string())),
            Some(&Value::List(vec![world.clone(), j_a.clone(), j_x.clone()])),
            "path_a must still be [world, j_a, j_x] (unchanged)"
        );
        // chain_b reaches the SAME pivot through `parent` and must stop
        // there: the closing joint's transform belongs to exactly one side.
        assert_eq!(
            lc.get(&Value::String("path_b".to_string())),
            Some(&Value::List(vec![world.clone(), j_b.clone()])),
            "path_b must be [world, j_b] — the closing joint must NOT be re-appended"
        );
        // The record's explicit closing_joint field is unaffected: consumers
        // that need the closing joint read it from here, not from chain_b.last().
        assert_eq!(
            lc.get(&Value::String("closing_joint".to_string())),
            Some(&j_x),
            "closing_joint field is unchanged by the path-shape fix"
        );
    }

    // ── closing body pose enters the closure path (task 7186 defect B) ───

    /// The synthetic 0-DOF rigid link's SHAPE contract, pinned literally in
    /// exactly one place. Every other site — in this module and in
    /// `loop_closure.rs`'s tests — calls `pose_link` itself, so a shape change
    /// fails here (loudly, against the spelled-out Map) instead of silently
    /// agreeing with itself everywhere.
    ///
    /// `kind = "fixed"` is what makes the link compose correctly and stay out
    /// of `free_b`: `joints.rs::transform_at` applies `origin ∘ motion` outside
    /// every per-kind arm and the fixed arm's motion is the identity, so the
    /// link evaluates to exactly `origin`.
    #[test]
    fn pose_link_is_a_fixed_kind_map_carrying_the_pose_as_origin() {
        let pose = pose_translate_1mm_x();
        let mut expected = BTreeMap::new();
        expected.insert(
            Value::String("kind".to_string()),
            Value::String("fixed".to_string()),
        );
        expected.insert(Value::String("origin".to_string()), pose.clone());
        assert_eq!(
            super::pose_link(&pose),
            Value::Map(expected),
            "pose_link must emit exactly {{ kind: \"fixed\", origin: <pose> }}"
        );
    }

    /// Pull the single loop-closure record's (path_a, path_b) out of a
    /// Mechanism Map.
    fn only_closure_paths(m: &Value) -> (Value, Value) {
        let map = match m {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };
        assert!(
            !map.contains_key(&Value::String("error".to_string())),
            "fixture should not produce an errored mechanism"
        );
        let lcs = match map.get(&Value::String("loop_closures".to_string())) {
            Some(Value::List(lc)) => lc,
            other => panic!("expected loop_closures List, got {:?}", other),
        };
        assert_eq!(lcs.len(), 1, "exactly one loop-closure entry expected");
        let lc = match &lcs[0] {
            Value::Map(m) => m,
            other => panic!("expected loop_closure Map, got {:?}", other),
        };
        (
            lc.get(&Value::String("path_a".to_string()))
                .expect("path_a")
                .clone(),
            lc.get(&Value::String("path_b".to_string()))
                .expect("path_b")
                .clone(),
        )
    }

    /// **Task 7186 defect B.** A body's `pose` — the rigid-link offset
    /// between the joint frame and the body — is written into every body
    /// record but reaches only `walk_fk`; it never enters the closure
    /// residual. A rigid platform carried by several joints at different
    /// pivots is therefore inexpressible.
    ///
    /// The fix encodes the CLOSING call's own pose as a synthetic 0-DOF
    /// rigid link at `path_b`'s tail — the transform of the rigid tie
    /// `parent --pose--> at`, giving the residual
    /// `T_tree(at) == T(parent) ∘ pose`. `path_a` stays joint-only
    /// (review fix 2; see `first_recorded_body_pose_stays_out_of_path_a`).
    ///
    /// The pose lands at the path TERMINAL, never interleaved after each
    /// joint: it is the transform of ONE edge — the closing one — not a
    /// decoration of every joint along the walk.
    #[test]
    fn closing_body_pose_enters_path_b() {
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let world = eval_builtin("world", &[]);
        let pose = pose_translate_1mm_x();

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin(
            "body",
            &[
                m0,
                Value::String("solidA".to_string()),
                j_x.clone(),
                j_a.clone(),
            ],
        );
        // Closing edge via the 5-arg form: the pose is the rigid offset
        // between the two attachment frames of the loop.
        let m2 = eval_builtin(
            "body",
            &[
                m1,
                Value::String("solidD".to_string()),
                j_x.clone(),
                j_b.clone(),
                pose.clone(),
            ],
        );

        let (path_a, path_b) = only_closure_paths(&m2);
        // Task 7186 review fix 2: this is the PIN that stops the path_a
        // deletion from over-reaching. Review fix 2 removes the path_a pose
        // link; the closing call's OWN pose on path_b is the correct half and
        // must survive unchanged — it is the transform of the rigid 0-DOF tie
        // `parent --pose--> at` that the residual now encodes.
        assert_eq!(
            path_b,
            Value::List(vec![world.clone(), j_b.clone(), super::pose_link(&pose)]),
            "path_b must carry the closing call's own pose as a trailing 0-DOF link"
        );
        assert_eq!(
            path_a,
            Value::List(vec![world.clone(), j_a.clone(), j_x.clone()]),
            "path_a is joint-only — it never carries a pose link, whatever the \
             recorded bodies' poses are (task 7186 review fix 2)"
        );
    }

    /// Identity-pose omission rule: the 3-/4-arg `body()` forms default
    /// `pose` to `identity_transform()`, and an identity pose adds no link.
    /// The paths must stay byte-identical to the poseless shapes.
    ///
    /// This is a size optimisation, not a correctness gate — an identity
    /// pose that failed the structural test would simply contribute an
    /// identity link, which composes to a no-op.
    #[test]
    fn identity_body_pose_leaves_closure_paths_unchanged() {
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let world = eval_builtin("world", &[]);

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin(
            "body",
            &[
                m0,
                Value::String("solidA".to_string()),
                j_x.clone(),
                j_a.clone(),
            ],
        );
        // 4-arg closing form (pose defaults to identity).
        let four = eval_builtin(
            "body",
            &[
                m1.clone(),
                Value::String("solidD".to_string()),
                j_x.clone(),
                j_b.clone(),
            ],
        );
        // 5-arg closing form with an EXPLICIT identity pose.
        let five = eval_builtin(
            "body",
            &[
                m1,
                Value::String("solidD".to_string()),
                j_x.clone(),
                j_b.clone(),
                identity_transform_value(),
            ],
        );

        let expect_a = Value::List(vec![world.clone(), j_a.clone(), j_x.clone()]);
        let expect_b = Value::List(vec![world.clone(), j_b.clone()]);
        for (label, m) in [("4-arg", &four), ("5-arg identity", &five)] {
            let (path_a, path_b) = only_closure_paths(m);
            assert_eq!(path_a, expect_a, "{label}: path_a must be unchanged");
            assert_eq!(path_b, expect_b, "{label}: path_b must be unchanged");
        }
    }

    /// **Task 7186 review fix 2.** The FIRST-recorded body's pose must stay
    /// OUT of `path_a`. This directly inverts
    /// `first_recorded_body_pose_enters_path_a` (step-5/6), which step-12
    /// deletes as stale.
    ///
    /// Why the link never belonged there. The residual constrains JOINT
    /// frames: `path_a` descends the spanning tree and terminates at the
    /// closing joint's OUTPUT frame. `first_body_pose(&bodies, &at)` returns
    /// the pose of the FIRST-RECORDED body at `at` — in general a DIFFERENT
    /// body from the closing one (in the `p4_platform` fixture it is "post2",
    /// not "closing"). That pose places THAT body's own solid; where a third
    /// body's solid sits has no bearing on where the two joint frames must
    /// coincide. Composing it made chain_a's terminal a body-solid frame
    /// while chain_b's terminal is a joint frame plus an edge offset — two
    /// different things equated. It was inert in-tree only because every
    /// fixture's first-recorded body carries the default identity pose.
    ///
    /// The single meaning that replaces it: a closing edge is a rigid 0-DOF
    /// TIE from `parent` to `at` whose transform is the CLOSING call's own
    /// `pose`, so the residual is `T_tree(at) == T(parent) ∘ pose` — one
    /// pose, on `path_b`, and none on `path_a`.
    #[test]
    fn first_recorded_body_pose_stays_out_of_path_a() {
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let world = eval_builtin("world", &[]);
        let pose = pose_translate_1mm_x();

        let m0 = eval_builtin("mechanism", &[]);
        // First-recorded body at j_x carries a NON-identity pose — the case
        // that previously leaked a rigid link onto path_a.
        let m1 = eval_builtin(
            "body",
            &[
                m0,
                Value::String("solidA".to_string()),
                j_x.clone(),
                j_a.clone(),
                pose,
            ],
        );
        // Closing edge with the default identity pose.
        let m2 = eval_builtin(
            "body",
            &[
                m1,
                Value::String("solidD".to_string()),
                j_x.clone(),
                j_b.clone(),
            ],
        );

        let (path_a, path_b) = only_closure_paths(&m2);
        assert_eq!(
            path_a,
            Value::List(vec![world.clone(), j_a, j_x]),
            "path_a must be joint-only — the first-recorded body's pose places a \
             DIFFERENT body's solid and must not enter the joint-frame residual"
        );
        assert_eq!(
            path_b,
            Value::List(vec![world, j_b]),
            "path_b is unchanged — the closing call's own pose is identity here"
        );
    }

    // ── world-parented closing edge is rejected (task 7186 review fix 1) ──

    /// Build the three-call shape whose closing edge is parented to
    /// `world()`:
    ///
    /// ```text
    /// body(m0, solidA, j1, world)   → joint_parents: j1 → world
    /// body(m1, solidB, j2, j1)      → joint_parents: j2 → j1
    /// body(m2, solidC, j2, world)   → parent conflict: j2 already → j1
    /// ```
    ///
    /// The third call takes `append_body`'s parent-conflict branch with
    /// `parent == world`, which is the rejected shape. `pose` selects the
    /// 4-arg (`None` → identity) or 5-arg (`Some(p)`) closing form.
    fn world_parented_closure_fixture(pose: Option<Value>) -> Value {
        let j1 = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j2 = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let world = eval_builtin("world", &[]);

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin(
            "body",
            &[
                m0,
                Value::String("solidA".to_string()),
                j1.clone(),
                world.clone(),
            ],
        );
        let m2 = eval_builtin(
            "body",
            &[m1, Value::String("solidB".to_string()), j2.clone(), j1],
        );
        let mut args = vec![m2, Value::String("solidC".to_string()), j2, world];
        if let Some(p) = pose {
            args.push(p);
        }
        eval_builtin("body", &args)
    }

    /// Assert `m` is the mechanism error Map for a world-parented closing
    /// edge — the same four-key decoration `make_duplicate_solid_error`
    /// produces (`error`, `error_message`, empty-List `error_path1` /
    /// `error_path2`), stamped with this error's own discriminator.
    fn assert_world_parented_closure_error(m: &Value) {
        let map = match m {
            Value::Map(m) => m,
            other => panic!("expected errored Mechanism Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("mechanism".to_string())),
            "the error Map decorates the mechanism in place, keeping kind='mechanism'"
        );
        assert_eq!(
            map.get(&Value::String("error".to_string())),
            Some(&Value::String("world_parented_closure".to_string())),
            "error field should be 'world_parented_closure'"
        );
        match map.get(&Value::String("error_message".to_string())) {
            Some(Value::String(s)) => {
                assert!(!s.is_empty(), "error_message should be non-empty");
                assert!(
                    s.contains("world()"),
                    "error_message must name the world-parented closing edge, got {:?}",
                    s
                );
                assert!(
                    s.contains("free variable"),
                    "error_message must say the loop-closure solver has no free variable \
                     on the closing side, got {:?}",
                    s
                );
            }
            other => panic!("expected error_message String, got {:?}", other),
        }
        assert_eq!(
            map.get(&Value::String("error_path1".to_string())),
            Some(&Value::List(vec![])),
            "error_path1 is an empty List (v0.1 error-Map shape uniformity)"
        );
        assert_eq!(
            map.get(&Value::String("error_path2".to_string())),
            Some(&Value::List(vec![])),
            "error_path2 is an empty List (v0.1 error-Map shape uniformity)"
        );
    }

    /// **Task 7186 review fix 1 (a).** A closing edge parented to `world()`
    /// must be rejected at BUILD time with a loud, actionable error Map.
    ///
    /// Why this is this task's to close: `path_b` is
    /// `[world] ++ walk_to_world(joint_parents, parent)`, and `walk_to_world`
    /// returns an EMPTY vec iff `is_world(parent)` (it breaks before pushing
    /// only for the world sentinel; a non-world parent with no recorded
    /// ancestor still yields `[parent]`). So `path_b == [world]` (len 1) iff
    /// the closing edge's `parent` is the world sentinel. Both
    /// `strip_world_sentinel` impls reject `len < 2`, so
    /// `extract_loop_closure_chains` returns None and snapshot.rs maps that
    /// to `Value::Undef` for the WHOLE mechanism, with no diagnostic. Before
    /// step-2 dropped the closing joint from `path_b` this was unreachable
    /// (`path_b` always ended with `at`), so it is a failure mode this task
    /// introduced.
    ///
    /// Measured on this exact fixture before the fix: `path_a.len() == 3`,
    /// `path_b.len() == 1` (`[world]` only), `snapshot(...) == Undef`.
    ///
    /// Why rejection and not repair: with `parent == world`, chain_b holds
    /// NO joints, so `free_b == []` for any bindings — see
    /// `world_parented_closing_edge_with_pose_is_rejected` for the measured
    /// silent-wrong-answer that softer remedies produce.
    #[test]
    fn world_parented_closing_edge_is_rejected() {
        let errored = world_parented_closure_fixture(None);
        assert_world_parented_closure_error(&errored);

        // Propagation: the error must not be swallowed downstream. `body()`
        // returns an errored Mechanism Map verbatim (its generic `error`-key
        // short-circuit), and `snapshot()` on an errored mechanism is Undef
        // rather than a partial Snapshot of the pre-error bodies.
        let j3 = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let propagated = eval_builtin(
            "body",
            &[
                errored.clone(),
                Value::String("solidD".to_string()),
                j3,
                eval_builtin("world", &[]),
            ],
        );
        assert_eq!(
            propagated, errored,
            "a subsequent body() call on the errored mechanism returns it verbatim"
        );
        assert!(
            eval_builtin("snapshot", &[errored.clone(), Value::List(vec![])]).is_undef(),
            "snapshot() of the errored mechanism must be Undef, not a normal Snapshot Map"
        );
        assert!(
            eval_builtin(
                "body_id_of",
                &[errored, Value::String("solidA".to_string())],
            )
            .is_undef(),
            "body_id_of() on the errored mechanism must be Undef"
        );
    }

    /// **Task 7186 review fix 1 (b).** The 5-arg closing form of the same
    /// shape must be rejected identically. This is the case that is strictly
    /// WORSE than the 4-arg `Undef`: a non-identity `pose` appends a
    /// synthetic 0-DOF rigid link, so `path_b == [world, fixed]` (len 2)
    /// clears `strip_world_sentinel` and the mechanism reports a normal,
    /// plausible-looking Snapshot carrying an unsatisfied closure.
    ///
    /// Measured on this exact fixture (`pose = translate(0.2m, 0, 0)`,
    /// `j1` bound to 0.5 m) before the fix: `path_b.len() == 2`, snapshot
    /// NOT Undef, `free_values == [[]]`, bodies at 0.5 / 1.5 / 1.7 m, and a
    /// direct residual probe gives `T_a = (1.5, 0, 0)`, `T_b = (0.2, 0, 0)`,
    /// residual twist `[0, 0, 0, -1.3, 0, 0]` — a 1.3 m unsatisfied closure
    /// returned as a normal Snapshot Map with no diagnostic. That is the
    /// silent-wrong-answer this rejection closes.
    #[test]
    fn world_parented_closing_edge_with_pose_is_rejected() {
        let pose = Value::Transform {
            rotation: Box::new(Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(Value::Vector(vec![
                Value::length(0.2),
                Value::length(0.0),
                Value::length(0.0),
            ])),
        };
        let errored = world_parented_closure_fixture(Some(pose));
        assert_world_parented_closure_error(&errored);
        assert!(
            eval_builtin("snapshot", &[errored, Value::List(vec![])]).is_undef(),
            "snapshot() of the errored mechanism must be Undef — not the 0.5/1.5/1.7 m \
             bodies carrying a 1.3 m unsatisfied closure"
        );
    }

    /// **Task 7186 review fix 1 (c).** The negative control that pins the
    /// rejection's exact boundary: a closing edge whose `parent` is a REAL
    /// joint with no recorded ancestor still records a loop closure and is
    /// NOT rejected. `walk_to_world` pushes such a parent (it stops only at
    /// the world sentinel), so `path_b == [world, parent]` — len 2, which
    /// clears `strip_world_sentinel`, and chain_b carries one real joint so
    /// `free_b` is non-empty and the closure is solvable.
    ///
    /// This must PASS both before and after the step-10 guard: it is the
    /// assertion that stops the BUILDER's `is_world(parent)` rejection from
    /// being widened into "parent has no recorded ancestor", which would
    /// over-reject.
    ///
    /// snapshot.rs's FK base-frame arm keeps the two shapes SEPARATE, and
    /// deliberately so: a world parent contributes a bare identity, while an
    /// unregistered real joint is rooted at the identity and then composes
    /// its OWN `transform_at` — matching `chain_transform`, whose chain_b
    /// terminal for `[parent]` is `T(parent)`, not `I`. Collapsing them into
    /// one arm is what shipped a 1.1 m mis-placement; see
    /// `snapshot_closing_edge_on_unregistered_parent_rides_that_parent_frame`.
    ///
    /// SCOPE OF THIS TEST: it pins the BUILDER contract (records, is not
    /// rejected) plus liveness of the snapshot. It deliberately does NOT pin
    /// geometry — its `j3` is a revolute whose midpoint rotation makes the
    /// loop infeasible, so the free variable converges to 0 and a base-frame
    /// error is numerically invisible here. The geometry is pinned by the
    /// feasible all-prismatic fixture in snapshot.rs named above; keep the
    /// two in step.
    #[test]
    fn non_world_parented_closing_edge_still_records() {
        let j1 = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j2 = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        // j3 is never used as an `at`, so joint_parents records no ancestor
        // for it — the boundary case one step away from the world sentinel.
        let j3 = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let world = eval_builtin("world", &[]);

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin(
            "body",
            &[
                m0,
                Value::String("solidA".to_string()),
                j1.clone(),
                world.clone(),
            ],
        );
        let m2 = eval_builtin(
            "body",
            &[
                m1,
                Value::String("solidB".to_string()),
                j2.clone(),
                j1.clone(),
            ],
        );
        let m3 = eval_builtin(
            "body",
            &[
                m2,
                Value::String("solidC".to_string()),
                j2.clone(),
                j3.clone(),
            ],
        );

        let map = match &m3 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };
        assert!(
            !map.contains_key(&Value::String("error".to_string())),
            "a closing edge parented to a real joint must NOT be rejected, got {:?}",
            map.get(&Value::String("error".to_string()))
        );
        let (path_a, path_b) = only_closure_paths(&m3);
        assert_eq!(
            path_a,
            Value::List(vec![world.clone(), j1, j2]),
            "path_a is the spanning-tree walk down to the closing joint"
        );
        assert_eq!(
            path_b,
            Value::List(vec![world, j3]),
            "path_b is [world, parent] — len 2, so strip_world_sentinel accepts it"
        );

        // Task 7186 review fix 3: the claim above is "NOT rejected AND
        // solvable", so assert the second half too — a build that records the
        // closure but whose `snapshot()` is `Undef` is the same silent
        // whole-mechanism failure the step-10 guard exists to eliminate,
        // just moved one step past the boundary it pins.
        //
        // LIVENESS ONLY — read "solvable" here as "not Undef", nothing more.
        // This assertion cannot see WHERE the bodies land, which is how review
        // fix 3's 1.1 m mis-placement shipped green past this very test. The
        // placement is pinned separately by
        // `snapshot_closing_edge_on_unregistered_parent_rides_that_parent_frame`
        // (snapshot.rs), on a feasible fixture where it is observable.
        //
        // This is what caught the regression: `walk_fk`'s closing-body arm
        // routed `body.parent` through `joint_world_transform`, whose leading
        // `joint_parents.get(joint)?` returns None for an UNREGISTERED parent
        // such as `j3` — turning the whole snapshot Undef. The residual side
        // roots the same unregistered parent at the identity (`walk_to_world`
        // stops at it, `chain_transform` accumulates from identity), so FK now
        // degrades identically. See the `!joint_parents.contains_key(p)` arm
        // in snapshot.rs.
        assert!(
            !eval_builtin("snapshot", &[m3, Value::List(vec![])]).is_undef(),
            "a recorded, non-rejected closure must still produce a snapshot — FK must root \
             an unregistered closing parent at the identity, exactly as chain_transform does"
        );
    }

    // ── closed-chain detection: joint-graph cycle ────────────────────────

    /// v0.2: `body()` calls whose recorded `(at → parent)` edges introduce a
    /// cycle now record a loop-closure constraint instead of erroring.
    ///
    /// Scenario: `body(m, solid_a, j_a, j_b)` then
    /// `body(m', solid_b, j_b, j_a)`. After call 1, `joint_parents` records
    /// `j_a → j_b`. Call 2 would add `j_b → j_a`, closing the cycle
    /// `j_a → j_b → j_a`.
    ///
    /// Assertions:
    /// - returned Map has NO `error` key
    /// - bodies.len() == 2 (closing body IS appended)
    /// - joint_parents has only `j_a → j_b` (the cycle-closing `j_b → j_a`
    ///   edge is NOT recorded in joint_parents)
    /// - loop_closures has one entry with:
    ///   kind="loop_closure", body_id=Int(1), closing_joint=j_b,
    ///   path_a=[world, j_b]  (walk_to_world({j_a:j_b}, j_b) = [j_b]; world prepended);
    ///   path_b=[world, j_b, j_a, j_b]  (walk_to_world({j_a:j_b}, j_a)=[j_b,j_a]
    ///   top-down; world prepended; closing edge at=j_b appended)
    #[test]
    fn cycle_records_loop_closure_constraint() {
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
        // Call 2: body(m1, solid_b, j_b, j_a) — would close cycle j_a→j_b→j_a.
        // v0.2: must record a loop-closure, NOT error.
        let m2 = eval_builtin("body", &[m1, solid_b, j_b.clone(), j_a.clone()]);

        let map = match m2 {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map after call 2, got {:?}", other),
        };

        // No error key.
        assert!(
            !map.contains_key(&Value::String("error".to_string())),
            "cycle in v0.2 must NOT produce an error key; got error={:?}",
            map.get(&Value::String("error".to_string()))
        );

        // Both bodies are present.
        let bodies = match map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected bodies List, got {:?}", other),
        };
        assert_eq!(
            bodies.len(),
            2,
            "closing body must be appended (bodies.len()==2)"
        );

        // Spanning tree: only j_a → j_b, NOT j_b → j_a.
        let jp = match map.get(&Value::String("joint_parents".to_string())) {
            Some(Value::Map(jp)) => jp,
            other => panic!("expected joint_parents Map, got {:?}", other),
        };
        let world = eval_builtin("world", &[]);
        assert_eq!(
            jp.get(&j_a),
            Some(&j_b),
            "joint_parents[j_a] should be j_b (from call 1)"
        );
        assert!(
            !jp.contains_key(&j_b),
            "cycle-closing edge j_b→j_a must NOT be in joint_parents"
        );
        assert_eq!(jp.len(), 1, "joint_parents should have exactly one entry");

        // loop_closures: one entry.
        let loop_closures = match map.get(&Value::String("loop_closures".to_string())) {
            Some(Value::List(lc)) => lc,
            other => panic!("expected loop_closures List, got {:?}", other),
        };
        assert_eq!(
            loop_closures.len(),
            1,
            "exactly one loop-closure entry expected"
        );

        let lc = match &loop_closures[0] {
            Value::Map(m) => m,
            other => panic!("expected loop_closure Map, got {:?}", other),
        };
        assert_eq!(
            lc.get(&Value::String("kind".to_string())),
            Some(&Value::String("loop_closure".to_string()))
        );
        assert_eq!(
            lc.get(&Value::String("body_id".to_string())),
            Some(&Value::Int(1))
        );
        assert_eq!(
            lc.get(&Value::String("closing_joint".to_string())),
            Some(&j_b),
            "closing_joint should be j_b (the `at` argument of call 2)"
        );
        assert_eq!(
            lc.get(&Value::String("path_a".to_string())),
            Some(&Value::List(vec![world.clone(), j_b.clone()])),
            "path_a should be [world, j_b]"
        );
        assert_eq!(
            lc.get(&Value::String("path_b".to_string())),
            Some(&Value::List(vec![
                world.clone(),
                j_b.clone(),
                j_a.clone(),
                j_b.clone()
            ])),
            "path_b should be [world, j_b, j_a, j_b] (closing edge at=j_b appended)"
        );
    }

    // ── closed-chain detection: self-loop ────────────────────────────────

    /// v0.2: `body()` with the same joint as both `at` and `parent` records
    /// a loop-closure constraint instead of erroring.
    ///
    /// Self-loops are subsumed by `cycle_introduced` (returns true on the
    /// very first iteration when `current == at`). Regression-prevention
    /// intent: an unsuspecting refactor of `cycle_introduced` that started
    /// the comparison only after one ancestor hop would silently pass the
    /// self-loop through.
    ///
    /// Pin shapes (fresh mechanism, `joint_parents` empty):
    ///   path_a = [world, j]    — walk_to_world({}, j) yields [j]; world prepended.
    ///   path_b = [world, j, j] — same walk yields [j]; world prepended;
    ///                            closing edge at=j appended (j appears twice).
    #[test]
    fn self_loop_records_loop_closure_constraint() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solid".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        // Pass j as both `at` (args[2]) and `parent` (args[3]).
        let result = eval_builtin("body", &[m0, solid, j.clone(), j.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Mechanism Map, got {:?}", other),
        };

        // No error key.
        assert!(
            !map.contains_key(&Value::String("error".to_string())),
            "self-loop in v0.2 must NOT produce an error key; got error={:?}",
            map.get(&Value::String("error".to_string()))
        );

        // One body appended.
        let bodies = match map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected bodies List, got {:?}", other),
        };
        assert_eq!(bodies.len(), 1, "self-loop body must be appended");

        // joint_parents is empty (the self-loop edge is NOT inserted).
        let jp = match map.get(&Value::String("joint_parents".to_string())) {
            Some(Value::Map(jp)) => jp,
            other => panic!("expected joint_parents Map, got {:?}", other),
        };
        assert!(
            jp.is_empty(),
            "joint_parents should be empty for a self-loop"
        );

        // loop_closures: one entry.
        let loop_closures = match map.get(&Value::String("loop_closures".to_string())) {
            Some(Value::List(lc)) => lc,
            other => panic!("expected loop_closures List, got {:?}", other),
        };
        assert_eq!(
            loop_closures.len(),
            1,
            "exactly one loop-closure entry expected"
        );

        let lc = match &loop_closures[0] {
            Value::Map(m) => m,
            other => panic!("expected loop_closure Map, got {:?}", other),
        };
        let world = eval_builtin("world", &[]);
        assert_eq!(
            lc.get(&Value::String("kind".to_string())),
            Some(&Value::String("loop_closure".to_string()))
        );
        assert_eq!(
            lc.get(&Value::String("body_id".to_string())),
            Some(&Value::Int(0))
        );
        assert_eq!(
            lc.get(&Value::String("closing_joint".to_string())),
            Some(&j)
        );
        assert_eq!(
            lc.get(&Value::String("path_a".to_string())),
            Some(&Value::List(vec![world.clone(), j.clone()])),
            "path_a = [world, j]"
        );
        assert_eq!(
            lc.get(&Value::String("path_b".to_string())),
            Some(&Value::List(vec![world.clone(), j.clone(), j.clone()])),
            "path_b = [world, j, j] (closing edge at=j appended)"
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
        // Build an errored mechanism via duplicate-solid (same solid
        // value used twice). After the v0.2 closed-chain → loop-closure
        // migration, duplicate_solid remains the error trigger here; the
        // contract under test — errored-Map propagation through subsequent
        // body() calls — is unchanged.
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a.clone(), j_a]);
        let errored = eval_builtin("body", &[m1, solid_a.clone(), j_b]);
        // Sanity: the setup actually produced an errored mechanism.
        match &errored {
            Value::Map(m) => {
                assert_eq!(
                    m.get(&Value::String("error".to_string())),
                    Some(&Value::String("duplicate_solid".to_string())),
                    "setup precondition: errored mechanism has error='duplicate_solid'"
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
    /// produce an errored Mechanism Map with `error="duplicate_solid"`,
    /// a non-empty `error_message`, and empty-List `error_path1`/`error_path2`
    /// fields (shape-uniformity with the v0.1 error-Map convention).
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
        assert_eq!(
            map.get(&Value::String("error_path1".to_string())),
            Some(&Value::List(vec![])),
            "error_path1 should be an empty List for duplicate_solid"
        );
        assert_eq!(
            map.get(&Value::String("error_path2".to_string())),
            Some(&Value::List(vec![])),
            "error_path2 should be an empty List for duplicate_solid"
        );
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
            eval_builtin(
                "body_id_of",
                &[world, Value::String("anything".to_string())]
            )
            .is_undef(),
            "non-mechanism Map yields Undef"
        );

        // Wrong arity → Undef.
        assert!(eval_builtin("body_id_of", &[]).is_undef());
        assert!(eval_builtin("body_id_of", std::slice::from_ref(&m2)).is_undef());
        assert!(
            eval_builtin(
                "body_id_of",
                &[m2, Value::String("a".to_string()), Value::Int(1)]
            )
            .is_undef()
        );
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
        // Build an errored mechanism via duplicate-solid. After the v0.2
        // closed-chain → loop-closure migration, duplicate_solid remains
        // the error trigger here. solid_a IS present in the pre-error
        // bodies list (recorded by the first body() call), and the second
        // call with the same solid surfaces error="duplicate_solid".
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a.clone(), j_a]);
        let errored = eval_builtin("body", &[m1, solid_a.clone(), j_b]);
        // Sanity: setup actually produced an errored mechanism with
        // solid_a still in the pre-error bodies list.
        match &errored {
            Value::Map(m) => {
                assert_eq!(
                    m.get(&Value::String("error".to_string())),
                    Some(&Value::String("duplicate_solid".to_string())),
                    "setup precondition: errored mechanism has error='duplicate_solid'"
                );
            }
            other => panic!("expected errored Mechanism Map, got {:?}", other),
        }

        // body_id_of on the errored mechanism must yield Undef even
        // though solid_a IS present in the pre-error bodies list (the
        // "duplicate_solid" error decorates the mechanism but preserves
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

    // ── loop_closures field type-guard (Part B) ───────────────────────────

    /// A Mechanism Map with a present-but-wrong-typed `loop_closures` field
    /// must cause `body()` to return `Value::Undef`, matching the type-guard
    /// contract of the sibling fields `bodies`, `joint_parents`, and `next_id`
    /// (mechanism.rs:435-446).
    ///
    /// This test hand-constructs a structurally valid Mechanism Map except that
    /// `loop_closures` is bound to `Value::Int(0)` instead of a `Value::List`.
    /// The v0.2 builder (`make_empty_mechanism`) always emits a `Value::List`,
    /// so this shape only arises from external/test callers or a corrupted Map.
    ///
    /// Before Part B the wildcard `_ => Vec::new()` branch in `append_body`
    /// silently coerced the wrong-typed field to an empty Vec, causing `body()`
    /// to proceed and return a Mechanism Map with one body instead of Undef.
    /// This test regression-proofs against that silent-coercion footgun being
    /// re-introduced.
    #[test]
    fn append_body_wrong_typed_loop_closures_returns_undef() {
        // Build a valid joint using the standard test helpers.
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("solid".to_string());

        // Build the base mechanism via the public builder so the fixture
        // automatically tracks any future additions to the canonical Mechanism
        // Map shape; override only the one field under test.
        let m0 = eval_builtin("mechanism", &[]);
        let mut map = match m0 {
            Value::Map(m) => m,
            _ => panic!("mechanism() must return a Value::Map"),
        };
        // Wrong type: Int instead of List.  A present-but-wrong-typed
        // loop_closures field simulates a corrupt Mechanism Map.
        map.insert(Value::String("loop_closures".to_string()), Value::Int(0));
        let mech = Value::Map(map);

        let result = eval_builtin("body", &[mech, solid, j]);
        assert!(
            result.is_undef(),
            "body() on a Mechanism Map with wrong-typed loop_closures must return \
             Value::Undef (present-but-wrong-type is a corrupt mechanism), got {:?}",
            result
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
