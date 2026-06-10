//! Pure `f64` mass-properties helpers for the `body_mass_props` stdlib fn
//! (RBD-β, task 3829; PRD `docs/prds/v0_3/rigid-body-dynamics.md` §2.1/§5.4).
//!
//! Dependency-free `f64` math — no `reify_ir::Value`, no diagnostics sink, no
//! geometry kernel. Mirrors the modal split (`reify-stdlib/src/modal/
//! free_vibration.rs` holds pure scalar math; `reify-eval/src/dynamics_ops.rs`
//! owns the `Value`/diagnostic/kernel wiring that calls into this module).
//!
//! Two responsibilities:
//!   * [`resolve_density_strict`] — the shared explicit→material rung-walk,
//!     returning `None` when neither source is present (strict / no-water tail).
//!   * [`resolve_density`] — thin wrapper over `resolve_density_strict` that
//!     adds the `DEFAULT_DENSITY_KG_M3` water tail used by `body_mass_props`
//!     (explicit arg > body `Material` density > default water).
//!   * `uniform_box_inertia` (added in step-4) — the closed-form analytic
//!     ground-truth mass/com/inertia for a uniform-density box, the value the
//!     deferred KGQ kernel query (task 3620) will later be cross-checked against.

/// Default mass density (kg/m³) used when `body_mass_props` can resolve no
/// other density: the density of water at ~4 °C.
///
/// PRD `docs/prds/v0_3/rigid-body-dynamics.md` §5.4 specifies water (1000
/// kg/m³) as the bottom rung of the density ladder so a body with neither an
/// explicit `density` argument nor a `Material` density still yields a
/// physically-plausible (if approximate) inertial estimate rather than zero
/// mass or `Undef`. Falling back to this value also raises
/// [`DensitySource::DefaultWater`], which the eval layer turns into the
/// `W_DynamicsDefaultDensity` advisory warning.
pub const DEFAULT_DENSITY_KG_M3: f64 = 1000.0;

/// Which rung of the [`resolve_density`] priority ladder supplied the density.
///
/// Returned alongside the resolved density so the eval layer can decide whether
/// to emit the `W_DynamicsDefaultDensity` warning (only on
/// [`DefaultWater`](DensitySource::DefaultWater)). The pure layer itself stays
/// diagnostic-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DensitySource {
    /// The caller passed an explicit `density` argument to `body_mass_props`.
    Explicit,
    /// No explicit argument; the body's `Material.density` was used.
    Material,
    /// Neither was available; the [`DEFAULT_DENSITY_KG_M3`] water default was
    /// used. The eval layer emits `W_DynamicsDefaultDensity` for this case.
    DefaultWater,
}

/// Walk the explicit→material priority ladder and return the winning rung, or
/// `None` if neither source is present (strict / no-water tail).
///
/// This is the **single canonical definition** of the density rung-walk.
/// Both `body_mass_props` (via [`resolve_density`]) and modal FEA (via
/// `extract_density_or_degenerate`) delegate here so the ladder is defined in
/// exactly one place; callers differ only in how they handle the `None` tail:
///
/// * [`resolve_density`] maps `None → (DEFAULT_DENSITY_KG_M3, DefaultWater)`.
/// * `modal_ops::extract_density_or_degenerate` maps `None → E_ModalNoMassMatrix`
///   (eigenfrequencies scale with √(1/ρ); a silent ρ=1000 would yield
///   plausible-but-wrong physics).
///
/// Like [`resolve_density`] this is pure `f64` selection — no validation of
/// the magnitude (a non-positive or `NaN` density is returned verbatim).
pub fn resolve_density_strict(
    explicit: Option<f64>,
    material: Option<f64>,
) -> Option<(f64, DensitySource)> {
    if let Some(rho) = explicit {
        Some((rho, DensitySource::Explicit))
    } else {
        material.map(|rho| (rho, DensitySource::Material))
    }
}

/// Resolve the mass density for `body_mass_props` via the fn-level priority
/// ladder (PRD §5.4): an explicit `density` argument wins; failing that, the
/// body's `Material` density; failing that, the [`DEFAULT_DENSITY_KG_M3`] water
/// default.
///
/// Returns the chosen density (kg/m³) paired with the [`DensitySource`] rung it
/// came from, so the caller knows whether a default-density warning is due.
/// This is pure `f64` selection — no validation of the magnitude (a non-positive
/// or `NaN` density is returned verbatim; physical validity of the resulting
/// inertia is enforced downstream by the existing MassProperties PSD hook).
///
/// Implemented as a thin wrapper over [`resolve_density_strict`] so the
/// explicit→material ladder is defined in exactly one place.
pub fn resolve_density(explicit: Option<f64>, material: Option<f64>) -> (f64, DensitySource) {
    resolve_density_strict(explicit, material)
        .unwrap_or((DEFAULT_DENSITY_KG_M3, DensitySource::DefaultWater))
}

/// Closed-form mass/center-of-mass/inertia of a uniform-density axis-aligned
/// box with edge lengths `dims = [a, b, c]` (metres) and the given `density`
/// (kg/m³), expressed in a **corner-origin** body frame (one corner at the
/// frame origin, edges along +x/+y/+z).
///
/// Returns `(mass, com, inertia)` where:
///   * `mass = ρ·a·b·c`,
///   * `com = [a/2, b/2, c/2]` (the box's geometric centre relative to the
///     corner origin),
///   * `inertia` is the 3×3 tensor **about the centre of mass** — a diagonal
///     matrix `m/12 · diag(b²+c², a²+c², a²+b²)` with zero products of inertia
///     (the principal axes of a box align with its edges).
///
/// This is the analytic ground truth referenced by the RBD PRD
/// (`docs/prds/v0_3/rigid-body-dynamics.md` §10 Phase 1 β): the value the
/// density-aware KGQ kernel query (task 3620 / KGQ-λ `moment_of_inertia`) will
/// later be cross-checked against once it is wired into `body_mass_props`. It
/// is `pub` so that future supervisor wiring and its cross-validation test can
/// reuse the exact same closed form.
#[allow(dead_code)] // G-allow: test-only analytic ground-truth closed form; KGQ wiring into body_mass_props landed via #3829 (done) + #4237 dynamics_ops seam (done); fn is permanent test-only helper, zero production callers by design
pub fn uniform_box_inertia(dims: [f64; 3], density: f64) -> (f64, [f64; 3], [[f64; 3]; 3]) {
    let [a, b, c] = dims;
    let mass = density * a * b * c;
    let com = [a / 2.0, b / 2.0, c / 2.0];
    let coeff = mass / 12.0;
    let inertia = [
        [coeff * (b * b + c * c), 0.0, 0.0],
        [0.0, coeff * (a * a + c * c), 0.0],
        [0.0, 0.0, coeff * (a * a + b * b)],
    ];
    (mass, com, inertia)
}

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

    // ── resolve_density_strict — strict (None) tail ──────────────────────────

    #[test]
    fn strict_explicit_density_wins_over_material() {
        // (a) explicit Some(2700) beats material Some(7850): the explicit
        // `density` arg is the highest ladder rung.
        let result = resolve_density_strict(Some(2700.0), Some(7850.0));
        assert_eq!(
            result,
            Some((2700.0, DensitySource::Explicit)),
            "explicit rung must win verbatim"
        );
    }

    #[test]
    fn strict_material_density_used_when_no_explicit() {
        // (b) explicit None, material Some(7850) -> Material rung.
        let result = resolve_density_strict(None, Some(7850.0));
        assert_eq!(
            result,
            Some((7850.0, DensitySource::Material)),
            "material rung must be used when no explicit arg"
        );
    }

    #[test]
    fn strict_returns_none_when_neither_present() {
        // (c) explicit None, material None -> STRICT tail: no water fallback.
        let result = resolve_density_strict(None, None);
        assert_eq!(result, None, "strict tail must return None, not water");
    }

    #[test]
    fn strict_shared_rung_walk_invariant() {
        // (d) Invariant: for the explicit and material rungs,
        //     resolve_density_strict(e, m) == Some(resolve_density(e, m)).
        //     At the empty tail the two functions diverge by design:
        //     resolve_density(None,None) == (1000.0, DefaultWater)
        //     resolve_density_strict(None,None) == None.

        // explicit rung — both agree
        let strict_e = resolve_density_strict(Some(2700.0), Some(7850.0));
        let water_e = resolve_density(Some(2700.0), Some(7850.0));
        assert_eq!(
            strict_e,
            Some(water_e),
            "on the explicit rung strict and water wrappers must agree"
        );

        // material rung — both agree
        let strict_m = resolve_density_strict(None, Some(7850.0));
        let water_m = resolve_density(None, Some(7850.0));
        assert_eq!(
            strict_m,
            Some(water_m),
            "on the material rung strict and water wrappers must agree"
        );

        // empty tail — intentional divergence
        let strict_none = resolve_density_strict(None, None);
        let (water_rho, water_src) = resolve_density(None, None);
        assert_eq!(strict_none, None, "strict tail must be None");
        assert_eq!(
            water_rho, 1000.0,
            "water wrapper must fall back to 1000 kg/m³"
        );
        assert_eq!(
            water_src,
            DensitySource::DefaultWater,
            "water wrapper must report DefaultWater"
        );
    }

    // ── uniform_box_inertia analytic ground truth ────────────────────────────

    #[test]
    fn uniform_box_inertia_matches_hand_computed_values() {
        // Distinct extents so all three inertia diagonal entries differ.
        // a=0.1, b=0.2, c=0.3 m; ρ=1000 kg/m³.
        //
        // Hand-computed expected values (independent of the impl, so this pins
        // real numbers rather than impl==impl):
        //   mass = ρ·a·b·c = 1000·0.1·0.2·0.3 = 6.0 kg
        //   com  = [a/2, b/2, c/2] = [0.05, 0.10, 0.15] (corner-origin box)
        //   Ixx  = m/12·(b²+c²) = 6/12·(0.04+0.09) = 0.5·0.13 = 0.065
        //   Iyy  = m/12·(a²+c²) = 6/12·(0.01+0.09) = 0.5·0.10 = 0.05
        //   Izz  = m/12·(a²+b²) = 6/12·(0.01+0.04) = 0.5·0.05 = 0.025
        //   all products of inertia = 0
        let (mass, com, inertia) = uniform_box_inertia([0.1, 0.2, 0.3], 1000.0);

        assert!((mass - 6.0).abs() < 1e-12, "mass should be 6.0 kg, got {mass}");

        let expected_com = [0.05, 0.10, 0.15];
        for (i, (got, want)) in com.iter().zip(expected_com.iter()).enumerate() {
            assert!(
                (got - want).abs() < 1e-12,
                "com[{i}] should be {want}, got {got}"
            );
        }

        let expected_diag = [0.065, 0.05, 0.025];
        for (r, row) in inertia.iter().enumerate() {
            for (c, &got) in row.iter().enumerate() {
                let want = if r == c { expected_diag[r] } else { 0.0 };
                assert!(
                    (got - want).abs() < 1e-12,
                    "inertia[{r}][{c}] should be {want}, got {got}"
                );
            }
        }
    }
}
