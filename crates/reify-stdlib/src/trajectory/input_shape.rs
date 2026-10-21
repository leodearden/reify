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
//!    The dispatcher checks for a TOTS-family shaper (`TOTSShaper` /
//!    `RevoluteTOTSShaper`, one per joint kind — membership is decided by
//!    [`super::tots::tots_kind`], the single source of truth)
//!    BEFORE the impulse-train path (λ): when the shaper is one of those, it
//!    runs the real SQP loop
//!    ([`super::tots::solve_tots`], which calls `simulate_trajectory_core` +
//!    `inverse_dynamics_open_chain` per iteration) on a canonical single-DOF
//!    point-to-point stand-in parameterised by [`read_tots_shaper`], the one
//!    reader of a TOTS shaper's limits (also used by
//!    `trampoline::input_shape_value`). `ConstraintInfeasible` →
//!    `Value::Undef`; `Converged` / `NonConvergence` → profile echo (identical
//!    to the impulse arms). Full profile-waypoint / modes Value marshalling is
//!    θ-deferred.

use std::f64::consts::PI;

use reify_ir::arg_acceptance::{
    ArgSpec, FieldRejection, acceleration_spec, angular_acceleration_spec, angular_velocity_spec,
    dimensionless_spec, force_spec, frequency_spec, read_field, read_field_or_bare, torque_spec,
    velocity_spec,
};
use reify_ir::{StructureInstanceData, Value};

use super::impulse_shaper::ImpulseTrain;
use super::simulate::{EffectorLocation, MechanismModel, ModeDesc, ModalModel};
use super::simulate::LinkDesc;
use super::tots::{
    JointWaypoints, SqpConfig, TotsKind, TotsModel, TotsOutcome, TotsParams, solve_tots, tots_kind,
};
use crate::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};

/// The design damping ratio ζ of a `Shaper` `Value` — its `damping_ratio` field,
/// a dimensionless ratio (0 when absent, unresolved, or not a
/// `StructureInstance`). `Err` names a present `damping_ratio` that is not a
/// ratio.
///
/// This is the **single source of truth** for a shaper's ζ. `build_train_for_shaper`
/// constructs each impulse train with it, and `reify-eval`'s band-sweep robustness
/// metric (`worst_case_residual_fraction`) evaluates the residual with the same
/// value (via the `reify_stdlib::shaper_damping_ratio` re-export) — so the train is
/// always swept at the very ζ it was built from, and the reading contract cannot
/// drift between the two call sites.
pub fn shaper_damping_ratio(shaper: &Value) -> Result<f64, FieldRejection> {
    let Value::StructureInstance(data) = shaper else {
        return Ok(0.0);
    };
    Ok(read_field(data, "damping_ratio", &dimensionless_spec())?.unwrap_or(0.0))
}

/// `ω_n = 2π·f` (rad/s) of an impulse shaper's `target_frequency` `f`, a
/// FREQUENCY in Hz (0 when absent or unresolved).
///
/// This is a **D4 rad/cycle crossing**: 2π rad·cycle⁻¹ is its own crossing
/// class in the angle-dimension doctrine, distinct from the η = 1 rad
/// SI-coherence identity. The typed layer forces the distinction (FREQUENCY ≠
/// ANGULAR_VELOCITY as `DimensionVector`s) and `frequency_spec` refuses the
/// other one here, so the `f64` returned is the only place the type
/// distinction is dropped. The inverse crossing is
/// `modal::free_vibration::eigenvalue_to_frequency_hz`. See
/// `docs/prds/v0_6/angle-dimension-completion.md` D4 (#6184).
fn natural_frequency_rad_s(data: &StructureInstanceData) -> Result<f64, FieldRejection> {
    let hz = read_field(data, "target_frequency", &frequency_spec())?.unwrap_or(0.0);
    Ok(2.0 * PI * hz)
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
///   resolve**: a single unrecognised child fails the whole cascade
///   (`Ok(None)`, surfacing as `Value::Undef`) rather than being silently
///   dropped — silently weakening a requested shaper would be a robustness
///   hazard on a safety-relevant signal. An empty / missing list is the
///   identity unit-impulse train (a no-op shaping, per `CascadedShaper.ri`).
///
/// Every field is read in its declared dimension ([`natural_frequency_rad_s`]
/// for `target_frequency`, a dimensionless ratio for `damping_ratio` and
/// `vibration_tolerance`); an absent or unresolved field reads its default,
/// and a present one its dimension refuses is the `Err` — a child's refusal
/// included.
///
/// Returns `Ok(None)` for a non-`StructureInstance` argument or an
/// unrecognised `type_name`. `pub` (re-exported at the crate root as
/// `reify_stdlib::build_train_for_shaper`) so `reify-eval/src/trajectory_ops.rs`
/// can reach it across the crate boundary.
pub fn build_train_for_shaper(shaper: &Value) -> Result<Option<ImpulseTrain>, FieldRejection> {
    let Value::StructureInstance(data) = shaper else {
        return Ok(None);
    };

    let train = match data.type_name.as_str() {
        "ZVShaper" => ImpulseTrain::zv(
            natural_frequency_rad_s(data)?,
            shaper_damping_ratio(shaper)?,
        ),
        "ZVDShaper" => ImpulseTrain::zvd(
            natural_frequency_rad_s(data)?,
            shaper_damping_ratio(shaper)?,
        ),
        "EIShaper" => {
            let v_tol =
                read_field(data, "vibration_tolerance", &dimensionless_spec())?.unwrap_or(0.0);
            ImpulseTrain::ei(
                natural_frequency_rad_s(data)?,
                shaper_damping_ratio(shaper)?,
                v_tol,
            )
        }
        "CascadedShaper" => {
            let trains = match data.fields.get("shapers") {
                Some(Value::List(items)) => {
                    let children: Option<Vec<ImpulseTrain>> = items
                        .iter()
                        .map(build_train_for_shaper)
                        .collect::<Result<_, _>>()?;
                    let Some(trains) = children else {
                        return Ok(None);
                    };
                    trains
                }
                _ => Vec::new(),
            };
            ImpulseTrain::cascade(&trains)
        }
        _ => return Ok(None),
    };
    Ok(Some(train))
}

/// The dimension each TOTS limit is declared in, for one joint kind.
struct TotsLimitSpecs {
    velocity: ArgSpec,
    acceleration: ArgSpec,
    force: ArgSpec,
}

fn tots_limit_specs(kind: TotsKind) -> TotsLimitSpecs {
    match kind {
        TotsKind::Linear => TotsLimitSpecs {
            velocity: velocity_spec(),
            acceleration: acceleration_spec(),
            force: force_spec(),
        },
        TotsKind::Revolute => TotsLimitSpecs {
            velocity: angular_velocity_spec(),
            acceleration: angular_acceleration_spec(),
            force: torque_spec(),
        },
    }
}

/// One resolved `actuator_limits` entry: the force (or torque) bound of the
/// profile joint whose index is `joint`.
struct ActuatorLimit {
    joint: f64,
    max_force: f64,
}

/// The solver-parameterising fields of a TOTS-family shaper, read by
/// [`read_tots_shaper`].
pub(super) struct TotsShaperParams {
    pub(super) vel_limit: f64,
    pub(super) acc_limit: f64,
    pub(super) vib_tol: f64,
    pub(super) max_iters: usize,
    pub(super) tol: f64,
    actuator_limits: Vec<ActuatorLimit>,
}

impl TotsShaperParams {
    /// The force bound of profile joint `joint`: the `max_force` of the
    /// actuator limit whose `joint` index is `joint`, or `None` when no limit
    /// covers it — a joint that must not be solved, never one solved against a
    /// substituted bound.
    pub(super) fn max_force(&self, joint: usize) -> Option<f64> {
        self.actuator_limits
            .iter()
            .find(|limit| limit.joint == joint as f64)
            .map(|limit| limit.max_force)
    }
}

/// The ONE reader of a TOTS-family shaper's parameters, shared by both
/// dispatch sites ([`eval_input_shape`] and `trampoline::input_shape_value`).
///
/// `kind` selects the dimension of the three limits (m·s⁻¹ / m·s⁻² / N for
/// [`TotsKind::Linear`], rad·s⁻¹ / rad·s⁻² / N·m for [`TotsKind::Revolute`]);
/// `vibration_tolerance`, `max_iters`, `tol` and each limit's `joint` index
/// are dimensionless. A field absent or unresolved reads its default
/// (velocity 100, acceleration 1000, vibration tolerance 0.02, 100
/// iterations, tolerance 1e-6); an `actuator_limits` entry with an unresolved
/// field covers no joint. `Err` names the first present field its declared
/// dimension refuses, on the shaper or on one of its actuator limits.
///
/// The two kinematic limits still also take the legacy bare (SI) spelling,
/// through α's transitional `read_field_or_bare`:
/// `examples/trajectory/printer_print_envelope.ri` keeps `velocity_limit:
/// 300.0` / `acceleration_limit: 5000.0` bare by Leo's esc-5758-4 ruling until
/// #5847 dimensions them together with that file's waypoints (alone, the
/// dimensioned spelling makes its TOTS solve infeasible). A limit of the wrong
/// dimension is refused either way.
pub(super) fn read_tots_shaper(
    shaper: &StructureInstanceData,
    kind: TotsKind,
) -> Result<TotsShaperParams, FieldRejection> {
    let specs = tots_limit_specs(kind);
    let ratio = dimensionless_spec();
    Ok(TotsShaperParams {
        vel_limit: read_field_or_bare(shaper, "velocity_limit", &specs.velocity)?
            .unwrap_or(100.0),
        acc_limit: read_field_or_bare(shaper, "acceleration_limit", &specs.acceleration)?
            .unwrap_or(1000.0),
        vib_tol: read_field(shaper, "vibration_tolerance", &ratio)?.unwrap_or(0.02),
        max_iters: read_field(shaper, "max_iters", &ratio)?.unwrap_or(100.0) as usize,
        tol: read_field(shaper, "tol", &ratio)?.unwrap_or(1e-6),
        actuator_limits: read_actuator_limits(shaper, &specs.force)?,
    })
}

/// Every `actuator_limits` entry whose `joint` and `max_force` (read against
/// `force`) both resolve.
fn read_actuator_limits(
    shaper: &StructureInstanceData,
    force: &ArgSpec,
) -> Result<Vec<ActuatorLimit>, FieldRejection> {
    let Some(Value::List(entries)) = shaper.fields.get("actuator_limits") else {
        return Ok(Vec::new());
    };
    let mut limits = Vec::with_capacity(entries.len());
    for entry in entries {
        let Value::StructureInstance(limit) = entry else {
            continue;
        };
        let joint = read_field(limit, "joint", &dimensionless_spec())?;
        let max_force = read_field(limit, "max_force", force)?;
        if let (Some(joint), Some(max_force)) = (joint, max_force) {
            limits.push(ActuatorLimit { joint, max_force });
        }
    }
    Ok(limits)
}

/// Build the canonical single-DOF gantry stand-in model for a TOTS-family
/// shaper (`TOTSShaper` / `RevoluteTOTSShaper`; see
/// [`super::tots::tots_kind`]).
///
/// Joint-kind-independent by construction: the model is the same canonical
/// stand-in for both halves of the family. Only the dimension each limit is
/// read in differs per joint kind ([`read_tots_shaper`]).
///
/// This is the θ-deferred placeholder: full Value marshalling of the shaper's
/// `modes` (`List<Mode>`) into a multi-mode `TotsModel` waits until the
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

/// Run the TOTS SQP loop for a TOTS-family shaper of joint kind `kind` on a
/// canonical single-DOF P2P stand-in parameterised by
/// [`read_tots_shaper`]'s reading of `shaper_data`.
///
/// Returns the [`TotsOutcome`] so the caller can map
/// `ConstraintInfeasible` → `Value::Undef` and
/// `Converged` / `NonConvergence` → profile echo; `None` — never a solve —
/// when the shaper refuses a field or no actuator limit covers the stand-in's
/// joint 0.
///
/// Full Value marshalling of the profile waypoints and the shaper's `modes`
/// into the solver is θ-deferred.
fn run_tots(shaper_data: &StructureInstanceData, kind: TotsKind) -> Option<TotsOutcome> {
    let shaper = read_tots_shaper(shaper_data, kind).ok()?;
    let params = TotsParams {
        joints: vec![JointWaypoints {
            start: 0.0,
            interior: vec![0.5],
            end: 1.0,
            vel_limit: shaper.vel_limit,
            acc_limit: shaper.acc_limit,
            max_force: shaper.max_force(0)?,
        }],
        t_initial: 3.0,
        vib_tol: shaper.vib_tol,
        n_grid: 30,
    };
    let model = canonical_tots_model();
    let config = SqpConfig {
        max_iters: shaper.max_iters,
        tol: shaper.tol,
        ..Default::default()
    };
    Some(solve_tots(params, &model, &config).outcome)
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
///   [`build_train_for_shaper`] (ZV/ZVD/EI/Cascaded), or be a TOTS-family
///   shaper (`TOTSShaper` / `RevoluteTOTSShaper`, one per joint kind) whose
///   fields read and whose SQP run is feasible. A refused field, any other
///   shaper or an infeasible TOTS problem returns `Value::Undef` (the
///   compute-node path, `trampoline::input_shape_value`, is the one that
///   names a refused field).
///
/// **Dispatch order**: the TOTS FAMILY is checked FIRST (λ arm), because
/// `build_train_for_shaper` returns `Ok(None)` for every member and would
/// otherwise immediately return `Value::Undef`. Family membership is decided
/// by [`super::tots::tots_kind`] — the single source of truth, shared with
/// `trampoline::input_shape_value` so the two arms cannot drift.
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
    let [profile, shaper] = args else {
        return Value::Undef;
    };
    let Value::StructureInstance(profile_data) = profile else {
        return Value::Undef;
    };
    let Value::StructureInstance(shaper_data) = shaper else {
        return Value::Undef;
    };

    if let Some(kind) = tots_kind(&shaper_data.type_name) {
        return match run_tots(shaper_data, kind) {
            // NonConvergence returns the solver's best feasible iterate (PRD:
            // W_TrajectorySolverNonConvergence), a valid shaped profile.
            Some(TotsOutcome::Converged | TotsOutcome::NonConvergence) => {
                Value::StructureInstance(profile_data.clone())
            }
            Some(TotsOutcome::ConstraintInfeasible) | None => Value::Undef,
        };
    }

    match build_train_for_shaper(shaper) {
        Ok(Some(_)) => Value::StructureInstance(profile_data.clone()),
        Ok(None) | Err(_) => Value::Undef,
    }
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
        let train = build_train_for_shaper(&zv)
            .expect("a well-dimensioned shaper is accepted")
            .expect("ZVShaper → Some(train)");
        let omega_n = 2.0 * PI * 10.0;
        assert_points_close(
            &train.points(),
            &[(0.0, 0.5), (PI / omega_n, 0.5)],
            "ZVShaper(10Hz, ζ=0)",
        );
    }

    /// ZVShaper's `damping_ratio` carries a `.ri` default (0.0); a marshalled
    /// value may omit the field, so `build_train_for_shaper` must default ζ→0
    /// rather than returning `Ok(None)`.
    #[test]
    fn zv_shaper_damping_ratio_defaults_to_zero_when_absent() {
        let zv = shaper("ZVShaper", vec![freq(10.0)]);
        let train = build_train_for_shaper(&zv)
            .expect("a well-dimensioned shaper is accepted")
            .expect("ZVShaper (no ζ field) → Some");
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
        let train = build_train_for_shaper(&zvd)
            .expect("a well-dimensioned shaper is accepted")
            .expect("ZVDShaper → Some");
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
        let train = build_train_for_shaper(&ei)
            .expect("a well-dimensioned shaper is accepted")
            .expect("EIShaper → Some");
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
        let train = build_train_for_shaper(&cascade)
            .expect("a well-dimensioned shaper is accepted")
            .expect("CascadedShaper([zv,zv]) → Some");
        let reference = ImpulseTrain::zvd(2.0 * PI * 10.0, 0.0).points();
        assert_points_close(&train.points(), &reference, "CascadedShaper([zv,zv]) ≡ zvd");
    }

    /// CascadedShaper([]) is the identity: a single unit impulse {(0, 1)}
    /// (convolving with nothing is a no-op, per CascadedShaper.ri).
    #[test]
    fn cascaded_empty_is_identity_unit_impulse() {
        let cascade = shaper("CascadedShaper", vec![("shapers", Value::List(vec![]))]);
        let train = build_train_for_shaper(&cascade)
            .expect("a well-dimensioned shaper is accepted")
            .expect("CascadedShaper([]) → Some(identity)");
        assert_points_close(&train.points(), &[(0.0, 1.0)], "CascadedShaper([]) identity");
    }

    /// A CascadedShaper with ANY unresolved child fails the whole cascade
    /// (→ `Ok(None)` → `eval_input_shape` `Undef`) rather than silently dropping the
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
            matches!(build_train_for_shaper(&cascade), Ok(None)),
            "a cascade with any unresolved child → Ok(None) (not a silently-weakened shaper)"
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

    /// A TOTS-family shaper of joint kind `kind`, exactly as the eval path
    /// receives it from the compiled `.ri` output: `velocity_limit` (in the
    /// kind's velocity dimension), 5000 of the kind's acceleration, a 0.02
    /// vibration tolerance, the declared solver defaults, and a 1000-unit
    /// actuator limit (N or N·m) on joint 0.
    ///
    /// Parameterised by kind (task 6096) because the family has TWO members —
    /// `TOTSShaper` (prismatic/linear) and `RevoluteTOTSShaper`
    /// (revolute/rotational) — and the dispatch arm keys on exactly this
    /// string, while the reader reads each limit in that kind's dimension
    /// (task 6941). The magnitudes are the same for both kinds, so the two
    /// stay provably identical apart from the name and the dimensions.
    fn tots_shaper(kind: TotsKind, velocity_limit: f64) -> Value {
        let (type_name, limit_type, velocity, acceleration, force) = match kind {
            TotsKind::Linear => (
                "TOTSShaper",
                "JointLimit",
                DimensionVector::VELOCITY,
                DimensionVector::ACCELERATION,
                DimensionVector::FORCE,
            ),
            TotsKind::Revolute => (
                "RevoluteTOTSShaper",
                "RevoluteJointLimit",
                DimensionVector::ANGULAR_VELOCITY,
                DimensionVector::ANGULAR_VELOCITY.div(&DimensionVector::TIME),
                DimensionVector::TORQUE,
            ),
        };
        let scalar = |si_value, dimension| Value::Scalar {
            si_value,
            dimension,
        };
        let limit = shaper(
            limit_type,
            vec![
                ("joint", Value::Real(0.0)),
                ("max_force", scalar(1000.0, force)),
            ],
        );
        shaper(
            type_name,
            vec![
                ("velocity_limit", scalar(velocity_limit, velocity)),
                ("acceleration_limit", scalar(5000.0, acceleration)),
                ("vibration_tolerance", Value::Real(0.02)),
                ("max_iters", Value::Int(100)),
                ("tol", Value::Real(1e-6)),
                ("actuator_limits", Value::List(vec![limit])),
            ],
        )
    }

    /// A feasible TOTS-family shaper of `kind`: 300 units of velocity.
    fn feasible_tots_shaper(kind: TotsKind) -> Value {
        tots_shaper(kind, 300.0)
    }

    /// An infeasible TOTS-family shaper of `kind`: `velocity_limit = 0` on the
    /// canonical nonzero P2P model. Constructible directly as a
    /// `StructureInstance`, bypassing the `.ri` `velocity_limit > 0` ctor
    /// constraint.
    fn infeasible_tots_shaper(kind: TotsKind) -> Value {
        tots_shaper(kind, 0.0)
    }

    /// `instance` with field `key` set to `value`.
    fn with_field(instance: Value, key: &str, value: Value) -> Value {
        let Value::StructureInstance(mut data) = instance else {
            panic!("with_field: {key} must be set on a StructureInstance");
        };
        data.fields.insert(key.to_string(), value);
        Value::StructureInstance(data)
    }

    // ── TOTSShaper arm ────────────────────────────────────────────────────────

    /// A feasible TOTSShaper causes `eval_input_shape` to take the λ arm:
    /// `run_tots` → `solve_tots` → `Converged` or `NonConvergence` → echo the
    /// input profile's `StructureInstanceData` as a shaped-profile stand-in.
    /// (Command re-waypointing is θ-deferred; the echo is the type-correct
    /// stand-in at this phase, mirroring the impulse-shaper arms.)
    #[test]
    fn tots_shaper_feasible_echoes_profile() {
        let p = profile();
        let s = feasible_tots_shaper(TotsKind::Linear);
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
                 TOTSShaper, got {other:?}"
            ),
        }
    }

    /// (task 6096) The REVOLUTE member of the TOTS shaper family must take the
    /// same λ arm as the linear one.
    ///
    /// This is one of TWO independent dispatch sites keyed on `type_name` (the
    /// other is `trampoline.rs::input_shape_value`), which is why each gets its
    /// own coverage: a `RevoluteTOTSShaper` that this arm does not recognise
    /// falls through to `build_train_for_shaper` — which knows only
    /// ZV/ZVD/EI/Cascaded — gets `Ok(None)`, and returns `Value::Undef` with ZERO
    /// diagnostics. Silent Undef, not a loud failure.
    #[test]
    fn revolute_tots_shaper_feasible_echoes_profile() {
        let p = profile();
        let s = feasible_tots_shaper(TotsKind::Revolute);
        let result = eval_input_shape(&[p, s]);
        match result {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.type_name, "PiecewisePolynomialProfile",
                    "eval_input_shape with a feasible RevoluteTOTSShaper should echo the \
                     profile (type_name = PiecewisePolynomialProfile), got: {:?}",
                    data.type_name
                );
            }
            other => panic!(
                "expected Value::StructureInstance(PiecewisePolynomialProfile) for a feasible \
                 RevoluteTOTSShaper, got {other:?} — an Undef here means this dispatch arm \
                 does not recognise the revolute member of the TOTS shaper family (task 6096)"
            ),
        }
    }

    /// `velocity_limit = 0` on the canonical nonzero P2P model causes `solve_tots`
    /// to return `ConstraintInfeasible` (early-exit; see
    /// `tots.rs::sqp_infeasible_zero_velocity_limit`), which the λ arm maps to
    /// `Value::Undef` — there is no feasible shaped profile to return.
    ///
    /// `velocity_limit = 0` is constructible directly as a `StructureInstance`
    /// (bypassing the `.ri` `velocity_limit > 0` ctor constraint), making it a
    /// valid test vector for the infeasible path.
    #[test]
    fn tots_shaper_infeasible_returns_undef() {
        let p = profile();
        // velocity_limit = 0 → canonical P2P (start=0, end=1, nonzero) is
        // infeasible; all other params are slack positives.
        let s = infeasible_tots_shaper(TotsKind::Linear);
        assert_eq!(
            eval_input_shape(&[p, s]),
            Value::Undef,
            "eval_input_shape with velocity_limit=0 TOTSShaper should return Value::Undef \
             (ConstraintInfeasible → Undef)"
        );
    }

    /// (task 6096) The revolute member reaches the SAME infeasibility path.
    ///
    /// Paired with `revolute_tots_shaper_feasible_echoes_profile` above so the
    /// revolute Undef is pinned as coming from `ConstraintInfeasible` — not
    /// from the arm silently failing to recognise the type. Without both
    /// polarities, a completely unwired revolute dispatch would still satisfy
    /// the infeasible test.
    #[test]
    fn revolute_tots_shaper_infeasible_returns_undef() {
        let p = profile();
        let s = infeasible_tots_shaper(TotsKind::Revolute);
        assert_eq!(
            eval_input_shape(&[p, s]),
            Value::Undef,
            "eval_input_shape with a velocity_limit=0 RevoluteTOTSShaper should return \
             Value::Undef (ConstraintInfeasible → Undef), same as the linear half"
        );
    }

    /// A TOTS shaper this arm cannot solve honestly is `Value::Undef`, never
    /// solved against a default: a present limit of the wrong joint kind's
    /// dimension (a linear m·s⁻¹ velocity on the revolute member), and a
    /// stand-in joint 0 that no actuator limit covers.
    #[test]
    fn tots_shaper_refused_or_uncovered_returns_undef() {
        let linear_velocity = Value::Scalar {
            si_value: 300.0,
            dimension: DimensionVector::VELOCITY,
        };
        let refused = with_field(
            feasible_tots_shaper(TotsKind::Revolute),
            "velocity_limit",
            linear_velocity,
        );
        let uncovered = with_field(
            feasible_tots_shaper(TotsKind::Linear),
            "actuator_limits",
            Value::List(vec![]),
        );
        for (what, s) in [("refused", refused), ("uncovered", uncovered)] {
            assert_eq!(
                eval_input_shape(&[profile(), s]),
                Value::Undef,
                "{what} TOTS shaper → Value::Undef"
            );
        }
    }

    /// A present `target_frequency` of the wrong dimension is refused by name,
    /// a child's refusal propagating out of a `CascadedShaper`.
    #[test]
    fn angular_velocity_target_frequency_is_refused() {
        let rad_per_s = Value::Scalar {
            si_value: 50.0,
            dimension: DimensionVector::ANGULAR_VELOCITY,
        };
        let zv = shaper("ZVShaper", vec![("target_frequency", rad_per_s)]);
        let cascade = shaper(
            "CascadedShaper",
            vec![("shapers", Value::List(vec![zv.clone()]))],
        );
        for s in [zv, cascade] {
            let rejection = build_train_for_shaper(&s).expect_err("a rad/s frequency is refused");
            assert_eq!(rejection.path, "ZVShaper.target_frequency");
        }
    }

    // ── bad inputs → Ok(None) ──────────────────────────────────────────────────

    /// A non-`StructureInstance` argument is not a shaper → `None`.
    #[test]
    fn non_structure_instance_is_none() {
        assert!(matches!(build_train_for_shaper(&Value::Int(5)), Ok(None)));
        assert!(matches!(
            build_train_for_shaper(&Value::Real(10.0)),
            Ok(None)
        ));
        assert!(matches!(
            build_train_for_shaper(&Value::String("ZVShaper".to_string())),
            Ok(None)
        ));
    }

    /// A `StructureInstance` whose `type_name` is not a recognised shaper → `None`.
    #[test]
    fn unknown_type_name_is_none() {
        let bogus = shaper("FooShaper", vec![freq(10.0)]);
        assert!(
            matches!(build_train_for_shaper(&bogus), Ok(None)),
            "unknown shaper type_name → Ok(None)"
        );
    }
}
