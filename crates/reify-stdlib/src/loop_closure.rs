//! Loop-closure machinery: value-level helpers operating on joint-Map `Value`s.
//!
//! This module provides the building blocks the kinematic snapshot evaluator
//! (future task 2585) and the generic Newton solver in
//! `reify_constraints::loop_closure` use to drive closed-chain mechanisms to
//! consistency.  It is the value-side companion to `reify-constraints::loop_closure`.
//!
//! ## γ widening (PRD KCC-γ, 2026-05-27)
//!
//! The per-joint motion-variable type widened from `f64` to
//! [`JointValue`](crate::loop_closure_value::JointValue), enabling planar
//! (3-DOF), spherical (3-DOF), and cylindrical (2-DOF) joints to participate
//! in closed-chain Newton solves.  Signatures of [`chain_transform`],
//! [`loop_residual_twist`], [`value_for_joint`], [`joint_range_midpoint`],
//! [`chain_jacobian_fd`], and [`extract_loop_closure_chains`] all changed
//! shape; callers no longer bridge through `&flatten_dofs(&vals)`.  The
//! Newton-state flat `Vec<f64>` boundary now lives entirely inside
//! `solve_loop_closure` (see `loop_closure_solver.rs`).
//!
//! Public API surface:
//!   * [`chain_transform`] — left-fold a sequence of joint Maps + motion
//!     variables into a single composed `Value::Transform`.
//!   * [`loop_residual_twist`] — log of `inv(T_a) · T_b`, returned as a
//!     6-vector twist suitable for stacking into a Newton residual.
//!   * [`joint_range_midpoint`] — joint-range midpoint for fresh-snapshot
//!     start strategy; recurses through `coupling` joints to the parent.
//!   * [`per_joint_jacobian_local`] — wraps the existing `joint_jacobian`
//!     builtin to return an analytic per-joint twist column as `[f64; 6]`.
//!     Returns `None` for joint kinds that lack an analytic form (used as
//!     the FD-fallback signal once task 4–7's spherical/cylindrical/planar
//!     joints land).
//!   * [`chain_jacobian_fd`] — central-difference chain Jacobian, one
//!     `[f64; 6]` column per free joint index.
//!
//! Twist convention: `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` (angular first, linear last)
//! mirroring the `Map { angular, linear }` shape emitted by `transform_log` and
//! `joint_jacobian`.  All `[f64; 6]` returns and arguments in this module follow
//! this single canonical ordering.
//!
//! Jacobian strategy (v0.2 task 2 MVP): chain Jacobians use central-difference
//! finite difference ([`chain_jacobian_fd`]).  This is correct for all joint
//! kinds that [`value_for_joint`] handles (currently prismatic, revolute,
//! coupling, fixed); planar, spherical, and cylindrical are deferred to PRD
//! v0.2 kinematic task 2 (taskmaster #2670 — "FD fallback for multi-DOF
//! kinds") because their f64-per-joint scalar representation is insufficient.
//! The analytic per-joint twist column is exposed via [`per_joint_jacobian_local`]
//! for future adjoint-transport composition; that optimisation is out of scope
//! for this task and tracked as a follow-up design note.
//!
//! See `docs/prds/v0_2/kinematic-constraints.md` §"Loop-closure solver" for the
//! design rationale and convergence-tolerance defaults (1 µm position, 1 µrad
//! rotation — surfaced as `NewtonConfig` knobs in `reify_constraints::loop_closure`).

use reify_ir::Value;

use crate::eval_builtin;
use crate::loop_closure_value::JointValue;

/// Fold a chain of joint Maps into a single composed Transform.
///
/// `chain[i]` is a joint `Value::Map` (any kind in `joints::JOINT_KINDS`);
/// `values[i]` is its per-DOF motion variable carried as a typed
/// [`JointValue`]: `Scalar(v)` for single-DOF kinds (prismatic / revolute /
/// coupling / fixed — fixed's value is ignored), `Planar([x,y,θ])` /
/// `Sphere([w,x,y,z])` / `Cyl([d,θ])` for the three multi-DOF kinds.
///
/// Composition is left-to-right: `T_total = T_0 * T_1 * ... * T_{n-1}`,
/// matching the semantics of nesting joints from base outward.  Returns
/// `None` if any joint produces `Value::Undef` from `transform_at` (invalid
/// joint Map, dimension mismatch, JointValue variant mismatched to the
/// joint kind, etc.) or if `chain.len() != values.len()`.
///
/// KCC-γ (PRD §5.2) widened the per-joint type from `f64` to `JointValue` so
/// the multi-DOF kinds participate in chain composition.  Single-DOF chains
/// remain a strict subset: a `Vec<JointValue::Scalar(_)>` walks the same
/// dispatch path the pre-widening `&[f64]` signature did.
pub fn chain_transform(chain: &[Value], values: &[JointValue]) -> Option<Value> {
    if chain.len() != values.len() {
        return None;
    }
    let mut acc = eval_builtin("transform3_identity", &[]);
    if acc.is_undef() {
        return None;
    }
    for (joint, v) in chain.iter().zip(values.iter()) {
        let v_value = value_for_joint(joint, v)?;
        // PRD §7.2 no-bypass invariant (route 1): per-joint transforms MUST route through
        // `transform_at` and MUST NOT be reconstructed from the joint Map directly.
        // `transform_at` applies the "origin" pre-compose uniformly, so any offset is baked
        // in here. Verified behaviourally by
        // `chain_transform_offset_single_joint_equals_transform_at_route1`.
        let next = eval_builtin("transform_at", &[joint.clone(), v_value]);
        if next.is_undef() {
            return None;
        }
        let composed = eval_builtin("transform_compose", &[acc, next]);
        if composed.is_undef() {
            return None;
        }
        acc = composed;
    }
    Some(acc)
}

/// Compute the SE(3) loop-closure residual twist between two chains.
///
/// Returns `transform_log(transform_inverse(T_a) ⋅ T_b)` flattened to a
/// 6-element `[f64; 6]` in `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` ordering.
///
/// Returns `None` if either chain produces a `None` from `chain_transform`,
/// or if any underlying SE(3) operation produces `Value::Undef`.
///
/// KCC-γ (PRD §5.2): both `vals_a` / `vals_b` are typed `&[JointValue]`
/// slices — the chain composition routes multi-DOF kinds through
/// `chain_transform` without re-flattening to a scalar vector.
pub fn loop_residual_twist(
    chain_a: &[Value],
    vals_a: &[JointValue],
    chain_b: &[Value],
    vals_b: &[JointValue],
) -> Option<[f64; 6]> {
    let t_a = chain_transform(chain_a, vals_a)?;
    let t_b = chain_transform(chain_b, vals_b)?;
    let t_a_inv = eval_builtin("transform_inverse", &[t_a]);
    if t_a_inv.is_undef() {
        return None;
    }
    let t_rel = eval_builtin("transform_compose", &[t_a_inv, t_b]);
    if t_rel.is_undef() {
        return None;
    }
    let twist_map = eval_builtin("transform_log", &[t_rel]);
    if twist_map.is_undef() {
        return None;
    }
    twist_map_to_array(&twist_map)
}

/// Convert a twist `Value::Map { angular, linear }` into the canonical
/// `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` `[f64; 6]` layout.
///
/// Reads each Vector3 component via `Value::as_f64` (accepts `Real`, `Int`,
/// `Scalar`).  Returns `None` if either field is missing, malformed, or any
/// component is non-numeric.
fn twist_map_to_array(twist_map: &Value) -> Option<[f64; 6]> {
    let map = match twist_map {
        Value::Map(m) => m,
        _ => return None,
    };
    let read_vec3 = |key: &str| -> Option<[f64; 3]> {
        match map.get(&Value::String(key.to_string())) {
            Some(Value::Vector(items)) if items.len() == 3 => {
                let a = items[0].as_f64()?;
                let b = items[1].as_f64()?;
                let c = items[2].as_f64()?;
                if !a.is_finite() || !b.is_finite() || !c.is_finite() {
                    return None;
                }
                Some([a, b, c])
            }
            _ => None,
        }
    };
    let ang = read_vec3("angular")?;
    let lin = read_vec3("linear")?;
    Some([ang[0], ang[1], ang[2], lin[0], lin[1], lin[2]])
}

/// Return the midpoint of a joint's free-variable range, wrapped as a
/// `JointValue` whose variant matches the joint's kind:
///   * `prismatic` / `revolute` / `coupling` → `JointValue::Scalar(mid)`
///     where `mid` is the range midpoint in SI units (metres or radians).
///   * `planar` → `JointValue::Planar([mid_x, mid_y, mid_θ])` from the
///     three per-DOF range midpoints (`range_x`, `range_y`, `range_theta`).
///   * `spherical` → `JointValue::Sphere([1, 0, 0, 0])` — the identity
///     quaternion (axis-isotropic; the `range_angle` bound constrains
///     downstream solver motion magnitude, not the seed direction).
///   * `cylindrical` → `JointValue::Cyl([mid_d, mid_θ])` from
///     `translation_range` / `rotation_range` midpoints.
///
/// Returns `None` for joints whose range is missing, unbounded on either
/// side, for fixed (0-DOF) joints whose free-variable space is empty, or
/// for non-Map / unknown-kind inputs.
///
/// **Coupling note**: returns the *parent's* range midpoint (not scaled by
/// `ratio`), wrapped as `JointValue::Scalar`.  The free-variable space of a
/// coupling joint is the parent's motion variable — the coupling's
/// `transform_at` arm applies the ratio downstream when computing the
/// parent's coupled position.
///
/// KCC-γ (PRD §5.2) widened this from `Option<f64>` to `Option<JointValue>`
/// so multi-DOF kinds (planar, spherical, cylindrical) participate in the
/// chain machinery and the loop-closure Newton solver.  The explicit
/// per-arm dispatch is retained so a future kind addition cannot silently
/// drift; the JOINT_KINDS-iteration partition test in this module's
/// `tests` block loud-fails any unhandled kind.
pub fn joint_range_midpoint(joint: &Value) -> Option<JointValue> {
    let map = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    match kind {
        "prismatic" | "revolute" => {
            let mid = range_midpoint(map, "range")?;
            Some(JointValue::Scalar(mid))
        }
        "coupling" => {
            let parent = map.get(&Value::String("parent".to_string()))?;
            joint_range_midpoint(parent)
        }
        // 0-DOF — empty free-variable space; no midpoint to seed.
        "fixed" => None,
        "planar" => {
            let mid_x = range_midpoint(map, "range_x")?;
            let mid_y = range_midpoint(map, "range_y")?;
            let mid_theta = range_midpoint(map, "range_theta")?;
            Some(JointValue::Planar([mid_x, mid_y, mid_theta]))
        }
        // Axis-isotropic: identity quaternion is the canonical seed regardless
        // of `range_angle` (which bounds the rotation magnitude downstream).
        "spherical" => Some(JointValue::Sphere([1.0, 0.0, 0.0, 0.0])),
        "cylindrical" => {
            let mid_d = range_midpoint(map, "translation_range")?;
            let mid_theta = range_midpoint(map, "rotation_range")?;
            Some(JointValue::Cyl([mid_d, mid_theta]))
        }
        _ => None,
    }
}

/// Lookup helper: extract the midpoint of a `Value::Range` stored at `key`
/// in a joint Map.  Returns `None` for missing keys, unbounded ranges, or
/// non-numeric / non-finite bounds.  Shared by `joint_range_midpoint`'s
/// per-kind arms (single-DOF `range`, planar `range_{x,y,theta}`,
/// cylindrical `translation_range` / `rotation_range`).
fn range_midpoint(
    map: &std::collections::BTreeMap<Value, Value>,
    key: &str,
) -> Option<f64> {
    let range = map.get(&Value::String(key.to_string()))?;
    let (lo, up) = match range {
        Value::Range {
            lower: Some(lo),
            upper: Some(up),
            ..
        } => (lo.as_ref(), up.as_ref()),
        _ => return None,
    };
    let lo_si = lo.as_f64()?;
    let up_si = up.as_f64()?;
    if !lo_si.is_finite() || !up_si.is_finite() {
        return None;
    }
    Some((lo_si + up_si) / 2.0)
}

/// Return the analytic per-joint twist column expressed in the joint's own
/// input frame, as `[ω_x, ω_y, ω_z, v_x, v_y, v_z]`.
///
/// Wraps the existing `joint_jacobian` builtin (analytic for prismatic /
/// revolute / coupling) and converts the resulting `Map { angular, linear }`
/// to the canonical `[f64; 6]` layout.  Returns `None` for joint kinds the
/// builtin can't analyse (the FD chain assembly in
/// [`chain_jacobian_fd`] is the fallback for those — it perturbs the chain
/// without consulting this accessor).
///
/// **Note**: this is the per-joint analytic column.  Chain-level Jacobians
/// for the v0.2 task 2 MVP are computed via finite difference in
/// [`chain_jacobian_fd`]; a future optimisation can compose these per-joint
/// columns via SE(3) adjoint transport.
pub fn per_joint_jacobian_local(joint: &Value) -> Option<[f64; 6]> {
    let result = eval_builtin("joint_jacobian", std::slice::from_ref(joint));
    if result.is_undef() {
        return None;
    }
    twist_map_to_array(&result)
}

/// Compute the chain Jacobian by finite difference: one twist column per
/// per-DOF storage component of every free joint.
///
/// For each `i ∈ free_indices`, the joint at `values[i]` contributes
/// `JointValue::as_f64_slice().len()` columns (1 for Scalar, 2 for Cyl, 3 for
/// Planar, 4 for Sphere — equal to `JointKind::flat_len()`).  Each column
/// perturbs one storage f64 by `±eps`, evaluates `chain_transform` at both
/// perturbed values, and computes
/// `transform_log(transform_inverse(T_minus) ⋅ T_plus) / (2*eps)` to recover
/// the central-difference column.  This is symmetric-error O(ε²) and works
/// for every joint kind `value_for_joint` handles (all seven JOINT_KINDS post
/// KCC-γ widening).
///
/// Returns `None` if `eps <= 0`, any free index is out of range, the
/// `chain.len() != values.len()`, or any `chain_transform` along the way
/// produces `None`.
///
/// Sphere slots contribute 4 raw-storage columns (matching `flat_len = 4`).
/// This is the shipped, PRD-ratified contract (§5.3) — no switch to 3
/// body-frame angular tangent columns is pending.  The redundant fourth
/// column reflects the off-manifold direction in unit-quaternion storage;
/// the solver damps it with per-component Tikhonov regularization
/// (`NewtonConfig::regularization_per_diag`, applied to the `JᵀJ` diagonal
/// in `solve_normal_equations`) and projects each iterate back to S³ via
/// the closure-internal `renormalize_quaternion` (see
/// `loop_closure_solver.rs`).
pub fn chain_jacobian_fd(
    chain: &[Value],
    values: &[JointValue],
    free_indices: &[usize],
    eps: f64,
) -> Option<Vec<[f64; 6]>> {
    if eps <= 0.0 || !eps.is_finite() {
        return None;
    }
    if chain.len() != values.len() {
        return None;
    }
    let n = chain.len();
    let mut cols: Vec<[f64; 6]> = Vec::new();
    for &i in free_indices {
        if i >= n {
            return None;
        }
        let width = values[i].as_f64_slice().len();
        for c in 0..width {
            let mut plus = values.to_vec();
            let mut minus = values.to_vec();
            perturb_storage_component(&mut plus[i], c, eps);
            perturb_storage_component(&mut minus[i], c, -eps);
            let t_plus = chain_transform(chain, &plus)?;
            let t_minus = chain_transform(chain, &minus)?;
            let t_minus_inv = eval_builtin("transform_inverse", &[t_minus]);
            if t_minus_inv.is_undef() {
                return None;
            }
            let rel = eval_builtin("transform_compose", &[t_minus_inv, t_plus]);
            if rel.is_undef() {
                return None;
            }
            let twist_map = eval_builtin("transform_log", &[rel]);
            if twist_map.is_undef() {
                return None;
            }
            let twist = twist_map_to_array(&twist_map)?;
            let scale = 1.0 / (2.0 * eps);
            let mut col = [0.0; 6];
            for k in 0..6 {
                col[k] = twist[k] * scale;
            }
            cols.push(col);
        }
    }
    Some(cols)
}

/// Add `delta` to the `c`-th storage f64 of a `JointValue` in place.
///
/// Used by [`chain_jacobian_fd`] to drive per-component central-difference
/// perturbations.  Out-of-range `c` is a no-op (defence-in-depth — the caller
/// derives `c` from `as_f64_slice().len()`, so this branch is unreachable
/// from normal use).
fn perturb_storage_component(jv: &mut JointValue, c: usize, delta: f64) {
    match jv {
        JointValue::Scalar(s) => {
            if c == 0 {
                *s += delta;
            }
        }
        JointValue::Cyl(arr) => {
            if c < 2 {
                arr[c] += delta;
            }
        }
        JointValue::Planar(arr) => {
            if c < 3 {
                arr[c] += delta;
            }
        }
        JointValue::Sphere(arr) => {
            if c < 4 {
                arr[c] += delta;
            }
        }
    }
}

/// Wrap a typed [`JointValue`] motion variable in the dimensioned `Value`
/// shape `transform_at` consumes per joint kind:
///
///   * `prismatic`   ← `Scalar(s)`           → `Value::length(s)` (metres)
///   * `revolute`    ← `Scalar(s)`           → `Value::angle(s)`  (radians)
///   * `coupling`    ← `Scalar(s)`           → length/angle per parent kind
///   * `fixed`       ← any                   → `Value::Real(0.0)` (ignored)
///   * `planar`      ← `Planar([x,y,θ])`     → `Value::List([length(x), length(y), angle(θ)])`
///   * `spherical`   ← `Sphere([w,x,y,z])`   → `Value::Orientation { w, x, y, z }`
///   * `cylindrical` ← `Cyl([d,θ])`          → `Value::List([length(d), angle(θ)])`
///
/// Returns `None` for unknown kinds, malformed Maps, or when the
/// `JointValue` variant does not match the joint kind (e.g. a `Planar`
/// JointValue paired with a revolute joint — the chain machinery
/// short-circuits to `None` so the caller's `transform_at` invocation never
/// receives a mismatched motion variable).
///
/// KCC-γ (PRD §5.2): widened from `(joint, f64) -> Option<Value>` to
/// `(joint, &JointValue) -> Option<Value>` to dispatch on the per-DOF
/// surface shape rather than a single f64.  The multi-DOF arms now produce
/// the exact `Value::List` / `Value::Orientation` shapes `transform_at`
/// accepts (joints.rs:239-393); chain_transform can fold any JOINT_KINDS
/// member without a parallel dispatch path.  The explicit per-arm match
/// (rather than relying on a catch-all `_ => None`) is retained so a future
/// kind addition cannot silently drift; the JOINT_KINDS-iteration partition
/// test in this module's `tests` block loud-fails any unhandled kind.
pub(crate) fn value_for_joint(joint: &Value, jv: &JointValue) -> Option<Value> {
    let map = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    match (kind, jv) {
        ("prismatic", JointValue::Scalar(s)) => Some(Value::length(*s)),
        ("revolute", JointValue::Scalar(s)) => Some(Value::angle(*s)),
        ("coupling", JointValue::Scalar(s)) => {
            let parent_map = match map.get(&Value::String("parent".to_string())) {
                Some(Value::Map(pm)) => pm,
                _ => return None,
            };
            let parent_kind = match parent_map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s)) => s.as_str(),
                _ => return None,
            };
            match parent_kind {
                "prismatic" => Some(Value::length(*s)),
                "revolute" => Some(Value::angle(*s)),
                _ => None,
            }
        }
        // 0-DOF — JointValue ignored; sentinel `Real(0.0)` passes
        // `transform_at("fixed", _)`'s numeric guard.
        ("fixed", _) => Some(Value::Real(0.0)),
        ("planar", JointValue::Planar([x, y, theta])) => Some(Value::List(vec![
            Value::length(*x),
            Value::length(*y),
            Value::angle(*theta),
        ])),
        ("spherical", JointValue::Sphere([w, x, y, z])) => Some(Value::Orientation {
            w: *w,
            x: *x,
            y: *y,
            z: *z,
        }),
        ("cylindrical", JointValue::Cyl([d, theta])) => Some(Value::List(vec![
            Value::length(*d),
            Value::angle(*theta),
        ])),
        // Mismatched JointValue variant / joint kind (e.g. Planar paired
        // with a revolute joint) — surface as None so the chain
        // machinery short-circuits gracefully.
        _ => None,
    }
}

/// Solver-input bundle returned by [`extract_loop_closure_chains`].
///
/// Tuple layout — see the function's doc comment for the full per-field
/// contract (kept there to keep the field semantics next to the extraction
/// logic that produces them):
///
///   `(chain_a, vals_a, chain_b, vals_b_initial, free_b)`
///
/// Aliased here purely to satisfy `clippy::type_complexity` — the 5-tuple
/// is a one-shot return shape consumed by snapshot.rs's loop-closure arm
/// and not a durable structural concern, so a tuple alias (rather than a
/// new struct) keeps the call-site destructuring pattern unchanged.
///
/// KCC-γ step-10 (PRD §5.2): `vals_a` and `vals_b_initial` widen from
/// `Vec<f64>` to `Vec<JointValue>` so multi-DOF joints (planar / spherical /
/// cylindrical) flow into the Newton solver as typed per-DOF surfaces
/// (`JointValue::Planar([x,y,θ])`, `JointValue::Sphere([w,x,y,z])`,
/// `JointValue::Cyl([d,θ])`) rather than collapsing to None at the f64-shim.
pub type LoopClosureSolverInputs = (
    Vec<Value>,
    Vec<JointValue>,
    Vec<Value>,
    Vec<JointValue>,
    Vec<usize>,
);

/// Extract the per-loop solver inputs from a single `loop_closure` Map record.
///
/// Translates a `loop_closure` Map record (the kind appended to a Mechanism
/// Map's `loop_closures` list by the v0.2 builder) plus the user's
/// `bindings: &[Value]` slice into the five vectors the closed-chain Newton
/// solver in `reify_constraints::loop_closure::solve_loop_closure_with_diagnostics`
/// consumes:
///
///   * `chain_a`         — the joints in `path_a` with the leading world
///     sentinel stripped (left-to-right composition order).
///   * `vals_a`          — the SI-unit motion values for `chain_a`,
///     resolved from `bindings` per-joint (prismatic → metres,
///     revolute → radians, coupling → parent's input units, fixed → `0.0`
///     sentinel since `transform_at` ignores the value).  Missing bindings
///     fall back to the joint's range midpoint.
///   * `chain_b`         — the joints in `path_b` with the world sentinel
///     stripped.
///   * `vals_b_initial`  — initial-guess SI values for `chain_b`. Joints with
///     a direct binding entry use the bound value; otherwise the range midpoint.
///   * `free_b`          — positions in `chain_b` whose joints are free
///     (no direct binding entry); the solver iterates these.
///
/// Returns `None` on:
///   * non-Map record,
///   * missing/non-List `path_a` or `path_b`,
///   * either path empty or missing the leading world sentinel,
///   * any chain joint that has no resolvable SI value (no binding,
///     no midpoint — e.g. multi-DOF kinds, malformed Maps).
///
/// The world sentinel at chain head is identified by `kind = "world"`
/// (matching `mechanism::is_world`) and dropped before composition — the
/// closing-chain composition starts at the joint immediately attached to
/// world, not at the world sentinel itself.
///
/// **Pure value-side helper** — performs no FK walk and no solver invocation.
/// Built so it can be tested in isolation before snapshot.rs consumes it.
pub fn extract_loop_closure_chains(
    record: &Value,
    bindings: &[Value],
) -> Option<LoopClosureSolverInputs> {
    let map = match record {
        Value::Map(m) => m,
        _ => return None,
    };
    let path_a = match map.get(&Value::String("path_a".to_string())) {
        Some(Value::List(p)) => p.as_slice(),
        _ => return None,
    };
    let path_b = match map.get(&Value::String("path_b".to_string())) {
        Some(Value::List(p)) => p.as_slice(),
        _ => return None,
    };

    let chain_a = strip_world_sentinel(path_a)?;
    let chain_b = strip_world_sentinel(path_b)?;

    // chain_a is the spanning-tree (already-resolved) side: every joint must
    // resolve to a JointValue via direct binding, coupling-tracks-parent
    // recursion, fixed-joint sentinel, or range-midpoint fallback.  Any
    // joint that fails all four short-circuits the whole call to None.
    let mut vals_a = Vec::with_capacity(chain_a.len());
    for joint in &chain_a {
        let v_jv = resolve_joint_value(joint, bindings)?;
        vals_a.push(v_jv);
    }

    // chain_b is the closing side: a joint with a *direct* binding entry
    // is a fixed initial value; any joint without a direct binding becomes
    // a free index, seeded from its range midpoint.  Coupling and fixed
    // arms intentionally fall through the direct-lookup branch — multi-loop
    // coupling is out of v0.2 scope (see plan design-decisions §4).
    //
    // **Asymmetry note (v0.2 limitation).**  `chain_a` resolves via
    // `resolve_joint_value`, which carries a fixed-joint sentinel arm
    // (returns `Some(JointValue::Scalar(0.0))`).  `chain_b`'s unbound-joint
    // fallback uses `joint_range_midpoint` directly, which returns `None`
    // for fixed joints — so a fixed joint appearing in `path_b` without a
    // direct binding would collapse the whole record to None and the
    // snapshot to Undef.  In practice the mechanism builder does not place
    // fixed joints on closing paths (the closing edge always references a
    // motion joint to drive the solver), so this asymmetry is a latent
    // shape constraint rather than a live bug.  A future v0.3 refactor
    // can route this fallback through `resolve_joint_value` (and skip the
    // index from `free_b` when the result is the fixed sentinel) once a
    // real fixture demands fixed joints in path_b.
    let mut vals_b_initial = Vec::with_capacity(chain_b.len());
    let mut free_b: Vec<usize> = Vec::new();
    for (i, joint) in chain_b.iter().enumerate() {
        if let Some(v_jv) = direct_binding_value(joint, bindings) {
            vals_b_initial.push(v_jv);
        } else {
            // KCC-γ step-10: `joint_range_midpoint` now returns
            // `Option<JointValue>` — multi-DOF kinds (planar / spherical /
            // cylindrical) produce per-DOF surfaces that flow directly into
            // the widened `Vec<JointValue>` solver-input shape.  The
            // f64-shim that collapsed multi-DOF midpoints to None is gone.
            let mid_jv = joint_range_midpoint(joint)?;
            vals_b_initial.push(mid_jv);
            free_b.push(i);
        }
    }

    Some((chain_a, vals_a, chain_b, vals_b_initial, free_b))
}

/// Strip the leading world sentinel from a path (`[world, j_1, ..., j_k]` →
/// `[j_1, ..., j_k]`).  Returns None if the path is shorter than 2 elements
/// (the stripped tail would terminate before the closing joint) or if the
/// first element is not a `kind = "world"` Map.
///
/// Mirrors `reify_constraints::loop_closure::strip_world_sentinel` (which
/// is private to that module); duplicated here so the stdlib helper is
/// self-contained without crossing the constraints crate boundary.
fn strip_world_sentinel(path: &[Value]) -> Option<Vec<Value>> {
    if path.len() < 2 {
        return None;
    }
    let first = path.first()?;
    let is_world = match first {
        Value::Map(m) => {
            m.get(&Value::String("kind".to_string())) == Some(&Value::String("world".to_string()))
        }
        _ => false,
    };
    if !is_world {
        return None;
    }
    Some(path[1..].to_vec())
}

/// Look up a joint's motion value via a *direct* binding entry (no coupling
/// recursion, no midpoint fallback), wrapped as a typed [`JointValue`].
///
/// Linear scan — same shape as snapshot.rs's `value_for` direct-binding
/// arm, but returns the bound value as a per-DOF surface keyed on joint
/// kind:
///
///   * `prismatic` / `revolute`              → `JointValue::Scalar(s)`
///   * `coupling`                            → delegates to parent kind
///   * `fixed`                               → `JointValue::Scalar(0.0)`
///     (sentinel; `transform_at("fixed", _)` ignores the value)
///   * `planar`     ← `Value::List([len, len, ang])` → `JointValue::Planar`
///   * `spherical`  ← `Value::Orientation { w, x, y, z }` → `JointValue::Sphere`
///   * `cylindrical` ← `Value::List([len, ang])`    → `JointValue::Cyl`
///
/// Returns None when no binding entry's `joint` field is structurally equal
/// to `joint`, when the joint kind is unknown, or when the bound `value`
/// shape doesn't match the expected per-kind surface (e.g. a Scalar bound
/// to a planar joint, or a Vector bound to a revolute).
///
/// Used in the closing-side `chain_b` walk to distinguish "user-pinned
/// joint with explicit binding" (fixed initial value) from "free joint
/// the solver should iterate" (no direct binding, falls through to
/// midpoint seed + free_b membership).
///
/// **Dimension validation is deferred.**  `Value::as_f64()` returns the
/// bare `si_value` regardless of the carried `Dimension`, so a user who
/// binds a length-dimensioned scalar to a revolute joint (or vice versa)
/// will silently feed a wrong-magnitude SI numeric value into the
/// solver's `vals_a` / `vals_b_initial`.  The closed-chain path here is
/// upstream of snapshot.rs's `transform_at` validation site, which
/// catches the dimension mismatch when the solver-converged value is
/// rehydrated for the FK re-walk via `wrap_jointvalue_for_joint` →
/// `transform_at`.  In practice the residual-evaluation path inside
/// `solve_loop_closure` invokes `chain_transform`, which itself routes
/// through dimension-checked Transform composition; a wrong-dimension
/// value will surface as a non-converging residual rather than as a
/// silent acceptance of an inconsistent configuration.  Callers that
/// want eager validation should compose this with the joint-kind
/// predicate at the snapshot boundary.
fn direct_binding_value(joint: &Value, bindings: &[Value]) -> Option<JointValue> {
    for entry in bindings {
        let map = match entry {
            Value::Map(m) => m,
            _ => continue,
        };
        if map.get(&Value::String("joint".to_string())) == Some(joint)
            && let Some(v) = map.get(&Value::String("value".to_string()))
        {
            return jointvalue_from_bound_value(joint, v);
        }
    }
    None
}

/// Convert a bound `Value` (the `value` field of a `bind(joint, value)`
/// binding Map) into the typed `JointValue` shape that matches the joint's
/// kind.  Inverse of `value_for_joint` — `direct_binding_value` calls this
/// after locating a matching binding entry.
///
/// Per-kind surface contract (mirrors `value_for_joint` at loop_closure.rs:399):
///   * `prismatic` / `revolute`              → `Scalar(v.as_f64()?)`
///   * `coupling`                            → recurses through `parent` kind
///   * `fixed`                               → `Scalar(0.0)` (value ignored)
///   * `planar`     ← `Value::List([len, len, ang])` → `Planar([x, y, θ])`
///   * `spherical`  ← `Value::Orientation`           → `Sphere([w, x, y, z])`
///   * `cylindrical` ← `Value::List([len, ang])`     → `Cyl([d, θ])`
fn jointvalue_from_bound_value(joint: &Value, bound: &Value) -> Option<JointValue> {
    let map = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    match kind {
        "prismatic" | "revolute" => Some(JointValue::Scalar(bound.as_f64()?)),
        "coupling" => {
            let parent = map.get(&Value::String("parent".to_string()))?;
            jointvalue_from_bound_value(parent, bound)
        }
        // 0-DOF: `transform_at("fixed", _)` ignores the value; downstream
        // chain machinery ignores the Scalar payload via the kind dispatch.
        "fixed" => Some(JointValue::Scalar(0.0)),
        "planar" => {
            let items = match bound {
                Value::List(items) if items.len() == 3 => items,
                _ => return None,
            };
            let x = items[0].as_f64()?;
            let y = items[1].as_f64()?;
            let theta = items[2].as_f64()?;
            Some(JointValue::Planar([x, y, theta]))
        }
        "spherical" => match bound {
            Value::Orientation { w, x, y, z } => Some(JointValue::Sphere([*w, *x, *y, *z])),
            _ => None,
        },
        "cylindrical" => {
            let items = match bound {
                Value::List(items) if items.len() == 2 => items,
                _ => return None,
            };
            let d = items[0].as_f64()?;
            let theta = items[1].as_f64()?;
            Some(JointValue::Cyl([d, theta]))
        }
        _ => None,
    }
}

/// Resolve a joint's motion value to a typed [`JointValue`] via the same
/// fallback chain `snapshot::value_for` uses.
///
/// Resolution order:
/// 1. Direct binding by structural `Value::Eq` on the joint Map.
/// 2. Coupling-tracks-parent: when `joint` is a coupling and isn't directly
///    bound, recurse on the coupling's `parent` joint.
/// 3. Fixed joint sentinel: `JointValue::Scalar(0.0)` (snapshot.rs's
///    `transform_at` arm ignores the second argument for fixed joints).
/// 4. Range-midpoint fallback via [`joint_range_midpoint`] (per-kind
///    surface: Length / Planar / Sphere / Cyl).
///
/// Returns None for malformed joint Maps, unknown kinds, or joints with no
/// resolvable value (no binding AND no midpoint — e.g. a planar joint with
/// no range_x).  KCC-γ step-10: multi-DOF kinds now resolve to the typed
/// per-DOF surface rather than collapsing to None at the f64-shim.
fn resolve_joint_value(joint: &Value, bindings: &[Value]) -> Option<JointValue> {
    if let Some(v) = direct_binding_value(joint, bindings) {
        return Some(v);
    }
    if let Value::Map(map) = joint {
        let kind = match map.get(&Value::String("kind".to_string())) {
            Some(Value::String(s)) => s.as_str(),
            _ => return None,
        };
        if kind == "coupling"
            && let Some(parent) = map.get(&Value::String("parent".to_string()))
        {
            return resolve_joint_value(parent, bindings);
        }
        if kind == "fixed" {
            return Some(JointValue::Scalar(0.0));
        }
    }
    joint_range_midpoint(joint)
}

/// Compute the loop-closure residual Jacobian with respect to caller-supplied
/// tree-joint targets by central-difference of the full two-chain residual.
///
/// # GAP 1 resolution
///
/// Unlike [`chain_jacobian_fd`], which differentiates a single chain w.r.t.
/// that chain's free joints, this function differentiates the full two-chain
/// residual `loop_residual_twist(chain_a, vals_a, chain_b, vals_b)` w.r.t.
/// each caller-supplied target joint.  For each target joint the perturbation
/// is applied **everywhere that joint appears in either chain** (located by
/// structural `Value::Eq`) so that a joint shared between chain_a and chain_b
/// (a world-rooted prefix joint) is perturbed consistently in both chains,
/// yielding the correct total derivative.
///
/// The closing joint — not being a tree `at` joint — is never passed as a
/// target and therefore never perturbed; its direction is removed by
/// [`reduce_constraint_rank`](crate::dynamics::closed_chain::reduce_constraint_rank).
///
/// # Parameters
/// - `chain_a`, `vals_a`: spanning-tree side of the loop; `vals_a.len()` must
///   equal `chain_a.len()`.
/// - `chain_b`, `vals_b`: closing side; `vals_b.len()` must equal
///   `chain_b.len()`.
/// - `target_joints`: the ordered list of tree `at` joints to differentiate
///   w.r.t.  Each single-DOF joint contributes one `[f64; 6]` column;
///   multi-DOF joints contribute `JointValue::as_f64_slice().len()` columns.
///   A joint **absent from both chains** contributes a zero column (it has no
///   perturbation site — the resulting constraint Jacobian column is genuinely
///   zero for that coordinate).
/// - `eps`: central-difference step size.  Must be positive and finite.
///
/// # Returns
/// `Some(columns)` — one `[f64; 6]` column per storage component per target
/// joint, in target order (single-DOF: one column; multi-DOF: flat).
///
/// Returns `None` on:
///   * `eps <= 0` or non-finite,
///   * `chain_a.len() != vals_a.len()` or `chain_b.len() != vals_b.len()`,
///   * any call to `loop_residual_twist` returns `None` (malformed chain,
///     unknown joint kind, or transform Undef).
pub fn loop_residual_jacobian_by_joint(
    chain_a: &[Value],
    vals_a: &[JointValue],
    chain_b: &[Value],
    vals_b: &[JointValue],
    target_joints: &[Value],
    eps: f64,
) -> Option<Vec<[f64; 6]>> {
    if eps <= 0.0 || !eps.is_finite() {
        return None;
    }
    if chain_a.len() != vals_a.len() || chain_b.len() != vals_b.len() {
        return None;
    }

    let mut columns: Vec<[f64; 6]> = Vec::new();

    for target in target_joints {
        // Determine the storage width for this target by scanning either chain
        // for a matching joint (same structural value).  If absent from both,
        // the width defaults to 1 (scalar single-DOF fallback); the resulting
        // zero perturbation produces a zero column regardless.
        let width: usize = chain_a
            .iter()
            .zip(vals_a.iter())
            .find_map(|(j, v)| if j == target { Some(v.as_f64_slice().len()) } else { None })
            .or_else(|| {
                chain_b
                    .iter()
                    .zip(vals_b.iter())
                    .find_map(|(j, v)| if j == target { Some(v.as_f64_slice().len()) } else { None })
            })
            .unwrap_or(1);

        for c in 0..width {
            let mut va_plus = vals_a.to_vec();
            let mut va_minus = vals_a.to_vec();
            let mut vb_plus = vals_b.to_vec();
            let mut vb_minus = vals_b.to_vec();

            // Perturb every occurrence of `target` in chain_a.
            for (i, j) in chain_a.iter().enumerate() {
                if j == target {
                    perturb_storage_component(&mut va_plus[i], c, eps);
                    perturb_storage_component(&mut va_minus[i], c, -eps);
                }
            }
            // Perturb every occurrence of `target` in chain_b.
            for (i, j) in chain_b.iter().enumerate() {
                if j == target {
                    perturb_storage_component(&mut vb_plus[i], c, eps);
                    perturb_storage_component(&mut vb_minus[i], c, -eps);
                }
            }

            // PRD §7.2 no-bypass invariant (route 3): this central-difference
            // evaluates `loop_residual_twist`, which internally calls `chain_transform`
            // → `transform_at`. The offset is inherited automatically through that call
            // chain — this site MUST NOT reconstruct per-joint transforms directly.
            // Verified behaviourally by γ B8 route-3 test and the B5 Jacobian test.
            let rp = loop_residual_twist(chain_a, &va_plus, chain_b, &vb_plus)?;
            let rm = loop_residual_twist(chain_a, &va_minus, chain_b, &vb_minus)?;

            let scale = 1.0 / (2.0 * eps);
            let mut col = [0.0f64; 6];
            for k in 0..6 {
                col[k] = (rp[k] - rm[k]) * scale;
            }
            columns.push(col);
        }
    }

    Some(columns)
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::loop_closure_value::JointValue;
    use crate::test_fixtures::{
        angle_range_0_to_pi, axis_x_unit, axis_y_unit, axis_z_unit, cylindrical_z_joint,
        length_range_0_to_1m, offset_prismatic_x, offset_revolute_z, planar_xy_joint,
        spherical_joint, two_link_offset_chain,
    };
    use reify_ir::Value;

    /// The subset of `crate::joints::JOINT_KINDS` whose motion variables span
    /// more than one f64 — planar (3), spherical (4 storage / 3 manifold DOF),
    /// cylindrical (2).  After KCC-γ widening (PRD §5.2), every kind in this
    /// list returns a non-Scalar `JointValue::{Planar,Sphere,Cyl}` from
    /// `joint_range_midpoint` and the chain machinery accepts those variants
    /// directly without bridging through a flat f64 vector.  See the contract
    /// tests below for the JOINT_KINDS-iteration partition guard.
    const MULTI_DOF_KINDS: &[&str] = &["planar", "spherical", "cylindrical"];

    fn prismatic_x() -> Value {
        eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()])
    }

    fn revolute_z() -> Value {
        eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()])
    }

    /// Extract the translation Vector3 from a Transform; helper for tests.
    fn translation_xyz(t: &Value) -> [f64; 3] {
        let translation = match t {
            Value::Transform { translation, .. } => translation.as_ref(),
            other => panic!("expected Transform, got {other:?}"),
        };
        let comps = match translation {
            Value::Vector(v) if v.len() == 3 => v,
            other => panic!("expected Vector3 translation, got {other:?}"),
        };
        [
            comps[0].as_f64().unwrap(),
            comps[1].as_f64().unwrap(),
            comps[2].as_f64().unwrap(),
        ]
    }

    /// Extract orientation (w, x, y, z) from a Transform.
    fn rotation_wxyz(t: &Value) -> (f64, f64, f64, f64) {
        let rot = match t {
            Value::Transform { rotation, .. } => rotation.as_ref(),
            other => panic!("expected Transform, got {other:?}"),
        };
        match rot {
            Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
            other => panic!("expected Orientation, got {other:?}"),
        }
    }

    // ── chain_transform tests ────────────────────────────────────────────

    #[test]
    fn chain_transform_empty_chain_returns_identity() {
        let result = super::chain_transform(&[], &[]).expect("identity Transform");
        let trans = translation_xyz(&result);
        assert!(trans[0].abs() < 1e-12 && trans[1].abs() < 1e-12 && trans[2].abs() < 1e-12);
        let (w, x, y, z) = rotation_wxyz(&result);
        assert!((w - 1.0).abs() < 1e-12);
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12);
    }

    #[test]
    fn chain_transform_single_prismatic_x_at_half_metre() {
        let chain = vec![prismatic_x()];
        // KCC-γ: chain_transform now takes &[JointValue] directly — the
        // f64-per-joint shim via `flatten_dofs` is gone; tests pass typed
        // `JointValue::Scalar` slots straight through.
        let vals = vec![JointValue::Scalar(0.5)];
        let result = super::chain_transform(&chain, &vals).expect("Transform");
        let trans = translation_xyz(&result);
        assert!((trans[0] - 0.5).abs() < 1e-12);
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
        let (w, x, y, z) = rotation_wxyz(&result);
        assert!((w - 1.0).abs() < 1e-12);
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12);
    }

    #[test]
    fn chain_transform_two_prismatic_x_compose_left_to_right() {
        let chain = vec![prismatic_x(), prismatic_x()];
        let vals = vec![JointValue::Scalar(0.3), JointValue::Scalar(0.5)];
        let result = super::chain_transform(&chain, &vals).expect("Transform");
        let trans = translation_xyz(&result);
        assert!(
            (trans[0] - 0.8).abs() < 1e-12,
            "expected translation_x = 0.8, got {}",
            trans[0]
        );
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
    }

    #[test]
    fn chain_transform_prismatic_then_revolute() {
        // chain = [prismatic_x at 0.5m, revolute_z at π/2]
        // After prismatic: T1 has translation [0.5,0,0], rot identity.
        // After revolute composed: rotation = rot_z(π/2), translation
        // unchanged ([0.5,0,0]) because R1*t2 + t1 with t2=0 ⇒ t1 = [0.5,0,0].
        let chain = vec![prismatic_x(), revolute_z()];
        let vals = vec![
            JointValue::Scalar(0.5),
            JointValue::Scalar(std::f64::consts::FRAC_PI_2),
        ];
        let result = super::chain_transform(&chain, &vals).expect("Transform");
        let trans = translation_xyz(&result);
        assert!((trans[0] - 0.5).abs() < 1e-12);
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
        let (w, _x, _y, z) = rotation_wxyz(&result);
        let half = std::f64::consts::FRAC_PI_4;
        assert!((w - half.cos()).abs() < 1e-12 || (w + half.cos()).abs() < 1e-12);
        assert!((z.abs() - half.sin()).abs() < 1e-12);
    }

    #[test]
    fn chain_transform_length_mismatch_returns_none() {
        let chain = vec![prismatic_x(), prismatic_x()];
        let short = vec![JointValue::Scalar(0.3)];
        let long = vec![
            JointValue::Scalar(0.3),
            JointValue::Scalar(0.5),
            JointValue::Scalar(0.1),
        ];
        assert!(super::chain_transform(&chain, &short).is_none());
        assert!(super::chain_transform(&chain, &long).is_none());
    }

    // ── loop_residual_twist tests ────────────────────────────────────────

    #[test]
    fn loop_residual_twist_identical_chains_zero() {
        let a = vec![prismatic_x()];
        let b = vec![prismatic_x()];
        let vals_a = vec![JointValue::Scalar(0.5)];
        let vals_b = vec![JointValue::Scalar(0.5)];
        let twist: [f64; 6] =
            super::loop_residual_twist(&a, &vals_a, &b, &vals_b).expect("twist");
        for v in twist.iter() {
            assert!(v.abs() < 1e-12, "expected zero twist, got {twist:?}");
        }
    }

    #[test]
    fn loop_residual_twist_prismatic_diff_in_x() {
        // chain_a = prismatic_x at 0.5m, chain_b = prismatic_x at 0.3m.
        // T_a inverse * T_b = pure translation (-0.2, 0, 0). log of that is
        // a twist with angular = 0 and linear = (-0.2, 0, 0).
        let a = vec![prismatic_x()];
        let b = vec![prismatic_x()];
        let vals_a = vec![JointValue::Scalar(0.5)];
        let vals_b = vec![JointValue::Scalar(0.3)];
        let twist =
            super::loop_residual_twist(&a, &vals_a, &b, &vals_b).expect("twist");
        // [ω_x, ω_y, ω_z, v_x, v_y, v_z]
        assert!(twist[0].abs() < 1e-12);
        assert!(twist[1].abs() < 1e-12);
        assert!(twist[2].abs() < 1e-12);
        assert!(
            (twist[3] + 0.2).abs() < 1e-12,
            "expected v_x ≈ -0.2, got {}",
            twist[3]
        );
        assert!(twist[4].abs() < 1e-12);
        assert!(twist[5].abs() < 1e-12);
    }

    #[test]
    fn loop_residual_twist_two_joint_identical_chains_zero() {
        let a = vec![prismatic_x(), revolute_z()];
        let b = vec![prismatic_x(), revolute_z()];
        let vals_a = vec![
            JointValue::Scalar(0.5),
            JointValue::Scalar(std::f64::consts::FRAC_PI_2),
        ];
        let vals_b = vec![
            JointValue::Scalar(0.5),
            JointValue::Scalar(std::f64::consts::FRAC_PI_2),
        ];
        let twist: [f64; 6] =
            super::loop_residual_twist(&a, &vals_a, &b, &vals_b).expect("twist");
        for v in twist.iter() {
            assert!(v.abs() < 1e-10, "expected ~zero twist, got {twist:?}");
        }
    }

    #[test]
    fn loop_residual_twist_length_mismatch_returns_none() {
        let a = vec![prismatic_x(), prismatic_x()];
        let b = vec![prismatic_x()];
        let vals_one = vec![JointValue::Scalar(0.5)];
        let vals_one_short = vec![JointValue::Scalar(0.3)];
        let vals_two = vec![JointValue::Scalar(0.3), JointValue::Scalar(0.1)];
        // chain_a length mismatches vals_a
        assert!(super::loop_residual_twist(&a, &vals_one, &b, &vals_one_short).is_none());
        // chain_b length mismatches vals_b
        assert!(super::loop_residual_twist(&b, &vals_one, &b, &vals_two).is_none());
    }

    // ── joint_range_midpoint tests ───────────────────────────────────────

    #[test]
    fn joint_range_midpoint_prismatic_0_to_1m() {
        // KCC-γ: widened return type — single-DOF kinds wrap their midpoint
        // in `JointValue::Scalar`.
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let mid = super::joint_range_midpoint(&j).expect("midpoint");
        match mid {
            JointValue::Scalar(s) => assert!((s - 0.5).abs() < 1e-12),
            other => panic!("expected Scalar(0.5), got {other:?}"),
        }
    }

    #[test]
    fn joint_range_midpoint_prismatic_neg_to_pos() {
        let j = eval_builtin(
            "prismatic",
            &[
                axis_x_unit(),
                Value::Range {
                    lower: Some(Box::new(Value::length(-2.0))),
                    upper: Some(Box::new(Value::length(2.0))),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
            ],
        );
        let mid = super::joint_range_midpoint(&j).expect("midpoint");
        match mid {
            JointValue::Scalar(s) => assert!(s.abs() < 1e-12),
            other => panic!("expected Scalar(0), got {other:?}"),
        }
    }

    #[test]
    fn joint_range_midpoint_revolute_0_to_pi() {
        let j = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let mid = super::joint_range_midpoint(&j).expect("midpoint");
        match mid {
            JointValue::Scalar(s) => {
                assert!((s - std::f64::consts::FRAC_PI_2).abs() < 1e-12)
            }
            other => panic!("expected Scalar(π/2), got {other:?}"),
        }
    }

    #[test]
    fn joint_range_midpoint_revolute_neg_pi_2_to_pi_2() {
        let j = eval_builtin(
            "revolute",
            &[
                axis_z_unit(),
                Value::Range {
                    lower: Some(Box::new(Value::angle(-std::f64::consts::FRAC_PI_2))),
                    upper: Some(Box::new(Value::angle(std::f64::consts::FRAC_PI_2))),
                    lower_inclusive: true,
                    upper_inclusive: true,
                },
            ],
        );
        let mid = super::joint_range_midpoint(&j).expect("midpoint");
        match mid {
            JointValue::Scalar(s) => assert!(s.abs() < 1e-12),
            other => panic!("expected Scalar(0), got {other:?}"),
        }
    }

    #[test]
    fn joint_range_midpoint_coupling_delegates_to_parent() {
        let parent = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let coupling = eval_builtin("couple", &[parent, Value::Real(2.0)]);
        let mid = super::joint_range_midpoint(&coupling).expect("midpoint");
        match mid {
            JointValue::Scalar(s) => assert!(
                (s - std::f64::consts::FRAC_PI_2).abs() < 1e-12,
                "expected π/2, got {s}"
            ),
            other => panic!("expected Scalar(π/2), got {other:?}"),
        }
    }

    #[test]
    fn joint_range_midpoint_missing_range_returns_none() {
        // Build a Map with a "kind" but no "range" key.
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        let j = Value::Map(m);
        assert!(super::joint_range_midpoint(&j).is_none());
    }

    #[test]
    fn joint_range_midpoint_unbounded_returns_none() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        m.insert(
            Value::String("range".to_string()),
            Value::Range {
                lower: Some(Box::new(Value::length(0.0))),
                upper: None,
                lower_inclusive: true,
                upper_inclusive: false,
            },
        );
        let j = Value::Map(m);
        assert!(super::joint_range_midpoint(&j).is_none());
    }

    // ── per_joint_jacobian_local tests ───────────────────────────────────

    #[test]
    fn per_joint_jacobian_local_prismatic_x_unit() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let col = super::per_joint_jacobian_local(&j).expect("col");
        // [ω; v]: angular zero, linear = unit X
        assert!(col[0].abs() < 1e-12);
        assert!(col[1].abs() < 1e-12);
        assert!(col[2].abs() < 1e-12);
        assert!((col[3] - 1.0).abs() < 1e-12);
        assert!(col[4].abs() < 1e-12);
        assert!(col[5].abs() < 1e-12);
    }

    #[test]
    fn per_joint_jacobian_local_prismatic_unnormalized_axis() {
        // axis [3, 4, 0] has magnitude 5; normalized → [0.6, 0.8, 0]
        let axis = Value::Vector(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        let j = eval_builtin("prismatic", &[axis, length_range_0_to_1m()]);
        let col = super::per_joint_jacobian_local(&j).expect("col");
        assert!(col[0].abs() < 1e-12);
        assert!(col[1].abs() < 1e-12);
        assert!(col[2].abs() < 1e-12);
        assert!((col[3] - 0.6).abs() < 1e-12);
        assert!((col[4] - 0.8).abs() < 1e-12);
        assert!(col[5].abs() < 1e-12);
    }

    #[test]
    fn per_joint_jacobian_local_revolute_z() {
        let j = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let col = super::per_joint_jacobian_local(&j).expect("col");
        // angular = unit Z, linear zero
        assert!(col[0].abs() < 1e-12);
        assert!(col[1].abs() < 1e-12);
        assert!((col[2] - 1.0).abs() < 1e-12);
        assert!(col[3].abs() < 1e-12);
        assert!(col[4].abs() < 1e-12);
        assert!(col[5].abs() < 1e-12);
    }

    #[test]
    fn per_joint_jacobian_local_coupling_revolute_ratio_2() {
        let parent = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let coupling = eval_builtin("couple", &[parent, Value::Real(2.0)]);
        let col = super::per_joint_jacobian_local(&coupling).expect("col");
        // ratio * parent_jac = ratio * [0,0,1, 0,0,0] = [0,0,2, 0,0,0]
        assert!(col[0].abs() < 1e-12);
        assert!(col[1].abs() < 1e-12);
        assert!((col[2] - 2.0).abs() < 1e-12);
        assert!(col[3].abs() < 1e-12);
        assert!(col[4].abs() < 1e-12);
        assert!(col[5].abs() < 1e-12);
    }

    #[test]
    fn per_joint_jacobian_local_unknown_kind_returns_none() {
        // "cylindrical" is intentionally not in JOINT_KINDS — the v0.2 PRD
        // tracks it as a future kind (see #2670). Any string not in
        // `JOINT_KINDS` will exercise the unknown-kind path; "cylindrical"
        // is preferred over an arbitrary placeholder so the test reads as a
        // realistic future-kind probe rather than an artificial fixture.
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("cylindrical".to_string()),
        );
        let j = Value::Map(m);
        assert!(super::per_joint_jacobian_local(&j).is_none());
    }

    #[test]
    fn per_joint_jacobian_local_missing_axis_returns_none() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        let j = Value::Map(m);
        assert!(super::per_joint_jacobian_local(&j).is_none());
    }

    #[test]
    fn per_joint_jacobian_local_non_map_returns_none() {
        assert!(super::per_joint_jacobian_local(&Value::Real(0.5)).is_none());
    }

    /// The "fixed" arm of `joint_jacobian_value` (joints.rs:336) must return a
    /// zero-magnitude Map — i.e. `Some([0; 6])` — not `Undef` (which would
    /// produce `None` here).  This pins the contract so a future change to that
    /// arm (e.g. returning `Undef`) cannot silently break callers that rely on
    /// `Some` with a zero twist column.
    #[test]
    fn per_joint_jacobian_local_fixed_returns_zero_column() {
        let col = super::per_joint_jacobian_local(&fixed_joint())
            .expect("per_joint_jacobian_local must return Some for a fixed joint");
        for v in col.iter() {
            assert!(v.abs() < 1e-12, "expected zero entry, got {v}");
        }
    }

    /// Regression pin (KCC-γ step-5): `per_joint_jacobian_local` returns `None`
    /// for a planar joint because the analytic Jacobian is a `Value::List` of
    /// three per-DOF columns (joints.rs:785 post-KCC-γ), not a single Map.
    /// `twist_map_to_array` fails on the List shape, which is the contract that
    /// triggers FD-fallback chain composition in `chain_jacobian_fd`. Mirrors
    /// the unwritten cylindrical equivalent — preventing a future change to
    /// joint_jacobian_value that flattens the multi-column List into a single
    /// (incorrect) Map without us noticing.
    #[test]
    fn per_joint_jacobian_local_planar_returns_none() {
        let pj = planar_xy_joint();
        assert!(
            super::per_joint_jacobian_local(&pj).is_none(),
            "per_joint_jacobian_local(planar) must return None — the analytic \
             joint_jacobian for planar is a Value::List of 3 per-DOF columns \
             (the FD-fallback trigger), not a single Map. If this test fails, \
             either joints.rs:785 collapsed the multi-column List into a single \
             Map (incorrect), or per_joint_jacobian_local was widened to \
             unpack multi-column Lists (which would change the chain Jacobian \
             contract — see KCC-γ task 3843)."
        );
    }

    /// Regression pin (KCC-γ step-7): same as planar above but for spherical.
    /// The analytic spherical Jacobian is a Value::List of 3 body-basis columns
    /// (joints.rs:800 post-KCC-γ); per_joint_jacobian_local must return None to
    /// trigger FD-fallback chain composition.
    #[test]
    fn per_joint_jacobian_local_spherical_returns_none() {
        let sj = spherical_joint();
        assert!(
            super::per_joint_jacobian_local(&sj).is_none(),
            "per_joint_jacobian_local(spherical) must return None — the analytic \
             joint_jacobian for spherical is a Value::List of 3 body-basis columns \
             (the FD-fallback trigger), not a single Map. See KCC-γ task 3843."
        );
    }

    // ── chain_jacobian_fd tests ──────────────────────────────────────────

    fn assert_columns_close(actual: &[[f64; 6]], expected: &[[f64; 6]], tol: f64, label: &str) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{label}: column count mismatch"
        );
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            for k in 0..6 {
                assert!(
                    (a[k] - e[k]).abs() < tol,
                    "{label}: col[{i}][{k}] expected {}, got {} (diff {})",
                    e[k],
                    a[k],
                    (a[k] - e[k]).abs(),
                );
            }
        }
    }

    #[test]
    fn chain_jacobian_fd_single_prismatic_matches_analytic() {
        let chain = vec![prismatic_x()];
        let analytic = super::per_joint_jacobian_local(&chain[0]).unwrap();
        let vals = vec![JointValue::Scalar(0.5)];
        let cols = super::chain_jacobian_fd(&chain, &vals, &[0], 1e-6).expect("cols");
        assert_eq!(cols.len(), 1);
        assert_columns_close(&cols, &[analytic], 1e-7, "single_prismatic");
    }

    #[test]
    fn chain_jacobian_fd_single_revolute_matches_analytic() {
        let chain = vec![revolute_z()];
        let analytic = super::per_joint_jacobian_local(&chain[0]).unwrap();
        let vals = vec![JointValue::Scalar(0.0)];
        let cols = super::chain_jacobian_fd(&chain, &vals, &[0], 1e-6).expect("cols");
        assert_eq!(cols.len(), 1);
        assert_columns_close(&cols, &[analytic], 1e-7, "single_revolute");
    }

    #[test]
    fn chain_jacobian_fd_two_joint_returns_two_columns() {
        let chain = vec![prismatic_x(), revolute_z()];
        let vals = vec![JointValue::Scalar(0.5), JointValue::Scalar(0.0)];
        let cols = super::chain_jacobian_fd(&chain, &vals, &[0, 1], 1e-6).expect("cols");
        assert_eq!(cols.len(), 2);
        for col in &cols {
            for v in col.iter() {
                assert!(v.is_finite(), "expected finite, got {col:?}");
            }
        }
    }

    #[test]
    fn chain_jacobian_fd_out_of_range_returns_none() {
        let chain = vec![prismatic_x()];
        let vals = vec![JointValue::Scalar(0.5)];
        assert!(super::chain_jacobian_fd(&chain, &vals, &[5], 1e-6).is_none());
    }

    #[test]
    fn chain_jacobian_fd_zero_eps_returns_none() {
        let chain = vec![prismatic_x()];
        let vals = vec![JointValue::Scalar(0.5)];
        assert!(super::chain_jacobian_fd(&chain, &vals, &[0], 0.0).is_none());
    }

    #[test]
    fn chain_jacobian_fd_undef_chain_returns_none() {
        let mut bogus = std::collections::BTreeMap::new();
        bogus.insert(
            Value::String("kind".to_string()),
            Value::String("bogus".to_string()),
        );
        let chain = vec![Value::Map(bogus)];
        let vals = vec![JointValue::Scalar(0.5)];
        assert!(super::chain_jacobian_fd(&chain, &vals, &[0], 1e-6).is_none());
    }

    #[test]
    fn chain_jacobian_fd_two_joint_only_second_free_axis_aligned() {
        // chain = [prismatic_x at 0.5m, revolute_z at 0]
        // free index 1 (revolute_z) → column should be axis-aligned around Z
        // (angular ≈ [0,0,1], linear close to zero — exact form depends on
        // the SE(3) chain's left-Jacobian at the trunk; we only assert finite,
        // axis-aligned structure here).
        let chain = vec![prismatic_x(), revolute_z()];
        let vals = vec![JointValue::Scalar(0.5), JointValue::Scalar(0.0)];
        let cols = super::chain_jacobian_fd(&chain, &vals, &[1], 1e-6).expect("cols");
        assert_eq!(cols.len(), 1);
        let col = cols[0];
        for v in col.iter() {
            assert!(v.is_finite(), "expected finite, got {col:?}");
        }
        // Angular: dominant Z component (within 1e-4 of unity at small angle).
        assert!(
            (col[2] - 1.0).abs() < 1e-4,
            "expected angular Z ≈ 1, got {}",
            col[2]
        );
    }

    #[test]
    fn joint_range_midpoint_non_map_returns_none() {
        assert!(super::joint_range_midpoint(&Value::Real(0.5)).is_none());
    }

    #[test]
    fn loop_residual_twist_undef_chain_returns_none() {
        // Hand-built joint Map with bogus kind triggers chain_transform → None.
        let mut bogus = std::collections::BTreeMap::new();
        bogus.insert(
            Value::String("kind".to_string()),
            Value::String("bogus".to_string()),
        );
        let a = vec![Value::Map(bogus)];
        let b = vec![prismatic_x()];
        let vals = vec![JointValue::Scalar(0.0)];
        assert!(super::loop_residual_twist(&a, &vals, &b, &vals).is_none());
    }

    #[test]
    fn chain_transform_invalid_kind_returns_none() {
        // Hand-built joint Map with bogus kind
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("bogus".to_string()),
        );
        let chain = vec![Value::Map(m)];
        let vals = vec![JointValue::Scalar(0.5)];
        assert!(super::chain_transform(&chain, &vals).is_none());
    }

    // ── fixed-joint chain integration tests ─────────────────────────────

    fn fixed_joint() -> Value {
        eval_builtin("fixed", &[])
    }

    /// A chain containing only a fixed joint must produce the identity
    /// Transform — a fixed joint contributes no translation or rotation.
    #[test]
    fn chain_transform_single_fixed_joint_returns_identity() {
        let chain = vec![fixed_joint()];
        let vals = vec![JointValue::Scalar(0.0)];
        let result = super::chain_transform(&chain, &vals)
            .expect("chain_transform must return Some for a fixed joint");
        let trans = translation_xyz(&result);
        assert!(
            trans[0].abs() < 1e-12 && trans[1].abs() < 1e-12 && trans[2].abs() < 1e-12,
            "expected zero translation, got {trans:?}"
        );
        let (w, x, y, z) = rotation_wxyz(&result);
        assert!(
            (w - 1.0).abs() < 1e-12,
            "expected w ≈ 1.0 (identity rotation), got {w}"
        );
        assert!(
            x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12,
            "expected x,y,z ≈ 0 (identity rotation), got {x},{y},{z}"
        );
    }

    /// A fixed joint mid-chain contributes identity: the net translation must
    /// equal the sum of the two prismatic joints on either side of it.
    /// The garbage scalar (1234.5) for the fixed slot proves value_for_joint
    /// discards the input — the result is the same as passing 0.0.
    #[test]
    fn chain_transform_fixed_joint_in_middle_acts_as_identity_contribution() {
        // [prismatic_x @ 0.3m, fixed @ 1234.5 (ignored), prismatic_x @ 0.5m]
        // Expected: total translation_x ≈ 0.8m regardless of the fixed slot's value.
        let chain = vec![prismatic_x(), fixed_joint(), prismatic_x()];
        // The middle Scalar(1234.5) is the fixed joint's placeholder slot —
        // value_for_joint discards it.  Keeping the garbage scalar in the
        // JointValue slot proves the widened signature preserves the same
        // "ignored value" semantics the test originally pinned.
        let vals = vec![
            JointValue::Scalar(0.3),
            JointValue::Scalar(1234.5),
            JointValue::Scalar(0.5),
        ];
        let result = super::chain_transform(&chain, &vals)
            .expect("chain_transform must return Some with fixed joint in the middle");
        let trans = translation_xyz(&result);
        assert!(
            (trans[0] - 0.8).abs() < 1e-12,
            "expected translation_x = 0.8, got {}",
            trans[0]
        );
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
    }

    /// A fixed joint in the chain but NOT in free_indices must not prevent
    /// chain_jacobian_fd from assembling the other (free) columns.
    ///
    /// Strengthened: we also assert that the resulting columns equal those of a
    /// reference chain with the fixed joint removed entirely (free indices
    /// re-indexed to [0, 1]).  This cross-chain comparison proves the fixed
    /// joint contributes exactly identity to chain composition — not merely
    /// that the output is finite.
    #[test]
    fn chain_jacobian_fd_with_fixed_joint_outside_free_indices() {
        // chain = [prismatic_x, fixed, revolute_z]
        // free_indices = [0, 2] (the two DOF joints — fixed at index 1 is not free)
        let chain = vec![prismatic_x(), fixed_joint(), revolute_z()];
        let vals = vec![
            JointValue::Scalar(0.5),
            JointValue::Scalar(0.0),
            JointValue::Scalar(0.0),
        ];
        let cols = super::chain_jacobian_fd(&chain, &vals, &[0, 2], 1e-6)
            .expect("chain_jacobian_fd must return Some when fixed joint is not in free_indices");
        assert_eq!(cols.len(), 2, "expected 2 columns for 2 free indices");
        for col in &cols {
            for v in col.iter() {
                assert!(
                    v.is_finite(),
                    "expected all column entries to be finite, got {col:?}"
                );
            }
        }
        // Cross-chain reference: same joints without the fixed slot; free indices [0, 1].
        let vals_ref = vec![JointValue::Scalar(0.5), JointValue::Scalar(0.0)];
        let cols_no_fixed = super::chain_jacobian_fd(
            &[prismatic_x(), revolute_z()],
            &vals_ref,
            &[0, 1],
            1e-6,
        )
        .expect("chain_jacobian_fd reference (no fixed) must return Some");
        assert_columns_close(
            &cols,
            &cols_no_fixed,
            1e-7,
            "fixed_joint_outside_free_indices_vs_reference",
        );
    }

    /// A fixed joint listed in free_indices must produce a zero-twist column.
    /// `value_for_joint` drops the perturbed scalar and returns `Real(0.0)` for
    /// both the +eps and −eps evaluations, so `transform_at` receives identical
    /// inputs both times — `T_plus == T_minus == identity` — and the
    /// central-difference quotient is exactly 0.
    #[test]
    fn chain_jacobian_fd_with_fixed_joint_in_free_indices_yields_zero_column() {
        let chain = vec![fixed_joint()];
        let vals = vec![JointValue::Scalar(0.0)];
        let cols = super::chain_jacobian_fd(&chain, &vals, &[0], 1e-6)
            .expect("chain_jacobian_fd must return Some for a fixed-only chain");
        assert_eq!(cols.len(), 1, "expected 1 column");
        let col = cols[0];
        for (k, &v) in col.iter().enumerate() {
            assert!(
                v.abs() < 1e-10,
                "expected zero-twist column entry [{}], got {}",
                k,
                v
            );
        }
    }

    // ── planar joint widened tests (KCC-γ step-3) ────────────────────────

    /// `joint_range_midpoint` returns `Some(JointValue::Planar([mid_x, mid_y, mid_θ]))`
    /// for a planar joint.
    ///
    /// KCC-γ widening (PRD §5.2): planar's 3-DOF free-variable space has three
    /// independent range midpoints — wrapped as a `JointValue::Planar([x, y, θ])`
    /// triple in canonical storage order (matches `JointValue::Planar` layout
    /// and `transform_at("planar", _)`'s `Value::List([length, length, angle])`
    /// surface).  `planar_xy_joint()` uses two 0..1m length ranges and a 0..π
    /// angle range, so midpoints are `[0.5, 0.5, π/2]`.
    #[test]
    fn joint_range_midpoint_planar_returns_planar_midpoint() {
        let mid = super::joint_range_midpoint(&planar_xy_joint())
            .expect("joint_range_midpoint must return Some(Planar(..)) for a planar joint");
        match mid {
            JointValue::Planar([x, y, theta]) => {
                assert!((x - 0.5).abs() < 1e-12, "expected x = 0.5, got {x}");
                assert!((y - 0.5).abs() < 1e-12, "expected y = 0.5, got {y}");
                assert!(
                    (theta - std::f64::consts::FRAC_PI_2).abs() < 1e-12,
                    "expected θ = π/2, got {theta}"
                );
            }
            other => panic!("expected JointValue::Planar([0.5, 0.5, π/2]), got {other:?}"),
        }
    }

    /// KCC-γ (PRD §5.2): `value_for_joint(planar, Planar([x,y,θ]))` returns
    /// the canonical 3-element `Value::List([length(x), length(y), angle(θ)])`
    /// surface that `transform_at("planar", _)` consumes.
    #[test]
    fn value_for_joint_planar_returns_value_list() {
        let jv = JointValue::Planar([0.5, 0.5, std::f64::consts::FRAC_PI_2]);
        let result = super::value_for_joint(&planar_xy_joint(), &jv)
            .expect("value_for_joint must wrap a planar JointValue into a 3-element List");
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3, "expected 3-element list, got {items:?}");
                assert_eq!(items[0], Value::length(0.5));
                assert_eq!(items[1], Value::length(0.5));
                assert_eq!(items[2], Value::angle(std::f64::consts::FRAC_PI_2));
            }
            other => panic!("expected Value::List, got {other:?}"),
        }
    }

    /// KCC-γ: a chain containing only a planar joint with a non-zero x
    /// motion variable composes to a pure +X translation Transform with
    /// identity rotation.
    #[test]
    fn chain_transform_planar_only_chain_returns_transform() {
        let chain = vec![planar_xy_joint()];
        let vals = vec![JointValue::Planar([0.5, 0.0, 0.0])];
        let result = super::chain_transform(&chain, &vals)
            .expect("chain_transform must return Some for a planar-only chain");
        let trans = translation_xyz(&result);
        assert!((trans[0] - 0.5).abs() < 1e-12, "expected tx = 0.5, got {}", trans[0]);
        assert!(trans[1].abs() < 1e-12, "expected ty = 0, got {}", trans[1]);
        assert!(trans[2].abs() < 1e-12, "expected tz = 0, got {}", trans[2]);
        let (w, x, y, z) = rotation_wxyz(&result);
        assert!((w - 1.0).abs() < 1e-12, "expected identity rotation w=1, got {w}");
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12);
    }

    /// KCC-γ: a mixed `[prismatic_x @ 0.5m, planar_xy @ (0.25, 0, 0)]` chain
    /// composes the two translations along +X for total 0.75m.
    #[test]
    fn chain_transform_mixed_prismatic_planar_returns_transform() {
        let chain = vec![prismatic_x(), planar_xy_joint()];
        let vals = vec![
            JointValue::Scalar(0.5),
            JointValue::Planar([0.25, 0.0, 0.0]),
        ];
        let result = super::chain_transform(&chain, &vals)
            .expect("chain_transform must return Some for a mixed prismatic+planar chain");
        let trans = translation_xyz(&result);
        assert!((trans[0] - 0.75).abs() < 1e-12, "expected tx = 0.75, got {}", trans[0]);
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
    }

    /// KCC-γ: `chain_jacobian_fd` for a planar-only chain returns 3 columns
    /// (one per planar storage f64 — flat_len = 3).  This matches the
    /// Newton-state-width-by-flat_len contract used by `solve_loop_closure`
    /// (step-12).  Per-column finiteness is the only structural assertion
    /// here; sign/magnitude pins are deferred to the analytic-J widening
    /// in step-6 (joints.rs:785 planar arm).
    #[test]
    fn chain_jacobian_fd_planar_only_emits_three_columns() {
        let chain = vec![planar_xy_joint()];
        let vals = vec![JointValue::Planar([0.0, 0.0, 0.0])];
        let cols = super::chain_jacobian_fd(&chain, &vals, &[0], 1e-6)
            .expect("chain_jacobian_fd must return Some for a planar-only chain");
        assert_eq!(cols.len(), 3, "planar slot must produce flat_len=3 columns");
        for col in &cols {
            for v in col.iter() {
                assert!(v.is_finite(), "expected finite, got {col:?}");
            }
        }
    }

    /// KCC-γ: `loop_residual_twist` on two identical planar chains is zero.
    /// Sanity check on the multi-DOF residual path.
    #[test]
    fn loop_residual_twist_planar_chain_zero_for_identical_configurations() {
        let a = vec![planar_xy_joint()];
        let b = vec![planar_xy_joint()];
        let vals = vec![JointValue::Planar([0.3, 0.4, std::f64::consts::FRAC_PI_3])];
        let twist = super::loop_residual_twist(&a, &vals, &b, &vals)
            .expect("loop_residual_twist must return Some for identical planar chains");
        for v in twist.iter() {
            assert!(v.abs() < 1e-10, "expected ~zero twist, got {twist:?}");
        }
    }

    // ── spherical joint widened tests (KCC-γ step-3) ──────────────────────

    /// `joint_range_midpoint` returns `Some(JointValue::Sphere([1, 0, 0, 0]))`
    /// for a spherical joint — the identity quaternion (PRD §5.2).
    ///
    /// KCC-γ widening: spherical is axis-isotropic (no preferred direction is
    /// stored), so the natural seed for fresh-snapshot start values is the
    /// identity rotation `q = (w=1, x=0, y=0, z=0)`.  The `range_angle` bound
    /// constrains rotation magnitude downstream during solver iteration; the
    /// midpoint seed only needs to be a valid (unit-norm) quaternion on S³.
    #[test]
    fn joint_range_midpoint_spherical_returns_identity_quaternion() {
        let mid = super::joint_range_midpoint(&spherical_joint())
            .expect("joint_range_midpoint must return Some(Sphere(..)) for a spherical joint");
        match mid {
            JointValue::Sphere([w, x, y, z]) => {
                assert!((w - 1.0).abs() < 1e-12, "expected w = 1, got {w}");
                assert!(x.abs() < 1e-12, "expected x = 0, got {x}");
                assert!(y.abs() < 1e-12, "expected y = 0, got {y}");
                assert!(z.abs() < 1e-12, "expected z = 0, got {z}");
            }
            other => panic!("expected JointValue::Sphere([1, 0, 0, 0]), got {other:?}"),
        }
    }

    /// KCC-γ (PRD §5.2): `value_for_joint(spherical, Sphere([w,x,y,z]))` returns
    /// the canonical `Value::Orientation { w, x, y, z }` surface that
    /// `transform_at("spherical", _)` consumes.
    #[test]
    fn value_for_joint_spherical_returns_value_orientation() {
        // Use a non-identity unit quaternion (rotate +π/2 about Z):
        //   w = cos(π/4), x = 0, y = 0, z = sin(π/4)
        let half = std::f64::consts::FRAC_PI_4;
        let jv = JointValue::Sphere([half.cos(), 0.0, 0.0, half.sin()]);
        let result = super::value_for_joint(&spherical_joint(), &jv)
            .expect("value_for_joint must wrap a spherical JointValue into Value::Orientation");
        match result {
            Value::Orientation { w, x, y, z } => {
                assert!((w - half.cos()).abs() < 1e-12, "expected w ≈ cos(π/4), got {w}");
                assert!(x.abs() < 1e-12);
                assert!(y.abs() < 1e-12);
                assert!((z - half.sin()).abs() < 1e-12);
            }
            other => panic!("expected Value::Orientation, got {other:?}"),
        }
    }

    /// KCC-γ: a spherical-only chain composed at the identity quaternion
    /// yields the identity Transform.
    #[test]
    fn chain_transform_spherical_only_chain_returns_transform() {
        let chain = vec![spherical_joint()];
        let vals = vec![JointValue::Sphere([1.0, 0.0, 0.0, 0.0])];
        let result = super::chain_transform(&chain, &vals)
            .expect("chain_transform must return Some for a spherical-only chain");
        let trans = translation_xyz(&result);
        assert!(trans[0].abs() < 1e-12);
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
        let (w, x, y, z) = rotation_wxyz(&result);
        assert!((w - 1.0).abs() < 1e-12);
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12);
    }

    // ── cylindrical joint widened tests (KCC-γ step-3) ────────────────────

    /// `joint_range_midpoint` returns `Some(JointValue::Cyl([mid_d, mid_θ]))`
    /// for a cylindrical joint.
    ///
    /// KCC-γ widening (PRD §5.2): cylindrical is 2-DOF (translation along axis,
    /// rotation about axis); the midpoints of `translation_range` and
    /// `rotation_range` form the canonical seed.  `cylindrical_z_joint()` uses
    /// a 0..1m translation range and a 0..π rotation range, so midpoints are
    /// `[0.5, π/2]`.
    #[test]
    fn joint_range_midpoint_cylindrical_returns_cyl_midpoint() {
        let mid = super::joint_range_midpoint(&cylindrical_z_joint())
            .expect("joint_range_midpoint must return Some(Cyl(..)) for a cylindrical joint");
        match mid {
            JointValue::Cyl([d, theta]) => {
                assert!((d - 0.5).abs() < 1e-12, "expected d = 0.5, got {d}");
                assert!(
                    (theta - std::f64::consts::FRAC_PI_2).abs() < 1e-12,
                    "expected θ = π/2, got {theta}"
                );
            }
            other => panic!("expected JointValue::Cyl([0.5, π/2]), got {other:?}"),
        }
    }

    /// KCC-γ (PRD §5.2): `value_for_joint(cylindrical, Cyl([d,θ]))` returns
    /// the canonical 2-element `Value::List([length(d), angle(θ)])` surface
    /// that `transform_at("cylindrical", _)` consumes.
    #[test]
    fn value_for_joint_cylindrical_returns_value_list() {
        let jv = JointValue::Cyl([0.5, std::f64::consts::FRAC_PI_2]);
        let result = super::value_for_joint(&cylindrical_z_joint(), &jv)
            .expect("value_for_joint must wrap a cylindrical JointValue into a 2-element List");
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 2, "expected 2-element list, got {items:?}");
                assert_eq!(items[0], Value::length(0.5));
                assert_eq!(items[1], Value::angle(std::f64::consts::FRAC_PI_2));
            }
            other => panic!("expected Value::List, got {other:?}"),
        }
    }

    /// KCC-γ: a cylindrical-only chain composed at d=0.5m, θ=0 yields a pure
    /// translation along the joint's axis (+Z), identity rotation.
    #[test]
    fn chain_transform_cylindrical_only_chain_returns_transform() {
        let chain = vec![cylindrical_z_joint()];
        let vals = vec![JointValue::Cyl([0.5, 0.0])];
        let result = super::chain_transform(&chain, &vals)
            .expect("chain_transform must return Some for a cylindrical-only chain");
        let trans = translation_xyz(&result);
        assert!(trans[0].abs() < 1e-12);
        assert!(trans[1].abs() < 1e-12);
        assert!((trans[2] - 0.5).abs() < 1e-12, "expected tz = 0.5, got {}", trans[2]);
        let (w, x, y, z) = rotation_wxyz(&result);
        assert!((w - 1.0).abs() < 1e-12);
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12);
    }

    // ── widened JOINT_KINDS-iteration partition (KCC-γ step-3) ────────────
    //
    // Pins the contract that `value_for_joint` and `joint_range_midpoint`
    // partition `crate::joints::JOINT_KINDS` cleanly after the KCC-γ widening:
    //   - `value_for_joint`: every kind returns Some when the JointValue
    //     variant matches the kind (Scalar for prismatic/revolute/coupling/fixed,
    //     Planar for planar, Sphere for spherical, Cyl for cylindrical).
    //   - `joint_range_midpoint`: every kind returns Some EXCEPT `fixed`
    //     (0-DOF, empty free-variable space).
    //
    // Any future kind addition is forced through this partition: an unhandled
    // kind triggers `minimal_joint`'s panic (with a remediation message) and
    // the assertions cleanly fail if the classification needs updating.

    /// Build a minimal well-formed joint for each kind in JOINT_KINDS.
    /// Mirrors `joints::tests::joint_kind_minimal_fixture` but joint-only
    /// (no value_arg pairing) — a separate copy lives here so loop_closure's
    /// test module is self-contained without leaking joints.rs's private
    /// fixture helper.
    fn minimal_joint(kind: &str) -> Value {
        match kind {
            "prismatic" => prismatic_x(),
            "revolute" => revolute_z(),
            "coupling" => eval_builtin("couple", &[prismatic_x(), Value::Real(1.0)]),
            "fixed" => eval_builtin("fixed", &[]),
            "planar" => planar_xy_joint(),
            "spherical" => spherical_joint(),
            "cylindrical" => cylindrical_z_joint(),
            _ => panic!(
                "minimal_joint: unknown kind '{kind}' — JOINT_KINDS contains a \
                 kind that loop_closure's tests have no fixture for. Add a \
                 fixture row here and decide which JointValue variant pairs \
                 with the new kind."
            ),
        }
    }

    /// Per-kind canonical JointValue used by the post-γ partition tests.
    /// Mirrors `joint_range_midpoint`'s per-kind output shape — every kind in
    /// `JOINT_KINDS` has a matching JointValue variant (Scalar for the four
    /// single-DOF kinds; Planar/Sphere/Cyl for the three multi-DOF kinds).
    /// Used by `value_for_joint_partition_covers_joint_kinds` to drive the
    /// widened `value_for_joint(&Value, &JointValue)` signature.
    fn minimal_jointvalue(kind: &str) -> JointValue {
        match kind {
            "prismatic" | "revolute" | "coupling" | "fixed" => JointValue::Scalar(0.0),
            "planar" => JointValue::Planar([0.0, 0.0, 0.0]),
            "spherical" => JointValue::Sphere([1.0, 0.0, 0.0, 0.0]),
            "cylindrical" => JointValue::Cyl([0.0, 0.0]),
            _ => panic!(
                "minimal_jointvalue: unknown kind '{kind}' — JOINT_KINDS contains a \
                 kind without a matching JointValue variant. Add a fixture row here."
            ),
        }
    }

    /// KCC-γ widening: multi-DOF joint kinds now return `Some(JointValue::..)`
    /// from `joint_range_midpoint`, with per-DOF range midpoints (planar,
    /// cylindrical) or the identity quaternion (spherical, axis-isotropic).
    /// Only `fixed` (0-DOF, empty free-variable space) still returns `None`.
    #[test]
    fn joint_range_midpoint_returns_some_for_all_multi_dof_kinds() {
        for &kind in MULTI_DOF_KINDS {
            assert!(
                super::joint_range_midpoint(&minimal_joint(kind)).is_some(),
                "joint_range_midpoint must return Some(JointValue::..) for \
                 multi-DOF kind '{kind}' (KCC-γ widening — PRD §5.2)"
            );
        }
    }

    /// KCC-γ widening: every JOINT_KINDS entry returns Some from the widened
    /// `value_for_joint(&Value, &JointValue)` when the JointValue variant
    /// matches the kind.  The old multi-DOF None contract is dropped.
    #[test]
    fn value_for_joint_partition_covers_joint_kinds() {
        use crate::joints::JOINT_KINDS;
        // Subset guard: MULTI_DOF_KINDS must be in JOINT_KINDS.
        for &k in MULTI_DOF_KINDS {
            assert!(
                JOINT_KINDS.contains(&k),
                "MULTI_DOF_KINDS member '{k}' must also be in JOINT_KINDS"
            );
        }
        for &kind in JOINT_KINDS {
            let jv = minimal_jointvalue(kind);
            let result = super::value_for_joint(&minimal_joint(kind), &jv);
            assert!(
                result.is_some(),
                "value_for_joint must return Some for kind '{kind}' with matching \
                 JointValue variant (KCC-γ widening — PRD §5.2)"
            );
        }
    }

    /// Partition contract for `joint_range_midpoint`: returns None for every
    /// kind in `JOINT_RANGE_MIDPOINT_NONE_KINDS`, Some for the rest.
    ///
    /// KCC-γ widening: only `fixed` (0-DOF, no free-variable space to seed)
    /// remains in the None partition; all other kinds — single-DOF
    /// (prismatic/revolute/coupling) and multi-DOF (planar/spherical/
    /// cylindrical) — return `Some(JointValue::..)` with the per-DOF surface
    /// shape matched by `value_for_joint` and the chain machinery.
    ///
    /// Note this partition still differs from `value_for_joint`'s: `fixed`
    /// returns `Some(JointValue::Scalar(0))` from `value_for_joint` (where
    /// the value is discarded) but None from `joint_range_midpoint` (no
    /// free-variable space to seed).
    #[test]
    fn joint_range_midpoint_partition_covers_joint_kinds() {
        use crate::joints::JOINT_KINDS;
        const JOINT_RANGE_MIDPOINT_NONE_KINDS: &[&str] = &["fixed"];
        for &kind in JOINT_KINDS {
            let result = super::joint_range_midpoint(&minimal_joint(kind));
            if JOINT_RANGE_MIDPOINT_NONE_KINDS.contains(&kind) {
                assert!(
                    result.is_none(),
                    "joint_range_midpoint must return None for kind '{kind}'"
                );
            } else {
                let mid = result.unwrap_or_else(|| {
                    panic!("joint_range_midpoint must return Some for kind '{kind}'")
                });
                // Finite-component check across all four JointValue variants —
                // every f64 stored in the per-DOF surface must be finite.
                let all_finite = mid.as_f64_slice().iter().all(|v| v.is_finite());
                assert!(
                    all_finite,
                    "joint_range_midpoint('{kind}') midpoint must be finite, got {mid:?}"
                );
            }
        }
    }

    // ── extract_loop_closure_chains tests ───────────────────────────────────
    //
    // Pure value-side helper: translates a loop_closure Map record + bindings
    // slice into the five solver-input vectors.  The world sentinel at chain
    // head is stripped; per-joint SI values come from `value_for`-style
    // resolution (binding → midpoint fallback); free indices are positions
    // in path_b with no direct binding entry.

    /// Build the canonical world sentinel Map (kind="world") used as the
    /// leading element of a `loop_closure` record's `path_a` / `path_b`.
    /// Mirrors the construction in `mechanism::make_world_sentinel` (private
    /// to mechanism.rs); duplicated here so the test module stays self-
    /// contained rather than reaching across modules for a private helper.
    fn world_sentinel() -> Value {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("world".to_string()),
        );
        Value::Map(m)
    }

    /// Build a `loop_closure` Map record paralleling
    /// `mechanism::make_loop_closure_record`.  Test-local copy so the
    /// loop_closure tests don't depend on mechanism.rs's private helper.
    fn loop_closure_record(path_a: Vec<Value>, path_b: Vec<Value>, closing_joint: Value) -> Value {
        let mut m = std::collections::BTreeMap::new();
        m.insert(Value::String("body_id".to_string()), Value::Int(0));
        m.insert(Value::String("closing_joint".to_string()), closing_joint);
        m.insert(
            Value::String("kind".to_string()),
            Value::String("loop_closure".to_string()),
        );
        m.insert(Value::String("path_a".to_string()), Value::List(path_a));
        m.insert(Value::String("path_b".to_string()), Value::List(path_b));
        Value::Map(m)
    }

    /// `extract_loop_closure_chains` returns the expected five-vector tuple
    /// for a record with `path_a = [world, jA]` (driven by a bound length
    /// of 0.5m) and `path_b = [world, jB]` (free — no binding entry).
    ///
    /// Asserts:
    ///   * chain_a stripped of world sentinel: `[jA]`
    ///   * vals_a populated from binding: `[JointValue::Scalar(0.5)]` (SI metres)
    ///   * chain_b stripped of world sentinel: `[jB]`
    ///   * vals_b_initial seeded from jB's range midpoint:
    ///     `[JointValue::Scalar(0.5)]` (jB has range 0..1m → midpoint 0.5m).
    ///   * free_b indices: `[0]` (the only joint in chain_b is unbound).
    ///
    /// KCC-γ step-9: `LoopClosureSolverInputs` widens vals_a / vals_b_initial
    /// from `Vec<f64>` to `Vec<JointValue>` so multi-DOF closing joints can
    /// flow into the Newton solver without the f64-shim collapsing them to
    /// None.  Single-DOF joints still produce `JointValue::Scalar(..)` for
    /// continuity with the pre-widening test.
    #[test]
    fn extract_loop_closure_chains_returns_chains_vals_and_free_indices() {
        // jA and jB must be structurally distinct Maps so the binding for
        // jA does not also match jB by `Value::Eq` — same-axis prismatic
        // joints would collapse to the same Map and falsely satisfy the
        // direct-binding lookup for jB.  Use jA on +X and jB on +Y so the
        // closing-side joint is unambiguously "free".
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let bind_a = eval_builtin("bind", &[j_a.clone(), Value::length(0.5)]);
        let bindings = vec![bind_a];
        let record = loop_closure_record(
            vec![world_sentinel(), j_a.clone()],
            vec![world_sentinel(), j_b.clone()],
            j_b.clone(),
        );

        let (chain_a, vals_a, chain_b, vals_b_initial, free_b) =
            super::extract_loop_closure_chains(&record, &bindings)
                .expect("extract_loop_closure_chains must return Some for a well-formed record");

        assert_eq!(
            chain_a,
            vec![j_a.clone()],
            "chain_a should strip world sentinel"
        );
        assert_eq!(vals_a.len(), 1, "vals_a length must equal chain_a length");
        match &vals_a[0] {
            JointValue::Scalar(s) => assert!(
                (s - 0.5).abs() < 1e-12,
                "vals_a[0] expected JointValue::Scalar(0.5) (bound), got Scalar({s})"
            ),
            other => panic!("vals_a[0] expected JointValue::Scalar(0.5), got {other:?}"),
        }
        assert_eq!(
            chain_b,
            vec![j_b.clone()],
            "chain_b should strip world sentinel"
        );
        assert_eq!(
            vals_b_initial.len(),
            1,
            "vals_b_initial length must equal chain_b length"
        );
        match &vals_b_initial[0] {
            JointValue::Scalar(s) => assert!(
                (s - 0.5).abs() < 1e-12,
                "vals_b_initial[0] expected JointValue::Scalar(0.5) (midpoint), got Scalar({s})"
            ),
            other => panic!(
                "vals_b_initial[0] expected JointValue::Scalar(0.5), got {other:?}"
            ),
        }
        assert_eq!(free_b, vec![0], "free_b should mark jB (index 0) as free");
    }

    /// KCC-γ step-9: `extract_loop_closure_chains` must populate
    /// `vals_b_initial` with a `JointValue::Planar([mid_x, mid_y, mid_θ])`
    /// surface when a multi-DOF planar joint in chain_b has no direct
    /// binding entry — exactly the case the pre-widening f64-tuple shim
    /// collapsed to None (and through which `snapshot()` short-circuited
    /// to Undef).  Multi-DOF closing joints are the user-observable signal
    /// the KCC-γ widening turns from None to Some.
    #[test]
    fn extract_loop_closure_chains_returns_jointvalue_vectors() {
        // jA is a single-DOF prismatic driven by an explicit binding;
        // jB is a 3-DOF planar joint with NO direct binding.  Under
        // KCC-γ widening, vals_b_initial[0] for jB must be the
        // JointValue::Planar([0.5, 0.5, π/2]) range-midpoint surface
        // (length_range_0_to_1m → midpoint 0.5; angle_range_0_to_pi →
        // midpoint π/2).  Before widening the unbound multi-DOF joint
        // collapsed extract_loop_closure_chains to None.
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = planar_xy_joint();
        let bind_a = eval_builtin("bind", &[j_a.clone(), Value::length(0.5)]);
        let bindings = vec![bind_a];
        let record = loop_closure_record(
            vec![world_sentinel(), j_a.clone()],
            vec![world_sentinel(), j_b.clone()],
            j_b.clone(),
        );

        let (chain_a, vals_a, chain_b, vals_b_initial, free_b) =
            super::extract_loop_closure_chains(&record, &bindings).expect(
                "extract_loop_closure_chains must return Some for a chain_b with an \
                 unbound multi-DOF planar joint (KCC-γ widening — PRD §5.2)",
            );

        // chain_a / vals_a: same single-DOF prismatic shape as the
        // single-DOF test above.
        assert_eq!(chain_a, vec![j_a.clone()]);
        match &vals_a[0] {
            JointValue::Scalar(s) => assert!(
                (s - 0.5).abs() < 1e-12,
                "vals_a[0] expected JointValue::Scalar(0.5), got Scalar({s})"
            ),
            other => panic!("vals_a[0] expected JointValue::Scalar(0.5), got {other:?}"),
        }

        // chain_b carries the planar joint, vals_b_initial[0] is the
        // 3-component planar midpoint surface.
        assert_eq!(chain_b, vec![j_b.clone()]);
        let pi_2 = std::f64::consts::FRAC_PI_2;
        match &vals_b_initial[0] {
            JointValue::Planar([x, y, theta]) => {
                assert!(
                    (x - 0.5).abs() < 1e-12,
                    "vals_b_initial[0].x expected 0.5 (length midpoint), got {x}"
                );
                assert!(
                    (y - 0.5).abs() < 1e-12,
                    "vals_b_initial[0].y expected 0.5 (length midpoint), got {y}"
                );
                assert!(
                    (theta - pi_2).abs() < 1e-12,
                    "vals_b_initial[0].theta expected π/2 (angle midpoint), got {theta}"
                );
            }
            other => panic!(
                "vals_b_initial[0] expected JointValue::Planar([0.5, 0.5, π/2]), got {other:?}"
            ),
        }

        // The planar joint is unbound → marked as free.
        assert_eq!(free_b, vec![0], "free_b should mark planar jB as free");
    }

    /// Negative case: a malformed record missing the `path_b` key collapses
    /// to None.  The arity guard in snapshot.rs's closed-chain arm relies on
    /// this so a bogus loop_closure record cannot smuggle bad chains into
    /// the solver.
    #[test]
    fn extract_loop_closure_chains_missing_path_b_returns_none() {
        let j_a = prismatic_x();
        let bindings: Vec<Value> = Vec::new();
        // Hand-built loop_closure record WITHOUT path_b.
        let mut m = std::collections::BTreeMap::new();
        m.insert(Value::String("body_id".to_string()), Value::Int(0));
        m.insert(Value::String("closing_joint".to_string()), j_a.clone());
        m.insert(
            Value::String("kind".to_string()),
            Value::String("loop_closure".to_string()),
        );
        m.insert(
            Value::String("path_a".to_string()),
            Value::List(vec![world_sentinel(), j_a]),
        );
        let record = Value::Map(m);

        assert!(
            super::extract_loop_closure_chains(&record, &bindings).is_none(),
            "extract_loop_closure_chains must return None for a record missing path_b"
        );
    }

    // ── loop_residual_jacobian_by_joint tests ────────────────────────────
    //
    // Verifies GAP 1 resolution: the full two-chain residual Jacobian assembled
    // by perturbing tree `at` joints in BOTH chains wherever they appear.
    //
    // Fixture:
    //   jA = prismatic_x  (single-DOF, axis +X)
    //   jB = revolute_z   (single-DOF, axis +Z)
    //   jC = revolute_y   (third joint NOT in either chain)
    //
    //   chain_a = [jA]        vals_a = [0.3]
    //   chain_b = [jA, jB]    vals_b = [0.3, 0.5]
    //
    // jA appears in BOTH chains at their first position — structurally
    // identical Values (same eval_builtin output), so Value::Eq locates
    // both occurrences correctly.
    //
    // For each target joint the independent FD reference is computed by
    // perturbing that joint's value in ALL positions it appears:
    //   jA: vals_a±=[0.3±ε], vals_b±=[0.3±ε, 0.5]
    //   jB: vals_a unchanged, vals_b±=[0.3, 0.5±ε]
    //   jC: absent from both chains → zero column (no perturbation site)
    //
    // Achievability: FD-vs-FD identity — both references use the same
    // loop_residual_twist formula, so roundoff is the only divergence.
    // Tolerance 1e-7 is well within central-difference O(ε²) accuracy.

    /// Independent FD helper: central-difference column for perturbing
    /// `target_idx.0` (index in vals_a, or None) AND `target_idx.1` (index
    /// in vals_b, or None) simultaneously. The two indices travel as one
    /// `(chain_a, chain_b)` pair — they describe a single target joint's
    /// occurrence sites (also keeps the arg count within clippy's limit).
    fn fd_column(
        chain_a: &[Value],
        vals_a: &[JointValue],
        chain_b: &[Value],
        vals_b: &[JointValue],
        target_idx: (Option<usize>, Option<usize>),
        storage_c: usize,
        eps: f64,
    ) -> [f64; 6] {
        let (target_idx_a, target_idx_b) = target_idx;
        let mut va_plus = vals_a.to_vec();
        let mut va_minus = vals_a.to_vec();
        let mut vb_plus = vals_b.to_vec();
        let mut vb_minus = vals_b.to_vec();
        if let Some(ia) = target_idx_a {
            super::perturb_storage_component(&mut va_plus[ia], storage_c, eps);
            super::perturb_storage_component(&mut va_minus[ia], storage_c, -eps);
        }
        if let Some(ib) = target_idx_b {
            super::perturb_storage_component(&mut vb_plus[ib], storage_c, eps);
            super::perturb_storage_component(&mut vb_minus[ib], storage_c, -eps);
        }
        let rp = super::loop_residual_twist(chain_a, &va_plus, chain_b, &vb_plus)
            .expect("residual_plus");
        let rm = super::loop_residual_twist(chain_a, &va_minus, chain_b, &vb_minus)
            .expect("residual_minus");
        let mut col = [0.0f64; 6];
        for k in 0..6 {
            col[k] = (rp[k] - rm[k]) / (2.0 * eps);
        }
        col
    }

    #[test]
    fn loop_residual_jacobian_by_joint_central_difference() {
        let eps = 1e-6_f64;
        let tol = 1e-7_f64;

        // Build three structurally-distinct joints.
        let j_a = prismatic_x(); // axis +X, prismatic
        let j_b = revolute_z(); // axis +Z, revolute
        let j_c = eval_builtin("revolute", &[axis_y_unit(), angle_range_0_to_pi()]); // axis +Y

        // chain_a = [jA], chain_b = [jA, jB] — jA appears in both chains.
        let chain_a: Vec<Value> = vec![j_a.clone()];
        let chain_b: Vec<Value> = vec![j_a.clone(), j_b.clone()];
        let vals_a = vec![JointValue::Scalar(0.3_f64)];
        let vals_b = vec![JointValue::Scalar(0.3_f64), JointValue::Scalar(0.5_f64)];

        // ── case 1: target = [jA, jB] ─────────────────────────────────────────
        let cols = super::loop_residual_jacobian_by_joint(
            &chain_a, &vals_a, &chain_b, &vals_b, &[j_a.clone(), j_b.clone()], eps,
        )
        .expect("loop_residual_jacobian_by_joint must return Some for valid inputs");

        assert_eq!(cols.len(), 2, "two single-DOF target joints → two columns");

        // Column for jA: perturb jA in BOTH chains (index 0 in chain_a, index 0 in chain_b).
        let expected_a = fd_column(
            &chain_a, &vals_a, &chain_b, &vals_b,
            (Some(0), Some(0)), // chain_a index 0, chain_b index 0
            0,                  // storage component 0 (Scalar)
            eps,
        );
        for k in 0..6 {
            assert!(
                (cols[0][k] - expected_a[k]).abs() < tol,
                "col_jA[{k}]: got {:.6e}, want {:.6e}, diff {:.2e}",
                cols[0][k], expected_a[k], (cols[0][k] - expected_a[k]).abs()
            );
        }

        // Column for jB: perturb jB only in chain_b (index 1), chain_a unchanged.
        let expected_b = fd_column(
            &chain_a, &vals_a, &chain_b, &vals_b,
            (None, Some(1)), // chain_a — jB absent; chain_b index 1
            0,               // storage component 0 (Scalar)
            eps,
        );
        for k in 0..6 {
            assert!(
                (cols[1][k] - expected_b[k]).abs() < tol,
                "col_jB[{k}]: got {:.6e}, want {:.6e}, diff {:.2e}",
                cols[1][k], expected_b[k], (cols[1][k] - expected_b[k]).abs()
            );
        }

        // ── case 2: target = [jC] (absent from both chains) ──────────────────
        // jC is not in chain_a or chain_b → zero column (no perturbation site).
        let cols_c = super::loop_residual_jacobian_by_joint(
            &chain_a, &vals_a, &chain_b, &vals_b, std::slice::from_ref(&j_c), eps,
        )
        .expect("jC absent → Some(zero-column), not None");

        assert_eq!(cols_c.len(), 1, "one target joint → one column");
        for (k, &col_val) in cols_c[0].iter().enumerate() {
            assert!(
                col_val.abs() < 1e-12,
                "col_jC[{k}] expected zero (jC absent from both chains), got {col_val}"
            );
        }

        // ── case 3: eps <= 0 → None ───────────────────────────────────────────
        assert!(
            super::loop_residual_jacobian_by_joint(
                &chain_a, &vals_a, &chain_b, &vals_b, std::slice::from_ref(&j_a), 0.0
            )
            .is_none(),
            "eps=0 must return None"
        );
    }

    // ── KIN-OFFSET γ step-2 (B8 routes 1 + 3): no-bypass invariant ───────────────

    /// B8 route 1: `chain_transform([j], [Scalar(v)])` reproduces
    /// `transform_at(j, v)` exactly for an offset-bearing joint.
    ///
    /// Proves `chain_transform` routes every offset through `transform_at`
    /// — no bypass (PRD §7.2).
    #[test]
    fn chain_transform_offset_single_joint_equals_transform_at_route1() {
        let j = offset_revolute_z(0.3);
        let v = std::f64::consts::PI / 6.0;

        let chain_result = super::chain_transform(std::slice::from_ref(&j), &[JointValue::Scalar(v)])
            .expect("chain_transform must return Some for an offset revolute joint");
        let direct = eval_builtin("transform_at", &[j.clone(), Value::angle(v)]);
        assert!(!direct.is_undef(), "transform_at with offset must not return Undef");

        let ct = translation_xyz(&chain_result);
        let dt = translation_xyz(&direct);
        let (cw, cx, cy, cz) = rotation_wxyz(&chain_result);
        let (dw, dx, dy, dz) = rotation_wxyz(&direct);
        let tol = 1e-12;
        assert!(
            (cw - dw).abs() < tol
                && (cx - dx).abs() < tol
                && (cy - dy).abs() < tol
                && (cz - dz).abs() < tol,
            "B8 route-1: rotation mismatch — chain=({cw},{cx},{cy},{cz}) direct=({dw},{dx},{dy},{dz})"
        );
        for i in 0..3 {
            assert!(
                (ct[i] - dt[i]).abs() < tol,
                "B8 route-1: translation[{i}] mismatch — chain={} direct={}",
                ct[i],
                dt[i]
            );
        }
    }

    /// B8 route 3: the offset PROPAGATES into `loop_residual_twist` — the
    /// residual of an offset chain differs from the same chain with origins
    /// stripped, proving the Jacobian basis is offset-aware (PRD §7.2).
    #[test]
    fn loop_residual_twist_offset_propagates_into_residual_route3() {
        let j_offset = offset_revolute_z(0.3);
        let j_bare = revolute_z();
        let v = std::f64::consts::FRAC_PI_4;

        // Use the same "other side" chain for both comparisons.
        let chain_ref = vec![revolute_z()];
        let vals_ref = vec![JointValue::Scalar(v)];

        let chain_offset = vec![j_offset.clone()];
        let chain_bare = vec![j_bare.clone()];
        let vals = vec![JointValue::Scalar(v)];

        let twist_offset = super::loop_residual_twist(&chain_offset, &vals, &chain_ref, &vals_ref)
            .expect("loop_residual_twist with offset must return Some");
        let twist_bare = super::loop_residual_twist(&chain_bare, &vals, &chain_ref, &vals_ref)
            .expect("loop_residual_twist without offset must return Some");

        // The offset (0.3 m translation baked into chain_a) must change the
        // residual — the difference should be dominated by the 0.3 m offset.
        let max_diff = twist_offset
            .iter()
            .zip(twist_bare.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f64, f64::max);
        assert!(
            max_diff > 1e-6,
            "B8 route-3: offset must change loop_residual_twist \
             (max |Δtwist| = {max_diff:.3e}, want > 1e-6)"
        );
    }

    // ── KIN-OFFSET γ step-5 (B3): 2-link offset chain analytic FK ────────────────

    /// B3 chain_transform: 2-link offset revolute-Z chain at θ_a=π/6, θ_b=π/3.
    ///
    /// With L_a=0.3 m (joint_a pivot) and L_b=0.2 m (joint_b pivot), the
    /// composed Transform via `chain_transform` must equal the planar-arm
    /// closed form (PRD §7.2 design decision 4):
    ///   translation = (L_a + L_b·cos θ_a, L_b·sin θ_a, 0)
    ///               = (0.3 + 0.2·cos 30°, 0.2·sin 30°, 0)
    ///               ≈ (0.473205, 0.1, 0)
    ///   rotation    = R_z(θ_a + θ_b) = R_z(90°) = (0.707107, 0, 0, 0.707107)
    ///
    /// Confirms the chain_transform consumer (loop_closure.rs:87) is offset-aware.
    #[test]
    fn chain_transform_two_link_offset_chain_analytic() {
        let pi = std::f64::consts::PI;
        let theta_a = pi / 6.0; // 30°
        let theta_b = pi / 3.0; // 60°

        let (joint_a, joint_b) = two_link_offset_chain();
        let chain = vec![joint_a.clone(), joint_b.clone()];
        let vals = vec![JointValue::Scalar(theta_a), JointValue::Scalar(theta_b)];

        let result = super::chain_transform(&chain, &vals)
            .expect("chain_transform must return Some for a 2-link offset chain");

        // Closed-form expected values (PRD design decision 4).
        let l_a = 0.3_f64;
        let l_b = 0.2_f64;
        let exp_tx = l_a + l_b * theta_a.cos();
        let exp_ty = l_b * theta_a.sin();
        let half = (theta_a + theta_b) / 2.0; // = π/4
        let exp_qw = half.cos();
        let exp_qz = half.sin();

        let tol = 1e-9;
        let trans = translation_xyz(&result);
        assert!(
            (trans[0] - exp_tx).abs() < tol,
            "tx: expected {exp_tx:.9}, got {:.9}", trans[0]
        );
        assert!(
            (trans[1] - exp_ty).abs() < tol,
            "ty: expected {exp_ty:.9}, got {:.9}", trans[1]
        );
        assert!(
            trans[2].abs() < tol,
            "tz: expected 0, got {:.3e}", trans[2]
        );

        let (w, x, y, z) = rotation_wxyz(&result);
        let matches_pos = (w - exp_qw).abs() < tol && x.abs() < tol
            && y.abs() < tol && (z - exp_qz).abs() < tol;
        let matches_neg = (w + exp_qw).abs() < tol && x.abs() < tol
            && y.abs() < tol && (z + exp_qz).abs() < tol;
        assert!(
            matches_pos || matches_neg,
            "rotation: expected R_z(90°) ≈ ({exp_qw:.6}, 0, 0, {exp_qz:.6}) up to sign, \
             got ({w:.6}, {x:.6}, {y:.6}, {z:.6})"
        );
    }

    // ── KIN-OFFSET γ step-6 (B5): twist↔Jacobian consistency on offset chains ───

    /// B5: `loop_residual_jacobian_by_joint` columns match a manual central
    /// difference of the OFFSET-AWARE `loop_residual_twist`, within the eps²
    /// FD floor (≤ 1e-9 at eps=1e-7).
    ///
    /// Chain structure:
    ///   chain_a = [jA = offset_revolute_z(0.3)]
    ///   chain_b = [jB = offset_revolute_z(0.2), jC = offset_prismatic_x(0.1)]
    ///   target_joints = [jA, jB, jC] (structurally distinct — each appears in
    ///   exactly one chain).
    ///
    /// The manual FD uses the identical `fd_column` helper and the same
    /// offset-aware `loop_residual_twist`, so divergence is only operation-
    /// ordering roundoff (~1 ULP, far below 1e-9).  Non-vacuity assertion
    /// confirms at least one column is non-trivially nonzero (the offset is
    /// genuinely exercised, not a vacuous zero-Jacobian).
    #[test]
    fn loop_residual_jacobian_by_joint_offset_chain_matches_manual_fd() {
        let eps = 1e-7_f64;
        let tol = 1e-9_f64;

        // Three structurally-distinct offset-bearing joints (each a unique Map).
        let j_a = offset_revolute_z(0.3);
        let j_b = offset_revolute_z(0.2);
        let j_c = offset_prismatic_x(0.1);

        // chain_a = [jA], chain_b = [jB, jC]
        let chain_a = vec![j_a.clone()];
        let chain_b = vec![j_b.clone(), j_c.clone()];
        let vals_a = vec![JointValue::Scalar(0.5_f64)];
        let vals_b = vec![JointValue::Scalar(0.3_f64), JointValue::Scalar(0.2_f64)];

        let cols = super::loop_residual_jacobian_by_joint(
            &chain_a,
            &vals_a,
            &chain_b,
            &vals_b,
            &[j_a.clone(), j_b.clone(), j_c.clone()],
            eps,
        )
        .expect("loop_residual_jacobian_by_joint must return Some for valid offset chains");

        assert_eq!(cols.len(), 3, "three single-DOF target joints → three columns");

        // Manual FD: jA appears in chain_a[0] only.
        let manual_a = fd_column(
            &chain_a, &vals_a, &chain_b, &vals_b, (Some(0), None), 0, eps,
        );
        // Manual FD: jB appears in chain_b[0] only.
        let manual_b = fd_column(
            &chain_a, &vals_a, &chain_b, &vals_b, (None, Some(0)), 0, eps,
        );
        // Manual FD: jC appears in chain_b[1] only.
        let manual_c = fd_column(
            &chain_a, &vals_a, &chain_b, &vals_b, (None, Some(1)), 0, eps,
        );

        for k in 0..6 {
            assert!(
                (cols[0][k] - manual_a[k]).abs() < tol,
                "B5 col_jA[{k}]: function={:.6e} manual={:.6e} diff={:.2e}",
                cols[0][k], manual_a[k], (cols[0][k] - manual_a[k]).abs()
            );
            assert!(
                (cols[1][k] - manual_b[k]).abs() < tol,
                "B5 col_jB[{k}]: function={:.6e} manual={:.6e} diff={:.2e}",
                cols[1][k], manual_b[k], (cols[1][k] - manual_b[k]).abs()
            );
            assert!(
                (cols[2][k] - manual_c[k]).abs() < tol,
                "B5 col_jC[{k}]: function={:.6e} manual={:.6e} diff={:.2e}",
                cols[2][k], manual_c[k], (cols[2][k] - manual_c[k]).abs()
            );
        }

        // Non-vacuity: at least one column entry is non-trivially nonzero.
        // The offset makes the residual config-dependent → Jacobian columns
        // are non-zero whenever the two chains are not at the same pose.
        let max_abs = cols
            .iter()
            .flat_map(|col| col.iter())
            .map(|v| v.abs())
            .fold(0.0_f64, f64::max);
        assert!(
            max_abs > 1e-4,
            "B5 non-vacuity: at least one Jacobian entry must be nonzero \
             (offset is genuinely exercised), max_abs = {max_abs:.3e}"
        );
    }

    // ── KIN-OFFSET γ amend: analytic Jacobian check for single offset joint ─

    /// Analytic check: `loop_residual_jacobian_by_joint` column for a single
    /// `offset_revolute_z(L)` joint in chain_a, with empty chain_b (≡ identity).
    ///
    /// The residual is `r = log(T_a^{-1}(θ))` where
    ///   `T_a(θ) = {R_z(θ), (L,0,0)}`  (offset_revolute_z(L) at angle θ)
    ///   `T_a^{-1}(θ) = {R_z(−θ), (−L·cos θ, L·sin θ, 0)}`
    ///
    /// Closed-form log components (derivation: SE(3) log with α=−θ,
    /// tx=−L·cos θ, ty=L·sin θ, using the 2×2 rotation-coupled translation
    /// formula):
    ///   r[2] (ω_z)  = −θ           → ∂r[2]/∂θ = −1  (exact)
    ///   r[4] (v_y)  = θ·L/2        → ∂r[4]/∂θ = L/2 (exact for all θ)
    ///
    /// The L/2 term is the offset's fingerprint in the Jacobian: without an
    /// offset (L=0), r[4]=0 for all θ, so col[4]=0.  With L=0.2, col[4]=0.1.
    /// This is the genuinely offset-discriminating assertion that the B5
    /// FD-vs-FD test (which is near-tautological on its own) cannot provide.
    #[test]
    fn loop_residual_jacobian_analytic_offset_single_joint() {
        let eps = 1e-7_f64;
        let tol = 1e-9_f64;
        let l = 0.2_f64;        // pivot offset
        let theta_a = 0.5_f64;  // well away from 0 and π to avoid log singularities

        let j_a = offset_revolute_z(l);
        let chain_a = vec![j_a.clone()];
        let vals_a = vec![JointValue::Scalar(theta_a)];
        // Empty chain_b ≡ identity transform.
        let chain_b: Vec<Value> = vec![];
        let vals_b: Vec<JointValue> = vec![];

        let cols = super::loop_residual_jacobian_by_joint(
            &chain_a,
            &vals_a,
            &chain_b,
            &vals_b,
            std::slice::from_ref(&j_a),
            eps,
        )
        .expect("jacobian must return Some for a single-joint offset chain vs identity");

        assert_eq!(cols.len(), 1, "single target joint → one column");
        let col = cols[0];

        // Analytic: ∂r[2]/∂θ = −1 (ω_z = −θ from the angular part of log(T_a^{-1}))
        assert!(
            (col[2] - (-1.0)).abs() < tol,
            "analytic ∂ω_z/∂θ = −1; FD gave {:.12}", col[2]
        );
        // Analytic: ∂r[4]/∂θ = L/2 (v_y = θ·L/2, exact for all θ ≠ 0).
        // The offset L enters here: col[4]=0 with no offset, col[4]=L/2=0.1 with offset.
        assert!(
            (col[4] - l / 2.0).abs() < tol,
            "analytic ∂v_y/∂θ = L/2 = {:.4}; FD gave {:.12}", l / 2.0, col[4]
        );
    }
}
