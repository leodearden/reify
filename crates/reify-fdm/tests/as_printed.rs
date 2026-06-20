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

use reify_fdm::{
    AxisAlignedBox, BaseElastic, CouponOverride, DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD, InfillPattern,
    Zone, ZoneProcessParams, material_constants_at, zone_solid_fraction,
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
