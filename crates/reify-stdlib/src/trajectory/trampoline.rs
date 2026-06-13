//! Pure cache-key + `Value`↔core marshalling half of the trajectory
//! ComputeNode trampolines (`simulate_trajectory` + `input_shape`), task π
//! (3876; `docs/prds/v0_3/trajectory-input-shaping.md` §6/§11,
//! `docs/prds/v0_3/compute-node-contract.md` §4 GR-002).
//!
//! Mirrors the modal `free_vibration`/`transient_response` split
//! (`modal/trampoline.rs`) and the `inverse_dynamics` split
//! (`dynamics/trampoline.rs`): the engine-facing `ComputeFn` wrappers
//! (warm-state cache + cancellation) live in `reify-eval`
//! (`trajectory_ops.rs`), because `ComputeOutcome` / `OpaqueState` /
//! `CancellationHandle` are `reify-eval` (resp. `reify-ir`) types and the
//! dependency graph `reify-eval → reify-expr → reify-stdlib` forbids
//! `reify-stdlib` from depending on `reify-eval`.
//!
//! This module holds only the pure, `reify-eval`-free half:
//! - the content-hash cache keys (`SimulateTrajectoryCacheKey`,
//!   `InputShapeCacheKey`) the warm-state cache is keyed on;
//! - the `Value`↔core marshalling helpers (`value_to_multijoint_spline` /
//!   `value_to_modal_model` / `value_to_mechanism_model` /
//!   `track_data_to_value`), which must run inside `reify-stdlib` because the
//!   θ/κ core types (`MechanismModel` / `ModalModel` / `MultiJointSpline` /
//!   `EndEffectorTrackData`) are `pub(crate)` here;
//! - the two `Value`→`Value` composers (`simulate_trajectory_value` /
//!   `input_shape_value`) reify-eval calls (re-exported at the crate root,
//!   mirroring `reify_stdlib::build_train_for_shaper`);
//! - the three accessor impls (`end_effector_track_at` /
//!   `deviation_from_nominal_at` / `peak_deviation_at`) routed from
//!   `eval_trajectory`.
//!
//! Populated incrementally across task π's TDD steps (cache keys → marshalling
//! → composers → accessors).

use reify_core::{ContentHash, DimensionVector};
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

use crate::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};
use super::impulse_shaper::convolve_at;
use super::input_shape::build_train_for_shaper;
use super::simulate::{
    simulate_trajectory_core, EffectorLocation, EndEffectorTrackData, LinkDesc, MechanismModel,
    ModalModel, ModeDesc, Pose3,
};
use super::spline::{BoundaryCondition, CubicSpline, KnotData, MultiJointSpline, QuinticSpline};
use super::tots::{solve_tots, JointWaypoints, SqpConfig, TotsModel, TotsOutcome, TotsParams};

/// The result-determining inputs of a `simulate_trajectory` forward-pass solve,
/// used to decide whether a cached `EndEffectorTrack` result (`reify-eval`'s
/// `trajectory_ops` warm state) can be reused for a new call.
///
/// Three [`ContentHash`]es — one per `simulate_trajectory(profile, mech, modal)`
/// input ([`Value::content_hash`]). A full per-field match certifies the cached
/// result for reuse (a cache HIT). The user-observable signals map directly:
/// identical inputs ⇒ all hashes match ⇒ HIT; a profile control-point change ⇒
/// `profile_hash` differs ⇒ MISS (invalidation).
///
/// Compared via [`matches`](SimulateTrajectoryCacheKey::matches) — per-field
/// `ContentHash` equality. `Copy`/`Debug` but deliberately NOT `PartialEq` (the
/// single comparison path is `matches`, exactly mirroring
/// `dynamics::trampoline::InverseDynamicsCacheKey`); `Value::content_hash`
/// canonicalizes `NaN` and preserves `-0.0`, so comparison is collision-free
/// and deterministic.
#[derive(Clone, Copy, Debug)]
pub struct SimulateTrajectoryCacheKey {
    /// Content hash of the profile `Value` (`profile.content_hash()`).
    pub profile_hash: ContentHash,
    /// Content hash of the mechanism `Value` (`mech.content_hash()`).
    pub mech_hash: ContentHash,
    /// Content hash of the modal-result `Value` (`modal.content_hash()`).
    pub modal_hash: ContentHash,
}

impl SimulateTrajectoryCacheKey {
    /// Build a key from the three `simulate_trajectory` inputs, each hashed via
    /// [`Value::content_hash`].
    pub fn from_inputs(profile: &Value, mech: &Value, modal: &Value) -> Self {
        Self {
            profile_hash: profile.content_hash(),
            mech_hash: mech.content_hash(),
            modal_hash: modal.content_hash(),
        }
    }

    /// `true` iff every field hash equals `other`'s — i.e. a cached result built
    /// for `other` may be reused for `self` (a cache HIT). Per-field
    /// `ContentHash` equality is symmetric and collision-free.
    pub fn matches(&self, other: &SimulateTrajectoryCacheKey) -> bool {
        self.profile_hash == other.profile_hash
            && self.mech_hash == other.mech_hash
            && self.modal_hash == other.modal_hash
    }
}

/// The result-determining inputs of an `input_shape` solve, used to decide
/// whether a cached shaped-`Profile` result (`reify-eval`'s `trajectory_ops`
/// warm state) can be reused for a new call.
///
/// Two [`ContentHash`]es — one per `input_shape(profile, shaper)` input
/// ([`Value::content_hash`]). A full per-field match certifies the cached
/// shaped `Profile` for reuse (a cache HIT); a profile control-point change ⇒
/// `profile_hash` differs ⇒ MISS. Folding the whole `shaper` `Value` covers
/// both the cheap impulse arms (ZV/ZVD/EI/Cascaded) and the heavy TOTS arm
/// uniformly — the cache is high-value only for TOTS, but routing the impulse
/// arms through the same key is harmless.
///
/// Compared via [`matches`](InputShapeCacheKey::matches). `Copy`/`Debug`, NOT
/// `PartialEq` — the same single-`matches`-path discipline as
/// [`SimulateTrajectoryCacheKey`] and
/// `dynamics::trampoline::InverseDynamicsCacheKey`.
#[derive(Clone, Copy, Debug)]
pub struct InputShapeCacheKey {
    /// Content hash of the profile `Value` (`profile.content_hash()`).
    pub profile_hash: ContentHash,
    /// Content hash of the shaper `Value` (`shaper.content_hash()`).
    pub shaper_hash: ContentHash,
}

impl InputShapeCacheKey {
    /// Build a key from the two `input_shape` inputs, each hashed via
    /// [`Value::content_hash`].
    pub fn from_inputs(profile: &Value, shaper: &Value) -> Self {
        Self {
            profile_hash: profile.content_hash(),
            shaper_hash: shaper.content_hash(),
        }
    }

    /// `true` iff every field hash equals `other`'s (a cache HIT). Per-field
    /// `ContentHash` equality is symmetric and collision-free.
    pub fn matches(&self, other: &InputShapeCacheKey) -> bool {
        self.profile_hash == other.profile_hash && self.shaper_hash == other.shaper_hash
    }
}

// ── Value→core marshalling ──────────────────────────────────────────────────

/// Read a numeric stdlib field as `f64` — a dimensioned `Scalar` (SI magnitude),
/// a `Real`, or an `Int`. Any other variant yields `None`. Mirrors
/// `input_shape::read_scalar_si` / `modal_ops::read_scalar_si`.
fn read_scalar_si(val: &Value) -> Option<f64> {
    match val {
        Value::Scalar { si_value, .. } => Some(*si_value),
        Value::Real(r) => Some(*r),
        Value::Int(n) => Some(*n as f64),
        _ => None,
    }
}

/// Read a `Value::List<numeric>` into `Vec<f64>` (each element via
/// [`read_scalar_si`]). `None` if `val` is not a `List` or any element is
/// non-numeric.
fn read_real_list(val: &Value) -> Option<Vec<f64>> {
    let Value::List(items) = val else {
        return None;
    };
    items.iter().map(read_scalar_si).collect()
}

/// Read an optional per-waypoint `List<JointValue>` field (`vels` / `accels`):
/// `Value::Option(Some(list))` → `Some(vec)`; `Value::Option(None)`, an absent
/// field, or a non-numeric list → `None`. The `None` return is the "no
/// per-knot derivative data" case the cubic path tolerates and the quintic
/// path rejects.
fn read_opt_real_list(val: Option<&Value>) -> Option<Vec<f64>> {
    match val {
        Some(Value::Option(Some(inner))) => read_real_list(inner),
        _ => None,
    }
}

/// Marshal a `PiecewisePolynomialProfile` `Value` into a [`MultiJointSpline`]
/// — the deferred β `Value`→spline boundary (PRD §4.1).
///
/// Reads the four eval-path fields:
/// - `waypoints : List<Waypoint>` — each `Waypoint` carries a `t` (`Time`
///   scalar, SI seconds), a `values : List<JointValue>` (per-joint positions),
///   and optional `vels` / `accels` (`Option<List<JointValue>>`). At least two
///   waypoints are required; all must share the same joint count (≥ 1).
/// - `spline_kind : SplineKind` — the enum tag selects degree:
///   `CubicSpline` → [`CubicSpline::fit`] per joint; `QuinticSpline` →
///   [`QuinticSpline::fit`] per joint (which needs every waypoint's `vels` AND
///   `accels`).
/// - `boundary : BoundaryCondition` — for the cubic path, the `StructureInstance`
///   `type_name` dispatches the [`BoundaryCondition`]: `NaturalSpline` →
///   `Natural`, `PeriodicSpline` → `Periodic`, `ClampedSpline` → `Clamped`
///   (reading the per-joint `start_velocity` / `end_velocity` lists, which must
///   match the joint count). The quintic Hermite path is fully determined by
///   per-knot value/vel/accel and ignores `boundary`.
///
/// Returns `None` (never panics) for any malformed / degenerate input: a
/// non-`StructureInstance`, a missing / non-`List` `waypoints`, `< 2`
/// waypoints, an inconsistent or empty joint count, a non-numeric `t` / value,
/// an unrecognised `boundary` tag (cubic path) or `spline_kind` variant, a
/// degenerate knot set (`CubicSpline::fit` / `QuinticSpline::fit` returning
/// `None`), or a quintic profile missing per-waypoint `vels` / `accels`.
pub(crate) fn value_to_multijoint_spline(profile: &Value) -> Option<MultiJointSpline> {
    let Value::StructureInstance(data) = profile else {
        return None;
    };

    // waypoints : List<Waypoint>, at least two.
    let Some(Value::List(wps)) = data.fields.get(&"waypoints".to_string()) else {
        return None;
    };
    if wps.len() < 2 {
        return None;
    }

    // Parse each waypoint's t / values / (optional) vels / accels.
    let mut ts: Vec<f64> = Vec::with_capacity(wps.len());
    let mut vals: Vec<Vec<f64>> = Vec::with_capacity(wps.len()); // [waypoint][joint]
    let mut vels: Vec<Option<Vec<f64>>> = Vec::with_capacity(wps.len());
    let mut accels: Vec<Option<Vec<f64>>> = Vec::with_capacity(wps.len());
    for wp in wps {
        let Value::StructureInstance(wp_data) = wp else {
            return None;
        };
        let t = read_scalar_si(wp_data.fields.get(&"t".to_string())?)?;
        let v = read_real_list(wp_data.fields.get(&"values".to_string())?)?;
        ts.push(t);
        vals.push(v);
        vels.push(read_opt_real_list(wp_data.fields.get(&"vels".to_string())));
        accels.push(read_opt_real_list(wp_data.fields.get(&"accels".to_string())));
    }

    // Consistent, non-empty joint count across all waypoints.
    let n_joints = vals[0].len();
    if n_joints == 0 || vals.iter().any(|v| v.len() != n_joints) {
        return None;
    }

    // spline_kind enum tag selects polynomial degree.
    let Some(Value::Enum { variant, .. }) = data.fields.get(&"spline_kind".to_string()) else {
        return None;
    };

    match variant.as_str() {
        "CubicSpline" => {
            // boundary dispatch (per-joint velocities for Clamped).
            let Value::StructureInstance(b) = data.fields.get(&"boundary".to_string())? else {
                return None;
            };
            let clamped_vels: Option<(Vec<f64>, Vec<f64>)> = if b.type_name == "ClampedSpline" {
                let start = read_real_list(b.fields.get(&"start_velocity".to_string())?)?;
                let end = read_real_list(b.fields.get(&"end_velocity".to_string())?)?;
                if start.len() != n_joints || end.len() != n_joints {
                    return None;
                }
                Some((start, end))
            } else {
                None
            };

            let mut joints = Vec::with_capacity(n_joints);
            for j in 0..n_joints {
                let values_j: Vec<f64> = vals.iter().map(|v| v[j]).collect();
                let bc = match b.type_name.as_str() {
                    "NaturalSpline" => BoundaryCondition::Natural,
                    "PeriodicSpline" => BoundaryCondition::Periodic,
                    "ClampedSpline" => {
                        let (start, end) = clamped_vels.as_ref().unwrap();
                        BoundaryCondition::Clamped {
                            start_vel: start[j],
                            end_vel: end[j],
                        }
                    }
                    _ => return None, // unrecognised boundary tag
                };
                joints.push(CubicSpline::fit(&ts, &values_j, &bc)?);
            }
            MultiJointSpline::new_cubic(joints)
        }
        "QuinticSpline" => {
            let mut joints = Vec::with_capacity(n_joints);
            for j in 0..n_joints {
                let mut knots = Vec::with_capacity(wps.len());
                for i in 0..wps.len() {
                    // Quintic Hermite needs every waypoint's vel AND accel.
                    let vel = vels[i].as_ref()?;
                    let acc = accels[i].as_ref()?;
                    if vel.len() != n_joints || acc.len() != n_joints {
                        return None;
                    }
                    knots.push(KnotData {
                        t: ts[i],
                        value: vals[i][j],
                        vel: vel[j],
                        accel: acc[j],
                    });
                }
                joints.push(QuinticSpline::fit(&knots)?);
            }
            MultiJointSpline::new_quintic(joints)
        }
        _ => None, // unrecognised spline_kind variant
    }
}

// ── evaluate_profile* / profile_duration composers (task 4539) ─────────────
//
// Thin `Value`→`Value` adapters that call `value_to_multijoint_spline` + the
// `MultiJointSpline` evaluators. Routed from `eval_trajectory` for both the
// public name and the `*_at` undeclared-delegate name (same two-name pattern
// as `gcode_import`/`gcode_import_lower`). Return `Value::Undef` on any
// unmarshalable / degenerate input — the loud not-computed signal rather than
// a numeric placeholder.
//
// TODO(perf): each call re-fits the spline from scratch (O(n_knots) via // ptodo:allow permanent perf note, no live owner task
// value_to_multijoint_spline). Dense sampling loops pay that cost N times.
// A future fitted-spline cache keyed on the profile Value (mirroring
// InputShapeCacheKey) would amortize the fit to once per profile.

/// Sample a profile at time `t` (SI seconds), returning a `Value::List` of
/// `Value::Real` per joint — or `Value::Undef` on unmarshalable input.
pub(crate) fn evaluate_profile_value(profile: &Value, t: &Value) -> Value {
    let Some(spline) = value_to_multijoint_spline(profile) else {
        return Value::Undef;
    };
    let Some(t_si) = read_scalar_si(t) else {
        return Value::Undef;
    };
    Value::List(spline.eval(t_si).into_iter().map(Value::Real).collect())
}

/// First-derivative companion: returns `[q̇(t)]` per joint, or `Value::Undef`.
pub(crate) fn evaluate_profile_dot_value(profile: &Value, t: &Value) -> Value {
    let Some(spline) = value_to_multijoint_spline(profile) else {
        return Value::Undef;
    };
    let Some(t_si) = read_scalar_si(t) else {
        return Value::Undef;
    };
    Value::List(spline.eval_dot(t_si).into_iter().map(Value::Real).collect())
}

/// Second-derivative companion: returns `[q̈(t)]` per joint, or `Value::Undef`.
pub(crate) fn evaluate_profile_ddot_value(profile: &Value, t: &Value) -> Value {
    let Some(spline) = value_to_multijoint_spline(profile) else {
        return Value::Undef;
    };
    let Some(t_si) = read_scalar_si(t) else {
        return Value::Undef;
    };
    Value::List(
        spline.eval_ddot(t_si).into_iter().map(Value::Real).collect(),
    )
}

/// Duration accessor: returns `Value::Scalar{TIME, si_value: spline.duration()}`
/// — the `[t_first, t_last]` knot span — or `Value::Undef` on unmarshalable input.
pub(crate) fn profile_duration_value(profile: &Value) -> Value {
    let Some(spline) = value_to_multijoint_spline(profile) else {
        return Value::Undef;
    };
    Value::Scalar {
        si_value: spline.duration(),
        dimension: DimensionVector::TIME,
    }
}

/// Flatten a `Mode.shape` (`List<Vector3<Dimensionless>>`) into a flat
/// `Vec<f64>` of length `3·n_nodes` — each per-node `Value::Vector` / `List`'s
/// three components read via [`read_scalar_si`]. A non-`List` / malformed shape
/// yields an empty vector; non-numeric components are skipped.
#[allow(dead_code)]
fn flatten_shape(shape: &Value) -> Vec<f64> {
    let Value::List(nodes) = shape else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(nodes.len() * 3);
    for node in nodes {
        let comps = match node {
            Value::Vector(c) | Value::List(c) => c,
            _ => continue,
        };
        out.extend(comps.iter().filter_map(read_scalar_si));
    }
    out
}

/// Marshal a `ModalResult` `Value` into a [`ModalModel`] (PRD §6.2): map each
/// `Mode` (`frequency` → `freq_hz` (Hz), `damping_ratio` → `zeta`, flattened
/// `shape` → `force_projection`) to a [`ModeDesc`].
///
/// Degenerate inputs yield an EMPTY [`ModalModel`] (never panics): a
/// non-`StructureInstance`, a missing / non-`List` `modes` field, or an empty
/// `modes` list. Individual non-`StructureInstance` mode entries are skipped.
/// The θ core handles an empty modal model gracefully (zero vibration), so an
/// empty result is the right "no modal data" signal rather than a hard error.
#[allow(dead_code)]
pub(crate) fn value_to_modal_model(modal: &Value) -> ModalModel {
    let Value::StructureInstance(data) = modal else {
        return ModalModel { modes: Vec::new() };
    };
    let Some(Value::List(modes)) = data.fields.get(&"modes".to_string()) else {
        return ModalModel { modes: Vec::new() };
    };
    let mut out = Vec::with_capacity(modes.len());
    for m in modes {
        let Value::StructureInstance(mode) = m else {
            continue;
        };
        let freq_hz = mode
            .fields
            .get(&"frequency".to_string())
            .and_then(read_scalar_si)
            .unwrap_or(0.0);
        let zeta = mode
            .fields
            .get(&"damping_ratio".to_string())
            .and_then(read_scalar_si)
            .unwrap_or(0.0);
        let force_projection = mode
            .fields
            .get(&"shape".to_string())
            .map(flatten_shape)
            .unwrap_or_default();
        out.push(ModeDesc {
            freq_hz,
            zeta,
            force_projection,
        });
    }
    ModalModel { modes: out }
}

/// Read a body's `mass` field (a `StructureInstance` field or a `Map` entry) as
/// `f64`. `None` if the body is neither shape, the field is absent, or the value
/// is non-numeric — the caller then defaults to unit mass.
#[allow(dead_code)]
fn body_mass(body: &Value) -> Option<f64> {
    match body {
        Value::StructureInstance(d) => d.fields.get(&"mass".to_string()).and_then(read_scalar_si),
        Value::Map(m) => m
            .get(&Value::String("mass".to_string()))
            .and_then(read_scalar_si),
        _ => None,
    }
}

/// Build one placeholder [`LinkDesc`]: an identity parent-to-child transform, a
/// single prismatic-X motion DOF, the given mass, and a point-mass inertia (COM
/// at the body origin, zero rotational inertia).
///
/// This is the uniform-`Real`-placeholder link shape used until the
/// kinematic-completion PRD lands a real per-link inertia tensor (an OCCT
/// geometry-kernel property not exposed at the `Value` layer). The θ core
/// handles point-mass / minimal links gracefully (its mandated tests use exactly
/// this single-link unit-mass fixture).
#[allow(dead_code)]
fn placeholder_link(mass: f64) -> LinkDesc {
    LinkDesc {
        parent_to_child: SpatialTransform6::from_frame3(&Frame3::identity()),
        subspace: vec![SpatialVector6::from_array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0])],
        mass,
        com: [0.0; 3],
        inertia_about_com: [[0.0; 3]; 3],
    }
}

/// Marshal a mechanism `Value` into a [`MechanismModel`] plus the per-location
/// modal-participation descriptors ([`EffectorLocation`]) the forward simulator
/// needs (PRD §6.1; design-decision: placeholder / point-mass inertia).
///
/// `n_modes` is the modal model's mode count — the single end-effector
/// [`EffectorLocation`]'s `mode_coeffs` default to unit participation
/// (`vec![1.0; n_modes]`), so they must be sized to the modal model and cannot
/// be derived from `mech` alone; the [`simulate_trajectory_value`] composer
/// (which holds the marshalled [`ModalModel`]) passes its mode count.
///
/// Two arms:
/// - A structured `Value::Map(kind = "mechanism")` carrying a non-empty
///   `bodies : List` builds one [`placeholder_link`] per body, reading each
///   body's `mass` (unit when absent). Per-body transforms remain identity (a
///   documented placeholder — real `pose`/`world_transform` marshalling tightens
///   when the kinematic-completion PRD lands).
/// - Anything else — the `mechanism : Real` placeholder that flows across
///   `trajectory.ri` today, an unrecognised variant, or an empty `bodies` list —
///   falls back to a single identity-transform, unit-mass, prismatic-X link.
///
/// Never panics; always returns at least one link and exactly one effector
/// location.
#[allow(dead_code)]
pub(crate) fn value_to_mechanism_model(
    mech: &Value,
    n_modes: usize,
) -> (MechanismModel, Vec<EffectorLocation>) {
    // The single end-effector location: unit per-mode participation, sized to
    // the modal model.
    let effector = vec![EffectorLocation {
        mode_coeffs: vec![1.0; n_modes],
    }];

    // Structured arm: Value::Map(kind="mechanism") with a non-empty bodies list.
    if let Value::Map(map) = mech {
        let is_mechanism = matches!(
            map.get(&Value::String("kind".to_string())),
            Some(Value::String(k)) if k == "mechanism"
        );
        let bodies: &[Value] = match map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(bodies)) => bodies,
            _ => &[],
        };
        if is_mechanism && !bodies.is_empty() {
            let links = bodies
                .iter()
                .map(|b| placeholder_link(body_mass(b).unwrap_or(1.0)))
                .collect();
            return (MechanismModel { links }, effector);
        }
    }

    // Fallback: the Real placeholder / unrecognised / empty-bodies case → one
    // identity-transform, unit-mass, prismatic-X link.
    (
        MechanismModel {
            links: vec![placeholder_link(1.0)],
        },
        effector,
    )
}

// ── core→Value marshalling ───────────────────────────────────────────────────

/// Marshal an [`EndEffectorTrackData`] (the pure-Rust forward-pass output) into
/// an `EndEffectorTrack` `Value::StructureInstance` — the η output boundary
/// (PRD §6.2). Mirrors `modal_ops`'s `DisplacementTimeHistory` construction (a
/// registry-free sentinel instance, `StructureTypeId(u32::MAX)`, version 1).
///
/// Field mapping (the core's `[location][time]` indexing is preserved — outer =
/// location, inner = time):
/// - `t_samples : List<Time>` — each instant a `Value::Scalar` carrying the
///   `TIME` dimension;
/// - `nominal_pose` / `combined_pose : List<List<Real>>` — each [`Pose3`]
///   flattened to its Z position (`position[2]`). The core maps scalar modal
///   vibration onto the Z axis (`[0, 0, s]`), so Z is the only deviation-bearing
///   component and the flatten is lossless for the residual-vibration metric;
/// - `vibration_offset : List<List<Real>>` — each `[dx, dy, dz]` flattened to
///   `dz`, consistent with the same scalar→Z mapping. This makes
///   `deviation_from_nominal = |combined − nominal| = |vibration|`.
///
/// `mechanism` / `modal_result` are `Real` placeholders (the kinematic-/modal-
/// completion PRDs are not landed and nothing in η reads them). An empty track
/// yields well-formed empty `List`s — never panics.
#[allow(dead_code)]
pub(crate) fn track_data_to_value(track: &EndEffectorTrackData) -> Value {
    // t_samples : List<Scalar TIME>.
    let t_samples = Value::List(
        track
            .t_samples
            .iter()
            .map(|&t| Value::Scalar {
                si_value: t,
                dimension: DimensionVector::TIME,
            })
            .collect(),
    );

    // Flatten a [location][time] Pose3 matrix to List<List<Real>> on the Z
    // position component (the core's scalar→Z vibration mapping).
    let poses_to_value = |matrix: &[Vec<Pose3>]| -> Value {
        Value::List(
            matrix
                .iter()
                .map(|row| Value::List(row.iter().map(|p| Value::Real(p.position[2])).collect()))
                .collect(),
        )
    };
    // Flatten a [location][time] vibration matrix to List<List<Real>> on dz.
    let offsets_to_value = |matrix: &[Vec<[f64; 3]>]| -> Value {
        Value::List(
            matrix
                .iter()
                .map(|row| Value::List(row.iter().map(|o| Value::Real(o[2])).collect()))
                .collect(),
        )
    };

    let fields: PersistentMap<String, Value> = [
        ("mechanism".to_string(), Value::Real(0.0)),
        ("modal_result".to_string(), Value::Real(0.0)),
        ("t_samples".to_string(), t_samples),
        (
            "nominal_pose".to_string(),
            poses_to_value(&track.nominal_pose),
        ),
        (
            "vibration_offset".to_string(),
            offsets_to_value(&track.vibration_offset),
        ),
        (
            "combined_pose".to_string(),
            poses_to_value(&track.combined_pose),
        ),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "EndEffectorTrack".to_string(),
        version: 1,
        fields,
    }))
}

// ── Value→Value composers ────────────────────────────────────────────────────

/// Compose `simulate_trajectory(profile, mech, modal)` at the `Value` layer —
/// the θ-deferred Value integration (PRD §6.2). `reify-eval`'s
/// `simulate_trajectory_trampoline` calls this through the crate-root re-export,
/// mirroring the `build_train_for_shaper` boundary (the θ/κ core types are
/// `pub(crate)`, so the pipeline must run inside `reify-stdlib`).
///
/// Pipeline: marshal `modal`→[`ModalModel`] (`value_to_modal_model`),
/// `mech`→([`MechanismModel`], effector locations) sized to the mode count
/// (`value_to_mechanism_model`), `profile`→[`MultiJointSpline`]
/// (`value_to_multijoint_spline`); run [`simulate_trajectory_core`]; marshal the
/// [`EndEffectorTrackData`] back with [`track_data_to_value`].
///
/// Two graceful non-results (never a panic):
/// - **bad args** — a `profile` that is not even a `StructureInstance` (a bare
///   `Real`, `Undef`, …) cannot be a profile → [`Value::Undef`];
/// - **degenerate profile** — a recognizable `PiecewisePolynomialProfile`
///   instance that nonetheless cannot yield a spline (`< 2` waypoints,
///   unfittable knots, an unknown boundary / spline-kind tag) → a well-formed
///   EMPTY `EndEffectorTrack` (the simulator has nothing to integrate).
///
/// The instance check disambiguates the two reasons `value_to_multijoint_spline`
/// returns `None`: "malformed input" (Undef) stays distinct from
/// "valid-but-trivial input" (empty track).
pub fn simulate_trajectory_value(profile: &Value, mech: &Value, modal: &Value) -> Value {
    // Bad args: a profile must be a (PiecewisePolynomialProfile) StructureInstance.
    if !matches!(profile, Value::StructureInstance(_)) {
        return Value::Undef;
    }

    let modal_model = value_to_modal_model(modal);
    let (mech_model, effector_locs) = value_to_mechanism_model(mech, modal_model.modes.len());

    let track = match value_to_multijoint_spline(profile) {
        Some(spline) => {
            simulate_trajectory_core(&spline, &mech_model, &modal_model, &effector_locs)
        }
        // A recognizable but degenerate profile → a well-formed empty track
        // (this mirrors `simulate.rs::empty_track_data`, which is private there:
        // `t_samples` empty, one empty inner series per effector location).
        None => EndEffectorTrackData {
            t_samples: Vec::new(),
            nominal_pose: vec![Vec::new(); effector_locs.len()],
            vibration_offset: vec![Vec::new(); effector_locs.len()],
            combined_pose: vec![Vec::new(); effector_locs.len()],
        },
    };
    track_data_to_value(&track)
}

/// Per-original-knot-interval oversample factor for the impulse arm's shaped
/// command grid — resolves the command curvature when the shaped waypoints are
/// re-fit to a spline downstream.
const SAMPLES_PER_KNOT_INTERVAL: usize = 16;
/// Per-shaper-delay oversample factor — resolves the shaper's impulse steps (the
/// breakpoints the convolution introduces at `knot + tᵢ`), so the Δ-extended
/// command re-fits faithfully (this is what lets the ≥40 dB cancellation survive
/// the round-trip through the downstream simulator's spline re-marshal).
const SAMPLES_PER_SHAPER_DELAY: usize = 8;
/// Upper bound on the shaped grid size — a backstop against a pathological
/// `t_domain` / Δ ratio producing an unbounded waypoint count.
const MAX_SHAPED_SAMPLES: usize = 1024;

/// Compose `input_shape(profile, shaper)` at the `Value` layer — the deferred ζ
/// REAL command-waveform shaping (impulse arm; PRD §5.3). `reify-eval`'s
/// `input_shape_trampoline` calls this through the crate-root re-export (mirroring
/// the `build_train_for_shaper` boundary).
///
/// **Impulse arm** (ZV / ZVD / EI / Cascaded): resolve the shaper to an
/// [`ImpulseTrain`](super::impulse_shaper::ImpulseTrain)
/// ([`build_train_for_shaper`]), marshal the profile command to a
/// [`MultiJointSpline`] ([`value_to_multijoint_spline`]), then resample the
/// convolved command `f_shaped(t) = Σ Aᵢ·command(t − tᵢ)` ([`convolve_at`]) on a
/// uniform grid over the Δ-extended domain `[0, duration + trailing_time]`. The
/// result is a fresh `PiecewisePolynomialProfile` whose `waypoints` carry the
/// shaped command while **every other field** (`mechanism` / `boundary` /
/// `spline_kind`) is preserved verbatim — real shaping changes ONLY the
/// waypoints, so the landed echo-era assertions (which check those three fields
/// are preserved) still hold.
///
/// **TOTS arm** (`TOTSShaper`): dispatched FIRST (the impulse path cannot
/// resolve a `TOTSShaper`). Marshal the profile's waypoints into a per-joint
/// point-to-point spec ([`JointWaypoints`] — `start` / `interior…` / `end`), the
/// shaper's scalar `velocity_limit` / `acceleration_limit` into the per-joint
/// constraints, its `vibration_tolerance` into `vib_tol`, its `modes` into a
/// [`ModalModel`] ([`value_to_modal_model`]), and its `max_iters` / `tol` into the
/// [`SqpConfig`]; seed `t_initial` from the input profile's own duration (the
/// un-optimised baseline). Run the time-optimal SQP loop ([`solve_tots`]) and
/// re-emit the optimised `[start, interior…, end]` per joint at uniform fractions
/// of the solved `T` (mirroring `tots::build_spline`'s knot layout), again
/// preserving every non-`waypoints` field — so the move is genuinely *re-timed*,
/// never an echo. `ConstraintInfeasible` ⇒ no feasible shaped profile exists ⇒
/// `Undef`; `Converged` / `NonConvergence` (best-feasible iterate) ⇒ the re-timed
/// `Profile`.
///
/// Returns [`Value::Undef`] for: a non-`StructureInstance` profile or shaper; an
/// unrecognised shaper with no resolvable train (not in {ZV, ZVD, EI, Cascaded}
/// and not a `TOTSShaper`); an impulse-arm profile that does not marshal to a
/// spline, or a TOTS-arm profile with `< 2` / inconsistent waypoints; or a
/// `TOTSShaper` whose problem is `ConstraintInfeasible`.
///
/// New waypoints reuse the input waypoints' registered `type_id` / `type_name` /
/// `version` so the shaped profile binds like the original; per-waypoint
/// `vels` / `accels` are emitted as `Option(None)` (the cubic re-fit ignores
/// them — a quintic round-trip would additionally need the convolved derivative
/// commands, out of scope for the impulse arm's cubic acceptance path).
pub fn input_shape_value(profile: &Value, shaper: &Value) -> Value {
    // Both args must be StructureInstances (the bad-args convention).
    let Value::StructureInstance(profile_data) = profile else {
        return Value::Undef;
    };
    let Value::StructureInstance(shaper_data) = shaper else {
        return Value::Undef;
    };

    // ── TOTS arm — dispatch BEFORE the impulse-train path ───────────────────
    // build_train_for_shaper returns None for a TOTSShaper (it only knows
    // ZV/ZVD/EI/Cascaded), so the heavy time-optimal SQP solve must be reached
    // here first. Real command re-timing (κ → π), not an echo: infeasible ⇒
    // Undef, Converged / NonConvergence ⇒ the re-timed Profile.
    if shaper_data.type_name == "TOTSShaper" {
        return input_shape_tots(profile_data, shaper);
    }

    // Resolve the shaper to its impulse train. `build_train_for_shaper` returns
    // None for an unrecognised shaper AND for a `TOTSShaper` (whose arm is above)
    // — either ⇒ Undef here.
    let Some(train) = build_train_for_shaper(shaper) else {
        return Value::Undef;
    };
    // Marshal the profile command; an unmarshallable profile cannot be shaped.
    let Some(spline) = value_to_multijoint_spline(profile) else {
        return Value::Undef;
    };

    let t_domain = spline.duration();
    let delta = train.trailing_time();
    let span = t_domain + delta;
    let n_joints = spline.eval(0.0).len();

    // Uniform resample spacing fine enough to resolve BOTH the command curvature
    // (oversample the input knot intervals) and the shaper steps (oversample Δ).
    let n_in = match profile_data.fields.get(&"waypoints".to_string()) {
        Some(Value::List(wps)) => wps.len().max(2),
        _ => 2,
    };
    let dt_curv = t_domain / ((n_in - 1) * SAMPLES_PER_KNOT_INTERVAL) as f64;
    let dt = if delta > 0.0 {
        dt_curv.min(delta / SAMPLES_PER_SHAPER_DELAY as f64)
    } else {
        dt_curv
    };
    let n_intervals = if dt > 0.0 && dt.is_finite() {
        ((span / dt).ceil() as usize).clamp(1, MAX_SHAPED_SAMPLES - 1)
    } else {
        1
    };
    let n_out = n_intervals + 1;
    let step = span / n_intervals as f64;

    // Waypoint template: reuse the input waypoints' registered identity so the
    // shaped profile's waypoints bind like the originals; sentinel fallback when
    // the input shape is unavailable.
    let (wp_type_id, wp_type_name, wp_version) =
        match profile_data.fields.get(&"waypoints".to_string()) {
            Some(Value::List(wps)) => match wps.first() {
                Some(Value::StructureInstance(d)) => (d.type_id, d.type_name.clone(), d.version),
                _ => (StructureTypeId(u32::MAX), "Waypoint".to_string(), 1),
            },
            _ => (StructureTypeId(u32::MAX), "Waypoint".to_string(), 1),
        };

    let mut new_waypoints = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let t = (i as f64) * step;
        let values: Vec<Value> = (0..n_joints)
            .map(|j| {
                Value::Real(convolve_at(
                    &train,
                    &|tau: f64| spline.eval(tau)[j],
                    t_domain,
                    t,
                ))
            })
            .collect();
        let fields: PersistentMap<String, Value> = [
            (
                "t".to_string(),
                Value::Scalar {
                    si_value: t,
                    dimension: DimensionVector::TIME,
                },
            ),
            ("values".to_string(), Value::List(values)),
            ("vels".to_string(), Value::Option(None)),
            ("accels".to_string(), Value::Option(None)),
        ]
        .into_iter()
        .collect();
        new_waypoints.push(Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: wp_type_id,
            type_name: wp_type_name.clone(),
            version: wp_version,
            fields,
        })));
    }

    // Preserve every profile field; replace only `waypoints`.
    let new_fields = profile_data
        .fields
        .insert_functional("waypoints".to_string(), Value::List(new_waypoints));
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: profile_data.type_id,
        type_name: profile_data.type_name.clone(),
        version: profile_data.version,
        fields: new_fields,
    }))
}

/// Read numeric field `name` from a `StructureInstance`'s fields as `f64`,
/// falling back to `default` when absent / non-numeric. Mirrors
/// `input_shape::field_f64` — the single coercion path for the `TOTSShaper`'s
/// scalar limit / solver fields.
fn field_f64(data: &StructureInstanceData, name: &str, default: f64) -> f64 {
    data.fields
        .get(&name.to_string())
        .and_then(read_scalar_si)
        .unwrap_or(default)
}

/// Marshal a `PiecewisePolynomialProfile`'s waypoints into the per-joint TOTS
/// point-to-point spec ([`JointWaypoints`]) plus the profile's baseline duration
/// (`last.t − first.t`, the un-optimised `t_initial` seed).
///
/// Per joint `j`: `start = wp[0].values[j]`, `end = wp[last].values[j]`,
/// `interior = [wp[1..last].values[j]]` (the optimisable waypoints). The scalar
/// `vel_limit` / `acc_limit` / `max_force` are applied uniformly to every joint —
/// the same uniform-placeholder convention `value_to_mechanism_model` uses for
/// inertia (structured per-joint `actuator_limits` marshalling tightens when the
/// kinematic-completion PRD lands).
///
/// Returns `None` (never panics) for `< 2` waypoints, a non-`List` `waypoints`
/// field, a non-`StructureInstance` waypoint, a non-numeric `t` / `values`, an
/// empty or inconsistent joint count, or a non-positive duration.
fn profile_to_joint_waypoints(
    profile_data: &StructureInstanceData,
    vel_limit: f64,
    acc_limit: f64,
    max_force: f64,
) -> Option<(Vec<JointWaypoints>, f64)> {
    let Some(Value::List(wps)) = profile_data.fields.get(&"waypoints".to_string()) else {
        return None;
    };
    if wps.len() < 2 {
        return None;
    }

    let mut ts: Vec<f64> = Vec::with_capacity(wps.len());
    let mut vals: Vec<Vec<f64>> = Vec::with_capacity(wps.len()); // [waypoint][joint]
    for wp in wps {
        let Value::StructureInstance(d) = wp else {
            return None;
        };
        let t = read_scalar_si(d.fields.get(&"t".to_string())?)?;
        let v = read_real_list(d.fields.get(&"values".to_string())?)?;
        ts.push(t);
        vals.push(v);
    }

    let n_joints = vals[0].len();
    if n_joints == 0 || vals.iter().any(|v| v.len() != n_joints) {
        return None;
    }
    let duration = ts[ts.len() - 1] - ts[0];
    // Reject a non-positive, NaN, or infinite baseline duration (`!is_finite()`
    // catches NaN/±inf; `<= 0.0` catches a zero/reversed knot span).
    if duration <= 0.0 || !duration.is_finite() {
        return None;
    }

    let n_wp = vals.len();
    let joints = (0..n_joints)
        .map(|j| JointWaypoints {
            start: vals[0][j],
            interior: (1..n_wp - 1).map(|i| vals[i][j]).collect(),
            end: vals[n_wp - 1][j],
            vel_limit,
            acc_limit,
            max_force,
        })
        .collect();
    Some((joints, duration))
}

/// The `TOTSShaper` arm of [`input_shape_value`]: marshal the profile + shaper
/// into the [`solve_tots`] inputs, run the time-optimal SQP loop, and re-emit the
/// optimised command as a re-timed `Profile` (or [`Value::Undef`] when the
/// problem is infeasible / the profile cannot be marshalled). See
/// [`input_shape_value`]'s docs for the full contract.
fn input_shape_tots(profile_data: &StructureInstanceData, shaper: &Value) -> Value {
    let Value::StructureInstance(shaper_data) = shaper else {
        // Unreachable: the caller dispatches here only for a StructureInstance.
        return Value::Undef;
    };

    // Solver-parameterising scalar fields (defaults mirror `input_shape::run_tots`).
    let vel_limit = field_f64(shaper_data, "velocity_limit", 100.0);
    let acc_limit = field_f64(shaper_data, "acceleration_limit", 1000.0);
    let max_force = field_f64(shaper_data, "force_limit", 1000.0);
    let vib_tol = field_f64(shaper_data, "vibration_tolerance", 0.02);
    let max_iters = field_f64(shaper_data, "max_iters", 100.0) as usize;
    let tol = field_f64(shaper_data, "tol", 1e-6);

    // Profile waypoints → per-joint P2P spec + baseline (un-optimised) duration.
    let Some((joints, duration)) =
        profile_to_joint_waypoints(profile_data, vel_limit, acc_limit, max_force)
    else {
        return Value::Undef;
    };

    // Modal model from the shaper's `modes` (value_to_modal_model reads the
    // `modes` field — shared with ModalResult); mechanism from the profile's
    // `mechanism` placeholder field (single-link unit-mass fallback).
    let modal = value_to_modal_model(shaper);
    let fallback_mech = Value::Real(0.0);
    let mech_value = profile_data
        .fields
        .get(&"mechanism".to_string())
        .unwrap_or(&fallback_mech);
    let (mechanism, effector_locations) = value_to_mechanism_model(mech_value, modal.modes.len());

    let params = TotsParams {
        joints,
        t_initial: duration,
        vib_tol,
        n_grid: 30,
    };
    let model = TotsModel {
        mechanism,
        modal,
        effector_locations,
    };
    let config = SqpConfig {
        max_iters,
        tol,
        ..Default::default()
    };

    let result = solve_tots(params, &model, &config);
    match result.outcome {
        // No feasible shaped profile exists (E_TrajectoryConstraintInfeasible).
        TotsOutcome::ConstraintInfeasible => Value::Undef,
        // Converged, or the solver's best-feasible iterate
        // (W_TrajectorySolverNonConvergence) — both are valid shaped profiles.
        TotsOutcome::Converged | TotsOutcome::NonConvergence => {
            tots_result_to_profile(profile_data, &result.params)
        }
    }
}

/// Build the re-timed `PiecewisePolynomialProfile` `Value` from a solved
/// [`TotsParams`]: one waypoint per TOTS knot (uniform fractions of the optimised
/// `T`, mirroring `tots::build_spline`'s knot layout) carrying the optimised
/// `[start, interior…, end]` per joint. Every non-`waypoints` field of the input
/// profile is preserved verbatim — only the command is re-timed — and the new
/// waypoints reuse the input waypoints' registered identity (sentinel fallback),
/// exactly like the impulse arm.
fn tots_result_to_profile(profile_data: &StructureInstanceData, params: &TotsParams) -> Value {
    let t = params.t_initial;
    let n_int = params.joints.first().map(|j| j.interior.len()).unwrap_or(0);
    let n_knots = n_int + 2;

    // Per-joint knot values: [start, interior…, end].
    let knot_values: Vec<Vec<f64>> = params
        .joints
        .iter()
        .map(|j| {
            let mut v = Vec::with_capacity(n_knots);
            v.push(j.start);
            v.extend_from_slice(&j.interior);
            v.push(j.end);
            v
        })
        .collect();

    // Waypoint identity reused from the input (sentinel fallback) — mirrors the
    // impulse arm so the shaped profile binds like the original.
    let (wp_type_id, wp_type_name, wp_version) =
        match profile_data.fields.get(&"waypoints".to_string()) {
            Some(Value::List(wps)) => match wps.first() {
                Some(Value::StructureInstance(d)) => (d.type_id, d.type_name.clone(), d.version),
                _ => (StructureTypeId(u32::MAX), "Waypoint".to_string(), 1),
            },
            _ => (StructureTypeId(u32::MAX), "Waypoint".to_string(), 1),
        };

    let mut new_waypoints = Vec::with_capacity(n_knots);
    for i in 0..n_knots {
        let ti = if n_knots > 1 {
            i as f64 / (n_knots - 1) as f64 * t
        } else {
            0.0
        };
        let values: Vec<Value> = knot_values.iter().map(|kv| Value::Real(kv[i])).collect();
        let fields: PersistentMap<String, Value> = [
            (
                "t".to_string(),
                Value::Scalar {
                    si_value: ti,
                    dimension: DimensionVector::TIME,
                },
            ),
            ("values".to_string(), Value::List(values)),
            ("vels".to_string(), Value::Option(None)),
            ("accels".to_string(), Value::Option(None)),
        ]
        .into_iter()
        .collect();
        new_waypoints.push(Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: wp_type_id,
            type_name: wp_type_name.clone(),
            version: wp_version,
            fields,
        })));
    }

    // Preserve every profile field; replace only `waypoints`.
    let new_fields = profile_data
        .fields
        .insert_functional("waypoints".to_string(), Value::List(new_waypoints));
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: profile_data.type_id,
        type_name: profile_data.type_name.clone(),
        version: profile_data.version,
        fields: new_fields,
    }))
}

// ── EndEffectorTrack accessor intrinsics ─────────────────────────────────────
//
// The three η lazy accessors over an `EndEffectorTrack` Value (PRD §6.2), routed
// from `eval_trajectory` via the `*_at` delegate names the `trajectory.ri` bodies
// call (`end_effector_track` → `end_effector_track_at`, …). They read the
// marshalled `[location][time]` matrices `track_data_to_value` emits: the
// combined-pose Z column at a location, the per-time deviation
// `|combined − nominal|` (= `|vibration|` under the core's scalar→Z mapping), and
// the peak of that deviation series. Every malformed / out-of-range input yields
// an empty list / zero — never a panic (the η-stub fallback contract).

/// Read a `LocationId` argument (a `Real` / `Int` index) into a `usize` location
/// index. `None` for a non-numeric, negative, or non-finite index — the accessor
/// then yields an empty / zero result.
fn read_location_index(location: &Value) -> Option<usize> {
    let idx = read_scalar_si(location)?;
    if !idx.is_finite() || idx < 0.0 {
        return None;
    }
    Some(idx.round() as usize)
}

/// Read the inner `List<Real>` series at `[location]` of a `[location][time]`
/// matrix field (`combined_pose` / `nominal_pose`) on an `EndEffectorTrack`
/// Value. `None` for a non-`StructureInstance` track, a missing / non-`List`
/// field, an out-of-range location, or a non-`List` / non-numeric inner row.
fn track_location_series(track: &Value, field: &str, location: usize) -> Option<Vec<f64>> {
    let Value::StructureInstance(data) = track else {
        return None;
    };
    let Some(Value::List(outer)) = data.fields.get(&field.to_string()) else {
        return None;
    };
    let Value::List(inner) = outer.get(location)? else {
        return None;
    };
    inner.iter().map(read_scalar_si).collect()
}

/// The per-time deviation series `|combined − nominal|` at `location` (one entry
/// per overlapping time sample). Empty for a bad index / malformed track /
/// missing series — the shared core of [`deviation_from_nominal_at`] and
/// [`peak_deviation_at`]. Each pose is a single Z scalar (the core's scalar→Z
/// mapping), so the per-time Euclidean distance reduces to `|combined − nominal|`.
fn deviation_series(track: &Value, location: &Value) -> Vec<f64> {
    let Some(loc) = read_location_index(location) else {
        return Vec::new();
    };
    let (Some(combined), Some(nominal)) = (
        track_location_series(track, "combined_pose", loc),
        track_location_series(track, "nominal_pose", loc),
    ) else {
        return Vec::new();
    };
    combined
        .iter()
        .zip(nominal.iter())
        .map(|(c, n)| (c - n).abs())
        .collect()
}

/// `end_effector_track_at(track, location)` — the combined-pose Z time-series at
/// `location` (one `Real` per `t_samples` instant; the core flattens each pose to
/// its Z component). An out-of-range location or malformed track → an empty
/// `List` (never a panic).
pub(crate) fn end_effector_track_at(track: &Value, location: &Value) -> Value {
    let series = read_location_index(location)
        .and_then(|loc| track_location_series(track, "combined_pose", loc))
        .unwrap_or_default();
    Value::List(series.into_iter().map(Value::Real).collect())
}

/// `deviation_from_nominal_at(track, location)` — the per-time Euclidean
/// deviation `|combined − nominal|` at `location` (one `Real` per `t_samples`
/// instant). The core maps scalar modal vibration onto the Z axis, so this equals
/// `|vibration|`. An out-of-range location / malformed track → an empty `List`
/// (never a panic).
pub(crate) fn deviation_from_nominal_at(track: &Value, location: &Value) -> Value {
    Value::List(
        deviation_series(track, location)
            .into_iter()
            .map(Value::Real)
            .collect(),
    )
}

/// `peak_deviation_at(track, location)` — the maximum per-time deviation
/// `maxₜ |combined − nominal|` at `location` (a single `Real`). An out-of-range
/// location / malformed track / empty series → `0.0` (never a panic).
pub(crate) fn peak_deviation_at(track: &Value, location: &Value) -> Value {
    let peak = deviation_series(track, location)
        .into_iter()
        .fold(0.0_f64, f64::max);
    Value::Real(peak)
}

#[cfg(test)]
mod tests {
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

    use super::{InputShapeCacheKey, SimulateTrajectoryCacheKey};

    /// A registry-free `Value::StructureInstance` with `type_name` + fields,
    /// mirroring the eval-side `mint_instance` shape (same fixture pattern as
    /// `dynamics/trampoline.rs` tests). Used to build distinguishable `Value`
    /// inputs whose `content_hash` folds in every field.
    fn instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
        let fields: PersistentMap<String, Value> = fields.into_iter().collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: type_name.to_string(),
            version: 1,
            fields,
        }))
    }

    /// Minimal `PiecewisePolynomialProfile`-shaped fixture distinguished by a
    /// single control value `p` (folded into the content hash). The cache-key
    /// tests care only that distinct `p` ⇒ distinct hash and identical `p` ⇒
    /// identical hash, not the full marshalled shape (that is steps 5/6).
    fn profile(p: f64) -> Value {
        instance(
            "PiecewisePolynomialProfile",
            vec![("control".to_string(), Value::Real(p))],
        )
    }

    /// Minimal `mech` fixture — the simulate path takes `mech : Real`
    /// (trajectory_fns.ri), so the bare `Value::Real` is the canonical input.
    fn mech(m: f64) -> Value {
        Value::Real(m)
    }

    /// Minimal `ModalResult`-shaped fixture distinguished by a single mode
    /// frequency `f`.
    fn modal(f: f64) -> Value {
        instance("ModalResult", vec![("frequency".to_string(), Value::Real(f))])
    }

    // ── step-1: SimulateTrajectoryCacheKey::from_inputs / matches ───────────────

    /// (a) Two keys built from identical `(profile, mech, modal)` match — the
    /// cache-HIT condition.
    #[test]
    fn simulate_cache_key_matches_identical_inputs() {
        let p = profile(1.0);
        let m = mech(1.0);
        let md = modal(10.0);
        let a = SimulateTrajectoryCacheKey::from_inputs(&p, &m, &md);
        let b = SimulateTrajectoryCacheKey::from_inputs(&p, &m, &md);
        assert!(a.matches(&b), "identical (profile, mech, modal) must match");
    }

    /// (b) A different profile `Value` must NOT match — and the relation is
    /// symmetric.
    #[test]
    fn simulate_cache_key_differs_on_profile() {
        let m = mech(1.0);
        let md = modal(10.0);
        let a = SimulateTrajectoryCacheKey::from_inputs(&profile(1.0), &m, &md);
        let b = SimulateTrajectoryCacheKey::from_inputs(&profile(2.0), &m, &md);
        assert!(!a.matches(&b), "a different profile must MISS");
        assert!(!b.matches(&a), "matches() must be symmetric");
    }

    /// (c) A different mech `Value` must NOT match.
    #[test]
    fn simulate_cache_key_differs_on_mech() {
        let p = profile(1.0);
        let md = modal(10.0);
        let a = SimulateTrajectoryCacheKey::from_inputs(&p, &mech(1.0), &md);
        let b = SimulateTrajectoryCacheKey::from_inputs(&p, &mech(2.0), &md);
        assert!(!a.matches(&b), "a different mech must MISS");
    }

    /// (d) A different modal `Value` must NOT match.
    #[test]
    fn simulate_cache_key_differs_on_modal() {
        let p = profile(1.0);
        let m = mech(1.0);
        let a = SimulateTrajectoryCacheKey::from_inputs(&p, &m, &modal(10.0));
        let b = SimulateTrajectoryCacheKey::from_inputs(&p, &m, &modal(20.0));
        assert!(!a.matches(&b), "a different modal must MISS");
    }

    /// Minimal `ZVShaper`-shaped fixture distinguished by a single target
    /// frequency `f`. Any `Shaper` variant works for the key tests — the cache
    /// key folds the whole shaper `Value` regardless of concrete type.
    fn shaper(f: f64) -> Value {
        instance(
            "ZVShaper",
            vec![("target_frequency".to_string(), Value::Real(f))],
        )
    }

    // ── step-3: InputShapeCacheKey::from_inputs / matches ───────────────────────

    /// (a) Two keys built from identical `(profile, shaper)` match — cache HIT.
    #[test]
    fn input_shape_cache_key_matches_identical_inputs() {
        let p = profile(1.0);
        let s = shaper(10.0);
        let a = InputShapeCacheKey::from_inputs(&p, &s);
        let b = InputShapeCacheKey::from_inputs(&p, &s);
        assert!(a.matches(&b), "identical (profile, shaper) must match");
    }

    /// (b) A different profile `Value` must NOT match.
    #[test]
    fn input_shape_cache_key_differs_on_profile() {
        let s = shaper(10.0);
        let a = InputShapeCacheKey::from_inputs(&profile(1.0), &s);
        let b = InputShapeCacheKey::from_inputs(&profile(2.0), &s);
        assert!(!a.matches(&b), "a different profile must MISS");
    }

    /// (c) A different shaper `Value` must NOT match — and the relation is
    /// symmetric.
    #[test]
    fn input_shape_cache_key_differs_on_shaper() {
        let p = profile(1.0);
        let a = InputShapeCacheKey::from_inputs(&p, &shaper(10.0));
        let b = InputShapeCacheKey::from_inputs(&p, &shaper(20.0));
        assert!(!a.matches(&b), "a different shaper must MISS");
        assert!(!b.matches(&a), "matches() must be symmetric");
    }

    // ── steps 5/6: value_to_multijoint_spline ───────────────────────────────────

    use reify_core::DimensionVector;

    use super::super::spline::MultiJointSpline;
    use super::value_to_multijoint_spline;

    /// Loose tolerance for marshalled-spline knot-interpolation assertions
    /// (the spline math itself is tested to 1e-12 in `spline.rs`; here we only
    /// confirm the `Value`→core marshalling wired the right knots/values).
    const SPLINE_TOL: f64 = 1e-9;

    /// A `Time` scalar `Value` (SI seconds), the shape a `Waypoint.t` field
    /// carries on the eval path.
    fn time(s: f64) -> Value {
        Value::Scalar {
            si_value: s,
            dimension: DimensionVector::TIME,
        }
    }

    /// A per-joint `List<Real>` `Value` (a `Waypoint.values` or velocity list).
    fn reals(vs: &[f64]) -> Value {
        Value::List(vs.iter().map(|&v| Value::Real(v)).collect())
    }

    /// An `Option<List<JointValue>>` field — `Value::Option(Some(List))` when
    /// supplied, `Value::Option(None)` otherwise (the eval-layer encoding of a
    /// Reify `Option`, per `reify_ir::Value::Option`).
    fn opt_reals(o: Option<&[f64]>) -> Value {
        match o {
            Some(vs) => Value::Option(Some(Box::new(reals(vs)))),
            None => Value::Option(None),
        }
    }

    /// A `Waypoint` `Value::StructureInstance` (t scalar SI; values List<Real>;
    /// vels/accels Option<List<Real>>) as `value_to_multijoint_spline` reads it.
    fn waypoint(t: f64, values: &[f64], vels: Option<&[f64]>, accels: Option<&[f64]>) -> Value {
        instance(
            "Waypoint",
            vec![
                ("t".to_string(), time(t)),
                ("values".to_string(), reals(values)),
                ("vels".to_string(), opt_reals(vels)),
                ("accels".to_string(), opt_reals(accels)),
            ],
        )
    }

    /// A `SplineKind` enum `Value` (`variant` ∈ {`CubicSpline`, `QuinticSpline`}).
    fn spline_kind(variant: &str) -> Value {
        Value::Enum {
            type_name: "SplineKind".to_string(),
            variant: variant.to_string(),
        }
    }

    /// A `PiecewisePolynomialProfile` `Value::StructureInstance` with the four
    /// eval-path fields (mechanism / waypoints / boundary / spline_kind).
    fn pp_profile(waypoints: Vec<Value>, boundary: Value, kind: Value) -> Value {
        instance(
            "PiecewisePolynomialProfile",
            vec![
                ("mechanism".to_string(), Value::Real(0.0)),
                ("waypoints".to_string(), Value::List(waypoints)),
                ("boundary".to_string(), boundary),
                ("spline_kind".to_string(), kind),
            ],
        )
    }

    /// (a) A well-formed 2-joint, 2-waypoint natural-cubic profile marshals to a
    /// `MultiJointSpline::Cubic` whose duration is the knot span and whose
    /// sampled `q` at each knot equals that waypoint's `values`.
    #[test]
    fn value_to_spline_natural_cubic_interpolates_at_knots() {
        let profile = pp_profile(
            vec![
                waypoint(0.0, &[1.0, 5.0], None, None),
                waypoint(2.0, &[3.0, 9.0], None, None),
            ],
            instance("NaturalSpline", vec![]),
            spline_kind("CubicSpline"),
        );
        let spline = value_to_multijoint_spline(&profile)
            .expect("a well-formed natural-cubic profile must marshal to Some");
        assert!(
            matches!(spline, MultiJointSpline::Cubic(_)),
            "SplineKind::CubicSpline must select a cubic spline"
        );
        assert!(
            (spline.duration() - 2.0).abs() < SPLINE_TOL,
            "duration must equal last-first knot t (2.0), got {}",
            spline.duration()
        );
        let q0 = spline.eval(0.0);
        assert_eq!(q0.len(), 2, "two joints");
        assert!(
            (q0[0] - 1.0).abs() < SPLINE_TOL && (q0[1] - 5.0).abs() < SPLINE_TOL,
            "q at the first knot must equal waypoint[0].values, got {q0:?}"
        );
        let q1 = spline.eval(2.0);
        assert!(
            (q1[0] - 3.0).abs() < SPLINE_TOL && (q1[1] - 9.0).abs() < SPLINE_TOL,
            "q at the last knot must equal waypoint[1].values, got {q1:?}"
        );
    }

    /// Boundary dispatch: a `ClampedSpline` (per-joint start/end velocity lists)
    /// is read into `BoundaryCondition::Clamped`, so the marshalled spline
    /// reproduces the prescribed zero endpoint tangents (which a `NaturalSpline`
    /// over the same data — a slope-1 line — would NOT).
    #[test]
    fn value_to_spline_clamped_cubic_dispatches_boundary() {
        let profile = pp_profile(
            vec![
                waypoint(0.0, &[0.0], None, None),
                waypoint(1.0, &[1.0], None, None),
            ],
            instance(
                "ClampedSpline",
                vec![
                    ("start_velocity".to_string(), reals(&[0.0])),
                    ("end_velocity".to_string(), reals(&[0.0])),
                ],
            ),
            spline_kind("CubicSpline"),
        );
        let spline = value_to_multijoint_spline(&profile)
            .expect("a well-formed clamped-cubic profile must marshal to Some");
        assert!(matches!(spline, MultiJointSpline::Cubic(_)));
        assert!(
            spline.eval_dot(0.0)[0].abs() < SPLINE_TOL,
            "clamped start tangent must be the prescribed 0"
        );
        assert!(
            spline.eval_dot(1.0)[0].abs() < SPLINE_TOL,
            "clamped end tangent must be the prescribed 0"
        );
    }

    /// Boundary dispatch: a `PeriodicSpline` is read into
    /// `BoundaryCondition::Periodic` and marshals. The cyclic periodic solver
    /// needs ≥3 unknowns (`n-1 ≥ 3`, i.e. ≥4 waypoints) and matching endpoint
    /// values to close the loop — mirror `spline.rs`'s own periodic fixture (a
    /// sine sampled over one full period, endpoints equal).
    #[test]
    fn value_to_spline_periodic_cubic_dispatches_boundary() {
        let profile = pp_profile(
            vec![
                waypoint(0.0, &[0.0], None, None),
                waypoint(1.0, &[1.0], None, None),
                waypoint(2.0, &[0.0], None, None),
                waypoint(3.0, &[-1.0], None, None),
                waypoint(4.0, &[0.0], None, None),
            ],
            instance("PeriodicSpline", vec![]),
            spline_kind("CubicSpline"),
        );
        let spline = value_to_multijoint_spline(&profile)
            .expect("a well-formed periodic-cubic profile must marshal to Some");
        assert!(matches!(spline, MultiJointSpline::Cubic(_)));
        assert!((spline.duration() - 4.0).abs() < SPLINE_TOL);
    }

    /// cubic-vs-quintic selection: `SplineKind::QuinticSpline` + per-waypoint
    /// `vels` AND `accels` marshals to a `MultiJointSpline::Quintic` that
    /// reproduces the prescribed endpoint value/velocity exactly.
    #[test]
    fn value_to_spline_quintic_selects_quintic_and_reproduces_knots() {
        let profile = pp_profile(
            vec![
                waypoint(0.0, &[0.0], Some(&[0.0]), Some(&[0.0])),
                waypoint(1.0, &[1.0], Some(&[0.0]), Some(&[0.0])),
            ],
            instance("NaturalSpline", vec![]),
            spline_kind("QuinticSpline"),
        );
        let spline = value_to_multijoint_spline(&profile)
            .expect("a well-formed quintic profile with vels+accels must marshal to Some");
        assert!(
            matches!(spline, MultiJointSpline::Quintic(_)),
            "SplineKind::QuinticSpline must select a quintic spline"
        );
        assert!((spline.eval(0.0)[0] - 0.0).abs() < SPLINE_TOL);
        assert!((spline.eval(1.0)[0] - 1.0).abs() < SPLINE_TOL);
        assert!(
            spline.eval_dot(0.0)[0].abs() < SPLINE_TOL,
            "quintic start vel must equal the prescribed 0"
        );
        assert!(
            spline.eval_dot(1.0)[0].abs() < SPLINE_TOL,
            "quintic end vel must equal the prescribed 0"
        );
    }

    /// (b) Degenerate / malformed inputs all return `None` (no panic): empty or
    /// `<2` waypoints, a non-`StructureInstance`, an unrecognised boundary tag,
    /// an unrecognised `spline_kind`, and a quintic profile missing vels/accels.
    #[test]
    fn value_to_spline_rejects_degenerate_inputs() {
        let nat = || instance("NaturalSpline", vec![]);
        let cubic = || spline_kind("CubicSpline");
        let two_wp = || {
            vec![
                waypoint(0.0, &[0.0], None, None),
                waypoint(1.0, &[1.0], None, None),
            ]
        };

        // Empty waypoint list.
        assert!(
            value_to_multijoint_spline(&pp_profile(vec![], nat(), cubic())).is_none(),
            "empty waypoints → None"
        );
        // Fewer than 2 waypoints.
        assert!(
            value_to_multijoint_spline(&pp_profile(
                vec![waypoint(0.0, &[0.0], None, None)],
                nat(),
                cubic()
            ))
            .is_none(),
            "<2 waypoints → None"
        );
        // Non-StructureInstance profile.
        assert!(
            value_to_multijoint_spline(&Value::Real(1.0)).is_none(),
            "a bare scalar is not a profile → None"
        );
        // Unrecognised boundary type-tag (the cubic path validates the boundary).
        assert!(
            value_to_multijoint_spline(&pp_profile(
                two_wp(),
                instance("FooSpline", vec![]),
                cubic()
            ))
            .is_none(),
            "unrecognised boundary → None"
        );
        // Unrecognised spline_kind variant.
        assert!(
            value_to_multijoint_spline(&pp_profile(
                two_wp(),
                nat(),
                spline_kind("SepticSpline")
            ))
            .is_none(),
            "unrecognised spline_kind → None"
        );
        // Quintic without per-waypoint vels/accels (cannot build KnotData).
        assert!(
            value_to_multijoint_spline(&pp_profile(
                two_wp(),
                nat(),
                spline_kind("QuinticSpline")
            ))
            .is_none(),
            "quintic without vels/accels → None"
        );
    }

    // ── steps 7/8: value_to_modal_model ─────────────────────────────────────────

    use super::super::simulate::{ModalModel, ModeDesc};
    use super::value_to_modal_model;

    /// A per-node `Vector3` `Value::Vector([Real, Real, Real])` — the modal
    /// `Mode.shape` element shape, per `modal_ops::mode_shape_value`.
    fn vec3(c: [f64; 3]) -> Value {
        Value::Vector(c.iter().map(|&x| Value::Real(x)).collect())
    }

    /// A `Mode` `Value::StructureInstance` (frequency Hz; damping_ratio ζ;
    /// shape List<Vector3>) as the modal eval path emits it.
    fn mode(freq_hz: f64, zeta: f64, shape: &[[f64; 3]]) -> Value {
        instance(
            "Mode",
            vec![
                ("frequency".to_string(), Value::Real(freq_hz)),
                ("damping_ratio".to_string(), Value::Real(zeta)),
                (
                    "shape".to_string(),
                    Value::List(shape.iter().map(|&c| vec3(c)).collect()),
                ),
            ],
        )
    }

    /// A `ModalResult` `Value::StructureInstance` wrapping a `modes` list.
    fn modal_result(modes: Vec<Value>) -> Value {
        instance("ModalResult", vec![("modes".to_string(), Value::List(modes))])
    }

    /// (a) A one-mode `ModalResult` marshals to a one-mode `ModalModel` whose
    /// `freq_hz` / `zeta` come straight from the `Mode` and whose
    /// `force_projection` is the flattened mode shape.
    #[test]
    fn value_to_modal_one_mode_maps_fields_and_flattens_shape() {
        let modal = modal_result(vec![mode(10.0, 0.05, &[[1.0, 0.0, 0.0], [0.0, 2.0, 0.0]])]);
        let mm: ModalModel = value_to_modal_model(&modal);
        assert_eq!(mm.modes.len(), 1, "one input Mode → one ModeDesc");
        let md: &ModeDesc = &mm.modes[0];
        assert!(
            (md.freq_hz - 10.0).abs() < SPLINE_TOL,
            "frequency → freq_hz (Hz), got {}",
            md.freq_hz
        );
        assert!(
            (md.zeta - 0.05).abs() < SPLINE_TOL,
            "damping_ratio → zeta, got {}",
            md.zeta
        );
        assert_eq!(
            md.force_projection,
            vec![1.0, 0.0, 0.0, 0.0, 2.0, 0.0],
            "shape List<Vector3> flattens to force_projection"
        );
    }

    /// (b) An empty `modes` list → an empty `ModalModel` (no panic).
    #[test]
    fn value_to_modal_empty_modes_is_empty_model() {
        let mm = value_to_modal_model(&modal_result(vec![]));
        assert!(mm.modes.is_empty(), "empty modes → empty ModalModel");
    }

    /// (c) A non-`StructureInstance` modal `Value` → an empty `ModalModel`.
    #[test]
    fn value_to_modal_non_instance_is_empty_model() {
        let mm = value_to_modal_model(&Value::Real(3.0));
        assert!(
            mm.modes.is_empty(),
            "non-StructureInstance modal → empty ModalModel"
        );
    }

    // ── steps 9/10: value_to_mechanism_model ────────────────────────────────────

    use std::collections::BTreeMap;

    use super::super::simulate::{EffectorLocation, MechanismModel};
    use super::value_to_mechanism_model;

    /// A structured mechanism `Value::Map(kind="mechanism")` carrying a `bodies`
    /// list — one `Body` `StructureInstance` per supplied mass. The schema is the
    /// forward-looking shape the kinematic-completion PRD will emit; until it
    /// lands `mechanism` stays a `Real` placeholder, so this exercises the
    /// structured arm in isolation.
    fn mechanism_map(masses: &[f64]) -> Value {
        let bodies: Vec<Value> = masses
            .iter()
            .map(|&m| instance("Body", vec![("mass".to_string(), Value::Real(m))]))
            .collect();
        let mut map = BTreeMap::new();
        map.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        map.insert(Value::String("bodies".to_string()), Value::List(bodies));
        Value::Map(map)
    }

    /// (a) The `mechanism : Real` placeholder (the only mechanism `Value` that
    /// flows today) marshals to a single-link, unit-mass, prismatic-X mechanism
    /// plus one `EffectorLocation` whose per-mode coefficients default to unit
    /// participation, sized to the modal model (`n_modes`).
    #[test]
    fn value_to_mechanism_real_placeholder_single_link_unit_mass() {
        let (mech, locs): (MechanismModel, Vec<EffectorLocation>) =
            value_to_mechanism_model(&Value::Real(1.0), 3);

        assert_eq!(
            mech.links.len(),
            1,
            "the Real placeholder → single-link fallback"
        );
        let link = &mech.links[0];
        assert_eq!(link.mass, 1.0, "unit-mass placeholder link");
        assert_eq!(link.subspace.len(), 1, "one DOF (prismatic)");
        assert_eq!(
            link.subspace[0].as_array(),
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            "prismatic-X motion subspace"
        );

        assert_eq!(locs.len(), 1, "one end-effector location");
        assert_eq!(
            locs[0].mode_coeffs,
            vec![1.0; 3],
            "unit per-mode coeffs sized to the modal model (n_modes = 3)"
        );
    }

    /// (b) A structured `Value::Map(kind="mechanism")` with a `bodies` list
    /// marshals to one `LinkDesc` per body, reading each body's `mass` field
    /// (point-mass placeholder inertia, unit mass when absent). The effector
    /// location is still single, sized to `n_modes`.
    #[test]
    fn value_to_mechanism_structured_one_link_per_body() {
        let (mech, locs) = value_to_mechanism_model(&mechanism_map(&[2.0, 3.0]), 2);

        assert_eq!(mech.links.len(), 2, "one LinkDesc per body");
        assert_eq!(mech.links[0].mass, 2.0, "body 0 mass read from the Value");
        assert_eq!(mech.links[1].mass, 3.0, "body 1 mass read from the Value");

        assert_eq!(locs.len(), 1, "one end-effector location");
        assert_eq!(locs[0].mode_coeffs, vec![1.0; 2], "coeffs sized to n_modes");
    }

    /// (c) Degenerate inputs fall back to the single-link unit-mass mechanism
    /// (never panics): an unrecognised `Value` variant, and a structured
    /// mechanism whose `bodies` list is empty.
    #[test]
    fn value_to_mechanism_degenerate_single_link_fallback() {
        // An unrecognised Value variant → single-link fallback.
        let (mech, locs) = value_to_mechanism_model(&Value::Undef, 1);
        assert_eq!(mech.links.len(), 1, "Undef → single-link fallback");
        assert_eq!(mech.links[0].mass, 1.0);
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].mode_coeffs, vec![1.0; 1]);

        // A structured mechanism with no bodies → single-link fallback.
        let (mech2, locs2) = value_to_mechanism_model(&mechanism_map(&[]), 1);
        assert_eq!(
            mech2.links.len(),
            1,
            "empty bodies → single-link fallback (no panic)"
        );
        assert_eq!(locs2.len(), 1);
    }

    // ── steps 11/12: track_data_to_value ────────────────────────────────────────

    use super::super::simulate::{EndEffectorTrackData, Pose3};
    use super::track_data_to_value;

    /// A `Pose3` whose Z position carries the per-`(location, time)` marker `z`
    /// (orientation identity). `track_data_to_value` flattens `position` to its
    /// Z component — the core maps scalar modal vibration onto the Z axis
    /// (`[0, 0, s]`, `simulate.rs`), so Z is the only pose component the
    /// `EndEffectorTrack` `Value` preserves.
    fn pose_z(z: f64) -> Pose3 {
        Pose3 {
            position: [0.0, 0.0, z],
            quaternion: [1.0, 0.0, 0.0, 0.0],
        }
    }

    /// A populated `EndEffectorTrackData` with `n_loc` locations × `n_times`
    /// times in the core's `[location][time]` convention: nominal Z = 0;
    /// vibration Z = the scalar marker `10·loc + t`; combined = nominal +
    /// vibration (so the flattened combined Z equals the marker and
    /// `combined − nominal` equals the vibration scalar).
    fn populated_track(n_loc: usize, n_times: usize) -> EndEffectorTrackData {
        let t_samples: Vec<f64> = (0..n_times).map(|t| t as f64 * 0.5).collect();
        let nominal_pose: Vec<Vec<Pose3>> = (0..n_loc)
            .map(|_| (0..n_times).map(|_| pose_z(0.0)).collect())
            .collect();
        let vibration_offset: Vec<Vec<[f64; 3]>> = (0..n_loc)
            .map(|loc| {
                (0..n_times)
                    .map(|t| [0.0, 0.0, (10 * loc + t) as f64])
                    .collect()
            })
            .collect();
        let combined_pose: Vec<Vec<Pose3>> = (0..n_loc)
            .map(|loc| {
                (0..n_times)
                    .map(|t| pose_z((10 * loc + t) as f64))
                    .collect()
            })
            .collect();
        EndEffectorTrackData {
            t_samples,
            nominal_pose,
            vibration_offset,
            combined_pose,
        }
    }

    /// Read a `Value::Real`, panicking otherwise (the three marshalled matrices
    /// must be `List<List<Real>>`).
    fn as_real(v: &Value) -> f64 {
        match v {
            Value::Real(r) => *r,
            other => panic!("expected a Real, got {other:?}"),
        }
    }

    /// (a) A populated track marshals to an `EndEffectorTrack`
    /// `StructureInstance`: `t_samples` is a `List<Scalar TIME>` of length
    /// `n_times` (SI values round-trip); each of nominal_pose / vibration_offset
    /// / combined_pose is a `List<List<Real>>` with outer len == `n_locations`
    /// and inner len == `n_times`.
    #[test]
    fn track_data_to_value_shapes_endeffectortrack_instance() {
        let (n_loc, n_times) = (2, 3);
        let track = populated_track(n_loc, n_times);
        let v = track_data_to_value(&track);

        let Value::StructureInstance(data) = &v else {
            panic!("track_data_to_value must yield a StructureInstance, got {v:?}");
        };
        assert_eq!(
            data.type_name, "EndEffectorTrack",
            "type_name must be EndEffectorTrack"
        );

        // t_samples : List<Scalar TIME>, length n_times, SI values round-trip.
        let Some(Value::List(ts)) = data.fields.get(&"t_samples".to_string()) else {
            panic!("t_samples must be a List");
        };
        assert_eq!(ts.len(), n_times, "t_samples length == n_times");
        for (i, item) in ts.iter().enumerate() {
            match item {
                Value::Scalar {
                    si_value,
                    dimension,
                } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::TIME,
                        "t_samples elements carry the TIME dimension"
                    );
                    assert!(
                        (si_value - i as f64 * 0.5).abs() < SPLINE_TOL,
                        "t_samples[{i}] SI value must round-trip, got {si_value}"
                    );
                }
                other => panic!("t_samples[{i}] must be a Scalar, got {other:?}"),
            }
        }

        // The three pose/offset matrices : List<List<Real>>, [location][time].
        for field in ["nominal_pose", "vibration_offset", "combined_pose"] {
            let Some(Value::List(outer)) = data.fields.get(&field.to_string()) else {
                panic!("{field} must be a List");
            };
            assert_eq!(outer.len(), n_loc, "{field} outer len == n_locations");
            for (loc, inner_v) in outer.iter().enumerate() {
                let Value::List(inner) = inner_v else {
                    panic!("{field}[{loc}] must be a List");
                };
                assert_eq!(inner.len(), n_times, "{field}[{loc}] inner len == n_times");
                for (t, cell) in inner.iter().enumerate() {
                    assert!(
                        matches!(cell, Value::Real(_)),
                        "{field}[{loc}][{t}] must be a Real, got {cell:?}"
                    );
                }
            }
        }
    }

    /// The Z-flatten is faithful: vibration_offset[loc][t] == the scalar marker;
    /// combined[loc][t] == nominal[loc][t] + vibration[loc][t]; nominal Z == 0.
    /// This pins `deviation_from_nominal = |combined − nominal| = |vibration|`
    /// (the Z-axis vibration mapping the ≥40 dB acceptance relies on).
    #[test]
    fn track_data_to_value_flattens_pose_position_to_z() {
        let (n_loc, n_times) = (2, 3);
        let v = track_data_to_value(&populated_track(n_loc, n_times));
        let Value::StructureInstance(data) = &v else {
            panic!("expected a StructureInstance");
        };
        let read = |name: &str| -> Vec<Vec<f64>> {
            let Some(Value::List(outer)) = data.fields.get(&name.to_string()) else {
                panic!("{name} must be a List");
            };
            outer
                .iter()
                .map(|row| match row {
                    Value::List(inner) => inner.iter().map(as_real).collect(),
                    other => panic!("{name} row must be a List, got {other:?}"),
                })
                .collect()
        };
        let nominal = read("nominal_pose");
        let vibration = read("vibration_offset");
        let combined = read("combined_pose");
        for loc in 0..n_loc {
            for t in 0..n_times {
                let marker = (10 * loc + t) as f64;
                assert!(nominal[loc][t].abs() < SPLINE_TOL, "nominal Z is 0");
                assert!(
                    (vibration[loc][t] - marker).abs() < SPLINE_TOL,
                    "vibration_offset Z == the scalar marker"
                );
                assert!(
                    (combined[loc][t] - (nominal[loc][t] + vibration[loc][t])).abs() < SPLINE_TOL,
                    "combined Z == nominal Z + vibration Z"
                );
            }
        }
    }

    /// (b) An empty track (no locations, no times) marshals to a well-formed
    /// `EndEffectorTrack` whose t_samples and three pose matrices are all empty
    /// `List`s (no panic).
    #[test]
    fn track_data_to_value_empty_track_is_well_formed_empty_lists() {
        let track = EndEffectorTrackData {
            t_samples: Vec::new(),
            nominal_pose: Vec::new(),
            vibration_offset: Vec::new(),
            combined_pose: Vec::new(),
        };
        let v = track_data_to_value(&track);
        let Value::StructureInstance(data) = &v else {
            panic!("an empty track must still yield a StructureInstance, got {v:?}");
        };
        assert_eq!(data.type_name, "EndEffectorTrack");
        for field in ["t_samples", "nominal_pose", "vibration_offset", "combined_pose"] {
            match data.fields.get(&field.to_string()) {
                Some(Value::List(items)) => {
                    assert!(items.is_empty(), "{field} must be an empty List")
                }
                other => panic!("{field} must be an empty List, got {other:?}"),
            }
        }
    }

    // ── steps 13/14: simulate_trajectory_value ──────────────────────────────────

    use std::f64::consts::PI;

    use super::simulate_trajectory_value;

    /// The analytic SDOF unit-step response (zero IC), copied from
    /// `simulate.rs`'s test module (a private test helper there). Used to
    /// independently certify the composer's vibration_offset against the same
    /// closed form θ validates the core against to 1e-9.
    fn analytic_step_response(p0: f64, omega: f64, zeta: f64, t: f64) -> f64 {
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega * t).exp();
        (p0 / (omega * omega))
            * (1.0 - decay * ((omega_d * t).cos() + (zeta * omega / omega_d) * (omega_d * t).sin()))
    }

    /// A single-mode `ModalResult` `Value` (5 Hz, ζ = 0.05, unit modal shape) —
    /// `value_to_modal_model` reads it to a one-mode `ModalModel` with unit
    /// `force_projection`.
    fn single_mode_modal() -> Value {
        modal_result(vec![mode(5.0, 0.05, &[[1.0, 0.0, 0.0]])])
    }

    /// A constant-acceleration `PiecewisePolynomialProfile` `Value` reproducing
    /// `simulate.rs::constant_accel_spline(duration, accel)`: 3 knots
    /// `[0, d/2, d]` of `q = ½·a·t²` with a clamped boundary (start vel 0, end
    /// vel `a·d`), so the marshalled cubic has `q̈ = a` exactly.
    fn constant_accel_profile(duration: f64, accel: f64) -> Value {
        let t1 = duration / 2.0;
        let t2 = duration;
        pp_profile(
            vec![
                waypoint(0.0, &[0.0], None, None),
                waypoint(t1, &[0.5 * accel * t1 * t1], None, None),
                waypoint(t2, &[0.5 * accel * t2 * t2], None, None),
            ],
            instance(
                "ClampedSpline",
                vec![
                    ("start_velocity".to_string(), reals(&[0.0])),
                    ("end_velocity".to_string(), reals(&[accel * duration])),
                ],
            ),
            spline_kind("CubicSpline"),
        )
    }

    /// (a) A constant-accel ramp profile + single-mode modal + the `mech : Real`
    /// placeholder (→ unit-mass prismatic link) marshals through the composer to
    /// an `EndEffectorTrack` whose `vibration_offset` Z series matches the
    /// analytic single-mode step response. The marshalled link is the unit-mass
    /// twin of θ's fixture, so the step magnitude is `p0 = mass·accel = accel`.
    #[test]
    fn simulate_trajectory_value_matches_analytic_single_mode_step() {
        let accel = 3.0_f64;
        let duration = 0.4_f64;
        let freq_hz = 5.0_f64;
        let zeta = 0.05_f64;
        let omega = 2.0 * PI * freq_hz;
        let p0 = accel; // unit marshalled mass

        let v = simulate_trajectory_value(
            &constant_accel_profile(duration, accel),
            &Value::Real(1.0),
            &single_mode_modal(),
        );
        let Value::StructureInstance(data) = &v else {
            panic!("composer must yield an EndEffectorTrack StructureInstance, got {v:?}");
        };
        assert_eq!(data.type_name, "EndEffectorTrack");

        let Some(Value::List(ts)) = data.fields.get(&"t_samples".to_string()) else {
            panic!("t_samples must be a List");
        };
        assert!(ts.len() >= 2, "a valid profile produces ≥2 samples");
        let Some(Value::List(outer)) = data.fields.get(&"vibration_offset".to_string()) else {
            panic!("vibration_offset must be a List");
        };
        assert_eq!(outer.len(), 1, "one effector location");
        let Value::List(series) = &outer[0] else {
            panic!("vibration_offset[0] must be a List");
        };
        assert_eq!(series.len(), ts.len(), "one vibration sample per time");

        let mut peak = 0.0_f64;
        for (cell, t_cell) in series.iter().zip(ts.iter()) {
            let dz = as_real(cell);
            let t = match t_cell {
                Value::Scalar { si_value, .. } => *si_value,
                other => panic!("t_samples element must be a Scalar, got {other:?}"),
            };
            let want = analytic_step_response(p0, omega, zeta, t);
            assert!(
                (dz - want).abs() < 1e-6,
                "vibration_offset Z must match the analytic step response at t={t:.4}: \
                 got {dz:.6e}, want {want:.6e}"
            );
            peak = peak.max(dz.abs());
        }
        assert!(
            peak > 1e-4,
            "the single-mode response must be a genuine (non-zero) vibration, peak={peak:.3e}"
        );
    }

    /// (b) A recognizable-but-degenerate profile (a `PiecewisePolynomialProfile`
    /// instance with `<2` waypoints — nothing to fit) yields a well-formed EMPTY
    /// `EndEffectorTrack` (empty `t_samples` / per-location series), NOT `Undef`
    /// and never a panic: the simulator simply has nothing to integrate.
    #[test]
    fn simulate_trajectory_value_degenerate_profile_is_empty_track() {
        let degenerate = pp_profile(
            vec![waypoint(0.0, &[0.0], None, None)], // a single waypoint cannot fit a spline
            instance("NaturalSpline", vec![]),
            spline_kind("CubicSpline"),
        );
        let v = simulate_trajectory_value(&degenerate, &Value::Real(1.0), &single_mode_modal());
        let Value::StructureInstance(data) = &v else {
            panic!("a degenerate profile must still yield an EndEffectorTrack, got {v:?}");
        };
        assert_eq!(data.type_name, "EndEffectorTrack");
        match data.fields.get(&"t_samples".to_string()) {
            Some(Value::List(items)) => {
                assert!(items.is_empty(), "degenerate profile → empty t_samples")
            }
            other => panic!("t_samples must be an empty List, got {other:?}"),
        }
        // The single per-location vibration series is empty (no time samples).
        if let Some(Value::List(outer)) = data.fields.get(&"vibration_offset".to_string()) {
            for row in outer {
                match row {
                    Value::List(inner) => assert!(inner.is_empty(), "empty per-location series"),
                    other => panic!("vibration_offset row must be a List, got {other:?}"),
                }
            }
        }
    }

    /// (c) Bad args — a non-`StructureInstance` profile (a bare `Real`, `Undef`)
    /// cannot be a profile at all → `Value::Undef` (distinct from the degenerate
    /// case, which still yields a track).
    #[test]
    fn simulate_trajectory_value_bad_args_is_undef() {
        assert!(
            matches!(
                simulate_trajectory_value(&Value::Real(0.0), &Value::Real(1.0), &single_mode_modal()),
                Value::Undef
            ),
            "a bare Real profile → Undef"
        );
        assert!(
            matches!(
                simulate_trajectory_value(&Value::Undef, &Value::Real(1.0), &single_mode_modal()),
                Value::Undef
            ),
            "an Undef profile → Undef"
        );
    }

    // ── steps 15/16: input_shape_value (impulse arm) ─────────────────────────────

    use super::super::impulse_shaper::convolve_at;
    use super::super::input_shape::build_train_for_shaper;
    use super::input_shape_value;

    /// A 1-joint unit ramp `q(t) = t` over `[0, 1]` as a natural-cubic
    /// `PiecewisePolynomialProfile` (collinear knots → the natural cubic is the
    /// exact line, so the marshalled command waveform is `f(τ) = τ`). The
    /// impulse arm convolves this command against the shaper train.
    fn ramp_profile() -> Value {
        pp_profile(
            vec![
                waypoint(0.0, &[0.0], None, None),
                waypoint(0.5, &[0.5], None, None),
                waypoint(1.0, &[1.0], None, None),
            ],
            instance("NaturalSpline", vec![]),
            spline_kind("CubicSpline"),
        )
    }

    /// Read a profile `Value`'s waypoints as `(t, values)` pairs — `t` a `Scalar
    /// TIME`, `values` a `List<Real>`. Panics on any other shape: the impl must
    /// emit exactly the eval-path Waypoint encoding `value_to_multijoint_spline`
    /// reads back (so a shaped profile round-trips through the simulator).
    fn read_waypoints(v: &Value) -> Vec<(f64, Vec<f64>)> {
        let Value::StructureInstance(data) = v else {
            panic!("expected a Profile StructureInstance, got {v:?}");
        };
        let Some(Value::List(wps)) = data.fields.get(&"waypoints".to_string()) else {
            panic!("waypoints must be a List");
        };
        wps.iter()
            .map(|wp| {
                let Value::StructureInstance(d) = wp else {
                    panic!("waypoint must be a StructureInstance");
                };
                let t = match d.fields.get(&"t".to_string()) {
                    Some(Value::Scalar { si_value, dimension }) => {
                        assert_eq!(*dimension, DimensionVector::TIME, "waypoint t carries TIME");
                        *si_value
                    }
                    other => panic!("waypoint t must be a Scalar TIME, got {other:?}"),
                };
                let vals = match d.fields.get(&"values".to_string()) {
                    Some(Value::List(xs)) => xs.iter().map(as_real).collect::<Vec<f64>>(),
                    other => panic!("waypoint values must be a List, got {other:?}"),
                };
                (t, vals)
            })
            .collect()
    }

    /// (impulse arm) A unit ramp shaped by `ZVShaper(10 Hz, ζ=0)` becomes a
    /// genuinely shaped `PiecewisePolynomialProfile` (NOT an echo of the input):
    /// - `mechanism` / `boundary` / `spline_kind` are preserved (only `waypoints`
    ///   change);
    /// - the total duration is extended by the shaper delay Δ = `trailing_time`;
    /// - every output waypoint's command equals the convolved reconstruction
    ///   `convolve_at(train, ramp_command, t_domain, t)` at that waypoint's time;
    /// - start (`ramp(0)=0`) and final (`ramp(1)=1`) values are preserved
    ///   (`Σ Aᵢ = 1`);
    /// - at least one interior command departs from the unshaped ramp.
    #[test]
    fn input_shape_value_impulse_arm_shapes_ramp() {
        let ramp = ramp_profile();
        let zv = shaper(10.0); // ZVShaper(10 Hz, ζ=0)

        // The impl marshals the profile to this same spline; reconstruct it here
        // to certify the convolution identity (and the grid mapping) independently.
        let spline = value_to_multijoint_spline(&ramp).expect("ramp marshals to a spline");
        let t_domain = spline.duration();
        let train = build_train_for_shaper(&zv).expect("ZVShaper → train");
        let delta = train.trailing_time();
        assert!(delta > 0.0, "ZV has a nonzero shaper delay Δ");

        let shaped = input_shape_value(&ramp, &zv);

        // Type + field preservation: only waypoints change.
        let Value::StructureInstance(data) = &shaped else {
            panic!("input_shape_value must yield a Profile StructureInstance, got {shaped:?}");
        };
        assert_eq!(
            data.type_name, "PiecewisePolynomialProfile",
            "real shaping preserves the Profile type_name"
        );
        assert_eq!(
            data.fields.get(&"mechanism".to_string()),
            Some(&Value::Real(0.0)),
            "mechanism preserved"
        );
        match data.fields.get(&"boundary".to_string()) {
            Some(Value::StructureInstance(b)) => {
                assert_eq!(b.type_name, "NaturalSpline", "boundary preserved")
            }
            other => panic!("boundary must be preserved as a StructureInstance, got {other:?}"),
        }
        match data.fields.get(&"spline_kind".to_string()) {
            Some(Value::Enum { variant, .. }) => {
                assert_eq!(variant, "CubicSpline", "spline_kind preserved")
            }
            other => panic!("spline_kind must be preserved as an Enum, got {other:?}"),
        }

        // Waypoints: genuinely re-sampled over the Δ-extended grid.
        let out = read_waypoints(&shaped);
        assert!(out.len() >= 2, "a shaped profile has ≥2 waypoints");
        let first_t = out.first().unwrap().0;
        let last_t = out.last().unwrap().0;
        assert!((first_t - 0.0).abs() < 1e-9, "shaped grid starts at 0");
        assert!(
            ((last_t - first_t) - (t_domain + delta)).abs() < 1e-9,
            "total shaped duration == original ({t_domain}) + trailing_time ({delta}), got {}",
            last_t - first_t
        );

        // Convolution identity at every output waypoint time.
        let mut max_dev_from_ramp = 0.0_f64;
        for (t, vals) in &out {
            assert_eq!(vals.len(), 1, "one joint");
            let want = convolve_at(&train, &|tau: f64| spline.eval(tau)[0], t_domain, *t);
            assert!(
                (vals[0] - want).abs() < 1e-9,
                "shaped command at t={t:.5} must equal the convolve_at reconstruction: \
                 got {}, want {want}",
                vals[0]
            );
            if *t <= t_domain {
                max_dev_from_ramp = max_dev_from_ramp.max((vals[0] - spline.eval(*t)[0]).abs());
            }
        }

        // Start + final-value preservation (Σ Aᵢ = 1) — independent of the loop above.
        assert!(
            out.first().unwrap().1[0].abs() < 1e-9,
            "shaped start value preserved (ramp(0) = 0)"
        );
        assert!(
            (out.last().unwrap().1[0] - 1.0).abs() < 1e-9,
            "shaped final value preserved (ramp(1) = 1), got {}",
            out.last().unwrap().1[0]
        );
        // Genuinely shaped: the interior command departs from the raw ramp.
        assert!(
            max_dev_from_ramp > 1e-3,
            "the ZV-shaped command must differ from the unshaped ramp (peak Δ={max_dev_from_ramp:.3e})"
        );
    }

    /// An unrecognised shaper — and non-`StructureInstance` args — yield
    /// `Value::Undef` (the bad-args convention; the impulse arm requires a
    /// resolvable train).
    #[test]
    fn input_shape_value_unknown_shaper_is_undef() {
        let ramp = ramp_profile();
        let unknown = instance("FooShaper", vec![]);
        assert!(
            matches!(input_shape_value(&ramp, &unknown), Value::Undef),
            "an unrecognised shaper → Undef"
        );
        assert!(
            matches!(input_shape_value(&Value::Real(0.0), &shaper(10.0)), Value::Undef),
            "a non-instance profile → Undef"
        );
        assert!(
            matches!(input_shape_value(&ramp, &Value::Real(0.0)), Value::Undef),
            "a non-instance shaper → Undef"
        );
    }

    // ── steps 17/18: input_shape_value (TOTS arm) ────────────────────────────────

    /// A `TOTSShaper` `Value::StructureInstance` carrying the readable scalar
    /// limit/solver fields plus a `modes : List<Mode>` modal model. The TOTS arm
    /// of `input_shape_value` marshals the input profile's waypoints into the
    /// per-joint P2P spec, these limits into the per-joint vel/acc constraints +
    /// `vib_tol`, and `max_iters`/`tol` into the `SqpConfig`. `actuator_limits` is
    /// deliberately omitted here — this exercises the scalar
    /// `velocity_limit`/`acceleration_limit` fallback path.
    fn tots_shaper(
        velocity_limit: f64,
        acceleration_limit: f64,
        vibration_tolerance: f64,
        modes: Vec<Value>,
    ) -> Value {
        instance(
            "TOTSShaper",
            vec![
                ("velocity_limit".to_string(), Value::Real(velocity_limit)),
                (
                    "acceleration_limit".to_string(),
                    Value::Real(acceleration_limit),
                ),
                (
                    "vibration_tolerance".to_string(),
                    Value::Real(vibration_tolerance),
                ),
                ("max_iters".to_string(), Value::Int(100)),
                ("tol".to_string(), Value::Real(1e-6)),
                ("modes".to_string(), Value::List(modes)),
            ],
        )
    }

    /// A single-joint point-to-point ramp `q : start → end` over `[0, duration]`
    /// as a natural-cubic `PiecewisePolynomialProfile` with a midpoint interior
    /// waypoint (so the marshalled `JointWaypoints` has exactly one optimisable
    /// interior). `duration` is the un-optimised baseline the time-optimal solve
    /// improves on.
    fn p2p_profile(duration: f64, start: f64, end: f64) -> Value {
        let mid = 0.5 * (start + end);
        pp_profile(
            vec![
                waypoint(0.0, &[start], None, None),
                waypoint(duration / 2.0, &[mid], None, None),
                waypoint(duration, &[end], None, None),
            ],
            instance("NaturalSpline", vec![]),
            spline_kind("CubicSpline"),
        )
    }

    /// (TOTS arm) A slow (baseline `T = 5 s`) unit P2P ramp shaped by a feasible
    /// `TOTSShaper` (slack vel/acc/vib limits) becomes a genuinely re-timed,
    /// time-optimal `PiecewisePolynomialProfile`:
    /// - `type_name` / `mechanism` / `boundary` / `spline_kind` preserved (only
    ///   `waypoints` change, mirroring the impulse arm);
    /// - the move endpoints are fixed (start = 0, end = 1);
    /// - the shaped duration is strictly faster than the slow baseline
    ///   (`T_opt < T_base`) — time-optimal shaping never returns a slower move,
    ///   and the constraint-limited optimum sits far below the baseline so the
    ///   improvement is real (not an echo).
    #[test]
    fn input_shape_value_tots_arm_shapes_profile() {
        let t_base = 5.0_f64;
        let base_profile = p2p_profile(t_base, 0.0, 1.0);
        let tots = tots_shaper(5.0, 50.0, 1.0, vec![mode(10.0, 0.05, &[[1.0, 0.0, 0.0]])]);

        let shaped = input_shape_value(&base_profile, &tots);

        // Real shaping → a Profile StructureInstance (NOT Undef), type preserved.
        let Value::StructureInstance(data) = &shaped else {
            panic!("a feasible TOTSShaper must yield a Profile StructureInstance, got {shaped:?}");
        };
        assert_eq!(
            data.type_name, "PiecewisePolynomialProfile",
            "TOTS shaping preserves the Profile type_name"
        );
        assert_eq!(
            data.fields.get(&"mechanism".to_string()),
            Some(&Value::Real(0.0)),
            "mechanism preserved"
        );
        match data.fields.get(&"boundary".to_string()) {
            Some(Value::StructureInstance(b)) => {
                assert_eq!(b.type_name, "NaturalSpline", "boundary preserved")
            }
            other => panic!("boundary must be preserved as a StructureInstance, got {other:?}"),
        }
        match data.fields.get(&"spline_kind".to_string()) {
            Some(Value::Enum { variant, .. }) => {
                assert_eq!(variant, "CubicSpline", "spline_kind preserved")
            }
            other => panic!("spline_kind must be preserved as an Enum, got {other:?}"),
        }

        // Endpoints fixed; duration strictly faster than the slow baseline.
        let out = read_waypoints(&shaped);
        assert!(out.len() >= 2, "a shaped profile has ≥2 waypoints");
        let first = out.first().unwrap();
        let last = out.last().unwrap();
        assert!((first.0 - 0.0).abs() < 1e-9, "shaped grid starts at t=0");
        assert!(
            first.1[0].abs() < 1e-6,
            "start position fixed at 0, got {}",
            first.1[0]
        );
        assert!(
            (last.1[0] - 1.0).abs() < 1e-6,
            "end position fixed at 1, got {}",
            last.1[0]
        );
        let t_opt = last.0 - first.0;
        assert!(
            t_opt.is_finite() && t_opt > 0.0,
            "shaped duration must be finite & positive, got {t_opt}"
        );
        assert!(
            t_opt < t_base,
            "time-optimal shaped duration {t_opt:.4} must be faster than the baseline {t_base}"
        );
    }

    /// (TOTS arm) An infeasible `TOTSShaper` — `velocity_limit = 0` on a nonzero
    /// move — makes `solve_tots` early-exit `ConstraintInfeasible`; the TOTS arm
    /// maps that to `Value::Undef` (no feasible shaped profile exists), never a
    /// panic. (`velocity_limit = 0` is constructible directly as a
    /// `StructureInstance`, bypassing the `.ri` `velocity_limit > 0` ctor guard.)
    #[test]
    fn input_shape_value_tots_arm_infeasible_is_undef() {
        let base_profile = p2p_profile(5.0, 0.0, 1.0);
        let infeasible = tots_shaper(0.0, 50.0, 1.0, vec![mode(10.0, 0.05, &[[1.0, 0.0, 0.0]])]);
        assert!(
            matches!(input_shape_value(&base_profile, &infeasible), Value::Undef),
            "an infeasible TOTSShaper (velocity_limit=0) → Undef (ConstraintInfeasible)"
        );
    }

    // ── steps 19/20: accessor intrinsics (eval_builtin boundary) ─────────────────
    //
    // The three EndEffectorTrack accessors are reached through `eval_builtin` on
    // their *_at intrinsic names (the delegate the trajectory.ri bodies call in
    // step-22), so these tests exercise the full `eval_builtin` →
    // `eval_trajectory` → trampoline routing rather than the impls directly.

    use crate::eval_builtin;

    /// Read a `Value::List<Real>` into `Vec<f64>` (the accessor list-return
    /// shape). Panics on any other variant — in RED the unrouted names return
    /// `Value::Undef`, so this panics and the test fails (the intended RED
    /// signal); in GREEN the impls return real `List<Real>` values.
    fn list_reals(v: &Value) -> Vec<f64> {
        match v {
            Value::List(items) => items.iter().map(as_real).collect(),
            other => panic!("expected a List<Real> from an accessor, got {other:?}"),
        }
    }

    /// (a) Over a populated 2-location × 3-time track (combined Z = 10·loc + t,
    /// nominal Z = 0), the three accessor intrinsics — reached via `eval_builtin`
    /// on their `*_at` names — read the per-location series:
    /// - `end_effector_track_at` → the combined-pose Z column (len == t_samples);
    /// - `deviation_from_nominal_at` → per-time |combined − nominal| (len ==
    ///   t_samples), which equals the |vibration| marker;
    /// - `peak_deviation_at` → the max of that deviation series.
    #[test]
    fn accessor_intrinsics_read_combined_and_deviation_per_location() {
        let (n_loc, n_times) = (2, 3);
        let track = track_data_to_value(&populated_track(n_loc, n_times));

        for loc in 0..n_loc {
            let loc_v = Value::Real(loc as f64);

            // end_effector_track_at → the combined-pose Z column [10·loc + t].
            let combined = list_reals(&eval_builtin(
                "end_effector_track_at",
                &[track.clone(), loc_v.clone()],
            ));
            assert_eq!(
                combined.len(),
                n_times,
                "end_effector_track_at column len == t_samples"
            );
            for (t, &got) in combined.iter().enumerate() {
                let want = (10 * loc + t) as f64;
                assert!(
                    (got - want).abs() < SPLINE_TOL,
                    "end_effector_track_at[{loc}][{t}] = {got} want {want}",
                );
            }

            // deviation_from_nominal_at → |combined − nominal| = the same marker.
            let dev = list_reals(&eval_builtin(
                "deviation_from_nominal_at",
                &[track.clone(), loc_v.clone()],
            ));
            assert_eq!(dev.len(), n_times, "deviation_from_nominal_at len == t_samples");
            for (t, &got) in dev.iter().enumerate() {
                let want = (10 * loc + t) as f64;
                assert!(
                    (got - want).abs() < SPLINE_TOL,
                    "deviation_from_nominal_at[{loc}][{t}] = {got} want {want}",
                );
            }

            // peak_deviation_at → max over time of the deviation series.
            let peak = as_real(&eval_builtin("peak_deviation_at", &[track.clone(), loc_v]));
            let want_peak = (10 * loc + (n_times - 1)) as f64;
            assert!(
                (peak - want_peak).abs() < SPLINE_TOL,
                "peak_deviation_at[{loc}] = {peak} want {want_peak}"
            );
        }
    }

    /// (b) An out-of-range location yields an empty series / zero peak — never a
    /// panic (the index is past the track's location count).
    #[test]
    fn accessor_intrinsics_out_of_range_location_is_empty_or_zero() {
        let track = track_data_to_value(&populated_track(2, 3));
        let oob = Value::Real(5.0); // only locations 0, 1 exist

        assert!(
            list_reals(&eval_builtin("end_effector_track_at", &[track.clone(), oob.clone()]))
                .is_empty(),
            "out-of-range end_effector_track_at → empty list"
        );
        assert!(
            list_reals(&eval_builtin("deviation_from_nominal_at", &[track.clone(), oob.clone()]))
                .is_empty(),
            "out-of-range deviation_from_nominal_at → empty list"
        );
        assert!(
            as_real(&eval_builtin("peak_deviation_at", &[track, oob])).abs() < SPLINE_TOL,
            "out-of-range peak_deviation_at → 0"
        );
    }

    /// (c) A malformed track (not even a StructureInstance) yields an empty
    /// series / zero peak — never a panic.
    #[test]
    fn accessor_intrinsics_malformed_track_is_empty_or_zero() {
        let bad = Value::Real(0.0);
        let loc = Value::Real(0.0);

        assert!(
            list_reals(&eval_builtin("end_effector_track_at", &[bad.clone(), loc.clone()]))
                .is_empty(),
            "malformed end_effector_track_at → empty list"
        );
        assert!(
            list_reals(&eval_builtin("deviation_from_nominal_at", &[bad.clone(), loc.clone()]))
                .is_empty(),
            "malformed deviation_from_nominal_at → empty list"
        );
        assert!(
            as_real(&eval_builtin("peak_deviation_at", &[bad, loc])).abs() < SPLINE_TOL,
            "malformed peak_deviation_at → 0"
        );
    }

    // ── step-1 (task 4539): evaluate_profile position eval-boundary ─────────────

    use super::super::test_polynomials::{cubic_p, cubic_dp, cubic_ddp};

    /// Build a clamped-cubic `PiecewisePolynomialProfile` `Value` at 4 knots
    /// ts = [0.0, 1.0, 2.5, 4.0] with values = cubic_p(t) and endpoint slopes
    /// equal to the exact cubic derivative (cubic_dp(0) / cubic_dp(4)). A
    /// clamped cubic whose boundary slopes equal the exact cubic derivatives
    /// reproduces p, p', p'' exactly (spline.rs:1187 tolerance 1e-12).
    fn clamped_cubic_profile() -> Value {
        let ts = [0.0_f64, 1.0, 2.5, 4.0];
        let wps: Vec<Value> = ts
            .iter()
            .map(|&t| waypoint(t, &[cubic_p(t)], None, None))
            .collect();
        let boundary = instance(
            "ClampedSpline",
            vec![
                ("start_velocity".to_string(), reals(&[cubic_dp(0.0)])),
                ("end_velocity".to_string(), reals(&[cubic_dp(4.0)])),
            ],
        );
        pp_profile(wps, boundary, spline_kind("CubicSpline"))
    }

    /// `evaluate_profile(profile, t)` returns `Value::List([Value::Real(q)])` with
    /// `q` within `SPLINE_TOL` of `cubic_p(t)` for all sampled `t` in `[0, 4]`.
    /// The result must NOT be `[0.0]` (the stub value that the unwired
    /// `.ri` body returned before task 4539).
    #[test]
    fn evaluate_profile_position_eval_boundary() {
        let profile = clamped_cubic_profile();
        let sample_ts = [0.0_f64, 0.5, 1.0, 2.5, 3.7, 4.0];

        for t in sample_ts {
            let result = eval_builtin("evaluate_profile", &[profile.clone(), time(t)]);
            let Value::List(items) = &result else {
                panic!(
                    "evaluate_profile(t={t}) should return Value::List, got {result:?}"
                );
            };
            assert_eq!(
                items.len(),
                1,
                "evaluate_profile(t={t}): expected 1-element list (one joint), got {}",
                items.len()
            );
            let Value::Real(q) = items[0] else {
                panic!(
                    "evaluate_profile(t={t}): list element should be Value::Real, got {:?}",
                    items[0]
                );
            };
            let expected = cubic_p(t);
            assert!(
                (q - expected).abs() < SPLINE_TOL,
                "evaluate_profile(t={t}): got {q}, want {expected} (diff {})",
                (q - expected).abs()
            );
            assert!(
                q.abs() > SPLINE_TOL || expected.abs() < SPLINE_TOL,
                "evaluate_profile(t={t}): result is [0.0] — the stub body is still live"
            );
        }
    }

    // ── step-3 (task 4539): evaluate_profile_dot / _ddot eval-boundary ──────────

    /// Helper: extract the single real from a `Value::List([Value::Real(r)])`.
    fn single_real(v: &Value, ctx: &str) -> f64 {
        let Value::List(items) = v else {
            panic!("{ctx}: expected Value::List, got {v:?}");
        };
        assert_eq!(items.len(), 1, "{ctx}: expected 1-element list, got {}", items.len());
        let Value::Real(r) = items[0] else {
            panic!("{ctx}: list element should be Value::Real, got {:?}", items[0]);
        };
        r
    }

    /// `evaluate_profile_dot(profile, t)` returns `[cubic_dp(t)]` within
    /// `SPLINE_TOL`. A clamped cubic whose endpoint slopes equal the exact
    /// cubic first-derivative reproduces p' exactly (spline.rs:1187).
    /// RED because `eval_trajectory` still returns `Some(Value::Undef)` for
    /// `"evaluate_profile_dot"`.
    #[test]
    fn evaluate_profile_dot_eval_boundary() {
        let profile = clamped_cubic_profile();
        let sample_ts = [0.0_f64, 0.5, 1.0, 2.5, 3.7, 4.0];

        for t in sample_ts {
            let result = eval_builtin("evaluate_profile_dot", &[profile.clone(), time(t)]);
            let got = single_real(&result, &format!("evaluate_profile_dot(t={t})"));
            let expected = cubic_dp(t);
            assert!(
                (got - expected).abs() < SPLINE_TOL,
                "evaluate_profile_dot(t={t}): got {got}, want {expected} (diff {})",
                (got - expected).abs()
            );
        }
    }

    /// `evaluate_profile_ddot(profile, t)` returns `[cubic_ddp(t)]` within
    /// `SPLINE_TOL`. A clamped cubic reproduces p'' exactly.
    #[test]
    fn evaluate_profile_ddot_eval_boundary() {
        let profile = clamped_cubic_profile();
        let sample_ts = [0.0_f64, 0.5, 1.0, 2.5, 3.7, 4.0];

        for t in sample_ts {
            let result = eval_builtin("evaluate_profile_ddot", &[profile.clone(), time(t)]);
            let got = single_real(&result, &format!("evaluate_profile_ddot(t={t})"));
            let expected = cubic_ddp(t);
            assert!(
                (got - expected).abs() < SPLINE_TOL,
                "evaluate_profile_ddot(t={t}): got {got}, want {expected} (diff {})",
                (got - expected).abs()
            );
        }
    }

    // ── step-5 (task 4539): profile_duration + loud-failure on bad input ─────────

    /// `profile_duration(profile)` returns a `Value::Scalar{TIME}` whose
    /// `si_value` equals the knot span of `clamped_cubic_profile()` (0..4 = 4s).
    /// RED because `eval_trajectory` still returns `Some(Value::Undef)` for
    /// `"profile_duration"`.
    #[test]
    fn profile_duration_eval_boundary() {
        let profile = clamped_cubic_profile();
        let result = eval_builtin("profile_duration", &[profile]);
        let Value::Scalar { si_value, dimension } = result else {
            panic!("profile_duration should return Value::Scalar, got {result:?}");
        };
        assert_eq!(
            dimension,
            reify_core::DimensionVector::TIME,
            "profile_duration should carry TIME dimension"
        );
        assert!(
            (si_value - 4.0).abs() < SPLINE_TOL,
            "profile_duration: got {si_value}s, want 4.0s"
        );
    }

    /// Bad inputs yield `Value::Undef` for all four public names — the loud
    /// not-computed signal, not a numeric placeholder. Tested vectors:
    /// - wrong arity (0 args for duration; 1 / 3 args for the position family)
    /// - a non-`StructureInstance` first arg (`Value::Real(1.0)`)
    /// - a degenerate `<2`-waypoint profile
    #[test]
    fn evaluate_profile_family_bad_args_return_undef() {
        let good_t = time(0.5);
        let bad_profile = Value::Real(1.0);
        let degenerate_profile = pp_profile(
            vec![waypoint(0.0, &[0.0], None, None)],
            instance("NaturalSpline", vec![]),
            spline_kind("CubicSpline"),
        );

        // profile_duration — wrong arity
        assert!(eval_builtin("profile_duration", &[]).is_undef(), "duration: 0 args");
        assert!(
            eval_builtin("profile_duration", &[bad_profile.clone(), good_t.clone()]).is_undef(),
            "duration: 2 args (wrong arity)"
        );
        // profile_duration — bad / degenerate profile
        assert!(
            eval_builtin("profile_duration", std::slice::from_ref(&bad_profile)).is_undef(),
            "duration: non-StructureInstance profile"
        );
        assert!(
            eval_builtin("profile_duration", std::slice::from_ref(&degenerate_profile))
                .is_undef(),
            "duration: <2-waypoint profile"
        );

        // evaluate_profile — wrong arity
        assert!(eval_builtin("evaluate_profile", &[]).is_undef(), "eval_profile: 0 args");
        assert!(
            eval_builtin("evaluate_profile", std::slice::from_ref(&bad_profile)).is_undef(),
            "eval_profile: 1 arg"
        );
        // evaluate_profile — bad profile
        assert!(
            eval_builtin("evaluate_profile", &[bad_profile.clone(), good_t.clone()]).is_undef(),
            "eval_profile: non-StructureInstance"
        );
        assert!(
            eval_builtin("evaluate_profile", &[degenerate_profile.clone(), good_t.clone()])
                .is_undef(),
            "eval_profile: <2-waypoint"
        );

        // evaluate_profile_dot — bad profile
        assert!(
            eval_builtin("evaluate_profile_dot", &[bad_profile.clone(), good_t.clone()]).is_undef(),
            "eval_dot: non-StructureInstance"
        );
        assert!(
            eval_builtin(
                "evaluate_profile_dot",
                &[degenerate_profile.clone(), good_t.clone()]
            )
            .is_undef(),
            "eval_dot: <2-waypoint"
        );

        // evaluate_profile_ddot — bad profile
        assert!(
            eval_builtin("evaluate_profile_ddot", &[bad_profile.clone(), good_t.clone()])
                .is_undef(),
            "eval_ddot: non-StructureInstance"
        );
        assert!(
            eval_builtin(
                "evaluate_profile_ddot",
                &[degenerate_profile.clone(), good_t.clone()]
            )
            .is_undef(),
            "eval_ddot: <2-waypoint"
        );
    }
}
