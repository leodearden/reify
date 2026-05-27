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

use reify_solver_elastic::{
    AnisotropicMaterial, ConstantField, DiscreteCellField, ElementStiffness, IsotropicElastic,
    OrthotropicMaterial, element_stiffness_hex_p1, element_stiffness_wedge_p1,
};
// Legacy and new field-aware entry points accessed via their owning module
// until step-16 lifts them into the crate-root `pub use` block.
use reify_solver_elastic::assembly::hex::element_stiffness_hex_p1_with_field;
use reify_solver_elastic::assembly::tet::{
    element_stiffness_p1, element_stiffness_p1_with_field, element_stiffness_p2,
    element_stiffness_p2_with_field,
};
use reify_solver_elastic::assembly::wedge::element_stiffness_wedge_p1_with_field;

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

/// Build the canonical 10-node P2 phys-node layout for a uniformly scaled
/// reference tet (mirror of `assembly::test_support::scaled_p2_phys_nodes`,
/// inlined here because that helper is `pub(crate)`).
///
/// Vertices in indices 0..4: `(0,0,0), (s,0,0), (0,s,0), (0,0,s)`.
/// Edge midpoints in indices 4..10 in canonical Hughes/Gmsh edge order
/// `(0,1), (1,2), (2,0), (0,3), (1,3), (2,3)`.
fn scaled_p2_phys_nodes(s: f64) -> [[f64; 3]; 10] {
    let v: [[f64; 3]; 4] = [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]];
    let mid = |a: usize, b: usize| {
        [
            0.5 * (v[a][0] + v[b][0]),
            0.5 * (v[a][1] + v[b][1]),
            0.5 * (v[a][2] + v[b][2]),
        ]
    };
    let edges: [(usize, usize); 6] =
        [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)];
    let mut nodes = [[0.0_f64; 3]; 10];
    nodes[..4].copy_from_slice(&v);
    for (i, &(a, b)) in edges.iter().enumerate() {
        nodes[4 + i] = mid(a, b);
    }
    nodes
}

/// Build the 8 physical nodes of a scaled unit hex `[−s, s]³` in canonical
/// Hughes/Gmsh hex8 order (mirror of
/// `assembly::test_support::scaled_unit_hex_phys_nodes`).
fn scaled_unit_hex_phys_nodes(s: f64) -> [[f64; 3]; 8] {
    [
        [-s, -s, -s],
        [s, -s, -s],
        [s, s, -s],
        [-s, s, -s],
        [-s, -s, s],
        [s, -s, s],
        [s, s, s],
        [-s, s, s],
    ]
}

/// Build the 6 physical nodes of a scaled unit wedge (unit triangle ×
/// `[−s, s]`) in canonical Gmsh PRI6 order (mirror of
/// `assembly::test_support::scaled_unit_wedge_phys_nodes`).
fn scaled_unit_wedge_phys_nodes(s: f64) -> [[f64; 3]; 6] {
    [
        [0.0, 0.0, -s],
        [s, 0.0, -s],
        [0.0, s, -s],
        [0.0, 0.0, s],
        [s, 0.0, s],
        [0.0, s, s],
    ]
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

// ─────────────────────────────────────────────────────────────────────────────
// Step 11: tet P2 + hex P1 + wedge P1 bit-identity regression rows
// ─────────────────────────────────────────────────────────────────────────────

/// P2 tet — constant lift of an identity-frame isotropic material is
/// bitwise equal to the legacy `element_stiffness_p2` path.
#[test]
fn tet_p2_with_constant_isotropic_lift_identity_frame_is_bit_identical_to_legacy_p2() {
    let iso = dimensionless_steel_like();
    let field = ConstantField {
        material: AnisotropicMaterial::from_law(&iso, IDENTITY_3X3),
    };
    let phys = scaled_p2_phys_nodes(1.0);
    let via_field = element_stiffness_p2_with_field(&phys, &field);
    let legacy = element_stiffness_p2(&phys, &iso);
    assert_element_stiffness_bitwise_eq(
        &via_field,
        &legacy,
        "tet P2 constant-lift identity-frame bit-identity",
    );
}

/// P1 hex — constant lift of an identity-frame isotropic material is
/// bitwise equal to the legacy `element_stiffness_hex_p1` path.
#[test]
fn hex_p1_with_constant_isotropic_lift_identity_frame_is_bit_identical_to_legacy_hex() {
    let iso = dimensionless_steel_like();
    let field = ConstantField {
        material: AnisotropicMaterial::from_law(&iso, IDENTITY_3X3),
    };
    let phys = scaled_unit_hex_phys_nodes(1.0);
    let via_field = element_stiffness_hex_p1_with_field(&phys, &field);
    let legacy = element_stiffness_hex_p1(&phys, &iso);
    assert_element_stiffness_bitwise_eq(
        &via_field,
        &legacy,
        "hex P1 constant-lift identity-frame bit-identity",
    );
}

/// P1 wedge — constant lift of an identity-frame isotropic material is
/// bitwise equal to the legacy `element_stiffness_wedge_p1` path.
#[test]
fn wedge_p1_with_constant_isotropic_lift_identity_frame_is_bit_identical_to_legacy_wedge() {
    let iso = dimensionless_steel_like();
    let field = ConstantField {
        material: AnisotropicMaterial::from_law(&iso, IDENTITY_3X3),
    };
    let phys = scaled_unit_wedge_phys_nodes(1.0);
    let via_field = element_stiffness_wedge_p1_with_field(&phys, &field);
    let legacy = element_stiffness_wedge_p1(&phys, &iso);
    assert_element_stiffness_bitwise_eq(
        &via_field,
        &legacy,
        "wedge P1 constant-lift identity-frame bit-identity",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step 13: non-trivial rotation + discrete-cell composition
// ─────────────────────────────────────────────────────────────────────────────

/// CFRP-like orthotropic material (mirrors
/// `tests/constitutive_laws.rs::cfrp_orthotropic`; copied locally to keep
/// this integration test file self-contained).
fn cfrp_orthotropic() -> OrthotropicMaterial {
    OrthotropicMaterial {
        e1: 140e9,
        e2: 10e9,
        e3: 10e9,
        g12: 5e9,
        g13: 5e9,
        g23: 3.5e9,
        nu12: 0.3,
        nu13: 0.3,
        nu23: 0.5,
    }
}

/// 90°-about-z frame: x → y, y → −x (active rotation of the material
/// frame). Same convention as `tests/constitutive_laws.rs`.
const R_90_Z: [[f64; 3]; 3] = [
    [0.0, -1.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0],
];

/// Sanity row: the anisotropy direction must actually flow through the
/// assembly hook. For a CFRP-like orthotropic with a 90°-about-z frame,
/// at least one `K_e` entry must differ from the identity-frame CFRP `K_e`
/// by more than 1e-6 in relative magnitude — anything tighter and the
/// rotation may have been silently dropped (e.g. `d_matrix_global` returns
/// `d_local` because the frame was overwritten with identity, or
/// `rotate_voigt` was bypassed).
#[test]
fn non_trivial_frame_rotates_orthotropic_k_observably_for_tet_p1() {
    let law = cfrp_orthotropic();
    let field_identity = ConstantField {
        material: AnisotropicMaterial::from_law(&law, IDENTITY_3X3),
    };
    let field_rot_90_z = ConstantField {
        material: AnisotropicMaterial::from_law(&law, R_90_Z),
    };
    let k_identity = element_stiffness_p1_with_field(&UNIT_TET_P1, &field_identity);
    let k_rot_90_z = element_stiffness_p1_with_field(&UNIT_TET_P1, &field_rot_90_z);

    assert_eq!(
        k_identity.n_dofs, k_rot_90_z.n_dofs,
        "tet P1 with_field: n_dofs must match across frames",
    );
    assert_eq!(
        k_identity.data.len(),
        k_rot_90_z.data.len(),
        "tet P1 with_field: data.len() must match across frames",
    );

    let mut max_rel_diff = 0.0_f64;
    let mut max_idx = 0usize;
    for (i, (a, b)) in k_identity
        .data
        .iter()
        .zip(k_rot_90_z.data.iter())
        .enumerate()
    {
        let scale = a.abs().max(b.abs()).max(1.0);
        let rel = (a - b).abs() / scale;
        if rel > max_rel_diff {
            max_rel_diff = rel;
            max_idx = i;
        }
    }
    assert!(
        max_rel_diff > 1e-6,
        "tet P1 with_field: 90°-about-z rotation of CFRP-like orthotropic must \
         change at least one K_e entry observably (max relative diff was \
         {max_rel_diff:e} at index {max_idx}; expected > 1e-6 — anisotropy \
         direction is not flowing through the assembly hook)",
    );
}

/// Composition row: a `DiscreteCellField` with two distinct per-cell
/// materials and a locator that returns the right cell index for each
/// element's centroid must produce per-element K matching the per-cell
/// `ConstantField` K. Bit-equality is required (same centroid → same
/// material → same `d_global` → same inner loop).
#[test]
fn discrete_cell_field_per_element_sampling_picks_per_cell_material_for_two_tet_p1_fixtures() {
    // Element 0 — the canonical unit tet at the origin; its centroid is
    // 0.25 * (sum of 4 corners) = [0.25, 0.25, 0.25].
    let phys_e0: [[f64; 3]; 4] = UNIT_TET_P1;
    let centroid_e0 = [0.25_f64, 0.25, 0.25];

    // Element 1 — a translated copy of the unit tet shifted by +2 along
    // x; its centroid is [2.25, 0.25, 0.25].
    let phys_e1: [[f64; 3]; 4] = [
        [2.0, 0.0, 0.0],
        [3.0, 0.0, 0.0],
        [2.0, 1.0, 0.0],
        [2.0, 0.0, 1.0],
    ];
    let centroid_e1 = [2.25_f64, 0.25, 0.25];

    // Two distinct materials — `mat_a` is the dimensionless steel-like
    // isotropic, `mat_b` is a softer isotropic (factor-2 lower E).
    let iso_a = dimensionless_steel_like();
    let iso_b = IsotropicElastic {
        youngs_modulus: 0.5,
        poisson_ratio: 0.3,
    };
    let mat_a = AnisotropicMaterial::from_law(&iso_a, IDENTITY_3X3);
    let mat_b = AnisotropicMaterial::from_law(&iso_b, IDENTITY_3X3);

    // Locator: x < 1.0 → cell 0; else cell 1. Both centroids land cleanly
    // on the right side of the threshold.
    let discrete_field = DiscreteCellField {
        cells: vec![mat_a, mat_b],
        locator: Box::new(|p: [f64; 3]| if p[0] < 1.0 { Some(0) } else { Some(1) }),
    };

    // Per-element `ConstantField` references for comparison — these are
    // the bit-target each element must produce.
    let field_a = ConstantField { material: mat_a };
    let field_b = ConstantField { material: mat_b };

    // Sanity-check the locator picks the right cell for each centroid.
    // (Catches off-by-one in the centroid math before the K compare runs.)
    assert!(
        centroid_e0[0] < 1.0,
        "centroid_e0.x = {} must be < 1.0 (cell 0 boundary)",
        centroid_e0[0],
    );
    assert!(
        centroid_e1[0] >= 1.0,
        "centroid_e1.x = {} must be >= 1.0 (cell 1 boundary)",
        centroid_e1[0],
    );

    let k_e0_discrete = element_stiffness_p1_with_field(&phys_e0, &discrete_field);
    let k_e0_constant_a = element_stiffness_p1_with_field(&phys_e0, &field_a);
    assert_element_stiffness_bitwise_eq(
        &k_e0_discrete,
        &k_e0_constant_a,
        "tet P1 e0 (centroid in cell 0) — discrete vs constant(mat_a)",
    );

    let k_e1_discrete = element_stiffness_p1_with_field(&phys_e1, &discrete_field);
    let k_e1_constant_b = element_stiffness_p1_with_field(&phys_e1, &field_b);
    assert_element_stiffness_bitwise_eq(
        &k_e1_discrete,
        &k_e1_constant_b,
        "tet P1 e1 (centroid in cell 1) — discrete vs constant(mat_b)",
    );
}
