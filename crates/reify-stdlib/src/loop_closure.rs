//! Loop-closure machinery: value-level helpers operating on joint-Map `Value`s.
//!
//! This module provides the building blocks the kinematic snapshot evaluator
//! (future task 2585) and the generic Newton solver in
//! `reify_constraints::loop_closure` use to drive closed-chain mechanisms to
//! consistency.  It is the value-side companion to `reify-constraints::loop_closure`.
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

use reify_types::Value;

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
/// TODO(KCC-γ step-12): Sphere slots currently contribute 4 raw-storage
/// columns (matching `flat_len = 4`).  The redundant fourth column reflects
/// the off-manifold direction in unit-quaternion storage; step-12's solver
/// wiring switches to 3 body-frame angular tangent columns
/// (`δq = renormalize(q ⊗ exp(½·δω))`) per the dof_count = 3 manifold and
/// composes the off-manifold projection via `renormalize_quaternion` after
/// each Newton step.  Until that wiring lands, the redundant column is
/// damped harmlessly by `transform_at`'s `normalize_quaternion` call (which
/// projects every spherical input back to S³).
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
pub type LoopClosureSolverInputs = (Vec<Value>, Vec<f64>, Vec<Value>, Vec<f64>, Vec<usize>);

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
    // resolve to an SI f64 via direct binding, coupling-tracks-parent
    // recursion, fixed-joint sentinel, or range-midpoint fallback.  Any
    // joint that fails all four short-circuits the whole call to None.
    let mut vals_a = Vec::with_capacity(chain_a.len());
    for joint in &chain_a {
        let v_si = resolve_joint_value_si(joint, bindings)?;
        vals_a.push(v_si);
    }

    // chain_b is the closing side: a joint with a *direct* binding entry
    // is a fixed initial value; any joint without a direct binding becomes
    // a free index, seeded from its range midpoint.  Coupling and fixed
    // arms intentionally fall through the direct-lookup branch — multi-loop
    // coupling is out of v0.2 scope (see plan design-decisions §4).
    //
    // **Asymmetry note (v0.2 limitation).**  `chain_a` resolves via
    // `resolve_joint_value_si`, which carries a fixed-joint sentinel arm
    // (returns `Some(0.0)`).  `chain_b`'s unbound-joint fallback uses
    // `joint_range_midpoint` directly, which returns `None` for fixed
    // joints — so a fixed joint appearing in `path_b` without a direct
    // binding would collapse the whole record to None and the snapshot
    // to Undef.  In practice the mechanism builder does not place fixed
    // joints on closing paths (the closing edge always references a
    // motion joint to drive the solver), so this asymmetry is a latent
    // shape constraint rather than a live bug.  A future v0.3 refactor
    // can route this fallback through `resolve_joint_value_si` (and
    // skip the index from `free_b` when the result is the fixed
    // sentinel) once a real fixture demands fixed joints in path_b.
    let mut vals_b_initial = Vec::with_capacity(chain_b.len());
    let mut free_b: Vec<usize> = Vec::new();
    for (i, joint) in chain_b.iter().enumerate() {
        if let Some(v_si) = direct_binding_value_si(joint, bindings) {
            vals_b_initial.push(v_si);
        } else {
            // KCC-γ: joint_range_midpoint now returns Option<JointValue>;
            // the tuple-typed (Vec<f64>) chain bridge still operates on
            // scalar SI values, so we extract the f64 from Scalar and
            // collapse to None for multi-DOF kinds (step-10 widens the
            // tuple type and removes this extraction step).
            let mid_jv = joint_range_midpoint(joint)?;
            let mid_si = match mid_jv {
                JointValue::Scalar(s) => s,
                _ => return None,
            };
            vals_b_initial.push(mid_si);
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

/// Look up a joint's SI value via a *direct* binding entry (no coupling
/// recursion, no midpoint fallback).
///
/// Linear scan — same shape as snapshot.rs's `value_for` direct-binding
/// arm, but returns the bound value's SI f64 rather than a dimensioned
/// `Value`.  Returns None when no binding entry's `joint` field is
/// structurally equal to `joint`, or when the bound `value` is not a
/// numeric type the dimension extractor recognises.
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
/// rehydrated for the FK re-walk via `wrap_midpoint_for_joint` →
/// `transform_at`.  In practice the residual-evaluation path inside
/// `solve_loop_closure` invokes `chain_transform`, which itself routes
/// through dimension-checked Transform composition; a wrong-dimension
/// value will surface as a non-converging residual rather than as a
/// silent acceptance of an inconsistent configuration.  Callers that
/// want eager validation should compose this with the joint-kind
/// predicate at the snapshot boundary.
fn direct_binding_value_si(joint: &Value, bindings: &[Value]) -> Option<f64> {
    for entry in bindings {
        let map = match entry {
            Value::Map(m) => m,
            _ => continue,
        };
        if map.get(&Value::String("joint".to_string())) == Some(joint)
            && let Some(v) = map.get(&Value::String("value".to_string()))
        {
            // See fn-doc: as_f64() is dimension-blind here; downstream
            // `transform_at` / `chain_transform` is the canonical
            // dimension-validation site for closed-chain inputs.
            return v.as_f64();
        }
    }
    None
}

/// Resolve a joint's motion value to an SI f64 via the same fallback
/// chain `snapshot::value_for` uses, then extract the underlying SI scalar.
///
/// Resolution order:
/// 1. Direct binding by structural `Value::Eq` on the joint Map.
/// 2. Coupling-tracks-parent: when `joint` is a coupling and isn't directly
///    bound, recurse on the coupling's `parent` joint.
/// 3. Fixed joint sentinel: `Value::Real(0.0)` (snapshot.rs's `transform_at`
///    arm ignores the second argument for fixed joints).
/// 4. Range-midpoint fallback via [`joint_range_midpoint`].
///
/// Returns None for malformed joint Maps, multi-DOF kinds the f64-per-joint
/// signature cannot represent (planar / spherical / cylindrical), or
/// joints with no resolvable value (no binding AND no midpoint).
fn resolve_joint_value_si(joint: &Value, bindings: &[Value]) -> Option<f64> {
    if let Some(v) = direct_binding_value_si(joint, bindings) {
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
            return resolve_joint_value_si(parent, bindings);
        }
        if kind == "fixed" {
            return Some(0.0);
        }
    }
    // KCC-γ: joint_range_midpoint now returns Option<JointValue>; this
    // f64-typed resolver only handles Scalar-shaped midpoints (single-DOF
    // kinds + couplings) and returns None for multi-DOF kinds.  The
    // chain-bridge-widening in step-10 lifts this resolver to return
    // Option<JointValue> directly.
    joint_range_midpoint(joint).and_then(|jv| match jv {
        JointValue::Scalar(s) => Some(s),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::loop_closure_value::JointValue;
    use crate::test_fixtures::{
        angle_range_0_to_pi, axis_x_unit, axis_y_unit, axis_z_unit, cylindrical_z_joint,
        length_range_0_to_1m, planar_xy_joint, spherical_joint,
    };
    use reify_types::Value;

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
    ///   * vals_a populated from binding: `[0.5]` (SI metres)
    ///   * chain_b stripped of world sentinel: `[jB]`
    ///   * vals_b_initial seeded from jB's range midpoint: `[0.5]`
    ///     (jB has range 0..1m → midpoint 0.5m).
    ///   * free_b indices: `[0]` (the only joint in chain_b is unbound).
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
        assert!(
            (vals_a[0] - 0.5).abs() < 1e-12,
            "vals_a[0] expected 0.5 (bound), got {}",
            vals_a[0]
        );
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
        assert!(
            (vals_b_initial[0] - 0.5).abs() < 1e-12,
            "vals_b_initial[0] expected midpoint 0.5 (jB range 0..1m), got {}",
            vals_b_initial[0]
        );
        assert_eq!(free_b, vec![0], "free_b should mark jB (index 0) as free");
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
}
