// SPDX-License-Identifier: AGPL-3.0-or-later

//! R-fast as-printed wiring (task δ) and rung-selection policy (task ι / 3791).
//!
//! Combines the two zero-dependency pieces of the FDM as-printed model into
//! the per-point constitutive constants the δ ComputeNode samples across a
//! body to build a `Field<Point3<Length>, AnisotropicMaterial>`:
//!
//! - [`zone`](crate::zone) (task γ): maps a query point to a Wall / Skin /
//!   Infill zone against the body's bounding box.
//! - [`correlation`](crate::correlation) (task β): turns a base filament
//!   material + infill solid fraction + pattern into transverse-isotropic
//!   (default) or orthotropic (opt-in) effective constants.
//!
//! The wiring is a thin, pure composition: classify the point → map the zone
//! to a solid fraction (dense walls/skins, sparse infill) → run the β
//! correlation. Walls and skins are fully dense (ρ = 1.0); only the infill
//! interior is knocked down by the process `infill_density`, which is what
//! makes the resulting material field non-constant. In every zone the build
//! (Z) axis is the weakest direction (β's `BUILD_Z_MODULUS_RATIO` knockdown,
//! PRD §C4 invariant).
//!
//! Real-body distance probes (OCCT) are a higher-rung concern; R-fast uses
//! γ's analytic [`AxisAlignedBox`] probe, which is exact for box bodies (the
//! δ user-observable signal).

use crate::correlation::{
    BaseElastic, CouponOverride, InfillPattern, OrthotropicConstants, TransverseIsoConstants,
    effective_orthotropic, effective_transverse_isotropic,
};
use crate::zone::{AxisAlignedBox, Zone, ZoneProcessParams, classify_zone};

/// Classify a single query `point` into a [`Zone`] against the body `aabb`.
///
/// Convenience composition of γ's [`AxisAlignedBox::build_zone_probe`] +
/// [`classify_zone`]: builds the side / top-bottom distance probe for `point`
/// using the build axis carried in `params`, then runs the Wall → Skin →
/// Infill cascade. `cos_threshold` is the normal-vs-build-direction cutoff
/// (use [`crate::DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD`]).
pub fn classify_point(
    aabb: &AxisAlignedBox,
    params: &ZoneProcessParams,
    cos_threshold: f64,
    point: [f64; 3],
) -> Zone {
    classify_zone(&aabb.build_zone_probe(point, params, cos_threshold), params)
}

/// Map a [`Zone`] to its solid (relative-density) fraction ρ ∈ (0, 1].
///
/// Walls and skins are solid perimeters / solid layers — fully dense
/// (ρ = 1.0). Only the sparse infill interior carries the process
/// `infill_density`; this is the single source of the field's spatial
/// variation. The returned fraction feeds the β `solid_fraction` argument.
pub fn zone_solid_fraction(zone: Zone, infill_density: f64) -> f64 {
    match zone {
        Zone::Wall | Zone::Skin => 1.0,
        Zone::Infill => infill_density,
    }
}

/// Transverse-isotropic effective constants at a single `point` (the default
/// constitutive model).
///
/// Classifies the point → maps the zone to a solid fraction → runs
/// [`effective_transverse_isotropic`]. The in-plane (print-plane) is the
/// isotropy plane; the axial direction is the (weakest) build axis. Coupon
/// overrides in `coupon` beat the computed defaults per the β contract.
///
/// `infill_density` must be in (0, 1] — for an `Infill` point the returned
/// solid fraction IS `infill_density`, so the β domain `debug_assert!` guards
/// the caller's value. Walls/skins use ρ = 1.0 unconditionally.
#[allow(clippy::too_many_arguments)]
pub fn material_constants_at(
    aabb: &AxisAlignedBox,
    params: &ZoneProcessParams,
    cos_threshold: f64,
    base: BaseElastic,
    pattern: InfillPattern,
    infill_density: f64,
    coupon: &CouponOverride,
    point: [f64; 3],
) -> TransverseIsoConstants {
    let zone = classify_point(aabb, params, cos_threshold, point);
    let solid_fraction = zone_solid_fraction(zone, infill_density);
    effective_transverse_isotropic(base, solid_fraction, pattern, coupon)
}

// ── Rung-selection policy (task ι / 3791) ────────────────────────────────────

/// Fidelity rung for the FDM as-printed material model.
///
/// Ordered by fidelity: `RFast < R0`. Used by [`select_rungs`] to determine
/// the ordered sequence of compute targets for a given target fidelity, slicer
/// availability, and determinism flag (PRD task ι, #3791).
///
/// - `RFast`: transverse-isotropic model derived from the `FDMProcess`
///   parameters alone (Gibson-Ashby knockdown + fixed 0.67 build-Z ratio).
///   Available without a real sliced toolpath.
/// - `R0`: orthotropic model derived from a real sliced toolpath (PrusaSlicer
///   G-code), using closed-form Rodríguez + Halpin-Tsai physics. Requires a
///   slicer output to be available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rung {
    /// R-fast: transverse-isotropic zone model; available without a slicer.
    RFast = 0,
    /// R0: orthotropic zone model from parsed toolpath; requires a slicer.
    R0 = 1,
}

/// Select the ordered sequence of rungs to execute for the given `target_fidelity`.
///
/// # Parameters
///
/// - `target_fidelity`: the highest fidelity rung the consumer requested.
/// - `deterministic`: if `true`, pin to exactly one rung (the achievable cap)
///   — the `#deterministic` mode that guarantees bit-stable repeated runs.
/// - `slicer_available`: if `false`, R0 cannot be produced (a real toolpath is
///   required), so the cap is `RFast` regardless of `target_fidelity`.
///
/// # Semantics
///
/// The achievable cap = `min(target_fidelity, highest_achievable_rung)` where
/// `highest_achievable_rung` is `R0` when a slicer is available, `RFast` otherwise.
///
/// - `deterministic = true` → return exactly `[cap]` (one rung, bit-stable).
/// - `deterministic = false` → return the inclusive progressive ladder
///   `[RFast, …, cap]`.
pub fn select_rungs(
    target_fidelity: Rung,
    deterministic: bool,
    slicer_available: bool,
) -> Vec<Rung> {
    // R0 requires a slicer; without one the highest achievable rung is RFast.
    let highest_achievable = if slicer_available { Rung::R0 } else { Rung::RFast };
    let cap = target_fidelity.min(highest_achievable);

    if deterministic {
        // #deterministic: pin exactly one rung = the achievable cap.
        vec![cap]
    } else {
        // Progressive ladder: all rungs from RFast up to and including cap.
        // The enum variants are exhaustively listed in fidelity order.
        [Rung::RFast, Rung::R0]
            .iter()
            .copied()
            .filter(|&r| r <= cap)
            .collect()
    }
}

/// Orthotropic effective constants at a single `point` (opt-in path for
/// known-unidirectional raster; the transverse-isotropic model is the
/// default).
///
/// Same classify → solid-fraction → correlate composition as
/// [`material_constants_at`], dispatching to [`effective_orthotropic`]
/// instead. Used by the δ ComputeNode when `AsPrintedOptions.orthotropic` is
/// set.
#[allow(clippy::too_many_arguments)]
pub fn orthotropic_constants_at(
    aabb: &AxisAlignedBox,
    params: &ZoneProcessParams,
    cos_threshold: f64,
    base: BaseElastic,
    pattern: InfillPattern,
    infill_density: f64,
    coupon: &CouponOverride,
    point: [f64; 3],
) -> OrthotropicConstants {
    let zone = classify_point(aabb, params, cos_threshold, point);
    let solid_fraction = zone_solid_fraction(zone, infill_density);
    effective_orthotropic(base, solid_fraction, pattern, coupon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD;

    fn fdm_params() -> ZoneProcessParams {
        ZoneProcessParams {
            walls: 3,
            top_bottom_layers: 4,
            layer_height: 0.0002,
            line_width: 0.0004,
            build_direction: [0.0, 0.0, 1.0],
        }
    }

    #[test]
    fn zone_solid_fraction_maps_zones() {
        assert_eq!(zone_solid_fraction(Zone::Wall, 0.3), 1.0);
        assert_eq!(zone_solid_fraction(Zone::Skin, 0.3), 1.0);
        assert_eq!(zone_solid_fraction(Zone::Infill, 0.3), 0.3);
    }

    #[test]
    fn classify_point_box_wall_and_interior() {
        let bx = AxisAlignedBox {
            min: [0.0, 0.0, 0.0],
            max: [0.040, 0.040, 0.010],
        };
        let params = fdm_params();
        let t = DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD;
        // 0.3 mm from -X side ≤ 1.2 mm wall band → Wall.
        assert_eq!(
            classify_point(&bx, &params, t, [0.0003, 0.020, 0.005]),
            Zone::Wall
        );
        // Box centre → Infill.
        assert_eq!(
            classify_point(&bx, &params, t, [0.020, 0.020, 0.005]),
            Zone::Infill
        );
    }
}
