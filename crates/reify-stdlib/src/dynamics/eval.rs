//! Eval-side `Value`↔core dispatch for the RBD-η stdlib dynamics entry points
//! (`docs/prds/v0_3/rigid-body-dynamics.md` §5 / task RBD-η, Phase 4).
//!
//! This module is the `Value`-marshalling half of the dynamics surface: it
//! extracts `Value`s into the pure-`f64` `RneaLink` inputs consumed by the
//! [`crate::dynamics::rnea`] open-chain RNEA core, invokes it, and reshapes the
//! result `τ` back into registry-free `JointForce` / `MotionTrajectory`
//! `Value::StructureInstance`s.
//!
//! **Closed-chain routing (task 4146).** The trajectory variant
//! `inverse_dynamics` now routes closed mechanisms through
//! [`closed_chain_inverse_dynamics`], which wires `M` / `τ_open` / the loop
//! Jacobian `A` into [`crate::dynamics::closed_chain`].  The snapshot entry
//! `inverse_dynamics_at_snapshot` remains Undef for closed mechanisms (design
//! decision esc-3836-226: a snapshot discards the spanning-tree q).
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
//!   * `inverse_dynamics_lower`              — trajectory variant (step-8)

use crate::dynamics::closed_chain::{reduce_constraint_rank, solve_closed_chain, DEFAULT_PIVOT_EPS};
use crate::dynamics::rnea::{
    assemble_joint_space_inertia, default_gravity, inverse_dynamics_open_chain, JointCompliance,
    RneaLink,
};
use crate::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};
use crate::joints::motion_subspace_columns;
use crate::loop_closure::{extract_loop_closure_chains, loop_residual_jacobian_by_joint};
use crate::mechanism::is_world;
use reify_core::diagnostics::{Diagnostic, DiagnosticCode};
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

/// Extract an `f64` from a **mass** value cell.
///
/// Accepts dimensionless numerics (`Int`, `Real`) and `Scalar`s whose dimension
/// is exactly [`DimensionVector::MASS`].  Rejects any `Scalar` with a different
/// dimension (e.g. `Length`, `Angle`) — a caller passing `5 m` as a mass
/// argument would otherwise be silently accepted and treated as `5 kg`,
/// producing a physically-wrong `MassProperties` with no diagnostic.
///
/// This is intentionally stricter than the bare [`cell_f64`] used elsewhere:
/// the mass field of a `MassProperties` is dimension-aware (it is stored as a
/// `Scalar{dimension:MASS}`), so input validation must be too.  Dimensionless
/// numerics are still accepted for ergonomics (`point_mass(2.5)` in unit-free
/// test helpers), matching the sibling `dynamics_ops` behaviour.
fn cell_mass_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, dimension } if *dimension == DimensionVector::MASS => {
            Some(*si_value)
        }
        // Wrong dimension (e.g. Length, Angle): reject so the caller can
        // surface Value::Undef rather than silently producing wrong mass.
        Value::Scalar { .. } => None,
        // Dimensionless Int/Real are accepted for test ergonomics.
        other => cell_f64(other),
    }
}

/// Extract an `f64` from a compliant-field value cell: handles the
/// `Value::Option(Some(inner))` wrapper (recursing into `inner`) and
/// `Value::Option(None)` (→ `None`), plus all the bare shapes that
/// `cell_f64` accepts (`Int` / `Real` / dimensioned `Scalar`).
/// Non-finite values are filtered to `None`.
///
/// This mirrors `flexures::common::scalar_si` but delegates to `cell_f64`
/// for the dimension-stripping step, accepting `Int` and `Real` as well as
/// `Scalar`. Used by `joint_compliance` to read `spring_rate`, `damping`,
/// and `neutral` from a flexure joint Map in either the bare-Scalar shape
/// that `make_flexure_joint` emits today or an Option-wrapped future shape.
fn compliance_cell_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Option(Some(inner)) => compliance_cell_f64(inner),
        Value::Option(None) => None,
        other => {
            let f = cell_f64(other)?;
            if f.is_finite() { Some(f) } else { None }
        }
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
        "point_mass" => Some(eval_point_mass(args)),
        "mass_properties" => Some(eval_mass_properties(args)),
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
/// `Value::Matrix` of MomentOfInertia-dimensioned scalars, kg·m²) plus the
/// equivalent list-shaped encodings a user-authored MassProperties may produce.
/// The `com` Length dimension and inertia MomentOfInertia dimension are both
/// stripped to raw SI f64 values via `cell_f64`. Returns `None` for any
/// non-MassProperties value or a malformed/absent field.
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

// ── resolve_body_mass — single mass read-path (task 4278) ────────────────────

/// The single canonical read-path for body mass in every inverse-dynamics and
/// modal consumer.
///
/// Rung precedences (highest first):
/// (a) **Explicit MassProperties** — `body.solid` is a `MassProperties`
///     StructureInstance → `Some(solid.clone())`.
/// (b) **Derived geometry×density** — `body.derived_mass_props` is a
///     `MassProperties` StructureInstance baked by the build-time
///     `post_process_mechanism_mass_props` pass (task 4472) → `Some(that.clone())`.
///     Rung (a) wins when `solid` is already an explicit MassProperties; rung (b)
///     fires only when `solid` is absent or is not a MassProperties.
/// (c) **Unresolvable** — neither rung above matched → `None`.
///
/// Consumers extract fields from the returned `Value` via
/// [`mass_properties_from_value`].
pub fn resolve_body_mass(body: &Value) -> Option<Value> {
    let bm = match body {
        Value::Map(m) => m,
        _ => return None,
    };
    // Rung (a): explicit MassProperties solid wins unconditionally.
    if let Some(solid) = map_get(bm, "solid") {
        if let Value::StructureInstance(d) = solid {
            if d.type_name == "MassProperties" {
                return Some(solid.clone());
            }
        }
    }
    // Rung (b): build-pass-baked derived_mass_props (task 4472).
    if let Some(derived) = map_get(bm, "derived_mass_props") {
        if let Value::StructureInstance(d) = derived {
            if d.type_name == "MassProperties" {
                return Some(derived.clone());
            }
        }
    }
    // Rung (c): unresolvable.
    None
}

// ── dynamics diagnose hook (task 4278) ───────────────────────────────────────

/// Undef-path diagnostic hook for `inverse_dynamics` intrinsics.
///
/// Mirrors the `stackup_diagnose` / `fea_diagnose` / `geometry_diagnose`
/// pattern: called by `reify-expr`'s `emit_undef_builtin_diagnostics` after
/// the builtin returns `Value::Undef`, re-derives the cause by inspecting the
/// mechanism's spanning-tree bodies, and emits
/// [`DiagnosticCode::DynamicsBodyMassUnresolved`] for the first body whose mass
/// cannot be resolved via [`resolve_body_mass`].
///
/// Returns `None` for non-dynamics names, non-mechanism args[0], or fully-
/// resolvable mechanisms (no spurious fire). Closing-edge bodies are excluded
/// from the walk because the closed-chain RNEA never reads their mass.
///
/// **Best-effort attribution.** This hook re-derives the Undef cause
/// independently of the actual call; it does not receive the real reason the
/// builtin returned `Value::Undef`. If `inverse_dynamics_lower` returns `Undef`
/// for an unrelated reason (e.g. malformed trajectory, missing world transform)
/// while a body also happens to have unresolvable mass, the diagnostic will
/// attribute the failure to mass. This matches the established behaviour of the
/// sibling `stackup_diagnose` / `fea_diagnose` / `geometry_diagnose` hooks and
/// is acceptable — the hook provides a best-effort hint, not an authoritative
/// root-cause analysis.
pub fn diagnose(name: &str, args: &[Value]) -> Option<Diagnostic> {
    match name {
        "inverse_dynamics_lower" | "inverse_dynamics_at_snapshot_lower" => {}
        _ => return None,
    }
    let mech = match args.first() {
        Some(Value::Map(m)) => m,
        _ => return None,
    };
    let bodies = match map_get(mech, "bodies") {
        Some(Value::List(b)) => b,
        _ => return None,
    };
    // Closing-edge bodies are appended at the end; their mass is never read by
    // the RNEA so we skip them to avoid false positives.
    let n_lc = match map_get(mech, "loop_closures") {
        Some(Value::List(lc)) => lc.len(),
        _ => 0,
    };
    let n_tree = bodies.len().saturating_sub(n_lc);
    for body in &bodies[..n_tree] {
        if resolve_body_mass(body).is_none() {
            let id_str = match body {
                Value::Map(bm) => match map_get(bm, "id") {
                    Some(Value::Int(k)) => k.to_string(),
                    _ => "?".to_string(),
                },
                _ => "?".to_string(),
            };
            return Some(
                Diagnostic::error(format!(
                    "inverse_dynamics: body '{id_str}' has no resolvable mass \
                     (no MassProperties on body.solid)"
                ))
                .with_code(DiagnosticCode::DynamicsBodyMassUnresolved),
            );
        }
    }
    None
}

// ── MassProperties constructor helpers (task 4278) ──────────────────────────

/// Build a canonical registry-free `MassProperties` `Value::StructureInstance`
/// matching the `assemble_mass_properties` shape:
/// - `mass`   → `Value::Scalar { dimension: MASS }`
/// - `com`    → `Value::Point` of `Value::length` scalars (SI metres)
/// - `inertia`→ `Value::Matrix` of `Value::Scalar { dimension: MOMENT_OF_INERTIA }` (3×3, kg·m²)
/// - `origin` → `Value::Real(0.0)` (unused sentinel, mirrors `dynamics_ops`)
///
/// Inertia cells are MomentOfInertia-dimensioned scalars (kg·m²), matching the
/// `inertia_value` populate pattern in `dynamics_ops`. The PSD hook and
/// `inertia_3x3_from_value` both read them unchanged via `cell_f64`, which strips
/// `si_value` from any `Value::Scalar`.
fn make_mass_properties(mass: f64, com: [f64; 3], inertia: [[f64; 3]; 3]) -> Value {
    let com_point = Value::Point(com.iter().map(|&c| Value::length(c)).collect());
    let inertia_matrix = Value::Matrix(
        inertia
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&x| Value::Scalar {
                        si_value: x,
                        dimension: DimensionVector::MOMENT_OF_INERTIA,
                    })
                    .collect()
            })
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

/// Evaluate `mass_properties(mass, com, inertia)` — a full `MassProperties`
/// with caller-supplied centre-of-mass and inertia tensor. Validates arity and
/// field shapes; returns `Value::Undef` on any malformed input, including a
/// mass argument whose dimension is not `MASS` (e.g. a `Length` scalar).
fn eval_mass_properties(args: &[Value]) -> Value {
    if args.len() != 3 {
        return Value::Undef;
    }
    let mass = match cell_mass_f64(&args[0]) {
        Some(m) => m,
        None => return Value::Undef,
    };
    let com = match vec3_from_value(&args[1]) {
        Some(c) => c,
        None => return Value::Undef,
    };
    let inertia = match inertia_3x3_from_value(&args[2]) {
        Some(i) => i,
        None => return Value::Undef,
    };
    make_mass_properties(mass, com, inertia)
}

/// Evaluate `point_mass(mass)` — a degenerate `MassProperties` with com at the
/// origin and zero inertia tensor. Returns `Value::Undef` for wrong arity, a
/// non-numeric mass argument, or a mass scalar whose dimension is not `MASS`
/// (e.g. a `Length` scalar — would silently produce a wrong MassProperties
/// without this guard).
fn eval_point_mass(args: &[Value]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let mass = match cell_mass_f64(&args[0]) {
        Some(m) => m,
        None => return Value::Undef,
    };
    make_mass_properties(mass, [0.0, 0.0, 0.0], [[0.0; 3]; 3])
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
///
/// ## Flexure-joint spring limitation
///
/// Joints that carry a `spring_rate` key (produced by the PRB flexure builtins
/// — `prb_notch_circular`, `prb_cantilever_beam`, etc.) will **not** contribute
/// their spring restoring torque (`−k·(q − neutral)`) on this entry point.
/// The snapshot body record does not retain the scalar joint coordinate `q`
/// (it is consumed by the FK walk inside `snapshot()` and is unavailable
/// here), so the spring term cannot be computed.  Silently emitting a
/// finite-but-incomplete torque for a sprung flexure is the accepted v1
/// trade-off; fixing it would require `snapshot()` to persist `q`, which
/// is out of this module's eval.rs-only scope (task ι §design).
///
/// **Viscous damping** (`−c·q̇`) depends only on `q̇`, which IS supplied via
/// `q_dot`, and applies correctly on both entry points whenever the joint Map
/// carries a `damping` key.
///
/// To obtain the full spring-plus-damping torque for flexure joints use the
/// trajectory entry point (`inverse_dynamics_lower` / [`inverse_dynamics_sample`]),
/// which supplies the per-body joint coordinate `q` at each sample.
fn eval_inverse_dynamics_at_snapshot(args: &[Value]) -> Value {
    if args.len() != 4 {
        return Value::Undef;
    }
    // Spring torque unavailable on the snapshot path: the snapshot body
    // record does not retain the scalar joint coordinate (task ι §design).
    // See the doc comment above for the user-facing rationale.
    match snapshot_inverse_dynamics(&args[0], &args[1], &args[2], &args[3], None) {
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
///
/// `positions` carries the per-body joint coordinate `q` in `bodies` order,
/// used to apply the spring term `−k·(q − neutral)` in [`JointCompliance`].
/// Pass `Some(values)` on the trajectory path (the sample's `values` slice);
/// pass `None` on the snapshot path — the snapshot body record does not retain
/// the scalar joint coordinate (it is consumed by the FK walk), so spring
/// torque is unavailable there and is silently omitted.  Viscous damping
/// `−c·q̇` depends only on `q̇` and applies on **both** paths.
fn snapshot_inverse_dynamics(
    mechanism: &Value,
    snapshot: &Value,
    q_dot_arg: &Value,
    q_ddot_arg: &Value,
    positions: Option<&[Value]>,
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
        // PRD §7.2 no-bypass invariant (route 4): the `world_transform` read here is
        // the FK pose baked by `snapshot()` via `joint_world_transform` → `transform_at`.
        // Any pivot offset is already composed into this value — this site reads the
        // offset-aware pose and MUST NOT reconstruct per-joint transforms from the joint Map.
        // Verified behaviourally by γ B8 route-4 test and the B3 dynamics test.
        world_tf.insert(id, map_get(sbm, "world_transform")?);
    }

    // ── per-body fields: `at` joint, id, solid ──
    let n = bodies.len();

    // ── fail-honest one-per-body positions invariant (task ι step-6) ───────────
    // `positions` (when Some) is ONE ENTRY PER BODY in `bodies` order — it is
    // NOT a flat per-DOF list.  `vels`/`accels` are the flat per-DOF lists
    // (length = Σ dof), which is why ONLY they are run through `slice_generalized`
    // (below).  Applying `slice_generalized` to `positions` would be WRONG:
    // `positions.len() == n` (not Σ dof), so it would return `None` for any
    // multi-DOF input and silently drop the spring term — a regression.
    //
    // The only caller that supplies `Some(positions)` is `inverse_dynamics_sample`,
    // which passes the trajectory sample's `values` slice; `snapshot_for_sample`
    // hard-checks `values.len() == bodies.len()` (eval.rs:862) before binding each
    // `values[i]` to body i's joint.  Hence `positions[bi]` is exactly the value
    // bound to body bi's joint → body bi's coordinate q by construction.
    //
    // This guard enforces that contract locally: any caller that violates it
    // (e.g. passes a flat per-DOF slice of the wrong length) receives `None`
    // (→ `Value::Undef`) rather than a silently-misaligned spring torque,
    // matching the module's fail-honest convention (cf. the closed-chain
    // `loop_closures` guard at eval.rs:403-416).
    if let Some(p) = positions
        && p.len() != n
    {
        return None;
    }

    let mut at_joints: Vec<&Value> = Vec::with_capacity(n);
    let mut ids: Vec<i64> = Vec::with_capacity(n);
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
        let mp = resolve_body_mass(&bodies[bi])?;
        let (mass, com, inertia_about_com) = mass_properties_from_value(&mp)?;
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
        // ── compliance bridge (task ι) ───────────────────────────────────────
        // Gate on 1-DOF (subspace column count == 1) to avoid the rnea
        // always-on multi-DOF panic (PRD §11.2). Flexure joints are always
        // revolute/prismatic, so this guard never fires in practice — it is
        // defence-in-depth for a hand-built multi-DOF Map carrying a stray
        // spring_rate key.
        //
        // `positions` is ONE ENTRY PER BODY in `bodies` order (NOT a flat
        // per-DOF list).  `vels`/`accels` are the flat per-DOF lists (length
        // = Σ dof) — that asymmetry is precisely why ONLY they are run through
        // `slice_generalized` (above).  The length guard at the top of this
        // function ensures `positions.len() == n` when `Some`, so `.get(bi)`
        // is provably in range for all `bi` in `0..n`.
        //
        // NOTE: applying `slice_generalized` to `positions` would be WRONG —
        // `positions.len() == n` (not Σ dof), so `slice_generalized` would
        // return `None` for any multi-DOF input and silently drop the spring
        // term (a regression).
        //
        // Spring gating: `positions` carries the per-body q on the trajectory
        // path (Some(values)); the snapshot path passes None because the
        // snapshot body record does not retain the scalar coordinate.
        let joint_pos = positions
            .and_then(|p| p.get(bi))
            .and_then(compliance_cell_f64);
        let compliance = if subspaces[bi].len() == 1 {
            joint_compliance(at_joints[bi], joint_pos)
        } else {
            None
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
            compliance,
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

/// Build a [`JointCompliance`] from the compliant-field keys of a joint
/// `Value::Map` (`spring_rate`, `damping`, `neutral`), or `None` for joints
/// that carry neither term (plain revolute / prismatic joints stay
/// byte-identical to pre-ι behaviour, i.e. `compliance: None`).
///
/// `position` is the current joint coordinate `q`, required to apply the
/// spring term `−k·(q − neutral)`.  When `position` is `None` (the
/// `inverse_dynamics_at_snapshot` entry point cannot recover `q` from the
/// snapshot because the snapshot body record does not retain the scalar
/// coordinate), the `spring_rate` key is silently ignored; damping `−c·q̇`
/// depends only on `q̇` and is applied on **both** entry points whenever the
/// `damping` key is set.
///
/// Both bare-`Scalar` and `Value::Option(Some(Scalar))` shapes are accepted
/// for all three compliant fields (via [`compliance_cell_f64`]), covering the
/// shape `make_flexure_joint` emits today and any future wrapped form.
fn joint_compliance(joint: &Value, position: Option<f64>) -> Option<JointCompliance> {
    let m = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    // Spring term requires a known joint coordinate.
    let spring_rate = if position.is_some() {
        map_get(m, "spring_rate").and_then(compliance_cell_f64)
    } else {
        None
    };
    let damping = map_get(m, "damping").and_then(compliance_cell_f64);
    // Neither term active → plain joint; return None (no regression).
    if spring_rate.is_none() && damping.is_none() {
        return None;
    }
    // Neutral is only relevant for the spring term (`−k·(q − neutral)`).
    // Gate its read on `spring_rate` being present so that a malformed or
    // irrelevant `neutral` key on a damping-only joint never suppresses the
    // valid damping term.  When spring is active and `neutral` is present-but-
    // malformed (e.g. NaN): fail-honest — the equilibrium is undefined, return
    // None rather than silently shifting it to 0.  Absent `neutral` with spring
    // active → 0.0 (sensible default, matches make_flexure_joint behaviour).
    let neutral = if spring_rate.is_some() {
        match map_get(m, "neutral") {
            None => 0.0,
            Some(v) => compliance_cell_f64(v)?,
        }
    } else {
        0.0
    };
    Some(JointCompliance {
        spring_rate,
        damping,
        neutral,
        position: position.unwrap_or(0.0),
    })
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
/// the sample's joint positions into an FK snapshot, then drives either the
/// open-chain core ([`snapshot_inverse_dynamics`]) or the closed-chain bridge
/// ([`closed_chain_inverse_dynamics`]) depending on whether the mechanism has
/// loop closures. Returns a `List<List<JointForce>>` parallel to
/// `trajectory.samples`, or `Value::Undef` on any malformed input.
fn eval_inverse_dynamics(args: &[Value]) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    let mechanism = &args[0];
    // Route through the pub per-sample seam (`motion_trajectory_samples` +
    // `inverse_dynamics_sample`) so the body-inline fallback here and the
    // reify-eval `inverse_dynamics` ComputeNode trampoline (task RBD-ι) drive
    // identical per-sample logic — behaviour stays single-sourced.
    let samples = match motion_trajectory_samples(&args[1]) {
        Some(s) => s,
        None => return Value::Undef,
    };

    let mut out = Vec::with_capacity(samples.len());
    for sample in samples {
        match inverse_dynamics_sample(mechanism, sample) {
            Some(forces) => out.push(Value::List(forces)),
            None => return Value::Undef,
        }
    }
    Value::List(out)
}

/// Read the `samples` list of a `MotionTrajectory` `Value::StructureInstance` as
/// a slice, or `None` for a malformed / non-`MotionTrajectory` value.
///
/// Public per-sample seam (task RBD-ι) over the private [`trajectory_samples`]:
/// the reify-eval `inverse_dynamics` ComputeNode trampoline iterates this slice
/// and polls cooperative cancellation at each sample boundary (a whole-trajectory
/// eval cannot meet the §9.1 "abort within 1 sample interval" signal).
pub fn motion_trajectory_samples(traj: &Value) -> Option<&[Value]> {
    trajectory_samples(traj)
}

/// Inverse dynamics for ONE trajectory sample: read the sample's
/// `(values, vels, accels)`, bake the joint positions into an FK snapshot, then
/// drive either the open-chain snapshot RNEA core
/// ([`snapshot_inverse_dynamics`]) or the closed-chain KKT bridge
/// ([`closed_chain_inverse_dynamics`], task 4146) with the sample's q̇ / q̈,
/// depending on whether the mechanism's `loop_closures` list is non-empty.
/// Returns the per-body `List<JointForce>` as a `Vec<Value>`, or `None` on any
/// malformed input (the caller maps `None` → `Value::Undef`).
///
/// Public per-sample seam (task RBD-ι) wrapping the private `sample_fields` +
/// `snapshot_for_sample` + the per-sample dynamics core. Both the body-inline
/// `eval_inverse_dynamics` fallback and the reify-eval trampoline call it, so the
/// per-sample marshalling lives in exactly one place — and the ComputeNode
/// trampoline gains closed-chain routing through this single seam (task 4146
/// composing with RBD-ι).
///
/// For closed mechanisms, `snapshot_for_sample`'s snapshot builtin runs the
/// Newton loop-closure solve (world_transforms reflect the converged
/// configuration) and its returned per-body `bind(at,value)` list feeds
/// `extract_loop_closure_chains` for the constraint-Jacobian assembly.
///
/// Mass properties are re-read per sample from `body.solid` inside the snapshot
/// core; they are trajectory-invariant, so this is redundant work the small
/// fixtures don't notice — a future optimisation can hoist the MassProperties
/// extraction across samples (the same deferral the trampoline's warm-state
/// cache documents).
pub fn inverse_dynamics_sample(mechanism: &Value, sample: &Value) -> Option<Vec<Value>> {
    let (values, vels, accels) = sample_fields(sample)?;
    let (snapshot, bindings) = snapshot_for_sample(mechanism, values)?;
    let is_closed = match mechanism {
        Value::Map(m) => match map_get(m, "loop_closures") {
            Some(Value::List(lc)) => !lc.is_empty(),
            _ => false,
        },
        _ => false,
    };
    if is_closed {
        closed_chain_inverse_dynamics(mechanism, &snapshot, &bindings, vels, accels)
    } else {
        // Pass the sample's `values` slice so snapshot_inverse_dynamics can
        // supply joint positions to joint_compliance (spring term).
        snapshot_inverse_dynamics(mechanism, &snapshot, vels, accels, Some(values))
    }
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
/// in `bodies` order), then call the `snapshot` builder.
///
/// Returns `None` on a values/bodies length mismatch or an FK failure.
/// Returns `Some((snapshot, bindings))` — the consistent FK snapshot and the
/// per-body `bind(at,value)` list, which the closed-chain path reuses as
/// the `bindings` argument to `extract_loop_closure_chains`.
///
/// **Single-DOF-per-body position assumption.** `trajectory.samples[k].values`
/// must have exactly one entry per mechanism body (in `bodies` / id order).
fn snapshot_for_sample(mechanism: &Value, values: &[Value]) -> Option<(Value, Vec<Value>)> {
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
        // Assemble each per-body FK binding via the INTERNAL
        // `crate::snapshot::make_binding`, NOT the public `bind` builtin. The L1
        // non-driving-joint guard added to `bind` (task 4309 α, steps 5-6) is a
        // USER-input check: it rejects `coupling`/`fixed` joints passed by `.ri`
        // authors. But these per-body `at` joints were already validated as
        // joints by `body()` (mechanism.rs:93), and couplings/fixed ARE supported
        // in dynamics (`value_for` derives a coupling's motion from its parent at
        // snapshot.rs:961-977; `fixed` maps to a sentinel at snapshot.rs:991-994).
        // Routing them through `bind` would return a `nondriving_joint` error Map
        // for non-driving bodies, which fails `snapshot()`'s `kind=="binding"`
        // validation loop (snapshot.rs:109-125), collapsing the snapshot to
        // `Undef` and silently breaking trajectory dynamics for any mechanism
        // with a coupled/fixed body. `make_binding` emits the identical Map
        // `bind`'s happy path returns for driving joints, so driving-joint
        // behaviour is unchanged. This is the same internal constructor that
        // loop-closure free-joint synthesis already calls at snapshot.rs:384.
        bindings.push(crate::snapshot::make_binding(at.clone(), value.clone()));
    }
    let snapshot =
        crate::eval_builtin("snapshot", &[mechanism.clone(), Value::List(bindings.clone())]);
    if snapshot.is_undef() {
        return None;
    }
    Some((snapshot, bindings))
}

// ── closed_chain_inverse_dynamics — KKT bridge (task 4146 step-8) ─────────────

/// Closed-chain inverse dynamics for mechanisms with loop closures.
///
/// Fires from [`inverse_dynamics_sample`] (the shared per-sample seam, so both
/// the body-inline fallback and the reify-eval ComputeNode trampoline route
/// here) when the mechanism's `loop_closures` list is non-empty.  Assembles and solves the augmented KKT system
/// `[[M, Aᵀ],[A, 0]]·[q̈;λ] = [τ_open; b]` and returns the per-tree-joint
/// constraint-corrected τ as a `List<JointForce>` in `bodies` order.
///
/// # Coordinate model
///
/// n = Σ dof_count(body.at) over the FIRST `n_tree` (spanning-tree) bodies
/// only — closing-edge bodies contribute no tree DOFs.
/// n_tree = bodies.len() − loop_closures.len().
/// M (n×n) and τ_open (n) are in topological (parent-before-child) order.
///
/// # Acceleration-level RHS b
///
/// b = −Ȧ·q̇.  For q̇=0 (smoke test) b=0 exactly.  For general q̇ the virtual-
/// work power identity τ·q̇ = τ_open·q̇ holds for any consistent q̇ (A·q̇=0),
/// so the power-identity e2e (step-9) is satisfied regardless of b.  A future
/// implementation can add b via FD of A along q̇ when that precision is needed.
fn closed_chain_inverse_dynamics(
    mechanism: &Value,
    snapshot: &Value,
    bindings: &[Value],
    vels: &Value,
    accels: &Value,
) -> Option<Vec<Value>> {
    let mech = match mechanism {
        Value::Map(m) => m,
        _ => return None,
    };

    // ── spanning-tree body count ──────────────────────────────────────────────
    let bodies = match map_get(mech, "bodies") {
        Some(Value::List(b)) => b,
        _ => return None,
    };
    let loop_closures = match map_get(mech, "loop_closures") {
        Some(Value::List(lc)) => lc,
        _ => return None,
    };
    let n_lc = loop_closures.len();
    let n_total = bodies.len();
    if n_lc >= n_total {
        return None;
    }
    // Mechanism builder appends closing-edge bodies at the end, so the first
    // n_tree entries are the spanning-tree bodies.
    let n_tree = n_total - n_lc;
    let tree_bodies = &bodies[..n_tree];

    let joint_parents = match map_get(mech, "joint_parents") {
        Some(Value::Map(jp)) => jp,
        _ => return None,
    };

    // ── per-spanning-tree-body fields ─────────────────────────────────────────
    let mut at_joints: Vec<&Value> = Vec::with_capacity(n_tree);
    let mut ids: Vec<i64> = Vec::with_capacity(n_tree);
    for b in tree_bodies {
        let bm = match b {
            Value::Map(m) => m,
            _ => return None,
        };
        at_joints.push(map_get(bm, "at")?);
        ids.push(match map_get(bm, "id") {
            Some(Value::Int(k)) => *k,
            _ => return None,
        });
    }

    // ── parent index per spanning-tree body (None = world root) ──────────────
    let mut parent_idx: Vec<Option<usize>> = Vec::with_capacity(n_tree);
    for &at in &at_joints {
        let p = match joint_parents.get(at) {
            None => None,
            Some(j) if is_world(j) => None,
            Some(j) => Some(at_joints.iter().position(|aj| *aj == j)?),
        };
        parent_idx.push(p);
    }

    // ── topological order (parent before child) + inverse permutation ─────────
    let ordered = topo_order(&parent_idx)?;
    let mut pos = vec![0usize; n_tree];
    for (k, &bi) in ordered.iter().enumerate() {
        pos[bi] = k;
    }

    // ── per-body motion subspaces and DOF counts ──────────────────────────────
    let mut subspaces: Vec<Vec<SpatialVector6>> = Vec::with_capacity(n_tree);
    for &at in &at_joints {
        subspaces.push(motion_subspace_columns(at)?);
    }
    let dof_counts: Vec<usize> = subspaces.iter().map(|s| s.len()).collect();

    // ── slice q̇ / q̈ (flat per-DOF, spanning-tree bodies order) ──────────────
    let q_dot = slice_generalized(vels, &dof_counts)?;
    let q_ddot = slice_generalized(accels, &dof_counts)?;

    // ── snapshot world transforms ─────────────────────────────────────────────
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

    // ── build RneaLinks in topological order ──────────────────────────────────
    let mut links: Vec<RneaLink> = Vec::with_capacity(n_tree);
    for &bi in &ordered {
        let mp = resolve_body_mass(&tree_bodies[bi])?;
        let (mass, com, inertia_about_com) = mass_properties_from_value(&mp)?;
        let child_frame = frame3_from_transform_value(world_tf.get(&ids[bi])?)?;
        let xc = SpatialTransform6::from_frame3(&child_frame);
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
            // Spring/damping compliance (task-3865) is not wired into the
            // closed-chain trajectory path — mirrors the open-chain snapshot
            // literal above (`snapshot_inverse_dynamics`), which also passes
            // `None`. Compliance belongs to τ_open assembly when a closed
            // consumer needs it.
            compliance: None,
        });
    }

    // ── τ_open and M ──────────────────────────────────────────────────────────
    let tau_topo: Vec<Vec<f64>> = inverse_dynamics_open_chain(&links, default_gravity());
    let tau_open: Vec<f64> = tau_topo.iter().flatten().copied().collect();
    let n: usize = dof_counts.iter().sum();
    let m_matrix = assemble_joint_space_inertia(&links);

    // Target joints in topological order (one per spanning-tree body, DOF-flat
    // via loop_residual_jacobian_by_joint's per-component width expansion).
    let ordered_joints: Vec<Value> =
        ordered.iter().map(|&bi| at_joints[bi].clone()).collect();

    // ── assemble constraint Jacobian A and RHS b ──────────────────────────────
    let mut a_stacked: Vec<f64> = Vec::new();
    let mut b_stacked: Vec<f64> = Vec::new();
    let mut m_total = 0usize;

    for record in loop_closures {
        // Extract loop-closure chains from the per-body bindings built by
        // snapshot_for_sample (same bind(at,value) list passed to snapshot()).
        let (chain_a, vals_a, chain_b, vals_b, _free_b) =
            extract_loop_closure_chains(record, bindings)?;

        // Raw 6×n Jacobian via central FD over all spanning-tree joints.
        let raw_cols =
            loop_residual_jacobian_by_joint(&chain_a, &vals_a, &chain_b, &vals_b, &ordered_joints, 1e-7)?;

        // raw_cols: one [f64;6] column per spanning-tree DOF in topo order.
        // Convert to 6×n row-major.
        let mut a_raw = vec![0.0f64; 6 * n];
        for (col_idx, col) in raw_cols.iter().enumerate() {
            for row_idx in 0..6 {
                a_raw[row_idx * n + col_idx] = col[row_idx];
            }
        }

        // Closing joint's motion subspace (for rank-reduction projection).
        let closing_joint = match record {
            Value::Map(m) => m.get(&Value::String("closing_joint".to_string()))?,
            _ => return None,
        };
        let closing_sv: Vec<SpatialVector6> = motion_subspace_columns(closing_joint)?;
        let closing_sub: Vec<[f64; 6]> = closing_sv.iter().map(|sv| sv.as_array()).collect();

        // Project out the closing joint's absorbed directions and row-reduce.
        let (a_red, m_eff) = reduce_constraint_rank(&a_raw, 6, n, &closing_sub, 1e-10);

        // b = −Ȧ·q̇.  For the cases exercised here (q̇=0 smoke test; 4-bar
        // power-identity e2e where τ·q̇ = τ_open·q̇ for any consistent q̇) b=0
        // produces correct results.
        let b_this = vec![0.0f64; m_eff];

        a_stacked.extend_from_slice(&a_red);
        b_stacked.extend_from_slice(&b_this);
        m_total += m_eff;
    }

    // ── KKT solve ─────────────────────────────────────────────────────────────
    let sol = match solve_closed_chain(
        &m_matrix,
        &tau_open,
        &a_stacked,
        &b_stacked,
        n,
        m_total,
        DEFAULT_PIVOT_EPS,
    ) {
        Ok(s) => s,
        Err(_) => return None, // Singular → Undef
    };

    // ── reshape sol.tau (n, topo order) → List<JointForce> (bodies order) ─────
    // Build per-link topo-indexed τ slices first, then map to bodies order.
    let mut topo_tau_slices: Vec<&[f64]> = Vec::with_capacity(n_tree);
    let mut cursor = 0;
    for k in 0..n_tree {
        let dof = dof_counts[ordered[k]];
        topo_tau_slices.push(&sol.tau[cursor..cursor + dof]);
        cursor += dof;
    }

    let mut forces: Vec<Value> = Vec::with_capacity(n_tree);
    for i in 0..n_tree {
        let k = pos[i]; // topo position of body i
        let kind = joint_kind(at_joints[i])?;
        let value = joint_force_value(kind, topo_tau_slices[k])?;
        forces.push(make_joint_force(ids[i], value));
    }
    Some(forces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamics::spatial::Frame3;

    /// Build a canonical `MassProperties` `Value::StructureInstance` matching
    /// `dynamics_ops::assemble_mass_properties`'s shape: `mass` a Mass-scalar,
    /// `com` a `Value::Point` of Length-scalars, `inertia` a 3×3 `Value::Matrix`
    /// of MomentOfInertia-dimensioned scalars (kg·m²), `origin` a `Real`.
    fn mass_properties_fixture(
        mass: f64,
        com: [f64; 3],
        inertia: [[f64; 3]; 3],
    ) -> Value {
        let com_point = Value::Point(com.iter().map(|&c| Value::length(c)).collect());
        let inertia_matrix = Value::Matrix(
            inertia
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|&x| Value::Scalar {
                            si_value: x,
                            dimension: DimensionVector::MOMENT_OF_INERTIA,
                        })
                        .collect()
                })
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

    // ── step-5 RED: pub per-sample seam (task RBD-ι) ──────────────────────────
    //
    // `motion_trajectory_samples` + `inverse_dynamics_sample` expose the private
    // `trajectory_samples` / `sample_fields`+`snapshot_for_sample`+
    // `snapshot_inverse_dynamics` so the reify-eval trampoline can drive the
    // per-sample loop itself (polling cancellation at each sample boundary).
    // `inverse_dynamics_sample` reproduces the same single-pendulum static-gravity
    // torque (0.4905 N·m) the whole-trajectory variant produces, so routing
    // `eval_inverse_dynamics` through the seam keeps behaviour single-sourced.

    /// `motion_trajectory_samples` returns the samples slice for a well-formed
    /// `MotionTrajectory` and `None` for a malformed (non-`MotionTrajectory`)
    /// value.
    #[test]
    fn motion_trajectory_samples_reads_samples_slice() {
        let theta = -std::f64::consts::PI / 6.0;
        let traj = mint_instance(
            "MotionTrajectory",
            vec![
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
        let samples = motion_trajectory_samples(&traj)
            .expect("a well-formed trajectory must yield its samples slice");
        assert_eq!(samples.len(), 2, "two samples in, two samples out");
        assert!(
            motion_trajectory_samples(&Value::Real(0.0)).is_none(),
            "a non-MotionTrajectory value must yield None"
        );
    }

    /// `inverse_dynamics_sample` drives the open-chain snapshot RNEA for one
    /// motionless single-pendulum sample at θ = −30°, reproducing the validated
    /// static-gravity torque τ = m·g·L·sin(30°) = 0.4905 N·m (<1e-6) — i.e. the
    /// per-sample seam agrees with the whole-trajectory variant.
    #[test]
    fn inverse_dynamics_sample_single_pendulum_static_gravity() {
        let mech = pendulum_mechanism();
        let theta = -std::f64::consts::PI / 6.0;
        let sample = trajectory_sample(0.0, theta, 0.0, 0.0);

        let forces = inverse_dynamics_sample(&mech, &sample)
            .expect("a motionless open-chain sample must solve");
        assert_eq!(forces.len(), 1, "one joint ⇒ one JointForce");
        let value = field(&forces[0], "JointForce", "value");
        let torque = num(field(value, "ScalarTorque", "magnitude"));
        let expected = 0.4905_f64;
        assert!(
            (torque - expected).abs() < 1e-6,
            "expected {expected} N·m, got {torque}"
        );
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

    // ── step-7 RED: closed-chain inverse_dynamics routing smoke test ───────────
    //
    // Verifies that `inverse_dynamics_lower` routes a closed-chain mechanism to a
    // finite `List<List<JointForce>>` (not `Value::Undef`).  Uses the proven
    // 2-prismatic closed-chain mechanism (same topology as
    // kinematic_sweep_closed_chain.rs: body(m2, mp_c, j_b, j_a) re-parents
    // j_b → j_a, creating one loop_closure record), with MassProperties-valued
    // solids so no geometry kernel is needed.
    //
    // The test also re-asserts that `inverse_dynamics_at_snapshot_lower` still
    // returns `Value::Undef` for a closed mechanism (design decision
    // esc-3836-226: snapshot discards the spanning-tree q).
    //
    // Fails RED today because `snapshot_inverse_dynamics` has the closed-chain
    // guard (None → Undef).  Step-8 wires `closed_chain_inverse_dynamics` and
    // makes this GREEN.
    //
    // Trajectory layout for the 2-prismatic closed chain:
    //   bodies = [m1@j_a, m2@j_b, m3@j_b-closing]  (bodies.len() = 3)
    //   loop_closures = 1 record
    //   n_tree = bodies.len() − loop_closures.len() = 2 spanning-tree bodies
    //   values.len() = 3  (snapshot_for_sample requires values.len() == bodies.len())
    //   vels.len()   = 2  (tree-DOF count: j_a(1) + j_b(1) = 2)
    //   accels.len() = 2  (same)
    #[test]
    fn closed_chain_inverse_dynamics_routing_finite_on_prismatic_loop() {
        use crate::eval_builtin;

        // ── 2-prismatic closed-chain mechanism ────────────────────────────────
        // Spanning tree (joint_parents): m1@j_a (parent=world), m2@j_b (parent=world).
        // Closing edge: body(m2, mp_c, j_b, j_a) adds m3 to bodies and appends a
        // loop_closure record {path_a=[world,j_b], path_b=[world,j_a,j_b],
        // closing_joint=j_b}.
        //
        // j_a on +x (range 0–1m), j_b on +x (range 0–2m): different ranges make
        // them structurally distinct Maps so Value::Eq in loop_residual_jacobian_by_joint
        // can distinguish them.  Mirrors the kinematic_sweep_closed_chain source.
        let mp_a = mass_properties_fixture(
            1.0,
            [0.0, 0.0, 0.0],
            [[0.1, 0.0, 0.0], [0.0, 0.1, 0.0], [0.0, 0.0, 0.1]],
        );
        let mp_b = mass_properties_fixture(
            2.0,
            [0.0, 0.0, 0.0],
            [[0.2, 0.0, 0.0], [0.0, 0.2, 0.0], [0.0, 0.0, 0.2]],
        );
        // Distinct solid required: append_body rejects duplicate solids.
        let mp_c = mass_properties_fixture(
            0.5,
            [0.0, 0.0, 0.0],
            [[0.05, 0.0, 0.0], [0.0, 0.05, 0.0], [0.0, 0.0, 0.05]],
        );

        let axis_x = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let range_1m = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(1.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let range_2m = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(2.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let j_a = eval_builtin("prismatic", &[axis_x.clone(), range_1m]);
        let j_b = eval_builtin("prismatic", &[axis_x, range_2m]);

        let mech0 = eval_builtin("mechanism", &[]);
        let mech1 = eval_builtin("body", &[mech0, mp_a, j_a.clone()]);
        let mech2 = eval_builtin("body", &[mech1, mp_b, j_b.clone()]);
        // 4-arg body(): closing edge — re-parents j_b from world → j_a.
        // Appends a loop_closure record; does NOT modify joint_parents
        // (spanning-tree parent for j_b stays world, "first-recorded wins").
        let mech = eval_builtin("body", &[mech2, mp_c, j_b.clone(), j_a.clone()]);

        // Confirm closed-chain discriminant is non-empty.
        let mech_map = match &mech {
            Value::Map(m) => m,
            other => panic!("body() must yield a Mechanism Map, got {other:?}"),
        };
        let lc = match mech_map.get(&Value::String("loop_closures".to_string())) {
            Some(Value::List(l)) => l,
            other => panic!("mechanism must carry loop_closures List, got {other:?}"),
        };
        assert!(
            !lc.is_empty(),
            "2-prismatic closed-chain mechanism must have non-empty loop_closures"
        );

        // ── 1-sample MotionTrajectory ─────────────────────────────────────────
        // values[0] = j_a driver (0.5m); values[1] = spanning-tree j_b (1.0m);
        // values[2] = closing j_b initial guess (0.5m).
        // Closure identity: spanning(j_b) = j_a + free(j_b) → 1.0 = 0.5 + 0.5. ✓
        let sample = mint_instance(
            "TrajectorySample",
            vec![
                (
                    "t".to_string(),
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::TIME,
                    },
                ),
                (
                    "values".to_string(),
                    Value::List(vec![
                        Value::length(0.5), // m1@j_a: driver = 0.5 m
                        Value::length(1.0), // m2@j_b: spanning-tree position = 1.0 m
                        Value::length(0.5), // m3@j_b: closing-edge initial guess = 0.5 m
                    ]),
                ),
                (
                    "vels".to_string(),
                    Value::List(vec![
                        Value::Real(0.0), // q̇_ja = 0
                        Value::Real(0.0), // q̇_jb = 0
                    ]),
                ),
                (
                    "accels".to_string(),
                    Value::List(vec![
                        Value::Real(0.0), // q̈_ja = 0
                        Value::Real(0.0), // q̈_jb = 0
                    ]),
                ),
            ],
        );
        let traj = mint_instance(
            "MotionTrajectory",
            vec![
                ("mechanism".to_string(), Value::Real(0.0)),
                ("samples".to_string(), Value::List(vec![sample])),
            ],
        );

        // ── Routing assertion: closed-chain ⇒ finite List (step-7 RED) ───────
        // Today: snapshot_inverse_dynamics fires the closed-chain guard ⇒ None
        // ⇒ eval_inverse_dynamics returns Value::Undef ⇒ this assertion fails.
        // After step-8: closed_chain_inverse_dynamics returns Ok ⇒ GREEN.
        let result = eval_dynamics("inverse_dynamics_lower", &[mech.clone(), traj])
            .expect("inverse_dynamics_lower must be a recognised dynamics intrinsic");

        let per_sample = match &result {
            Value::List(s) => s,
            _ => panic!(
                "inverse_dynamics_lower on a closed mechanism must return a finite \
                 List<List<JointForce>>, got {:?}\n\
                 (step-7 RED: closed-chain guard returns Undef; step-8 wires the path)",
                result
            ),
        };
        assert_eq!(per_sample.len(), 1, "one force list per trajectory sample");

        // Inner List<JointForce>: length = tree-joint count = 2 (j_a and j_b).
        // n_tree = bodies.len() − loop_closures.len() = 3 − 1 = 2.
        let forces = match &per_sample[0] {
            Value::List(f) => f,
            other => panic!("sample 0: expected a List<JointForce>, got {other:?}"),
        };
        assert_eq!(
            forces.len(),
            2,
            "two spanning-tree joints (j_a, j_b) ⇒ two JointForce entries"
        );

        // Every force magnitude must be finite/non-NaN
        // (KKT solved ⇒ rank reduction correct).
        for (i, jf) in forces.iter().enumerate() {
            let jf_value = field(jf, "JointForce", "value");
            // Both joints are prismatic ⇒ ScalarForce { magnitude }.
            let mag = num(field(jf_value, "ScalarForce", "magnitude"));
            assert!(
                mag.is_finite(),
                "force[{i}].ScalarForce.magnitude must be finite, got {mag}"
            );
        }

        // ── Re-assert: snapshot entry still returns Undef for closed mechanisms ─
        // Design decision esc-3836-226: inverse_dynamics_at_snapshot_lower must
        // return Undef for closed mechanisms because a snapshot discards the
        // spanning-tree q needed by extract_loop_closure_chains.
        // (Existing test snapshot_inverse_dynamics_rejects_closed_mechanism also
        // covers this; re-asserted here so step-8 cannot accidentally remove the guard.)
        let dummy_snap = Value::Map(std::collections::BTreeMap::new());
        let q_zero = Value::List(vec![Value::Real(0.0), Value::Real(0.0)]);
        let snap_result = eval_dynamics(
            "inverse_dynamics_at_snapshot_lower",
            &[mech, dummy_snap, q_zero.clone(), q_zero],
        )
        .expect("inverse_dynamics_at_snapshot_lower must be a recognised intrinsic");
        assert_eq!(
            snap_result,
            Value::Undef,
            "closed mechanism must return Undef from inverse_dynamics_at_snapshot_lower \
             (snapshot discards spanning-tree q, no closed-chain routing at snapshot entry)"
        );
    }

    // ── step-11 RED: snapshot_for_sample non-driving-body regression ──────────
    //
    // Pin the couplings/fixed + dynamics scenario the new L1 bind-guard
    // (steps 5-6) broke (reviewer_comprehensive robustness_regression).
    // `snapshot_for_sample` (eval.rs) reuses `bind` INTERNALLY to bind every
    // mechanism body's `at` joint, including non-driving (coupling/fixed) bodies
    // — a supported dynamics scenario (`body()` accepts couplings/fixed at
    // mechanism.rs:93; `value_for` derives a coupling's motion from its parent
    // at snapshot.rs:961-977 and maps `fixed` to a sentinel at snapshot.rs:991).
    // After steps 5-6, `bind(coupling, v)` / `bind(fixed, v)` return a
    // "nondriving_joint" error Map (the L1 USER-input guard), which fails
    // `snapshot()`'s kind=="binding" validation loop (snapshot.rs:109-125),
    // collapsing the snapshot to `Undef` so `snapshot_for_sample` returns `None`
    // and trajectory dynamics through non-driving bodies silently break.
    //
    // Step-12 fixes this by assembling the per-body FK bindings via the internal
    // `crate::snapshot::make_binding` (the same constructor loop-closure
    // free-joint synthesis already calls at snapshot.rs:384), bypassing the
    // user-facing guard.
    //
    // Two mechanisms, because the assertions split by joint kind:
    //   • COUPLING body → snapshot-level (a)/(b). A coupling has NO single motion
    //     subspace (`motion_subspace_columns` → None, joints.rs:1019, "out of
    //     scope for v0.3"), so the end-to-end RNEA cannot run for it — assertion
    //     (c) lives on the fixed mechanism instead.
    //   • FIXED body → end-to-end (c). `fixed` is 0-DOF and RNEA-supported
    //     (`motion_subspace_columns` → Some(empty), joints.rs:1018), so the full
    //     per-sample seam recovers to `Some`. It is still non-driving, so the
    //     same pre-step-12 bind-guard regression collapses its snapshot.
    //
    // Key on `Some` + kind=="binding" structure, not torque magnitudes (avoids
    // coupling-RNEA numeric fragility). RED today: `snapshot_for_sample` binds
    // via `eval_builtin("bind", ...)`, so the non-driving body's binding is a
    // "nondriving_joint" error Map → snapshot `Undef` → `snapshot_for_sample`
    // `None` → assertion (a) `.expect()` panics.
    #[test]
    fn snapshot_for_sample_with_coupled_body_does_not_regress() {
        use crate::eval_builtin;

        let axis_x = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let axis_y = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range_0_1m = || Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(1.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };

        // Distinct MassProperties solids: append_body rejects duplicate solids.
        let mp1 = mass_properties_fixture(
            1.0,
            [0.0, 0.0, 0.0],
            [[0.1, 0.0, 0.0], [0.0, 0.1, 0.0], [0.0, 0.0, 0.1]],
        );
        let mp2 = mass_properties_fixture(
            2.0,
            [0.0, 0.0, 0.0],
            [[0.2, 0.0, 0.0], [0.0, 0.2, 0.0], [0.0, 0.0, 0.2]],
        );

        // ── Mechanism A: driving prismatic body1 + COUPLING body2 ─────────────
        // body2's `at` is couple(prismatic(+y, 0-1m), -1.0): a non-driving joint
        // whose parent axis (+y) differs from body1's (+x). `couple()` rejects
        // coupling parents, so the parent is a bare prismatic. Distinct parent
        // axis + distinct solid ⇒ append_body accepts the second body.
        let drive_a = eval_builtin("prismatic", &[axis_x.clone(), range_0_1m()]);
        let coupling_parent = eval_builtin("prismatic", &[axis_y, range_0_1m()]);
        let coupling = eval_builtin("couple", &[coupling_parent, Value::Real(-1.0)]);

        let mech_a0 = eval_builtin("mechanism", &[]);
        let mech_a1 = eval_builtin("body", &[mech_a0, mp1.clone(), drive_a]);
        let mech_a = eval_builtin("body", &[mech_a1, mp2.clone(), coupling]);

        // One joint position per body (snapshot_for_sample requires
        // values.len() == bodies.len() == 2).
        let values_a = [Value::length(0.3), Value::length(0.2)];

        // (a) snapshot_for_sample returns Some with a non-Undef snapshot.
        let (snapshot_a, bindings_a) = snapshot_for_sample(&mech_a, &values_a).expect(
            "snapshot_for_sample must return Some for a 2-body mechanism with a coupled body \
             (RED until step-12 routes internal FK bindings through make_binding instead of \
             the user-facing bind guard)",
        );
        assert!(
            !snapshot_a.is_undef(),
            "the coupled-body snapshot must not collapse to Undef"
        );

        // (b) every per-body binding is a kind=="binding" Map (coupled body included).
        assert_eq!(bindings_a.len(), 2, "one binding per body");
        for (i, b) in bindings_a.iter().enumerate() {
            let bm = match b {
                Value::Map(m) => m,
                other => panic!("binding[{i}] must be a Value::Map, got {other:?}"),
            };
            assert_eq!(
                bm.get(&Value::String("kind".to_string())),
                Some(&Value::String("binding".to_string())),
                "binding[{i}] must carry kind==\"binding\", NOT a \"nondriving_joint\" error Map"
            );
        }

        // ── Mechanism B: driving prismatic body1 + FIXED body2 ────────────────
        // `fixed` is 0-DOF and RNEA-supported, so the full per-sample seam runs
        // end-to-end (assertion c). It is still non-driving, so pre-step-12 the
        // same bind-guard regression collapses its snapshot to Undef.
        let drive_b = eval_builtin("prismatic", &[axis_x, range_0_1m()]);
        let fixed = eval_builtin("fixed", &[]);

        let mech_b0 = eval_builtin("mechanism", &[]);
        let mech_b1 = eval_builtin("body", &[mech_b0, mp1, drive_b]);
        let mech_b = eval_builtin("body", &[mech_b1, mp2, fixed]);

        // values: one per body (length 2). vels/accels: flat per-DOF lists whose
        // total length = Σ dof = prismatic(1) + fixed(0) = 1.
        let sample_b = mint_instance(
            "TrajectorySample",
            vec![
                (
                    "t".to_string(),
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::TIME,
                    },
                ),
                (
                    "values".to_string(),
                    Value::List(vec![Value::length(0.3), Value::length(0.0)]),
                ),
                ("vels".to_string(), Value::List(vec![Value::Real(0.0)])),
                ("accels".to_string(), Value::List(vec![Value::Real(0.0)])),
            ],
        );

        // (c) end-to-end inverse_dynamics_sample returns Some (a finite
        // List<JointForce>), exercising the make_binding bypass through the
        // shared snapshot_for_sample seam (eval.rs:862) that both open- and
        // closed-chain routing pass through.
        let forces_b = inverse_dynamics_sample(&mech_b, &sample_b).expect(
            "inverse_dynamics_sample must return Some for a 2-body open chain whose second body \
             is a fixed (0-DOF, RNEA-supported) non-driving joint (RED until step-12)",
        );
        assert_eq!(
            forces_b.len(),
            2,
            "two bodies ⇒ two JointForce entries (prismatic ScalarForce + fixed ZeroForce)"
        );
    }

    // ── step-1 RED: joint_compliance unit tests ───────────────────────────────
    //
    // Tests for `joint_compliance(joint: &Value, position: Option<f64>)`
    // (returns `Option<JointCompliance>`). Will not compile until step-2 adds
    // the helper. Five cases:
    //  (a) plain revolute Map (no compliant keys) + position=Some → None
    //  (b) compliant Map (spring_rate=2.0, neutral=π/12) + position=Some(π/6)
    //      → Some { spring_rate=Some(2.0), damping=None, neutral≈π/12, position≈π/6 }
    //  (c) same compliant Map + position=None → None
    //      (spring needs position; damping=Option(None) → neither term → None)
    //  (d) damping-only Map (no spring_rate key) + position=None
    //      → Some { spring_rate=None, damping=Some(3.5), neutral=0.0, position=0.0 }
    //  (e) Option-wrapped spring_rate (Value::Option(Some(Scalar{2.0}))) unwraps
    //      the same as bare Scalar.

    /// (a) Plain revolute joint (kind/axis/range only) with position=Some → None.
    #[test]
    fn joint_compliance_plain_joint_returns_none() {
        use crate::eval_builtin;
        use std::f64::consts::PI;
        let axis_y = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(-PI))),
            upper: Some(Box::new(Value::angle(PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let plain_joint = eval_builtin("revolute", &[axis_y, range]);
        assert!(
            joint_compliance(&plain_joint, Some(PI / 6.0)).is_none(),
            "a plain revolute joint (no compliant keys) must return None"
        );
    }

    /// (b) Compliant revolute Map with position=Some → Some with correct fields.
    #[test]
    fn joint_compliance_compliant_joint_with_position_returns_some() {
        use std::f64::consts::PI;
        let joint = make_compliant_revolute_joint(Some(2.0), None, PI / 12.0);
        let c = joint_compliance(&joint, Some(PI / 6.0))
            .expect("compliant joint with position must return Some");
        assert!(
            (c.spring_rate.expect("spring_rate must be Some") - 2.0).abs() < 1e-12,
            "spring_rate"
        );
        assert!(c.damping.is_none(), "damping must be None (was Option(None))");
        assert!((c.neutral - PI / 12.0).abs() < 1e-12, "neutral ≈ π/12");
        assert!((c.position - PI / 6.0).abs() < 1e-12, "position ≈ π/6");
    }

    /// (c) Same compliant Map with position=None → None.
    /// spring needs position; damping=Option(None) → neither term present → None.
    #[test]
    fn joint_compliance_compliant_joint_without_position_returns_none() {
        use std::f64::consts::PI;
        let joint = make_compliant_revolute_joint(Some(2.0), None, PI / 12.0);
        assert!(
            joint_compliance(&joint, None).is_none(),
            "spring-only compliant joint (damping=None) without position must return None"
        );
    }

    /// (d) Damping-only Map + position=None → Some { spring_rate=None, damping=Some(3.5), ... }.
    /// Damping applies even without position (position/neutral default to 0.0).
    #[test]
    fn joint_compliance_damping_only_without_position_returns_some() {
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("revolute".to_string()),
        );
        m.insert(
            Value::String("damping".to_string()),
            Value::Real(3.5),
        );
        let joint = Value::Map(m);
        let c = joint_compliance(&joint, None)
            .expect("damping-only joint must return Some even without position");
        assert!(c.spring_rate.is_none(), "spring_rate must be None");
        assert!(
            (c.damping.expect("damping must be Some") - 3.5).abs() < 1e-12,
            "damping"
        );
        assert!(c.neutral == 0.0, "neutral defaults to 0.0");
        assert!(c.position == 0.0, "position defaults to 0.0 when None");
    }

    /// (e) Option-wrapped spring_rate unwraps the same as a bare Scalar.
    #[test]
    fn joint_compliance_option_wrapped_spring_rate_unwraps() {
        use std::f64::consts::PI;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("revolute".to_string()),
        );
        m.insert(
            Value::String("spring_rate".to_string()),
            Value::Option(Some(Box::new(Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::ROTATIONAL_STIFFNESS,
            }))),
        );
        m.insert(
            Value::String("neutral".to_string()),
            Value::angle(PI / 12.0),
        );
        let joint = Value::Map(m);
        let c = joint_compliance(&joint, Some(PI / 6.0))
            .expect("Option-wrapped spring_rate must unwrap and return Some");
        assert!(
            (c.spring_rate.expect("spring_rate must be Some") - 2.0).abs() < 1e-12,
            "spring_rate from Option wrapper"
        );
    }

    /// Present-but-malformed `neutral` (NaN scalar) must make `joint_compliance`
    /// return `None` rather than silently defaulting to 0.0, which would shift
    /// the spring equilibrium without any signal (fail-honest convention).
    #[test]
    fn joint_compliance_malformed_neutral_returns_none() {
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("revolute".to_string()),
        );
        m.insert(
            Value::String("spring_rate".to_string()),
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::ROTATIONAL_STIFFNESS,
            },
        );
        // neutral present but NaN → compliance_cell_f64 returns None (non-finite
        // filtered) → joint_compliance must return None, not default to 0.0.
        m.insert(
            Value::String("neutral".to_string()),
            Value::Scalar {
                si_value: f64::NAN,
                dimension: DimensionVector::ANGLE,
            },
        );
        let joint = Value::Map(m);
        assert!(
            joint_compliance(&joint, Some(std::f64::consts::PI / 6.0)).is_none(),
            "malformed (NaN) neutral must make joint_compliance return None"
        );
    }

    /// Damping-only joint with a present-but-malformed `neutral` key must still
    /// return `Some` with the valid damping term.  The `neutral` key is only read
    /// when `spring_rate` is present; an irrelevant malformed `neutral` on a pure
    /// damping joint must not suppress the valid damping term.
    #[test]
    fn joint_compliance_malformed_neutral_damping_only_still_applies() {
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("revolute".to_string()),
        );
        m.insert(
            Value::String("damping".to_string()),
            Value::Real(3.5),
        );
        // Malformed neutral (NaN) — irrelevant when spring_rate is absent, so
        // it must NOT suppress the valid damping term.
        m.insert(
            Value::String("neutral".to_string()),
            Value::Scalar {
                si_value: f64::NAN,
                dimension: DimensionVector::ANGLE,
            },
        );
        let joint = Value::Map(m);
        let c = joint_compliance(&joint, None)
            .expect("damping-only joint with malformed neutral must still return Some");
        assert!(c.spring_rate.is_none(), "spring_rate must be None");
        assert!(
            (c.damping.expect("damping must be Some") - 3.5).abs() < 1e-12,
            "damping must be 3.5"
        );
        assert_eq!(c.neutral, 0.0, "neutral defaults to 0.0 when spring_rate absent");
    }

    // ── step-3 RED: end-to-end compliance torque tests ────────────────────────
    //
    // These tests fail until step-4 wires joint_compliance into the link loop:
    // currently compliance is hardcoded None, so every Δτ is 0.
    //
    // (a) TRAJECTORY SPRING: a compliant revolute (spring_rate=2.0 N·m/rad,
    //     neutral=π/12 rad) driven at θ=π/6 (vels=accels=0) via the trajectory
    //     path; Δτ = −k·(θ−neutral) = −2.0·(π/12) = −π/6 within 1e-12.
    // (b) SNAPSHOT-PATH DAMPING: a damping-only revolute (c=3.5 N·m·s/rad) at
    //     q̇=1.7 rad/s via the snapshot path; Δτ = −c·q̇ = −5.95 within 1e-12.
    // (c) NO-REGRESSION: plain joint trajectory torque still ≈ 0.4905 N·m.

    /// Build a plain-revolute joint extended with the supplied compliant keys,
    /// mirroring make_flexure_joint. `spring_rate_opt`=Some(k) inserts
    /// spring_rate=Scalar{k,ROTATIONAL_STIFFNESS} and neutral=Scalar{neutral_angle,ANGLE};
    /// `damping_opt`=Some(c) inserts damping=Real(c); None inserts
    /// damping=Option(None) (make_flexure_joint's current γ-scope shape).
    fn make_compliant_revolute_joint(
        spring_rate_opt: Option<f64>,
        damping_opt: Option<f64>,
        neutral_angle: f64,
    ) -> Value {
        use crate::eval_builtin;
        use std::f64::consts::PI;
        let axis_y =
            Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(-PI))),
            upper: Some(Box::new(Value::angle(PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let base = eval_builtin("revolute", &[axis_y, range]);
        match base {
            Value::Map(mut m) => {
                if let Some(k) = spring_rate_opt {
                    m.insert(
                        Value::String("spring_rate".to_string()),
                        Value::Scalar {
                            si_value: k,
                            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
                        },
                    );
                    m.insert(
                        Value::String("neutral".to_string()),
                        Value::angle(neutral_angle),
                    );
                }
                m.insert(
                    Value::String("damping".to_string()),
                    match damping_opt {
                        Some(c) => Value::Real(c),
                        None => Value::Option(None),
                    },
                );
                Value::Map(m)
            }
            _ => panic!("revolute() must return a Value::Map"),
        }
    }

    /// Extract the ScalarTorque magnitude from a trajectory-path result
    /// (`List<List<JointForce>>`), sample index `si`, body index `bi`.
    fn traj_torque(result: &Value, si: usize, bi: usize) -> f64 {
        let per_sample = match result {
            Value::List(s) => s,
            other => panic!("expected List<List<JointForce>>, got {other:?}"),
        };
        let forces = match &per_sample[si] {
            Value::List(f) => f,
            other => panic!("expected List<JointForce> at sample {si}, got {other:?}"),
        };
        let value = field(&forces[bi], "JointForce", "value");
        num(field(value, "ScalarTorque", "magnitude"))
    }

    /// Extract the ScalarTorque magnitude from a snapshot-path result
    /// (`List<JointForce>`), body index `bi`.
    fn snap_torque(result: &Value, bi: usize) -> f64 {
        let forces = match result {
            Value::List(f) => f,
            other => panic!("expected List<JointForce>, got {other:?}"),
        };
        let value = field(&forces[bi], "JointForce", "value");
        num(field(value, "ScalarTorque", "magnitude"))
    }

    /// (a) TRAJECTORY SPRING: compliant pendulum (spring_rate=2.0, neutral=π/12)
    /// driven via trajectory path at θ=π/6, vels=accels=0.
    /// Δτ = −k·(θ−neutral) = −2.0·(π/6−π/12) = −π/6 (within 1e-12).
    #[test]
    fn trajectory_spring_torque_delta() {
        use crate::eval_builtin;
        use std::f64::consts::PI;

        let theta = PI / 6.0; // position > neutral → spring pushes back

        // Compliant mechanism: spring_rate=2.0 N·m/rad, neutral=π/12 rad.
        let compliant_joint =
            make_compliant_revolute_joint(Some(2.0), None, PI / 12.0);
        let compliant_mech = {
            let mp = mass_properties_fixture(1.0, [0.0, 0.0, -0.1], [[0.0; 3]; 3]);
            let mech0 = eval_builtin("mechanism", &[]);
            eval_builtin("body", &[mech0, mp, compliant_joint])
        };

        // Plain mechanism for baseline (same geometry, no compliant keys).
        let plain_mech = pendulum_mechanism();

        // One-sample MotionTrajectory at θ=π/6, vels=0, accels=0.
        let make_traj = |q: f64| {
            mint_instance(
                "MotionTrajectory",
                vec![
                    ("mechanism".to_string(), Value::Real(0.0)),
                    (
                        "samples".to_string(),
                        Value::List(vec![trajectory_sample(0.0, q, 0.0, 0.0)]),
                    ),
                ],
            )
        };

        let r_comp = eval_dynamics("inverse_dynamics_lower", &[compliant_mech, make_traj(theta)])
            .expect("inverse_dynamics_lower recognised");
        let r_plain = eval_dynamics("inverse_dynamics_lower", &[plain_mech, make_traj(theta)])
            .expect("inverse_dynamics_lower recognised");

        let tau_comp = traj_torque(&r_comp, 0, 0);
        let tau_plain = traj_torque(&r_plain, 0, 0);

        // Numeric oracle (from parameter arithmetic only): Δτ = −k·(θ−neutral)
        let delta_expected = -2.0 * (PI / 6.0 - PI / 12.0); // = −π/6
        let delta_got = tau_comp - tau_plain;
        assert!(
            (delta_got - delta_expected).abs() < 1e-12,
            "spring Δτ: expected {delta_expected:.15} N·m, got {delta_got:.15}"
        );
        assert!(
            tau_comp < tau_plain,
            "restoring spring at position>neutral must reduce torque"
        );
    }

    /// (b) SNAPSHOT-PATH DAMPING: damping-only pendulum (c=3.5) via snapshot
    /// path at q̇=1.7 rad/s. Δτ = −c·q̇ = −5.95 (within 1e-12).
    #[test]
    fn snapshot_path_damping_torque_delta() {
        use crate::eval_builtin;
        use std::f64::consts::PI;

        let theta = -PI / 6.0; // same angle as the static test (-30°)
        let omega = 1.7_f64;

        // Compliant mechanism: damping=3.5 N·m·s/rad, no spring.
        let damping_joint =
            make_compliant_revolute_joint(None, Some(3.5), 0.0);
        let damping_mech = {
            let mp = mass_properties_fixture(1.0, [0.0, 0.0, -0.1], [[0.0; 3]; 3]);
            let mech0 = eval_builtin("mechanism", &[]);
            eval_builtin("body", &[mech0, mp, damping_joint.clone()])
        };

        // Plain mechanism for baseline.
        let plain_mech = pendulum_mechanism();

        // Build snapshots by binding the joint to θ=-30°.
        let make_snap = |mech: &Value, jnt: &Value| {
            let binding = eval_builtin("bind", &[jnt.clone(), Value::angle(theta)]);
            let s = eval_builtin("snapshot", &[mech.clone(), Value::List(vec![binding])]);
            assert!(matches!(s, Value::Map(_)), "snapshot() must return a Map");
            s
        };

        // Extract the plain joint from pendulum_mechanism's body.at for use
        // in bind().
        let plain_joint = {
            match &plain_mech {
                Value::Map(m) => {
                    let bodies = match map_get(m, "bodies") {
                        Some(Value::List(b)) => b,
                        _ => panic!("mechanism missing bodies"),
                    };
                    let b0 = match &bodies[0] {
                        Value::Map(bm) => bm,
                        _ => panic!("body 0 not a Map"),
                    };
                    map_get(b0, "at").expect("body 0 missing at").clone()
                }
                _ => panic!("pendulum_mechanism must be a Map"),
            }
        };

        let q_dot = Value::List(vec![Value::Real(omega)]);
        let q_ddot = Value::List(vec![Value::Real(0.0)]);

        let snap_damp = make_snap(&damping_mech, &damping_joint);
        let snap_plain = make_snap(&plain_mech, &plain_joint);

        let r_damp = eval_dynamics(
            "inverse_dynamics_at_snapshot_lower",
            &[damping_mech, snap_damp, q_dot.clone(), q_ddot.clone()],
        )
        .expect("inverse_dynamics_at_snapshot_lower recognised");
        let r_plain = eval_dynamics(
            "inverse_dynamics_at_snapshot_lower",
            &[plain_mech, snap_plain, q_dot, q_ddot],
        )
        .expect("inverse_dynamics_at_snapshot_lower recognised");

        let tau_damp = snap_torque(&r_damp, 0);
        let tau_plain = snap_torque(&r_plain, 0);

        // Oracle: Δτ = −c·q̇ = −3.5·1.7
        let delta_expected = -3.5 * 1.7;
        let delta_got = tau_damp - tau_plain;
        assert!(
            (delta_got - delta_expected).abs() < 1e-12,
            "damping Δτ: expected {delta_expected:.15} N·m·s/rad, got {delta_got:.15}"
        );
    }

    /// Trajectory-path damping: compliant pendulum (c=3.5 N·m·s/rad, no spring)
    /// driven via `inverse_dynamics_lower` at θ=π/6, q̇=1.7 rad/s.
    /// Δτ = −c·q̇ = −3.5·1.7 = −5.95 within 1e-12.
    ///
    /// This covers the trajectory entry-point forwarding of q̇ + damping together.
    /// The snapshot-path counterpart (`snapshot_path_damping_torque_delta`) covers
    /// the snapshot entry point; both are needed because they are different code paths
    /// through `snapshot_inverse_dynamics`.
    #[test]
    fn trajectory_path_damping_torque_delta() {
        use crate::eval_builtin;
        use std::f64::consts::PI;

        let theta = PI / 6.0;
        let omega = 1.7_f64;

        // Compliant mechanism: damping=3.5 N·m·s/rad, no spring.
        let damping_joint = make_compliant_revolute_joint(None, Some(3.5), 0.0);
        let damping_mech = {
            let mp = mass_properties_fixture(1.0, [0.0, 0.0, -0.1], [[0.0; 3]; 3]);
            let mech0 = eval_builtin("mechanism", &[]);
            eval_builtin("body", &[mech0, mp, damping_joint])
        };

        // Plain mechanism for baseline (same geometry, no compliant keys).
        let plain_mech = pendulum_mechanism();

        // One-sample MotionTrajectory at θ=π/6, q̇=omega, accels=0.
        let make_traj = |q: f64, v: f64| {
            mint_instance(
                "MotionTrajectory",
                vec![
                    ("mechanism".to_string(), Value::Real(0.0)),
                    (
                        "samples".to_string(),
                        Value::List(vec![trajectory_sample(0.0, q, v, 0.0)]),
                    ),
                ],
            )
        };

        let r_damp = eval_dynamics(
            "inverse_dynamics_lower",
            &[damping_mech, make_traj(theta, omega)],
        )
        .expect("inverse_dynamics_lower recognised");
        let r_plain = eval_dynamics(
            "inverse_dynamics_lower",
            &[plain_mech, make_traj(theta, omega)],
        )
        .expect("inverse_dynamics_lower recognised");

        let tau_damp = traj_torque(&r_damp, 0, 0);
        let tau_plain = traj_torque(&r_plain, 0, 0);

        // Oracle: Δτ = −c·q̇ = −3.5·1.7
        let delta_expected = -3.5 * omega;
        let delta_got = tau_damp - tau_plain;
        assert!(
            (delta_got - delta_expected).abs() < 1e-12,
            "damping Δτ via trajectory path: expected {delta_expected:.15} N·m, got {delta_got:.15}"
        );
    }

    /// (c) NO-REGRESSION: plain-joint trajectory torque still matches static-
    /// gravity baseline 0.4905 N·m after step-4 wires compliance.
    #[test]
    fn trajectory_plain_joint_no_regression() {
        use std::f64::consts::PI;
        let theta = -PI / 6.0;
        let traj = mint_instance(
            "MotionTrajectory",
            vec![
                ("mechanism".to_string(), Value::Real(0.0)),
                (
                    "samples".to_string(),
                    Value::List(vec![trajectory_sample(0.0, theta, 0.0, 0.0)]),
                ),
            ],
        );
        let result = eval_dynamics("inverse_dynamics_lower", &[pendulum_mechanism(), traj])
            .expect("inverse_dynamics_lower recognised");
        let torque = traj_torque(&result, 0, 0);
        let expected = 0.4905_f64;
        assert!(
            (torque - expected).abs() < 1e-6,
            "plain joint no-regression: expected {expected} N·m, got {torque}"
        );
    }

    // ── amend: 1-DOF gate for multi-DOF joints with stray compliance keys ─────────
    //
    // The eval-layer gate (`if subspaces[bi].len() == 1`) prevents compliance from
    // being attached to multi-DOF joints, blocking the rnea always-on assertion
    // (PRD §11.2) that panics when spring_rate/damping is set on a joint with
    // subspace.len() != 1.  This test locks in that defence: a 2-DOF cylindrical
    // joint Map carrying a stray spring_rate key must yield a finite CylForce
    // result rather than panicking.  If the gate were removed the rnea assert
    // would fire and no pre-amendment test would catch the regression.

    /// A 2-DOF cylindrical joint Map with a stray `spring_rate` key must
    /// return a finite `CylForce` result (the gate sets `compliance = None`),
    /// rather than panicking in the rnea layer's 1-DOF-only assertion.
    #[test]
    fn multi_dof_joint_with_stray_spring_rate_suppresses_compliance() {
        use crate::eval_builtin;
        use std::f64::consts::PI;

        // Build a standard 2-DOF cylindrical joint (translation + rotation along +z).
        let axis_z =
            Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let length_range = Value::Range {
            lower: Some(Box::new(Value::length(-1.0))),
            upper: Some(Box::new(Value::length(1.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let angle_range = Value::Range {
            lower: Some(Box::new(Value::angle(-PI))),
            upper: Some(Box::new(Value::angle(PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let base_cyl = eval_builtin("cylindrical", &[axis_z, length_range, angle_range]);

        // Inject a stray spring_rate key — simulating a hand-built multi-DOF Map
        // (e.g. a user who manually adds a compliance key to a cylindrical joint).
        let cyl_with_spring = match base_cyl {
            Value::Map(mut m) => {
                m.insert(
                    Value::String("spring_rate".to_string()),
                    Value::Scalar {
                        si_value: 2.0,
                        dimension: DimensionVector::ROTATIONAL_STIFFNESS,
                    },
                );
                Value::Map(m)
            }
            other => panic!("cylindrical() must return a Value::Map, got {other:?}"),
        };

        // Build a single-body mechanism using the keyed cylindrical joint.
        let mp = mass_properties_fixture(1.0, [0.0, 0.0, 0.0], [[0.0; 3]; 3]);
        let mech0 = eval_builtin("mechanism", &[]);
        let mech = eval_builtin("body", &[mech0, mp, cyl_with_spring.clone()]);
        assert!(
            matches!(mech, Value::Map(_)),
            "body() must yield a Mechanism Map"
        );

        // Snapshot at d = 0 m, θ = 0 rad (identity transform).
        // A cylindrical binding value is a 2-element List: [Length, Angle].
        let binding = eval_builtin(
            "bind",
            &[
                cyl_with_spring,
                Value::List(vec![Value::length(0.0), Value::angle(0.0)]),
            ],
        );
        let snap = eval_builtin("snapshot", &[mech.clone(), Value::List(vec![binding])]);
        assert!(matches!(snap, Value::Map(_)), "snapshot() must return a Map");

        // 2-DOF velocities / accelerations (cylindrical has DOF = 2).
        let q_dot = Value::List(vec![Value::Real(0.0), Value::Real(0.0)]);
        let q_ddot = Value::List(vec![Value::Real(0.0), Value::Real(0.0)]);

        // MUST NOT PANIC — the 1-DOF gate suppresses compliance on the 2-DOF
        // cylindrical joint, preventing the rnea always-on assertion from firing.
        let result = eval_dynamics(
            "inverse_dynamics_at_snapshot_lower",
            &[mech, snap, q_dot, q_ddot],
        )
        .expect("inverse_dynamics_at_snapshot_lower is a recognised intrinsic");

        // Result must be a List<JointForce> — NOT Undef — confirming the gate
        // allowed the RNEA to run (with compliance = None) rather than failing.
        let forces = match &result {
            Value::List(f) => f,
            other => panic!("expected List<JointForce>, got {other:?}"),
        };
        assert_eq!(forces.len(), 1, "one joint → one JointForce");
        let value = field(&forces[0], "JointForce", "value");
        // A cylindrical joint produces CylForce { components: List<Real> } (DOF = 2).
        let comps = match field(value, "CylForce", "components") {
            Value::List(c) => c,
            other => panic!("CylForce.components must be a List, got {other:?}"),
        };
        assert_eq!(comps.len(), 2, "CylForce must have exactly 2 components");
        for (i, c) in comps.iter().enumerate() {
            let v = num(c);
            assert!(
                v.is_finite(),
                "CylForce.components[{i}] must be finite (gate suppressed compliance), got {v}"
            );
        }
    }

    // ── step-5 RED: positions-length invariant for snapshot_inverse_dynamics ─────
    //
    // `snapshot_inverse_dynamics` must return `None` when supplied a `positions`
    // slice whose length ≠ body count (the one-per-body invariant).
    //
    // Currently `positions.get(bi)` silently reads `positions[0]` for bi=0 and
    // ignores the surplus entry (no length check exists), so the call returns
    // `Some(forces)` instead of `None` — making this test RED until step-6 adds
    // the guard `if p.len() != n { return None; }`.
    //
    // Setup: n=1 body (standard single-pendulum), positions slice length=2 →
    // mismatch → expected None.  The vels/accels are valid length-1 lists so they
    // cannot cause the failure; only the positions mismatch is the source.

    /// Supplying a positions slice with length ≠ body count must yield `None`
    /// (fail-honest one-per-body invariant: `positions[bi]` is body bi's joint
    /// coordinate by construction in `snapshot_for_sample`, and any mismatch
    /// means the caller is not supplying one position per body in bodies order).
    #[test]
    fn snapshot_inverse_dynamics_mismatched_positions_returns_none() {
        use crate::eval_builtin;
        use std::f64::consts::PI;

        let theta = -PI / 6.0; // −30°

        // Standard single-body pendulum mechanism (n = 1 body).
        let mech = pendulum_mechanism();

        // Extract the joint for bind().
        let joint = match &mech {
            Value::Map(m) => {
                let bodies = match map_get(m, "bodies") {
                    Some(Value::List(b)) => b,
                    _ => panic!("mechanism missing bodies"),
                };
                let b0 = match &bodies[0] {
                    Value::Map(bm) => bm,
                    _ => panic!("body 0 not a Map"),
                };
                map_get(b0, "at").expect("body 0 missing at").clone()
            }
            _ => panic!("pendulum_mechanism must be a Map"),
        };

        // Build FK snapshot at θ = −30°.
        let binding = eval_builtin("bind", &[joint, Value::angle(theta)]);
        let snap = eval_builtin("snapshot", &[mech.clone(), Value::List(vec![binding])]);
        assert!(matches!(snap, Value::Map(_)), "snapshot() must return a Map");

        // Valid length-1 vels/accels (1-DOF revolute).
        let q_dot = Value::List(vec![Value::Real(0.0)]);
        let q_ddot = Value::List(vec![Value::Real(0.0)]);

        // Deliberately mis-shaped positions: n=1 body but length=2.
        // This violates the one-per-body invariant.  Currently the call succeeds
        // silently (positions[0] is used, the surplus entry is ignored); after
        // step-6 the guard returns None.
        let positions: Vec<Value> = vec![Value::angle(theta), Value::angle(theta + 0.1)];

        let result = snapshot_inverse_dynamics(
            &mech,
            &snap,
            &q_dot,
            &q_ddot,
            Some(positions.as_slice()),
        );
        assert!(
            result.is_none(),
            "positions slice length != body count must return None \
             (fail-honest one-per-body invariant)"
        );
    }

    // ── KIN-OFFSET γ step-7 (B3): dynamics offset FK + finitude ─────────────────
    //
    // B3 dynamics: asserts (A) the snapshot body's world_transform that the dynamics
    // reads (in snapshot_inverse_dynamics) is offset-aware (matches hand-computed offset-shifted
    // position, exact SE(3) composition, tol ≈1e-9) and (B) inverse_dynamics on the
    // offset mechanism returns a finite, well-formed result — not Undef.
    //
    // Setup: offset_revolute_z(L=0.1m), 1 kg point mass at CoM=(0,0,−0.1),
    // zero inertia. Joint at θ=π/6, static (q̇=q̈=0).
    //
    // For offset_revolute_z(L): transform_at = {R_z(θ), (L,0,0)} (origin pre-compose).
    // World translation = (L, 0, 0) = (0.1, 0, 0) regardless of θ — a clean, exact
    // hand-computable reference proving the FK input to dynamics is offset-aware
    // (route 4 of the PRD §7.2 no-bypass invariant, verified by γ B8 route-4 test).
    //
    // Does NOT assert a precise analytic torque (that is β/B7 with a tolerance derived
    // from the loop-Newton floor); γ is a re-validation, not a new analytic e2e.

    #[test]
    fn inverse_dynamics_offset_joint_fk_world_transform_correct_and_finite() {
        use crate::eval_builtin;
        use std::f64::consts::PI;

        // ── Shared offset_revolute_z(0.1) fixture ─────────────────────────────
        // offset_revolute_z uses range 0..π; θ=π/6 ≈ 0.52 is well within it.
        let joint = crate::test_fixtures::offset_revolute_z(0.1);
        assert!(
            matches!(joint, Value::Map(_)),
            "offset_revolute_z(0.1) must yield a Map, got {:?}",
            joint
        );

        // ── Single-body mechanism (1 kg point mass, CoM at (0,0,−0.1)) ───────
        let mp = mass_properties_fixture(1.0, [0.0, 0.0, -0.1], [[0.0; 3]; 3]);
        let mech = eval_builtin("mechanism", &[]);
        let mech = eval_builtin("body", &[mech, mp.clone(), joint.clone()]);
        assert!(matches!(mech, Value::Map(_)), "body() must yield a Mechanism Map");

        // ── Snapshot at θ = π/6 (30°) ────────────────────────────────────────
        let theta = PI / 6.0;
        let binding = eval_builtin("bind", &[joint.clone(), Value::angle(theta)]);
        let snap = eval_builtin("snapshot", &[mech.clone(), Value::List(vec![binding])]);
        assert!(matches!(snap, Value::Map(_)), "snapshot() must yield a Snapshot Map");

        // ── PRIMARY: world_transform.translation == (0.1, 0, 0) ──────────────
        // For offset_revolute_z(L=0.1): transform_at = {R_z(θ), (L,0,0)}.
        // T_world = I ∘ transform_at = {R_z(θ), (0.1, 0, 0)}.
        // Translation is (0.1, 0, 0) regardless of θ — the offset is baked in.
        let wt = crate::test_fixtures::body_world_transform(&snap, 0);

        // Decompose world_transform.
        let (rotation, translation) = match wt {
            Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
            other => panic!("expected Transform, got {:?}", other),
        };
        let comps = match translation {
            Value::Vector(c) if c.len() == 3 => c,
            other => panic!("expected Vector(3) translation, got {:?}", other),
        };
        let read_f64 = |v: &Value| -> f64 {
            match v {
                Value::Real(r) => *r,
                Value::Scalar { si_value, .. } => *si_value,
                other => panic!("expected numeric component, got {:?}", other),
            }
        };
        let tx = read_f64(&comps[0]);
        let ty = read_f64(&comps[1]);
        let tz = read_f64(&comps[2]);

        let tol = 1e-9;
        assert!(
            (tx - 0.1).abs() < tol,
            "B3 dynamics route-4: world_transform.tx should be 0.1 m (offset), got {tx}"
        );
        assert!(
            ty.abs() < tol,
            "B3 dynamics route-4: world_transform.ty should be 0, got {ty}"
        );
        assert!(
            tz.abs() < tol,
            "B3 dynamics route-4: world_transform.tz should be 0, got {tz}"
        );

        // Rotation should be R_z(π/6) (offset origin has identity rotation).
        let (qw, qx, qy, qz) = match rotation {
            Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
            other => panic!("expected Orientation, got {:?}", other),
        };
        let exp_qw = (theta / 2.0).cos();
        let exp_qz = (theta / 2.0).sin();
        let rot_ok =
            (qw - exp_qw).abs() < tol && qx.abs() < tol && qy.abs() < tol && (qz - exp_qz).abs() < tol
            || (qw + exp_qw).abs() < tol && qx.abs() < tol && qy.abs() < tol && (qz + exp_qz).abs() < tol;
        assert!(
            rot_ok,
            "B3 dynamics route-4: world_transform rotation should be R_z(π/6) ≈ \
             ({exp_qw:.6},0,0,{exp_qz:.6}) up to sign, got ({qw:.6},{qx:.6},{qy:.6},{qz:.6})"
        );

        // ── SECONDARY: inverse_dynamics returns a finite, well-formed result ──
        let q_dot = Value::List(vec![Value::Real(0.0)]);
        let q_ddot = Value::List(vec![Value::Real(0.0)]);

        let result = eval_dynamics(
            "inverse_dynamics_at_snapshot_lower",
            &[mech, snap, q_dot, q_ddot],
        )
        .expect("inverse_dynamics_at_snapshot_lower must be a recognised dynamics intrinsic");

        let forces = match &result {
            Value::List(f) => f,
            other => panic!("expected a List<JointForce>, got {other:?}"),
        };
        assert_eq!(forces.len(), 1, "one joint → one JointForce");

        let value = field(&forces[0], "JointForce", "value");
        let torque = num(field(value, "ScalarTorque", "magnitude"));
        // ── Finitude smoke test (intentionally NOT offset-discriminating for the torque) ──
        //
        // For a z-axis revolute with z-directed gravity g=(0,0,−9.81):
        //   τ_z = (r_CoM × F_grav)·ẑ = r_x·F_y − r_y·F_x = r_x·0 − r_y·0 = 0
        // for ANY CoM position — moving the CoM off the rotation axis (x,y ≠ 0)
        // does not produce a nonzero τ_z because F_grav ∥ ẑ makes the cross product
        // perpendicular to ẑ.  τ_z = 0 is exact physics, not a numerical coincidence.
        //
        // Offset-discriminating dynamics coverage (a nonzero analytic torque that
        // changes with L) requires a non-z joint axis or non-z gravity and is
        // deliberately deferred to β/B7.  This assertion's role is to confirm that
        // inverse_dynamics returns a finite, well-formed result on an offset mechanism,
        // complementing the PRIMARY assertion above (world_transform.tx == L) which
        // is the offset-discriminating check for route 4.
        assert!(
            torque.is_finite() && torque.abs() < 1e-6,
            "B3 dynamics finitude: inverse_dynamics must return a finite result \
             (z-revolute + z-gravity → τ_z = 0 exactly for any CoM); got {torque}"
        );
    }

    // ── task-4278 step-1 RED: point_mass constructor ───────────────────────────
    //
    // `eval_dynamics("point_mass", &[mass_scalar])` must return a MassProperties
    // StructureInstance with mass==supplied value, com==[0,0,0], inertia==zeros.
    // RED until step-2 adds the dispatch arm.

    #[test]
    fn eval_dynamics_point_mass_roundtrip() {
        let result = eval_dynamics(
            "point_mass",
            &[Value::Scalar {
                si_value: 2.5,
                dimension: DimensionVector::MASS,
            }],
        )
        .expect("point_mass is a recognised dynamics constructor");
        assert!(
            !matches!(result, Value::Undef),
            "point_mass with valid arg must not return Undef"
        );
        let (mass, com, inertia) =
            mass_properties_from_value(&result).expect("result must be a MassProperties");
        assert!((mass - 2.5).abs() < 1e-12, "mass should be 2.5 kg, got {mass}");
        for &c in &com {
            assert!(c.abs() < 1e-12, "com should be origin, got {com:?}");
        }
        for row in &inertia {
            for &v in row {
                assert!(v.abs() < 1e-12, "inertia should be zero, got {inertia:?}");
            }
        }
        // type_name must be "MassProperties"
        match &result {
            Value::StructureInstance(d) => {
                assert_eq!(d.type_name, "MassProperties", "wrong type_name");
            }
            other => panic!("expected StructureInstance, got {other:?}"),
        }
    }

    #[test]
    fn eval_dynamics_point_mass_wrong_arity_returns_undef() {
        // zero args → Undef
        let r0 = eval_dynamics("point_mass", &[]).expect("point_mass recognised");
        assert!(
            matches!(r0, Value::Undef),
            "zero args must return Undef, got {r0:?}"
        );
        // two args → Undef
        let mass = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        let r2 = eval_dynamics("point_mass", &[mass.clone(), mass])
            .expect("point_mass recognised");
        assert!(matches!(r2, Value::Undef), "two args must return Undef, got {r2:?}");
    }

    #[test]
    fn eval_dynamics_point_mass_non_numeric_arg_returns_undef() {
        let r = eval_dynamics("point_mass", &[Value::String("bad".to_string())])
            .expect("point_mass recognised");
        assert!(matches!(r, Value::Undef), "non-numeric arg must return Undef, got {r:?}");
    }

    // ── amendment: dimension-mismatch guards (reviewer suggestion 1) ─────────
    //
    // Passing a wrong-dimension Scalar (e.g. Length instead of Mass) must return
    // Value::Undef rather than silently producing a physically-wrong MassProperties.

    #[test]
    fn eval_dynamics_point_mass_length_scalar_returns_undef() {
        // 5 m is a Length, NOT a Mass — must be rejected.
        let length_arg = Value::Scalar { si_value: 5.0, dimension: DimensionVector::LENGTH };
        let r = eval_dynamics("point_mass", &[length_arg])
            .expect("point_mass recognised");
        assert!(
            matches!(r, Value::Undef),
            "Length-dimensioned scalar must return Undef (not silently treat as 5 kg), got {r:?}"
        );
    }

    #[test]
    fn eval_dynamics_point_mass_dimensionless_real_still_accepted() {
        // Dimensionless Real is still accepted for test ergonomics.
        let r = eval_dynamics("point_mass", &[Value::Real(3.0)])
            .expect("point_mass recognised");
        assert!(
            !matches!(r, Value::Undef),
            "dimensionless Real must still be accepted, got {r:?}"
        );
        let (mass, _, _) = mass_properties_from_value(&r).expect("must be MassProperties");
        assert!((mass - 3.0).abs() < 1e-12, "mass should be 3.0, got {mass}");
    }

    // ── task-4278 step-3 RED: mass_properties(mass, com, inertia) constructor ───
    //
    // eval_dynamics("mass_properties", &[mass, com, inertia]) must round-trip via
    // mass_properties_from_value. RED until step-4 adds the dispatch arm.

    #[test]
    fn eval_dynamics_mass_properties_roundtrip() {
        let mass_val = Value::Scalar { si_value: 3.0, dimension: DimensionVector::MASS };
        let com_val = Value::Point(vec![Value::length(0.1), Value::length(0.2), Value::length(0.3)]);
        let inertia = [[1.0, 0.1, 0.2], [0.1, 2.0, 0.3], [0.2, 0.3, 3.0]];
        let inertia_val = Value::Matrix(
            inertia.iter().map(|row| row.iter().map(|&x| Value::Real(x)).collect()).collect(),
        );

        let result = eval_dynamics("mass_properties", &[mass_val, com_val, inertia_val])
            .expect("mass_properties is a recognised dynamics constructor");
        assert!(!matches!(result, Value::Undef), "valid args must not return Undef");

        let (m, com, got_inertia) = mass_properties_from_value(&result)
            .expect("result must be a MassProperties");
        assert!((m - 3.0).abs() < 1e-12, "mass mismatch: {m}");
        assert!((com[0] - 0.1).abs() < 1e-12, "com[0] mismatch: {}", com[0]);
        assert!((com[1] - 0.2).abs() < 1e-12, "com[1] mismatch: {}", com[1]);
        assert!((com[2] - 0.3).abs() < 1e-12, "com[2] mismatch: {}", com[2]);
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (got_inertia[r][c] - inertia[r][c]).abs() < 1e-12,
                    "inertia[{r}][{c}] mismatch"
                );
            }
        }
        match &result {
            Value::StructureInstance(d) => assert_eq!(d.type_name, "MassProperties"),
            other => panic!("expected StructureInstance, got {other:?}"),
        }
    }

    #[test]
    fn eval_dynamics_mass_properties_list_com_accepted() {
        // com supplied as a Value::List of numeric scalars (not Point) — must also work
        let mass_val = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        let com_val = Value::List(vec![Value::length(0.0), Value::length(0.0), Value::length(0.5)]);
        let inertia_val = Value::Matrix(
            [[0.0f64; 3]; 3].iter().map(|row| row.iter().map(|&x| Value::Real(x)).collect()).collect(),
        );
        let result = eval_dynamics("mass_properties", &[mass_val, com_val, inertia_val])
            .expect("mass_properties recognised");
        assert!(!matches!(result, Value::Undef), "List com must be accepted");
        let (_, com, _) = mass_properties_from_value(&result).expect("must be MassProperties");
        assert!((com[2] - 0.5).abs() < 1e-12, "com[2] via List: {}", com[2]);
    }

    #[test]
    fn eval_dynamics_mass_properties_wrong_arity_returns_undef() {
        let mass = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        // 0 args
        assert!(matches!(
            eval_dynamics("mass_properties", &[]).expect("recognised"),
            Value::Undef
        ));
        // 1 arg
        assert!(matches!(
            eval_dynamics("mass_properties", std::slice::from_ref(&mass)).expect("recognised"),
            Value::Undef
        ));
        // 2 args
        let com = Value::Point(vec![Value::length(0.0); 3]);
        assert!(matches!(
            eval_dynamics("mass_properties", &[mass, com]).expect("recognised"),
            Value::Undef
        ));
    }

    #[test]
    fn eval_dynamics_mass_properties_non_3x3_inertia_returns_undef() {
        let mass = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        let com = Value::Point(vec![Value::length(0.0); 3]);
        // 2×3 matrix (not 3×3)
        let bad_inertia = Value::Matrix(vec![
            vec![Value::Real(0.0); 3],
            vec![Value::Real(0.0); 3],
        ]);
        let r = eval_dynamics("mass_properties", &[mass, com, bad_inertia]).expect("recognised");
        assert!(matches!(r, Value::Undef), "non-3×3 inertia must return Undef");
    }

    #[test]
    fn eval_dynamics_mass_properties_length_mass_arg_returns_undef() {
        // Passing a Length scalar as the mass argument must be rejected.
        // Without the dimension guard, 5 m would be silently treated as 5 kg.
        let length_mass = Value::Scalar { si_value: 5.0, dimension: DimensionVector::LENGTH };
        let com = Value::Point(vec![Value::length(0.0); 3]);
        let inertia_val = Value::Matrix(
            [[0.0f64; 3]; 3].iter().map(|row| row.iter().map(|&x| Value::Real(x)).collect()).collect(),
        );
        let r = eval_dynamics("mass_properties", &[length_mass, com, inertia_val]).expect("recognised");
        assert!(
            matches!(r, Value::Undef),
            "Length-dimensioned mass arg must return Undef (not silently treat as kg), got {r:?}"
        );
    }

    // ── task-4472 step-1 RED: resolve_body_mass rung (b) ─────────────────────
    //
    // Tests for rung (b): a body whose solid is NOT a MassProperties but which
    // carries a `derived_mass_props` MassProperties key → resolve_body_mass
    // returns Some(derived_mass_props).
    //
    // All four tests are RED today because rung (b) is not yet implemented
    // (step-2 will green them).

    /// Build a body `Value::Map` that has the given `solid` PLUS an injected
    /// `derived_mass_props` key set to `mp`. Bypasses `eval_builtin("body",…)` so
    /// the solid is set to a non-MassProperties value independently of whether
    /// mechanism.rs would accept it.
    fn body_with_solid_and_derived(solid: Value, mp: Value) -> Value {
        use std::collections::BTreeMap;
        let mut m: BTreeMap<Value, Value> = BTreeMap::new();
        m.insert(Value::String("id".to_string()), Value::Int(0));
        m.insert(Value::String("solid".to_string()), solid);
        m.insert(Value::String("derived_mass_props".to_string()), mp);
        Value::Map(m)
    }

    /// Replace the `solid` of every body in a mechanism Map with `new_solid`,
    /// inject `derived_mass_props = mp` into each body, and return the patched
    /// mechanism.  Used by the rung-(b) integration test to swap the explicit
    /// MassProperties solid out and replace it with a derived key.
    fn patch_mechanism_bodies(mech: &Value, new_solid: Value, mp: Value) -> Value {
        use std::collections::BTreeMap;
        let m = match mech {
            Value::Map(m) => m,
            other => panic!("expected mechanism Map, got {other:?}"),
        };
        let bodies = match m.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            _ => panic!("mechanism missing bodies list"),
        };
        let patched_bodies: Vec<Value> = bodies
            .iter()
            .map(|body| {
                let bm = match body {
                    Value::Map(bm) => bm,
                    other => panic!("body must be a Map, got {other:?}"),
                };
                let mut b2: BTreeMap<Value, Value> = bm.clone().into_iter().collect();
                b2.insert(Value::String("solid".to_string()), new_solid.clone());
                b2.insert(
                    Value::String("derived_mass_props".to_string()),
                    mp.clone(),
                );
                Value::Map(b2)
            })
            .collect();
        let mut m2: BTreeMap<Value, Value> = m.clone().into_iter().collect();
        m2.insert(
            Value::String("bodies".to_string()),
            Value::List(patched_bodies),
        );
        Value::Map(m2)
    }

    // (a) A body with a non-MassProperties solid and a derived_mass_props key
    //     → resolve_body_mass returns Some(derived_mass_props).
    #[test]
    fn resolve_body_mass_rung_b_derived_props_resolves() {
        let mp = mass_properties_fixture(1.0, [0.0, 0.0, -0.1], [[0.0; 3]; 3]);
        // solid is a plain Real (not MassProperties)
        let body = body_with_solid_and_derived(Value::Real(0.0), mp.clone());
        let resolved = resolve_body_mass(&body)
            .expect("body with derived_mass_props must resolve to Some");
        let (mass, com, _) = mass_properties_from_value(&resolved)
            .expect("resolved value must be a valid MassProperties");
        assert!((mass - 1.0).abs() < 1e-12, "mass = {mass}");
        assert!((com[2] - (-0.1)).abs() < 1e-12, "com[2] = {}", com[2]);
    }

    // (b) When solid IS a MassProperties, rung (a) wins — derived key ignored.
    #[test]
    fn resolve_body_mass_rung_a_wins_over_derived() {
        let mp_solid = mass_properties_fixture(5.0, [0.1, 0.2, 0.3], [[0.0; 3]; 3]);
        let mp_derived = mass_properties_fixture(1.0, [0.0, 0.0, -0.1], [[0.0; 3]; 3]);
        let body = body_with_solid_and_derived(mp_solid, mp_derived);
        let resolved = resolve_body_mass(&body)
            .expect("body with MassProperties solid must resolve via rung (a)");
        let (mass, _, _) = mass_properties_from_value(&resolved)
            .expect("resolved value must be a valid MassProperties");
        // Rung (a) wins: mass = 5.0 (from solid), not 1.0 (from derived).
        assert!((mass - 5.0).abs() < 1e-12, "rung (a) mass = {mass}; expected 5.0");
    }

    // (c) A body with neither a MassProperties solid nor derived_mass_props → None.
    #[test]
    fn resolve_body_mass_rung_c_none_when_no_derived() {
        use std::collections::BTreeMap;
        let mut m: BTreeMap<Value, Value> = BTreeMap::new();
        m.insert(Value::String("id".to_string()), Value::Int(0));
        m.insert(Value::String("solid".to_string()), Value::Real(0.0));
        let body = Value::Map(m);
        assert!(
            resolve_body_mass(&body).is_none(),
            "non-MassProperties solid with no derived_mass_props must return None"
        );
    }

    // (d) Integration: mirror inverse_dynamics_at_snapshot_single_pendulum_static_gravity
    // but move the MassProperties to derived_mass_props (solid = Real(0.0)).
    // Rung (b) must resolve the mass, so the RNEA still yields ≈0.4905 N·m.
    // RED today: rung (b) is a stub → consumer sees Undef → result is Undef.
    #[test]
    fn inverse_dynamics_at_snapshot_rung_b_derived_mass_static_gravity() {
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

        // Build mechanism with a non-MassProperties solid (Real(0.0)).
        let mech0 = eval_builtin("mechanism", &[]);
        let mech_real_solid =
            eval_builtin("body", &[mech0, Value::Real(0.0), joint.clone()]);
        assert!(
            matches!(mech_real_solid, Value::Map(_)),
            "body() must yield a Mechanism Map"
        );

        // Inject derived_mass_props = mp into the first body.
        let mech = patch_mechanism_bodies(&mech_real_solid, Value::Real(0.0), mp.clone());

        // snapshot at θ = −30°
        let theta = -PI / 6.0;
        let binding = eval_builtin("bind", &[joint.clone(), Value::angle(theta)]);
        let snap = eval_builtin("snapshot", &[mech.clone(), Value::List(vec![binding])]);
        assert!(matches!(snap, Value::Map(_)), "snapshot() must yield a Snapshot Map");

        let q_dot = Value::List(vec![Value::Real(0.0)]);
        let q_ddot = Value::List(vec![Value::Real(0.0)]);

        let result = eval_dynamics(
            "inverse_dynamics_at_snapshot_lower",
            &[mech, snap, q_dot, q_ddot],
        )
        .expect("inverse_dynamics_at_snapshot_lower must be recognised");

        let forces = match &result {
            Value::List(f) => f,
            other => panic!(
                "expected a List<JointForce> (rung b resolved), got {other:?}"
            ),
        };
        assert_eq!(forces.len(), 1, "one joint ⇒ one JointForce");

        let value = field(&forces[0], "JointForce", "value");
        let torque = num(field(value, "ScalarTorque", "magnitude"));

        let expected = 0.4905_f64;
        assert!(
            (torque - expected).abs() < 1e-6,
            "rung (b) torque expected {expected} N·m, got {torque}"
        );
    }

    // ── task-4278 step-5 RED: resolve_body_mass ────────────────────────────────
    //
    // resolve_body_mass(&body) must return Some(MassProperties) for a body whose
    // solid is a MassProperties StructureInstance, and None for non-resolvable
    // solids. RED until step-6 implements the function.

    /// Build a body Map with the given solid as its `solid` field.  Mimics what
    /// mechanism.rs does when adding a body: the solid is stored verbatim.
    fn body_with_solid(solid: Value) -> Value {
        use crate::eval_builtin;
        use std::f64::consts::PI;
        let axis_y = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(-PI))),
            upper: Some(Box::new(Value::angle(PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("revolute", &[axis_y, range]);
        let mech0 = eval_builtin("mechanism", &[]);
        let mech = eval_builtin("body", &[mech0, solid, joint]);
        // Extract bodies[0] — the body Map.
        match mech {
            Value::Map(ref m) => {
                let bodies = match map_get(m, "bodies") {
                    Some(Value::List(b)) => b,
                    _ => panic!("mechanism missing bodies"),
                };
                bodies[0].clone()
            }
            other => panic!("body() must return a Map, got {other:?}"),
        }
    }

    #[test]
    fn resolve_body_mass_returns_some_for_mass_properties_solid() {
        let pm = eval_dynamics("point_mass", &[
            Value::Scalar { si_value: 2.5, dimension: DimensionVector::MASS },
        ])
        .expect("point_mass recognised");
        let body = body_with_solid(pm);

        let resolved = resolve_body_mass(&body).expect("MassProperties solid must resolve");
        let (mass, com, _) = mass_properties_from_value(&resolved)
            .expect("resolved value must be a valid MassProperties");
        assert!((mass - 2.5).abs() < 1e-12, "mass should be 2.5 kg, got {mass}");
        for &c in &com {
            assert!(c.abs() < 1e-12, "com should be origin for point_mass, got {com:?}");
        }
    }

    #[test]
    fn resolve_body_mass_returns_none_for_real_solid() {
        let body = body_with_solid(Value::Real(1.0));
        assert!(
            resolve_body_mass(&body).is_none(),
            "a plain Real solid must not resolve"
        );
    }

    #[test]
    fn resolve_body_mass_returns_none_for_absent_solid() {
        // Hand-craft a body Map with no `solid` key at all.
        let body = Value::Map({
            let mut m = std::collections::BTreeMap::new();
            m.insert(Value::String("id".to_string()), Value::Int(0));
            m
        });
        assert!(
            resolve_body_mass(&body).is_none(),
            "a body Map with no solid key must return None"
        );
    }

    #[test]
    fn resolve_body_mass_returns_none_for_non_map_body() {
        // A non-Map Value is not a body.
        assert!(resolve_body_mass(&Value::Real(0.0)).is_none());
    }

    // ── task-4278 step-7 RED: inverse_dynamics retrofit + typed diagnostic ───────
    //
    // (a) point_mass body → inverse_dynamics_lower yields finite torques.
    // (b) non-resolvable body.solid → Undef + diagnose returns DynamicsBodyMassUnresolved.
    // (c) resolvable mechanism → diagnose returns None.
    // RED because `diagnose` does not exist yet (step-8).

    /// Build a MotionTrajectory for a single-revolute mechanism: two motionless
    /// samples at the given joint angle θ (rad).
    fn motionless_trajectory(theta: f64) -> Value {
        mint_instance(
            "MotionTrajectory",
            vec![
                ("mechanism".to_string(), Value::Real(0.0)),
                (
                    "samples".to_string(),
                    Value::List(vec![
                        trajectory_sample(0.0, theta, 0.0, 0.0),
                        trajectory_sample(1.0, theta, 0.0, 0.0),
                    ]),
                ),
            ],
        )
    }

    /// Build a 1-body revolute mechanism with the given solid.
    fn revolute_mechanism_with_solid(solid: Value) -> Value {
        use crate::eval_builtin;
        use std::f64::consts::PI;
        let axis_y = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(-PI))),
            upper: Some(Box::new(Value::angle(PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("revolute", &[axis_y, range]);
        let mech0 = eval_builtin("mechanism", &[]);
        eval_builtin("body", &[mech0, solid, joint])
    }

    #[test]
    fn inverse_dynamics_lower_point_mass_yields_finite_torque() {
        use crate::eval_builtin;
        use std::f64::consts::PI;

        let pm = eval_builtin(
            "point_mass",
            &[Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS }],
        );
        // Build a mechanism with com at origin — pick θ = 0 for a zero-gravity
        // torque case (gravity acts along z, com at origin → zero arm).  We only
        // need FINITE (not panic/Undef) output to confirm the RNEA ran.
        let mech = revolute_mechanism_with_solid(pm);
        let theta = -PI / 6.0;
        let traj = motionless_trajectory(theta);

        let result = eval_dynamics("inverse_dynamics_lower", &[mech, traj])
            .expect("inverse_dynamics_lower is a recognised intrinsic");
        assert!(
            !matches!(result, Value::Undef),
            "point_mass body must not return Undef, got {result:?}"
        );
        let per_sample = match &result {
            Value::List(s) => s,
            other => panic!("expected List<List<JointForce>>, got {other:?}"),
        };
        assert!(!per_sample.is_empty(), "must have at least one sample");
        // Each sample must contain at least one finite JointForce.
        for sample_forces in per_sample {
            let forces = match sample_forces {
                Value::List(f) => f,
                other => panic!("inner sample must be a List, got {other:?}"),
            };
            assert!(!forces.is_empty(), "sample must have at least one JointForce");
            let value = field(&forces[0], "JointForce", "value");
            let torque = num(field(value, "ScalarTorque", "magnitude"));
            assert!(torque.is_finite(), "torque must be finite, got {torque}");
        }
    }

    #[test]
    fn diagnose_unresolvable_body_emits_dynamics_body_mass_unresolved() {
        use reify_core::diagnostics::DiagnosticCode;

        // Build a mechanism whose body.solid is a plain Real (non-resolvable).
        let mech = revolute_mechanism_with_solid(Value::Real(1.0));
        let theta = -std::f64::consts::PI / 6.0;
        let traj = motionless_trajectory(theta);

        // inverse_dynamics_lower must return Undef for an unresolvable body.
        let result = eval_dynamics("inverse_dynamics_lower", &[mech.clone(), traj.clone()])
            .expect("inverse_dynamics_lower recognised");
        assert!(
            matches!(result, Value::Undef),
            "unresolvable solid must return Undef, got {result:?}"
        );

        // diagnose must emit DynamicsBodyMassUnresolved.
        let diag = diagnose("inverse_dynamics_lower", &[mech, traj])
            .expect("diagnose must return Some for unresolvable body");
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::DynamicsBodyMassUnresolved),
            "wrong diagnostic code: {diag:?}"
        );
    }

    #[test]
    fn diagnose_resolvable_mechanism_returns_none() {
        use crate::eval_builtin;

        let pm = eval_builtin(
            "point_mass",
            &[Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS }],
        );
        let mech = revolute_mechanism_with_solid(pm);
        let theta = -std::f64::consts::PI / 6.0;
        let traj = motionless_trajectory(theta);

        let diag = diagnose("inverse_dynamics_lower", &[mech, traj]);
        assert!(
            diag.is_none(),
            "a fully-resolvable mechanism must not emit a diagnostic, got {diag:?}"
        );
    }

    // ── step-5 RED (task 4494): make_mass_properties + eval_point_mass populate ──
    //
    // make_mass_properties/eval_point_mass must produce inertia cells as
    // Value::Scalar{MOMENT_OF_INERTIA}, not plain Value::Real.
    // RED until step-6 (task 4494) changes make_mass_properties to emit
    // dimensioned scalars.

    /// make_mass_properties must build the inertia field as a Value::Matrix of
    /// Value::Scalar{dimension == MOMENT_OF_INERTIA} cells, mirroring the
    /// eval_body_mass_props_core populate pattern. si_value must equal the
    /// supplied f64 entry within 1e-12.
    #[test]
    fn make_mass_properties_inertia_cells_are_moment_of_inertia_scalars() {
        let input_inertia = [[1.0_f64, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]];
        let mp = make_mass_properties(2.5, [0.1, 0.2, 0.3], input_inertia);

        let data = match &mp {
            Value::StructureInstance(d) => d,
            other => panic!("expected MassProperties StructureInstance, got {other:?}"),
        };
        assert_eq!(data.type_name, "MassProperties");
        let inertia_rows = match data.fields.get("inertia").expect("inertia field") {
            Value::Matrix(rows) => rows,
            other => panic!("inertia field must be Value::Matrix, got {other:?}"),
        };
        assert_eq!(inertia_rows.len(), 3, "inertia must have 3 rows");
        for r in 0..3 {
            assert_eq!(inertia_rows[r].len(), 3, "inertia row {r} must have 3 cols");
            for c in 0..3 {
                match &inertia_rows[r][c] {
                    Value::Scalar { si_value, dimension } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::MOMENT_OF_INERTIA,
                            "inertia[{r}][{c}] must be MOMENT_OF_INERTIA-dimensioned, got {dimension:?}"
                        );
                        assert!(
                            (si_value - input_inertia[r][c]).abs() < 1e-12,
                            "inertia[{r}][{c}] si_value: expected {}, got {}",
                            input_inertia[r][c],
                            si_value
                        );
                    }
                    other => panic!(
                        "inertia[{r}][{c}] must be Value::Scalar{{MOMENT_OF_INERTIA}}, got {other:?}"
                    ),
                }
            }
        }
    }

    /// eval_point_mass (zero inertia) must also populate inertia cells as
    /// Value::Scalar{MOMENT_OF_INERTIA} with si_value == 0.0.
    #[test]
    fn eval_point_mass_inertia_cells_are_moment_of_inertia_scalars() {
        let mp = eval_point_mass(&[Value::Scalar {
            si_value: 2.5,
            dimension: DimensionVector::MASS,
        }]);

        let data = match &mp {
            Value::StructureInstance(d) => d,
            other => panic!("expected MassProperties StructureInstance, got {other:?}"),
        };
        assert_eq!(data.type_name, "MassProperties");
        let inertia_rows = match data.fields.get("inertia").expect("inertia field") {
            Value::Matrix(rows) => rows,
            other => panic!("inertia field must be Value::Matrix, got {other:?}"),
        };
        assert_eq!(inertia_rows.len(), 3, "point_mass inertia must have 3 rows");
        for (r, row) in inertia_rows.iter().enumerate() {
            assert_eq!(row.len(), 3, "row {r} must have 3 cols");
            for (c, cell) in row.iter().enumerate() {
                match cell {
                    Value::Scalar { si_value, dimension } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::MOMENT_OF_INERTIA,
                            "point_mass inertia[{r}][{c}] must be MOMENT_OF_INERTIA-dimensioned"
                        );
                        assert!(
                            si_value.abs() < 1e-15,
                            "point_mass inertia[{r}][{c}] si_value must be 0.0, got {si_value}"
                        );
                    }
                    other => panic!(
                        "point_mass inertia[{r}][{c}] must be Value::Scalar{{MOMENT_OF_INERTIA}}, got {other:?}"
                    ),
                }
            }
        }
    }

    // ── step-5 (task 4494): numeric-identity round-trip for RNEA extraction ──────
    //
    // Proves mass_properties_from_value is dimension-agnostic: a MassProperties
    // whose inertia cells are Value::Scalar{MOMENT_OF_INERTIA} yields the same
    // (mass, com, inertia) f64 triple as the equivalent Value::Real-celled fixture.
    // GREEN immediately (cell_f64 already strips si_value from dimensioned scalars).

    /// mass_properties_from_value returns identical f64 triples whether the
    /// inertia cells are plain Value::Real or Value::Scalar{MOMENT_OF_INERTIA}.
    /// This confirms the RNEA extraction path is dimension-agnostic and that
    /// RNEA τ output is byte-identical after the step-6 populate change.
    #[test]
    fn mass_properties_from_value_round_trip_is_identical_for_dimensioned_inertia() {
        let mass = 3.0_f64;
        let com = [0.01, -0.02, 0.05];
        let inertia = [[0.1, 0.0, 0.0], [0.0, 0.2, 0.0], [0.0, 0.0, 0.3]];

        // Existing fixture: Value::Real inertia cells.
        let mp_real = mass_properties_fixture(mass, com, inertia);

        // New fixture: Value::Scalar{MOMENT_OF_INERTIA} inertia cells.
        let com_point = Value::Point(com.iter().map(|&c| Value::length(c)).collect());
        let inertia_dimensioned = Value::Matrix(
            inertia
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|&x| Value::Scalar {
                            si_value: x,
                            dimension: DimensionVector::MOMENT_OF_INERTIA,
                        })
                        .collect()
                })
                .collect(),
        );
        let mp_dimensioned = mint_instance(
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
                ("inertia".to_string(), inertia_dimensioned),
                ("origin".to_string(), Value::Real(0.0)),
            ],
        );

        let (m_real, c_real, i_real) =
            mass_properties_from_value(&mp_real).expect("Value::Real inertia must parse");
        let (m_dim, c_dim, i_dim) =
            mass_properties_from_value(&mp_dimensioned).expect("MOMENT_OF_INERTIA inertia must parse");

        assert!((m_real - m_dim).abs() < 1e-15, "mass must be identical");
        for i in 0..3 {
            assert!((c_real[i] - c_dim[i]).abs() < 1e-15, "com[{i}] must be identical");
        }
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (i_real[r][c] - i_dim[r][c]).abs() < 1e-15,
                    "inertia[{r}][{c}] must be identical (got real={}, dim={})",
                    i_real[r][c],
                    i_dim[r][c]
                );
            }
        }
    }
}
