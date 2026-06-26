// SPDX-License-Identifier: AGPL-3.0-or-later

//! FDM as-printed constitutive mapping — **R0 fidelity rung** (task θ / 3790).
//!
//! Where the R-fast rung (task δ, `reify-eval` `as_printed_material`) derives
//! per-zone effective properties from the stdlib `FDMProcess` alone
//! (Gibson-Ashby infill knockdown + a fixed build-Z modulus ratio of 0.67),
//! the R0 rung maps a **real sliced [`Toolpath`]** (task ζ,
//! [`crate::toolpath`]) to per-zone orthotropic constants using closed-form
//! physics. The toolpath supplies the *measured* per-zone inputs — mean bead
//! width / height / nominal temperature, dominant bead direction, and (through
//! the lumped-cooling model) the build-Z knockdown ratio. The in-plane
//! transverse split `E2/E1`, by contrast, is a **fixed Rodríguez-model
//! parameter** ([`R0_TRANSVERSE_RATIO`]), not a toolpath-derived quantity. The
//! three composed laws are:
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
use crate::toolpath::{Bead, BeadRole, Toolpath};

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

// ── Lumped-cooling build-Z knockdown ────────────────────────────────────────

/// Inter-layer bond fraction from a lumped-capacitance cooling model — the R0
/// build-Z modulus knockdown `E3/E2 ∈ (0, 1)` that replaces R-fast's fixed
/// `0.67`.
///
/// The just-deposited interface cools from the nominal deposition temperature
/// toward ambient over the inter-layer time `t` with time constant `τ`:
///
/// ```text
/// T_interface = T_ambient + (T_nominal − T_ambient)·exp(−t/τ)
/// ```
///
/// The bond fraction is `1 − exp(−ΔT / temp_scale)` where `ΔT = T_interface −
/// T_ambient` — a still-hot interface (large `ΔT`) re-melts and fuses the
/// incoming layer well (ratio → 1); a cold interface barely bonds (ratio → 0).
/// For valid inputs (`T_nominal > T_ambient`, positive `t`, `τ`, `temp_scale`)
/// the result is strictly in `(0, 1)`, rises monotonically with `T_nominal`,
/// and falls monotonically with the inter-layer time. A defensive `clamp` keeps
/// degenerate inputs inside the open interval.
pub fn lumped_cooling_z_ratio(
    nominal_temp: f64,
    ambient_temp: f64,
    layer_time: f64,
    cooling_tau: f64,
    temp_scale: f64,
) -> f64 {
    let interface_temp =
        ambient_temp + (nominal_temp - ambient_temp) * (-layer_time / cooling_tau).exp();
    let delta = interface_temp - ambient_temp;
    let ratio = 1.0 - (-delta / temp_scale).exp();
    // Open-interval safety net for degenerate inputs (nominal ≤ ambient,
    // non-positive τ/scale); valid inputs already land strictly inside.
    ratio.clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON)
}

// ── Toolpath → per-zone R0 materials ────────────────────────────────────────

/// Native G-code millimetres → SI metres.
const MM_TO_M: f64 = 1.0e-3;

/// In-plane transverse neck knockdown `E2/E1` for the R0 raster mesostructure
/// (`< 1` ⇒ genuine orthotropy, the structural differentiator from the R-fast
/// transverse-isotropic baseline).
///
/// This is a **fixed Rodríguez-model parameter**, NOT a toolpath-measured
/// quantity: every zone receives the same `E2/E1` regardless of the sliced
/// deposition, so the integration test's `e1 ≠ e2` assertion verifies that the
/// split is non-unity (orthotropy is present), not that it tracks the slice.
/// R0's *measured* anisotropy enters elsewhere — through the cooling-derived
/// build-Z ratio ([`lumped_cooling_z_ratio`], a function of the per-zone mean
/// temperature + inter-layer time) and the per-zone mean geometry / dominant
/// bead direction. Deriving the transverse split from raster geometry (bead
/// spacing / overlap vs nominal width) is a higher-rung refinement.
pub const R0_TRANSVERSE_RATIO: f64 = 0.8;

/// Default build-chamber ambient temperature, °C (lumped-cooling reference).
pub const DEFAULT_AMBIENT_TEMP_C: f64 = 25.0;
/// Default lumped-capacitance cooling time constant, s.
pub const DEFAULT_COOLING_TAU_S: f64 = 8.0;
/// Default inter-layer bond temperature scale, °C.
pub const DEFAULT_BOND_TEMP_SCALE_C: f64 = 60.0;

// Dense fallback used when the *whole* toolpath is empty, so the field stays
// total (every zone classifies into a material) even with no beads.
const FALLBACK_WIDTH_MM: f64 = 0.45;
const FALLBACK_HEIGHT_MM: f64 = 0.2;
const FALLBACK_TEMP_C: f64 = 210.0;
const FALLBACK_LAYER_TIME_S: f64 = 10.0;
const FALLBACK_DIR: [f64; 3] = [1.0, 0.0, 0.0];

/// Non-toolpath inputs to the R0 constitutive mapping (the stdlib `FDMProcess`
/// + `AsPrintedOptions` half).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct R0Options {
    /// Sparse-infill relative density `ρ ∈ (0, 1]` (mirrors
    /// `FDMProcess.infill_density`). Dense zones (wall / skin) always use `ρ=1`.
    pub infill_density: f64,
    /// Optional Halpin-Tsai fibre reinforcement. `None` ⇒ inert
    /// ([`Fibre::INERT`]).
    pub fibre: Option<Fibre>,
    /// Build direction (the frame z-axis / weakest axis). Carried for the
    /// downstream trampoline's frame; the bead direction supplies the x-axis.
    pub build_direction: [f64; 3],
    /// Lumped-cooling ambient temperature, °C.
    pub ambient_temp_c: f64,
    /// Lumped-cooling time constant, s.
    pub cooling_tau_s: f64,
    /// Lumped-cooling bond temperature scale, °C.
    pub temp_scale_c: f64,
}

impl Default for R0Options {
    fn default() -> Self {
        R0Options {
            infill_density: 0.2,
            fibre: None,
            build_direction: [0.0, 0.0, 1.0],
            ambient_temp_c: DEFAULT_AMBIENT_TEMP_C,
            cooling_tau_s: DEFAULT_COOLING_TAU_S,
            temp_scale_c: DEFAULT_BOND_TEMP_SCALE_C,
        }
    }
}

/// One printed zone's R0 material: the orthotropic constants plus the frame
/// x-axis (dominant local bead direction) and the measured mean bead geometry
/// (SI metres — θ owns the mm→SI conversion).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct R0Region {
    /// Rodríguez orthotropic constants (build-Z weakest).
    pub constants: OrthotropicConstants,
    /// Frame x-axis: the dominant unit bead-centerline direction in the zone.
    pub bead_direction: [f64; 3],
    /// Mean extrusion width, metres.
    pub mean_width_m: f64,
    /// Mean layer height, metres.
    pub mean_height_m: f64,
}

/// The three printed zones of the R0 as-printed material field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct R0RegionMaterials {
    /// Perimeter shell (dense, `ρ=1`).
    pub wall: R0Region,
    /// Solid top/bottom skin (dense, `ρ=1`).
    pub skin: R0Region,
    /// Sparse interior lattice (`ρ = infill_density`).
    pub infill: R0Region,
}

/// Map a sliced [`Toolpath`] to per-zone orthotropic R0 materials.
///
/// Beads are bucketed by [`BeadRole`]: `Perimeter → wall`, `SolidInfill → skin`
/// (both dense, `ρ=1`), `SparseInfill → infill` (`ρ = opts.infill_density`).
/// Each zone's mean width / height / nominal temperature + dominant bead
/// direction are measured from its beads; the three R0 laws are then composed
/// — `halpin_tsai_reinforced` (inert by default) feeds
/// `rodriguez_orthotropic`, whose build-Z knockdown comes from
/// `lumped_cooling_z_ratio`. An empty zone falls back to the whole-toolpath
/// aggregate (or, if the toolpath has no part beads at all, to a dense default)
/// so the field stays total.
pub fn r0_region_materials(
    toolpath: &Toolpath,
    base: BaseElastic,
    opts: &R0Options,
) -> R0RegionMaterials {
    let wall_beads: Vec<&Bead> = role_beads(toolpath, BeadRole::Perimeter);
    let skin_beads: Vec<&Bead> = role_beads(toolpath, BeadRole::SolidInfill);
    let infill_beads: Vec<&Bead> = role_beads(toolpath, BeadRole::SparseInfill);
    // Whole-toolpath fallback for an empty role (every part bead, regardless of
    // role) — keeps an unpopulated zone coherent with the rest of the body.
    let all_beads: Vec<&Bead> = toolpath.beads.iter().collect();

    R0RegionMaterials {
        wall: build_region(&wall_beads, &all_beads, 1.0, base, opts),
        skin: build_region(&skin_beads, &all_beads, 1.0, base, opts),
        infill: build_region(&infill_beads, &all_beads, opts.infill_density, base, opts),
    }
}

/// Borrow every bead with the given role.
fn role_beads(toolpath: &Toolpath, role: BeadRole) -> Vec<&Bead> {
    toolpath.beads.iter().filter(|b| b.role == role).collect()
}

/// Per-zone measured deposition statistics (native mm / °C / s).
struct BeadStats {
    mean_width_mm: f64,
    mean_height_mm: f64,
    mean_temp_c: f64,
    mean_layer_time_s: f64,
    dominant_dir: [f64; 3],
}

/// Compose the R0 laws for one zone from its beads (or the fallback set).
fn build_region(
    role_beads: &[&Bead],
    fallback_beads: &[&Bead],
    rho: f64,
    base: BaseElastic,
    opts: &R0Options,
) -> R0Region {
    // The role's own beads if populated, else the whole-toolpath fallback.
    let beads: &[&Bead] = if role_beads.is_empty() {
        fallback_beads
    } else {
        role_beads
    };
    let stats = aggregate(beads).unwrap_or_else(fallback_stats);

    // halpin_tsai_reinforced (inert default) → rodriguez_orthotropic, with the
    // build-Z knockdown from the lumped-cooling model.
    let reinforced = halpin_tsai_reinforced(base, &opts.fibre.unwrap_or(Fibre::INERT));
    let z_ratio = lumped_cooling_z_ratio(
        stats.mean_temp_c,
        opts.ambient_temp_c,
        stats.mean_layer_time_s,
        opts.cooling_tau_s,
        opts.temp_scale_c,
    );
    let meso = RasterMesostructure {
        transverse_ratio: R0_TRANSVERSE_RATIO,
        z_ratio,
    };
    let rho = rho.clamp(f64::MIN_POSITIVE, 1.0);
    let constants = rodriguez_orthotropic(reinforced, rho, meso);

    R0Region {
        constants,
        bead_direction: stats.dominant_dir,
        mean_width_m: stats.mean_width_mm * MM_TO_M,
        mean_height_m: stats.mean_height_mm * MM_TO_M,
    }
}

/// Mean width / height / temperature / per-bead deposition time + dominant
/// direction over a bead set, or `None` if empty.
fn aggregate(beads: &[&Bead]) -> Option<BeadStats> {
    if beads.is_empty() {
        return None;
    }
    let n = beads.len() as f64;
    let mean_width_mm = beads.iter().map(|b| b.width).sum::<f64>() / n;
    let mean_height_mm = beads.iter().map(|b| b.height).sum::<f64>() / n;
    let mean_temp_c = beads.iter().map(|b| b.nominal_temp).sum::<f64>() / n;

    // Per-bead deposition time = centerline length / feedrate (mm / (mm·min⁻¹)
    // → min → s). A non-positive feedrate / zero-length bead contributes none;
    // if no bead yields a time, fall back to a default.
    let mut total_time = 0.0;
    let mut time_count = 0u32;
    for b in beads {
        let len = polyline_length_mm(&b.centerline);
        if b.speed > 0.0 && len > 0.0 {
            total_time += len / b.speed * 60.0;
            time_count += 1;
        }
    }
    let mean_layer_time_s = if time_count == 0 {
        FALLBACK_LAYER_TIME_S
    } else {
        total_time / f64::from(time_count)
    };

    let dominant_dir = dominant_direction(beads).unwrap_or(FALLBACK_DIR);

    Some(BeadStats {
        mean_width_mm,
        mean_height_mm,
        mean_temp_c,
        mean_layer_time_s,
        dominant_dir,
    })
}

/// Dense default statistics when a toolpath has no part beads at all, so
/// [`r0_region_materials`] stays **total** — every zone still classifies into a
/// finite, build-Z-weakest material — even for a beadless toolpath.
///
/// This is a **unit-level-only safety net** for direct library callers that
/// supply their own field domain. The θ `FDMPrint` trampoline (`reify-eval`
/// `as_printed_material_r0`) never reaches it: a beadless toolpath has no
/// bead-centerline AABB, so the trampoline degrades to an `Undef`-lambda field
/// *before* any material is sampled. The two layers thus share one policy — a
/// beadless toolpath defines no field domain — expressed two ways: the library
/// function keeps its return total, while the trampoline (which owns the field
/// domain) degrades honestly. Pinned by
/// `r0_region_materials_beadless_toolpath_stays_total`.
fn fallback_stats() -> BeadStats {
    BeadStats {
        mean_width_mm: FALLBACK_WIDTH_MM,
        mean_height_mm: FALLBACK_HEIGHT_MM,
        mean_temp_c: FALLBACK_TEMP_C,
        mean_layer_time_s: FALLBACK_LAYER_TIME_S,
        dominant_dir: FALLBACK_DIR,
    }
}

/// Total length (mm) of a centerline polyline.
fn polyline_length_mm(pts: &[[f64; 3]]) -> f64 {
    pts.windows(2)
        .map(|w| {
            let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .sum()
}

/// The dominant bead-centerline direction in a zone: the unit direction of the
/// **longest single segment** (sign-canonicalised so it is deterministic). This
/// is, by construction, parallel to a real deposited bead — the frame x-axis.
fn dominant_direction(beads: &[&Bead]) -> Option<[f64; 3]> {
    let mut best_len = 0.0;
    let mut best_dir = None;
    for b in beads {
        for w in b.centerline.windows(2) {
            let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if len > best_len + 1e-12 {
                best_len = len;
                best_dir = Some([d[0] / len, d[1] / len, d[2] / len]);
            }
        }
    }
    best_dir.map(canonicalize_sign)
}

/// Flip a direction so its first non-negligible component is positive (a line
/// and its reverse are the same orientation; this picks one deterministically).
fn canonicalize_sign(d: [f64; 3]) -> [f64; 3] {
    let lead = d.iter().copied().find(|c| c.abs() > 1e-12).unwrap_or(0.0);
    if lead < 0.0 {
        [-d[0], -d[1], -d[2]]
    } else {
        d
    }
}
