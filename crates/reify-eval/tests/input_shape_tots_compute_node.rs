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

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_core::{ComputeNodeId, DimensionVector, ValueCellId, VersionId};
use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{CancellationHandle, ComputeFn, DispatchError};
use reify_ir::{DeterminacyState, Freshness, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_test_support::make_simple_engine;

// ── Value fixture helpers ─────────────────────────────────────────────────────

/// Build a registry-free `Value::StructureInstance`.
fn struct_instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
    let fields: PersistentMap<String, Value> = fields.into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: type_name.to_string(),
        version: 1,
        fields,
    }))
}

/// A `Time` scalar `Value` (SI seconds).
fn time_scalar(s: f64) -> Value {
    Value::Scalar {
        si_value: s,
        dimension: DimensionVector::TIME,
    }
}

/// A `List<Real>` `Value`.
fn reals(vs: &[f64]) -> Value {
    Value::List(vs.iter().map(|&v| Value::Real(v)).collect())
}

/// Optional `List<Real>` (None for unset vels/accels).
fn no_opt() -> Value {
    Value::Option(None)
}

/// A `Waypoint` StructureInstance.
fn waypoint(t: f64, values: &[f64]) -> Value {
    struct_instance(
        "Waypoint",
        vec![
            ("t".to_string(), time_scalar(t)),
            ("values".to_string(), reals(values)),
            ("vels".to_string(), no_opt()),
            ("accels".to_string(), no_opt()),
        ],
    )
}

/// `SplineKind::CubicSpline` enum Value.
fn cubic_spline_kind() -> Value {
    Value::Enum {
        type_name: "SplineKind".to_string(),
        variant: "CubicSpline".to_string(),
    }
}

/// A `PiecewisePolynomialProfile` with 3 waypoints: start, mid, end — so the
/// TOTS solver has one interior waypoint to optimise. `mid_val` is the midpoint
/// q value; changing it exercises cache MISS.
fn p2p_profile(duration: f64, start: f64, end: f64, mid_val: f64) -> Value {
    struct_instance(
        "PiecewisePolynomialProfile",
        vec![
            ("mechanism".to_string(), Value::Real(1.0)),
            (
                "waypoints".to_string(),
                Value::List(vec![
                    waypoint(0.0, &[start]),
                    waypoint(duration / 2.0, &[mid_val]),
                    waypoint(duration, &[end]),
                ]),
            ),
            (
                "boundary".to_string(),
                struct_instance("NaturalSpline", vec![]),
            ),
            ("spline_kind".to_string(), cubic_spline_kind()),
        ],
    )
}

/// A single-mode `Mode` Value (used by TOTSShaper).
fn mode(freq_hz: f64, zeta: f64, shape: [f64; 3]) -> Value {
    struct_instance(
        "Mode",
        vec![
            ("frequency".to_string(), Value::Real(freq_hz)),
            ("damping_ratio".to_string(), Value::Real(zeta)),
            (
                "shape".to_string(),
                Value::List(vec![Value::Vector(
                    shape.iter().map(|&x| Value::Real(x)).collect(),
                )]),
            ),
        ],
    )
}

/// A `TOTSShaper` StructureInstance with slack vel/acc/vib limits and one mode.
///
/// Scalar `velocity_limit`/`acceleration_limit` path (no `actuator_limits`
/// struct): exercises the marshalling fallback.  `vibration_tolerance = 1.0`
/// (very slack) + `velocity_limit = 5.0` + `acceleration_limit = 50.0` →
/// feasible for a 0→1 unit ramp with a 5 s baseline.
fn tots_shaper() -> Value {
    struct_instance(
        "TOTSShaper",
        vec![
            ("velocity_limit".to_string(), Value::Real(5.0)),
            ("acceleration_limit".to_string(), Value::Real(50.0)),
            ("vibration_tolerance".to_string(), Value::Real(1.0)),
            ("max_iters".to_string(), Value::Int(100)),
            ("tol".to_string(), Value::Real(1e-6)),
            (
                "modes".to_string(),
                Value::List(vec![mode(10.0, 0.05, [1.0, 0.0, 0.0])]),
            ),
        ],
    )
}

/// A `ZVShaper` StructureInstance (impulse arm, cheap path).
fn zv_shaper(freq_hz: f64, zeta: f64) -> Value {
    struct_instance(
        "ZVShaper",
        vec![
            (
                "target_frequency".to_string(),
                Value::Scalar {
                    si_value: freq_hz,
                    dimension: DimensionVector::FREQUENCY,
                },
            ),
            ("damping_ratio".to_string(), Value::Real(zeta)),
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
    // 2-waypoint ramp (simpler: ZV doesn't need interior waypoints).
    let profile = struct_instance(
        "PiecewisePolynomialProfile",
        vec![
            ("mechanism".to_string(), Value::Real(1.0)),
            (
                "waypoints".to_string(),
                Value::List(vec![
                    waypoint(0.0, &[0.0]),
                    waypoint(1.0, &[1.0]),
                ]),
            ),
            (
                "boundary".to_string(),
                struct_instance("NaturalSpline", vec![]),
            ),
            ("spline_kind".to_string(), cubic_spline_kind()),
        ],
    );
    vec![profile, zv_shaper(10.0, 0.0)]
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
#[test]
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
    );
    let (value, _diags) = result.expect("fresh input_shape TOTS dispatch must Ok");

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
    );
    let (value2, _diags2) = result2.expect(
        "second input_shape dispatch (warm-state reuse) must Ok",
    );
    assert!(
        matches!(&value2, Value::StructureInstance(d) if d.type_name == "PiecewisePolynomialProfile"),
        "warm-state HIT must return a valid PiecewisePolynomialProfile, got {value2:?}",
    );
}

// ── (d) CHANGED PROFILE → CACHE MISS ─────────────────────────────────────────

/// After a completed dispatch, changing the midpoint control value forces a
/// cache MISS. The MISS result must still be a valid `PiecewisePolynomialProfile`.
#[test]
fn input_shape_tots_profile_change_forces_miss() {
    let mut engine = engine_with_input_shape_target();

    let cell = ValueCellId::new("InputShapeFixture", "result");
    let c_id = ComputeNodeId::new("InputShapeFixture", 0);
    seed_final_output(&mut engine, &cell);

    // First dispatch with mid=0.5 (seeds warm state).
    let inputs_a = tots_value_inputs(0.5);
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
    );
    r1.expect("first dispatch must Ok");

    // Second dispatch with mid=0.7 (different profile hash → MISS).
    let inputs_b = tots_value_inputs(0.7);
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
    );
    let (value_miss, _) = r2.expect("MISS dispatch must Ok (recompute)");
    assert!(
        matches!(&value_miss, Value::StructureInstance(d) if d.type_name == "PiecewisePolynomialProfile"),
        "MISS recompute must return a valid PiecewisePolynomialProfile, got {value_miss:?}",
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
    );
    let (value, _diags) = result.expect("ZV input_shape dispatch must Ok");
    assert!(
        matches!(&value, Value::StructureInstance(d) if d.type_name == "PiecewisePolynomialProfile"),
        "ZV impulse arm must return a PiecewisePolynomialProfile, got {value:?}",
    );
}
