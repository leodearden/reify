//! ≥40 dB ZV residual-vibration suppression acceptance test (task π, step-29).
//!
//! Exercises the complete `"trajectory::simulate"` + `"trajectory::input_shape"`
//! pipeline end-to-end through the registered `ComputeFn` trampolines:
//!
//!   1. `register_compute_fns` — always-run guard that both trajectory targets
//!      are installed (mirrors the `modal_analysis_e2e.rs` seam pin).
//!   2. `zv_shaped_ramp_achieves_40_db_residual_reduction` — the numerical
//!      acceptance gate:
//!        - shape a clamped S-curve ramp with a ZVShaper tuned to the EXACT
//!          modal frequency → analytic residual cancellation;
//!        - simulate both unshaped and shaped trajectories;
//!        - compare the vibration amplitude at the END of each simulation
//!          (the post-move residual):
//!            * unshaped last sample ≈ −2C/ω² (peak residual amplitude);
//!            * shaped last sample ≈ 0 (ZV cancellation exact at ζ=0 / exact f₀);
//!        - assert 20·log₁₀(unshaped / shaped) ≥ 40 dB.
//!
//! Fixture physics:
//!   f₀ = 10 Hz, ζ = 0, T_move = 1.0 s (= 10·T_period → ωT = 20π).
//!   For a clamped cubic spline with F(t) = C·(1 − 2t/T), the residual at t=T
//!   is x(T) = −2C/ω² and ẋ(T) = 0 at ωT = 2πn — so the last sample IS the
//!   peak-amplitude point of the free vibration (|residual| = 2C/ω² ≈ 3 mm).
//!   ZV shaper: trailing_time = T/2 = 0.05 s, so the shaped simulation ends at
//!   1.05 s with exactly zero residual vibration (superposition cancels).
//!
//! CRITICAL: `register_compute_fns` MUST be called before any dispatch.
//!   `make_simple_engine` does NOT auto-register compute fns (design-decision-10).
//!   Without registration, both trampolines body-inline to echo/empty-track and
//!   the dB ratio is meaningless.
//!
//! Mode shape: `[1.0, 0.0, 0.0]` (X component = 1) for the prismatic-X
//! placeholder mechanism — `forces_to_forcing_history` takes the dot product of
//! tau (1-DOF, X-direction force) with force_projection[:1], so the first
//! shape component must be 1.0 to get nonzero modal forcing. `[0.0, 0.0, 1.0]`
//! (Z-only shape) would give zero dot product with the 1-DOF tau and hence zero
//! vibration — the test would be trivially meaningless.
//!
//! RED until step-30 ensures the full wiring is correct.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_core::{ComputeNodeId, ContentHash, DimensionVector, ValueCellId, VersionId};
use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{CancellationHandle, ComputeFn};
use reify_ir::{
    DeterminacyState, Freshness, PersistentMap, StructureInstanceData, StructureTypeId, Value,
};
use reify_test_support::make_simple_engine;

// ── Value fixture helpers ─────────────────────────────────────────────────────

fn struct_instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
    let fields: PersistentMap<String, Value> = fields.into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: type_name.to_string(),
        version: 1,
        fields,
    }))
}

fn time_scalar(s: f64) -> Value {
    Value::Scalar {
        si_value: s,
        dimension: DimensionVector::TIME,
    }
}

fn reals(vs: &[f64]) -> Value {
    Value::List(vs.iter().map(|&v| Value::Real(v)).collect())
}

fn no_opt() -> Value {
    Value::Option(None)
}

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

fn cubic_spline_kind() -> Value {
    Value::Enum {
        type_name: "SplineKind".to_string(),
        variant: "CubicSpline".to_string(),
    }
}

/// A clamped S-curve ramp profile: zero velocity at both endpoints, so the
/// arm accelerates from rest at q0 and decelerates to rest at q1. The
/// acceleration is nonzero and excites the modal vibration — unlike a 2-waypoint
/// natural cubic spline (which degenerates to a linear profile with zero
/// acceleration and hence zero modal forcing).
///
/// ClampedSpline(start_velocity:[0.0], end_velocity:[0.0]) serializes as the
/// ClampedSpline StructureInstance that `value_to_multijoint_spline` dispatches
/// to `BoundaryCondition::Clamped { start_vel: 0.0, end_vel: 0.0 }`.
fn clamped_ramp_profile(q0: f64, q1: f64, t_end: f64) -> Value {
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
                struct_instance(
                    "ClampedSpline",
                    vec![
                        ("start_velocity".to_string(), reals(&[0.0])),
                        ("end_velocity".to_string(), reals(&[0.0])),
                    ],
                ),
            ),
            ("spline_kind".to_string(), cubic_spline_kind()),
        ],
    )
}

/// A ZVShaper StructureInstance tuned to `freq_hz` / `zeta`.
///
/// ZV: two impulses at t=0 (amplitude 0.5) and t=T/2 (amplitude 0.5) where
/// T = 1/freq_hz. At ζ=0 / exact frequency match the post-move residual is
/// analytically zero.
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

/// A Mode StructureInstance.
///
/// Shape: `[1.0, 0.0, 0.0]` — the X component is 1.0 so the dot product with
/// the 1-DOF prismatic-X joint tau gives nonzero modal forcing. A Z-only shape
/// `[0.0, 0.0, 1.0]` would be truncated to the first element (0.0) by
/// `forces_to_forcing_history` (common_len = min(1, 3) = 1), yielding zero
/// forcing and a meaningless test.
fn mode_x_shape(freq_hz: f64, zeta: f64) -> Value {
    struct_instance(
        "Mode",
        vec![
            ("frequency".to_string(), Value::Real(freq_hz)),
            ("damping_ratio".to_string(), Value::Real(zeta)),
            (
                "shape".to_string(),
                Value::List(vec![
                    // One node with shape vector [1, 0, 0] (X axis).
                    Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
                ]),
            ),
        ],
    )
}

fn modal_result(modes: Vec<Value>) -> Value {
    struct_instance(
        "ModalResult",
        vec![("modes".to_string(), Value::List(modes))],
    )
}

/// Seed a Final output VC (required by `begin_compute_dispatch`).
fn seed_final(engine: &mut reify_eval::Engine, cell: &ValueCellId) {
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

/// Run one dispatch and return the result Value (panics on error/cancel).
fn dispatch(
    engine: &mut reify_eval::Engine,
    c_id: &ComputeNodeId,
    cell: &ValueCellId,
    target: &str,
    inputs: &[Value],
) -> Value {
    let handle = CancellationHandle::new();
    let (value, _diags) = engine
        .run_compute_dispatch(
            c_id,
            std::slice::from_ref(cell),
            target,
            inputs,
            &[],
            &Value::Undef,
            &handle,
            VersionId(2),
            ContentHash(0), // inert: no cache dir in tests
        )
        .unwrap_or_else(|e| panic!("dispatch({target}) must succeed, got: {:?}", e));
    value
}

/// Extract the `vibration_offset[loc][:]` dz scalar series from an
/// `EndEffectorTrack` StructureInstance. Returns an empty Vec on any structural
/// mismatch (not panic).
fn vibration_at_loc(track: &Value, loc: usize) -> Vec<f64> {
    let Value::StructureInstance(data) = track else {
        return vec![];
    };
    let Some(Value::List(locs)) = data.fields.get(&"vibration_offset".to_string()) else {
        return vec![];
    };
    let Some(Value::List(times)) = locs.get(loc) else {
        return vec![];
    };
    times
        .iter()
        .filter_map(|v| {
            if let Value::Real(r) = v {
                Some(*r)
            } else {
                None
            }
        })
        .collect()
}

// ── Seam pin ─────────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn _seam_pin() {
    let _sim: ComputeFn = reify_eval::trajectory_ops::simulate_trajectory_trampoline;
    let _shp: ComputeFn = reify_eval::trajectory_ops::input_shape_trampoline;
}

// ── Step-29: registration guard (always-run) ──────────────────────────────────

/// `register_compute_fns` installs BOTH trajectory trampolines.
///
/// Mirrors `modal_analysis_e2e.rs::register_compute_fns_installs_modal_free_vibration`
/// and `modal_transient_e2e.rs::register_compute_fns_installs_transient_trampolines`.
/// This always-run guard catches registration regressions independent of the
/// heavy numerical acceptance gate below.
#[test]
fn register_compute_fns_installs_trajectory_trampolines() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    assert!(
        engine.compute_dispatch("trajectory::simulate").is_some(),
        "register_compute_fns must install a trampoline under 'trajectory::simulate'"
    );
    assert!(
        engine.compute_dispatch("trajectory::input_shape").is_some(),
        "register_compute_fns must install a trampoline under 'trajectory::input_shape'"
    );
}

// ── Step-29: ≥40 dB acceptance gate ──────────────────────────────────────────
//
// Physics recap (design-decision-7):
//
//   Profile: clamped cubic spline 0 → 1 over T_move = 1.0 s.
//     Acceleration: F(t) = C·(1 − 2t/T), C = 6·mass/T² = 6 N.
//     At ωT = 2πn (T_move = 1.0 s, f₀ = 10 Hz → n = 10):
//       x(T_move) = −2C/ω²  ≈ −0.00304 m  (peak-amplitude point: ẋ(T) = 0)
//   So vib_unshaped[last] = 0.00304 m ≫ 1e-6 (meaningful residual).
//
//   ZV shaper (ζ=0, exact f₀): trailing_time = T/2 = 0.05 s.
//     z_shaped(T_move + T/2) = 0.5·z_orig(1.05) + 0.5·z_orig(1.00)
//                             = 0.5·A − 0.5·A = 0  (exact cancellation)
//   So vib_shaped[last] ≈ 0  (numerical machine-precision residual).
//
//   dB ratio = 20·log₁₀(0.00304 / ε) ≫ 40 dB.

/// ≥40 dB ZV residual-vibration suppression acceptance test.
///
/// CRITICAL: uses `register_compute_fns` (not the raw trampolines) to exercise
/// the production registration path. `make_simple_engine` does NOT auto-register
/// (design-decision-10), so without this call the dispatches body-inline to
/// empty/echo, making the dB ratio meaningless.
///
/// RED until step-30 ensures the full wiring is correct (input_shape genuinely
/// shapes and simulate_trajectory correctly marshals the single mode + minimal
/// mechanism so the Z-axis vibration is captured).
#[cfg_attr(debug_assertions, ignore = "heavy modal ODE integration; release-only")]
#[test]
fn zv_shaped_ramp_achieves_40_db_residual_reduction() {
    // ── fixture parameters ────────────────────────────────────────────────────
    let f0 = 10.0_f64; // modal frequency (Hz)
    let zeta = 0.0_f64; // undamped — analytic ZV residual = exactly 0
    let t_move = 1.0_f64; // ramp duration (s) — 10 periods at 10 Hz → ωT = 20π
    // (residual at peak-amplitude phase, see physics note)

    // ── engine setup ──────────────────────────────────────────────────────────
    let mut engine = make_simple_engine();
    // CRITICAL: register before any dispatch.
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    // ── value fixtures ────────────────────────────────────────────────────────
    let profile = clamped_ramp_profile(0.0, 1.0, t_move);
    let shaper = zv_shaper(f0, zeta);
    let modal = modal_result(vec![mode_x_shape(f0, zeta)]);
    let mech = Value::Real(1.0); // Real placeholder → single-link unit-mass prismatic-X

    // ── (1) ZV-shape the ramp profile ────────────────────────────────────────
    let shape_cell = ValueCellId::new("ZvDbAcceptance", "shaped");
    let shape_cid = ComputeNodeId::new("ZvDbAcceptance", 0);
    seed_final(&mut engine, &shape_cell);
    let shaped = dispatch(
        &mut engine,
        &shape_cid,
        &shape_cell,
        "trajectory::input_shape",
        &[profile.clone(), shaper],
    );

    let shaped_type = match &shaped {
        Value::StructureInstance(d) => d.type_name.as_str(),
        _ => panic!(
            "input_shape must return a StructureInstance, got: {:?}",
            shaped
        ),
    };
    assert_eq!(
        shaped_type, "PiecewisePolynomialProfile",
        "ZV input_shape must return a PiecewisePolynomialProfile, got type {:?}",
        shaped_type
    );

    // ── (2) Simulate the UNSHAPED trajectory ──────────────────────────────────
    let sim_u_cell = ValueCellId::new("ZvDbAcceptance", "track_u");
    let sim_u_cid = ComputeNodeId::new("ZvDbAcceptance", 1);
    seed_final(&mut engine, &sim_u_cell);
    let track_unshaped = dispatch(
        &mut engine,
        &sim_u_cid,
        &sim_u_cell,
        "trajectory::simulate",
        &[profile.clone(), mech.clone(), modal.clone()],
    );

    assert!(
        matches!(&track_unshaped, Value::StructureInstance(d) if d.type_name == "EndEffectorTrack"),
        "simulate_trajectory(unshaped) must return an EndEffectorTrack, got: {:?}",
        track_unshaped
    );

    // ── (3) Simulate the ZV-SHAPED trajectory ─────────────────────────────────
    let sim_s_cell = ValueCellId::new("ZvDbAcceptance", "track_s");
    let sim_s_cid = ComputeNodeId::new("ZvDbAcceptance", 2);
    seed_final(&mut engine, &sim_s_cell);
    let track_shaped = dispatch(
        &mut engine,
        &sim_s_cid,
        &sim_s_cell,
        "trajectory::simulate",
        &[shaped, mech, modal],
    );

    assert!(
        matches!(&track_shaped, Value::StructureInstance(d) if d.type_name == "EndEffectorTrack"),
        "simulate_trajectory(shaped) must return an EndEffectorTrack"
    );

    // ── (4) Extract post-move residual from vibration_offset[0] ───────────────
    //
    // The vibration_offset[loc][t] dz series is the scalar modal displacement
    // (the core maps scalar vibration onto the Z axis). deviation_from_nominal
    // = |combined − nominal| = |vib_z|, so we read vibration_offset directly
    // to avoid requiring the accessor dispatch infrastructure here.
    //
    // Post-move residual = vibration at the LAST time sample:
    //   - unshaped: last sample at t = T_move (= 1.0 s). At ωT = 2πn the
    //     residual is at peak phase (x = −2C/ω², ẋ = 0) → max amplitude.
    //   - shaped: last sample at t = T_move + T/2 (= 1.05 s). ZV cancellation
    //     makes x ≈ 0 exactly (verified analytically; see module docstring).
    let vib_u = vibration_at_loc(&track_unshaped, 0);
    let vib_s = vibration_at_loc(&track_shaped, 0);

    assert!(
        !vib_u.is_empty(),
        "unshaped track must have time samples — \
         clamped ramp profile must marshal to a valid spline"
    );
    assert!(
        !vib_s.is_empty(),
        "shaped track must have time samples — \
         ZV-shaped profile must marshal to a valid spline"
    );

    let residual_u = vib_u.last().copied().unwrap_or(0.0).abs();
    let residual_s = vib_s.last().copied().unwrap_or(0.0).abs();

    // Guard: unshaped residual must be meaningful (≫ 1e-6).
    // If this fails the mode shape / profile / mechanism are not coupled
    // (e.g. Z-only shape gives zero forcing for prismatic-X joint).
    assert!(
        residual_u > 1e-6,
        "unshaped residual must be non-negligible (got {residual_u:.3e}) — \
         use X-axis mode shape [[1,0,0]] for prismatic-X mechanism, \
         and ensure the clamped ramp has nonzero acceleration"
    );

    // The shaped residual may be exactly 0.0 (exact ZV cancellation at ζ=0).
    // Guard log10(x/0) = ∞ which vacuously satisfies the assertion.
    let db_ratio = if residual_s < 1e-30 {
        f64::INFINITY
    } else {
        20.0 * (residual_u / residual_s).log10()
    };

    assert!(
        db_ratio >= 40.0,
        "ZV shaping must achieve ≥40 dB residual suppression; \
         got {db_ratio:.1} dB (unshaped={residual_u:.3e}, shaped={residual_s:.3e}). \
         Check: (a) input_shape ZV arm genuinely shapes (not echo); \
                (b) simulate_trajectory marshals the single X-projection mode \
                    so vibration_offset reflects the analytic response; \
                (c) the shaped profile's re-fitted spline faithfully captures \
                    the convolved command (SAMPLES_PER_SHAPER_DELAY oversample)."
    );
}
