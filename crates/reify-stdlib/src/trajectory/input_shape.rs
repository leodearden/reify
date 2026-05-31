//! `input_shape(profile, shaper)` dispatcher + Profile/Shaper `Value`
//! marshalling for the trajectory stdlib module (PRD
//! `docs/prds/v0_3/trajectory-input-shaping.md` §5.3, §11 Phase 2 task ζ).
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

use reify_ir::Value;

use super::impulse_shaper::ImpulseTrain;

/// Build the [`ImpulseTrain`] for a `Shaper` `Value::StructureInstance`.
///
/// STUB (prereq-1): always returns `None`. The real dispatch (ZVShaper /
/// ZVDShaper / EIShaper → impulse convolution; CascadedShaper → fold) is wired
/// in step-4.
///
/// `pub` (re-exported at the crate root as `reify_stdlib::build_train_for_shaper`)
/// so `reify-eval/src/trajectory_ops.rs` can reach it across the crate boundary.
pub fn build_train_for_shaper(_shaper: &Value) -> Option<ImpulseTrain> {
    None
}

/// Evaluate `input_shape(profile, shaper)`.
///
/// STUB (prereq-1): always returns [`Value::Undef`]. The real marshalling
/// (arity / `StructureInstance` guards, `build_train_for_shaper` dispatch,
/// shaped-Profile result) is wired in step-6.
///
/// `#[allow(dead_code)]`: this fn is implemented ahead of the `eval_trajectory`
/// registrar arm that calls it (step-6), so it is written-but-never-read in the
/// prereq-1/step-4 builds. Same "implemented ahead of wiring" suppression the
/// sibling `gcode_import` / `spline` / `impulse_shaper` modules use; removed
/// once the registrar arm lands.
#[allow(dead_code)]
pub(crate) fn eval_input_shape(_args: &[Value]) -> Value {
    Value::Undef
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
