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
