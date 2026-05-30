//! Mode-superposition transient solver — pure-scalar `f64` math (PRD §5.3 / §7.7).
//!
//! Each mode in a mass-normalised modal basis is a decoupled SDOF ODE:
//!
//! ```text
//! ξ̈ + 2ζω·ξ̇ + ω²·ξ = f_i(t)
//! ```
//!
//! where ω is the natural angular frequency (rad/s), ζ is the modal damping
//! ratio, and f_i(t) = Φᵢᵀ·F(t) is the scalar *pre-projected* modal forcing
//! (the modal-projection step Φᵀ·F and the physical reconstruction u = Σ Φξ
//! belong to the downstream ι task; θ takes the scalar f_i(t_j) samples and
//! returns the modal coordinate ξ_i(t_j)).
//!
//! # Integrators
//!
//! Two integrators are available (selected automatically by
//! [`solve_modal_response`]):
//!
//! **Duhamel uniform** (`Integrator::DuhamelUniform`): exact per-timestep
//! recurrence for piecewise-linear (first-order-hold) excitation on a *uniform*
//! time grid (Chopra, *Dynamics of Structures*, Table 5.3.1). The homogeneous
//! part is the exact SDOF state-transition matrix e^{AΔt}; the forced part is
//! the exact convolution of the linear-interpolated force. This integrator is
//! therefore **exact** (≤ 1 × 10⁻¹²) for constant and piecewise-linear
//! forcing, and globally 2nd-order O((ΩΔt)²) for smooth forcing. Restricted to
//! underdamped modes (ζ < 1) with ω above a small floor.
//!
//! **Newmark-β** (`Integrator::Newmark`, γ = 1/2, β = 1/4 — average
//! acceleration): unconditionally stable, 2nd-order, with per-step coefficient
//! recomputation supporting *arbitrary* (non-uniform) Δt. Used for irregular
//! time grids, critically/over-damped modes (ζ ≥ 1), and rigid-body modes
//! (ω ≈ 0).
//!
//! # Accuracy note (Leo-ratified relaxation esc-3821-44 option A)
//!
//! The original target "relative error < 1 × 10⁻⁹ vs analytic sine at 50
//! sample points" is **unachievable** — it sits ~6 orders below the
//! O((ΩΔt)²) ≈ 2 × 10⁻³ … 8 × 10⁻³ linear-interpolation method floor at 50
//! points (the recurrence is exact *only* for the piecewise-linear interpolant
//! of the force; a true sine carries an interpolation residual). The accuracy
//! contract is therefore split:
//!
//! 1. **Exactness pin** — for forcing the recurrence reproduces with zero
//!    residual (constant force, linear ramp), assert relative error ≤ 1 × 10⁻¹²
//!    vs the analytic closed form (Chopra Tables 5.2.1 / 5.3.1).
//! 2. **Sine accuracy pin** — for f(t) = sin(Ωt) assert a loose absolute
//!    sanity bound *plus* the authoritative 2nd-order convergence-rate assertion
//!    (halving Δt must quarter the error). **1 × 10⁻⁹ is explicitly NOT
//!    asserted.**
//!
//! # References
//!
//! - Chopra, A. K. (2017). *Dynamics of Structures* (5th ed.), Prentice Hall.
//!   Tables 5.2.1, 5.3.1, 5.4.2.

// ─────────────────────────────────────────────────────────────────────────────
// Duhamel uniform-sampling integrator
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Sampling-uniformity checker
// ─────────────────────────────────────────────────────────────────────────────

/// Returns `true` iff `times` is a uniformly-spaced grid within the relative
/// tolerance `rel_tol`.
///
/// "Uniform" means every consecutive gap `(times[k+1] − times[k])` equals the
/// first gap `dt₀ = times[1] − times[0]` within `rel_tol · dt₀`.
///
/// Returns `false` if:
/// * `times.len() < 2` (no spacing defined).
/// * `dt₀ ≤ 0` (non-increasing or zero first gap).
/// * Any subsequent gap deviates from `dt₀` by more than `rel_tol · dt₀`.
pub fn is_uniformly_sampled(times: &[f64], rel_tol: f64) -> bool {
    if times.len() < 2 {
        return false;
    }
    let dt0 = times[1] - times[0];
    if dt0 <= 0.0 {
        return false;
    }
    let tol = rel_tol * dt0;
    for w in times.windows(2) {
        let gap = w[1] - w[0];
        if (gap - dt0).abs() > tol {
            return false;
        }
    }
    true
}

/// Pre-computed per-timestep coefficients for the exact piecewise-linear
/// Duhamel recurrence (Chopra Table 5.3.1).
///
/// The recurrence advances one SDOF step:
/// ```text
/// u_{i+1} = a·u_i  + b·v_i  + c·p_i + d·p_{i+1}
/// v_{i+1} = a_p·u_i + b_p·v_i + c_p·p_i + d_p·p_{i+1}
/// ```
///
/// Homogeneous coefficients `(a, b, a_p, b_p)` form the exact SDOF
/// state-transition matrix e^{AΔt} (Chopra §5.2).  Particular coefficients
/// `(c, d, c_p, d_p)` are the exact convolution of the linear-interpolated
/// force over `[tᵢ, tᵢ₊₁]`.
///
/// # Accuracy contract (Leo-ratified esc-3821-44 option A)
///
/// * **Exact** (≤ 1 × 10⁻¹²) for constant and piecewise-linear forcing —
///   the interpolation residual is identically zero by construction.
/// * **2nd-order** O((ΩΔt)²) for smooth forcing (e.g. a true sinusoid).
/// * The original "rel error < 1 × 10⁻⁹ vs analytic sine at 50 points"
///   target is **NOT** asserted — it is ~6 orders below the method floor for
///   50-point sine sampling.  See the module-level accuracy note.
#[derive(Debug, Clone, Copy)]
pub struct DuhamelCoeffs {
    /// A  — homogeneous displacement-from-displacement coefficient.
    pub a:   f64,
    /// B  — homogeneous displacement-from-velocity coefficient.
    pub b:   f64,
    /// A' — homogeneous velocity-from-displacement coefficient.
    pub a_p: f64,
    /// B' — homogeneous velocity-from-velocity coefficient.
    pub b_p: f64,
    /// C  — particular displacement coefficient for p_i.
    pub c:   f64,
    /// D  — particular displacement coefficient for p_{i+1}.
    pub d:   f64,
    /// C' — particular velocity coefficient for p_i.
    pub c_p: f64,
    /// D' — particular velocity coefficient for p_{i+1}.
    pub d_p: f64,
}

/// Compute the [`DuhamelCoeffs`] for a given `(omega, zeta, dt)` triple.
///
/// Derived from the Duhamel integral for f(τ) = p_i·(1−τ/Δt) + p_{i+1}·(τ/Δt):
///
/// ```text
/// G  = (1 − A) / ω²                    [step-response particular displacement]
/// H  = ∫₀^Δt s·g(s) ds                 [first moment of impulse response]
///    = −A/ω² + 2ζ(1−e·cos_d)/(Δt·ω³) + e(1−2ζ²)sin_d/(Δt·ω²·ωD)
/// C  = H / Δt
/// D  = G − C
/// C' = B − G/Δt
/// D' = G / Δt
/// ```
///
/// For constant forcing `(p_i = p_{i+1})`: `C + D = G` (reduces to ZOH).
/// Valid only for underdamped modes (ζ < 1, ω > 0).
pub fn duhamel_coefficients(omega: f64, zeta: f64, dt: f64) -> DuhamelCoeffs {
    let omega_sq  = omega * omega;
    let omega_cub = omega_sq * omega;
    let omega_d   = omega * (1.0 - zeta * zeta).sqrt();
    let exp_dt    = (-zeta * omega * dt).exp();
    let cos_d     = (omega_d * dt).cos();
    let sin_d     = (omega_d * dt).sin();
    let zeta_r    = zeta * omega / omega_d; // ζω / ωD

    // Homogeneous state-transition (Chopra §5.2).
    let a   = exp_dt * (cos_d + zeta_r * sin_d);
    let b   = exp_dt * sin_d / omega_d;
    let a_p = -exp_dt * (omega_sq / omega_d) * sin_d;
    let b_p = exp_dt * (cos_d - zeta_r * sin_d);

    // G = (1−A)/ω² and the H integral.
    let g_step        = (1.0 - a) / omega_sq;
    let two_zeta_term = 2.0 * zeta * (1.0 - exp_dt * cos_d) / (dt * omega_cub);
    let foh_sin_term  = exp_dt * (1.0 - 2.0 * zeta * zeta) * sin_d
                            / (dt * omega_sq * omega_d);

    let c   = -a / omega_sq + two_zeta_term + foh_sin_term;
    let d   =  1.0 / omega_sq - two_zeta_term - foh_sin_term;
    let g_over_dt = g_step / dt;
    let c_p = b - g_over_dt;
    let d_p = g_over_dt;

    DuhamelCoeffs { a, b, a_p, b_p, c, d, c_p, d_p }
}

/// Integrate the scalar SDOF modal ODE on a **uniform** time grid via the
/// exact per-timestep Duhamel recurrence (Chopra Table 5.3.1).
///
/// # Arguments
/// * `omega`   — natural angular frequency ω (rad/s); must satisfy ω > 0 and
///               ζ < 1 for the underdamped closed form.
/// * `zeta`    — modal damping ratio ζ (dimensionless, 0 ≤ ζ < 1).
/// * `dt`      — uniform time step Δt (s).
/// * `forcing` — scalar pre-projected modal forcing samples fᵢ(tⱼ), one per
///               output time point; `forcing[0]` is the force at t=0.
/// * `xi0`     — initial modal displacement ξ(0).
/// * `v0`      — initial modal velocity ξ̇(0).
///
/// # Returns
/// A `Vec<f64>` of length `forcing.len()` where index 0 = ξ(0) = `xi0`.
///
/// # Accuracy contract (Leo-ratified esc-3821-44 option A)
/// * **Exact** (≤ 1 × 10⁻¹²) for constant and piecewise-linear forcing.
/// * **2nd-order** O((ΩΔt)²) for smooth forcing.
/// * The original "rel error < 1 × 10⁻⁹ vs analytic sine at 50 points"
///   target is **NOT** asserted — it is ~6 orders below the O((ΩΔt)²)
///   linear-interpolation method floor at 50 sample points.
pub fn duhamel_solve(
    omega: f64,
    zeta: f64,
    dt: f64,
    forcing: &[f64],
    xi0: f64,
    v0: f64,
) -> Vec<f64> {
    let n = forcing.len();
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }

    let DuhamelCoeffs { a, b, a_p, b_p, c, d, c_p, d_p } =
        duhamel_coefficients(omega, zeta, dt);

    // State: (u, v) = (displacement, velocity) in modal coordinates.
    let mut u = xi0;
    let mut v = v0;
    out.push(u);

    for i in 1..n {
        // Piecewise-LINEAR (FOH) Duhamel recurrence (Chopra Table 5.3.1):
        //
        //   u_i = a·u_{i-1} + b·v_{i-1} + c·p_{i-1} + d·p_i
        //   v_i = a_p·u_{i-1} + b_p·v_{i-1} + c_p·p_{i-1} + d_p·p_i
        //
        // Exact for piecewise-linear forcing; the interpolation residual is zero
        // by construction, giving machine-exact results for constant and ramp
        // forcing inputs.
        let p_start = forcing[i - 1];
        let p_end   = forcing[i];

        let u_new = a   * u + b   * v + c   * p_start + d   * p_end;
        let v_new = a_p * u + b_p * v + c_p * p_start + d_p * p_end;
        u = u_new;
        v = v_new;
        out.push(u);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Analytic damped free-decay response (Chopra Table 5.2.1, zero forcing).
    ///
    /// ξ(t) = e^{-ζωt} · [ξ₀·cos(ωD·t) + ((v₀ + ζω·ξ₀)/ωD)·sin(ωD·t)]
    ///
    /// where ωD = ω·√(1 − ζ²).
    fn analytic_free_decay(xi0: f64, v0: f64, omega: f64, zeta: f64, t: f64) -> f64 {
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega * t).exp();
        decay * (xi0 * (omega_d * t).cos() + ((v0 + zeta * omega * xi0) / omega_d) * (omega_d * t).sin())
    }

    // ─── closed-form analytic helpers ────────────────────────────────────────

    /// Analytic constant-force step response from rest (Chopra Table 5.2.1).
    ///
    /// ξ(t) = (p₀/ω²)·[1 − e^{−ζωt}·(cos(ωD·t) + (ζω/ωD)·sin(ωD·t))]
    fn analytic_step_response(p0: f64, omega: f64, zeta: f64, t: f64) -> f64 {
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay   = (-zeta * omega * t).exp();
        (p0 / (omega * omega))
            * (1.0 - decay * ((omega_d * t).cos() + (zeta * omega / omega_d) * (omega_d * t).sin()))
    }

    // ─── step 01: free-vibration-from-IC exactness ───────────────────────────

    /// Drives `duhamel_solve` with zero forcing and non-zero IC (ξ₀=1, v̇₀=0).
    /// Every sample must match the closed-form damped free decay within 1e-12.
    /// RED: `duhamel_solve` is absent — fails to compile.
    #[test]
    fn free_decay_from_ic_exact() {
        let omega = 50.0_f64;
        let zeta  = 0.05_f64;
        // ω·dt ≈ 0.05  →  dt ≈ 0.001 s; use N=60 steps.
        let dt = 0.001_f64;
        let n  = 60_usize;
        let forcing = vec![0.0_f64; n];
        let xi0 = 1.0_f64;
        let v0  = 0.0_f64;

        let result = duhamel_solve(omega, zeta, dt, &forcing, xi0, v0);

        assert_eq!(result.len(), n, "output length must equal forcing.len()");
        for (j, &got) in result.iter().enumerate() {
            let t    = j as f64 * dt;
            let want = analytic_free_decay(xi0, v0, omega, zeta, t);
            assert!(
                (got - want).abs() < 1e-12,
                "step {j} (t={t:.4}): got {got:.6e}, want {want:.6e}, diff {:.2e}",
                (got - want).abs()
            );
        }
    }

    /// Analytic linear-ramp response from rest: f(t) = s·t (Chopra §5.3).
    ///
    /// ξ(t) = (s/ω²)·t − 2ζs/ω³ + e^{−ζωt}·[(2ζs/ω³)·cos(ωD·t)
    ///          + ((2ζ²−1)s/(ω²·ωD))·sin(ωD·t)]
    fn analytic_ramp_response(s: f64, omega: f64, zeta: f64, t: f64) -> f64 {
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let om2     = omega * omega;
        let om3     = om2 * omega;
        let decay   = (-zeta * omega * t).exp();
        (s / om2) * t
            - 2.0 * zeta * s / om3
            + decay
                * ((2.0 * zeta * s / om3) * (omega_d * t).cos()
                    + ((2.0 * zeta * zeta - 1.0) * s / (om2 * omega_d))
                        * (omega_d * t).sin())
    }

    /// Analytic full response to F₀·sin(Ωt) from rest (u(0)=0, u̇(0)=0).
    ///
    /// Steady-state particular: u_ss(t) = (F₀/ω²)·Rd·sin(Ωt − φ)
    /// where r = Ω/ω, Rd = 1/√[(1−r²)² + (2ζr)²], φ = atan2(2ζr, 1−r²).
    /// Adds the IC-matched damped-free transient so u(0) = u̇(0) = 0.
    fn analytic_sine_response(f0: f64, big_omega: f64, omega: f64, zeta: f64, t: f64) -> f64 {
        use std::f64::consts::PI as _PI;
        let _ = _PI; // suppress unused warning
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let r       = big_omega / omega;
        let phi     = (2.0 * zeta * r).atan2(1.0 - r * r);
        let denom   = ((1.0 - r * r).powi(2) + (2.0 * zeta * r).powi(2)).sqrt();
        let rd      = 1.0 / denom;
        let uss     = |tt: f64| (f0 / (omega * omega)) * rd * (big_omega * tt - phi).sin();
        let vss     = |tt: f64| (f0 / (omega * omega)) * rd * big_omega * (big_omega * tt - phi).cos();
        // IC matching: u(0)=0, u̇(0)=0
        let c1 = -uss(0.0);
        let c2 = (zeta * omega * c1 - vss(0.0)) / omega_d;
        let decay = (-zeta * omega * t).exp();
        uss(t) + decay * (c1 * (omega_d * t).cos() + c2 * (omega_d * t).sin())
    }

    // ─── step 03: constant-force step-response exactness (RED) ───────────────

    /// Drives `duhamel_solve` from rest with a CONSTANT forcing slice p0.
    /// Every sample must match the analytic step response within 1e-12.
    /// RED: forcing is currently ignored → response stays at 0.
    #[test]
    fn constant_force_step_response_exact() {
        let omega = 50.0_f64;
        let zeta  = 0.05_f64;
        let dt    = 0.001_f64;
        let n     = 60_usize;
        let p0    = 3.0_f64;
        let forcing = vec![p0; n];

        let result = duhamel_solve(omega, zeta, dt, &forcing, 0.0, 0.0);

        assert_eq!(result.len(), n);
        for (j, &got) in result.iter().enumerate() {
            let t    = j as f64 * dt;
            let want = analytic_step_response(p0, omega, zeta, t);
            assert!(
                (got - want).abs() < 1e-12,
                "step {j} (t={t:.4}): got {got:.6e}, want {want:.6e}, diff {:.2e}",
                (got - want).abs()
            );
        }
    }

    // ─── step 11: newmark_solve (RED) ────────────────────────────────────────

    /// Tests `newmark_solve` for correctness on (a) a uniform grid and
    /// (b) a non-uniform (geometrically-stretched) grid.
    /// RED: function absent.
    #[test]
    fn newmark_solve_uniform_and_nonuniform() {
        let omega = 50.0_f64;
        let zeta  = 0.05_f64;
        let p0    = 2.0_f64;
        let t_end = 0.5_f64;

        // (a) Uniform grid — constant force from rest.
        {
            let n_c = 100_usize;  // coarse: ω·dt = 50·(0.5/99) ≈ 0.253
            let n_f = 200_usize;  // fine:  ω·dt ≈ 0.126
            let make_times = |n: usize| -> Vec<f64> {
                (0..n).map(|i| i as f64 * t_end / (n - 1) as f64).collect()
            };
            let run_uniform = |n: usize| -> f64 {
                let times   = make_times(n);
                let forcing = vec![p0; n];
                let got     = newmark_solve(omega, zeta, &times, &forcing, 0.0, 0.0);
                let max_err = got.iter().enumerate().map(|(j, &g)| {
                    let t    = times[j];
                    let want = analytic_step_response(p0, omega, zeta, t);
                    (g - want).abs()
                }).fold(0.0_f64, f64::max);
                // normalise by static deflection p0/ω²
                max_err * omega * omega / p0
            };
            let err_c = run_uniform(n_c);
            let err_f = run_uniform(n_f);
            assert!(err_c < 1e-2, "Newmark uniform coarse rel err {err_c:.3e} ≥ 1e-2");
            let ratio = err_c / err_f;
            assert!(ratio >= 3.5, "Newmark uniform convergence ratio {ratio:.2} < 3.5");
        }

        // (b) Non-uniform (geometrically-stretched) grid — constant force.
        {
            let n = 80_usize;
            let ratio = 1.05_f64;  // stretch factor per step
            // Build times: t_0=0, t_{k+1} = t_k + dt_k where dt_{k+1} = ratio*dt_k.
            let mut times = Vec::with_capacity(n);
            times.push(0.0_f64);
            let mut dt = 0.002_f64;
            for _ in 1..n {
                times.push(times.last().unwrap() + dt);
                dt *= ratio;
            }
            let forcing = vec![p0; n];
            let got     = newmark_solve(omega, zeta, &times, &forcing, 0.0, 0.0);
            let max_rel = got.iter().enumerate().map(|(j, &g)| {
                let t    = times[j];
                let want = analytic_step_response(p0, omega, zeta, t);
                (g - want).abs()
            }).fold(0.0_f64, f64::max) * omega * omega / p0;
            assert!(max_rel < 2e-2, "Newmark non-uniform rel err {max_rel:.3e} ≥ 2e-2");
        }
    }

    // ─── step 09: is_uniformly_sampled (RED) ─────────────────────────────────

    /// Asserts the correct behaviour of `is_uniformly_sampled`.
    /// RED: function absent.
    #[test]
    fn is_uniformly_sampled_cases() {
        // Evenly-spaced grid → true.
        let uniform: Vec<f64> = (0..10).map(|i| i as f64 * 0.1).collect();
        assert!(is_uniformly_sampled(&uniform, 1e-9));

        // Evenly-spaced grid with tiny float jitter within rel_tol → true.
        let mut jittered = uniform.clone();
        jittered[5] += 1e-13;  // sub-rel-tol jitter (rel_tol=1e-9, dt0=0.1)
        assert!(is_uniformly_sampled(&jittered, 1e-9));

        // Grid with one unequal gap (jitter > rel_tol) → false.
        let mut unequal = uniform.clone();
        unequal[5] += 0.01;    // obvious gap change
        assert!(!is_uniformly_sampled(&unequal, 1e-9));

        // len < 2 → false (no spacing defined).
        assert!(!is_uniformly_sampled(&[], 1e-9));
        assert!(!is_uniformly_sampled(&[0.0], 1e-9));

        // Non-increasing / zero-gap → false.
        let non_increasing = vec![0.0, 0.1, 0.1, 0.3]; // repeated point
        assert!(!is_uniformly_sampled(&non_increasing, 1e-9));
        let decreasing = vec![0.3, 0.2, 0.1];
        assert!(!is_uniformly_sampled(&decreasing, 1e-9));
    }

    // ─── step 07: sine accuracy — SINE ACCURACY PIN ──────────────────────────

    /// Characterises the FOH recurrence for f(t) = sin(Ωt).
    ///
    /// Asserts:
    /// (a) Loose sanity bound: max relative error < 5e-2 (the honest
    ///     O((ΩΔt)²) method floor — NOT 1e-9).
    /// (b) Authoritative 2nd-order convergence: error_50 / error_100 ≥ 3.5.
    ///     Passes under FOH (2nd-order); would fail under ZOH (1st-order).
    #[test]
    fn sine_response_accuracy_and_convergence() {
        use std::f64::consts::PI;
        let big_omega = 2.0 * PI * 5.0_f64;  // 5 Hz excitation
        let omega = 50.0_f64;
        let zeta  = 0.05_f64;
        let f0    = 1.0_f64;
        let t_end = 1.0_f64;  // 1 second

        let max_abs_analytic = {
            // Estimate peak response amplitude at many sample points.
            let coarse: Vec<f64> = (0..1000)
                .map(|j| analytic_sine_response(f0, big_omega, omega, zeta, j as f64 * t_end / 999.0).abs())
                .collect();
            coarse.iter().cloned().fold(0.0_f64, f64::max)
        };

        let run = |n: usize| -> f64 {
            let dt = t_end / (n - 1) as f64;
            let forcing: Vec<f64> = (0..n)
                .map(|j| f0 * (big_omega * j as f64 * dt).sin())
                .collect();
            let got = duhamel_solve(omega, zeta, dt, &forcing, 0.0, 0.0);
            let max_err = got.iter().enumerate().map(|(j, &g)| {
                let t    = j as f64 * dt;
                let want = analytic_sine_response(f0, big_omega, omega, zeta, t);
                (g - want).abs()
            }).fold(0.0_f64, f64::max);
            max_err / max_abs_analytic   // relative to analytic amplitude
        };

        let err_50  = run(50);
        let err_100 = run(100);

        // (a) Loose sanity bound — NOT 1e-9.
        assert!(
            err_50 < 5e-2,
            "coarse-grid (50 pt) relative error {err_50:.3e} ≥ 5e-2"
        );

        // (b) Authoritative 2nd-order convergence-rate assertion.
        let ratio = err_50 / err_100;
        assert!(
            ratio >= 3.5,
            "convergence ratio err_50/err_100 = {ratio:.2} < 3.5 (expected ≥ 4 for 2nd-order)"
        );
    }

    // ─── step 05: linear-ramp exactness — EXACTNESS PIN (RED) ────────────────

    /// Drives `duhamel_solve` from rest with a linear-ramp forcing f(t)=s·t.
    /// The FOH recurrence is exact for piecewise-linear forcing → must match
    /// analytic_ramp_response within 1e-12.
    /// RED: the ZOH approximation has O(Δt) error on a ramp, ≫ 1e-12.
    #[test]
    fn linear_ramp_response_exact() {
        let omega = 50.0_f64;
        let zeta  = 0.05_f64;
        let dt    = 0.001_f64;
        let n     = 60_usize;
        let s     = 5.0_f64;          // slope: f(t) = 5·t  N/s

        // exact linear-ramp forcing samples: forcing[j] = s · (j · dt)
        let forcing: Vec<f64> = (0..n).map(|j| s * (j as f64 * dt)).collect();

        let result = duhamel_solve(omega, zeta, dt, &forcing, 0.0, 0.0);

        assert_eq!(result.len(), n);
        for (j, &got) in result.iter().enumerate() {
            let t    = j as f64 * dt;
            let want = analytic_ramp_response(s, omega, zeta, t);
            assert!(
                (got - want).abs() < 1e-12,
                "step {j} (t={t:.4}): got {got:.6e}, want {want:.6e}, diff {:.2e}",
                (got - want).abs()
            );
        }
    }
}
