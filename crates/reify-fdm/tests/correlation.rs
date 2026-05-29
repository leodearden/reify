// SPDX-License-Identifier: AGPL-3.0-or-later

//! User-observable signal for FDM β — the public correlation API turns a base
//! filament + infill into transverse-isotropic constants with the build (Z)
//! axis weakest, applies the Gibson-Ashby infill knockdown, and lets a coupon
//! override beat the computed default. See
//! `docs/prds/v0_5/fdm-as-printed-fea.md` §"Built-in property correlations".

use reify_fdm::{
    BUILD_Z_MODULUS_RATIO, BaseElastic, CouponOverride, InfillPattern,
    effective_transverse_isotropic,
};

/// ABS-like base filament: E ≈ 2.3 GPa, ν = 0.35, ρ ≈ 1.04 g/cc.
fn abs_base() -> BaseElastic {
    BaseElastic {
        youngs_modulus: 2.3e9,
        poisson_ratio: 0.35,
        density: 1040.0,
    }
}

#[test]
fn default_transverse_iso_is_build_z_weakest_with_infill_knockdown() {
    let base = abs_base();
    // Sparse 20% gyroid infill, no coupon data → pure correlation path.
    let c = effective_transverse_isotropic(
        base,
        0.2,
        InfillPattern::Gyroid,
        &CouponOverride::default(),
    );

    // Build-Z (axial) is the weakest axis (PRD invariant).
    assert!(
        c.e_axial < c.e_in_plane,
        "build-Z axial modulus {} must be weaker than in-plane {}",
        c.e_axial,
        c.e_in_plane
    );
    // The axial modulus is exactly the in-plane modulus × the knockdown ratio.
    assert!(
        (c.e_axial - c.e_in_plane * BUILD_Z_MODULUS_RATIO).abs() <= 1e-9 * c.e_in_plane,
        "e_axial should equal e_in_plane · BUILD_Z_MODULUS_RATIO"
    );
    // Gibson-Ashby knockdown at ρ=0.2 ⇒ in-plane modulus ≈ base E · 0.04.
    let expected_in_plane = base.youngs_modulus * 0.04;
    assert!(
        (c.e_in_plane - expected_in_plane).abs() <= 1e-3 * expected_in_plane,
        "20% infill should knock the in-plane modulus to ≈ base·0.04 = {}, got {}",
        expected_in_plane,
        c.e_in_plane
    );
}

#[test]
fn coupon_override_changes_the_public_result() {
    let base = abs_base();
    let solid_fraction = 0.2;
    let default = effective_transverse_isotropic(
        base,
        solid_fraction,
        InfillPattern::Gyroid,
        &CouponOverride::default(),
    );

    // A coupon-measured in-plane modulus beats the computed default.
    let coupon = CouponOverride {
        ex: Some(1.9e9),
        ..Default::default()
    };
    let measured =
        effective_transverse_isotropic(base, solid_fraction, InfillPattern::Gyroid, &coupon);

    assert_eq!(
        measured.e_in_plane, 1.9e9,
        "coupon ex overrides the computed in-plane modulus"
    );
    assert!(
        measured.e_in_plane != default.e_in_plane,
        "the override must change the result relative to the default path"
    );
    // The axial knockdown still tracks the (now overridden) in-plane modulus.
    assert!(
        (measured.e_axial - 1.9e9 * BUILD_Z_MODULUS_RATIO).abs() <= 1e-9 * 1.9e9,
        "axial modulus should track the overridden in-plane modulus"
    );
}
