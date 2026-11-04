//! Integration tests for the `input_shape` ComputeNode trampoline
//! (task π, steps 27–28): warm-state cache (HIT / MISS), cooperative
//! cancellation, and a smoke check that both the TOTS and impulse arms
//! route through the same registered target.
//!
//! Mirrors `tests/simulate_trajectory_compute_node.rs` and
//! `tests/modal_compute_node.rs`. Both the TOTS (heavy, cache-valuable) and
//! impulse ZV/ZVD/EI/Cascaded (cheap real shaping) arms route through
//! `"trajectory::input_shape"` — the same registered trampoline branches
//! internally.
//!
//! Observable signals:
//!
//! - **(a) TOTS COMPLETED → shaped Profile + Final VC** — a fresh TOTS dispatch
//!   returns a `Value::StructureInstance` with `type_name == "PiecewisePolynomialProfile"`
//!   and flips the output VC to `Freshness::Final`.
//!
//! - **(b) COMPLETED DONATES WARM STATE** — the compute node carries a non-`None`
//!   `warm_state` under `NodeId::Compute(c_id)` after a completed dispatch.
//!
//! - **(c) SECOND DISPATCH IS A CACHE HIT** — identical `(profile, shaper)` reuses
//!   the donated cache and returns a valid shaped `PiecewisePolynomialProfile`.
//!
//! - **(d) CHANGED PROFILE → CACHE MISS** — a dispatch after mutating one profile
//!   control point forces a MISS (recompute). The new result is still a valid Profile.
//!
//! - **(e) PRE-CANCELLED → Err(DispatchError::Cancelled), VC stays Pending.**
//!
//! - **(f) ZV IMPULSE ARM SMOKE** — a ZVShaper dispatch returns a
//!   `PiecewisePolynomialProfile` (the impulse arm routes through the same target).
//!
//! RED until step-28 registers `"trajectory::input_shape"` in
//! `compute_targets::register_compute_fns`. (Step-24 already implemented the
//! trampoline body; step-28 adds the registration line.)
//!
//! Task 6941 ζ (PRD `docs/prds/v0_6/dimension-checked-readers.md`) adds the
//! reader signals, driven straight through the trampoline: a present field of
//! the wrong dimension (`target_frequency`, the TOTS limits, a JointLimit's
//! `max_force`, a waypoint's `t`) is `ComputeOutcome::Failed` carrying α's one
//! `DimensionedArgRejected` Error, and the TOTS force bound is read from
//! `actuator_limits` — a joint no limit covers is refused, never solved
//! against a default.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::f64::consts::PI;

use reify_core::{
    ComputeNodeId, ContentHash, DiagnosticCode, DimensionVector, Severity, ValueCellId, VersionId,
};
use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::trajectory_ops::input_shape_trampoline;
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, DispatchError};
use reify_ir::arg_acceptance::{
    Acceptance, ArgSpec, FieldRejection, accept_arg, angular_velocity_spec, dimensionless_spec,
    force_spec, frequency_spec, time_spec, velocity_spec,
};
use reify_ir::{
    DeterminacyState, Freshness, PersistentMap, StructureInstanceData, StructureTypeId, Value,
};
use reify_stdlib::impulse_shaper::ImpulseTrain;
use reify_stdlib::{build_train_for_shaper, shaper_damping_ratio};
use reify_test_support::make_simple_engine;

// ── Value fixture helpers ─────────────────────────────────────────────────────

/// Build a registry-free `Value::StructureInstance`.
fn struct_instance(type_name: &str, fields: Vec<(&str, Value)>) -> Value {
    let fields: PersistentMap<String, Value> = fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: type_name.to_string(),
        version: 1,
        fields,
    }))
}

/// `instance` with field `key` set to `value` (inserted or replaced).
fn with_field(instance: Value, key: &str, value: Value) -> Value {
    let Value::StructureInstance(mut data) = instance else {
        panic!("with_field: {key} must be set on a StructureInstance");
    };
    data.fields.insert(key.to_string(), value);
    Value::StructureInstance(data)
}

/// `instance` with field `key` removed.
fn without_field(instance: Value, key: &str) -> Value {
    let Value::StructureInstance(mut data) = instance else {
        panic!("without_field: {key} must be removed from a StructureInstance");
    };
    data.fields.remove(&key.to_string());
    Value::StructureInstance(data)
}

/// A dimensioned scalar `Value`.
fn scalar(si_value: f64, dimension: DimensionVector) -> Value {
    Value::Scalar {
        si_value,
        dimension,
    }
}

/// A `Time` scalar `Value` (SI seconds).
fn time_scalar(s: f64) -> Value {
    scalar(s, DimensionVector::TIME)
}

/// A `List<Real>` `Value`.
fn reals(vs: &[f64]) -> Value {
    Value::List(vs.iter().map(|&v| Value::Real(v)).collect())
}

/// Optional `List<Real>` (None for unset vels/accels).
fn no_opt() -> Value {
    Value::Option(None)
}

/// A `Waypoint` StructureInstance at `t` seconds.
fn waypoint(t: f64, values: &[f64]) -> Value {
    waypoint_at(time_scalar(t), values)
}

/// A `Waypoint` StructureInstance whose `t` field is exactly `t`.
fn waypoint_at(t: Value, values: &[f64]) -> Value {
    struct_instance(
        "Waypoint",
        vec![
            ("t", t),
            ("values", reals(values)),
            ("vels", no_opt()),
            ("accels", no_opt()),
        ],
    )
}

/// `SplineKind::CubicSpline` enum Value.
fn cubic_spline_kind() -> Value {
    Value::Enum {
        type_name: "SplineKind".to_string(),
        variant: "CubicSpline".to_string(),
        payload: vec![],
    }
}

/// A `PiecewisePolynomialProfile` over `waypoints`: natural cubic spline on a
/// `Real` mechanism placeholder.
fn pp_profile(waypoints: Vec<Value>) -> Value {
    struct_instance(
        "PiecewisePolynomialProfile",
        vec![
            ("mechanism", Value::Real(1.0)),
            ("waypoints", Value::List(waypoints)),
            ("boundary", struct_instance("NaturalSpline", vec![])),
            ("spline_kind", cubic_spline_kind()),
        ],
    )
}

/// A `PiecewisePolynomialProfile` with 3 waypoints: start, mid, end — so the
/// TOTS solver has one interior waypoint to optimise. `mid_val` is the midpoint
/// q value; changing it exercises cache MISS.
fn p2p_profile(duration: f64, start: f64, end: f64, mid_val: f64) -> Value {
    pp_profile(vec![
        waypoint(0.0, &[start]),
        waypoint(duration / 2.0, &[mid_val]),
        waypoint(duration, &[end]),
    ])
}

/// The 2-waypoint unit ramp the impulse arm shapes (ZV needs no interior
/// waypoint).
fn ramp_profile() -> Value {
    pp_profile(vec![waypoint(0.0, &[0.0]), waypoint(1.0, &[1.0])])
}

/// A single-mode `Mode` Value (used by TOTSShaper).
fn mode(freq_hz: f64, zeta: f64, shape: [f64; 3]) -> Value {
    struct_instance(
        "Mode",
        vec![
            ("frequency", Value::Real(freq_hz)),
            ("damping_ratio", Value::Real(zeta)),
            (
                "shape",
                Value::List(vec![Value::Vector(
                    shape.iter().map(|&x| Value::Real(x)).collect(),
                )]),
            ),
        ],
    )
}

/// An actuator limit (`JointLimit` / `RevoluteJointLimit`) on profile joint
/// `joint` (a dimensionless index, `param joint : Real`).
fn joint_limit(type_name: &str, joint: f64, max_force: Value) -> Value {
    struct_instance(
        type_name,
        vec![("joint", Value::Real(joint)), ("max_force", max_force)],
    )
}

/// The fields one TOTS-family shaper carries, each limit spelled in its
/// declared dimension, with `vibration_tolerance = 1.0` (very slack), one mode
/// and one actuator limit on joint 0.
fn tots_shaper_of_kind(
    type_name: &str,
    velocity_limit: Value,
    acceleration_limit: Value,
    actuator_limit: Value,
) -> Value {
    struct_instance(
        type_name,
        vec![
            ("velocity_limit", velocity_limit),
            ("acceleration_limit", acceleration_limit),
            ("vibration_tolerance", Value::Real(1.0)),
            ("max_iters", Value::Int(100)),
            ("tol", Value::Real(1e-6)),
            (
                "modes",
                Value::List(vec![mode(10.0, 0.05, [1.0, 0.0, 0.0])]),
            ),
            ("actuator_limits", Value::List(vec![actuator_limit])),
        ],
    )
}

/// A linear `TOTSShaper` (5 m/s, 50 m/s²) whose joint-0 `JointLimit` allows
/// `max_force_n` newtons — feasible for a 0→1 unit ramp with a 5 s baseline.
fn tots_shaper_with_force_limit(max_force_n: f64) -> Value {
    tots_shaper_of_kind(
        "TOTSShaper",
        scalar(5.0, DimensionVector::VELOCITY),
        scalar(50.0, DimensionVector::ACCELERATION),
        joint_limit(
            "JointLimit",
            0.0,
            scalar(max_force_n, DimensionVector::FORCE),
        ),
    )
}

/// A `TOTSShaper` StructureInstance with slack vel/acc/vib limits, one mode and
/// a non-binding 1000 N actuator limit on joint 0.
fn tots_shaper() -> Value {
    tots_shaper_with_force_limit(1000.0)
}

/// A `RevoluteTOTSShaper` (5 rad/s, 50 rad/s²) whose joint-0
/// `RevoluteJointLimit` allows 1000 N·m.
fn revolute_tots_shaper() -> Value {
    tots_shaper_of_kind(
        "RevoluteTOTSShaper",
        scalar(5.0, DimensionVector::ANGULAR_VELOCITY),
        scalar(50.0, angular_acceleration()),
        joint_limit(
            "RevoluteJointLimit",
            0.0,
            scalar(1000.0, DimensionVector::TORQUE),
        ),
    )
}

/// rad·s⁻², composed as the `.ri` decl composes `Rate<AngularVelocity>` (no
/// `DimensionVector::ANGULAR_ACCELERATION` registry const exists).
fn angular_acceleration() -> DimensionVector {
    DimensionVector::ANGULAR_VELOCITY.div(&DimensionVector::TIME)
}

/// An impulse-family shaper (`ZVShaper` / `ZVDShaper` / `EIShaper`) with the
/// given `target_frequency` value and ζ = 0.
fn impulse_shaper(type_name: &str, target_frequency: Value) -> Value {
    struct_instance(
        type_name,
        vec![
            ("target_frequency", target_frequency),
            ("damping_ratio", Value::Real(0.0)),
            ("vibration_tolerance", Value::Real(0.05)),
        ],
    )
}

/// A `ZVShaper` StructureInstance (impulse arm, cheap path).
fn zv_shaper(freq_hz: f64, zeta: f64) -> Value {
    struct_instance(
        "ZVShaper",
        vec![
            (
                "target_frequency",
                scalar(freq_hz, DimensionVector::FREQUENCY),
            ),
            ("damping_ratio", Value::Real(zeta)),
        ],
    )
}

/// A 2-input `[profile, shaper]` flat list for a TOTS dispatch. `mid_val`
/// controls the interior waypoint so callers can force a cache MISS.
fn tots_value_inputs(mid_val: f64) -> Vec<Value> {
    vec![p2p_profile(5.0, 0.0, 1.0, mid_val), tots_shaper()]
}

/// A 2-input `[profile, shaper]` flat list for a ZV dispatch.
fn zv_value_inputs() -> Vec<Value> {
    vec![ramp_profile(), zv_shaper(10.0, 0.0)]
}

// ── Engine setup ──────────────────────────────────────────────────────────────

/// Register the `input_shape_trampoline` under its production target on a fresh
/// engine. Uses the full `register_compute_fns` (which includes both trajectory
/// targets) to exercise the production registration path.
///
/// Each test owns its engine (the registry panics on duplicate targets, and
/// `make_simple_engine` does NOT auto-register compute fns — design-decision-10).
fn engine_with_input_shape_target() -> reify_eval::Engine {
    let mut engine = make_simple_engine();
    engine.register_compute_fn(
        "trajectory::input_shape",
        reify_eval::trajectory_ops::input_shape_trampoline as ComputeFn,
    );
    engine
}

/// Seed a Final output VC so `begin_compute_dispatch` has a `last_substantive`
/// to keep on display when a dispatch is cancelled.
fn seed_final_output(engine: &mut reify_eval::Engine, cell: &ValueCellId) {
    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
            Freshness::Final,
            reify_eval::deps::DependencyTrace::default(),
            VersionId(1),
        ),
    );
}

// ── (a) + (b) + (c) TOTS COMPLETED → shaped Profile, warm state, HIT ─────────

/// A fresh TOTS input_shape dispatch must:
///
///   (a) return a `PiecewisePolynomialProfile` StructureInstance and flip VC to Final;
///   (b) donate non-None warm state under `NodeId::Compute(c_id)`;
///   (c) a second dispatch with identical `(profile, shaper)` is a cache HIT.
///
/// RED until step-28: `"trajectory::input_shape"` is not registered until then.
///
/// Gated `release-only` — the TOTS solve takes ~8–10 s in debug (argmin
/// optimisation + cubic-spline evaluation in unoptimised code).  Matches the
/// buckling/modal pattern (`#[cfg_attr(debug_assertions, ignore = …)]`).
#[test]
#[cfg_attr(debug_assertions, ignore = "heavy TOTS solve; release-only")]
fn input_shape_tots_completed_donates_warm_state_then_reuses() {
    let mut engine = engine_with_input_shape_target();

    let cell = ValueCellId::new("InputShapeFixture", "result");
    let c_id = ComputeNodeId::new("InputShapeFixture", 0);
    seed_final_output(&mut engine, &cell);

    // ── (a) + (b) first fresh dispatch → Completed, VC Final, warm state donated
    let inputs = tots_value_inputs(0.5);
    let handle = CancellationHandle::new();
    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::input_shape",
        &inputs,
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
        ContentHash(0), // inert: no cache dir in tests
    );
    let (value, _diags, _) = result.expect("fresh input_shape TOTS dispatch must Ok");

    // (a) result is a PiecewisePolynomialProfile StructureInstance
    match &value {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "PiecewisePolynomialProfile",
                "input_shape TOTS must return a PiecewisePolynomialProfile, got {:?}",
                data.type_name,
            );
        }
        other => panic!(
            "fresh input_shape TOTS dispatch must return a PiecewisePolynomialProfile, got {other:?}"
        ),
    }

    // (a) output VC flipped to Final
    let node = NodeId::Value(cell.clone());
    assert_eq!(
        engine.freshness(&node),
        Freshness::Final,
        "Completed input_shape dispatch must flip the output VC to Final",
    );

    // (b) warm state donated under NodeId::Compute(c_id)
    let compute_node = NodeId::Compute(c_id.clone());
    let entry = engine
        .cache_store()
        .get(&compute_node)
        .expect("a Completed input_shape dispatch must create a Compute(c_id) entry");
    assert!(
        entry.warm_state.is_some(),
        "Completed input_shape dispatch must donate warm state",
    );

    // ── (c) second dispatch with identical inputs → cache HIT, valid result ──
    let handle2 = CancellationHandle::new();
    let result2 = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::input_shape",
        &inputs,
        &[],
        &Value::Undef,
        &handle2,
        VersionId(3),
        ContentHash(0), // inert: no cache dir in tests
    );
    let (value2, _diags2, _) =
        result2.expect("second input_shape dispatch (warm-state reuse) must Ok");
    assert!(
        matches!(&value2, Value::StructureInstance(d) if d.type_name == "PiecewisePolynomialProfile"),
        "warm-state HIT must return a valid PiecewisePolynomialProfile, got {value2:?}",
    );

    // Beyond the type check (which a broken cache would also satisfy): identical
    // `(profile, shaper)` ⇒ the HIT must reproduce the first dispatch's shaped
    // profile exactly (`solve_tots` is deterministic). A HIT that re-donated a
    // corrupted/wrong cached profile would diverge here.
    assert_eq!(
        value2.content_hash(),
        value.content_hash(),
        "warm-state HIT must return the SAME shaped profile as the first dispatch",
    );
}

// ── (d) CHANGED PROFILE → CACHE MISS ─────────────────────────────────────────

/// After a completed dispatch, changing a profile control point forces a cache
/// MISS. The MISS result must be a valid `PiecewisePolynomialProfile` AND a
/// genuine recompute — distinct from the cached first result, not a stale HIT.
///
/// We perturb the move ENDPOINT (`end`: 1.0 → 2.0), not the interior waypoint:
/// TOTS treats interior waypoints as free optimisation variables (tots.rs
/// `variable_vector` packs `[interiors…, T]`), so two interior seeds can converge
/// to the SAME time-optimal solution — a value-inequality assertion on an
/// interior perturbation would be fragile. The endpoints are fixed boundary
/// conditions copied verbatim into the solved waypoints (`tots_result_to_profile`
/// emits `j.end` as the last knot value), so a changed endpoint is reliably
/// result-determining and the MISS recompute is guaranteed to differ.
///
/// Gated `release-only` — two TOTS solves in debug consume ~16 s.
#[test]
#[cfg_attr(debug_assertions, ignore = "heavy TOTS solve; release-only")]
fn input_shape_tots_profile_change_forces_miss() {
    let mut engine = engine_with_input_shape_target();

    let cell = ValueCellId::new("InputShapeFixture", "result");
    let c_id = ComputeNodeId::new("InputShapeFixture", 0);
    seed_final_output(&mut engine, &cell);

    // First dispatch: a 0 → 1 move (seeds warm state).
    let inputs_a = vec![p2p_profile(5.0, 0.0, 1.0, 0.5), tots_shaper()];
    let h1 = CancellationHandle::new();
    let r1 = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::input_shape",
        &inputs_a,
        &[],
        &Value::Undef,
        &h1,
        VersionId(2),
        ContentHash(0), // inert: no cache dir in tests
    );
    let (value_first, _, _) = r1.expect("first dispatch must Ok");

    // Second dispatch: change the move endpoint (1.0 → 2.0) → different
    // profile_hash → MISS.
    let inputs_b = vec![p2p_profile(5.0, 0.0, 2.0, 0.5), tots_shaper()];
    let h2 = CancellationHandle::new();
    let r2 = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::input_shape",
        &inputs_b,
        &[],
        &Value::Undef,
        &h2,
        VersionId(3),
        ContentHash(0), // inert: no cache dir in tests
    );
    let (value_miss, _, _) = r2.expect("MISS dispatch must Ok (recompute)");
    assert!(
        matches!(&value_miss, Value::StructureInstance(d) if d.type_name == "PiecewisePolynomialProfile"),
        "MISS recompute must return a valid PiecewisePolynomialProfile, got {value_miss:?}",
    );

    // An always-HIT cache would return the cached 0→1 profile; the genuine MISS
    // recomputes the 0→2 profile, whose final waypoint (2.0 vs 1.0) differs — so
    // the shaped-profile content hashes must differ. This is the assertion a
    // type-only check could not make.
    assert_ne!(
        value_miss.content_hash(),
        value_first.content_hash(),
        "a changed profile endpoint must force a recompute (distinct shaped \
         profile), not reuse the cached 0→1 result",
    );
}

// ── (e) PRE-CANCELLED → Err(DispatchError::Cancelled), VC stays Pending ──────

/// A pre-cancelled handle must return `Err(DispatchError::Cancelled)` and leave
/// the output VC `Freshness::Pending` (prior best on display).
#[test]
fn input_shape_precancelled_leaves_output_vc_pending() {
    let mut engine = engine_with_input_shape_target();

    let cell = ValueCellId::new("InputShapeFixture", "result");
    let c_id = ComputeNodeId::new("InputShapeFixture", 0);
    seed_final_output(&mut engine, &cell);

    let inputs = tots_value_inputs(0.5);

    // Pre-cancel before dispatch.
    let handle = CancellationHandle::new();
    handle.cancel();

    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::input_shape",
        &inputs,
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
        ContentHash(0), // inert: no cache dir in tests
    );

    assert!(
        matches!(result, Err(DispatchError::Cancelled)),
        "pre-cancelled input_shape dispatch must return Err(DispatchError::Cancelled), \
         got {result:?}",
    );

    let node = NodeId::Value(cell.clone());
    assert!(
        matches!(engine.freshness(&node), Freshness::Pending { .. }),
        "cancelled dispatch must leave the output VC Pending; got {:?}",
        engine.freshness(&node),
    );
}

// ── (f) ZV IMPULSE ARM SMOKE ─────────────────────────────────────────────────

/// A `ZVShaper` dispatch routes through the same `"trajectory::input_shape"`
/// trampoline (the impulse arm) and must return a `PiecewisePolynomialProfile`.
///
/// This smoke check verifies the cheap impulse path is accessible via the
/// ComputeNode registration without regression from the TOTS arm.
#[test]
fn input_shape_zv_impulse_arm_returns_profile() {
    let mut engine = engine_with_input_shape_target();

    let cell = ValueCellId::new("InputShapeZV", "result");
    let c_id = ComputeNodeId::new("InputShapeZV", 0);
    seed_final_output(&mut engine, &cell);

    let inputs = zv_value_inputs();
    let handle = CancellationHandle::new();
    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::input_shape",
        &inputs,
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
        ContentHash(0), // inert: no cache dir in tests
    );
    let (value, _diags, _) = result.expect("ZV input_shape dispatch must Ok");
    assert!(
        matches!(&value, Value::StructureInstance(d) if d.type_name == "PiecewisePolynomialProfile"),
        "ZV impulse arm must return a PiecewisePolynomialProfile, got {value:?}",
    );
}

// ── task 6941 ζ: the trajectory readers refuse a present-but-wrong field ─────

/// Dispatch `input_shape(profile, shaper)` straight through the production
/// trampoline (no engine, no warm state).
fn shape(profile: Value, shaper: Value) -> ComputeOutcome {
    input_shape_trampoline(
        &[profile, shaper],
        &[],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    )
}

/// The message α renders when `spec` refuses `value` at `<owner>.<field>` of
/// an `input_shape` call — computed from α's own reader, never restated.
fn canonical_rejection(owner: &str, field: &str, value: &Value, spec: &ArgSpec) -> String {
    let Acceptance::Rejected(rejection) = accept_arg(value, spec) else {
        panic!(
            "fixture {owner}.{field} = {value:?} must be a value {} refuses",
            spec.type_name
        );
    };
    FieldRejection::new(owner, field, rejection)
        .to_error("input_shape")
        .message
}

/// `outcome` is `Failed` carrying exactly one Error, coded
/// `DimensionedArgRejected`, whose message is `expected`.
fn assert_rejected(outcome: &ComputeOutcome, expected: &str) {
    let ComputeOutcome::Failed { diagnostics, .. } = outcome else {
        panic!("expected Failed carrying {expected:?}, got {outcome:?}");
    };
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one diagnostic expected, got {diagnostics:?}"
    );
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(
        diagnostic.code,
        Some(DiagnosticCode::DimensionedArgRejected)
    );
    assert_eq!(diagnostic.message, expected);
}

/// The shaped `PiecewisePolynomialProfile` of a `Completed` dispatch.
fn shaped_profile(outcome: ComputeOutcome, what: &str) -> Value {
    let ComputeOutcome::Completed { result, .. } = &outcome else {
        panic!("{what}: expected Completed, got {outcome:?}");
    };
    assert!(
        matches!(result, Value::StructureInstance(d) if d.type_name == "PiecewisePolynomialProfile"),
        "{what}: expected a shaped PiecewisePolynomialProfile, got {result:?}"
    );
    result.clone()
}

/// `last.t − first.t` of a shaped profile, in seconds.
fn profile_duration(profile: &Value) -> f64 {
    let Value::StructureInstance(data) = profile else {
        panic!("a profile is a StructureInstance, got {profile:?}");
    };
    let Some(Value::List(waypoints)) = data.fields.get("waypoints") else {
        panic!("a profile carries a waypoints list, got {data:?}");
    };
    let t = |wp: &Value| match wp {
        Value::StructureInstance(d) => match d.fields.get("t") {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) if *dimension == DimensionVector::TIME => *si_value,
            other => panic!("a shaped waypoint's t is a Time scalar, got {other:?}"),
        },
        other => panic!("a waypoint is a StructureInstance, got {other:?}"),
    };
    t(waypoints.last().expect("non-empty")) - t(waypoints.first().expect("non-empty"))
}

/// (a) `target_frequency` is a FREQUENCY (Hz). An ANGULAR_VELOCITY value
/// (rad·s⁻¹) used to be stripped to its SI magnitude and multiplied by 2π —
/// a silent 6.28× mis-design. Every impulse shaper, and a shaper nested in a
/// `CascadedShaper` behind a valid sibling, now refuses it by name.
#[test]
fn impulse_shapers_refuse_an_angular_velocity_target_frequency() {
    let rad_per_s = scalar(50.0, DimensionVector::ANGULAR_VELOCITY);
    for type_name in ["ZVShaper", "ZVDShaper", "EIShaper"] {
        assert_rejected(
            &shape(ramp_profile(), impulse_shaper(type_name, rad_per_s.clone())),
            &canonical_rejection(type_name, "target_frequency", &rad_per_s, &frequency_spec()),
        );
    }

    let cascaded = struct_instance(
        "CascadedShaper",
        vec![(
            "shapers",
            Value::List(vec![
                impulse_shaper("ZVDShaper", scalar(10.0, DimensionVector::FREQUENCY)),
                impulse_shaper("ZVShaper", rad_per_s.clone()),
            ]),
        )],
    );
    assert_rejected(
        &shape(ramp_profile(), cascaded),
        &canonical_rejection(
            "ZVShaper",
            "target_frequency",
            &rad_per_s,
            &frequency_spec(),
        ),
    );
}

/// (b) The floor: a 50 Hz `ZVShaper` builds exactly `ImpulseTrain::zv(2π·50,
/// 0)`, bit for bit, and an absent `damping_ratio` still reads its declared 0.
#[test]
fn a_50_hz_zv_shaper_builds_exactly_the_2pi_50_train() {
    let expected = ImpulseTrain::zv(2.0 * PI * 50.0, 0.0).points();
    let absent_damping = without_field(zv_shaper(50.0, 0.0), "damping_ratio");
    for (what, shaper) in [
        ("damping_ratio: 0", zv_shaper(50.0, 0.0)),
        ("damping_ratio absent", absent_damping.clone()),
    ] {
        let train = build_train_for_shaper(&shaper)
            .unwrap_or_else(|rejection| {
                panic!("{what}: a 50 Hz ZVShaper is accepted, got {rejection}")
            })
            .unwrap_or_else(|| panic!("{what}: ZVShaper is a recognised shaper"));
        assert_eq!(train.points(), expected, "{what}");
    }
    assert_eq!(shaper_damping_ratio(&absent_damping), Ok(0.0));
}

/// (c) The TOTS limit and solver fields are read in their declared
/// dimensions: a LENGTH `velocity_limit`, a PRESSURE `vibration_tolerance` and
/// a String `max_iters` are each refused by name — never read as a number or
/// replaced by a default.
#[test]
fn tots_shaper_refuses_wrong_dimension_limit_and_solver_fields() {
    for (field, value, spec) in [
        (
            "velocity_limit",
            scalar(5.0, DimensionVector::LENGTH),
            velocity_spec(),
        ),
        (
            "vibration_tolerance",
            scalar(1.0, DimensionVector::PRESSURE),
            dimensionless_spec(),
        ),
        (
            "max_iters",
            Value::String("100".to_string()),
            dimensionless_spec(),
        ),
    ] {
        let shaper = with_field(tots_shaper(), field, value.clone());
        assert_rejected(
            &shape(p2p_profile(5.0, 0.0, 1.0, 0.5), shaper),
            &canonical_rejection("TOTSShaper", field, &value, &spec),
        );
    }
}

/// (c) An omitted `max_iters` / `tol` reads its declared default (100 /
/// 1e-6): the re-timed profile is identical to the explicit spelling's.
#[test]
#[cfg_attr(debug_assertions, ignore = "heavy TOTS solve; release-only")]
fn tots_shaper_omitted_max_iters_and_tol_read_their_declared_defaults() {
    let explicit = shaped_profile(
        shape(p2p_profile(5.0, 0.0, 1.0, 0.5), tots_shaper()),
        "explicit max_iters/tol",
    );
    let omitted = shaped_profile(
        shape(
            p2p_profile(5.0, 0.0, 1.0, 0.5),
            without_field(without_field(tots_shaper(), "max_iters"), "tol"),
        ),
        "omitted max_iters/tol",
    );
    assert_eq!(omitted.content_hash(), explicit.content_hash());
}

/// (d) The force bound is the joint's `actuator_limits` entry. `force_limit`
/// never existed on `TOTSShaper`, so the solver used to see 1000 N whatever
/// the author wrote; a binding 2 N limit (unit mass ⇒ |a| ≤ 2 m/s²) must now
/// re-time the move differently from a slack 1000 N one, and never faster.
///
/// Both shapers cap the solve at the same `SQP_ITERS` iterations: an active
/// force bound keeps the SQP from meeting its step tolerance, so it would
/// otherwise run all 100 default iterations (~30 min of debug CPU), and the
/// best feasible iterate is already bound-limited well before that.
#[test]
#[cfg_attr(debug_assertions, ignore = "heavy TOTS solve; release-only")]
fn tots_force_bound_comes_from_the_joint_actuator_limit() {
    const SQP_ITERS: i64 = 10;
    let shaped_with_force_limit = |max_force_n: f64, what: &str| {
        let shaper = with_field(
            tots_shaper_with_force_limit(max_force_n),
            "max_iters",
            Value::Int(SQP_ITERS),
        );
        shaped_profile(shape(p2p_profile(5.0, 0.0, 1.0, 0.5), shaper), what)
    };
    let slack = shaped_with_force_limit(1000.0, "1000 N limit");
    let binding = shaped_with_force_limit(2.0, "2 N limit");
    assert_ne!(
        binding.content_hash(),
        slack.content_hash(),
        "a binding actuator limit must change the re-timed profile"
    );
    let (t_slack, t_binding) = (profile_duration(&slack), profile_duration(&binding));
    assert!(
        t_binding >= t_slack,
        "the 2 N move ({t_binding:.4} s) must be no faster than the 1000 N one ({t_slack:.4} s)"
    );
}

/// (d) A `JointLimit.max_force` is a FORCE; a LENGTH one is refused by name.
#[test]
fn tots_refuses_a_length_joint_limit_max_force() {
    let metres = scalar(1000.0, DimensionVector::LENGTH);
    let shaper = with_field(
        tots_shaper(),
        "actuator_limits",
        Value::List(vec![joint_limit("JointLimit", 0.0, metres.clone())]),
    );
    assert_rejected(
        &shape(p2p_profile(5.0, 0.0, 1.0, 0.5), shaper),
        &canonical_rejection("JointLimit", "max_force", &metres, &force_spec()),
    );
}

/// (d) A profile joint no `JointLimit` covers is refused — `Failed` with the
/// one Error naming `actuator_limits` and the joint, never solved against a
/// substituted bound: with no `actuator_limits` field, with an empty list,
/// and with a limit on a different joint only.
#[test]
fn tots_refuses_a_joint_no_actuator_limit_covers() {
    for (what, shaper) in [
        (
            "no actuator_limits field",
            without_field(tots_shaper(), "actuator_limits"),
        ),
        (
            "empty actuator_limits",
            with_field(tots_shaper(), "actuator_limits", Value::List(vec![])),
        ),
        (
            "a limit on joint 1 only",
            with_field(
                tots_shaper(),
                "actuator_limits",
                Value::List(vec![joint_limit(
                    "JointLimit",
                    1.0,
                    scalar(1000.0, DimensionVector::FORCE),
                )]),
            ),
        ),
    ] {
        let outcome = shape(p2p_profile(5.0, 0.0, 1.0, 0.5), shaper);
        let ComputeOutcome::Failed { diagnostics, .. } = &outcome else {
            panic!("{what}: expected Failed, got {outcome:?}");
        };
        let [error] = diagnostics.as_slice() else {
            panic!("{what}: exactly one diagnostic expected, got {diagnostics:?}");
        };
        assert_eq!(error.severity, Severity::Error, "{what}");
        assert_eq!(
            error.message,
            "input_shape: actuator_limits has no entry for profile joint 0; a TOTS shaper \
             solves only the joints an actuator limit bounds",
            "{what}"
        );
    }
}

/// (e) A waypoint's `t` is a TIME; a LENGTH one is refused by name on both
/// the impulse (ZV) and the TOTS arm.
#[test]
fn a_length_waypoint_time_is_refused_on_both_arms() {
    let metres = scalar(1.0, DimensionVector::LENGTH);
    let expected = canonical_rejection("Waypoint", "t", &metres, &time_spec());

    let impulse_profile = pp_profile(vec![
        waypoint(0.0, &[0.0]),
        waypoint_at(metres.clone(), &[1.0]),
    ]);
    assert_rejected(&shape(impulse_profile, zv_shaper(10.0, 0.0)), &expected);

    let tots_profile = pp_profile(vec![
        waypoint(0.0, &[0.0]),
        waypoint_at(metres.clone(), &[0.5]),
        waypoint(5.0, &[1.0]),
    ]);
    assert_rejected(&shape(tots_profile, tots_shaper()), &expected);
}

/// (f) The revolute member reads its limits as rad/s, rad/s² and a Torque
/// `max_force`, and re-times the move.
#[test]
#[cfg_attr(debug_assertions, ignore = "heavy TOTS solve; release-only")]
fn revolute_tots_shaper_retimes_with_angular_limits() {
    let shaped = shaped_profile(
        shape(p2p_profile(5.0, 0.0, 1.0, 0.5), revolute_tots_shaper()),
        "RevoluteTOTSShaper",
    );
    let duration = profile_duration(&shaped);
    assert!(
        duration.is_finite() && duration > 0.0 && duration < 5.0,
        "a revolute re-time must be faster than the 5 s baseline, got {duration}"
    );
}

/// (f) A linear m/s `velocity_limit` on the revolute member is refused by name.
#[test]
fn revolute_tots_shaper_refuses_a_linear_velocity_limit() {
    let metres_per_s = scalar(5.0, DimensionVector::VELOCITY);
    let shaper = with_field(
        revolute_tots_shaper(),
        "velocity_limit",
        metres_per_s.clone(),
    );
    assert_rejected(
        &shape(p2p_profile(5.0, 0.0, 1.0, 0.5), shaper),
        &canonical_rejection(
            "RevoluteTOTSShaper",
            "velocity_limit",
            &metres_per_s,
            &angular_velocity_spec(),
        ),
    );
}
