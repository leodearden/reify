//! Integration tests for the `mesh_volume` pipeline wrapper and its helpers.
//!
//! This file tests both the pure-Rust helpers (no `cfg(has_gmsh)` gate) and
//! the FFI-backed orchestrating wrapper (`#[cfg(has_gmsh)]` module).
//!
//! Pure-Rust helper tests run on every host — no libgmsh required.
//! Integration tests that call `mesh_surface_to_volume_with_diagnostics` are
//! inside the `with_libgmsh` module, gated with `#[cfg(has_gmsh)]`.

use reify_kernel_gmsh::auto_size::AutoSizeConfig;
use reify_kernel_gmsh::mesh_volume::{
    apply_repair_if_requested, compute_thickness_warnings, resolve_mesh_size,
    MeshSurfaceToVolumeReport,
};
use reify_kernel_gmsh::repair::RepairConfig;
use reify_kernel_gmsh::through_thickness::ThroughThicknessConfig;
use reify_kernel_gmsh::MeshingOptions;
use reify_types::{ElementOrderTag, Mesh, VolumeMesh};

// ---------------------------------------------------------------------------
// Helpers shared across multiple tests in this file
// ---------------------------------------------------------------------------

/// A sliver-laden mesh: one good equilateral-ish triangle + one sliver
/// triangle (near-collinear vertices). Mirrors the mesh constructed in
/// `repair_tests.rs::sliver_triangles_below_area_threshold_are_collapsed`.
fn sliver_mesh() -> Mesh {
    Mesh {
        vertices: vec![
            // Triangle 0 corners (equilateral-ish, area ~0.43)
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.5, 0.866, 0.0, // v2
            // Triangle 1 corners (collinear-ish, area ~5e-9 — sliver)
            5.0, 0.0, 0.0, // v3
            6.0, 0.0, 0.0, // v4
            5.5, 1e-8, 0.0, // v5
        ],
        indices: vec![
            0, 1, 2, // good triangle
            3, 4, 5, // sliver
        ],
        normals: None,
    }
}

// ---------------------------------------------------------------------------
// apply_repair_if_requested — None passes through, Some delegates
// ---------------------------------------------------------------------------

/// `apply_repair_if_requested` with `None` must return the input unchanged:
/// identical vertices and indices (Cow::Borrowed semantics — no copy, no repair).
#[test]
fn apply_repair_if_requested_none_passes_input_through() {
    let mesh = sliver_mesh();
    let result = apply_repair_if_requested(&mesh, None);
    assert_eq!(
        result.vertices, mesh.vertices,
        "None must pass vertices through unchanged"
    );
    assert_eq!(
        result.indices, mesh.indices,
        "None must pass indices through unchanged"
    );
}

/// `apply_repair_if_requested` with `Some(cfg)` must delegate to
/// `repair_surface_mesh` and return the repaired mesh (fewer indices because
/// the sliver triangle was dropped).
#[test]
fn apply_repair_if_requested_some_delegates_to_repair_surface_mesh() {
    let mesh = sliver_mesh();
    let cfg = RepairConfig {
        sliver_area_threshold: 1e-6,
        vertex_merge_epsilon: 1e-9,
    };
    let result = apply_repair_if_requested(&mesh, Some(cfg));
    assert!(
        result.indices.len() < mesh.indices.len(),
        "Some(cfg) must invoke repair and drop the sliver triangle; \
         before: {} indices, after: {} indices",
        mesh.indices.len(),
        result.indices.len()
    );
    // The sliver is dropped — only one triangle (3 indices) survives.
    assert_eq!(
        result.indices.len(),
        3,
        "exactly one triangle should survive; got {} indices",
        result.indices.len()
    );
}

// ---------------------------------------------------------------------------
// resolve_mesh_size — caller-wins, auto-fires, none-defers
// ---------------------------------------------------------------------------

/// A unit cube surface mesh — 8 vertices, 12 triangles (2 per face).
/// Inline duplicate of `mesh_to_volume_tests.rs::unit_cube_mesh`.
fn unit_cube_mesh() -> Mesh {
    Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            1.0, 0.0, 0.0, // 1
            1.0, 1.0, 0.0, // 2
            0.0, 1.0, 0.0, // 3
            0.0, 0.0, 1.0, // 4
            1.0, 0.0, 1.0, // 5
            1.0, 1.0, 1.0, // 6
            0.0, 1.0, 1.0, // 7
        ],
        #[rustfmt::skip]
        indices: vec![
            0, 2, 1,  0, 3, 2,
            4, 5, 6,  4, 6, 7,
            0, 1, 5,  0, 5, 4,
            3, 7, 6,  3, 6, 2,
            0, 4, 7,  0, 7, 3,
            1, 2, 6,  1, 6, 5,
        ],
        normals: None,
    }
}

/// Caller's explicit `mesh_size` must win over the auto-derived value, even
/// when both are supplied. Pin: the caller-wins policy from the design decision.
#[test]
fn resolve_mesh_size_caller_value_wins_over_auto() {
    let cube = unit_cube_mesh();
    let options = MeshingOptions {
        mesh_size: Some(0.42),
        ..Default::default()
    };
    let result = resolve_mesh_size(&cube, &options, Some(AutoSizeConfig::default()));
    let size = result.expect("caller-wins: must succeed");
    assert_eq!(
        size,
        Some(0.42),
        "caller's Some(0.42) must win over auto-size even when auto_size_cfg is Some"
    );
}

/// When both `mesh_size` and `auto_size_cfg` are `None`, the function must
/// return `Ok(None)` — deferring to `mesh_to_volume`'s internal default.
#[test]
fn resolve_mesh_size_no_caller_no_auto_returns_none() {
    let mesh = sliver_mesh();
    let options = MeshingOptions::default(); // mesh_size: None
    let result = resolve_mesh_size(&mesh, &options, None);
    let size = result.expect("none/none: must succeed");
    assert_eq!(
        size,
        None,
        "no caller override + no auto_size_cfg must return Ok(None)"
    );
}

/// When the caller's `mesh_size` is unset but `auto_size_cfg` is `Some`,
/// the function must call `auto_mesh_size_from_features` and return its result.
/// For a single triangle with all edges of length 0.5 and multiplier=1.0,
/// the auto-derived size is ≈ 0.5.
#[test]
fn resolve_mesh_size_auto_fires_when_caller_unset() {
    // Triangle with all edges exactly 0.5 m long.
    let mesh = Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            0.5, 0.0, 0.0, // v1 — edge v0→v1 = 0.5
            0.25, 0.433012702_f32, 0.0, // v2 — equilateral (approx)
        ],
        indices: vec![0, 1, 2],
        normals: None,
    };
    let options = MeshingOptions::default(); // mesh_size: None
    let result = resolve_mesh_size(&mesh, &options, Some(AutoSizeConfig::default()));
    let size = result.expect("auto_size must succeed for a well-formed triangle");
    let size = size.expect("auto_size must return Some for a non-empty mesh");
    assert!(
        (size - 0.5).abs() < 0.01,
        "auto-derived size should be ≈ 0.5 (smallest edge length × 1.0 multiplier); got {size}"
    );
}

/// When `auto_size_cfg` fires but the mesh has no indices, `auto_mesh_size_from_features`
/// returns `Ok(0.0)`. The wrapper must collapse `0.0` to `None` (per design
/// decision: zero means "auto-size unavailable", defer to kernel default).
#[test]
fn resolve_mesh_size_empty_indices_collapses_to_none() {
    let mesh = Mesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        indices: vec![], // no triangles → auto returns 0.0
        normals: None,
    };
    let options = MeshingOptions::default();
    let result = resolve_mesh_size(&mesh, &options, Some(AutoSizeConfig::default()));
    let size = result.expect("empty-indices collapse: must succeed");
    assert_eq!(
        size,
        None,
        "auto returns 0.0 for empty-indices mesh; wrapper must collapse to Ok(None)"
    );
}

// ---------------------------------------------------------------------------
// compute_thickness_warnings — None skips, Some delegates
// ---------------------------------------------------------------------------

/// Surface mesh of an axis-aligned 10×10×0.5 slab.
/// Inline duplicate of `through_thickness_tests.rs::slab_surface_mesh`.
fn slab_surface_mesh() -> Mesh {
    let v = vec![
        0.0, 0.0, 0.0,
        10.0, 0.0, 0.0,
        10.0, 10.0, 0.0,
        0.0, 10.0, 0.0,
        0.0, 0.0, 0.5,
        10.0, 0.0, 0.5,
        10.0, 10.0, 0.5,
        0.0, 10.0, 0.5,
    ];
    let i = vec![
        0, 2, 1, 0, 3, 2,
        4, 5, 6, 4, 6, 7,
        0, 1, 5, 0, 5, 4,
        1, 2, 6, 1, 6, 5,
        2, 3, 7, 2, 7, 6,
        3, 0, 4, 3, 4, 7,
    ];
    Mesh { vertices: v, indices: i, normals: None }
}

/// Single-tet volume mesh that spans the slab thickness (mirrors the
/// fixture in `through_thickness_tests.rs::single_layer_tet_through_thin_region_emits_warning`).
fn single_tet_slab_volume() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0,   // 0
            10.0, 0.0, 0.0,  // 1
            10.0, 10.0, 0.5, // 2
            0.0, 10.0, 0.5,  // 3
        ],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    }
}

/// `compute_thickness_warnings` with `None` must return an empty Vec
/// regardless of how thin the slab is — the stage is skipped entirely.
#[test]
fn compute_thickness_warnings_none_returns_empty() {
    let surface = slab_surface_mesh();
    let volume = single_tet_slab_volume();
    let warnings = compute_thickness_warnings(&volume, &surface, None);
    assert!(
        warnings.is_empty(),
        "None cfg must skip the through-thickness stage; got {} warning(s)",
        warnings.len()
    );
}

/// `compute_thickness_warnings` with `Some(cfg)` must delegate to
/// `through_thickness_check` and return warnings. For a single-tet slab the
/// layer count is 1, which is below the min_elements_through_thickness=2
/// default, so at least one warning is expected.
#[test]
fn compute_thickness_warnings_some_delegates_to_through_thickness_check() {
    let surface = slab_surface_mesh();
    let volume = single_tet_slab_volume();
    let warnings = compute_thickness_warnings(&volume, &surface, Some(ThroughThicknessConfig::default()));
    assert!(
        !warnings.is_empty(),
        "Some(cfg) must delegate to through_thickness_check; \
         single-tet slab should produce at least one warning"
    );
    assert_eq!(
        warnings[0].element_count,
        1,
        "single-tet slab must be detected as 1-element-thick; got {}",
        warnings[0].element_count
    );
}
