//! Eval-side `Value`↔core dispatch for the RBD-η stdlib dynamics entry points
//! (`docs/prds/v0_3/rigid-body-dynamics.md` §5 / task RBD-η, Phase 4).
//!
//! This module is the `Value`-marshalling half of the dynamics surface: it
//! extracts `Value`s into the pure-`f64` `RneaLink` / KKT inputs consumed by
//! the [`crate::dynamics::rnea`] and [`crate::dynamics::closed_chain`] cores,
//! invokes them, and reshapes the result `τ` back into registry-free
//! `JointForce` / `MotionTrajectory` `Value::StructureInstance`s.
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
//!     (closed-chain routing layered into the snapshot core, step-10)

use reify_core::dimension::DimensionVector;
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

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
        // inverse_dynamics_at_snapshot_lower / inverse_dynamics_lower land in
        // RBD-η steps 6/8/10.
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
    // The single driving joint is stored in the `mechanism` placeholder field
    // (Real per the structure_def); `inverse_dynamics` takes the mechanism as a
    // separate arg and does not consume this field.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
