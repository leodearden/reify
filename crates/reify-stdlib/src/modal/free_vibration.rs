//! Pure scalar helpers for free-vibration modal analysis (PRD §4 / §7.5).
//!
//! Dependency-free `f64` math (no `reify-solver-elastic` / `reify-ir::Value`
//! deps) so this module stays inside `reify-stdlib`. The `reify-eval` modal
//! trampoline (`modal_ops.rs`) calls these to convert eigen-solver output into
//! the `ModalResult` fields.

use std::f64::consts::PI;

/// Angular-frequency floor (rad/s) below which [`total_damping_ratio`] — and so
/// [`rayleigh_damping_ratio`], which delegates to it — reports `0.0` instead of
/// dividing by ω. A genuine flexible mode has ω ≫ this; only rigid-body /
/// spurious near-zero modes fall under it, where ζ = (α + β·ω²)/(2ω) → ∞ is
/// non-physical (a rigid-body mode carries no modal damping) and the
/// modal-strain-energy ratio is 0/0. Distinct from the caller-supplied
/// [`is_rigid_body_mode`] tolerance: this constant only guards the near-zero-ω
/// degeneracy. Deliberately PRIVATE — [`total_damping_ratio`] is what makes
/// exporting it unnecessary, so the floor lives in exactly one place.
const MIN_OMEGA_FOR_DAMPING: f64 = 1e-9;

/// Natural frequency in Hz from a free-vibration eigenvalue `λ = ω²`
/// (rad²/s²): `f = √λ / (2π)`.
///
/// `λ ≤ 0` — a zero-energy rigid-body mode, or a spurious negative pair from
/// numerical noise — clamps to `0.0` rather than producing `NaN` from
/// `√(negative)`. A `NaN` λ likewise maps to `0.0` (the `λ > 0` predicate is
/// false). PRD §4.1 / §7. The clamp is unrelated to the unit crossing below.
///
/// This is a **D4 rad/cycle crossing** — the INVERSE of the one in
/// `trajectory::input_shape::build_train_for_shaper`: rad/s → Hz, i.e. dividing
/// out 2π rad·cycle⁻¹. That is its own crossing class in the angle-dimension
/// doctrine, distinct from the η = 1 rad SI-coherence identity; the typed layer
/// already forces it (FREQUENCY ≠ ANGULAR_VELOCITY as `DimensionVector`s), and
/// this `f64` boundary is where that distinction is lost — hence a
/// doctrine-citing comment rather than a retype. See
/// `docs/prds/v0_6/angle-dimension-completion.md` D4 (#6184).
///
/// This function is the module's ONLY unit crossing:
/// [`rayleigh_damping_ratio`] and [`total_damping_ratio`] below stay entirely in
/// rad/s and cross nothing, so a reader need not check them.
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
///
/// This is the zero-material special case of [`total_damping_ratio`], which is
/// the entry point for a descriptor that COMPOSES a modal-strain-energy term
/// with a Rayleigh companion (`MaterialDamping`, task #6878). Delegating keeps
/// the floor comparison — and [`MIN_OMEGA_FOR_DAMPING`] itself — in exactly one
/// branch in this crate.
pub fn rayleigh_damping_ratio(alpha: f64, beta: f64, omega: f64) -> f64 {
    total_damping_ratio(0.0, alpha, beta, omega)
}

/// Composed modal damping ratio ζ for one mode — a mode-independent
/// modal-strain-energy (MSE) term plus a Rayleigh companion (task #6878, PRD
/// leaf β of `docs/prds/v0_6/damped-modal-bonded-heterogeneous.md`):
///
/// ```text
/// ζ = ζ_material + (α + β·ω²) / (2·ω)      for |ω| >  MIN_OMEGA_FOR_DAMPING
/// ζ = 0                                    for |ω| <= MIN_OMEGA_FOR_DAMPING
/// ```
///
/// CANONICAL — this doc is the single in-repo expansion of the floor argument
/// (the function owns the floor: `MIN_OMEGA_FOR_DAMPING` is compared in exactly
/// one branch in the workspace). Every other site — `modal_ops.rs`'s producer,
/// plan and classifier docs, the `MaterialDamping` declaration in
/// `crates/reify-compiler/stdlib/modal_analysis.ri`, and the tests — CITES this
/// rather than restating it, so a later leaf that makes ζ genuinely
/// mode-dependent (heterogeneous MSE, #6883) has one place to edit. The
/// author-facing semantics of ζ_material itself, and the normative derivation,
/// are correspondingly owned by that `MaterialDamping` declaration and by
/// PRD §C5 — not repeated here beyond the premise the floor argument needs.
///
/// The floor is shared by BOTH halves, and each half justifies it on its own:
///
/// * Rayleigh half — ζ = (α + β·ω²)/(2ω) → ∞ as ω → 0; a rigid-body mode
///   carries no modal damping.
/// * MSE half — ζ_material = ½·(Σ_e η_e·SE_e)/(Σ_e SE_e). A rigid-body mode
///   stores no strain energy, so Σ_e SE_e = 0 and that ratio is 0/0,
///   **UNDEFINED** rather than 1. The degenerate single-material identity
///   ζ = η/2 (PRD C5) is derived from a ratio that is 1 "by construction" only
///   when the denominator is nonzero, so it does not reach the rigid-body case:
///   η/2 is not the correct answer there, 0 is. A reader must not "restore" the
///   identity below the floor by hoisting `zeta_material` out of the guard.
///
/// Above the floor the result is EXACTLY `zeta_material + rayleigh_damping_ratio(
/// alpha, beta, omega)` — same two f64 operations in the same order — so an
/// exactness pin written against the sum stays bit-for-bit valid.
///
/// One observable nuance of `rayleigh_damping_ratio` delegating here: for ω < 0
/// with α = β = 0 the old direct form produced `-0.0` (a `+0.0` numerator over a
/// negative denominator), whereas `0.0 + (-0.0)` is `+0.0`. Negative ω is
/// unreachable in this crate (ω = 2π·f with f ≥ 0 by
/// [`eigenvalue_to_frequency_hz`]'s clamp) and IEEE `-0.0 == 0.0`, so no
/// assertion can observe the difference — noted so the delegation reads as
/// deliberate rather than accidental.
pub fn total_damping_ratio(zeta_material: f64, alpha: f64, beta: f64, omega: f64) -> f64 {
    if omega.abs() <= MIN_OMEGA_FOR_DAMPING {
        0.0
    } else {
        zeta_material + (alpha + beta * omega * omega) / (2.0 * omega)
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

    // ── total_damping_ratio: ζ = ζ_material + (α + β·ω²)/(2ω), shared ω-floor ─
    //
    // WHY the material half is floored too — and why a later reader must not
    // "restore" the degenerate identity below the floor — is argued once, on
    // `total_damping_ratio`'s own doc comment above. Not restated here.

    /// THE DEFECT this helper exists to close: a rigid-body mode (ω = 0) under a
    /// bare `MaterialDamping()` over `Steel_AISI_1045` (η = 0.0006 ⇒
    /// ζ_material = 0.0003) must carry NO modal damping. Exact, not toleranced —
    /// the floor returns `0.0` itself, not something near it.
    #[test]
    fn total_damping_ratio_zero_omega_suppresses_material_term() {
        assert_eq!(total_damping_ratio(0.0003, 0.0, 0.0, 0.0), 0.0);
    }

    /// ω exactly at the floor is inclusive (`<=`), with BOTH halves nonzero so
    /// neither can leak through.
    #[test]
    fn total_damping_ratio_floor_boundary_is_inclusive() {
        assert_eq!(total_damping_ratio(0.0003, 0.5, 1e-4, 1e-9), 0.0);
    }

    /// The floor is on |ω|, mirroring `rayleigh_damping_ratio`'s own `.abs()`.
    #[test]
    fn total_damping_ratio_negative_near_zero_omega_guarded() {
        assert_eq!(total_damping_ratio(0.0003, 0.5, 1e-4, -1e-10), 0.0);
    }

    /// Just ABOVE the floor the material half is NOT suppressed — the guard is a
    /// singularity guard, not a blanket zeroing. At ω = 1e-6 the α/2ω term
    /// dominates, so the total far exceeds ζ_material alone.
    #[test]
    fn total_damping_ratio_just_above_floor_is_not_zeroed() {
        let zeta_material = 0.0003;
        let got = total_damping_ratio(zeta_material, 0.5, 1e-4, 1e-6);
        assert!(got > zeta_material, "got {got}, want > {zeta_material}");
    }

    /// In the physical band the composition is EXACTLY the sum of the two halves
    /// — bit-for-bit, at the measured fundamental of task #6878's cantilever
    /// fixture. This is what keeps that task's 1e-9 relative identity pins
    /// (`ζ = η/2` and `ζ = η/2 + β·ω/2`) provably untouched by the floor.
    #[test]
    fn total_damping_ratio_above_floor_is_exactly_the_sum() {
        let zeta_material = 0.0003;
        let (alpha, beta) = (0.0, 1e-4);
        let omega = 2.0 * PI * 444.175_847_658_660_7;
        assert_eq!(
            total_damping_ratio(zeta_material, alpha, beta, omega),
            zeta_material + rayleigh_damping_ratio(alpha, beta, omega)
        );
    }

    /// ζ_material = 0 reduces the composition to plain Rayleigh, on BOTH sides of
    /// the floor. Every pre-existing descriptor (`Absent`/`NoDamping`/`Rayleigh`/
    /// `Unsupported`) plans to ζ_material = 0, so this is the B4 no-regression
    /// argument at helper altitude.
    #[test]
    fn total_damping_ratio_zero_material_is_plain_rayleigh() {
        let (alpha, beta) = (2.0, 0.001);
        let above = 2.0 * PI * 41.3;
        assert_eq!(
            total_damping_ratio(0.0, alpha, beta, above),
            rayleigh_damping_ratio(alpha, beta, above)
        );
        let below = 1e-12;
        assert_eq!(
            total_damping_ratio(0.0, alpha, beta, below),
            rayleigh_damping_ratio(alpha, beta, below)
        );
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
