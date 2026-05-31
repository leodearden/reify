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
// Dispatcher types and entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Which numerical integrator was selected by [`solve_modal_response`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrator {
    /// Exact per-timestep Duhamel recurrence (Chopra Table 5.3.1) for uniform
    /// time grids and underdamped (ζ < 1) modes with ω above a small floor.
    DuhamelUniform,
    /// Newmark-β average-acceleration (γ = 1/2, β = 1/4) for non-uniform grids,
    /// critically/over-damped modes (ζ ≥ 1), and rigid-body modes (ω ≈ 0).
    Newmark,
}

/// Result of [`solve_modal_response`]: modal coordinate time-history plus the
/// integrator that was selected (observable in tests and downstream diagnostics).
pub struct ModalResponse {
    /// Modal coordinate ξ(tⱼ) at each sample time; length = `times.len()`.
    pub coords: Vec<f64>,
    /// Which integrator produced `coords`.
    pub integrator: Integrator,
}

/// Floor below which ω is treated as a rigid-body mode and routed to Newmark.
const OMEGA_FLOOR: f64 = 1e-9;
/// Damping margin below the critical value ζ=1 below which Duhamel is valid.
const ZETA_CEILING: f64 = 1.0 - 1e-9;

// ─────────────────────────────────────────────────────────────────────────────
// Task λ: prepare/integrate split (cache seam)
// ─────────────────────────────────────────────────────────────────────────────
//
// These types and functions expose a forcing-independent "setup" step that the
// `reify-eval` transient-response cache (task λ) can store and reuse across
// calls that differ only in `forcing`. The split exactly mirrors
// `solve_modal_response`'s selection logic: callers that hold a
// `PreparedIntegrator` skip coefficient derivation and run only the recurrence.

/// Forcing-independent prepared integrator for one SDOF modal ODE mode.
///
/// Produced by [`prepare_modal_integrator`]; consumed by [`integrate_prepared`].
///
/// - `Duhamel { coeffs }` — the pre-derived per-timestep recurrence coefficients
///   (Chopra Table 5.3.1), valid for the specific `(ω, ζ, dt)` triple.  The
///   recurrence re-runs in O(n_times) per forcing vector with no coefficient
///   re-derivation.
/// - `Newmark { omega, zeta }` — the `(ω, ζ)` marker for modes routed to
///   Newmark (ζ ≥ 1, ω ≈ 0, or non-uniform grid); per-step coefficients are
///   re-derived from the local Δt inside [`newmark_solve`] (already O(n)).
///
/// `Copy + Clone + Debug + Send + Sync + 'static` — all fields are `f64` /
/// [`DuhamelCoeffs`] (which is `Copy`).
#[derive(Clone, Copy, Debug)]
pub enum PreparedIntegrator {
    /// Exact Duhamel recurrence for a uniform grid, underdamped mode.
    Duhamel {
        /// Pre-derived per-timestep coefficients (function of `(ω, ζ, dt)`).
        coeffs: DuhamelCoeffs,
    },
    /// Newmark-β (γ=1/2, β=1/4) for non-uniform grids, ζ≥1, or ω≈0.
    Newmark {
        /// Natural angular frequency ω (rad/s).
        omega: f64,
        /// Modal damping ratio ζ (dimensionless).
        zeta: f64,
    },
}

impl PreparedIntegrator {
    /// Which [`Integrator`] variant this prepared state will use.
    pub fn integrator(&self) -> Integrator {
        match self {
            PreparedIntegrator::Duhamel { .. } => Integrator::DuhamelUniform,
            PreparedIntegrator::Newmark { .. } => Integrator::Newmark,
        }
    }
}

/// Prepare the forcing-independent SDOF integrator for one mode, replicating
/// [`solve_modal_response`]'s integrator-selection logic.
///
/// # Selection
/// - If `times` is uniformly sampled (within 1 × 10⁻⁹ relative tolerance)
///   **and** `omega > OMEGA_FLOOR` **and** `zeta < ZETA_CEILING`: returns
///   `PreparedIntegrator::Duhamel { coeffs: duhamel_coefficients(omega, zeta, dt) }`
///   where `dt = times[1] − times[0]`.
/// - Otherwise: returns `PreparedIntegrator::Newmark { omega, zeta }`.
///
/// Returns `Newmark` for `times.len() < 2` (no spacing defined → not uniform).
///
/// The selection is identical to `solve_modal_response`'s dispatcher; the
/// resulting `PreparedIntegrator` can be passed to [`integrate_prepared`] to
/// reproduce the same output for any forcing vector on the same grid, with
/// coefficient derivation done once rather than per forcing call.
pub fn prepare_modal_integrator(omega: f64, zeta: f64, times: &[f64]) -> PreparedIntegrator {
    let use_duhamel = omega > OMEGA_FLOOR
        && zeta < ZETA_CEILING
        && is_uniformly_sampled(times, 1e-9);

    if use_duhamel {
        let dt = times[1] - times[0];
        PreparedIntegrator::Duhamel { coeffs: duhamel_coefficients(omega, zeta, dt) }
    } else {
        PreparedIntegrator::Newmark { omega, zeta }
    }
}

/// Integrate one scalar SDOF modal ODE using a pre-prepared integrator.
///
/// Routes to the appropriate recurrence based on `prep`:
/// - `Duhamel { coeffs }` → [`duhamel_solve_with_coeffs`] (no re-derivation).
/// - `Newmark { omega, zeta }` → [`newmark_solve`].
///
/// Produces the same output as `solve_modal_response` called with the same
/// `(omega, zeta, times, forcing, xi0, v0)` that produced `prep` — bit-identical
/// for a given forcing vector, by construction.
///
/// # Arguments
/// * `prep`    — the pre-prepared integrator from [`prepare_modal_integrator`].
/// * `times`   — same time grid that was passed to `prepare_modal_integrator`.
/// * `forcing` — per-timestep projected modal forcing; same length as `times`.
/// * `xi0`     — initial modal displacement ξ(t₀).
/// * `v0`      — initial modal velocity ξ̇(t₀).
pub fn integrate_prepared(
    prep: &PreparedIntegrator,
    times: &[f64],
    forcing: &[f64],
    xi0: f64,
    v0: f64,
) -> Vec<f64> {
    match prep {
        PreparedIntegrator::Duhamel { coeffs } => {
            duhamel_solve_with_coeffs(coeffs, forcing, xi0, v0)
        }
        PreparedIntegrator::Newmark { omega, zeta } => {
            newmark_solve(*omega, *zeta, times, forcing, xi0, v0)
        }
    }
}

/// Integrate one scalar SDOF modal ODE, automatically selecting the integrator.
///
/// # Selection logic
///
/// | Condition                                         | Integrator       |
/// |---------------------------------------------------|------------------|
/// | `times` uniform **and** ω > `OMEGA_FLOOR` **and** ζ < 1 − margin | `DuhamelUniform` |
/// | otherwise                                         | `Newmark`        |
///
/// The Duhamel closed form uses ω_D = ω√(1−ζ²) and divides by ω²/ω_D, which
/// is undefined for ζ ≥ 1 or ω ≈ 0; those modes always route to Newmark.
///
/// # Arguments
/// * `omega`   — natural angular frequency ω (rad/s).
/// * `zeta`    — modal damping ratio ζ (dimensionless, ≥ 0).
/// * `times`   — monotonically-increasing time sample points (s); length ≥ 1.
/// * `forcing` — pre-projected scalar modal forcing at each time point.
/// * `xi0`     — initial modal displacement ξ(t₀).
/// * `v0`      — initial modal velocity ξ̇(t₀).
pub fn solve_modal_response(
    omega: f64,
    zeta: f64,
    times: &[f64],
    forcing: &[f64],
    xi0: f64,
    v0: f64,
) -> ModalResponse {
    assert_eq!(
        times.len(),
        forcing.len(),
        "times and forcing must have equal length"
    );
    let prep = prepare_modal_integrator(omega, zeta, times);
    let coords = integrate_prepared(&prep, times, forcing, xi0, v0);
    ModalResponse { coords, integrator: prep.integrator() }
}

// ─────────────────────────────────────────────────────────────────────────────
// Newmark-β integrator (average-acceleration, γ=1/2, β=1/4)
// ─────────────────────────────────────────────────────────────────────────────

/// Integrate the scalar SDOF modal ODE on an **arbitrary** (non-uniform) time
/// grid via the Newmark-β average-acceleration method (γ = 1/2, β = 1/4,
/// Chopra Table 5.4.2).
///
/// # Arguments
/// * `omega`   — natural angular frequency ω (rad/s).
/// * `zeta`    — modal damping ratio ζ (dimensionless).  Valid for any ζ ≥ 0.
/// * `times`   — time sample points (s), monotonically increasing; length ≥ 1.
/// * `forcing` — scalar modal forcing samples at each time point; same length
///   as `times`.  `forcing[0]` is the force at `times[0]`.
/// * `xi0`     — initial modal displacement ξ(t₀).
/// * `v0`      — initial modal velocity ξ̇(t₀).
///
/// # Returns
/// A `Vec<f64>` of length `times.len()` where index 0 = `xi0`.
///
/// # Notes
/// * Unconditionally stable for any Δt.
/// * 2nd-order accurate (globally O(Δt²)) for smooth forcing.
/// * Per-step coefficients are recomputed at each step from that step's Δt,
///   supporting arbitrary non-uniform time grids.
/// * Well-defined for ω = 0 (rigid-body) and any ζ ≥ 0 (including ζ ≥ 1).
pub fn newmark_solve(
    omega: f64,
    zeta: f64,
    times: &[f64],
    forcing: &[f64],
    xi0: f64,
    v0: f64,
) -> Vec<f64> {
    assert_eq!(times.len(), forcing.len(), "times and forcing must have equal length");
    let n = times.len();
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }

    // SDOF equation:  m·ü + c·u̇ + k·u = p(t)   with m=1, c=2ζω, k=ω²
    let k = omega * omega;
    let c = 2.0 * zeta * omega;

    // Newmark parameters: average-acceleration (unconditionally stable, 2nd-order)
    let beta  = 0.25_f64;
    let gamma = 0.5_f64;

    // Initial state.
    let mut u = xi0;
    let mut v = v0;
    // Seed acceleration from equation of motion: a₀ = p₀ − c·v₀ − k·u₀
    let mut a = forcing[0] - c * v - k * u;
    out.push(u);

    for i in 1..n {
        let dt = times[i] - times[i - 1];
        let p_next = forcing[i];

        // Effective stiffness (recomputed for each Δt).
        //   k̂ = k + γ/(β·Δt)·c + 1/(β·Δt²)·m
        let khat = k
            + (gamma / (beta * dt)) * c
            + 1.0 / (beta * dt * dt);

        // Effective force at step i+1 (Chopra Table 5.4.2):
        //   p̂ = p_{i+1}
        //       + m·(a0·u_i + a2·v_i + a3·a_i)      [m = 1]
        //       + c·(a1·u_i + a4·v_i + a5·a_i)
        //
        // with  a0=1/(β·Δt²),  a1=γ/(β·Δt),  a2=1/(β·Δt),
        //       a3=1/(2β)−1,   a4=γ/β−1,      a5=Δt·(γ/(2β)−1)
        //
        // For average-acceleration (γ=1/2, β=1/4):
        //   a0=4/Δt², a1=2/Δt, a2=4/Δt, a3=1, a4=1, a5=0
        let a0 = 1.0 / (beta * dt * dt);
        let a1 = gamma / (beta * dt);
        let a2 = 1.0 / (beta * dt);
        let a3 = 1.0 / (2.0 * beta) - 1.0;
        let a4 = gamma / beta - 1.0;
        let a5 = dt * (gamma / (2.0 * beta) - 1.0);
        let phat = p_next
            + a0 * u + a2 * v + a3 * a          // mass contribution (m = 1)
            + c * (a1 * u + a4 * v + a5 * a);   // damping contribution

        // Displacement corrector.
        let u_next = phat / khat;

        // Acceleration corrector (Chopra Table 5.4.2):
        //   ü_{i+1} = a0·(u_{i+1} − u_i) − a2·u̇_i − a3·ü_i
        let a_next = a0 * (u_next - u) - a2 * v - a3 * a;

        // Velocity corrector.
        let v_next = v + (1.0 - gamma) * dt * a + gamma * dt * a_next;

        u = u_next;
        v = v_next;
        a = a_next;
        out.push(u);
    }

    out
}

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

// ─────────────────────────────────────────────────────────────────────────────
// Duhamel uniform-sampling integrator
// ─────────────────────────────────────────────────────────────────────────────

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

/// Integrate the scalar SDOF modal ODE using pre-derived [`DuhamelCoeffs`].
///
/// Identical to [`duhamel_solve`] but accepts already-computed coefficients,
/// so the caller can cache and reuse them across multiple forcing vectors
/// without re-deriving the trig/exp expression (the per-mode cache seam for
/// task λ).
///
/// # Arguments
/// * `coeffs`  — pre-derived per-timestep recurrence coefficients.
/// * `forcing` — scalar pre-projected modal forcing samples, one per output
///   time point; `forcing[0]` is the force at `t=0`.
/// * `xi0`     — initial modal displacement ξ(0).
/// * `v0`      — initial modal velocity ξ̇(0).
///
/// # Returns
/// A `Vec<f64>` of length `forcing.len()` where index 0 = ξ(0) = `xi0`.
pub fn duhamel_solve_with_coeffs(
    coeffs: &DuhamelCoeffs,
    forcing: &[f64],
    xi0: f64,
    v0: f64,
) -> Vec<f64> {
    let n = forcing.len();
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }

    let DuhamelCoeffs { a, b, a_p, b_p, c, d, c_p, d_p } = *coeffs;

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

/// Integrate the scalar SDOF modal ODE on a **uniform** time grid via the
/// exact per-timestep Duhamel recurrence (Chopra Table 5.3.1).
///
/// # Arguments
/// * `omega`   — natural angular frequency ω (rad/s); must satisfy ω > 0 and
///   ζ < 1 for the underdamped closed form.
/// * `zeta`    — modal damping ratio ζ (dimensionless, 0 ≤ ζ < 1).
/// * `dt`      — uniform time step Δt (s).
/// * `forcing` — scalar pre-projected modal forcing samples fᵢ(tⱼ), one per
///   output time point; `forcing[0]` is the force at t=0.
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
    let coeffs = duhamel_coefficients(omega, zeta, dt);
    duhamel_solve_with_coeffs(&coeffs, forcing, xi0, v0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Task ι pure helpers — time grid, forcing samplers, node resolution, series
// reconstruction
// ─────────────────────────────────────────────────────────────────────────────
//
// Dependency-free `f64` math consumed by the `modal::transient_response` /
// `modal::displacement_at` trampolines (crates/reify-eval/src/modal_ops.rs); the
// `Value` marshalling, node-string parsing, and diagnostics live there. These
// helpers build the uniform grid the θ solver integrates on, sample each forcing
// primitive's scalar p_src(t), resolve a geometry-free node from a mode shape,
// and reconstruct one location's modal-superposition series.

/// Build the uniform time grid `[t_start, t_start + dt, …]` that
/// [`solve_modal_response`] integrates on (uniform spacing auto-selects the exact
/// Duhamel path).
///
/// The sample count is `floor((t_end − t_start) / dt) + 1`, computed via `floor`
/// (not repeated accumulation) so the grid length is deterministic and free of
/// floating-point drift: `grid[j] = t_start + j·dt` for `j ∈ [0, count)`, and the
/// last sample `t_start + floor(span/dt)·dt ≤ t_end` by construction.
///
/// Degenerate inputs return early:
/// * `dt ≤ 0` → empty (no positive spacing → no grid).
/// * `t_end < t_start` → empty (negative span → a `floor` count ≤ 0).
/// * `t_end == t_start` → `[t_start]` (a single sample, `floor(0)+1 = 1`).
pub fn uniform_time_grid(t_start: f64, t_end: f64, dt: f64) -> Vec<f64> {
    if dt <= 0.0 || t_end < t_start {
        return Vec::new();
    }
    // span ≥ 0 and dt > 0 here, so the floor is ≥ 0 and the `as usize` cast is safe.
    let count = ((t_end - t_start) / dt).floor() as usize + 1;
    (0..count).map(|j| t_start + j as f64 * dt).collect()
}

/// Scalar `StepForce` sample: zero before `start_time`, constant `magnitude`
/// from `start_time` onward (PRD §5.1). The step switches on AT `start_time`
/// (`t == start_time` → `magnitude`).
pub fn step_force_at(magnitude: f64, start_time: f64, t: f64) -> f64 {
    if t >= start_time { magnitude } else { 0.0 }
}

/// Scalar `HarmonicForce` sample: `amplitude · sin(2π · frequency_hz · t +
/// phase_rad)` (PRD §5.1). `frequency_hz` is cycles/second (the 2π factor
/// converts to angular frequency); `phase_rad` is the phase offset in radians
/// (the trampoline converts the `Angle`-typed `phase` field to radians first).
pub fn harmonic_force_at(amplitude: f64, frequency_hz: f64, phase_rad: f64, t: f64) -> f64 {
    use std::f64::consts::PI;
    amplitude * (2.0 * PI * frequency_hz * t + phase_rad).sin()
}

/// Scalar `SampledForce` sample: piecewise-linear interpolation of the
/// `(times, forces)` table at `t`, and `0` outside the sampled window
/// `[times[0], times[last]]` (the v0.3 finite-window convention — a sampled
/// excitation carries no force before its first / after its last stamp).
///
/// `times` is assumed monotonically increasing (the non-uniform sample stamps of
/// a `SampledForce`). Defensive against a length mismatch between `times` and
/// `forces` (the `time_samples.count == force_samples.count` invariant is
/// deferred to the trampoline per modal_analysis.ri): only the common prefix of
/// length `min(times.len(), forces.len())` is used; an empty common prefix → `0`.
pub fn sampled_force_at(times: &[f64], forces: &[f64], t: f64) -> f64 {
    let n = times.len().min(forces.len());
    if n == 0 {
        return 0.0;
    }
    // Zero outside the sampled window.
    if t < times[0] || t > times[n - 1] {
        return 0.0;
    }
    // Locate the bracketing interval [times[k], times[k+1]] and interpolate.
    for k in 0..n - 1 {
        let (t0, t1) = (times[k], times[k + 1]);
        if t >= t0 && t <= t1 {
            if t1 <= t0 {
                // Degenerate zero-/negative-width interval: take the left value.
                return forces[k];
            }
            let frac = (t - t0) / (t1 - t0);
            return forces[k] + frac * (forces[k + 1] - forces[k]);
        }
    }
    // Single-sample table (n == 1, no intervals) with t == times[0], or a
    // floating-point edge at the last stamp: take the final sample.
    forces[n - 1]
}

/// Scalar `ImpulseForce` sample: the v0.3 discrete-pulse approximation of a Dirac
/// delta `impulse · δ(t − time)` (PRD §5.1). The continuous delta cannot be
/// sampled on a discrete grid, so the impulse is deposited as a single
/// rectangular pulse of height `impulse / dt` at the one grid sample whose
/// half-open window `[t − dt/2, t + dt/2)` contains the impulse `time` — so the
/// pulse integrates to `(impulse/dt)·dt = impulse`, conserving momentum. Every
/// other sample reads `0`. The half-open window makes the carrying-sample choice
/// deterministic (a `time` exactly on a window edge falls to the UPPER sample —
/// no double-count, no gap) for the uniform grid `uniform_time_grid` produces.
/// `dt ≤ 0` → `0` (no well-defined pulse width).
pub fn impulse_force_at(impulse: f64, time: f64, t: f64, dt: f64) -> f64 {
    if dt <= 0.0 {
        return 0.0;
    }
    let half = dt / 2.0;
    if time >= t - half && time < t + half {
        impulse / dt
    } else {
        0.0
    }
}

/// Geometry-free node resolver: the index of the node with the largest
/// displacement norm in a mode shape — the modal **antinode**. Used when a
/// `location` string is non-numeric (no `LocationId` topology has landed yet):
/// the fundamental mode's antinode is the cantilever free-end tip, so "force at
/// tip" / "query at tip" both resolve here against `modes[0].shape`
/// (design-decision-3), keeping forcing projection and reconstruction
/// self-consistent.
///
/// Argmax over nodes of `Σ_axis φ²` (the squared norm — monotonic in the norm,
/// so the argmax is identical while avoiding a per-node `sqrt`). The tie-break is
/// deterministic: a strict `>` keeps the FIRST (lowest-index) maximum, so equal
/// norms never reorder. Empty input → `0` (a degenerate zero-node shape; the
/// trampoline guards against it upstream — this is the safe floor). A `NaN`
/// component never compares `>`, so a NaN node is never selected.
pub fn dominant_antinode_index(shapes: &[[f64; 3]]) -> usize {
    let mut best_idx = 0;
    let mut best_sq = f64::NEG_INFINITY;
    for (i, s) in shapes.iter().enumerate() {
        let sq = s[0] * s[0] + s[1] * s[1] + s[2] * s[2];
        if sq > best_sq {
            best_sq = sq;
            best_idx = i;
        }
    }
    best_idx
}

/// Reconstruct one location's physical displacement time series by modal
/// superposition: `u_j = Σ_i coeffs[i]·mode_coords[i][j]`, where `coeffs[i]` is
/// the per-mode projection coefficient `Φ_i[node]·direction` and `mode_coords[i]`
/// is mode `i`'s modal-coordinate series ξ_i(t_j). The lazy core of
/// `displacement_at`: only the queried node's coefficients are formed, so the
/// full n_nodes × n_times displacement matrix is never materialized.
///
/// Graceful degradation (never panics on ragged / mismatched input):
/// * Empty `mode_coords` → empty (no time dimension is defined).
/// * The time length is taken from `mode_coords[0]`; a mode whose series is
///   shorter contributes `0` at the missing timesteps (`.get(j)`).
/// * Only the first `min(coeffs.len(), mode_coords.len())` modes contribute, so
///   surplus modes or surplus coeffs are ignored rather than indexed out of bounds.
pub fn reconstruct_series(coeffs: &[f64], mode_coords: &[Vec<f64>]) -> Vec<f64> {
    let time_len = match mode_coords.first() {
        Some(first) => first.len(),
        None => return Vec::new(),
    };
    let n_modes = coeffs.len().min(mode_coords.len());
    let mut out = vec![0.0_f64; time_len];
    for (c, series) in coeffs.iter().zip(mode_coords.iter()).take(n_modes) {
        for (j, slot) in out.iter_mut().enumerate() {
            *slot += c * series.get(j).copied().unwrap_or(0.0);
        }
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
        //     Choose N so that ω·Δt ≈ 0.05 for the coarse grid (per plan §11).
        //     With T=0.5, ω=50: N_c=501 → Δt=0.001 → ω·Δt=0.05.
        //     Halving Δt: N_f=1001 → ω·Δt=0.025.
        {
            let n_c = 501_usize;  // coarse: ω·dt ≈ 0.05
            let n_f = 1001_usize; // fine:   ω·dt ≈ 0.025
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
        //     Keep ω·Δt_max ≤ 0.25 so the 2nd-order Newmark error stays well
        //     under 2%.  With dt_start=0.001 and ratio=1.02 over 79 steps:
        //       dt_max ≈ 0.001 × 1.02^78 ≈ 0.00472 s
        //       ω·dt_max ≈ 50 × 0.00472 ≈ 0.236  →  error ≈ (0.236)²/12 ≈ 0.5%
        {
            let n = 80_usize;
            let ratio = 1.02_f64;  // gentle stretch factor per step
            // Build times: t_0=0, t_{k+1} = t_k + dt_k where dt_{k+1} = ratio*dt_k.
            let mut times = Vec::with_capacity(n);
            times.push(0.0_f64);
            let mut dt = 0.001_f64;
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

    // ─── step 13: solve_modal_response dispatcher (RED) ─────────────────────

    /// Tests the `solve_modal_response` dispatcher across all four routing cases.
    /// RED: `Integrator`, `ModalResponse`, and `solve_modal_response` are absent.
    #[test]
    fn solve_modal_response_dispatcher() {
        let omega = 50.0_f64;
        let zeta  = 0.05_f64;
        let p0    = 2.0_f64;
        let n     = 60_usize;
        let dt    = 0.001_f64;

        // (a) Uniform grid + underdamped ζ<1 → DuhamelUniform; coords exact.
        {
            let times:   Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
            let forcing: Vec<f64> = vec![p0; n];
            let resp = solve_modal_response(omega, zeta, &times, &forcing, 0.0, 0.0);
            assert_eq!(resp.integrator, Integrator::DuhamelUniform,
                "uniform+underdamped must choose DuhamelUniform");
            for (j, &got) in resp.coords.iter().enumerate() {
                let want = analytic_step_response(p0, omega, zeta, j as f64 * dt);
                assert!(
                    (got - want).abs() < 1e-12,
                    "case (a) step {j}: got {got:.6e} want {want:.6e} diff {:.2e}",
                    (got - want).abs()
                );
            }
        }

        // (b) Non-uniform grid → Newmark; coords within method tolerance.
        {
            let mut times = Vec::with_capacity(n);
            times.push(0.0_f64);
            let mut step = 0.001_f64;
            for _ in 1..n {
                times.push(times.last().unwrap() + step);
                step *= 1.02;
            }
            let forcing: Vec<f64> = vec![p0; n];
            let resp = solve_modal_response(omega, zeta, &times, &forcing, 0.0, 0.0);
            assert_eq!(resp.integrator, Integrator::Newmark,
                "non-uniform grid must choose Newmark");
            let max_rel = resp.coords.iter().enumerate().map(|(j, &g)| {
                let want = analytic_step_response(p0, omega, zeta, times[j]);
                (g - want).abs()
            }).fold(0.0_f64, f64::max) * omega * omega / p0;
            assert!(max_rel < 2e-2,
                "case (b) Newmark non-uniform rel err {max_rel:.3e} ≥ 2e-2");
        }

        // (c) Uniform grid but ζ≥1 (critically/over-damped) → Newmark; all finite.
        {
            let zeta_over = 1.5_f64;
            let times:   Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
            let forcing: Vec<f64> = vec![p0; n];
            let resp = solve_modal_response(omega, zeta_over, &times, &forcing, 0.0, 0.0);
            assert_eq!(resp.integrator, Integrator::Newmark,
                "ζ≥1 must route to Newmark even on uniform grid");
            assert!(resp.coords.iter().all(|x| x.is_finite()),
                "case (c) ζ≥1: coords must all be finite");
        }

        // (d) Uniform grid but ω≈0 (rigid-body) → Newmark; all finite (no NaN).
        {
            let omega_zero = 1e-10_f64;
            let times:   Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
            let forcing: Vec<f64> = vec![p0; n];
            let resp = solve_modal_response(omega_zero, zeta, &times, &forcing, 0.0, 0.0);
            assert_eq!(resp.integrator, Integrator::Newmark,
                "ω≈0 (rigid-body) must route to Newmark");
            assert!(resp.coords.iter().all(|x| x.is_finite()),
                "case (d) ω≈0: coords must all be finite (no NaN)");
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

    // ─── boundary cases: empty / single-element / length mismatch ────────────

    /// Empty `forcing` (or `times`) returns an empty `Vec`; single-element returns `[xi0]`.
    #[test]
    fn empty_and_single_element_boundary() {
        let omega = 10.0_f64;
        let zeta  = 0.05_f64;

        // duhamel_solve: empty forcing → empty Vec.
        let result = duhamel_solve(omega, zeta, 0.001, &[], 0.0, 0.0);
        assert_eq!(result.len(), 0, "duhamel_solve empty → empty Vec");

        // duhamel_solve: single element → [xi0].
        let result = duhamel_solve(omega, zeta, 0.001, &[5.0], 2.0, 1.0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], 2.0_f64, "duhamel_solve single-element: index 0 must be xi0");

        // newmark_solve: empty times/forcing → empty Vec.
        let result = newmark_solve(omega, zeta, &[], &[], 0.0, 0.0);
        assert_eq!(result.len(), 0, "newmark_solve empty → empty Vec");

        // newmark_solve: single element → [xi0].
        let result = newmark_solve(omega, zeta, &[0.0], &[5.0], 2.0, 1.0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], 2.0_f64, "newmark_solve single-element: index 0 must be xi0");

        // solve_modal_response on a single-point uniform-eligible grid routes to Newmark
        // (is_uniformly_sampled returns false for len < 2) and returns [xi0].
        let resp = solve_modal_response(omega, zeta, &[0.0], &[5.0], 2.0, 1.0);
        assert_eq!(resp.coords.len(), 1);
        assert_eq!(resp.coords[0], 2.0_f64);
    }

    /// Mismatched `times`/`forcing` lengths must panic with a descriptive message.
    #[test]
    #[should_panic(expected = "times and forcing must have equal length")]
    fn mismatched_times_forcing_panics() {
        let times   = vec![0.0_f64, 0.1, 0.2];
        let forcing = vec![1.0_f64, 2.0];
        let _ = solve_modal_response(10.0, 0.05, &times, &forcing, 0.0, 0.0);
    }

    // ─── step 07: sine accuracy — SINE ACCURACY PIN ──────────────────────────

    /// Characterises the FOH recurrence for f(t) = sin(Ωt).
    ///
    /// Asserts:
    /// (a) Loose sanity bound: max relative error < 5e-2 (the honest
    ///     O((ΩΔt)²) method floor — NOT 1e-9).
    /// (b) Authoritative 2nd-order convergence: error_coarse / error_fine ≥ 3.5.
    ///     Uses n_coarse=51 and n_fine=2*51-1=101 so every coarse point coincides
    ///     with every other fine point (true Δt-halving comparison).
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
            let dense: Vec<f64> = (0..1000)
                .map(|j| analytic_sine_response(f0, big_omega, omega, zeta, j as f64 * t_end / 999.0).abs())
                .collect();
            dense.iter().cloned().fold(0.0_f64, f64::max)
        };

        // n_coarse=51 and n_fine=2*51-1=101: coarse point j maps to fine point 2j,
        // so comparing errors at shared points is a true Δt-halving test.
        let n_coarse = 51_usize;
        let n_fine   = 2 * n_coarse - 1;  // 101

        // Run the integrator and return the max relative error measured only at
        // every `stride`-th output index (to restrict comparison to shared grid).
        let run = |n: usize, stride: usize| -> f64 {
            let dt = t_end / (n - 1) as f64;
            let forcing: Vec<f64> = (0..n)
                .map(|j| f0 * (big_omega * j as f64 * dt).sin())
                .collect();
            let got = duhamel_solve(omega, zeta, dt, &forcing, 0.0, 0.0);
            got.iter()
                .enumerate()
                .step_by(stride)
                .map(|(j, &g)| {
                    let t    = j as f64 * dt;
                    let want = analytic_sine_response(f0, big_omega, omega, zeta, t);
                    (g - want).abs()
                })
                .fold(0.0_f64, f64::max)
                / max_abs_analytic
        };

        // Coarse: all 51 points; fine: every 2nd of 101 points (= shared coarse grid).
        let err_coarse = run(n_coarse, 1);
        let err_fine   = run(n_fine, 2);

        // (a) Loose sanity bound — NOT 1e-9.
        assert!(
            err_coarse < 5e-2,
            "coarse-grid ({n_coarse} pt) relative error {err_coarse:.3e} ≥ 5e-2"
        );

        // (b) Authoritative 2nd-order convergence-rate assertion (Δt halved → error ÷4).
        let ratio = err_coarse / err_fine;
        assert!(
            ratio >= 3.5,
            "convergence ratio err_coarse/err_fine = {ratio:.2} < 3.5 (expected ≥ 4 for 2nd-order)"
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

    // ─── step-1: uniform_time_grid (RED) ─────────────────────────────────────

    /// `uniform_time_grid(t_start, t_end, dt)` builds the uniform grid
    /// `[t_start, t_start+dt, …]` with the last sample ≤ t_end,
    /// count = floor((t_end−t_start)/dt)+1, an exact start endpoint, and the
    /// documented degenerate edge cases (dt ≤ 0 → empty; t_end < t_start → empty;
    /// t_end == t_start → `[t_start]`). This grid feeds `solve_modal_response`
    /// (uniform → Duhamel path).
    /// RED: `uniform_time_grid` is absent — fails to compile.
    #[test]
    fn uniform_time_grid_cases() {
        // Evenly-divisible span: dt = 0.25 over [0, 1] → 5 points, last == t_end.
        let g = uniform_time_grid(0.0, 1.0, 0.25);
        assert_eq!(g.len(), 5, "floor(1.0/0.25)+1 = 5");
        assert_eq!(g[0], 0.0, "exact start endpoint");
        assert_eq!(*g.last().unwrap(), 1.0, "exact end endpoint when span divides dt");
        for (j, &t) in g.iter().enumerate() {
            assert!((t - j as f64 * 0.25).abs() < 1e-15, "grid[{j}] = t_start + j·dt");
        }

        // Non-divisible span: dt = 0.3 over [0, 1] → floor(3.333)+1 = 4 points,
        // last = 0.9 ≤ 1.0 (NOT clamped up to t_end).
        let g = uniform_time_grid(0.0, 1.0, 0.3);
        assert_eq!(g.len(), 4, "floor(1.0/0.3)+1 = 4");
        assert!(*g.last().unwrap() <= 1.0, "last sample must not exceed t_end");
        assert!((*g.last().unwrap() - 0.9).abs() < 1e-12, "last = t_start + 3·0.3 = 0.9");

        // Non-zero start: [1, 2] step 0.5 → [1.0, 1.5, 2.0], last == t_end.
        let g = uniform_time_grid(1.0, 2.0, 0.5);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0], 1.0);
        assert_eq!(*g.last().unwrap(), 2.0);

        // Strictly increasing.
        let g = uniform_time_grid(0.0, 5.0, 0.7);
        assert!(g.windows(2).all(|w| w[1] > w[0]), "grid must be strictly increasing");

        // t_end == t_start → single sample [t_start] (count = floor(0)+1 = 1).
        assert_eq!(uniform_time_grid(2.0, 2.0, 0.1), vec![2.0]);

        // dt ≤ 0 → empty.
        assert!(uniform_time_grid(0.0, 1.0, 0.0).is_empty(), "dt = 0 → empty");
        assert!(uniform_time_grid(0.0, 1.0, -0.1).is_empty(), "dt < 0 → empty");

        // t_end < t_start → empty (count clamps to 0).
        assert!(uniform_time_grid(1.0, 0.5, 0.1).is_empty(), "t_end < t_start → empty");
    }

    // ─── step-3: forcing samplers (RED) ──────────────────────────────────────

    /// The four per-source scalar forcing samplers against their closed forms.
    /// RED: `step_force_at` / `harmonic_force_at` / `sampled_force_at` /
    /// `impulse_force_at` are absent — fails to compile.
    #[test]
    fn forcing_samplers_closed_form() {
        use std::f64::consts::PI;

        // StepForce: 0 before start_time, magnitude from start_time onward.
        assert_eq!(step_force_at(10.0, 1.0, 0.5), 0.0, "before start → 0");
        assert_eq!(step_force_at(10.0, 1.0, 1.0), 10.0, "at start → magnitude (step on)");
        assert_eq!(step_force_at(10.0, 1.0, 2.0), 10.0, "after start → magnitude");

        // HarmonicForce: A·sin(2π·f·t + φ).
        let (a, f, phi) = (3.0_f64, 2.0_f64, PI / 6.0);
        for &t in &[0.0, 0.05, 0.123, 0.25, 1.0] {
            let want = a * (2.0 * PI * f * t + phi).sin();
            assert!(
                (harmonic_force_at(a, f, phi, t) - want).abs() < 1e-12,
                "harmonic at t={t}: got {}, want {want}",
                harmonic_force_at(a, f, phi, t)
            );
        }
        // phase = 0 → sin(0) = 0 at t = 0.
        assert!(harmonic_force_at(5.0, 1.0, 0.0, 0.0).abs() < 1e-12, "zero-phase at t=0 → 0");

        // SampledForce: linear interp inside the table, zero outside (the v0.3
        // finite-window convention). Includes a non-uniform interval [2, 4].
        let times = [0.0, 1.0, 2.0, 4.0];
        let forces = [0.0, 10.0, 20.0, 0.0];
        assert_eq!(sampled_force_at(&times, &forces, 0.0), 0.0, "first sample exact");
        assert_eq!(sampled_force_at(&times, &forces, 1.0), 10.0, "interior sample exact");
        assert!((sampled_force_at(&times, &forces, 0.5) - 5.0).abs() < 1e-12, "midpoint interp");
        assert!((sampled_force_at(&times, &forces, 1.5) - 15.0).abs() < 1e-12, "midpoint interp");
        assert!(
            (sampled_force_at(&times, &forces, 3.0) - 10.0).abs() < 1e-12,
            "interp across the non-uniform [2,4] interval"
        );
        assert_eq!(sampled_force_at(&times, &forces, -1.0), 0.0, "before table → 0");
        assert_eq!(sampled_force_at(&times, &forces, 5.0), 0.0, "after table → 0");
        // Empty / single-sample degenerate.
        assert_eq!(sampled_force_at(&[], &[], 1.0), 0.0, "empty table → 0");
        assert_eq!(sampled_force_at(&[2.0], &[7.0], 2.0), 7.0, "single sample, at point");
        assert_eq!(sampled_force_at(&[2.0], &[7.0], 2.5), 0.0, "single sample, outside → 0");

        // ImpulseForce: impulse/dt at the one grid sample whose half-open
        // [t−dt/2, t+dt/2) window contains `time`, else 0 (discrete-pulse v0.3
        // approximation). Verified by its physical invariant, NOT by an exact FP
        // boundary: a decimal literal like 0.15 is not bit-equal to a computed
        // window edge `t + dt/2`, so asserting edge behavior directly is
        // floating-point-fragile and meaningless. The half-open `<` is what
        // makes the windows tile the line — the no-double-count test below pins
        // that property robustly.
        let dt = 0.1;
        // (a) the sample nearest an interior `time` carries impulse/dt = 20.
        assert!(
            (impulse_force_at(2.0, 0.12, 0.1, dt) - 20.0).abs() < 1e-12,
            "nearest sample carries impulse/dt = 2.0/0.1 = 20"
        );
        assert_eq!(impulse_force_at(2.0, 0.12, 0.2, dt), 0.0, "non-nearest sample → 0");
        assert_eq!(impulse_force_at(2.0, 0.12, 0.0, dt), 0.0, "non-nearest sample → 0");
        // (b) no-double-count / momentum conservation: across a uniform grid
        // EXACTLY ONE sample is nonzero, with height impulse/dt — so
        // Σ_j p(t_j)·dt == impulse. Times are chosen clearly interior to a window.
        let grid = [0.0_f64, 0.1, 0.2, 0.3, 0.4, 0.5];
        for &time in &[0.07_f64, 0.13, 0.22, 0.38] {
            let nonzero: Vec<f64> = grid
                .iter()
                .map(|&tj| impulse_force_at(3.0, time, tj, dt))
                .filter(|&p| p != 0.0)
                .collect();
            assert_eq!(
                nonzero.len(),
                1,
                "impulse time={time} must land on exactly one sample, got {nonzero:?}"
            );
            assert!(
                (nonzero[0] - 30.0).abs() < 1e-12,
                "pulse height = impulse/dt = 3.0/0.1 = 30, got {}",
                nonzero[0]
            );
        }
        // dt ≤ 0 → 0 (no pulse width).
        assert_eq!(impulse_force_at(2.0, 0.12, 0.1, 0.0), 0.0, "dt = 0 → 0");
    }

    // ─── step-5: dominant_antinode_index (RED) ───────────────────────────────

    /// `dominant_antinode_index(shapes)` returns the node index of maximum
    /// displacement norm ‖Φ[node]‖ (the geometry-free node resolver: the
    /// fundamental-mode antinode = the cantilever free-end tip). Ties resolve to
    /// the LOWEST index (deterministic); empty input → 0.
    /// RED: function absent — fails to compile.
    #[test]
    fn dominant_antinode_index_cases() {
        // Unambiguous max at node 2 (a cantilever-like ramp toward the free end).
        let shapes = [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.5],
            [0.0, 0.0, 1.0], // ‖·‖ = 1.0, the antinode
            [0.0, 0.0, 0.7],
        ];
        assert_eq!(dominant_antinode_index(&shapes), 2);

        // Mixed-axis magnitudes: node 1 has the largest norm (3-4-0 → 5).
        let shapes = [
            [1.0, 1.0, 1.0], // √3 ≈ 1.73
            [3.0, 4.0, 0.0], // 5.0 ← max
            [0.0, 2.0, 2.0], // √8 ≈ 2.83
        ];
        assert_eq!(dominant_antinode_index(&shapes), 1);

        // Ties resolve to the LOWEST index (deterministic).
        let shapes = [
            [0.0, 0.0, 2.0], // 2.0
            [2.0, 0.0, 0.0], // 2.0 (tie, but higher index)
            [0.0, 1.0, 0.0], // 1.0
        ];
        assert_eq!(dominant_antinode_index(&shapes), 0, "ties → lowest index");

        // Single node → 0.
        assert_eq!(dominant_antinode_index(&[[5.0, 0.0, 0.0]]), 0);

        // Empty → 0 (degenerate; the trampoline guards against a zero-node shape).
        let empty: [[f64; 3]; 0] = [];
        assert_eq!(dominant_antinode_index(&empty), 0);
    }

    // ── task λ: prepare/integrate split (RED → GREEN in step-4) ──────────────

    /// (a) `integrate_prepared(&prepare_modal_integrator(ω,ζ,&times), &times,
    /// &forcing, xi0, v0)` returns coords BIT-IDENTICAL to
    /// `solve_modal_response(ω,ζ,&times,&forcing,xi0,v0).coords` for:
    ///   - a uniform-grid underdamped case (Duhamel path)
    ///   - a ζ≥1 (critically-damped) case (Newmark path)
    ///   - a ω≈0 (rigid-body) case (Newmark path)
    ///
    /// (b) `prepare_modal_integrator` selects DuhamelUniform vs Newmark
    /// identically to `solve_modal_response`'s dispatcher.
    ///
    /// (c) `duhamel_solve_with_coeffs(&duhamel_coefficients(ω,ζ,dt), &forcing,
    /// xi0, v0)` equals `duhamel_solve(ω,ζ,dt,&forcing,xi0,v0)` bit-for-bit.
    ///
    /// RED: `prepare_modal_integrator`, `integrate_prepared`, and
    /// `duhamel_solve_with_coeffs` are absent — fails to compile.
    #[test]
    fn prepare_integrate_split_matches_solve_modal_response() {
        let n = 60_usize;
        let dt = 0.001_f64;
        let p0 = 2.0_f64;
        let xi0 = 0.0_f64;
        let v0 = 0.0_f64;

        // ── (a) Duhamel path: uniform underdamped ─────────────────────────────
        {
            let omega = 50.0_f64;
            let zeta  = 0.05_f64;
            let times: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
            let forcing: Vec<f64> = vec![p0; n];

            let reference = solve_modal_response(omega, zeta, &times, &forcing, xi0, v0);
            assert_eq!(
                reference.integrator,
                Integrator::DuhamelUniform,
                "fixture must select DuhamelUniform",
            );

            let prep = prepare_modal_integrator(omega, zeta, &times);
            // (b) same variant selected
            assert!(
                matches!(prep, PreparedIntegrator::Duhamel { .. }),
                "prepare_modal_integrator must return Duhamel{{ .. }} for uniform+underdamped",
            );

            let got = integrate_prepared(&prep, &times, &forcing, xi0, v0);
            assert_eq!(got.len(), reference.coords.len());
            for (j, (&g, &r)) in got.iter().zip(reference.coords.iter()).enumerate() {
                assert_eq!(
                    g.to_bits(),
                    r.to_bits(),
                    "Duhamel path step {j}: integrate_prepared {g:.6e} != solve_modal_response {r:.6e}",
                );
            }
        }

        // ── (a) Newmark path: ζ≥1 (critically-damped) ────────────────────────
        {
            let omega = 50.0_f64;
            let zeta  = 1.5_f64; // over-damped → Newmark
            let times: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
            let forcing: Vec<f64> = vec![p0; n];

            let reference = solve_modal_response(omega, zeta, &times, &forcing, xi0, v0);
            assert_eq!(reference.integrator, Integrator::Newmark, "ζ≥1 must use Newmark");

            let prep = prepare_modal_integrator(omega, zeta, &times);
            assert!(
                matches!(prep, PreparedIntegrator::Newmark { .. }),
                "prepare_modal_integrator must return Newmark{{ .. }} for ζ≥1",
            );

            let got = integrate_prepared(&prep, &times, &forcing, xi0, v0);
            assert_eq!(got.len(), reference.coords.len());
            for (j, (&g, &r)) in got.iter().zip(reference.coords.iter()).enumerate() {
                assert_eq!(
                    g.to_bits(),
                    r.to_bits(),
                    "Newmark (ζ≥1) step {j}: integrate_prepared {g:.6e} != solve_modal_response {r:.6e}",
                );
            }
        }

        // ── (a) Newmark path: ω≈0 (rigid-body) ───────────────────────────────
        {
            let omega = 1e-10_f64; // below OMEGA_FLOOR → Newmark
            let zeta  = 0.05_f64;
            let times: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
            let forcing: Vec<f64> = vec![p0; n];

            let reference = solve_modal_response(omega, zeta, &times, &forcing, xi0, v0);
            assert_eq!(reference.integrator, Integrator::Newmark, "ω≈0 must use Newmark");

            let prep = prepare_modal_integrator(omega, zeta, &times);
            assert!(
                matches!(prep, PreparedIntegrator::Newmark { .. }),
                "prepare_modal_integrator must return Newmark{{ .. }} for ω≈0",
            );

            let got = integrate_prepared(&prep, &times, &forcing, xi0, v0);
            assert_eq!(got.len(), reference.coords.len());
            for (j, (&g, &r)) in got.iter().zip(reference.coords.iter()).enumerate() {
                assert_eq!(
                    g.to_bits(),
                    r.to_bits(),
                    "Newmark (ω≈0) step {j}: integrate_prepared {g:.6e} != solve_modal_response {r:.6e}",
                );
            }
        }
    }

    /// (c) `duhamel_solve_with_coeffs(&duhamel_coefficients(ω,ζ,dt), &forcing,
    /// xi0, v0)` returns the SAME bits as `duhamel_solve(ω,ζ,dt,&forcing,xi0,v0)`.
    ///
    /// RED: `duhamel_solve_with_coeffs` is absent — fails to compile.
    #[test]
    fn duhamel_solve_with_coeffs_equals_duhamel_solve_bit_for_bit() {
        let omega = 50.0_f64;
        let zeta  = 0.05_f64;
        let dt    = 0.001_f64;
        let n     = 60_usize;
        let p0    = 3.0_f64;
        let xi0   = 1.0_f64;
        let v0    = 0.5_f64;

        let forcing: Vec<f64> = (0..n).map(|i| p0 * (i as f64 * dt)).collect();
        let reference = duhamel_solve(omega, zeta, dt, &forcing, xi0, v0);
        let coeffs    = duhamel_coefficients(omega, zeta, dt);
        let got       = duhamel_solve_with_coeffs(&coeffs, &forcing, xi0, v0);

        assert_eq!(got.len(), reference.len());
        for (j, (&g, &r)) in got.iter().zip(reference.iter()).enumerate() {
            assert_eq!(
                g.to_bits(),
                r.to_bits(),
                "step {j}: duhamel_solve_with_coeffs {g:.6e} != duhamel_solve {r:.6e}",
            );
        }
    }

    // ─── step-7: reconstruct_series (RED) ────────────────────────────────────

    /// `reconstruct_series(coeffs, mode_coords)` computes the per-timestep
    /// weighted sum u_j = Σ_i coeffs[i]·mode_coords[i][j] — the lazy single-
    /// location modal-superposition core used by displacement_at. Mismatched /
    /// empty inputs degrade gracefully (zeros of the time length, never a panic).
    /// RED: function absent — fails to compile.
    #[test]
    fn reconstruct_series_cases() {
        // 2 modes × 3 timesteps, coeffs = [2, −1]:
        //   u_j = 2·[1,2,3] − 1·[10,20,30] = [2−10, 4−20, 6−30] = [−8, −16, −24].
        let coeffs = [2.0, -1.0];
        let mode_coords = vec![vec![1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0]];
        assert_eq!(reconstruct_series(&coeffs, &mode_coords), vec![-8.0, -16.0, -24.0]);

        // Single mode, identity coeff → echoes the series.
        assert_eq!(reconstruct_series(&[1.0], &[vec![5.0, 6.0]]), vec![5.0, 6.0]);

        // Empty mode_coords → empty (no time dimension defined).
        assert!(reconstruct_series(&[1.0, 2.0], &[]).is_empty(), "no modes → empty series");

        // Empty coeffs, non-empty mode_coords → zeros of the time length.
        assert_eq!(
            reconstruct_series(&[], &[vec![1.0, 2.0, 3.0]]),
            vec![0.0, 0.0, 0.0],
            "no coeffs → zero contribution, time length preserved"
        );

        // Mismatched: more modes than coeffs → only the first coeffs.len() modes
        // contribute (extras dropped, no panic).
        assert_eq!(
            reconstruct_series(&[1.0], &[vec![4.0, 5.0], vec![100.0, 200.0]]),
            vec![4.0, 5.0],
            "modes beyond coeffs are ignored"
        );
    }
}
