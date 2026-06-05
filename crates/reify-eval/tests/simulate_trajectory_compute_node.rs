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
use reify_eval::{CancellationHandle, ComputeFn, DispatchError};
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

/// A well-formed `PiecewisePolynomialProfile` Value for a single-joint *curved*
/// move from `q0` to `q1` over `[0, T]` seconds with `NaturalSpline` boundary
/// and `CubicSpline` kind. `value_to_multijoint_spline` will marshal this.
///
/// Three waypoints, with the mid-point deliberately OFF the straight line
/// (`q0 + 0.75·(q1−q0)` at `T/2`, not the collinear `0.5·(q1−q0)`). A 2-knot
/// natural cubic would be exactly *linear* (q̈ ≡ 0), and the forward
/// simulator's modal forcing is acceleration-driven (τ = m·q̈), so a linear
/// ramp excites ZERO vibration regardless of `q1` — every `q1` would then yield
/// a byte-identical all-zero track, and a profile-change cache MISS could not be
/// told apart from a stale HIT. The non-collinear mid-point gives the natural
/// cubic a non-zero interior acceleration; because a natural cubic is linear in
/// its control values, with `q0 = 0` the whole spline (and hence the vibration
/// it drives) scales linearly with `q1`, so a changed `q1` produces a genuinely
/// distinct track.
fn ramp_profile(q0: f64, q1: f64, t_end: f64) -> Value {
    let q_mid = q0 + 0.75 * (q1 - q0);
    struct_instance(
        "PiecewisePolynomialProfile",
        vec![
            ("mechanism".to_string(), Value::Real(1.0)),
            (
                "waypoints".to_string(),
                Value::List(vec![
                    waypoint(0.0, &[q0]),
                    waypoint(t_end / 2.0, &[q_mid]),
                    waypoint(t_end, &[q1]),
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
/// - `profile`: a 1-joint curved move `PiecewisePolynomialProfile` with a
///   control value `q1` that can be varied to force a cache MISS (and a
///   genuinely distinct vibration track — see [`ramp_profile`]).
/// - `mech`: a `Real` placeholder (value_to_mechanism_model falls back to a
///   single-link unit-mass prismatic-X model — the path θ tests use). Its lone
///   DOF is the linear-X axis, so the modal forcing reads τ's X component.
/// - `modal`: a `ModalResult` with one 10 Hz, ζ=0 mode whose `shape` projects
///   onto that prismatic-X DOF (`[1, 0, 0]`). `forces_to_forcing_history` dots
///   the per-DOF τ vector with the flattened shape over their common length
///   (1 DOF here), so a Z-only shape (`[0, 0, 1]`) would project to zero and
///   excite NO vibration; aligning the shape with the actuated X axis (mirroring
///   θ's `force_projection: [1.0]` step-response fixture) yields a plausible
///   non-zero, `q1`-scaling vibration_offset.
fn simulate_value_inputs(q1: f64) -> Vec<Value> {
    vec![
        ramp_profile(0.0, q1, 1.0),
        Value::Real(1.0),
        modal_result(vec![mode(10.0, 0.0, &[[1.0, 0.0, 0.0]])]),
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

    // A type-only check passes for a correct HIT, a correct MISS, AND a broken
    // cache alike — so additionally pin the *value*: identical inputs ⇒ the HIT
    // must reproduce the first dispatch's track byte-for-byte (the simulation is
    // deterministic). A HIT that re-donated a corrupted/wrong cached track would
    // diverge here. (The companion `..._profile_change_forces_miss` test pins the
    // other direction: a changed input must NOT reuse the cached track.)
    assert_eq!(
        value2.content_hash(),
        value.content_hash(),
        "warm-state HIT must return the SAME EndEffectorTrack as the first dispatch",
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
    let (value_first, _) = r1.expect("first dispatch must Ok");

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

    // The MISS must be a genuine recompute, not a stale HIT returning the q1=1.0
    // track. The changed control point (q1: 1.0 → 2.0) scales the modal forcing,
    // so the recomputed track's vibration_offset/combined_pose differ from the
    // first result — the whole-value content hash therefore differs. An always-HIT
    // cache (e.g. `key.matches` hard-wired true) would return the stale q1=1.0
    // track and FAIL this inequality — exactly the regression a type-only
    // assertion let through.
    assert_ne!(
        value_miss.content_hash(),
        value_first.content_hash(),
        "a changed profile control point must force a recompute (distinct track), \
         not reuse the cached q1=1.0 result",
    );
}

// ── (e) PRE-CANCELLED → Err(DispatchError::Cancelled), VC stays Pending ──────

/// A pre-cancelled [`CancellationHandle`] must drive the `simulate_trajectory`
/// trampoline to `ComputeOutcome::Cancelled` (the on-entry poll short-circuits
/// before any marshalling / `simulate_trajectory_core` call), so
/// `run_compute_dispatch` returns `Err(DispatchError::Cancelled)` and leaves
/// the seeded output VC `Freshness::Pending` (prior best on display) rather
/// than `Final`.
///
/// Mirrors `modal_compute_node::modal_dispatch_precancelled_leaves_output_vc_pending`.
/// GREEN immediately after step-24: the entry cancellation checkpoint was
/// added in that step alongside the warm-state cache logic.
#[test]
fn simulate_trajectory_precancelled_leaves_output_vc_pending() {
    let mut engine = engine_with_simulate_target();

    let cell = ValueCellId::new("SimTrajFixture", "result");
    let c_id = ComputeNodeId::new("SimTrajFixture", 0);
    seed_final_output(&mut engine, &cell);

    let inputs = simulate_value_inputs(1.0);

    // Pre-cancel before dispatch.
    let handle = CancellationHandle::new();
    handle.cancel();

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

    assert!(
        matches!(result, Err(DispatchError::Cancelled)),
        "pre-cancelled simulate_trajectory dispatch must return Err(DispatchError::Cancelled), \
         got {result:?}",
    );

    let node = NodeId::Value(cell.clone());
    assert!(
        matches!(engine.freshness(&node), Freshness::Pending { .. }),
        "cancelled dispatch must leave the output VC Pending (prior best on display); \
         got {:?}",
        engine.freshness(&node),
    );
}
