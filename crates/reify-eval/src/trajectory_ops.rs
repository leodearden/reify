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
/// STUB (prereq-2): always returns `1.0` (the unshaped baseline — a stub shaper
/// is treated as providing no suppression). The real band sweep over
/// `reify_stdlib::build_train_for_shaper` + `ImpulseTrain::residual_vibration`
/// is implemented in step-8 and exercised by the step-7 unit tests.
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
    let _ = (shaper, f_lo_hz, f_hi_hz, n_samples);
    1.0
}
