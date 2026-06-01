//! Pure-Rust forward-pass simulator — `simulate_trajectory_core`.
//!
//! Implements the pure-Rust f64 CORE of the forward-pass simulator described
//! in `docs/prds/v0_3/trajectory-input-shaping.md` §6.1, §11 Phase 3 θ.
//!
//! # Scope / pure-Rust layer
//!
//! Like the sibling submodules (`spline.rs`, `sampling.rs`, `impulse_shaper.rs`,
//! `gcode_import.rs`) and `modal/transient.rs`, this module is a **pure-Rust
//! f64 layer** — all inputs and outputs are plain Rust types with no
//! `reify_ir::Value` dependency.
//!
//! The following are **deferred** to the downstream Value-wiring task (π
//! ComputeNode trampoline / dedicated dispatch):
//! - `eval_trajectory` match-arm dispatch wiring
//! - Value marshalling (EndEffectorTrack Value construction)
//! - FK-snapshot integration (Value-level `snapshot`/`end_effector_pose`)
//! - `.ri` accessor bodies (`end_effector_track`, `deviation_from_nominal`,
//!   `peak_deviation`) — currently stub TODO(θ) bodies in trajectory.ri
//!
//! # Dead-code suppression
//!
//! The public(crate) API here is tested at the pure-function level ahead of
//! the π consumer that will wire it to the Value layer.  Suppress the lint
//! rather than adding a premature marshalling layer.
#![allow(dead_code)]

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compute the modal-aware integration timestep (§6.1).
///
/// dt = min(0.5 / f_max, duration / 1000)
///
/// where `f_max` is the largest finite positive frequency in `freqs_hz`.
/// If no finite positive frequency exists (empty slice, all non-positive, or all
/// non-finite) the formula falls back to `duration / 1000`.
///
/// # Degenerate inputs
///
/// * `duration ≤ 0` or non-finite → returns a safe positive floor of `1e-6` s
///   so callers always receive a finite positive dt.
/// * Non-positive or non-finite entries in `freqs_hz` are ignored.
pub(crate) fn modal_aware_dt(freqs_hz: &[f64], duration: f64) -> f64 {
    // Guard duration.
    let safe_duration = if duration.is_finite() && duration > 0.0 {
        duration
    } else {
        // Degenerate: return a safe floor.
        return 1e-6;
    };

    // Find the max finite positive frequency.
    let f_max = freqs_hz
        .iter()
        .copied()
        .filter(|&f| f.is_finite() && f > 0.0)
        .fold(f64::NEG_INFINITY, f64::max);

    let dt_duration = safe_duration / 1000.0;
    if f_max > 0.0 {
        (0.5 / f_max).min(dt_duration)
    } else {
        dt_duration
    }
}

use crate::modal::transient::{reconstruct_series, solve_modal_response};

/// Superpose modal responses to produce per-location physical vibration offsets.
///
/// For each mode `i`:
/// 1. Compute ωᵢ = 2π·freq_hz_i.
/// 2. Call `solve_modal_response(ωᵢ, ζᵢ, times, &forcing_i, 0.0, 0.0)` to get
///    the modal coordinate series ξᵢ(tⱼ).
/// 3. For each location, call `reconstruct_series(coeffs, &mode_coords)` where
///    `coeffs[i] = location_coeffs[loc][i]` to get the physical displacement.
///
/// # Arguments
/// * `times` — uniform time grid `[t₀, t₁, …]`.
/// * `modes` — per-mode `(omega_rad_s, zeta, modal_forcing_series)`.
/// * `location_coeffs` — `[n_locations][n_modes]` per-location participation
///   coefficients (Φᵢ[node] at each effector location).
///
/// # Returns
/// `[n_locations][n_times]` physical vibration displacement time series.
pub(crate) fn superpose_modes(
    times: &[f64],
    modes: &[(f64, f64, Vec<f64>)],  // (omega, zeta, forcing)
    location_coeffs: &[Vec<f64>],
) -> Vec<Vec<f64>> {
    // Solve each mode's modal coordinate series.
    let mode_coords: Vec<Vec<f64>> = modes
        .iter()
        .map(|(omega, zeta, forcing)| {
            solve_modal_response(*omega, *zeta, times, forcing, 0.0, 0.0).coords
        })
        .collect();

    // For each location, reconstruct the physical displacement by superposition.
    location_coeffs
        .iter()
        .map(|coeffs| reconstruct_series(coeffs, &mode_coords))
        .collect()
}

/// Project per-sample generalized forces onto modal shapes to produce per-mode
/// scalar modal forcing series: f_i(t_j) = Φᵢᵀ · F(t_j).
///
/// # Arguments
/// * `forces` — `[n_times][n_dofs]` — per-sample generalized force vectors.
/// * `projections` — `[n_modes][n_dofs]` — per-mode projection (modal shape)
///   coefficients.  Each entry is the projection of this mode onto the
///   generalized-force DOF space.
///
/// # Returns
/// `[n_modes][n_times]` — per-mode scalar modal forcing series.
///
/// # Graceful handling of mismatched DOF counts
/// The dot product at each time step uses the common DOF length
/// `min(forces[j].len(), projections[i].len())` — surplus entries are silently
/// ignored rather than indexing out of bounds.
pub(crate) fn forces_to_forcing_history(
    forces: &[Vec<f64>],
    projections: &[Vec<f64>],
) -> Vec<Vec<f64>> {
    let n_modes = projections.len();
    let n_times = forces.len();
    let mut result: Vec<Vec<f64>> = Vec::with_capacity(n_modes);
    for shape in projections {
        let mut series = Vec::with_capacity(n_times);
        for f_at_t in forces {
            let common_len = f_at_t.len().min(shape.len());
            let dot: f64 = f_at_t[..common_len]
                .iter()
                .zip(shape[..common_len].iter())
                .map(|(a, b)| a * b)
                .sum();
            series.push(dot);
        }
        result.push(series);
    }
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── step-5: RED — superpose_modes ───────────────────────────────────────

    /// Local analytic SDOF step-response oracle (matches transient.rs tests).
    ///
    /// ξ(t) = (p₀/ω²)·[1 − e^{−ζωt}·(cos(ωD·t) + (ζω/ωD)·sin(ωD·t))]
    fn analytic_step_response(p0: f64, omega: f64, zeta: f64, t: f64) -> f64 {
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega * t).exp();
        (p0 / (omega * omega))
            * (1.0 - decay * ((omega_d * t).cos() + (zeta * omega / omega_d) * (omega_d * t).sin()))
    }

    /// (a) SINGLE-MODE STEP: constant modal forcing p0, unit location coefficient
    ///     → reconstructed displacement matches analytic SDOF step response within 1e-9.
    #[test]
    fn superpose_modes_single_mode_step_matches_analytic() {
        let omega = 2.0 * std::f64::consts::PI * 5.0; // 5 Hz
        let zeta = 0.05_f64;
        let p0 = 1.0_f64;
        let n = 200;
        let t_end = 0.5_f64;
        let dt = t_end / (n - 1) as f64;
        let times: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
        // Constant step forcing.
        let forcing_series: Vec<f64> = vec![p0; n];

        // One mode, one location with unit coefficient.
        let modes = vec![(omega, zeta, forcing_series.clone())];
        let location_coeffs: Vec<Vec<f64>> = vec![vec![1.0]]; // 1 location × 1 mode

        let result = superpose_modes(&times, &modes, &location_coeffs);

        assert_eq!(result.len(), 1, "1 location");
        assert_eq!(result[0].len(), n, "n time samples");
        for (j, (&got, &t)) in result[0].iter().zip(times.iter()).enumerate() {
            let want = analytic_step_response(p0, omega, zeta, t);
            let diff = (got - want).abs();
            assert!(
                diff < 1e-9,
                "t={t:.4} step {j}: got {got:.6e}, want {want:.6e}, diff {diff:.2e}"
            );
        }
    }

    /// (b) ZERO forcing from rest → all-zero displacement.
    #[test]
    fn superpose_modes_zero_forcing_zero_displacement() {
        let omega = 2.0 * std::f64::consts::PI * 10.0;
        let zeta = 0.02_f64;
        let n = 50;
        let times: Vec<f64> = (0..n).map(|i| i as f64 * 0.01).collect();
        let forcing_series: Vec<f64> = vec![0.0; n];
        let modes = vec![(omega, zeta, forcing_series)];
        let location_coeffs: Vec<Vec<f64>> = vec![vec![1.0]];

        let result = superpose_modes(&times, &modes, &location_coeffs);
        assert_eq!(result.len(), 1);
        for &v in &result[0] {
            assert!(v.abs() < 1e-15, "expected zero, got {v:.2e}");
        }
    }

    /// (c) Two modes superpose additively (sum of single-mode responses).
    #[test]
    fn superpose_modes_two_modes_additive() {
        let omega1 = 2.0 * std::f64::consts::PI * 5.0;
        let omega2 = 2.0 * std::f64::consts::PI * 15.0;
        let zeta = 0.05_f64;
        let p0 = 1.0_f64;
        let n = 100;
        let dt = 0.005_f64;
        let times: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
        let f1: Vec<f64> = vec![p0; n];
        let f2: Vec<f64> = vec![p0 * 2.0; n];

        // Superpose both modes at unit location coefficient.
        let modes = vec![
            (omega1, zeta, f1.clone()),
            (omega2, zeta, f2.clone()),
        ];
        let location_coeffs: Vec<Vec<f64>> = vec![vec![1.0, 1.0]]; // 1 location × 2 modes

        let result = superpose_modes(&times, &modes, &location_coeffs);

        // Compare against sum of individual single-mode results.
        let modes_a = vec![(omega1, zeta, f1.clone())];
        let lc_a: Vec<Vec<f64>> = vec![vec![1.0]];
        let r1 = superpose_modes(&times, &modes_a, &lc_a);

        let modes_b = vec![(omega2, zeta, f2.clone())];
        let lc_b: Vec<Vec<f64>> = vec![vec![1.0]];
        let r2 = superpose_modes(&times, &modes_b, &lc_b);

        for j in 0..n {
            let want = r1[0][j] + r2[0][j];
            let got = result[0][j];
            assert!(
                (got - want).abs() < 1e-13,
                "step {j}: got {got:.6e}, want {want:.6e}"
            );
        }
    }

    // ─── step-3: RED — forces_to_forcing_history ──────────────────────────────

    /// (a) All-zero force input → every modal forcing series is all-zero.
    #[test]
    fn forces_to_forcing_history_zero_input() {
        // 3 time steps, 2 DOFs, 2 modes
        let forces: Vec<Vec<f64>> = vec![
            vec![0.0, 0.0],
            vec![0.0, 0.0],
            vec![0.0, 0.0],
        ];
        let projections: Vec<Vec<f64>> = vec![
            vec![1.0, 0.5],  // mode 0 shape
            vec![0.3, 0.7],  // mode 1 shape
        ];
        let result = forces_to_forcing_history(&forces, &projections);
        assert_eq!(result.len(), 2, "n_modes");
        for (i, series) in result.iter().enumerate() {
            assert_eq!(series.len(), 3, "mode {i}: n_times");
            for &v in series {
                assert_eq!(v, 0.0, "mode {i}: expected zero for zero input");
            }
        }
    }

    /// (b) Known single-DOF force with known projection coefficient.
    #[test]
    fn forces_to_forcing_history_known_projection() {
        // 1 DOF, 2 time steps: forces = [[2.0], [4.0]]
        // 1 mode, coefficient c=3.0 → modal forcing = [6.0, 12.0]
        let forces: Vec<Vec<f64>> = vec![vec![2.0], vec![4.0]];
        let projections: Vec<Vec<f64>> = vec![vec![3.0]];
        let result = forces_to_forcing_history(&forces, &projections);
        assert_eq!(result.len(), 1, "n_modes");
        assert_eq!(result[0].len(), 2, "n_times");
        assert!((result[0][0] - 6.0).abs() < 1e-14, "t0: expected 6.0, got {}", result[0][0]);
        assert!((result[0][1] - 12.0).abs() < 1e-14, "t1: expected 12.0, got {}", result[0][1]);
    }

    /// (c) Output shape = [n_modes][n_times].
    #[test]
    fn forces_to_forcing_history_output_shape() {
        let n_times = 7;
        let n_dofs = 3;
        let n_modes = 4;
        let forces: Vec<Vec<f64>> = (0..n_times).map(|_| vec![1.0; n_dofs]).collect();
        let projections: Vec<Vec<f64>> = (0..n_modes).map(|i| vec![i as f64 * 0.1; n_dofs]).collect();
        let result = forces_to_forcing_history(&forces, &projections);
        assert_eq!(result.len(), n_modes, "outer dim = n_modes");
        for (i, series) in result.iter().enumerate() {
            assert_eq!(series.len(), n_times, "mode {i}: inner dim = n_times");
        }
    }

    /// (d) DOF/shape length mismatch → truncate to common length, no panic.
    #[test]
    fn forces_to_forcing_history_dof_mismatch_no_panic() {
        // Force has 3 DOFs, mode shape has 2 → common = 2; result is defined, no panic.
        let forces: Vec<Vec<f64>> = vec![
            vec![1.0, 2.0, 3.0],
            vec![4.0, 5.0, 6.0],
        ];
        let projections: Vec<Vec<f64>> = vec![vec![1.0, 1.0]]; // only 2 entries
        let result = forces_to_forcing_history(&forces, &projections);
        assert_eq!(result.len(), 1, "n_modes");
        assert_eq!(result[0].len(), 2, "n_times");
        // mode 0, t=0: dot([1.0, 2.0], [1.0, 1.0]) = 3.0
        assert!((result[0][0] - 3.0).abs() < 1e-14, "t0 mismatch: {}", result[0][0]);
    }

    // ─── step-1: RED — modal_aware_dt ─────────────────────────────────────────

    /// (a) 0.5/f_max governs (high f_max, long duration).
    #[test]
    fn modal_aware_dt_governed_by_frequency() {
        // f_max=100 Hz → 0.5/100=0.005 s; duration/1000 = 10.0/1000 = 0.01 s
        // → min is 0.005
        let dt = modal_aware_dt(&[50.0, 100.0], 10.0);
        assert!((dt - 0.005).abs() < 1e-14, "expected 0.005, got {dt}");
    }

    /// (b) duration/1000 governs (low f_max, short duration).
    #[test]
    fn modal_aware_dt_governed_by_duration() {
        // f_max=1 Hz → 0.5/1=0.5 s; duration/1000 = 0.1/1000 = 0.0001 s
        // → min is 0.0001
        let dt = modal_aware_dt(&[0.5, 1.0], 0.1);
        assert!((dt - 0.0001).abs() < 1e-18, "expected 0.0001, got {dt}");
    }

    /// (c) single-mode input.
    #[test]
    fn modal_aware_dt_single_mode() {
        // f_max=20 Hz → 0.5/20=0.025 s; duration/1000 = 5.0/1000=0.005 s
        // → min is 0.005
        let dt = modal_aware_dt(&[20.0], 5.0);
        assert!((dt - 0.005).abs() < 1e-16, "expected 0.005, got {dt}");
    }

    /// (d) empty modes → falls back to duration/1000.
    #[test]
    fn modal_aware_dt_empty_modes_falls_back_to_duration() {
        let dt = modal_aware_dt(&[], 2.0);
        assert!((dt - 0.002).abs() < 1e-16, "expected 0.002, got {dt}");
    }

    /// (e) non-positive/non-finite frequencies are ignored; result stays finite > 0.
    #[test]
    fn modal_aware_dt_ignores_bad_frequencies() {
        // Only valid freq is 10.0 Hz → 0.5/10=0.05; duration/1000=1.0/1000=0.001
        // → min is 0.001
        let dt = modal_aware_dt(&[0.0, -5.0, f64::NAN, f64::INFINITY, 10.0], 1.0);
        assert!(dt > 0.0 && dt.is_finite(), "dt must be finite and positive, got {dt}");
        assert!((dt - 0.001).abs() < 1e-15, "expected 0.001, got {dt}");
    }
}
