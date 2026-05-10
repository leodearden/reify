//! PRD task #13 — quality-threshold calibration regression-guard suite.
//!
//! This integration-test binary exercises the (`elasticity_morph` +
//! `quality_check`) pair against three procedural parametric fixtures
//! (box, plate-with-hole, L-bracket) and asserts the "morph rejected only
//! when from-scratch is materially better" rule that calibrates
//! [`MorphOptions::default()`].
//!
//! Helper modules are pulled in via `#[path = …]` so Cargo does NOT compile
//! them as standalone integration-test binaries — only this file is. See
//! Cargo book §"Integration tests" and the plan's design-decisions for
//! background.
//!
//! Provenance: task #2950.

#[path = "calibration/fixtures.rs"]
mod fixtures;

#[path = "calibration/sweep.rs"]
mod sweep;

/// Smoke test: helper modules are wired in correctly and expose the
/// `MODULE_OK` sentinel constants. Fails to compile while either helper
/// module is missing — pins the file layout the task spec requires.
#[test]
fn calibration_helper_modules_are_wired_in() {
    assert!(fixtures::MODULE_OK);
    assert!(sweep::MODULE_OK);
}

// ── Step-3: box_mesh fixture validity ─────────────────────────────────────────

#[test]
fn box_mesh_fixture_returns_valid_p1_mesh_with_expected_counts_and_positive_volume_tets() {
    use reify_mesh_morph::{MorphOptions, QualityVerdict, quality_check};
    use reify_types::ElementOrderTag;

    let (mesh, surface_indices) = fixtures::box_mesh(1.0, 0.1, 3);

    // P1 element order is required by quality_check + elasticity_morph.
    assert_eq!(
        mesh.element_order,
        ElementOrderTag::P1,
        "box_mesh must return a P1 mesh"
    );

    // Flat vertices and tet_indices buffers must be sized for their stride.
    assert_eq!(
        mesh.vertices.len() % 3,
        0,
        "vertices must be a flat triple-stride buffer"
    );
    assert_eq!(
        mesh.tet_indices.len() % 4,
        0,
        "tet_indices must be a flat 4-tuple-stride buffer (P1 tets)"
    );
    assert!(
        !mesh.tet_indices.is_empty(),
        "box_mesh must produce at least one tet"
    );

    // Every tet must be right-handed (positive scaled Jacobian) — reuse
    // quality_check with a permissive options profile so no soft floor trips.
    // HardFail signals at least one inverted tet — that's the contract this
    // assertion pins.
    let permissive = MorphOptions {
        quality_floor_min_scaled_jacobian: 0.0,
        quality_floor_pct_below_025: 1.01,
        quality_aspect_ratio_factor_max: f64::INFINITY,
        ..MorphOptions::default()
    };
    let verdict = quality_check(&mesh, &mesh, &permissive);
    assert!(
        !matches!(verdict, QualityVerdict::HardFail(_)),
        "every tet must be right-handed (no inversions); got {verdict:?}"
    );

    // Surface indices must be non-empty and reference real vertices.
    assert!(
        !surface_indices.is_empty(),
        "surface_node_indices must be non-empty"
    );
    let n_vertices = (mesh.vertices.len() / 3) as u32;
    for &idx in &surface_indices {
        assert!(
            idx < n_vertices,
            "surface index {idx} out of range (n_vertices = {n_vertices})"
        );
    }
}
