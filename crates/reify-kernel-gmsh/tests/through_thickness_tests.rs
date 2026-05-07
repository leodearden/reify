//! Tests for the through-thickness element-count diagnostic.
//!
//! Per the v0.3 FEA PRD, a thin region with only one tet through its
//! thickness produces an FEA mesh that's almost guaranteed to under-predict
//! deflection by an order of magnitude. The diagnostic scans the produced
//! volume mesh and emits a warning whenever any geometric thickness has
//! fewer than `min_elements_through_thickness` (default 2) tets through it.

use reify_kernel_gmsh::through_thickness::{
    through_thickness_check, ThroughThicknessConfig, ThroughThicknessWarning,
};
use reify_types::{ElementOrderTag, Mesh, VolumeMesh};

/// Surface mesh of an axis-aligned 10×10×0.5 slab — six faces, two
/// triangles per face = 12 triangles. The thickness direction is Z.
fn slab_surface_mesh() -> Mesh {
    let v = vec![
        // Bottom face Z=0 (4 verts: 0..3)
        0.0, 0.0, 0.0,
        10.0, 0.0, 0.0,
        10.0, 10.0, 0.0,
        0.0, 10.0, 0.0,
        // Top face Z=0.5 (4 verts: 4..7)
        0.0, 0.0, 0.5,
        10.0, 0.0, 0.5,
        10.0, 10.0, 0.5,
        0.0, 10.0, 0.5,
    ];
    let i = vec![
        // Bottom (CCW from below)
        0, 2, 1, 0, 3, 2, // Top
        4, 5, 6, 4, 6, 7, // Side faces (we don't care about exact winding for this test)
        0, 1, 5, 0, 5, 4, //
        1, 2, 6, 1, 6, 5, //
        2, 3, 7, 2, 7, 6, //
        3, 0, 4, 3, 4, 7, //
    ];
    Mesh {
        vertices: v,
        indices: i,
        normals: None,
    }
}

/// Slab with only ONE tet spanning the thickness direction triggers a
/// warning whose message names "fewer than 2 elements" and includes a
/// numeric mesh_size suggestion.
#[test]
fn single_layer_tet_through_thin_region_emits_warning() {
    let surface = slab_surface_mesh();
    // 8 vertices of the slab volume (matching the surface vertex layout) +
    // a SINGLE tet that "spans" Z=0 → Z=0.5. We don't need a fully meshed
    // slab — the diagnostic counts the layers of unique tet centroids along
    // the thinnest axis, and one tet means one layer.
    let volume = VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0,    // 0
            10.0, 0.0, 0.0,   // 1
            10.0, 10.0, 0.5,  // 2
            0.0, 10.0, 0.5,   // 3
        ],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig {
        min_elements_through_thickness: 2,
    };
    let warnings = through_thickness_check(&volume, &surface, cfg);
    assert!(
        !warnings.is_empty(),
        "single-layer thin region must emit at least one warning"
    );
    let w = &warnings[0];
    assert!(
        w.message.contains("fewer than 2"),
        "warning message must call out 'fewer than 2 elements'; got: {}",
        w.message
    );
    // Heuristic check: the message should mention 'mesh_size' as part of the
    // remediation guidance.
    assert!(
        w.message.to_lowercase().contains("mesh_size"),
        "warning should suggest a smaller mesh_size; got: {}",
        w.message
    );
}

/// A well-resolved slab with several tet layers through the thickness emits
/// no warnings.
#[test]
fn well_resolved_thickness_emits_no_warning() {
    let surface = slab_surface_mesh();
    // Four tets stacked along Z: centroids at Z ≈ 0.0625, 0.1875, 0.3125,
    // 0.4375 (four distinct layers in the thinnest direction).
    let volume = VolumeMesh {
        vertices: vec![
            // Tet 0: Z 0.0..0.125
            0.0, 0.0, 0.0,
            10.0, 0.0, 0.0,
            10.0, 10.0, 0.125,
            0.0, 10.0, 0.125,
            // Tet 1: Z 0.125..0.25
            0.0, 0.0, 0.125,
            10.0, 0.0, 0.125,
            10.0, 10.0, 0.25,
            0.0, 10.0, 0.25,
            // Tet 2: Z 0.25..0.375
            0.0, 0.0, 0.25,
            10.0, 0.0, 0.25,
            10.0, 10.0, 0.375,
            0.0, 10.0, 0.375,
            // Tet 3: Z 0.375..0.5
            0.0, 0.0, 0.375,
            10.0, 0.0, 0.375,
            10.0, 10.0, 0.5,
            0.0, 10.0, 0.5,
        ],
        tet_indices: vec![
            0, 1, 2, 3, //
            4, 5, 6, 7, //
            8, 9, 10, 11, //
            12, 13, 14, 15, //
        ],
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig {
        min_elements_through_thickness: 2,
    };
    let warnings = through_thickness_check(&volume, &surface, cfg);
    assert!(
        warnings.is_empty(),
        "4-tet-thick slab should produce no warnings; got {} warnings: {:?}",
        warnings.len(),
        warnings.iter().map(|w| &w.message).collect::<Vec<_>>(),
    );
}

/// The warning struct exposes a `region_index` field so future v0.4+
/// per-face/per-region analysis can attach a face/region identifier.
/// For v0.3 the whole body is a single region (`region_index = 0`).
#[test]
fn warning_includes_face_or_region_identifier() {
    let surface = slab_surface_mesh();
    let volume = VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0,
            10.0, 0.0, 0.0,
            10.0, 10.0, 0.5,
            0.0, 10.0, 0.5,
        ],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig::default();
    let warnings: Vec<ThroughThicknessWarning> =
        through_thickness_check(&volume, &surface, cfg);
    assert!(
        !warnings.is_empty(),
        "expected at least one warning to inspect its region_index"
    );
    // Confirm the region_index field is reachable as a struct member; in v0.3
    // the placeholder is 0 (single-region), but the field exists so v0.4+
    // refinement can attach face/region IDs without changing the struct shape.
    let _: usize = warnings[0].region_index;
    // Defence-in-depth: thickness and element_count fields are also reachable
    // — these are part of the public diagnostic surface.
    let _: f64 = warnings[0].thickness;
    let _: u32 = warnings[0].element_count;
}
