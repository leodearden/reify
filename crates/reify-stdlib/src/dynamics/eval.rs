//! Eval-side `Value`↔core dispatch for the RBD-η stdlib dynamics entry points
//! (`docs/prds/v0_3/rigid-body-dynamics.md` §5 / task RBD-η, Phase 4).
//!
//! This module is the `Value`-marshalling half of the dynamics surface: it
//! extracts `Value`s into the pure-`f64` `RneaLink` inputs consumed by the
//! [`crate::dynamics::rnea`] open-chain RNEA core, invokes it, and reshapes the
//! result `τ` back into registry-free `JointForce` / `MotionTrajectory`
//! `Value::StructureInstance`s.
//!
//! **Open-chain only — closed-chain routing is deferred to task 4146.** The
//! open-chain path (`inverse_dynamics_at_snapshot`, the trajectory variant
//! `inverse_dynamics`, and `ramp_profile`) is complete and GREEN. Routing
//! closed mechanisms (marshalling `M` / `τ_open` / the loop Jacobian `A` into
//! [`crate::dynamics::closed_chain`]) is NOT wired here: the
//! `Value`→`closed_chain` bridge needs a per-body-DOF incidence map and an
//! effective-rank reduction of `A` that this task (RBD-η, 3836) did not design,
//! and its only real correctness check (the `closed_4bar_idyn.ri` virtual-work
//! e2e) was deferred — so both are carried to task 4146 rather than landed
//! behind a finiteness-only test. Closed mechanisms are not yet routed.
//!
//! **Why this lives in `reify-stdlib`, not `reify-eval/src/dynamics_ops.rs`.**
//! `joints::motion_subspace_columns` is `pub(crate)` and the RNEA / closed-chain
//! cores are crate-internal, so the marshalling MUST be in-crate to reach them.
//! `inverse_dynamics` needs no `GeometryKernel` (mass comes from `body.solid`),
//! so the engine-post-process path used by `body_mass_props` is unnecessary.
//! Registered through `lib.rs::eval_builtin`, dispatched via the gcode_import
//! delegate-to-intrinsic pattern: the `dynamics.ri` surface fns delegate to
//! `*_lower` intrinsics with no `.ri` declaration, which resolve
//! `NoUserFunctions → FunctionCall → eval_builtin → eval_dynamics`.
//!
//! The recognised intrinsic names are:
//!   * `ramp_profile_lower`                  — trajectory generator (step-2)
//!   * `inverse_dynamics_at_snapshot_lower`  — open-chain snapshot RNEA (step-6)
//!   * `inverse_dynamics_lower`              — trajectory variant (step-8;
//!     open-chain only — closed-chain routing deferred to task 4146)

use crate::dynamics::rnea::{default_gravity, inverse_dynamics_open_chain, RneaLink};
use crate::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};
use crate::joints::motion_subspace_columns;
use crate::mechanism::is_world;
use reify_core::dimension::DimensionVector;
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
use std::collections::BTreeMap;

/// Sentinel `StructureTypeId` for engine-assembled (registry-free) instances.
/// The `eval_builtin` path has no `StructureRegistry`, so result instances are
/// minted with the nominal `type_name` as the source of truth for downstream
/// hooks — mirrors `dynamics_ops::assemble_mass_properties` /
/// `modal_ops::degenerate_modal_result`.
const REGISTRY_FREE_TYPE_ID: StructureTypeId = StructureTypeId(u32::MAX);

/// Extract an `f64` from a numeric value cell (`Int` / `Real` / dimensioned
/// `Scalar`). Mirrors `dynamics_ops::cell_f64`; non-numeric cells yield `None`.
fn cell_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Real(r) => Some(*r),
        Value::Scalar { si_value, .. } => Some(*si_value),
        _ => None,
    }
}

/// Mint a registry-free `Value::StructureInstance` with the given nominal
/// `type_name` and field map. Single assembler for every result type this
/// module produces (`MotionTrajectory`, `TrajectorySample`, the `JointForce`
/// family), mirroring `dynamics_ops::assemble_mass_properties`.
fn mint_instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
    let fields: PersistentMap<String, Value> = fields.into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE_TYPE_ID,
        type_name: type_name.to_string(),
        version: 1,
        fields,
    }))
}

/// Evaluate an RBD-η dynamics intrinsic by name.
///
/// Returns `Some(Value)` for the dynamics `*_lower` intrinsics this module owns
/// (including `Some(Value::Undef)` on malformed input, matching the
/// mechanism/snapshot/body eval_builtin convention), or `None` for any other
/// name so that `eval_builtin` can fall through to the next module.
pub(crate) fn eval_dynamics(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "ramp_profile_lower" => Some(eval_ramp_profile(args)),
        "inverse_dynamics_at_snapshot_lower" => {
            Some(eval_inverse_dynamics_at_snapshot(args))
        }
        "inverse_dynamics_lower" => Some(eval_inverse_dynamics(args)),
        // Closed-chain routing (assembling the loop Jacobian `A` / `M` /
        // `τ_open` for `dynamics::closed_chain`) is deferred to task 4146;
        // the open-chain path above handles open mechanisms only.
        _ => None,
    }
}

// ── ramp_profile (PRD §4.3) ───────────────────────────────────────────────────

/// Fixed number of equal time intervals in a `ramp_profile` grid (⇒ `N + 1`
/// samples). Even so a sample lands exactly on the peak-velocity instant
/// `t_half`, and both endpoints (`t = 0`, `t = T`) are sampled exactly.
const RAMP_PROFILE_SEGMENTS: usize = 100;

/// `ramp_profile_lower(joint, from, to, max_accel)` — rest-to-rest triangular
/// constant-acceleration trajectory (PRD §4.3; no max-velocity arg ⇒ the
/// degenerate trapezoid). Returns a `MotionTrajectory` of `TrajectorySample`s,
/// or `Value::Undef` on malformed input (non-numeric / non-finite bounds, or a
/// non-positive / non-finite `max_accel`).
fn eval_ramp_profile(args: &[Value]) -> Value {
    if args.len() != 4 {
        return Value::Undef;
    }
    let joint = args[0].clone();
    let from = match cell_f64(&args[1]) {
        Some(x) if x.is_finite() => x,
        _ => return Value::Undef,
    };
    let to = match cell_f64(&args[2]) {
        Some(x) if x.is_finite() => x,
        _ => return Value::Undef,
    };
    let max_accel = match cell_f64(&args[3]) {
        Some(a) if a.is_finite() && a > 0.0 => a,
        _ => return Value::Undef,
    };

    let samples = ramp_profile_samples(from, to, max_accel);
    // NOTE — naming/semantics mismatch: the `MotionTrajectory` structure_def
    // declares `mechanism` as the containing mechanism handle. Here the *driving
    // joint* (not a mechanism) is stored in that field — an accepted v1
    // placeholder because the structure_def does not yet have a dedicated
    // `driving_joint` field.  Future readers: this field holds the `joint` arg
    // passed to `ramp_profile`, NOT a `mechanism()` handle. The
    // `inverse_dynamics` dispatch receives the real mechanism as its own
    // separate first argument and never reads this field.
    mint_instance(
        "MotionTrajectory",
        vec![
            ("mechanism".to_string(), joint),
            ("samples".to_string(), Value::List(samples)),
        ],
    )
}

/// Sample the triangular rest-to-rest profile on the fixed time grid.
///
/// Phase 1 (`0 ≤ t ≤ t_half`): accelerate at `+s·a` from rest;
/// Phase 2 (`t_half < t ≤ T`): decelerate at `−s·a` to rest, where
/// `s = sign(to − from)`, `D = |to − from|`, `T = 2·sqrt(D/a)`,
/// `t_half = T/2`. A zero-displacement move emits a single rest sample at
/// `t = 0`.
fn ramp_profile_samples(from: f64, to: f64, max_accel: f64) -> Vec<Value> {
    let signed = to - from;
    let dist = signed.abs();
    if dist == 0.0 {
        return vec![trajectory_sample(0.0, from, 0.0, 0.0)];
    }
    let s = signed.signum();
    let total_t = 2.0 * (dist / max_accel).sqrt();
    let t_half = total_t / 2.0;
    let v_peak = s * max_accel * t_half;
    let q_half = from + s * 0.5 * dist;

    let mut samples = Vec::with_capacity(RAMP_PROFILE_SEGMENTS + 1);
    for k in 0..=RAMP_PROFILE_SEGMENTS {
        let t = total_t * (k as f64) / (RAMP_PROFILE_SEGMENTS as f64);
        let (q, v, a) = if t <= t_half {
            // Phase 1: accelerate at +s·max_accel from rest.
            (
                from + s * 0.5 * max_accel * t * t,
                s * max_accel * t,
                s * max_accel,
            )
        } else {
            // Phase 2: decelerate at −s·max_accel to rest.
            let tau = t - t_half;
            (
                q_half + v_peak * tau - s * 0.5 * max_accel * tau * tau,
                v_peak - s * max_accel * tau,
                -s * max_accel,
            )
        };
        samples.push(trajectory_sample(t, q, v, a));
    }
    samples
}

/// Assemble a single-joint `TrajectorySample`: `t` is a Time-dimensioned
/// `Scalar`; `values` / `vels` / `accels` are length-1 `List<Real>`
/// (`JointValue` resolves to `Real`).
fn trajectory_sample(t: f64, q: f64, v: f64, a: f64) -> Value {
    mint_instance(
        "TrajectorySample",
        vec![
            (
                "t".to_string(),
                Value::Scalar {
                    si_value: t,
                    dimension: DimensionVector::TIME,
                },
            ),
            ("values".to_string(), Value::List(vec![Value::Real(q)])),
            ("vels".to_string(), Value::List(vec![Value::Real(v)])),
            ("accels".to_string(), Value::List(vec![Value::Real(a)])),
        ],
    )
}

// ── Value↔core marshalling extractors (RBD-η steps 4/6) ───────────────────────
//
// These are consumed by the open-chain dispatch (`snapshot_inverse_dynamics`,
// step-6) and exercised directly by the step-4 unit tests.

/// Extract three SI-unit components from a `Value::Point` / `Value::Vector` /
/// `Value::List` of exactly three numeric cells (dimensions stripped via
/// `si_value`). Returns `None` for any other shape or arity.
fn vec3_from_value(v: &Value) -> Option<[f64; 3]> {
    let comps = match v {
        Value::Point(c) | Value::Vector(c) | Value::List(c) => c,
        _ => return None,
    };
    if comps.len() != 3 {
        return None;
    }
    Some([cell_f64(&comps[0])?, cell_f64(&comps[1])?, cell_f64(&comps[2])?])
}

/// Parse a 3×3 inertia matrix from a `Value::Matrix` (or nested `Value::List` /
/// `Value::Vector`) of numeric cells. Re-spelled locally from
/// `reify_eval::dynamics_psd::inertia_3x3_from_value` (that one lives in another
/// crate). Returns `None` unless the value is exactly 3×3 and all-numeric.
fn inertia_3x3_from_value(v: &Value) -> Option<[[f64; 3]; 3]> {
    fn row3(vals: &[Value]) -> Option<[f64; 3]> {
        if vals.len() != 3 {
            return None;
        }
        Some([cell_f64(&vals[0])?, cell_f64(&vals[1])?, cell_f64(&vals[2])?])
    }
    match v {
        Value::Matrix(rows) => {
            if rows.len() != 3 {
                return None;
            }
            Some([row3(&rows[0])?, row3(&rows[1])?, row3(&rows[2])?])
        }
        Value::List(outer) => {
            if outer.len() != 3 {
                return None;
            }
            let parse_row = |r: &Value| -> Option<[f64; 3]> {
                match r {
                    Value::List(row) | Value::Vector(row) => row3(row),
                    _ => None,
                }
            };
            Some([
                parse_row(&outer[0])?,
                parse_row(&outer[1])?,
                parse_row(&outer[2])?,
            ])
        }
        _ => None,
    }
}

/// Extract `(mass, com, inertia)` from a `MassProperties` `Value::StructureInstance`.
///
/// Accepts the canonical `dynamics_ops::assemble_mass_properties` shape (mass: a
/// Mass-scalar; com: a `Value::Point` of Length-scalars; inertia: a 3×3
/// `Value::Matrix` of `Real`) plus the equivalent list-shaped encodings a
/// user-authored MassProperties may produce. The `com` Length dimension is
/// stripped to SI metres. Returns `None` for any non-MassProperties value or a
/// malformed/absent field.
fn mass_properties_from_value(v: &Value) -> Option<(f64, [f64; 3], [[f64; 3]; 3])> {
    let data = match v {
        Value::StructureInstance(d) if d.type_name == "MassProperties" => d,
        _ => return None,
    };
    let mass = cell_f64(data.fields.get("mass")?)?;
    let com = vec3_from_value(data.fields.get("com")?)?;
    let inertia = inertia_3x3_from_value(data.fields.get("inertia")?)?;
    Some((mass, com, inertia))
}

/// Convert a `Value::Transform { rotation: Orientation, translation: Vector }`
/// into a [`Frame3`]: the `(w, x, y, z)` quaternion verbatim and the translation
/// in SI metres (Length dimension stripped). Returns `None` for a non-Transform,
/// a non-Orientation rotation, or a translation that is not a 3-component
/// numeric vector.
fn frame3_from_transform_value(v: &Value) -> Option<Frame3> {
    let (rotation, translation) = match v {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        _ => return None,
    };
    let quat = match rotation {
        Value::Orientation { w, x, y, z } => [*w, *x, *y, *z],
        _ => return None,
    };
    let trans = vec3_from_value(translation)?;
    Some(Frame3::new(quat, trans))
}

// ── inverse_dynamics_at_snapshot — open-chain RNEA dispatch (RBD-η step-6) ─────

/// `inverse_dynamics_at_snapshot_lower(mechanism, snapshot, q_dot, q_ddot)` —
/// RNEA inverse dynamics at a single FK snapshot (PRD §5.2). Returns a
/// `List<JointForce>` (one per body, in mechanism `bodies` / id order), or
/// `Value::Undef` on any malformed input (matching the mechanism/snapshot/body
/// eval_builtin convention).
fn eval_inverse_dynamics_at_snapshot(args: &[Value]) -> Value {
    if args.len() != 4 {
        return Value::Undef;
    }
    match snapshot_inverse_dynamics(&args[0], &args[1], &args[2], &args[3]) {
        Some(forces) => Value::List(forces),
        None => Value::Undef,
    }
}

/// Shared open-chain snapshot RNEA core (factored so the trajectory variant,
/// step-8, can drive it per sample). Returns the per-body `JointForce` list, or
/// `None` on any structural failure (the caller maps `None` → `Value::Undef`).
///
/// `q_dot` / `q_ddot` are flat `List`s of per-DOF numeric cells in mechanism
/// `bodies` (id) order: each joint consumes `dof_count` consecutive cells (per
/// the joint's motion-subspace column count), so the total length must equal
/// Σ dof. A 1-DOF revolute pendulum takes a length-1 list (`[q̇]`).
fn snapshot_inverse_dynamics(
    mechanism: &Value,
    snapshot: &Value,
    q_dot_arg: &Value,
    q_ddot_arg: &Value,
) -> Option<Vec<Value>> {
    // ── mechanism validation (kind="mechanism", no error) ──
    let mech = match mechanism {
        Value::Map(m) => m,
        _ => return None,
    };
    if map_str(mech, "kind") != Some("mechanism") {
        return None;
    }
    if map_get(mech, "error").is_some() {
        return None;
    }

    // ── closed-chain guard: deferred to task 4146 ──────────────────────────────
    // A mechanism with a non-empty `loop_closures` list is a closed chain.
    // Silently routing it through the open-chain RNEA would produce finite but
    // physically-incorrect torques (the spanning-tree RNEA ignores the loop
    // constraints) — a plausible-but-wrong number rather than the `Undef` the
    // caller would expect for unsupported input. Return `None` (→ `Value::Undef`)
    // so closed mechanisms fail loudly until task 4146 wires the closed-chain path.
    // This guard is placed before the bodies.is_empty() early-return so it fires
    // regardless of body count.
    if let Some(Value::List(lc)) = map_get(mech, "loop_closures")
        && !lc.is_empty()
    {
        return None;
    }

    let bodies = match map_get(mech, "bodies") {
        Some(Value::List(b)) => b,
        _ => return None,
    };
    if bodies.is_empty() {
        // No bodies ⇒ no joints ⇒ empty force list (parallels the empty-snapshot
        // convention; never reached by the pendulum/trajectory fixtures).
        return Some(Vec::new());
    }
    let joint_parents = match map_get(mech, "joint_parents") {
        Some(Value::Map(jp)) => jp,
        _ => return None,
    };

    // ── snapshot validation + id → world_transform index ──
    let snap = match snapshot {
        Value::Map(m) => m,
        _ => return None,
    };
    if map_str(snap, "kind") != Some("snapshot") {
        return None;
    }
    let snap_bodies = match map_get(snap, "bodies") {
        Some(Value::List(b)) => b,
        _ => return None,
    };
    let mut world_tf: BTreeMap<i64, &Value> = BTreeMap::new();
    for sb in snap_bodies {
        let sbm = match sb {
            Value::Map(m) => m,
            _ => return None,
        };
        let id = match map_get(sbm, "id") {
            Some(Value::Int(n)) => *n,
            _ => return None,
        };
        world_tf.insert(id, map_get(sbm, "world_transform")?);
    }

    // ── per-body fields: `at` joint, id, solid ──
    let n = bodies.len();
    let mut at_joints: Vec<&Value> = Vec::with_capacity(n);
    let mut ids: Vec<i64> = Vec::with_capacity(n);
    let mut solids: Vec<&Value> = Vec::with_capacity(n);
    for b in bodies {
        let bm = match b {
            Value::Map(m) => m,
            _ => return None,
        };
        at_joints.push(map_get(bm, "at")?);
        ids.push(match map_get(bm, "id") {
            Some(Value::Int(k)) => *k,
            _ => return None,
        });
        solids.push(map_get(bm, "solid")?);
    }

    // ── parent body index per body (None = world-parented base) ──
    // The spanning tree is read from `joint_parents` (not `body.parent`), per
    // mechanism.rs: for closing edges they disagree, and FK / RNEA must follow
    // the spanning tree. A non-world parent joint that names no known body is a
    // structural error → None.
    let mut parent_idx: Vec<Option<usize>> = Vec::with_capacity(n);
    for &at in &at_joints {
        let p = match joint_parents.get(at) {
            None => None,
            Some(j) if is_world(j) => None,
            Some(j) => Some(at_joints.iter().position(|aj| *aj == j)?),
        };
        parent_idx.push(p);
    }

    // ── topological order (parent before child) + inverse permutation ──
    let ordered = topo_order(&parent_idx)?;
    let mut pos = vec![0usize; n];
    for (k, &bi) in ordered.iter().enumerate() {
        pos[bi] = k;
    }

    // ── per-body motion subspaces (drive DOF counts + the τ reshape) ──
    let mut subspaces: Vec<Vec<SpatialVector6>> = Vec::with_capacity(n);
    for &at in &at_joints {
        subspaces.push(motion_subspace_columns(at)?);
    }
    let dof_counts: Vec<usize> = subspaces.iter().map(|s| s.len()).collect();

    // ── slice q̇ / q̈ (flat per-DOF, bodies order) ──
    let q_dot = slice_generalized(q_dot_arg, &dof_counts)?;
    let q_ddot = slice_generalized(q_ddot_arg, &dof_counts)?;

    // ── build RneaLinks in topological order ──
    let mut links: Vec<RneaLink> = Vec::with_capacity(n);
    for &bi in &ordered {
        let (mass, com, inertia_about_com) = mass_properties_from_value(solids[bi])?;
        let child_frame = frame3_from_transform_value(world_tf.get(&ids[bi])?)?;
        let xc = SpatialTransform6::from_frame3(&child_frame);
        // X_{p→i}: base link is ᶜXᵂ = from_frame3(world_transform) (pins the
        // single_pendulum convention); a non-base link is the relative
        // parent→child coordinate transform ᶜXᴾ = ᶜXᵂ · ᵂXᴾ =
        // Xc · Xp⁻¹ (composing full poses sidesteps the rot·xlt offset footgun
        // documented on `RneaLink::parent_to_child`).
        let parent_to_child = match parent_idx[bi] {
            None => xc,
            Some(pb) => {
                let parent_frame = frame3_from_transform_value(world_tf.get(&ids[pb])?)?;
                xc.compose(&SpatialTransform6::from_frame3(&parent_frame).inverse())
            }
        };
        links.push(RneaLink {
            parent: parent_idx[bi].map(|pb| pos[pb]),
            parent_to_child,
            subspace: subspaces[bi].clone(),
            mass,
            com,
            inertia_about_com,
            q_dot: q_dot[bi].clone(),
            q_ddot: q_ddot[bi].clone(),
        });
    }

    // ── RNEA + reshape τ → List<JointForce> (bodies/id order) ──
    let tau = inverse_dynamics_open_chain(&links, default_gravity());
    let mut forces: Vec<Value> = Vec::with_capacity(n);
    for i in 0..n {
        let kind = joint_kind(at_joints[i])?;
        let value = joint_force_value(kind, &tau[pos[i]])?;
        forces.push(make_joint_force(ids[i], value));
    }
    Some(forces)
}

/// Read a string-keyed field from a `Value::Map`'s inner `BTreeMap`.
fn map_get<'a>(map: &'a BTreeMap<Value, Value>, key: &str) -> Option<&'a Value> {
    map.get(&Value::String(key.to_string()))
}

/// Read a string-valued field (e.g. the `kind` discriminant) from a `Value::Map`.
fn map_str<'a>(map: &'a BTreeMap<Value, Value>, key: &str) -> Option<&'a str> {
    match map_get(map, key) {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// The `kind` discriminant of a joint `Value::Map` (e.g. `"revolute"`).
fn joint_kind(joint: &Value) -> Option<&str> {
    match joint {
        Value::Map(m) => map_str(m, "kind"),
        _ => None,
    }
}

/// Kahn-style topological sort of body indices given each body's parent index
/// (`None` = world-parented base). Returns indices in parent-before-child order,
/// or `None` if the parent graph is cyclic (defence-in-depth — `mechanism::body`
/// rejects every spanning-tree cycle, recording closing edges as loop closures).
fn topo_order(parent: &[Option<usize>]) -> Option<Vec<usize>> {
    let n = parent.len();
    let mut emitted = vec![false; n];
    let mut ordered = Vec::with_capacity(n);
    while ordered.len() < n {
        let mut progressed = false;
        for i in 0..n {
            if emitted[i] {
                continue;
            }
            let ready = match parent[i] {
                None => true,
                Some(p) => emitted[p],
            };
            if ready {
                emitted[i] = true;
                ordered.push(i);
                progressed = true;
            }
        }
        if !progressed {
            return None; // cycle — unreachable for a builder-produced mechanism
        }
    }
    Some(ordered)
}

/// Slice a flat `List` of per-DOF numeric cells into one `Vec<f64>` per body,
/// consuming `dof_counts[i]` consecutive cells for body `i` (in `bodies` order).
/// Returns `None` for a non-`List` arg, a non-numeric cell, or a length mismatch
/// against `Σ dof_counts`.
fn slice_generalized(arg: &Value, dof_counts: &[usize]) -> Option<Vec<Vec<f64>>> {
    let flat = match arg {
        Value::List(items) => items,
        _ => return None,
    };
    let total: usize = dof_counts.iter().sum();
    if flat.len() != total {
        return None;
    }
    let mut out = Vec::with_capacity(dof_counts.len());
    let mut cursor = 0;
    for &dof in dof_counts {
        let mut row = Vec::with_capacity(dof);
        for _ in 0..dof {
            row.push(cell_f64(&flat[cursor])?);
            cursor += 1;
        }
        out.push(row);
    }
    Some(out)
}

/// Reshape a joint's generalized-force vector τ into the kind-specific
/// `JointForceValue` variant (PRD §4.2): revolute → `ScalarTorque`, prismatic →
/// `ScalarForce`, cylindrical → `CylForce`, planar → `PlanarForce`, spherical →
/// `SphereForce`, fixed → `ZeroForce`. Returns `None` on an unknown kind or a
/// τ-arity mismatch against the kind's DOF count.
fn joint_force_value(kind: &str, tau: &[f64]) -> Option<Value> {
    let components = |t: &[f64]| Value::List(t.iter().map(|&x| Value::Real(x)).collect());
    let scalar = |type_name: &str, t: &[f64]| -> Option<Value> {
        if t.len() != 1 {
            return None;
        }
        Some(mint_instance(
            type_name,
            vec![("magnitude".to_string(), Value::Real(t[0]))],
        ))
    };
    let multi = |type_name: &str, t: &[f64], dof: usize| -> Option<Value> {
        if t.len() != dof {
            return None;
        }
        Some(mint_instance(
            type_name,
            vec![("components".to_string(), components(t))],
        ))
    };
    match kind {
        "revolute" => scalar("ScalarTorque", tau),
        "prismatic" => scalar("ScalarForce", tau),
        "cylindrical" => multi("CylForce", tau, 2),
        "planar" => multi("PlanarForce", tau, 3),
        "spherical" => multi("SphereForce", tau, 3),
        "fixed" => {
            if tau.is_empty() {
                Some(mint_instance("ZeroForce", vec![]))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Mint a `JointForce` instance: `joint_id` is the body id (a `Real` per the
/// structure_def placeholder), `value` the kind-specific `JointForceValue`.
fn make_joint_force(joint_id: i64, value: Value) -> Value {
    mint_instance(
        "JointForce",
        vec![
            ("joint_id".to_string(), Value::Real(joint_id as f64)),
            ("value".to_string(), value),
        ],
    )
}

// ── inverse_dynamics — trajectory variant (RBD-η step-8) ──────────────────────

/// `inverse_dynamics_lower(mechanism, trajectory)` — RNEA inverse dynamics over
/// a whole `MotionTrajectory` (PRD §5.2). For each `TrajectorySample` it bakes
/// the sample's joint positions into an FK snapshot, then drives the shared
/// open-chain core ([`snapshot_inverse_dynamics`]) with the sample's velocities
/// / accelerations. Returns a `List<List<JointForce>>` parallel to
/// `trajectory.samples`, or `Value::Undef` on any malformed input.
fn eval_inverse_dynamics(args: &[Value]) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    let mechanism = &args[0];
    let samples = match trajectory_samples(&args[1]) {
        Some(s) => s,
        None => return Value::Undef,
    };
    let mut out = Vec::with_capacity(samples.len());
    for sample in samples {
        let (values, vels, accels) = match sample_fields(sample) {
            Some(t) => t,
            None => return Value::Undef,
        };
        // Bake the sample's joint positions into an FK snapshot, then run the
        // shared snapshot core with the sample's q̇ / q̈. Mass properties are
        // re-read per sample from `body.solid` inside the core; they are
        // trajectory-invariant, so this is redundant work the small fixtures
        // don't notice — a future optimisation can hoist the MassProperties
        // extraction across samples.
        let snapshot = match snapshot_for_sample(mechanism, values) {
            Some(s) => s,
            None => return Value::Undef,
        };
        let forces = match snapshot_inverse_dynamics(mechanism, &snapshot, vels, accels) {
            Some(f) => f,
            None => return Value::Undef,
        };
        out.push(Value::List(forces));
    }
    Value::List(out)
}

/// Read the `samples` list of a `MotionTrajectory` `Value::StructureInstance`.
fn trajectory_samples(traj: &Value) -> Option<&[Value]> {
    match instance_field(traj, "MotionTrajectory", "samples")? {
        Value::List(s) => Some(s.as_slice()),
        _ => None,
    }
}

/// Read a `TrajectorySample`'s `(values, vels, accels)` fields. `values` is a
/// slice (one joint position per body, consumed to build the snapshot); `vels` /
/// `accels` are the raw `List` `Value`s the snapshot core slices flat-per-DOF.
fn sample_fields(sample: &Value) -> Option<(&[Value], &Value, &Value)> {
    let values = match instance_field(sample, "TrajectorySample", "values")? {
        Value::List(v) => v.as_slice(),
        _ => return None,
    };
    let vels = instance_field(sample, "TrajectorySample", "vels")?;
    let accels = instance_field(sample, "TrajectorySample", "accels")?;
    Some((values, vels, accels))
}

/// Read a named field from a `Value::StructureInstance`, gated on `type_name`.
fn instance_field<'a>(v: &'a Value, type_name: &str, member: &str) -> Option<&'a Value> {
    match v {
        Value::StructureInstance(d) if d.type_name == type_name => d.fields.get(member),
        _ => None,
    }
}

/// Build an FK snapshot for one trajectory sample: bind each mechanism body's
/// `at` joint to the corresponding `values` entry (one joint position per body,
/// in `bodies` order), then call the `snapshot` builder. Returns `None` on a
/// values/bodies length mismatch or an FK failure (e.g. an unbindable value).
///
/// **Single-DOF-per-body position assumption.** `trajectory.samples[k].values`
/// must have exactly one entry per mechanism body (in `bodies` / id order). This
/// matches the `values.len() == bodies.len()` check below. For multi-DOF joints
/// (cylindrical DOF=2, planar/spherical DOF=3) the single-position encoding is
/// inconsistent with the flat-per-DOF `vels`/`accels` slicing used by
/// [`slice_generalized`]: a trajectory with such joints that supplies one
/// velocity/acceleration value per body would fail the DOF-count check and
/// silently return `Value::Undef`. Multi-DOF mechanisms in trajectories are
/// unsupported by this function; the open-chain-only caveat in
/// [`eval_inverse_dynamics`] applies here too.
fn snapshot_for_sample(mechanism: &Value, values: &[Value]) -> Option<Value> {
    let mech = match mechanism {
        Value::Map(m) => m,
        _ => return None,
    };
    let bodies = match map_get(mech, "bodies") {
        Some(Value::List(b)) => b,
        _ => return None,
    };
    if values.len() != bodies.len() {
        return None;
    }
    let mut bindings = Vec::with_capacity(bodies.len());
    for (body, value) in bodies.iter().zip(values) {
        let bm = match body {
            Value::Map(m) => m,
            _ => return None,
        };
        let at = map_get(bm, "at")?;
        bindings.push(crate::eval_builtin("bind", &[at.clone(), value.clone()]));
    }
    let snapshot = crate::eval_builtin("snapshot", &[mechanism.clone(), Value::List(bindings)]);
    if snapshot.is_undef() {
        return None;
    }
    Some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamics::spatial::Frame3;

    /// Build a canonical `MassProperties` `Value::StructureInstance` matching
    /// `dynamics_ops::assemble_mass_properties`'s shape: `mass` a Mass-scalar,
    /// `com` a `Value::Point` of Length-scalars, `inertia` a 3×3 `Value::Matrix`
    /// of `Real`, `origin` a `Real`.
    fn mass_properties_fixture(
        mass: f64,
        com: [f64; 3],
        inertia: [[f64; 3]; 3],
    ) -> Value {
        let com_point = Value::Point(com.iter().map(|&c| Value::length(c)).collect());
        let inertia_matrix = Value::Matrix(
            inertia
                .iter()
                .map(|row| row.iter().map(|&x| Value::Real(x)).collect())
                .collect(),
        );
        mint_instance(
            "MassProperties",
            vec![
                (
                    "mass".to_string(),
                    Value::Scalar {
                        si_value: mass,
                        dimension: DimensionVector::MASS,
                    },
                ),
                ("com".to_string(), com_point),
                ("inertia".to_string(), inertia_matrix),
                ("origin".to_string(), Value::Real(0.0)),
            ],
        )
    }

    /// Build a `Value::Transform` from a `(w, x, y, z)` quaternion and a metres
    /// translation (Length-scalar components), mirroring the FK `world_transform`
    /// shape that `snapshot()` produces.
    fn transform_fixture(quat: [f64; 4], translation: [f64; 3]) -> Value {
        Value::Transform {
            rotation: Box::new(Value::Orientation {
                w: quat[0],
                x: quat[1],
                y: quat[2],
                z: quat[3],
            }),
            translation: Box::new(Value::Vector(
                translation.iter().map(|&t| Value::length(t)).collect(),
            )),
        }
    }

    // ── step-3 RED: mass_properties_from_value ─────────────────────────────────

    #[test]
    fn mass_properties_from_value_extracts_mass_com_inertia() {
        let inertia = [
            [0.10, 0.01, 0.02],
            [0.03, 0.20, 0.04],
            [0.05, 0.06, 0.30],
        ];
        let mp = mass_properties_fixture(1.0, [0.0, 0.0, -0.1], inertia);

        let (mass, com, got_inertia) = mass_properties_from_value(&mp)
            .expect("a well-formed MassProperties must parse");
        assert!((mass - 1.0).abs() < 1e-12, "mass");
        assert!((com[0]).abs() < 1e-12 && (com[1]).abs() < 1e-12, "com x/y");
        assert!((com[2] - (-0.1)).abs() < 1e-12, "com z");
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (got_inertia[r][c] - inertia[r][c]).abs() < 1e-12,
                    "inertia[{r}][{c}]"
                );
            }
        }
    }

    #[test]
    fn mass_properties_from_value_rejects_non_mass_properties() {
        // A plain numeric cell is not a MassProperties.
        assert!(mass_properties_from_value(&Value::Real(1.0)).is_none());
        // A StructureInstance with a different type_name is rejected.
        let other = mint_instance("Block", vec![("name".to_string(), Value::Real(0.0))]);
        assert!(mass_properties_from_value(&other).is_none());
    }

    // ── step-3 RED: frame3_from_transform_value ────────────────────────────────

    #[test]
    fn frame3_from_transform_value_identity() {
        let identity = transform_fixture([1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        let f = frame3_from_transform_value(&identity).expect("identity Transform must parse");
        assert_eq!(f, Frame3::new([1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0]));
    }

    #[test]
    fn frame3_from_transform_value_matches_quaternion_and_translation() {
        let quat = [0.9659258262890683, 0.0, 0.25881904510252074, 0.0]; // 30° about +y
        let trans = [0.1, -0.2, 0.3];
        let f = frame3_from_transform_value(&transform_fixture(quat, trans))
            .expect("a well-formed Transform must parse");
        for (i, (got, want)) in f.rotation().iter().zip(quat.iter()).enumerate() {
            assert!((got - want).abs() < 1e-12, "quat[{i}]");
        }
        for (i, (got, want)) in f.translation().iter().zip(trans.iter()).enumerate() {
            assert!((got - want).abs() < 1e-12, "trans[{i}]");
        }
    }

    #[test]
    fn frame3_from_transform_value_rejects_non_transform() {
        assert!(frame3_from_transform_value(&Value::Real(0.0)).is_none());
    }

    /// Extract an `f64` from a numeric value cell (`Int` / `Real` / dimensioned
    /// `Scalar`). Panics on a non-numeric cell (tests want a hard failure).
    fn num(v: &Value) -> f64 {
        match v {
            Value::Int(n) => *n as f64,
            Value::Real(r) => *r,
            Value::Scalar { si_value, .. } => *si_value,
            other => panic!("expected a numeric cell, got {other:?}"),
        }
    }

    /// Pull the named field out of a `StructureInstance`, asserting `type_name`.
    fn field<'a>(v: &'a Value, type_name: &str, member: &str) -> &'a Value {
        match v {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.type_name, type_name,
                    "expected a {type_name} instance, got type_name {}",
                    data.type_name
                );
                data.fields
                    .get(member)
                    .unwrap_or_else(|| panic!("{type_name} missing field `{member}`"))
            }
            other => panic!("expected a {type_name} StructureInstance, got {other:?}"),
        }
    }

    /// Read a length-1 `List<Real>` joint-value cell as a single `f64`.
    fn single(v: &Value) -> f64 {
        match v {
            Value::List(items) => {
                assert_eq!(items.len(), 1, "expected a length-1 joint-value list");
                num(&items[0])
            }
            other => panic!("expected a Value::List, got {other:?}"),
        }
    }

    // ── step-1 RED: ramp_profile triangular sampler ────────────────────────────
    //
    // Rest-to-rest move from=0 → to=1 at max_accel=2 (no vmax arg ⇒ triangular).
    // Closed-form constant-acceleration kinematics:
    //   D = |to − from| = 1,  a = 2
    //   T   = 2·sqrt(D/a)     = 2·sqrt(0.5)  ≈ 1.41421356
    //   t_h = T/2             = sqrt(0.5)    ≈ 0.70710678  (peak-velocity instant)
    //   acc = +a for t < t_h, −a for t > t_h
    // Asserts: q(0)=from with v≈0; q(T)=to with v≈0; t strictly increasing;
    // total duration ≈ T; acceleration sign +a before the midpoint, −a after.
    #[test]
    fn ramp_profile_triangular_rest_to_rest_matches_closed_form() {
        let from = 0.0_f64;
        let to = 1.0_f64;
        let accel = 2.0_f64;
        let result = eval_dynamics(
            "ramp_profile_lower",
            &[
                Value::Real(0.0), // joint handle — stored verbatim, not interpreted
                Value::Real(from),
                Value::Real(to),
                Value::Real(accel),
            ],
        )
        .expect("ramp_profile_lower must be a recognised dynamics intrinsic");

        let samples = match field(&result, "MotionTrajectory", "samples") {
            Value::List(s) => s.clone(),
            other => panic!("MotionTrajectory.samples must be a List, got {other:?}"),
        };
        assert!(
            samples.len() >= 3,
            "expected a multi-sample grid, got {} samples",
            samples.len()
        );

        let d = (to - from).abs();
        let total_t = 2.0 * (d / accel).sqrt();
        let t_half = total_t / 2.0;

        // First sample: q = from, v ≈ 0, t = 0.
        let first = &samples[0];
        assert!((num(field(first, "TrajectorySample", "t"))).abs() < 1e-9, "t0 must be 0");
        assert!(
            (single(field(first, "TrajectorySample", "values")) - from).abs() < 1e-9,
            "q(0) must equal `from`"
        );
        assert!(
            single(field(first, "TrajectorySample", "vels")).abs() < 1e-9,
            "v(0) must be ~0 (rest start)"
        );

        // Last sample: q = to, v ≈ 0, t = T.
        let last = &samples[samples.len() - 1];
        assert!(
            (num(field(last, "TrajectorySample", "t")) - total_t).abs() < 1e-9,
            "total duration must be T = 2·sqrt(D/a)"
        );
        assert!(
            (single(field(last, "TrajectorySample", "values")) - to).abs() < 1e-9,
            "q(T) must equal `to`"
        );
        assert!(
            single(field(last, "TrajectorySample", "vels")).abs() < 1e-9,
            "v(T) must be ~0 (rest end)"
        );

        // Monotonically increasing t + acceleration-sign profile.
        let mut prev_t = f64::NEG_INFINITY;
        for s in &samples {
            let t = num(field(s, "TrajectorySample", "t"));
            assert!(t > prev_t, "t must strictly increase ({t} !> {prev_t})");
            prev_t = t;
            let acc = single(field(s, "TrajectorySample", "accels"));
            if t < t_half - 1e-9 {
                assert!(
                    (acc - accel).abs() < 1e-9,
                    "acceleration before midpoint must be +max_accel, got {acc}"
                );
            } else if t > t_half + 1e-9 {
                assert!(
                    (acc + accel).abs() < 1e-9,
                    "acceleration after midpoint must be −max_accel, got {acc}"
                );
            }
        }
    }

    // ── step-5 RED: open-chain inverse_dynamics_at_snapshot pendulum ───────────
    //
    // A 1 kg point mass at com = [0, 0, −0.1] (100 mm along −z) on a revolute
    // joint about +y, held static at θ = −30°. Expected actuator torque holding
    // it static:
    //     τ = m · g · L · sin(30°) = 1 · 9.81 · 0.1 · 0.5 = 0.4905 N·m
    //
    // This reproduces — through the full Value-marshalling path (mechanism +
    // snapshot builders, then `inverse_dynamics_at_snapshot_lower`) — the exact
    // config validated by `rnea.rs::single_pendulum_static_gravity_torque`
    // (mass=1, com=[0,0,−0.1], inertia=0, revolute +y, θ=−30° ⇒ 0.4905, <1e-6).
    // The snapshot's per-body `world_transform` bakes the −30° orientation
    // (`transform_at(revolute_+y, angle(−π/6))` ⇒ quaternion
    // [cos(π/12), 0, −sin(π/12), 0]) — the same quaternion the validated RNEA
    // test passes to `SpatialTransform6::from_frame3`. With q̇ = q̈ = 0 the
    // velocity-product terms vanish, so only the gravity/inertia/transmission
    // path is exercised; the +0.4905 sign pins the gravity-projection sense
    // (a wrong rotation sense would place the body at +30° ⇒ −0.4905).
    //
    // Fails against the pre-1 stub (`eval_dynamics` returns None for this name).
    #[test]
    fn inverse_dynamics_at_snapshot_single_pendulum_static_gravity() {
        use crate::eval_builtin;
        use std::f64::consts::PI;

        // MassProperties point mass: 1 kg at [0,0,−0.1], zero inertia. Stored
        // verbatim as the body's `solid` (the kernel-free mass-props path).
        let mp = mass_properties_fixture(1.0, [0.0, 0.0, -0.1], [[0.0; 3]; 3]);

        // Revolute about +y. The range only needs to be a bounded ANGLE range
        // (validated at construction); `transform_at` does not clamp the bound
        // value, so a symmetric [−π, π] range admits θ = −30°.
        let axis_y = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(-PI))),
            upper: Some(Box::new(Value::angle(PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("revolute", &[axis_y, range]);

        // mechanism().body(mp, joint) — single body parented to world (3-arg
        // form: default parent = world, identity pose).
        let mech = eval_builtin("mechanism", &[]);
        let mech = eval_builtin("body", &[mech, mp.clone(), joint.clone()]);
        assert!(matches!(mech, Value::Map(_)), "body() must yield a Mechanism Map");

        // snapshot(mech, [bind(joint, −30°)]) bakes θ = −30° into the body's
        // world_transform via the FK walk.
        let theta = -PI / 6.0; // −30°
        let binding = eval_builtin("bind", &[joint.clone(), Value::angle(theta)]);
        let snap = eval_builtin("snapshot", &[mech.clone(), Value::List(vec![binding])]);
        assert!(matches!(snap, Value::Map(_)), "snapshot() must yield a Snapshot Map");

        // Static configuration: one revolute DOF, q̇ = q̈ = 0.
        let q_dot = Value::List(vec![Value::Real(0.0)]);
        let q_ddot = Value::List(vec![Value::Real(0.0)]);

        let result = eval_dynamics(
            "inverse_dynamics_at_snapshot_lower",
            &[mech, snap, q_dot, q_ddot],
        )
        .expect("inverse_dynamics_at_snapshot_lower must be a recognised dynamics intrinsic");

        // Result: List<JointForce> of length 1 (one joint).
        let forces = match &result {
            Value::List(f) => f,
            other => panic!("expected a List<JointForce>, got {other:?}"),
        };
        assert_eq!(forces.len(), 1, "one joint ⇒ one JointForce");

        // revolute ⇒ JointForce { value: ScalarTorque { magnitude } }.
        let value = field(&forces[0], "JointForce", "value");
        let torque = num(field(value, "ScalarTorque", "magnitude"));

        let expected = 0.4905_f64; // m·g·L·sin(30°)
        assert!(
            (torque - expected).abs() < 1e-6,
            "expected {expected} N·m, got {torque}"
        );
    }

    /// Build the single-pendulum mechanism used by the snapshot + trajectory
    /// dispatch tests: a 1 kg point mass at com = [0,0,−0.1] on a revolute joint
    /// about +y (range [−π, π], admitting θ = −30°). Returns the Mechanism Map.
    fn pendulum_mechanism() -> Value {
        use crate::eval_builtin;
        use std::f64::consts::PI;
        let mp = mass_properties_fixture(1.0, [0.0, 0.0, -0.1], [[0.0; 3]; 3]);
        let axis_y = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(-PI))),
            upper: Some(Box::new(Value::angle(PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("revolute", &[axis_y, range]);
        let mech = eval_builtin("mechanism", &[]);
        eval_builtin("body", &[mech, mp, joint])
    }

    // ── step-7 RED: trajectory variant inverse_dynamics ───────────────────────
    //
    // The same single-pendulum mechanism, driven by a 2-sample MotionTrajectory
    // with both samples motionless at θ = −30° (vels = accels = [0]). Each sample
    // must reproduce the static-gravity torque 0.4905 N·m, so the result is a
    // length-2 outer List (parallel to samples), each inner a length-1
    // List<JointForce> whose ScalarTorque magnitude ≈ 0.4905 within 1e-6.
    //
    // Fails against the pre-1 stub (`inverse_dynamics_lower` returns None).
    #[test]
    fn inverse_dynamics_trajectory_static_pendulum_per_sample() {
        let mech = pendulum_mechanism();

        // Two motionless samples, both at θ = −30° (bare-Real radians, the
        // JointValue shape `transform_at` consumes for a revolute joint).
        let theta = -std::f64::consts::PI / 6.0;
        let traj = mint_instance(
            "MotionTrajectory",
            vec![
                // mechanism placeholder (Real per the structure_def); the
                // dispatch uses the explicit mechanism arg, not this field.
                ("mechanism".to_string(), Value::Real(0.0)),
                (
                    "samples".to_string(),
                    Value::List(vec![
                        trajectory_sample(0.0, theta, 0.0, 0.0),
                        trajectory_sample(1.0, theta, 0.0, 0.0),
                    ]),
                ),
            ],
        );

        let result = eval_dynamics("inverse_dynamics_lower", &[mech, traj])
            .expect("inverse_dynamics_lower must be a recognised dynamics intrinsic");

        // Outer List parallel to samples (length 2).
        let per_sample = match &result {
            Value::List(s) => s,
            other => panic!("expected a List<List<JointForce>>, got {other:?}"),
        };
        assert_eq!(per_sample.len(), 2, "one force list per trajectory sample");

        let expected = 0.4905_f64;
        for (i, sample_forces) in per_sample.iter().enumerate() {
            let forces = match sample_forces {
                Value::List(f) => f,
                other => panic!("sample {i}: expected a List<JointForce>, got {other:?}"),
            };
            assert_eq!(forces.len(), 1, "sample {i}: one joint ⇒ one JointForce");
            let value = field(&forces[0], "JointForce", "value");
            let torque = num(field(value, "ScalarTorque", "magnitude"));
            assert!(
                (torque - expected).abs() < 1e-6,
                "sample {i}: expected {expected} N·m, got {torque}"
            );
        }
    }

    // ── Suggestion 1: closed-chain guard ─────────────────────────────────────

    /// `snapshot_inverse_dynamics` must return `Value::Undef` for a mechanism
    /// with a non-empty `loop_closures` list instead of silently computing
    /// physically-incorrect open-chain torques (task 4146 deferred).
    #[test]
    fn snapshot_inverse_dynamics_rejects_closed_mechanism() {
        // Build a minimal mechanism Map that passes kind/error validation but has
        // a non-empty `loop_closures` list (the closed-chain discriminant).
        let mech_map: BTreeMap<Value, Value> = [
            (
                Value::String("kind".to_string()),
                Value::String("mechanism".to_string()),
            ),
            (
                Value::String("bodies".to_string()),
                Value::List(vec![]),
            ),
            (
                Value::String("joint_parents".to_string()),
                Value::Map(BTreeMap::new()),
            ),
            (
                Value::String("loop_closures".to_string()),
                Value::List(vec![Value::Real(1.0)]), // non-empty ⇒ closed chain
            ),
        ]
        .into_iter()
        .collect();
        let closed_mech = Value::Map(mech_map);
        // Snapshot and generalized-velocity/acceleration args won't be reached;
        // the guard fires before snapshot validation.
        let dummy: Value = Value::Map(BTreeMap::new());
        let q = Value::List(vec![]);

        let result = eval_dynamics(
            "inverse_dynamics_at_snapshot_lower",
            &[closed_mech, dummy, q.clone(), q],
        )
        .expect("inverse_dynamics_at_snapshot_lower is a recognised intrinsic");
        assert_eq!(
            result,
            Value::Undef,
            "closed mechanism (non-empty loop_closures) must return Value::Undef until task 4146"
        );
    }

    // ── Suggestion 3: ramp_profile edge branches ──────────────────────────────

    /// Zero-displacement move (`from == to`) must emit exactly one rest sample
    /// at t=0 with q=from, v=0, a=0, rather than an empty grid.
    #[test]
    fn ramp_profile_zero_displacement_emits_single_rest_sample() {
        let q_val = std::f64::consts::PI;
        let result = eval_dynamics(
            "ramp_profile_lower",
            &[
                Value::Real(0.0), // joint handle — stored verbatim
                Value::Real(q_val),
                Value::Real(q_val), // from == to → zero displacement
                Value::Real(1.0),
            ],
        )
        .expect("ramp_profile_lower must be a recognised intrinsic");
        let samples = match field(&result, "MotionTrajectory", "samples") {
            Value::List(s) => s.clone(),
            other => panic!("samples must be a List, got {other:?}"),
        };
        assert_eq!(
            samples.len(),
            1,
            "zero-displacement must emit exactly one rest sample"
        );
        let s = &samples[0];
        assert!(
            num(field(s, "TrajectorySample", "t")).abs() < 1e-12,
            "t must be 0"
        );
        assert!(
            (single(field(s, "TrajectorySample", "values")) - q_val).abs() < 1e-12,
            "q must equal `from`"
        );
        assert!(
            single(field(s, "TrajectorySample", "vels")).abs() < 1e-12,
            "v must be 0"
        );
        assert!(
            single(field(s, "TrajectorySample", "accels")).abs() < 1e-12,
            "a must be 0"
        );
    }

    /// Malformed inputs — non-positive / non-finite `max_accel` and non-finite
    /// bounds — must return `Value::Undef` rather than producing garbage output.
    #[test]
    fn ramp_profile_invalid_inputs_return_undef() {
        // max_accel = 0 → non-positive ⇒ Undef
        let r = eval_dynamics(
            "ramp_profile_lower",
            &[
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(1.0),
                Value::Real(0.0), // zero
            ],
        )
        .expect("recognised intrinsic");
        assert_eq!(r, Value::Undef, "max_accel=0 must return Undef");

        // max_accel negative ⇒ Undef
        let r = eval_dynamics(
            "ramp_profile_lower",
            &[
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(1.0),
                Value::Real(-2.0),
            ],
        )
        .expect("recognised intrinsic");
        assert_eq!(r, Value::Undef, "negative max_accel must return Undef");

        // max_accel NaN ⇒ Undef
        let r = eval_dynamics(
            "ramp_profile_lower",
            &[
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(1.0),
                Value::Real(f64::NAN),
            ],
        )
        .expect("recognised intrinsic");
        assert_eq!(r, Value::Undef, "NaN max_accel must return Undef");

        // `from` is NaN ⇒ Undef
        let r = eval_dynamics(
            "ramp_profile_lower",
            &[
                Value::Real(0.0),
                Value::Real(f64::NAN),
                Value::Real(1.0),
                Value::Real(1.0),
            ],
        )
        .expect("recognised intrinsic");
        assert_eq!(r, Value::Undef, "NaN `from` bound must return Undef");

        // `to` is +∞ ⇒ Undef
        let r = eval_dynamics(
            "ramp_profile_lower",
            &[
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(f64::INFINITY),
                Value::Real(1.0),
            ],
        )
        .expect("recognised intrinsic");
        assert_eq!(r, Value::Undef, "non-finite `to` bound must return Undef");
    }

    // ── Suggestion 4: joint_force_value reshape table ─────────────────────────

    /// All six joint kinds produce the correct `JointForceValue` variant with the
    /// expected type_name and component count. Wrong-arity and unknown kind each
    /// return `None`.
    #[test]
    fn joint_force_value_all_kinds_and_wrong_arity() {
        // revolute → ScalarTorque { magnitude: τ[0] }
        let r = joint_force_value("revolute", &[0.4905]).expect("revolute/1");
        if let Value::StructureInstance(d) = &r {
            assert_eq!(d.type_name, "ScalarTorque", "revolute type_name");
            let mag = cell_f64(d.fields.get("magnitude").unwrap()).unwrap();
            assert!((mag - 0.4905).abs() < 1e-12, "revolute magnitude");
        } else {
            panic!("revolute: expected StructureInstance, got {r:?}");
        }

        // prismatic → ScalarForce { magnitude: τ[0] }
        let r = joint_force_value("prismatic", &[12.5]).expect("prismatic/1");
        if let Value::StructureInstance(d) = &r {
            assert_eq!(d.type_name, "ScalarForce", "prismatic type_name");
            let mag = cell_f64(d.fields.get("magnitude").unwrap()).unwrap();
            assert!((mag - 12.5).abs() < 1e-12, "prismatic magnitude");
        } else {
            panic!("prismatic: expected StructureInstance, got {r:?}");
        }

        // cylindrical → CylForce { components: [τ[0], τ[1]] }  (DOF=2)
        let r = joint_force_value("cylindrical", &[1.0, 2.0]).expect("cylindrical/2");
        if let Value::StructureInstance(d) = &r {
            assert_eq!(d.type_name, "CylForce", "cylindrical type_name");
            match d.fields.get("components") {
                Some(Value::List(comps)) => assert_eq!(comps.len(), 2, "CylForce component count"),
                other => panic!("CylForce: expected components List, got {other:?}"),
            }
        } else {
            panic!("cylindrical: expected StructureInstance, got {r:?}");
        }

        // planar → PlanarForce { components: [τ[0..3]] }  (DOF=3)
        let r = joint_force_value("planar", &[1.0, 2.0, 3.0]).expect("planar/3");
        if let Value::StructureInstance(d) = &r {
            assert_eq!(d.type_name, "PlanarForce", "planar type_name");
            match d.fields.get("components") {
                Some(Value::List(comps)) => {
                    assert_eq!(comps.len(), 3, "PlanarForce component count")
                }
                other => panic!("PlanarForce: expected components List, got {other:?}"),
            }
        } else {
            panic!("planar: expected StructureInstance, got {r:?}");
        }

        // spherical → SphereForce { components: [τ[0..3]] }  (DOF=3)
        let r = joint_force_value("spherical", &[4.0, 5.0, 6.0]).expect("spherical/3");
        if let Value::StructureInstance(d) = &r {
            assert_eq!(d.type_name, "SphereForce", "spherical type_name");
            match d.fields.get("components") {
                Some(Value::List(comps)) => {
                    assert_eq!(comps.len(), 3, "SphereForce component count")
                }
                other => panic!("SphereForce: expected components List, got {other:?}"),
            }
        } else {
            panic!("spherical: expected StructureInstance, got {r:?}");
        }

        // fixed → ZeroForce (empty τ, no fields beyond type_name)
        let r = joint_force_value("fixed", &[]).expect("fixed/0");
        if let Value::StructureInstance(d) = &r {
            assert_eq!(d.type_name, "ZeroForce", "fixed type_name");
        } else {
            panic!("fixed: expected StructureInstance, got {r:?}");
        }

        // Wrong arity → None
        assert!(
            joint_force_value("revolute", &[1.0, 2.0]).is_none(),
            "revolute/2 must be None"
        );
        assert!(
            joint_force_value("prismatic", &[]).is_none(),
            "prismatic/0 must be None"
        );
        assert!(
            joint_force_value("cylindrical", &[1.0]).is_none(),
            "cylindrical/1 must be None"
        );
        assert!(
            joint_force_value("fixed", &[1.0]).is_none(),
            "fixed/1 (non-empty τ) must be None"
        );

        // Unknown kind → None
        assert!(
            joint_force_value("flapper", &[1.0]).is_none(),
            "unknown joint kind must be None"
        );
    }
}
