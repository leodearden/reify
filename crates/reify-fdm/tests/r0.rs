// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration tests for the θ / 3790 R0 constitutive mapping
//! (`reify_fdm::r0`): the closed-form Rodríguez 2003 orthotropic law,
//! Halpin-Tsai fibre reinforcement, the lumped-cooling build-Z knockdown, and
//! the toolpath → per-zone material mapping.
//!
//! Assertions are **ordering / interval / structural** — the R0 physics is
//! construction-guaranteed (no magnitude calibration is pinned here), matching
//! the plan's RED-premise checks.

use reify_fdm::BaseElastic;
use reify_fdm::r0::{
    RasterMesostructure, halpin_tsai_modulus, lumped_cooling_z_ratio, rodriguez_orthotropic,
};

/// A PLA-like base filament: E ≈ 2.3 GPa, ν = 0.35, ρ ≈ 1.24 g/cc.
fn pla_base() -> BaseElastic {
    BaseElastic {
        youngs_modulus: 2.3e9,
        poisson_ratio: 0.35,
        density: 1240.0,
    }
}

/// Rodríguez 2003 closed-form orthotropy: a unidirectional raster is stiffest
/// along the bead (E1), knocked down transverse in-plane (E2), and weakest in
/// build-Z (E3) — the `E1 > E2 > E3` ordering. Shear/Poisson are positive and
/// finite; a dense (ρ=1) along-raster modulus stays within a small fraction of
/// the base modulus.
#[test]
fn rodriguez_orthotropic_orders_e1_gt_e2_gt_e3() {
    let base = pla_base();
    // Unidirectional raster: transverse neck knockdown < 1 and inter-layer bond
    // knockdown < 1 (the two structural reductions of the Rodríguez model).
    let meso = RasterMesostructure {
        transverse_ratio: 0.7,
        z_ratio: 0.6,
    };
    let c = rodriguez_orthotropic(base, 1.0, meso);

    // Build-Z weakest, along-raster stiffest: E1 > E2 > E3.
    assert!(c.e1 > c.e2, "along-raster E1 ({}) > transverse E2 ({})", c.e1, c.e2);
    assert!(c.e2 > c.e3, "transverse E2 ({}) > build-Z E3 ({})", c.e2, c.e3);
    assert!(c.e1 > c.e3, "E1 ({}) > build-Z E3 ({}) — build-Z weakest", c.e1, c.e3);

    // Shear moduli and Poisson ratios are positive and finite.
    for (name, v) in [("g12", c.g12), ("g13", c.g13), ("g23", c.g23)] {
        assert!(v.is_finite() && v > 0.0, "{name} must be positive finite, got {v}");
    }
    for (name, v) in [("nu12", c.nu12), ("nu13", c.nu13), ("nu23", c.nu23)] {
        assert!(v.is_finite() && v > 0.0, "{name} must be positive finite, got {v}");
    }
    assert!(c.density.is_finite() && c.density > 0.0, "density positive finite");

    // Dense along-raster modulus is within a small fraction of the base modulus
    // (continuous material along the bead — no infill knockdown at ρ=1).
    assert!(
        (c.e1 - base.youngs_modulus).abs() <= 0.1 * base.youngs_modulus,
        "dense along-raster E1 ({}) should be within 10% of base E ({})",
        c.e1,
        base.youngs_modulus
    );
}

/// Halpin-Tsai short-fibre stiffening: identity at `Vf=0`, bounded strictly
/// between the matrix modulus and the Voigt rule-of-mixtures upper bound,
/// monotone increasing in fibre volume fraction, and increasing in aspect ratio.
#[test]
fn halpin_tsai_modulus_is_bounded_and_monotone() {
    let em = 2.3e9; // matrix (base filament) modulus
    let ef = 70.0e9; // glass-fibre-like reinforcement modulus (> matrix)
    let ar = 20.0; // fibre aspect ratio l/d

    // (a) Vf = 0 returns the matrix modulus EXACTLY (η·Vf = 0 ⇒ factor 1.0).
    assert_eq!(
        halpin_tsai_modulus(em, ef, 0.0, ar),
        em,
        "Vf=0 must recover the matrix modulus exactly"
    );

    // (b) For Vf > 0 the result lies strictly between Em and the Voigt bound
    //     Em(1−Vf)+Ef·Vf, and increases monotonically with Vf.
    let mut prev = em;
    for &vf in &[0.05, 0.10, 0.20, 0.30] {
        let e = halpin_tsai_modulus(em, ef, vf, ar);
        let voigt = em * (1.0 - vf) + ef * vf;
        assert!(e > em, "Vf={vf}: E ({e}) must exceed the matrix modulus ({em})");
        assert!(e < voigt, "Vf={vf}: E ({e}) must stay below the Voigt bound ({voigt})");
        assert!(e > prev, "Vf={vf}: E ({e}) must increase with Vf (prev {prev})");
        prev = e;
    }

    // (c) A larger aspect ratio raises the modulus (ξ grows ⇒ closer to Voigt).
    let vf = 0.2;
    assert!(
        halpin_tsai_modulus(em, ef, vf, 50.0) > halpin_tsai_modulus(em, ef, vf, 5.0),
        "higher aspect ratio must raise the Halpin-Tsai modulus"
    );
}

/// Lumped-cooling build-Z knockdown: the inter-layer bond fraction that replaces
/// R-fast's fixed 0.67. The ratio is in the open interval `(0, 1)`; a hotter
/// interface or a shorter inter-layer time (more re-melt / better fusion) gives
/// a ratio closer to 1, and the function is monotone in each input.
#[test]
fn lumped_cooling_z_ratio_in_unit_interval_and_monotone() {
    let ambient = 25.0; // °C build-chamber reference
    let tau = 8.0; // s lumped-capacitance time constant
    let scale = 60.0; // °C bond temperature scale

    // (a) Every ratio over a realistic input grid stays in the OPEN (0, 1).
    for &nominal in &[180.0, 210.0, 240.0] {
        for &layer_time in &[2.0, 10.0, 30.0] {
            let r = lumped_cooling_z_ratio(nominal, ambient, layer_time, tau, scale);
            assert!(
                r > 0.0 && r < 1.0,
                "ratio must be in (0,1); got {r} (nominal={nominal}, layer_time={layer_time})"
            );
        }
    }

    // (b) Monotone in interface temperature: hotter ⇒ closer to 1.
    let cold = lumped_cooling_z_ratio(180.0, ambient, 10.0, tau, scale);
    let hot = lumped_cooling_z_ratio(240.0, ambient, 10.0, tau, scale);
    assert!(hot > cold, "hotter interface ({hot}) must beat colder ({cold})");

    // (c) Monotone (inverse) in inter-layer time: shorter ⇒ closer to 1.
    let quick = lumped_cooling_z_ratio(210.0, ambient, 2.0, tau, scale);
    let slow = lumped_cooling_z_ratio(210.0, ambient, 30.0, tau, scale);
    assert!(quick > slow, "shorter inter-layer time ({quick}) must beat longer ({slow})");
}
