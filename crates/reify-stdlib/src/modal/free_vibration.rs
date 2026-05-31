//! Pure scalar helpers for free-vibration modal analysis (PRD §4 / §7.5).
//!
//! Dependency-free `f64` math (no `reify-solver-elastic` / `reify-ir::Value`
//! deps) so this module stays inside `reify-stdlib`. The `reify-eval` modal
//! trampoline (`modal_ops.rs`) calls these to convert eigen-solver output into
//! the `ModalResult` fields.

use std::f64::consts::PI;

/// Angular-frequency floor (rad/s) below which [`rayleigh_damping_ratio`]
/// reports `0.0` instead of dividing by ω. A genuine flexible mode has
/// ω ≫ this; only rigid-body / spurious near-zero modes fall under it, where
/// ζ = (α + β·ω²)/(2ω) → ∞ is non-physical (a rigid-body mode carries no modal
/// damping). Distinct from the caller-supplied [`is_rigid_body_mode`]
/// tolerance: this constant only guards the 1/ω singularity.
const MIN_OMEGA_FOR_DAMPING: f64 = 1e-9;

/// Natural frequency in Hz from a free-vibration eigenvalue `λ = ω²`
/// (rad²/s²): `f = √λ / (2π)`.
///
/// `λ ≤ 0` — a zero-energy rigid-body mode, or a spurious negative pair from
/// numerical noise — clamps to `0.0` rather than producing `NaN` from
/// `√(negative)`. A `NaN` λ likewise maps to `0.0` (the `λ > 0` predicate is
/// false). PRD §4.1 / §7.
pub fn eigenvalue_to_frequency_hz(lambda: f64) -> f64 {
    if lambda > 0.0 {
        lambda.sqrt() / (2.0 * PI)
    } else {
        0.0
    }
}

/// Rayleigh (proportional) modal damping ratio ζ for one mode (PRD §4.2):
///
/// ```text
/// ζ = (α + β·ω²) / (2·ω)
/// ```
///
/// where ω is the natural angular frequency (rad/s), α is mass-proportional,
/// and β is stiffness-proportional. `NoDamping` ⇒ α = β = 0 ⇒ ζ = 0. An ω at
/// or below [`MIN_OMEGA_FOR_DAMPING`] (rigid-body / spurious mode) returns
/// `0.0` to avoid the 1/ω singularity.
pub fn rayleigh_damping_ratio(alpha: f64, beta: f64, omega: f64) -> f64 {
    if omega.abs() <= MIN_OMEGA_FOR_DAMPING {
        0.0
    } else {
        (alpha + beta * omega * omega) / (2.0 * omega)
    }
}

/// Mass-normalization scale `1/√m` for a mode whose generalized mass is
/// `m = φᵀ·M·φ` (PRD §7.5). Scaling φ by this value makes `φᵀ·M·φ = 1`.
///
/// `m ≤ 0` (a degenerate / non-physical generalized mass) returns the `0.0`
/// sentinel rather than `±∞`/`NaN`; the caller treats a `0.0` scale as "skip
/// normalization" (the mode is degenerate and is flagged separately).
pub fn mass_normalization_scale(m: f64) -> f64 {
    if m > 0.0 {
        1.0 / m.sqrt()
    } else {
        0.0
    }
}

/// Effective modal participation mass `m_eff = p²` from the participation
/// factor `p = φᵀ·M·d` (φ mass-normalized, d the unit reference direction
/// broadcast to the translational DOFs; PRD §4.1 / §4.3). The sign of `p` is
/// irrelevant — the effective mass is its square.
pub fn modal_participation_mass(p: f64) -> f64 {
    p * p
}

/// `true` iff `|ω| ≤ tol` — the mode is a rigid-body (zero-frequency) mode
/// within the caller-supplied angular-frequency tolerance. Used to flag the
/// `W_ModalRigidBodyMode` diagnostic (an unconstrained / under-constrained
/// model admits ω ≈ 0 modes). PRD §9.
pub fn is_rigid_body_mode(omega: f64, tol: f64) -> bool {
    omega.abs() <= tol
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const EPS: f64 = 1e-9;

    // ── eigenvalue_to_frequency_hz: f = √λ / (2π) ────────────────────────────

    /// λ = ω² = (2π·f)²; recovering f for the cantilever fundamental (~41.3 Hz)
    /// must round-trip exactly (the helper is the inverse of (2π·f)²).
    #[test]
    fn eigenvalue_to_frequency_hz_round_trips_known_frequency() {
        let f = 41.3_f64;
        let lambda = (2.0 * PI * f).powi(2);
        let got = eigenvalue_to_frequency_hz(lambda);
        assert!((got - f).abs() < 1e-6, "got {got} Hz, want {f} Hz");
    }

    /// λ = 1 ⇒ ω = 1 rad/s ⇒ f = 1/(2π).
    #[test]
    fn eigenvalue_to_frequency_hz_unit_eigenvalue() {
        let got = eigenvalue_to_frequency_hz(1.0);
        assert!((got - 1.0 / (2.0 * PI)).abs() < EPS, "got {got}");
    }

    /// λ = 0 (zero-energy / rigid body) ⇒ 0 Hz exactly.
    #[test]
    fn eigenvalue_to_frequency_hz_zero_eigenvalue_is_zero() {
        assert_eq!(eigenvalue_to_frequency_hz(0.0), 0.0);
    }

    /// Negative λ (numerical noise / spurious near-zero pair) clamps to 0 Hz —
    /// must NOT produce NaN from √(negative).
    #[test]
    fn eigenvalue_to_frequency_hz_negative_eigenvalue_clamps_to_zero() {
        let got = eigenvalue_to_frequency_hz(-123.4);
        assert_eq!(got, 0.0, "negative λ must clamp to 0.0, got {got}");
    }

    // ── rayleigh_damping_ratio: ζ = (α + β·ω²) / (2ω) ────────────────────────

    /// Mass-proportional: α=2, β=0, ω=10 ⇒ 2/(2·10) = 0.1.
    #[test]
    fn rayleigh_damping_ratio_mass_proportional() {
        let got = rayleigh_damping_ratio(2.0, 0.0, 10.0);
        assert!((got - 0.1).abs() < EPS, "got {got}");
    }

    /// Stiffness-proportional: α=0, β=0.001, ω=100 ⇒ (0.001·10000)/200 = 0.05.
    #[test]
    fn rayleigh_damping_ratio_stiffness_proportional() {
        let got = rayleigh_damping_ratio(0.0, 0.001, 100.0);
        assert!((got - 0.05).abs() < EPS, "got {got}");
    }

    /// NoDamping ⇒ α = β = 0 ⇒ ζ = 0 for any ω > 0.
    #[test]
    fn rayleigh_damping_ratio_no_damping_is_zero() {
        assert_eq!(rayleigh_damping_ratio(0.0, 0.0, 250.0), 0.0);
    }

    /// ω ≈ 0 (rigid-body mode) ⇒ the div-by-zero guard returns 0.0.
    #[test]
    fn rayleigh_damping_ratio_zero_omega_guarded_to_zero() {
        assert_eq!(rayleigh_damping_ratio(1.0, 1.0, 0.0), 0.0);
    }

    // ── mass_normalization_scale: 1/√m for m > 0 ─────────────────────────────

    #[test]
    fn mass_normalization_scale_unit_mass_is_one() {
        assert!((mass_normalization_scale(1.0) - 1.0).abs() < EPS);
    }

    /// m = 4 ⇒ 1/√4 = 0.5.
    #[test]
    fn mass_normalization_scale_four_is_half() {
        assert!((mass_normalization_scale(4.0) - 0.5).abs() < EPS);
    }

    /// m ≤ 0 (degenerate generalized mass) ⇒ guarded to 0.0 sentinel.
    #[test]
    fn mass_normalization_scale_nonpositive_guarded_to_zero() {
        assert_eq!(mass_normalization_scale(0.0), 0.0);
        assert_eq!(mass_normalization_scale(-2.0), 0.0);
    }

    // ── modal_participation_mass: p² ─────────────────────────────────────────

    #[test]
    fn modal_participation_mass_squares_input() {
        assert!((modal_participation_mass(3.0) - 9.0).abs() < EPS);
        // Sign of the participation factor is irrelevant — the effective mass
        // is its square.
        assert!((modal_participation_mass(-2.0) - 4.0).abs() < EPS);
        assert_eq!(modal_participation_mass(0.0), 0.0);
    }

    // ── is_rigid_body_mode: |ω| ≤ tol ────────────────────────────────────────

    #[test]
    fn is_rigid_body_mode_true_for_near_zero_omega() {
        assert!(is_rigid_body_mode(0.0, 1e-6));
        assert!(is_rigid_body_mode(1e-7, 1e-6));
        // Magnitude test — a small negative ω is still "near zero".
        assert!(is_rigid_body_mode(-1e-7, 1e-6));
    }

    #[test]
    fn is_rigid_body_mode_false_for_real_mode() {
        assert!(!is_rigid_body_mode(100.0, 1e-6));
        assert!(!is_rigid_body_mode(2e-6, 1e-6));
    }

    /// |ω| == tol is inclusive (≤).
    #[test]
    fn is_rigid_body_mode_boundary_is_inclusive() {
        assert!(is_rigid_body_mode(1e-6, 1e-6));
    }
}
