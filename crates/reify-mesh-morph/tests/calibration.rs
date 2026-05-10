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

// ── Step-5: plate_with_hole fixture validity ──────────────────────────────────

#[test]
fn plate_with_hole_fixture_returns_valid_p1_mesh_with_hole_at_center_and_positive_volume_tets() {
    use reify_mesh_morph::{MorphOptions, QualityVerdict, quality_check};
    use reify_types::ElementOrderTag;

    let side = 1.0;
    let hole_diameter = 0.3;
    let thickness = 0.1;
    let (mesh, surface_indices) = fixtures::plate_with_hole(side, hole_diameter, thickness, 4, 2);

    assert_eq!(
        mesh.element_order,
        ElementOrderTag::P1,
        "plate_with_hole must return a P1 mesh"
    );
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
        "plate_with_hole must produce at least one tet"
    );

    // No tet may be inverted (right-handed connectivity contract).
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

    // No vertex may lie inside the hole's radial cylinder. The plate is
    // centered at (side/2, side/2) with the hole at the same xy center.
    let hole_radius = hole_diameter / 2.0;
    let cx = side / 2.0;
    let cy = side / 2.0;
    // Allow a small numerical slop because hole-boundary vertices sit at
    // exactly r = hole_radius (subject to f32 rounding).
    let tol = 1e-5_f32;
    let n_vertices = mesh.vertices.len() / 3;
    for v in 0..n_vertices {
        let x = mesh.vertices[v * 3] as f64;
        let y = mesh.vertices[v * 3 + 1] as f64;
        let r2 = (x - cx).powi(2) + (y - cy).powi(2);
        let r = r2.sqrt();
        assert!(
            r as f32 + tol >= hole_radius as f32,
            "vertex {v} at ({x:.5},{y:.5}) is inside hole (r={r:.5} < hole_radius={hole_radius:.5})"
        );
    }

    // Surface indices: must include nodes on the outer rim AND the inner
    // (hole) rim. Outer-rim test: at least one surface index has x or y at
    // the plate boundary. Inner-rim test: at least one surface index sits at
    // r ≈ hole_radius from the plate center.
    assert!(
        !surface_indices.is_empty(),
        "surface_node_indices must be non-empty"
    );
    let mut saw_outer_rim = false;
    let mut saw_inner_rim = false;
    let outer_tol = 1e-5_f32;
    let inner_tol = 1e-3_f32;
    for &idx in &surface_indices {
        let v = idx as usize;
        let x = mesh.vertices[v * 3];
        let y = mesh.vertices[v * 3 + 1];
        if x.abs() < outer_tol
            || (x - side as f32).abs() < outer_tol
            || y.abs() < outer_tol
            || (y - side as f32).abs() < outer_tol
        {
            saw_outer_rim = true;
        }
        let dx = x as f64 - cx;
        let dy = y as f64 - cy;
        let r = (dx * dx + dy * dy).sqrt();
        if (r - hole_radius).abs() < inner_tol as f64 {
            saw_inner_rim = true;
        }
    }
    assert!(saw_outer_rim, "surface_node_indices must include outer-rim nodes");
    assert!(saw_inner_rim, "surface_node_indices must include inner-rim (hole) nodes");
}
