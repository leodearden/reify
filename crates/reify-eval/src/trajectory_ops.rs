//! Engine-side trajectory vibration-evaluation primitives (PRD
//! `docs/prds/v0_3/trajectory-input-shaping.md` §5.3, §11 Phase 2).
//!
//! This module is the engine-side seam for *evaluating* the vibration behaviour
//! of an input shaper, as opposed to *constructing* its impulse train (which
//! lives in `reify-stdlib`'s `input_shape` / `impulse_shaper` marshalling
//! layer). It is placed in `reify-eval` because its consumers run on the engine
//! side:
//!
//! - `simulate_trajectory` (task θ/ι) — forward command-waveform simulation that
//!   reports residual vibration of a shaped vs. unshaped move.
//! - the Time-Optimal Trajectory Shaping solver (TOTS, task κ) — which scores
//!   candidate shapers by their worst-case residual across a robustness band.
//!
//! Both reuse [`worst_case_residual_fraction`]: it builds the shaper's
//! [`ImpulseTrain`](reify_stdlib::impulse_shaper::ImpulseTrain) via the
//! re-exported `reify_stdlib::build_train_for_shaper` marshalling boundary and
//! sweeps the Singer–Seering residual-vibration metric across a frequency band,
//! returning the worst (largest) residual fraction — the quantity a robust
//! shaper must keep small under modelling error (e.g. ZVD ≤ 5 % across ±10 %,
//! EI ≤ 5 % across ±15 %).

/// Worst-case (largest) residual-vibration fraction of `shaper` swept uniformly
/// across the frequency band `[f_lo_hz, f_hi_hz]` at `n_samples` points.
///
/// A residual fraction of `0.0` is perfect cancellation; `1.0` is the unshaped
/// baseline. A robust shaper keeps the *worst* residual across its insensitivity
/// band small even as the true plant frequency drifts from the design point.
///
/// A non-`StructureInstance` / unrecognised shaper — one that
/// [`reify_stdlib::build_train_for_shaper`] cannot resolve to an
/// [`ImpulseTrain`](reify_stdlib::impulse_shaper::ImpulseTrain) — returns
/// [`f64::INFINITY`]: a shaper that does not build a valid train must never read
/// as "robust" (a small residual). The damping ratio ζ used in the residual
/// evaluation is read from the shaper's `damping_ratio` field (default 0); the
/// Hz→rad/s conversion (`ω = 2π·f`) matches `build_train_for_shaper`'s
/// marshalling boundary, so the sweep evaluates the very train the shaper builds.
///
/// `#[allow(dead_code)]`: this is an engine-side seam exposed ahead of its
/// consumers (`simulate_trajectory` θ/ι, TOTS κ) and is meanwhile exercised only
/// by the in-module unit tests, so it is written-but-never-read in a non-test
/// `cargo build`. Same "implemented ahead of wiring" suppression the trajectory
/// stdlib modules use.
#[allow(dead_code)]
pub fn worst_case_residual_fraction(
    shaper: &reify_ir::Value,
    f_lo_hz: f64,
    f_hi_hz: f64,
    n_samples: usize,
) -> f64 {
    // A shaper that does not resolve to an impulse train must never read as
    // robust — return +∞ so any "residual ≤ tolerance?" check fails for it.
    let Some(train) = reify_stdlib::build_train_for_shaper(shaper) else {
        return f64::INFINITY;
    };

    // ζ for the residual evaluation: read the shaper's `damping_ratio` field
    // (default 0 when absent / non-numeric / not a StructureInstance), mirroring
    // the numeric-field reading idiom in reify-stdlib's `input_shape` marshalling.
    let zeta = match shaper {
        reify_ir::Value::StructureInstance(data) => {
            match data.fields.get(&"damping_ratio".to_string()) {
                Some(reify_ir::Value::Scalar { si_value, .. }) => *si_value,
                Some(reify_ir::Value::Real(r)) => *r,
                Some(reify_ir::Value::Int(n)) => *n as f64,
                _ => 0.0,
            }
        }
        _ => 0.0,
    };

    // Sweep [f_lo_hz, f_hi_hz] uniformly at n_samples points, convert each Hz to
    // rad/s (ω = 2π·f), evaluate the Singer–Seering residual, and keep the worst
    // (largest) fraction — the quantity a robust shaper must hold small across
    // its insensitivity band. (n_samples ≤ 1 samples only the low edge.)
    let mut worst = 0.0_f64;
    for i in 0..n_samples {
        let frac = if n_samples > 1 {
            i as f64 / (n_samples - 1) as f64
        } else {
            0.0
        };
        let f_hz = f_lo_hz + (f_hi_hz - f_lo_hz) * frac;
        let v = train.residual_vibration(2.0 * std::f64::consts::PI * f_hz, zeta);
        if v > worst {
            worst = v;
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::worst_case_residual_fraction;
    use reify_core::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

    /// Build a `Shaper` `Value::StructureInstance` (type_name + String-keyed
    /// fields) as the engine path produces it.
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

    fn freq(hz: f64) -> (&'static str, Value) {
        (
            "target_frequency",
            Value::Scalar {
                si_value: hz,
                dimension: DimensionVector::FREQUENCY,
            },
        )
    }

    /// ZVD(10Hz, ζ=0.05) keeps residual ≤ 5 % across the ±10 % band [9, 11] Hz.
    /// ZVD zeroes both residual and its frequency-derivative at the design
    /// point, giving a flat (quadratically-small) residual whose 5 %-level
    /// insensitivity band (≈±19 %) comfortably contains ±10 % (D8). Measured via
    /// ε's `residual_vibration`.
    #[test]
    fn zvd_worst_case_residual_within_5pct_over_plus_minus_10pct() {
        let zvd = shaper(
            "ZVDShaper",
            vec![freq(10.0), ("damping_ratio", Value::Real(0.05))],
        );
        let worst = worst_case_residual_fraction(&zvd, 9.0, 11.0, 21);
        assert!(
            worst <= 0.05,
            "ZVD worst-case residual over ±10% should be ≤ 0.05, got {worst:.6}"
        );
    }

    /// EI(10Hz, ζ=0.05, vtol=0.05) keeps residual ≤ vtol across the ±15 % band
    /// [8.5, 11.5] Hz. The 2-hump EI is ≤ vtol across its insensitivity band
    /// (half-width ≈±19 % at the 5 % level, Singhose 1996), containing ±15 %.
    #[test]
    fn ei_worst_case_residual_within_tolerance_over_plus_minus_15pct() {
        let ei = shaper(
            "EIShaper",
            vec![
                freq(10.0),
                ("damping_ratio", Value::Real(0.05)),
                ("vibration_tolerance", Value::Real(0.05)),
            ],
        );
        let worst = worst_case_residual_fraction(&ei, 8.5, 11.5, 31);
        assert!(
            worst <= 0.05 + 1e-9,
            "EI worst-case residual over ±15% should be ≤ vtol (0.05), got {worst:.6}"
        );
    }

    /// Robustness ordering: a plain ZV (narrow suppression) yields a strictly
    /// larger worst-case residual than the EI over the same ±15 % band — EI
    /// trades depth for width, so it wins at the band edges.
    #[test]
    fn ei_is_more_robust_than_zv_over_plus_minus_15pct() {
        let zv = shaper(
            "ZVShaper",
            vec![freq(10.0), ("damping_ratio", Value::Real(0.05))],
        );
        let ei = shaper(
            "EIShaper",
            vec![
                freq(10.0),
                ("damping_ratio", Value::Real(0.05)),
                ("vibration_tolerance", Value::Real(0.05)),
            ],
        );
        let zv_worst = worst_case_residual_fraction(&zv, 8.5, 11.5, 31);
        let ei_worst = worst_case_residual_fraction(&ei, 8.5, 11.5, 31);
        assert!(
            zv_worst > ei_worst,
            "ZV worst-case ({zv_worst:.6}) should exceed EI worst-case \
             ({ei_worst:.6}) over ±15%"
        );
    }
}
