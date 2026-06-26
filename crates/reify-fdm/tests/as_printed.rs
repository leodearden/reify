// SPDX-License-Identifier: AGPL-3.0-or-later

//! R-fast as-printed wiring helpers (task δ) — γ (zone classifier) + β
//! (effective-property correlations) combined into the per-point material
//! constants the δ ComputeNode packs into a
//! `Field<Point3<Length>, AnisotropicMaterial>`.
//!
//! User-observable signal: on a walled+infilled box, a wall point yields a
//! dense in-plane modulus while a deep-interior (infill) point is knocked
//! down by the Gibson-Ashby law — and in BOTH the build (Z) axis is the
//! weakest direction (`e_axial < e_in_plane`). See
//! `docs/prds/v0_5/fdm-as-printed-fea.md` §C4.
//!
//! Also tests the `Rung` + `select_rungs` rung-selection policy (task ι / 3791):
//! the pure function that maps `(target_fidelity, deterministic, slicer_available)`
//! to the ordered sequence of rungs to execute.

use reify_fdm::{
    AxisAlignedBox, BaseElastic, CouponOverride, DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD, InfillPattern,
    Rung, Zone, ZoneProcessParams, material_constants_at, select_rungs, zone_solid_fraction,
};

/// ABS-like base filament fixture (≈2 GPa, ν=0.35, 1040 kg/m³).
fn abs_base() -> BaseElastic {
    BaseElastic {
        youngs_modulus: 2.0e9,
        poisson_ratio: 0.35,
        density: 1040.0,
    }
}

/// Stdlib FDMProcess defaults + the consumer-derived line_width default.
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
fn zone_solid_fraction_dense_walls_skins_and_sparse_infill() {
    let rho = 0.2;
    // Walls and skins are solid perimeters / solid layers — fully dense.
    assert_eq!(zone_solid_fraction(Zone::Wall, rho), 1.0);
    assert_eq!(zone_solid_fraction(Zone::Skin, rho), 1.0);
    // Only the sparse infill carries the process infill density.
    assert_eq!(zone_solid_fraction(Zone::Infill, rho), rho);
}

#[test]
fn material_constants_at_wall_vs_infill_distinct_and_build_z_weakest() {
    // 40×40×10 mm tall-cap cube, Z build axis. Sides 20 mm from centre,
    // top/bottom 5 mm — so the centre is deep interior (Infill) and a
    // near-side point is a Wall (per the γ classifier test fixture).
    let bx = AxisAlignedBox {
        min: [0.0, 0.0, 0.0],
        max: [0.040, 0.040, 0.010],
    };
    let params = fdm_params();
    let t = DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD;
    let base = abs_base();
    let infill_density = 0.2;
    let pattern = InfillPattern::Gyroid; // near-isotropic
    let coupon = CouponOverride::default();

    // Wall point: 0.3 mm from the -X side face (≤ 1.2 mm wall band) → Wall.
    let wall = material_constants_at(
        &bx,
        &params,
        t,
        base,
        pattern,
        infill_density,
        &coupon,
        [0.0003, 0.020, 0.005],
    );
    // Deep-interior point: box centre → Infill.
    let infill = material_constants_at(
        &bx,
        &params,
        t,
        base,
        pattern,
        infill_density,
        &coupon,
        [0.020, 0.020, 0.005],
    );

    // (a) Non-constant field: wall is dense, infill is knocked down. The
    //     in-plane moduli must differ by more than a rounding wobble — the
    //     Gibson-Ashby factor at ρ=0.2 is 0.04, so wall ≈ 25× infill.
    assert!(
        wall.e_in_plane > infill.e_in_plane * 5.0,
        "wall in-plane modulus {} should dominate infill {} (dense wall vs \
         Gibson-Ashby-knocked-down infill)",
        wall.e_in_plane,
        infill.e_in_plane
    );

    // (b) Build-Z is the weakest axis in BOTH zones (PRD C4 invariant).
    assert!(
        wall.e_axial < wall.e_in_plane,
        "wall build-Z modulus {} must be weaker than in-plane {}",
        wall.e_axial,
        wall.e_in_plane
    );
    assert!(
        infill.e_axial < infill.e_in_plane,
        "infill build-Z modulus {} must be weaker than in-plane {}",
        infill.e_axial,
        infill.e_in_plane
    );
}

// ── Rung + select_rungs policy tests (task ι / 3791, step-1 RED) ────────────
//
// These tests exercise the pure rung-selection function:
//   `select_rungs(target_fidelity, deterministic, slicer_available) -> Vec<Rung>`.
//
// They fail to compile until step-2 adds `Rung` and `select_rungs` to
// `reify-fdm/src/as_printed.rs` and re-exports them from `lib.rs`.

/// (a) Progressive ladder with slicer available: target=R0, non-deterministic,
///     slicer present → full ladder [RFast, R0].
#[test]
fn select_rungs_progressive_ladder_r0_slicer_available() {
    assert_eq!(
        select_rungs(Rung::R0, false, true),
        vec![Rung::RFast, Rung::R0],
        "target=R0, non-deterministic, slicer available → progressive ladder [RFast, R0]"
    );
}

/// (b) No slicer: target=R0, non-deterministic, slicer absent → caps at RFast.
#[test]
fn select_rungs_tops_at_r_fast_when_no_slicer() {
    assert_eq!(
        select_rungs(Rung::R0, false, false),
        vec![Rung::RFast],
        "target=R0, no slicer → cap at RFast (R0 requires a slicer)"
    );
}

/// (c) #deterministic pins exactly one rung = cap (R0 with slicer present).
#[test]
fn select_rungs_deterministic_pins_r0_when_slicer_available() {
    assert_eq!(
        select_rungs(Rung::R0, true, true),
        vec![Rung::R0],
        "target=R0, deterministic, slicer available → exactly one rung [R0]"
    );
}

/// (d) #deterministic + target=RFast → single rung [RFast] regardless of slicer.
#[test]
fn select_rungs_deterministic_r_fast_target_pins_r_fast() {
    assert_eq!(
        select_rungs(Rung::RFast, true, true),
        vec![Rung::RFast],
        "target=RFast, deterministic → exactly one rung [RFast]"
    );
}

/// Rungs are ordered by fidelity: RFast < R0 (required for the inclusive-ladder
/// construction and for `target.min(highest_achievable)` to work correctly).
#[test]
fn rung_ordering_r_fast_lt_r0() {
    assert!(
        Rung::RFast < Rung::R0,
        "RFast must be less than R0 (lower fidelity ⇒ lower ordinal)"
    );
}
