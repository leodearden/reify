// SPDX-License-Identifier: AGPL-3.0-or-later

//! FDM as-printed constitutive mapping — **R0 fidelity rung** (task θ / 3790).
//!
//! Where the R-fast rung (task δ, `reify-eval` `as_printed_material`) derives
//! per-zone effective properties from the stdlib `FDMProcess` alone
//! (Gibson-Ashby infill knockdown + a fixed build-Z modulus ratio of 0.67),
//! the R0 rung maps a **real sliced [`Toolpath`]** (task ζ,
//! [`crate::toolpath`]) to per-zone orthotropic constants using closed-form
//! physics computed from the *measured* deposition:
//!
//! - **Rodríguez 2003 orthotropic** ([`rodriguez_orthotropic`]) — the FDM
//!   mesostructure (continuous bead axis vs inter-bead necks/voids vs
//!   inter-layer bonds) gives a genuine `E1 > E2 > E3` ordering: stiffest
//!   along the bead, knocked down transverse in-plane, weakest in build-Z.
//! - **Halpin-Tsai fibre** ([`halpin_tsai_modulus`] /
//!   [`halpin_tsai_reinforced`]) — short-fibre stiffening of the base
//!   filament. Opt-in; **inert by default** (`vol_fraction = 0` returns the
//!   matrix modulus exactly), since the stdlib `FDMProcess` carries no fibre
//!   fields yet.
//! - **Lumped-cooling build-Z knockdown** ([`lumped_cooling_z_ratio`]) — the
//!   R0 replacement for R-fast's fixed `0.67`: a lumped-capacitance cooling
//!   model converts the interface deposition temperature and the inter-layer
//!   time into an inter-layer bond fraction `∈ (0, 1)` that scales `E3`.
//!
//! [`r0_region_materials`] buckets the toolpath beads by [`BeadRole`] into the
//! wall / skin / infill zones, measures each zone's mean width / height /
//! nominal temperature + dominant bead-centerline direction, and composes the
//! three laws into a per-zone [`OrthotropicConstants`] plus the frame x-axis
//! (the dominant local bead direction). The downstream θ `FDMPrint` trampoline
//! (`reify-eval`) wraps these into the `AsPrintedZones` material field.
//!
//! # Units
//!
//! [`crate::toolpath`] stores native G-code **millimetres**; this module owns
//! the **mm → SI** conversion (its module doc explicitly delegates it here).
//! Lengths exposed to the field (mean widths/heights, the toolpath AABB) are
//! converted to metres; constitutive moduli stay SI throughout (base material
//! Pa × dimensionless R0 factors).

use crate::correlation::{BaseElastic, OrthotropicConstants};

// ── Rodríguez 2003 orthotropic law ──────────────────────────────────────────

/// The two structural knockdown ratios of the Rodríguez mesostructure that turn
/// an isotropic base modulus into an FDM orthotropic stiffness.
///
/// Both are dimensionless multipliers in `(0, 1]`: `1.0` recovers in-plane /
/// inter-layer isotropy, `< 1` produces the genuine orthotropic split that
/// distinguishes R0 from the R-fast transverse-isotropic baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterMesostructure {
    /// In-plane transverse knockdown `E2 / E1 ∈ (0, 1]` — the fraction of
    /// along-bead stiffness retained across the inter-bead neck/void. `< 1`
    /// because load transverse to the raster crosses the weaker bead-to-bead
    /// contacts rather than continuous material.
    pub transverse_ratio: f64,
    /// Build-Z knockdown `E3 / E2 ∈ (0, 1]` — the inter-layer bond ratio
    /// (supplied by [`lumped_cooling_z_ratio`]). `< 1` makes build-Z the
    /// weakest axis (Contract C3 / C4 invariant).
    pub z_ratio: f64,
}

/// Closed-form Rodríguez 2003 orthotropic stiffness for a unidirectional FDM
/// raster (ASTM-style coupon mesostructure model).
///
/// Axis convention mirrors [`OrthotropicConstants`]: `1` = along-bead (strong),
/// `2` = transverse in-plane, `3` = build-Z (weakest). The closed form is:
///
/// - `E1 = E_base · ρ` — continuous deposited material along the bead, scaled by
///   the relative density `ρ` (`solid_fraction`); at `ρ = 1` it recovers the
///   base modulus.
/// - `E2 = E1 · transverse_ratio` — transverse load crosses the inter-bead
///   necks/voids.
/// - `E3 = E2 · z_ratio` — the inter-layer bond is the weakest path.
///
/// With `transverse_ratio < 1` and `z_ratio < 1` this yields `E1 > E2 > E3` by
/// construction. Shear moduli use the geometric-mean estimate per plane (no
/// independent shear data — same convention as
/// [`crate::correlation::effective_orthotropic`]); Poisson ratios default to
/// the isotropic base value; effective density is `ρ_base · ρ`.
pub fn rodriguez_orthotropic(
    base: BaseElastic,
    solid_fraction: f64,
    meso: RasterMesostructure,
) -> OrthotropicConstants {
    debug_assert!(
        solid_fraction > 0.0 && solid_fraction <= 1.0,
        "solid_fraction (relative density) must be in (0, 1]; got {solid_fraction}"
    );
    debug_assert!(
        meso.transverse_ratio > 0.0 && meso.transverse_ratio <= 1.0,
        "transverse_ratio must be in (0, 1]; got {}",
        meso.transverse_ratio
    );
    debug_assert!(
        meso.z_ratio > 0.0 && meso.z_ratio <= 1.0,
        "z_ratio must be in (0, 1]; got {}",
        meso.z_ratio
    );

    // Along-bead (strong) → transverse in-plane → build-Z (weakest).
    let e1 = base.youngs_modulus * solid_fraction;
    let e2 = e1 * meso.transverse_ratio;
    let e3 = e2 * meso.z_ratio;

    let nu12 = base.poisson_ratio;
    let nu13 = base.poisson_ratio;
    let nu23 = base.poisson_ratio;

    // Geometric-mean shear estimate per plane (no independent shear data).
    let g12 = (e1 * e2).sqrt() / (2.0 * (1.0 + nu12));
    let g13 = (e1 * e3).sqrt() / (2.0 * (1.0 + nu13));
    let g23 = (e2 * e3).sqrt() / (2.0 * (1.0 + nu23));

    let density = base.density * solid_fraction;

    OrthotropicConstants {
        e1,
        e2,
        e3,
        g12,
        g13,
        g23,
        nu12,
        nu13,
        nu23,
        density,
    }
}

// ── Halpin-Tsai short-fibre reinforcement ───────────────────────────────────

/// Short-fibre reinforcement of the base filament (opt-in; **inert by
/// default**). The stdlib `FDMProcess` carries no fibre fields yet, so the R0
/// mapping passes a zero-`vol_fraction` fibre (an exact no-op) unless a future
/// fibre-filament surface supplies one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fibre {
    /// Fibre Young's modulus, Pa (e.g. glass ≈ 70 GPa, carbon ≈ 230 GPa).
    pub modulus: f64,
    /// Fibre volume fraction `∈ [0, 1)`. `0` ⇒ inert (no reinforcement).
    pub vol_fraction: f64,
    /// Fibre aspect ratio `l / d` (length over diameter); larger ⇒ stiffer
    /// (the Halpin-Tsai `ξ` grows, approaching the Voigt bound).
    pub aspect_ratio: f64,
}

impl Fibre {
    /// The inert (no-reinforcement) fibre: `vol_fraction = 0`, so
    /// [`halpin_tsai_reinforced`] returns the base material unchanged.
    pub const INERT: Fibre = Fibre {
        modulus: 0.0,
        vol_fraction: 0.0,
        aspect_ratio: 1.0,
    };
}

/// Halpin-Tsai longitudinal modulus of a short-fibre composite.
///
/// `E = E_m · (1 + ξ·η·V_f) / (1 − η·V_f)` with `η = (E_f/E_m − 1)/(E_f/E_m + ξ)`
/// and `ξ = 2·(l/d)` (the standard aspect-ratio reinforcing factor for the
/// fibre-axis modulus). At `V_f = 0` this is exactly `E_m` (`η·V_f = 0`). For
/// `E_f > E_m` and `V_f > 0` it lies strictly between `E_m` and the Voigt bound
/// `E_m(1−V_f) + E_f·V_f`, increasing monotonically in both `V_f` and the
/// aspect ratio.
pub fn halpin_tsai_modulus(matrix_e: f64, fibre_e: f64, vol_fraction: f64, aspect_ratio: f64) -> f64 {
    let xi = 2.0 * aspect_ratio;
    let ratio = fibre_e / matrix_e;
    let eta = (ratio - 1.0) / (ratio + xi);
    matrix_e * (1.0 + xi * eta * vol_fraction) / (1.0 - eta * vol_fraction)
}

/// Apply [`halpin_tsai_modulus`] reinforcement to a base filament, returning the
/// reinforced [`BaseElastic`]. Poisson ratio and density are carried through
/// unchanged (the R0 mapping reinforces stiffness only; a full mixture density
/// is a future fibre-surface concern). With [`Fibre::INERT`] (or any
/// `vol_fraction = 0`) the base is returned **exactly unchanged**.
pub fn halpin_tsai_reinforced(base: BaseElastic, fibre: &Fibre) -> BaseElastic {
    BaseElastic {
        youngs_modulus: halpin_tsai_modulus(
            base.youngs_modulus,
            fibre.modulus,
            fibre.vol_fraction,
            fibre.aspect_ratio,
        ),
        ..base
    }
}
