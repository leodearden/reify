//! `input_shape(profile, shaper)` dispatcher + Profile/Shaper `Value`
//! marshalling for the trajectory stdlib module (PRD
//! `docs/prds/v0_3/trajectory-input-shaping.md` §5.3, §11 Phase 2 task ζ,
//! Phase 4 task λ).
//!
//! Two pieces live here:
//!
//! 1. [`build_train_for_shaper`] — the marshalling boundary that reads a
//!    `Shaper` [`Value::StructureInstance`] (ZVShaper / ZVDShaper / EIShaper /
//!    CascadedShaper) and constructs the corresponding
//!    [`super::impulse_shaper::ImpulseTrain`]. This is where the Hz→rad/s
//!    conversion (`ω_n = 2π·f`) happens — the pure `impulse_shaper` math is
//!    entirely in angular frequency (rad/s). Exposed (via the `reify_stdlib`
//!    re-export) so the engine-side band-sweep robustness metric in
//!    `reify-eval/src/trajectory_ops.rs` can reuse it.
//!
//! 2. [`eval_input_shape`] — the thin `eval_trajectory` dispatch arm that maps
//!    `(profile, shaper)` `Value` arguments to the shaped `Profile`, mirroring
//!    the `gcode_import` precedent (arity / `StructureInstance` arg-reading,
//!    bad-args → [`Value::Undef`]). Full command-waveform resampling to new
//!    waypoints is deferred to task θ; ζ returns a registry-free shaped-Profile
//!    stand-in that echoes the input profile (a valid `Shaper` is still
//!    required — an unrecognised shaper ⇒ `Value::Undef`).
//!
//!    The dispatcher checks for `TOTSShaper` BEFORE the impulse-train path (λ):
//!    when the shaper is a `TOTSShaper`, it runs the real SQP loop
//!    ([`super::tots::solve_tots`], which calls `simulate_trajectory_core` +
//!    `inverse_dynamics_open_chain` per iteration) on a canonical single-DOF
//!    point-to-point stand-in parameterised by the shaper's readable scalar
//!    fields. `ConstraintInfeasible` → `Value::Undef`; `Converged` /
//!    `NonConvergence` → profile echo (identical to the impulse arms). Full
//!    profile-waypoint / modes / actuator_limits Value marshalling is θ-deferred.

use std::f64::consts::PI;

use reify_ir::{StructureInstanceData, Value};

use super::impulse_shaper::ImpulseTrain;
use super::simulate::{EffectorLocation, MechanismModel, ModeDesc, ModalModel};
use super::simulate::LinkDesc;
use super::tots::{
    JointWaypoints, SqpConfig, TotsModel, TotsOutcome, TotsParams, solve_tots,
};
use crate::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};

/// Read a numeric stdlib field as `f64`, accepting any spelling a shaper param
/// takes: a dimensioned `Scalar { si_value }` (`target_frequency`, whose SI
/// magnitude is Hz), a `Real` (`damping_ratio` / `vibration_tolerance`), or an
/// `Int`. Any other variant yields `None` so the caller can apply its default.
/// Mirrors `modal_ops::read_scalar_si`.
fn read_scalar_si(val: &Value) -> Option<f64> {
    match val {
        Value::Scalar { si_value, .. } => Some(*si_value),
        Value::Real(r) => Some(*r),
        Value::Int(n) => Some(*n as f64),
        _ => None,
    }
}

/// Read numeric field `name` from `data`'s fields as `f64`, falling back to
/// `default` when the field is absent or non-numeric.
fn field_f64(data: &StructureInstanceData, name: &str, default: f64) -> f64 {
    data.fields
        .get(&name.to_string())
        .and_then(read_scalar_si)
        .unwrap_or(default)
}

/// The design damping ratio ζ of a `Shaper` `Value` — its `damping_ratio` field
/// (default 0 when absent / non-numeric / not a `StructureInstance`).
///
/// This is the **single source of truth** for a shaper's ζ. `build_train_for_shaper`
/// constructs each impulse train with it, and `reify-eval`'s band-sweep robustness
/// metric (`worst_case_residual_fraction`) evaluates the residual with the same
/// value (via the `reify_stdlib::shaper_damping_ratio` re-export) — so the train is
/// always swept at the very ζ it was built from, and the numeric-coercion contract
/// (`Scalar`/`Real`/`Int` → `f64`) cannot drift between the two call sites.
pub fn shaper_damping_ratio(shaper: &Value) -> f64 {
    match shaper {
        Value::StructureInstance(data) => field_f64(data, "damping_ratio", 0.0),
        _ => 0.0,
    }
}

/// Build the [`ImpulseTrain`] for a `Shaper` `Value::StructureInstance`.
///
/// Dispatches on the structure `type_name` (the eval path has no
/// `StructureRegistry`, so the nominal tag is read directly):
///
/// - `ZVShaper`  → [`ImpulseTrain::zv`]`(2π·f, ζ)` — ζ defaults to 0 (ZVShaper's
///   `.ri` default) when the `damping_ratio` field is absent.
/// - `ZVDShaper` → [`ImpulseTrain::zvd`]`(2π·f, ζ)`.
/// - `EIShaper`  → [`ImpulseTrain::ei`]`(2π·f, ζ, v_tol)`.
/// - `CascadedShaper` → recurse over the `shapers` `List<Shaper>`, build each
///   child train and fold via [`ImpulseTrain::cascade`]. **Every child must
///   resolve**: a single unrecognised child fails the whole cascade (`None`,
///   surfacing as `Value::Undef`) rather than being silently dropped — silently
///   weakening a requested shaper would be a robustness hazard on a
///   safety-relevant signal. An empty / missing list is the identity unit-impulse
///   train (a no-op shaping, per `CascadedShaper.ri`).
///
/// The Hz→rad/s conversion `ω_n = 2π·f` happens here — this is ζ's marshalling
/// boundary; `impulse_shaper`'s entire API is in angular frequency (rad/s).
///
/// Returns `None` for a non-`StructureInstance` argument or an unrecognised
/// `type_name`. `pub` (re-exported at the crate root as
/// `reify_stdlib::build_train_for_shaper`) so `reify-eval/src/trajectory_ops.rs`
/// can reach it across the crate boundary.
pub fn build_train_for_shaper(shaper: &Value) -> Option<ImpulseTrain> {
    let Value::StructureInstance(data) = shaper else {
        return None;
    };

    match data.type_name.as_str() {
        "ZVShaper" => {
            let omega_n = 2.0 * PI * field_f64(data, "target_frequency", 0.0);
            let zeta = shaper_damping_ratio(shaper);
            Some(ImpulseTrain::zv(omega_n, zeta))
        }
        "ZVDShaper" => {
            let omega_n = 2.0 * PI * field_f64(data, "target_frequency", 0.0);
            let zeta = shaper_damping_ratio(shaper);
            Some(ImpulseTrain::zvd(omega_n, zeta))
        }
        "EIShaper" => {
            let omega_n = 2.0 * PI * field_f64(data, "target_frequency", 0.0);
            let zeta = shaper_damping_ratio(shaper);
            let v_tol = field_f64(data, "vibration_tolerance", 0.0);
            Some(ImpulseTrain::ei(omega_n, zeta, v_tol))
        }
        "CascadedShaper" => {
            // Recurse over the child shapers. EVERY child must resolve: collecting
            // into Option<Vec<_>> short-circuits to None if any child is None, so
            // `?` fails the whole cascade rather than silently dropping a child and
            // returning a weaker shaper. A missing / non-List `shapers` field is the
            // empty cascade (→ identity unit impulse); an explicit empty list folds
            // to the same identity.
            let trains: Vec<ImpulseTrain> = match data.fields.get(&"shapers".to_string()) {
                Some(Value::List(items)) => {
                    items.iter().map(build_train_for_shaper).collect::<Option<Vec<_>>>()?
                }
                _ => Vec::new(),
            };
            Some(ImpulseTrain::cascade(&trains))
        }
        _ => None,
    }
}

/// Build the canonical single-DOF gantry stand-in model for a `TOTSShaper`.
///
/// This is the θ-deferred placeholder: full Value marshalling of the
/// `TOTSShaper`'s `modes` (`List<Mode>`) and `actuator_limits`
/// (`List<JointLimit>`) into a multi-mode `TotsModel` waits until the
/// Profile↔spline `Value` marshalling (`evaluate_profile`) is unblocked.
///
/// The canonical model mirrors the gantry fixture in `tots.rs` tests:
/// * 1-DOF mechanism, 1 kg mass, X-axis translation subspace.
/// * 1-mode modal model with a 10 Hz representative mode, ζ = 0.01.
/// * 1 effector location with unit participation coefficient.
fn canonical_tots_model() -> TotsModel {
    let link = LinkDesc {
        parent_to_child: SpatialTransform6::from_frame3(&Frame3::identity()),
        subspace: vec![SpatialVector6::from_array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0])],
        mass: 1.0,
        com: [0.0; 3],
        inertia_about_com: [[0.0; 3]; 3],
    };
    TotsModel {
        mechanism: MechanismModel { links: vec![link] },
        modal: ModalModel {
            modes: vec![ModeDesc {
                freq_hz: 10.0,
                zeta: 0.01,
                force_projection: vec![1.0],
            }],
        },
        effector_locations: vec![EffectorLocation { mode_coeffs: vec![1.0] }],
    }
}

/// Run the TOTS SQP loop for a `TOTSShaper`, using the readable scalar fields
/// of `shaper_data` to parameterise a canonical single-DOF P2P stand-in.
///
/// Returns the [`TotsOutcome`] so the caller can map
/// `ConstraintInfeasible` → `Value::Undef` and
/// `Converged` / `NonConvergence` → profile echo.
///
/// Full Value marshalling of the profile waypoints and the shaper's
/// `modes` / `actuator_limits` fields into the solver is θ-deferred.
fn run_tots(shaper_data: &StructureInstanceData) -> TotsOutcome {
    let vel_limit = field_f64(shaper_data, "velocity_limit", 100.0);
    let acc_limit = field_f64(shaper_data, "acceleration_limit", 1000.0);
    let vib_tol = field_f64(shaper_data, "vibration_tolerance", 0.02);
    let max_iters = field_f64(shaper_data, "max_iters", 100.0) as usize;
    let tol = field_f64(shaper_data, "tol", 1e-6);

    let params = TotsParams {
        joints: vec![JointWaypoints {
            start: 0.0,
            interior: vec![0.5],
            end: 1.0,
            vel_limit,
            acc_limit,
            max_force: 1000.0,
        }],
        t_initial: 3.0,
        vib_tol,
        n_grid: 30,
    };
    let model = canonical_tots_model();
    let config = SqpConfig {
        max_iters,
        tol,
        ..Default::default()
    };
    solve_tots(params, &model, &config).outcome
}

/// Evaluate `input_shape(profile, shaper)` — the thin `eval_trajectory`
/// dispatch arm (wired for both the `input_shape` and `input_shape_apply`
/// names; see [`crate::trajectory::eval_trajectory`]).
///
/// Argument contract — any deviation returns [`Value::Undef`] (the stdlib
/// bad-args convention, mirroring [`super::gcode_import::eval_gcode_import`]):
/// - exactly two arguments `(profile, shaper)`;
/// - both must be a [`Value::StructureInstance`];
/// - the shaper must resolve to an [`ImpulseTrain`] via
///   [`build_train_for_shaper`] (ZV/ZVD/EI/Cascaded), or be a `TOTSShaper`
///   whose SQP run is feasible. Any other shaper or infeasible TOTS problem
///   returns `Value::Undef`.
///
/// **Dispatch order**: `TOTSShaper` is checked FIRST (λ arm), because
/// `build_train_for_shaper` returns `None` for it and would otherwise
/// immediately return `Value::Undef`. Impulse arms are structurally unchanged.
///
/// On success the shaped `Profile` is returned as a registry-free
/// [`Value::StructureInstance`] that **echoes the input profile's own**
/// [`StructureInstanceData`] — its existing `type_id` (so the value binds
/// cleanly into a typed `Profile` cell whose `type_id` the engine may validate
/// against the `StructureRegistry`), `type_name` (`"PiecewisePolynomialProfile"`),
/// `version`, and `fields`. Command-waveform resampling to new waypoints (via
/// `train.trailing_time` / `convolve_at`) is deferred to task θ — at ζ/λ the
/// Profile↔spline `Value` marshalling (`evaluate_profile`) is still a stub, so a
/// fully sample-evaluable shaped profile cannot be produced yet; echoing keeps
/// the result type-correct and the shaping observable now.
pub(crate) fn eval_input_shape(args: &[Value]) -> Value {
    // Arity guard: exactly (profile, shaper).
    let [profile, shaper] = args else {
        return Value::Undef;
    };
    // Both arguments must be StructureInstances.
    let Value::StructureInstance(profile_data) = profile else {
        return Value::Undef;
    };
    let Value::StructureInstance(shaper_data) = shaper else {
        return Value::Undef;
    };

    // ── λ: TOTSShaper arm — dispatch BEFORE impulse-train check ─────────────
    // build_train_for_shaper returns None for TOTSShaper (it only knows
    // ZV/ZVD/EI/Cascaded); placing this arm first prevents a TOTSShaper from
    // being mis-rejected as an unknown shaper.
    if shaper_data.type_name == "TOTSShaper" {
        return match run_tots(shaper_data) {
            // ConstraintInfeasible → Undef (no feasible shaped profile exists;
            // surfaces E_TrajectoryConstraintInfeasible semantics).
            TotsOutcome::ConstraintInfeasible => Value::Undef,
            // Converged / NonConvergence → echo the profile stand-in.
            // NonConvergence returns the solver's best feasible iterate (PRD:
            // W_TrajectorySolverNonConvergence "returned best feasible iterate"),
            // which is a valid shaped profile; echoing like Converged because
            // command re-waypointing is θ-deferred.
            TotsOutcome::Converged | TotsOutcome::NonConvergence => {
                Value::StructureInstance(profile_data.clone())
            }
        };
    }

    // ── ζ: impulse-train arms (ZV/ZVD/EI/Cascaded) ─────────────────────────
    // A valid, recognised shaper is required: build (and validate) its impulse
    // train, returning Undef when the shaper is unknown / unsupported. The train
    // itself is not yet stored on the result (waveform resampling is θ's job);
    // computing it here is the meaningful dispatch + bad-shaper rejection.
    if build_train_for_shaper(shaper).is_none() {
        return Value::Undef;
    }
    // Shaped Profile stand-in: echo the input profile's StructureInstanceData
    // verbatim (preserving its registered type_id — NOT a u32::MAX/0 sentinel).
    Value::StructureInstance(profile_data.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};
    use std::f64::consts::PI;

    // ── builders ───────────────────────────────────────────────────────────

    /// Build a `Shaper` `Value::StructureInstance` with the given `type_name`
    /// and String-keyed fields, exactly as the eval path receives it. The
    /// `type_id` is irrelevant to `build_train_for_shaper` (which routes on
    /// `type_name`), so a registry-free sentinel is used.
    fn shaper(type_name: &str, fields: Vec<(&str, Value)>) -> Value {
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

    /// A `target_frequency` field: a Frequency-dimensioned scalar at `hz` Hz
    /// (the SI magnitude of a `Frequency` is Hz; ζ converts to rad/s).
    fn freq(hz: f64) -> (&'static str, Value) {
        (
            "target_frequency",
            Value::Scalar {
                si_value: hz,
                dimension: DimensionVector::FREQUENCY,
            },
        )
    }

    /// Assert two `(time, amplitude)` point-lists are equal within 1e-12.
    fn assert_points_close(actual: &[(f64, f64)], expected: &[(f64, f64)], label: &str) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{label}: impulse count — got {actual:?}, want {expected:?}"
        );
        for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (a.0 - e.0).abs() < 1e-12,
                "{label}: impulse[{i}] time {} vs {}",
                a.0,
                e.0
            );
            assert!(
                (a.1 - e.1).abs() < 1e-12,
                "{label}: impulse[{i}] amplitude {} vs {}",
                a.1,
                e.1
            );
        }
    }

    // ── ZVShaper → 2-impulse train (Hz→rad/s) ────────────────────────────────

    /// ZVShaper(10Hz, ζ=0) → 2 impulses at [0, π/ω_n] with amplitudes [0.5, 0.5]
    /// where ω_n = 2π·10. Asserting t₁ = π/(2π·10) = 0.05 s (NOT π/10 ≈ 0.314)
    /// pins the Hz→rad/s conversion at the marshalling boundary.
    #[test]
    fn zv_shaper_builds_two_impulse_train_in_rad_per_sec() {
        let zv = shaper(
            "ZVShaper",
            vec![freq(10.0), ("damping_ratio", Value::Real(0.0))],
        );
        let train = build_train_for_shaper(&zv).expect("ZVShaper → Some(train)");
        let omega_n = 2.0 * PI * 10.0;
        assert_points_close(
            &train.points(),
            &[(0.0, 0.5), (PI / omega_n, 0.5)],
            "ZVShaper(10Hz, ζ=0)",
        );
    }

    /// ZVShaper's `damping_ratio` carries a `.ri` default (0.0); a marshalled
    /// value may omit the field, so `build_train_for_shaper` must default ζ→0
    /// rather than returning `None`.
    #[test]
    fn zv_shaper_damping_ratio_defaults_to_zero_when_absent() {
        let zv = shaper("ZVShaper", vec![freq(10.0)]);
        let train = build_train_for_shaper(&zv).expect("ZVShaper (no ζ field) → Some");
        let omega_n = 2.0 * PI * 10.0;
        assert_points_close(
            &train.points(),
            &[(0.0, 0.5), (PI / omega_n, 0.5)],
            "ZVShaper default ζ→0",
        );
    }

    // ── ZVDShaper → 3-impulse train (reads ζ) ────────────────────────────────

    /// ZVDShaper(10Hz, ζ=0.1) → 3 impulses matching `ImpulseTrain::zvd(2π·10,
    /// 0.1)`. A ζ=0 reference would NOT match, so this pins that
    /// `damping_ratio` is actually read from the field.
    #[test]
    fn zvd_shaper_builds_three_impulse_train_reading_damping_ratio() {
        let zeta = 0.1;
        let zvd = shaper(
            "ZVDShaper",
            vec![freq(10.0), ("damping_ratio", Value::Real(zeta))],
        );
        let train = build_train_for_shaper(&zvd).expect("ZVDShaper → Some");
        let pts = train.points();
        assert_eq!(pts.len(), 3, "ZVD has exactly 3 impulses");
        let reference = ImpulseTrain::zvd(2.0 * PI * 10.0, zeta).points();
        assert_points_close(&pts, &reference, "ZVDShaper(10Hz, ζ=0.1)");
    }

    // ── EIShaper → 4-impulse train ────────────────────────────────────────────

    /// EIShaper(10Hz, ζ=0, vtol=0.05) → 4 impulses matching
    /// `ImpulseTrain::ei(2π·10, 0, 0.05)`.
    #[test]
    fn ei_shaper_builds_four_impulse_train() {
        let ei = shaper(
            "EIShaper",
            vec![
                freq(10.0),
                ("damping_ratio", Value::Real(0.0)),
                ("vibration_tolerance", Value::Real(0.05)),
            ],
        );
        let train = build_train_for_shaper(&ei).expect("EIShaper → Some");
        let pts = train.points();
        assert_eq!(pts.len(), 4, "EI (2-hump) has exactly 4 impulses");
        let reference = ImpulseTrain::ei(2.0 * PI * 10.0, 0.0, 0.05).points();
        assert_points_close(&pts, &reference, "EIShaper(10Hz, ζ=0, vtol=0.05)");
    }

    // ── CascadedShaper → fold ────────────────────────────────────────────────

    /// CascadedShaper([zv, zv]) folds to the ZVD train at the same (ω, ζ)
    /// (cascade(ZV, ZV) ≡ ZVD), exercising the recursive child-train dispatch.
    #[test]
    fn cascaded_zv_zv_folds_to_zvd() {
        let zv = || {
            shaper(
                "ZVShaper",
                vec![freq(10.0), ("damping_ratio", Value::Real(0.0))],
            )
        };
        let cascade = shaper(
            "CascadedShaper",
            vec![("shapers", Value::List(vec![zv(), zv()]))],
        );
        let train = build_train_for_shaper(&cascade).expect("CascadedShaper([zv,zv]) → Some");
        let reference = ImpulseTrain::zvd(2.0 * PI * 10.0, 0.0).points();
        assert_points_close(&train.points(), &reference, "CascadedShaper([zv,zv]) ≡ zvd");
    }

    /// CascadedShaper([]) is the identity: a single unit impulse {(0, 1)}
    /// (convolving with nothing is a no-op, per CascadedShaper.ri).
    #[test]
    fn cascaded_empty_is_identity_unit_impulse() {
        let cascade = shaper("CascadedShaper", vec![("shapers", Value::List(vec![]))]);
        let train =
            build_train_for_shaper(&cascade).expect("CascadedShaper([]) → Some(identity)");
        assert_points_close(&train.points(), &[(0.0, 1.0)], "CascadedShaper([]) identity");
    }

    /// A CascadedShaper with ANY unresolved child fails the whole cascade
    /// (→ `None` → `eval_input_shape` `Undef`) rather than silently dropping the
    /// bad child and returning a weaker shaper — a requested shaper must not
    /// quietly degrade on this safety-relevant signal.
    #[test]
    fn cascaded_with_unresolved_child_is_none() {
        let good = shaper("ZVShaper", vec![freq(10.0)]);
        let bad = shaper("FooShaper", vec![freq(10.0)]); // unknown type_name → None
        let cascade = shaper(
            "CascadedShaper",
            vec![("shapers", Value::List(vec![good, bad]))],
        );
        assert!(
            build_train_for_shaper(&cascade).is_none(),
            "a cascade with any unresolved child → None (not a silently-weakened shaper)"
        );
    }

    // ── TOTSShaper builders ───────────────────────────────────────────────────

    /// Build a minimal `PiecewisePolynomialProfile` `Value::StructureInstance`.
    /// Registry-free sentinel type_id; `type_name` is what `eval_input_shape`
    /// echoes back. Fields omitted — the dispatcher only reads type_name/type_id.
    fn profile() -> Value {
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX - 1),
            type_name: "PiecewisePolynomialProfile".to_string(),
            version: 1,
            fields: PersistentMap::default(),
        }))
    }

    /// Build a `TOTSShaper` `Value::StructureInstance` with the given fields,
    /// exactly as the eval path receives it from the compiled `.ri` output.
    fn tots_shaper(fields: Vec<(&str, Value)>) -> Value {
        let fields: PersistentMap<String, Value> = fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "TOTSShaper".to_string(),
            version: 1,
            fields,
        }))
    }

    // ── TOTSShaper arm ────────────────────────────────────────────────────────

    /// A feasible TOTSShaper should cause `eval_input_shape` to echo the profile.
    /// Fails today because `build_train_for_shaper` returns None for TOTSShaper
    /// → `eval_input_shape` returns `Value::Undef` (the TOTS arm is not yet
    /// wired).
    #[test]
    fn tots_shaper_feasible_echoes_profile() {
        let p = profile();
        let s = tots_shaper(vec![
            ("velocity_limit", Value::Real(300.0)),
            ("acceleration_limit", Value::Real(5000.0)),
            ("vibration_tolerance", Value::Real(0.02)),
            ("max_iters", Value::Int(100)),
            ("tol", Value::Real(1e-6)),
        ]);
        let result = eval_input_shape(&[p, s]);
        match result {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.type_name, "PiecewisePolynomialProfile",
                    "eval_input_shape with feasible TOTSShaper should echo the profile \
                     (type_name = PiecewisePolynomialProfile), got: {:?}",
                    data.type_name
                );
            }
            other => panic!(
                "expected Value::StructureInstance(PiecewisePolynomialProfile) for feasible \
                 TOTSShaper, got {other:?} — TOTS arm not yet wired in eval_input_shape"
            ),
        }
    }

    /// An infeasible TOTSShaper (velocity_limit = 0 on a nonzero P2P) must cause
    /// `eval_input_shape` to return `Value::Undef`.
    ///
    /// `velocity_limit = 0` is constructible directly as a `StructureInstance`
    /// (bypassing the `.ri` `velocity_limit > 0` ctor constraint), making it a
    /// valid test vector. `solve_tots` detects this as `ConstraintInfeasible` at
    /// iteration 1 (early-exit, per `tots.rs::sqp_infeasible_zero_velocity_limit`).
    ///
    /// Fails after step-2 because the step-2 arm returns the profile echo for
    /// ALL outcomes, including `ConstraintInfeasible`. Outcome-mapping is wired
    /// in step-4.
    #[test]
    fn tots_shaper_infeasible_returns_undef() {
        let p = profile();
        // velocity_limit = 0 → canonical P2P (start=0, end=1, nonzero) is
        // infeasible; all other params are slack positives.
        let s = tots_shaper(vec![
            ("velocity_limit", Value::Real(0.0)),
            ("acceleration_limit", Value::Real(5000.0)),
            ("vibration_tolerance", Value::Real(0.02)),
            ("max_iters", Value::Int(100)),
            ("tol", Value::Real(1e-6)),
        ]);
        assert_eq!(
            eval_input_shape(&[p, s]),
            Value::Undef,
            "eval_input_shape with velocity_limit=0 TOTSShaper should return Value::Undef \
             (ConstraintInfeasible → Undef), but outcome mapping is not yet wired"
        );
    }

    // ── bad inputs → None ─────────────────────────────────────────────────────

    /// A non-`StructureInstance` argument is not a shaper → `None`.
    #[test]
    fn non_structure_instance_is_none() {
        assert!(build_train_for_shaper(&Value::Int(5)).is_none());
        assert!(build_train_for_shaper(&Value::Real(10.0)).is_none());
        assert!(build_train_for_shaper(&Value::String("ZVShaper".to_string())).is_none());
    }

    /// A `StructureInstance` whose `type_name` is not a recognised shaper → `None`.
    #[test]
    fn unknown_type_name_is_none() {
        let bogus = shaper("FooShaper", vec![freq(10.0)]);
        assert!(
            build_train_for_shaper(&bogus).is_none(),
            "unknown shaper type_name → None"
        );
    }
}
