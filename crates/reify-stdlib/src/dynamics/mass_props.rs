//! Pure `f64` mass-properties helpers for the `body_mass_props` stdlib fn
//! (RBD-β, task 3829; PRD `docs/prds/v0_3/rigid-body-dynamics.md` §2.1/§5.4).
//!
//! Dependency-free `f64` math — no `reify_ir::Value`, no diagnostics sink, no
//! geometry kernel. Mirrors the modal split (`reify-stdlib/src/modal/
//! free_vibration.rs` holds pure scalar math; `reify-eval/src/dynamics_ops.rs`
//! owns the `Value`/diagnostic/kernel wiring that calls into this module).
//!
//! Two responsibilities:
//!   * [`resolve_density`] — the fn-level density priority ladder
//!     (explicit arg > body `Material` density > default water).
//!   * `uniform_box_inertia` (added in step-4) — the closed-form analytic
//!     ground-truth mass/com/inertia for a uniform-density box, the value the
//!     deferred KGQ kernel query (task 3620) will later be cross-checked against.

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_density priority ladder ──────────────────────────────────────

    #[test]
    fn explicit_density_wins_over_material() {
        // (a) explicit Some(2700) beats material Some(7850): the explicit
        // `density` arg is the highest fn-level ladder rung.
        let (rho, src) = resolve_density(Some(2700.0), Some(7850.0));
        assert_eq!(rho, 2700.0, "explicit density must be used verbatim");
        assert_eq!(src, DensitySource::Explicit);
    }

    #[test]
    fn material_density_used_when_no_explicit() {
        // (b) explicit None, material Some(7850) -> Material rung.
        let (rho, src) = resolve_density(None, Some(7850.0));
        assert_eq!(rho, 7850.0, "material density must be used when no explicit arg");
        assert_eq!(src, DensitySource::Material);
    }

    #[test]
    fn defaults_to_water_when_neither_present() {
        // (c) explicit None, material None -> default water 1000 kg/m³.
        let (rho, src) = resolve_density(None, None);
        assert_eq!(rho, 1000.0, "must fall back to the 1000 kg/m³ water default");
        assert_eq!(src, DensitySource::DefaultWater);
    }
}
