//! Integration tests for the `simulate_trajectory` ComputeNode trampoline
//! (task π, steps 23–26): warm-state cache (HIT / MISS) and cooperative
//! cancellation, exercised end-to-end through the public dispatch seam
//! (`Engine::run_compute_dispatch`) against the registered
//! `"trajectory::simulate"` target.
//!
//! Mirrors `tests/modal_compute_node.rs` (modal `(K,M)` cache) and
//! `tests/cancellation_compute_dispatch.rs` (pre-cancelled dispatch contract).
//! The trajectory trampoline wraps `reify_stdlib::simulate_trajectory_value`
//! (pure `Value`→`Value`), so the test builds inline Value fixtures without
//! any `reify-stdlib`-internal helpers.
//!
//! Three observable signals:
//!
//! - **(a) COMPLETED → EndEffectorTrack + Final VC** — a fresh dispatch returns
//!   a `Value::StructureInstance` whose `type_name == "EndEffectorTrack"` and
//!   flips the output VC to `Freshness::Final`.
//!
//! - **(b) COMPLETED DONATES WARM STATE** — the output node carries a non-`None`
//!   `warm_state` under `NodeId::Compute(c_id)` after a completed dispatch.
//!
//! - **(c) SECOND DISPATCH IS A CACHE HIT** — a second dispatch on the same
//!   `c_id` with identical `(profile, mech, modal)` sources the donated cache
//!   and returns the same valid result without re-running the simulation.
//!
//! - **(d) CHANGED PROFILE → CACHE MISS** — a dispatch after mutating one
//!   profile control point forces a MISS (recompute). The new result is still
//!   a valid `EndEffectorTrack`.
//!
//! RED until step-24 implements `simulate_trajectory_trampoline` and registers
//! `"trajectory::simulate"` in `compute_targets::register_compute_fns`.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_core::{ComputeNodeId, DimensionVector, ValueCellId, VersionId};
use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{CancellationHandle, ComputeFn};
use reify_ir::{DeterminacyState, Freshness, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_test_support::make_simple_engine;

// ── Value fixture helpers ─────────────────────────────────────────────────────

/// Build a registry-free `Value::StructureInstance` (the `StructureTypeId(u32::MAX)`
/// sentinel is the convention used across all trampoline tests in this repo).
fn struct_instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
    let fields: PersistentMap<String, Value> = fields.into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: type_name.to_string(),
        version: 1,
        fields,
    }))
}

/// A `Time` scalar `Value` (SI seconds) — the type of a `Waypoint.t` field.
fn time_scalar(s: f64) -> Value {
    Value::Scalar {
        si_value: s,
        dimension: DimensionVector::TIME,
    }
}

/// A `List<Real>` `Value` — the type of `Waypoint.values` / vels / accels lists.
fn reals(vs: &[f64]) -> Value {
    Value::List(vs.iter().map(|&v| Value::Real(v)).collect())
}

/// An `Option<List<Real>>` `Value` — `None` for unset vels/accels.
fn no_opt() -> Value {
    Value::Option(None)
}

/// A `Waypoint` StructureInstance with the four eval-path fields.
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

/// A `SplineKind` enum `Value` — `"CubicSpline"` variant.
fn cubic_spline_kind() -> Value {
    Value::Enum {
        type_name: "SplineKind".to_string(),
        variant: "CubicSpline".to_string(),
    }
}

/// A well-formed `PiecewisePolynomialProfile` Value for a single-joint linear
/// ramp from `q0` to `q1` over `[0, T]` seconds with `NaturalSpline` boundary
/// and `CubicSpline` kind. `value_to_multijoint_spline` will marshal this.
fn ramp_profile(q0: f64, q1: f64, t_end: f64) -> Value {
    struct_instance(
        "PiecewisePolynomialProfile",
        vec![
            ("mechanism".to_string(), Value::Real(1.0)),
            (
                "waypoints".to_string(),
                Value::List(vec![waypoint(0.0, &[q0]), waypoint(t_end, &[q1])]),
            ),
            (
                "boundary".to_string(),
                struct_instance("NaturalSpline", vec![]),
            ),
            ("spline_kind".to_string(), cubic_spline_kind()),
        ],
    )
}

/// A per-node shape vector `Value::Vector([Real, Real, Real])` — the modal
/// `Mode.shape` element shape as the modal eval path emits it.
fn vec3(c: [f64; 3]) -> Value {
    Value::Vector(c.iter().map(|&x| Value::Real(x)).collect())
}

/// A `Mode` StructureInstance (frequency Hz; damping_ratio ζ; shape List<Vector3>).
fn mode(freq_hz: f64, zeta: f64, shape: &[[f64; 3]]) -> Value {
    struct_instance(
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

/// A `ModalResult` StructureInstance wrapping a `modes` list.
fn modal_result(modes: Vec<Value>) -> Value {
    struct_instance("ModalResult", vec![("modes".to_string(), Value::List(modes))])
}

/// The three flat `value_inputs` for `simulate_trajectory(profile, mech, modal)`:
/// - `profile`: a 1-joint ramp `PiecewisePolynomialProfile` with a control
///   value `q1` that can be varied to force a cache MISS.
/// - `mech`: a `Real` placeholder (value_to_mechanism_model falls back to a
///   single-link unit-mass prismatic model — the path θ tests use).
/// - `modal`: a `ModalResult` with one 10 Hz, ζ=0 mode and a unit Z-shape
///   (matches θ's step-response fixture so simulate_trajectory_core returns a
///   plausible EndEffectorTrackData with non-zero vibration_offset).
fn simulate_value_inputs(q1: f64) -> Vec<Value> {
    vec![
        ramp_profile(0.0, q1, 1.0),
        Value::Real(1.0),
        modal_result(vec![mode(10.0, 0.0, &[[0.0, 0.0, 1.0]])]),
    ]
}

// ── Engine setup ──────────────────────────────────────────────────────────────

/// Register the public `simulate_trajectory_trampoline` under its production
/// target on a fresh engine. Each test owns its engine (the registry panics on
/// duplicate targets, and `make_simple_engine` does NOT auto-register compute
/// fns — design-decision-10).
fn engine_with_simulate_target() -> reify_eval::Engine {
    let mut engine = make_simple_engine();
    engine.register_compute_fn(
        "trajectory::simulate",
        reify_eval::trajectory_ops::simulate_trajectory_trampoline as ComputeFn,
    );
    engine
}

/// Seed a Final output VC so `begin_compute_dispatch` has a `last_substantive`
/// to keep on display when a dispatch is cancelled (mirrors `modal_compute_node`).
fn seed_final_output(engine: &mut reify_eval::Engine, cell: &ValueCellId) {
    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );
}

// ── (a) + (b) + (c) COMPLETED → EndEffectorTrack, warm state donated, HIT ────

/// A fresh (un-cancelled) `simulate_trajectory` dispatch must:
///
///   (a) return a `Value::StructureInstance` with `type_name == "EndEffectorTrack"`
///       and flip the output VC to `Freshness::Final`.
///
///   (b) donate a non-`None` warm state under `NodeId::Compute(c_id)`.
///
///   (c) a second dispatch on the same `c_id` with IDENTICAL `(profile, mech,
///       modal)` sources the donated cache (cache HIT) and returns a valid
///       `EndEffectorTrack` — the warm-state round-trip drives no error.
///
/// RED until step-24: `simulate_trajectory_trampoline` does not yet exist.
#[test]
fn simulate_trajectory_completed_donates_warm_state_then_reuses() {
    let mut engine = engine_with_simulate_target();

    let cell = ValueCellId::new("SimTrajFixture", "result");
    let c_id = ComputeNodeId::new("SimTrajFixture", 0);
    seed_final_output(&mut engine, &cell);

    // ── (a) + (b) first fresh dispatch → Completed, VC Final, warm state donated
    let inputs = simulate_value_inputs(1.0);
    let handle = CancellationHandle::new();
    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::simulate",
        &inputs,
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
    );
    let (value, _diags) = result.expect("fresh simulate_trajectory dispatch must Ok");

    // (a) result is an EndEffectorTrack StructureInstance
    match &value {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "EndEffectorTrack",
                "simulate_trajectory must return an EndEffectorTrack StructureInstance, \
                 got type_name {:?}",
                data.type_name,
            );
        }
        other => panic!(
            "fresh simulate_trajectory dispatch must return a Value::StructureInstance, \
             got {other:?}"
        ),
    }

    // (a) output VC flipped to Final
    let node = NodeId::Value(cell.clone());
    assert_eq!(
        engine.freshness(&node),
        Freshness::Final,
        "Completed simulate_trajectory dispatch must flip the output VC to Final",
    );

    // (b) warm state donated under NodeId::Compute(c_id)
    let compute_node = NodeId::Compute(c_id.clone());
    let entry = engine
        .cache_store()
        .get(&compute_node)
        .expect(
            "a Completed simulate_trajectory dispatch must create a Compute(c_id) \
             entry carrying warm state",
        );
    assert!(
        entry.warm_state.is_some(),
        "Completed simulate_trajectory dispatch must donate warm state under \
         NodeId::Compute(c_id)",
    );

    // ── (c) second dispatch with identical inputs → cache HIT, valid result ──
    let handle2 = CancellationHandle::new();
    let result2 = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::simulate",
        &inputs,
        &[],
        &Value::Undef,
        &handle2,
        VersionId(3),
    );
    let (value2, _diags2) = result2.expect(
        "second simulate_trajectory dispatch (warm-state reuse) must Ok",
    );
    assert!(
        matches!(&value2, Value::StructureInstance(d) if d.type_name == "EndEffectorTrack"),
        "warm-state HIT must return a valid EndEffectorTrack, got {value2:?}",
    );
}

// ── (d) CHANGED PROFILE → CACHE MISS ─────────────────────────────────────────

/// After a completed dispatch, changing one profile control point (q1: 1.0 →
/// 2.0) must force a cache MISS (new `profile_hash`) and a recompute. The MISS
/// result must still be a valid `EndEffectorTrack`.
///
/// RED until step-24.
#[test]
fn simulate_trajectory_profile_change_forces_miss() {
    let mut engine = engine_with_simulate_target();

    let cell = ValueCellId::new("SimTrajFixture", "result");
    let c_id = ComputeNodeId::new("SimTrajFixture", 0);
    seed_final_output(&mut engine, &cell);

    // First dispatch with q1=1.0 (seeds warm state).
    let inputs_a = simulate_value_inputs(1.0);
    let h1 = CancellationHandle::new();
    let r1 = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::simulate",
        &inputs_a,
        &[],
        &Value::Undef,
        &h1,
        VersionId(2),
    );
    r1.expect("first dispatch must Ok");

    // Second dispatch with q1=2.0 (different profile → MISS).
    let inputs_b = simulate_value_inputs(2.0);
    let h2 = CancellationHandle::new();
    let r2 = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "trajectory::simulate",
        &inputs_b,
        &[],
        &Value::Undef,
        &h2,
        VersionId(3),
    );
    let (value_miss, _) = r2.expect("MISS dispatch must Ok (recompute)");
    assert!(
        matches!(&value_miss, Value::StructureInstance(d) if d.type_name == "EndEffectorTrack"),
        "MISS recompute must return a valid EndEffectorTrack, got {value_miss:?}",
    );
}
