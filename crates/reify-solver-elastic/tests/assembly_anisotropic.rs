// Inner attribute must precede any outer doc comment at crate level.
// Applies to every nested loop in this file (tests iterate over the K_e
// entries with index arithmetic that clippy flags as `needless_range_loop`).
#![allow(clippy::needless_range_loop)]

//! Integration tests for the foundation β assembly hook
//! (`element_stiffness_*_with_field`).
//!
//! PRD `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` §C4 / C3.
//!
//! Each test row exercises one of the C4 contract clauses:
//!
//! 1. **Bit-identity regression** — for each shape, a constant lift of an
//!    identity-frame isotropic material assembled via
//!    `element_stiffness_*_with_field` is entry-by-entry `to_bits()`-equal
//!    to the legacy `element_stiffness_*(phys, &iso)` path.
//! 2. **Non-trivial rotation** — orthotropic with a 90°-about-z frame
//!    differs observably from identity-frame orthotropic (step 13).
//! 3. **Discrete-cell composition** — per-element sampling picks the
//!    cell-indexed material (step 13).

use reify_solver_elastic::{AnisotropicMaterial, ConstantField, ElementStiffness, IsotropicElastic};
// Legacy and new field-aware entry points accessed via their owning module
// until step-16 lifts them into the crate-root `pub use` block.
use reify_solver_elastic::assembly::tet::{element_stiffness_p1, element_stiffness_p1_with_field};

/// Identity 3×3 frame — local axes align with global.
const IDENTITY_3X3: [[f64; 3]; 3] =
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Canonical 4-node P1 phys layout (unit reference tet, volume 1/6).
const UNIT_TET_P1: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

/// Steel-like dimensionless material (mirror of
/// `assembly::test_support::dimensionless_steel_like`).
fn dimensionless_steel_like() -> IsotropicElastic {
    IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    }
}

/// Assert two `ElementStiffness` matrices are bit-equal entry-by-entry.
fn assert_element_stiffness_bitwise_eq(
    got: &ElementStiffness,
    expected: &ElementStiffness,
    ctx: &str,
) {
    assert_eq!(
        got.n_dofs, expected.n_dofs,
        "{ctx}: n_dofs mismatch (got {}, expected {})",
        got.n_dofs, expected.n_dofs,
    );
    assert_eq!(
        got.data.len(),
        expected.data.len(),
        "{ctx}: data.len() mismatch (got {}, expected {})",
        got.data.len(),
        expected.data.len(),
    );
    for (i, (g, e)) in got.data.iter().zip(expected.data.iter()).enumerate() {
        assert_eq!(
            g.to_bits(),
            e.to_bits(),
            "{ctx}: K[{i}] = {g} must equal {e} bitwise",
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step 9: tet P1 bit-identity regression
// ─────────────────────────────────────────────────────────────────────────────

/// Pin the function-item signature of `element_stiffness_p1_with_field`.
/// Renaming or changing the surface trips this at compile time.
#[allow(dead_code)]
fn _signature_pin_p1_with_field() {
    let _: fn(&[[f64; 3]; 4], &ConstantField) -> ElementStiffness =
        element_stiffness_p1_with_field;
}

/// Constant lift of an identity-frame isotropic material must produce a
/// `K_e` bitwise equal to the legacy `element_stiffness_p1` path.
#[test]
fn tet_p1_with_constant_isotropic_lift_identity_frame_is_bit_identical_to_legacy_p1() {
    let iso = dimensionless_steel_like();
    let field = ConstantField {
        material: AnisotropicMaterial::from_law(&iso, IDENTITY_3X3),
    };
    let via_field = element_stiffness_p1_with_field(&UNIT_TET_P1, &field);
    let legacy = element_stiffness_p1(&UNIT_TET_P1, &iso);
    assert_element_stiffness_bitwise_eq(
        &via_field,
        &legacy,
        "tet P1 constant-lift identity-frame bit-identity",
    );
}
