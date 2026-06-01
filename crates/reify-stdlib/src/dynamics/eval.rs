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

use crate::dynamics::spatial::Frame3;
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

// ── Value↔core marshalling extractors (RBD-η steps 4/6) ───────────────────────
//
// These are consumed by the open-chain dispatch (`inverse_dynamics_at_snapshot`,
// step-6) and exercised directly by the step-4 unit tests. `#[allow(dead_code)]`
// covers the window before step-6 wires them into the (non-test) dispatch path.

/// Extract three SI-unit components from a `Value::Point` / `Value::Vector` /
/// `Value::List` of exactly three numeric cells (dimensions stripped via
/// `si_value`). Returns `None` for any other shape or arity.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
        for i in 0..4 {
            assert!((f.rotation()[i] - quat[i]).abs() < 1e-12, "quat[{i}]");
        }
        for i in 0..3 {
            assert!((f.translation()[i] - trans[i]).abs() < 1e-12, "trans[{i}]");
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
}
