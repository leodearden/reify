//! Pure-Rust trajectory sampling bridge — `to_trajectory_samples`.
//!
//! Implements the bridge from a parametric piecewise-polynomial profile
//! ([`super::spline::MultiJointSpline`]) to a sample-based
//! `MotionTrajectory` (a `Vec<TrajectorySample>` with per-sample
//! (q, q̇, q̈) for every joint).
//!
//! PRD: docs/prds/v0_3/trajectory-input-shaping.md §4.2, §11 Phase 1 γ.
//!
//! # Scope / pure-Rust layer
//!
//! Like the sibling submodules (`spline.rs`, `impulse_shaper.rs`,
//! `gcode_import.rs`) this module is a **pure-Rust f64 layer** — all inputs
//! and outputs are plain Rust types with no `reify_ir::Value` dependency.
//! Value marshalling (Profile StructureInstance → MultiJointSpline; Time →
//! f64; samples → Value) is deferred to the consuming dispatcher θ
//! (simulate_trajectory), exactly as ζ owns the impulse-shaper marshalling.
//!
//! # Dead-code suppression
//!
//! The public(crate) API here is tested at the pure-function level ahead of
//! the θ consumer that will wire it to the Value layer.  Suppress the lint
//! rather than adding a premature marshalling layer.
#![allow(dead_code)]

use super::spline::{BoundaryCondition, CubicSpline, MultiJointSpline};

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single time-stamped sample of a multi-joint trajectory.
///
/// Each of `values`, `vels`, and `accels` has length == joint count.
#[derive(Debug, Clone)]
pub(crate) struct TrajectorySample {
    /// Sample time (seconds from profile start, t=0).
    pub(crate) t: f64,
    /// Joint positions q at time t.
    pub(crate) values: Vec<f64>,
    /// Joint velocities q̇ at time t.
    pub(crate) vels: Vec<f64>,
    /// Joint accelerations q̈ at time t.
    pub(crate) accels: Vec<f64>,
}

/// A discretised motion trajectory: a time-ordered sequence of
/// [`TrajectorySample`]s produced by [`to_trajectory_samples`].
#[derive(Debug, Clone)]
pub(crate) struct MotionTrajectory {
    pub(crate) samples: Vec<TrajectorySample>,
}

impl MotionTrajectory {
    /// Return the sample times as a `Vec<f64>`.
    pub(crate) fn times(&self) -> Vec<f64> {
        self.samples.iter().map(|s| s.t).collect()
    }

    /// Return the total duration covered by the samples (last.t - first.t),
    /// or 0.0 if there are fewer than 2 samples.
    pub(crate) fn duration(&self) -> f64 {
        if self.samples.len() < 2 {
            return 0.0;
        }
        self.samples.last().unwrap().t - self.samples.first().unwrap().t
    }

    /// Fit a per-joint clamped cubic spline back through the trajectory
    /// samples, using the sampled endpoint velocities as the clamped BC slopes.
    ///
    /// Returns `None` if there are fewer than 2 samples or zero joints.
    /// Propagates `None` from `CubicSpline::fit` on any shape mismatch.
    pub(crate) fn resample_cubic(&self) -> Option<MultiJointSpline> {
        let samples = &self.samples;
        if samples.len() < 2 {
            return None;
        }
        let n_joints = samples[0].values.len();
        if n_joints == 0 {
            return None;
        }

        // Shared knot times for all joints.
        let knots: Vec<f64> = samples.iter().map(|s| s.t).collect();

        let mut joint_splines: Vec<CubicSpline> = Vec::with_capacity(n_joints);
        for j in 0..n_joints {
            let values: Vec<f64> = samples.iter().map(|s| s.values[j]).collect();
            let start_vel = samples.first().unwrap().vels[j];
            let end_vel = samples.last().unwrap().vels[j];
            let bc = BoundaryCondition::Clamped {
                start_vel,
                end_vel,
            };
            let spline = CubicSpline::fit(&knots, &values, &bc)?;
            joint_splines.push(spline);
        }

        MultiJointSpline::new_cubic(joint_splines)
    }
}

// ── Core sampler ──────────────────────────────────────────────────────────────

/// Sample a `MultiJointSpline` uniformly at step `dt` seconds.
///
/// Produces a [`MotionTrajectory`] covering `[0, profile.duration()]` where:
/// - The first sample is at `t = 0`.
/// - Interior samples are spaced exactly `dt` apart.
/// - The last sample is always exactly at `t = profile.duration()`.
/// - Total sample count ≥ 2 for any `duration > 0`.
///
/// Returns `None` if `dt` is not finite, not positive, or the profile has
/// zero / non-finite duration.
///
/// # Assumptions
///
/// The profile is authored from `t = 0` (PRD §4.1 / §4.2 convention).
/// `MultiJointSpline` exposes `duration()` but not `start_time`; for
/// non-zero-origin profiles the caller must adjust.
pub(crate) fn to_trajectory_samples(
    spline: &MultiJointSpline,
    dt: f64,
) -> Option<MotionTrajectory> {
    // Validate dt.
    if !dt.is_finite() || dt <= 0.0 {
        return None;
    }

    let d = spline.duration();
    if !d.is_finite() || d <= 0.0 {
        return None;
    }

    // Minimum gap between the last interior grid point and the endpoint.
    // Using a dt-relative threshold (rather than an absolute constant) ensures:
    //
    // 1. No near-coincident final knots: if the last i*dt lands within
    //    `min_gap` of d we skip it and let the explicit endpoint close the
    //    interval — preventing ill-conditioned cubic fits where consecutive
    //    knot spacing approaches the SINGULAR_PIVOT threshold (~2.2e-10).
    //
    // 2. The ≥ 2 sample guarantee holds for all d > 0: t = 0 is emitted
    //    unconditionally before the loop, so even d ≤ 1e-9 trajectories
    //    contain at least the start and end samples.
    let min_gap = dt * 1e-6;

    // Always emit t = 0 (the profile start).
    let mut samples: Vec<TrajectorySample> = vec![sample_at(spline, 0.0)];

    // Emit t = dt, 2*dt, … while strictly below d - min_gap.
    let mut i: usize = 1;
    loop {
        let t = (i as f64) * dt;
        if t >= d - min_gap {
            break;
        }
        samples.push(sample_at(spline, t));
        i += 1;
    }

    // Always append the exact endpoint (d).
    // For d > 0, the start sample at 0.0 ≠ d, so no dedup is needed.
    samples.push(sample_at(spline, d));

    Some(MotionTrajectory { samples })
}

/// Build a single [`TrajectorySample`] from `spline` at time `t`.
fn sample_at(spline: &MultiJointSpline, t: f64) -> TrajectorySample {
    TrajectorySample {
        t,
        values: spline.eval(t),
        vels: spline.eval_dot(t),
        accels: spline.eval_ddot(t),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::spline::{BoundaryCondition, CubicSpline, KnotData, MultiJointSpline, QuinticSpline};

    // ─── helpers ───────────────────────────────────────────────────────────────

    fn assert_close(a: f64, b: f64, tol: f64, msg: &str) {
        assert!(
            (a - b).abs() <= tol,
            "{msg}: |{a} - {b}| = {} > {tol}",
            (a - b).abs()
        );
    }

    /// Build a 2-joint cubic MultiJointSpline on [0, T] via CubicSpline::fit
    /// with clamped BC (start/end vel = 0 for simplicity).
    fn two_joint_cubic(knots: &[f64], j0_values: &[f64], j1_values: &[f64]) -> MultiJointSpline {
        let bc = BoundaryCondition::Clamped {
            start_vel: 0.0,
            end_vel: 0.0,
        };
        let s0 = CubicSpline::fit(knots, j0_values, &bc).expect("j0 spline fit");
        let s1 = CubicSpline::fit(knots, j1_values, &bc).expect("j1 spline fit");
        MultiJointSpline::new_cubic(vec![s0, s1]).expect("multi-joint cubic")
    }

    /// Closed-form cubic polynomial p(t) = 1 + 2t - 0.5t² + 0.3t³
    fn cubic_p(t: f64) -> f64 {
        1.0 + 2.0 * t - 0.5 * t * t + 0.3 * t * t * t
    }
    fn cubic_dp(t: f64) -> f64 {
        2.0 - t + 0.9 * t * t
    }
    fn cubic_ddp(t: f64) -> f64 {
        -1.0 + 1.8 * t
    }

    /// Closed-form quintic polynomial q(t) = 1 + t + t² + t³ - 0.5t⁴ + 0.1t⁵
    fn quintic_q(t: f64) -> f64 {
        1.0 + t + t * t + t * t * t - 0.5 * t.powi(4) + 0.1 * t.powi(5)
    }
    fn quintic_dq(t: f64) -> f64 {
        1.0 + 2.0 * t + 3.0 * t * t - 2.0 * t.powi(3) + 0.5 * t.powi(4)
    }
    fn quintic_ddq(t: f64) -> f64 {
        2.0 + 6.0 * t - 6.0 * t * t + 2.0 * t.powi(3)
    }

    // ─── step-1: RED — core uniform-grid sampling ──────────────────────────────

    #[test]
    fn uniform_grid_divisible_dt() {
        // T=4.0, dt=0.5 → 9 samples at t=0, 0.5, 1.0, …, 4.0
        let t_cap = 4.0_f64;
        let dt = 0.5_f64;
        let knots = vec![0.0, 1.0, 2.5, t_cap];
        let j0_vals: Vec<f64> = knots.iter().map(|&t| cubic_p(t)).collect();
        let j1_vals: Vec<f64> = knots.iter().map(|&t| 2.0 * cubic_p(t)).collect();
        let spline = two_joint_cubic(&knots, &j0_vals, &j1_vals);

        let traj = to_trajectory_samples(&spline, dt).expect("should return Some");

        let expected_count = ((t_cap / dt).round() as usize) + 1; // 9
        assert_eq!(
            traj.samples.len(),
            expected_count,
            "sample count should be T/dt + 1"
        );

        // Grid correctness: first==0, last==T, consecutive diffs==dt.
        let times = traj.times();
        assert_close(times[0], 0.0, 1e-12, "first sample t");
        assert_close(*times.last().unwrap(), t_cap, 1e-12, "last sample t");
        for w in times.windows(2) {
            assert_close(w[1] - w[0], dt, 1e-12, "consecutive dt");
        }

        // Each sample carries the correct q/q̇/q̈ from the spline.
        for s in &traj.samples {
            assert_eq!(s.values.len(), 2, "values len == joint count");
            assert_eq!(s.vels.len(), 2, "vels len == joint count");
            assert_eq!(s.accels.len(), 2, "accels len == joint count");
            let eval_q = spline.eval(s.t);
            let eval_dq = spline.eval_dot(s.t);
            let eval_ddq = spline.eval_ddot(s.t);
            for j in 0..2 {
                assert_close(s.values[j], eval_q[j], 1e-12, "values match spline.eval");
                assert_close(s.vels[j], eval_dq[j], 1e-12, "vels match spline.eval_dot");
                assert_close(s.accels[j], eval_ddq[j], 1e-12, "accels match spline.eval_ddot");
            }
        }

        // duration() and times() accessors.
        assert_close(traj.duration(), t_cap, 1e-12, "MotionTrajectory::duration");
    }

    // ─── step-3: RED — endpoint inclusion for non-divisible dt ────────────────

    #[test]
    fn endpoint_included_when_dt_does_not_divide_duration() {
        // T=1.0, dt=0.3 → floor(1.0/0.3)=3 interior points + 1 endpoint = 4 samples
        let t_cap = 1.0_f64;
        let dt = 0.3_f64;
        let knots = vec![0.0, 0.5, t_cap];
        let j0_vals: Vec<f64> = knots.iter().map(|&t| cubic_p(t)).collect();
        let j1_vals: Vec<f64> = knots.iter().map(|&t| -cubic_p(t)).collect();
        let spline = two_joint_cubic(&knots, &j0_vals, &j1_vals);

        let traj = to_trajectory_samples(&spline, dt).expect("should return Some");
        let times = traj.times();

        // floor(T/dt) = 3, but last 3*dt=0.9 ≠ T, so we need +1 endpoint → 4+1? Wait:
        // Interior grid: t=0, 0.3, 0.6, 0.9 (4 points, all < 1.0), then append 1.0.
        // Total = 5 samples.
        let expected_count = (t_cap / dt).floor() as usize + 2; // floor(1.0/0.3)+2 = 3+2 = 5
        assert_eq!(
            traj.samples.len(),
            expected_count,
            "sample count == floor(T/dt)+2"
        );

        // First t == 0.
        assert_close(times[0], 0.0, 1e-12, "first sample t");
        // Last t == T exactly.
        assert_close(*times.last().unwrap(), t_cap, 1e-12, "last sample t == T");
        // Interior consecutive diffs == dt (within 1e-12) except the final interval.
        for i in 0..times.len() - 2 {
            assert_close(times[i + 1] - times[i], dt, 1e-12, "interior dt spacing");
        }
        // Final interval > 0 and ≤ dt.
        let last_interval = times[times.len() - 1] - times[times.len() - 2];
        assert!(last_interval > 0.0, "final interval > 0");
        assert!(last_interval <= dt + 1e-12, "final interval ≤ dt");
        // Strictly increasing.
        for w in times.windows(2) {
            assert!(w[1] > w[0], "strictly increasing");
        }
    }

    // ─── step-5: RED — invalid dt rejection ────────────────────────────────────

    #[test]
    fn invalid_dt_returns_none() {
        let knots = vec![0.0, 1.0, 2.0];
        let vals = vec![1.0, 2.0, 1.5];
        let bc = BoundaryCondition::Clamped {
            start_vel: 0.0,
            end_vel: 0.0,
        };
        let s = CubicSpline::fit(&knots, &vals, &bc).unwrap();
        let spline = MultiJointSpline::new_cubic(vec![s]).unwrap();

        assert!(
            to_trajectory_samples(&spline, 0.0).is_none(),
            "dt=0 must return None"
        );
        assert!(
            to_trajectory_samples(&spline, -0.5).is_none(),
            "dt<0 must return None"
        );
        assert!(
            to_trajectory_samples(&spline, f64::NAN).is_none(),
            "dt=NAN must return None"
        );
        assert!(
            to_trajectory_samples(&spline, f64::INFINITY).is_none(),
            "dt=Inf must return None"
        );
    }

    // ─── step-7: RED — round-trip via resample_cubic ──────────────────────────

    /// (a) EXACTNESS: original is a clamped cubic → round-trip reproduces it
    /// within 1e-9 at off-grid points.
    #[test]
    fn resample_cubic_exactness_for_cubic_original() {
        // Build a 1-joint clamped cubic reproducing p(t) on knots [0,1,2.5,4].
        let knot_times = vec![0.0, 1.0, 2.5, 4.0];
        let values: Vec<f64> = knot_times.iter().map(|&t| cubic_p(t)).collect();
        let bc = BoundaryCondition::Clamped {
            start_vel: cubic_dp(0.0),
            end_vel: cubic_dp(4.0),
        };
        let s = CubicSpline::fit(&knot_times, &values, &bc).expect("cubic fit");
        let spline = MultiJointSpline::new_cubic(vec![s]).expect("multi-joint");

        let traj = to_trajectory_samples(&spline, 0.5).expect("samples");

        // The samples themselves must carry p/p'/p'' within 1e-12.
        for s in &traj.samples {
            assert_close(s.values[0], cubic_p(s.t), 1e-12, "sample.values matches p");
            assert_close(s.vels[0], cubic_dp(s.t), 1e-12, "sample.vels matches p'");
            assert_close(s.accels[0], cubic_ddp(s.t), 1e-12, "sample.accels matches p''");
        }

        // Resample and check off-grid reproduction.
        let resampled = traj.resample_cubic().expect("resample_cubic");
        for &t_check in &[0.3, 1.2, 2.0, 3.7_f64] {
            let got = resampled.eval(t_check)[0];
            let want = cubic_p(t_check);
            assert_close(got, want, 1e-9, &format!("resampled eval at t={t_check}"));
        }
    }

    /// (b) MESH-DENSITY CONVERGENCE: original is a quintic → cubic resample
    /// converges O(h⁴) as dt halves.
    #[test]
    fn resample_cubic_convergence_for_quintic_original() {
        // Build a 1-joint quintic Hermite spline reproducing q(t) on [0, 2.5]
        // (1 segment).
        let knots_q = vec![
            KnotData {
                t: 0.0,
                value: quintic_q(0.0),
                vel: quintic_dq(0.0),
                accel: quintic_ddq(0.0),
            },
            KnotData {
                t: 2.5,
                value: quintic_q(2.5),
                vel: quintic_dq(2.5),
                accel: quintic_ddq(2.5),
            },
        ];
        let qs = QuinticSpline::fit(&knots_q).expect("quintic fit");
        let spline = MultiJointSpline::new_quintic(vec![qs]).expect("multi-joint quintic");

        // Dense off-grid evaluation points.
        let check_pts: Vec<f64> = (1..=49).map(|i| i as f64 * 2.5 / 50.0).collect();

        let max_err = |dt: f64| -> f64 {
            let traj = to_trajectory_samples(&spline, dt).expect("samples");
            let resampled = traj.resample_cubic().expect("resample_cubic");
            check_pts
                .iter()
                .map(|&t| (resampled.eval(t)[0] - quintic_q(t)).abs())
                .fold(0.0_f64, f64::max)
        };

        let dt1 = 0.125_f64;
        let dt2 = 0.0625_f64;
        let err1 = max_err(dt1);
        let err2 = max_err(dt2);

        assert!(
            err1 <= 1e-2,
            "err at dt={dt1} should be ≤ 1e-2, got {err1}"
        );
        assert!(
            err2 < err1,
            "error should decrease as dt halves: err(dt2)={err2} >= err(dt1)={err1}"
        );

        // The ratio should be ≈ 16 (2⁴) for O(h⁴) clamped-cubic convergence;
        // assert ≥ 8 to tolerate numerical variability while still catching
        // degraded convergence (O(h²) gives ratio ≈ 4, O(h) gives ratio ≈ 2).
        let ratio = err1 / err2;
        assert!(
            ratio >= 8.0,
            "expected O(h⁴) convergence: ratio err1/err2 should be ≥ 8, got {ratio:.2} \
             (err1={err1:.2e}, err2={err2:.2e})"
        );
    }

    // ─── resample_cubic degenerate early-return paths ─────────────────────────

    /// `resample_cubic` must return `None` for every documented early-exit:
    /// fewer than 2 samples, empty trajectory, and zero joints.
    #[test]
    fn resample_cubic_returns_none_for_degenerate_inputs() {
        // Single sample → fewer than 2 samples → None.
        let single = MotionTrajectory {
            samples: vec![TrajectorySample {
                t: 0.0,
                values: vec![1.0],
                vels: vec![0.0],
                accels: vec![0.0],
            }],
        };
        assert!(
            single.resample_cubic().is_none(),
            "single-sample trajectory should yield None"
        );

        // Empty trajectory → fewer than 2 samples → None.
        let empty = MotionTrajectory { samples: vec![] };
        assert!(
            empty.resample_cubic().is_none(),
            "empty trajectory should yield None"
        );

        // Two samples but zero joints (values is empty) → None.
        let zero_joints = MotionTrajectory {
            samples: vec![
                TrajectorySample { t: 0.0, values: vec![], vels: vec![], accels: vec![] },
                TrajectorySample { t: 1.0, values: vec![], vels: vec![], accels: vec![] },
            ],
        };
        assert!(
            zero_joints.resample_cubic().is_none(),
            "zero-joint trajectory should yield None"
        );
    }
}
