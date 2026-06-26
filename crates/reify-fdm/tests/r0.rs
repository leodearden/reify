// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration tests for the θ / 3790 R0 constitutive mapping
//! (`reify_fdm::r0`): the closed-form Rodríguez 2003 orthotropic law,
//! Halpin-Tsai fibre reinforcement, the lumped-cooling build-Z knockdown, and
//! the toolpath → per-zone material mapping.
//!
//! Assertions are **ordering / interval / structural** — the R0 physics is
//! construction-guaranteed (no magnitude calibration is pinned here), matching
//! the plan's RED-premise checks.

use reify_fdm::{
    BUILD_Z_MODULUS_RATIO, BaseElastic, BeadRole, CouponOverride, InfillPattern, Toolpath,
    effective_transverse_isotropic, parse_prusaslicer_gcode,
};
use reify_fdm::r0::{
    R0Options, RasterMesostructure, halpin_tsai_modulus, lumped_cooling_z_ratio,
    r0_region_materials, rodriguez_orthotropic,
};

// ── step-7 helpers ───────────────────────────────────────────────────────────

/// All unit segment directions of the beads with a given role.
fn role_segment_dirs(tp: &Toolpath, role: BeadRole) -> Vec<[f64; 3]> {
    let mut dirs = Vec::new();
    for b in tp.beads.iter().filter(|b| b.role == role) {
        for w in b.centerline.windows(2) {
            let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if n > 1e-9 {
                dirs.push([d[0] / n, d[1] / n, d[2] / n]);
            }
        }
    }
    dirs
}

/// True when `a` and `b` are parallel (or anti-parallel) — zero cross product.
fn is_parallel(a: [f64; 3], b: [f64; 3]) -> bool {
    let c = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() < 1e-9
}

fn norm3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

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

/// The committed PrusaSlicer bracket fixture (real sliced toolpath). Roles:
/// External perimeter + Perimeter → wall; Top solid infill → skin; Internal
/// infill → infill; Custom / Skirt-Brim are skipped.
const BRACKET_FIXTURE: &str = include_str!("fixtures/prusaslicer_bracket.gcode");

/// A PLA-like base filament shared by the toolpath-mapping tests.
fn pla() -> BaseElastic {
    pla_base()
}

/// step-7(a): every populated region of the real bracket toolpath is build-Z
/// weakest + orthotropic, its frame x-axis is a unit vector aligned to an actual
/// bead direction in that role, and widths/heights are mm→SI converted.
#[test]
fn r0_region_materials_fixture_build_z_weakest_and_bead_aligned() {
    let tp = parse_prusaslicer_gcode(BRACKET_FIXTURE).expect("fixture must parse");
    let regions = r0_region_materials(&tp, pla(), &R0Options::default());

    for (name, region, role) in [
        ("wall", &regions.wall, BeadRole::Perimeter),
        ("skin", &regions.skin, BeadRole::SolidInfill),
        ("infill", &regions.infill, BeadRole::SparseInfill),
    ] {
        let c = &region.constants;
        // Build-Z weakest, orthotropic (E1 ≥ E2): E1 > E2 > E3.
        assert!(c.e1 >= c.e2, "{name}: orthotropic E1 ({}) ≥ E2 ({})", c.e1, c.e2);
        assert!(c.e3 < c.e2, "{name}: build-Z E3 ({}) < transverse E2 ({})", c.e3, c.e2);
        assert!(c.e3 < c.e1, "{name}: build-Z E3 ({}) < along-bead E1 ({})", c.e3, c.e1);

        // Frame x-axis: a unit vector aligned to an ACTUAL bead direction in
        // that role (every role above is populated in the fixture).
        let x = region.bead_direction;
        assert!((norm3(x) - 1.0).abs() < 1e-9, "{name}: x-axis must be unit, got {x:?}");
        let dirs = role_segment_dirs(&tp, role);
        assert!(!dirs.is_empty(), "{name}: role must be populated in the fixture");
        assert!(
            dirs.iter().any(|&d| is_parallel(d, x)),
            "{name}: x-axis {x:?} must be parallel to a real {role:?} bead direction"
        );

        // mm → SI: the fixture beads are 0.45 mm wide, 0.2 mm tall.
        assert!(
            (region.mean_width_m - 0.00045).abs() < 1e-9,
            "{name}: mean width should be 0.45 mm = 0.00045 m, got {}",
            region.mean_width_m
        );
        assert!(
            (region.mean_height_m - 0.0002).abs() < 1e-9,
            "{name}: mean height should be 0.2 mm = 0.0002 m, got {}",
            region.mean_height_m
        );
    }
}

/// step-7(b): a synthetic UNIFORM toolpath (one role / width / direction, solid
/// infill ρ=1) ⇒ wall == skin == infill (constant-input ⇒ constant field).
#[test]
fn r0_region_materials_uniform_toolpath_is_constant() {
    // Two parallel +X solid-infill beads, identical width / height / temp.
    let src = "\
M83
M104 S210
;LAYER_CHANGE
;Z:0.2
;HEIGHT:0.2
G1 Z0.2 F7200
;TYPE:Solid infill
;WIDTH:0.45
G1 X0 Y0 F9000
G1 X10 Y0 E1.0
G1 X0 Y1 F9000
G1 X10 Y1 E1.0
";
    let tp = parse_prusaslicer_gcode(src).expect("uniform snippet must parse");
    // Solid infill ρ=1 so the (empty, fallback) infill region is dense too.
    let opts = R0Options {
        infill_density: 1.0,
        ..R0Options::default()
    };
    let regions = r0_region_materials(&tp, pla(), &opts);

    assert_eq!(
        regions.wall.constants, regions.skin.constants,
        "uniform toolpath: wall and skin constants must match"
    );
    assert_eq!(
        regions.skin.constants, regions.infill.constants,
        "uniform toolpath: skin and infill constants must match"
    );
    assert_eq!(
        regions.wall.bead_direction, regions.infill.bead_direction,
        "uniform toolpath: all zone frame x-axes must match"
    );
}

/// step-7(c): on the same base, the R0 region constants differ STRUCTURALLY
/// from the R-fast transverse-isotropic baseline — R0 is orthotropic (E1 ≠ E2)
/// with a cooling-derived build-Z ratio ≠ R-fast's fixed 0.67.
#[test]
fn r0_region_materials_differs_from_r_fast_baseline() {
    let tp = parse_prusaslicer_gcode(BRACKET_FIXTURE).expect("fixture must parse");
    let base = pla();
    let regions = r0_region_materials(&tp, base, &R0Options::default());
    let wall = &regions.wall.constants;

    // R-fast baseline: in-plane isotropic + the fixed 0.67 build-Z ratio.
    let rfast =
        effective_transverse_isotropic(base, 1.0, InfillPattern::Gyroid, &CouponOverride::default());
    assert!(
        (rfast.e_axial / rfast.e_in_plane - BUILD_Z_MODULUS_RATIO).abs() < 1e-9,
        "R-fast baseline must carry the fixed 0.67 build-Z ratio"
    );

    // R0 is orthotropic (a genuine in-plane split the transverse-iso law cannot
    // represent) ...
    assert!(
        wall.e1 > wall.e2,
        "R0 wall is orthotropic (E1 {} > E2 {}); R-fast is in-plane isotropic",
        wall.e1,
        wall.e2
    );
    // ... and its build-Z ratio is cooling-derived, not the fixed 0.67.
    let r0_z_ratio = wall.e3 / wall.e1;
    assert!(
        (r0_z_ratio - BUILD_Z_MODULUS_RATIO).abs() > 1e-3,
        "R0 build-Z ratio {r0_z_ratio} must differ from R-fast's fixed {BUILD_Z_MODULUS_RATIO}"
    );
}
