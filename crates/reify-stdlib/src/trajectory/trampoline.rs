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

use reify_core::ContentHash;
use reify_ir::Value;

use super::simulate::{ModalModel, ModeDesc};
use super::spline::{BoundaryCondition, CubicSpline, KnotData, MultiJointSpline, QuinticSpline};

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
//
// The deferred β `Value`→`MultiJointSpline` marshalling (and, in later steps,
// the modal / mechanism / track marshalling). These helpers are `pub(crate)`
// internal — `reify-eval` reaches the trajectory core only through the
// `simulate_trajectory_value` / `input_shape_value` composers (steps 14 / 16),
// which call these. Tagged `#[allow(dead_code)]` until those composers consume
// them, mirroring `spline.rs`'s own dead-code suppression (the spline math is
// fully tested but its Value wiring lands incrementally across π's TDD steps).

/// Read a numeric stdlib field as `f64` — a dimensioned `Scalar` (SI magnitude),
/// a `Real`, or an `Int`. Any other variant yields `None`. Mirrors
/// `input_shape::read_scalar_si` / `modal_ops::read_scalar_si`.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
}
