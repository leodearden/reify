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
}
