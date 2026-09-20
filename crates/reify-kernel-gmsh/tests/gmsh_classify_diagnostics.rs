//! Documents two observed properties of `classify_surfaces` plus
//! `create_geometry` plus `mesh_generate(3)`, which together are what
//! `mesh_to_volume` runs:
//!
//!  1. The geometric CORNERS of the input solid survive. A complete B-rep
//!     decomposition emits dim-0 (corner point) entities, and gmsh places a
//!     node on each, so every one of a cube's 8 corner positions is
//!     coordinate-matchable in the output mesh.
//!  2. The input vertex SET as a whole is still not preserved. HXT re-meshes
//!     from the reconstructed parametric geometry, so output node positions,
//!     count and indices are gmsh's choice: a 26-vertex input yields far more
//!     output nodes and no index-identity map.
//!
//! Measured on this fixture (a 2×2-subdivided unit cube, 26 input vertices):
//! 8/8 corners recovered, m = 344 output vertices.
//!
//! This file pins those properties as a runtime diagnostic. It does not assert
//! "expected" output positions or an exact node count — gmsh may legitimately
//! revise its meshing choices across versions — so that a future change to
//! either property surfaces here rather than silently invalidating assumptions
//! made by downstream consumers.
//!
//! Background: property 1 has not always held. Until #6200, `mesh_to_volume`
//! passed `classify_surfaces` a feature angle of FRAC_PI_2 (90°); gmsh's
//! sharp-edge test is strictly-greater-than and a cube's dihedral angle is
//! exactly 90°, so no edge registered as sharp, the B-rep decomposition
//! degenerated (dim0/dim1/dim2 = 4/4/2 for a box) and the corners were lost
//! along with ~26% of the solid's volume. With the production feature angle
//! now FRAC_PI_4 (`reify_kernel_gmsh::CLASSIFY_FEATURE_ANGLE`) the census is
//! 8/14/8 and the corners are recovered. Property 1 as witnessed here agrees
//! with `tests/node_attachment_producer.rs`, which demands exactly the 8
//! cube-corner handles 301..=308 as `OnVertex` via the `mesh_boundary.rs`
//! attributed producer — the two producers now behave alike.
//!
//! Property 2 is why the per-B-rep-entity attribution design exists at all:
//! the original PRD `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.3 producer
//! plan assumed input vertex `i` maps to output vertex `i` after meshing, and
//! the diagnostic below still disproves that premise.
//!
//! Run with `--nocapture` to print the full vertex layout.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::MeshingOptions;
use reify_kernel_gmsh::mesh_volume::mesh_surface_to_volume_with_diagnostics;
use reify_ir::{ElementOrderTag, Mesh};

/// Build a 2×2-subdivided unit cube centred at the origin (side 1.0):
/// 8 corners + 12 edge midpoints + 6 face centers = 26 unique vertices,
/// 48 triangles (6 faces × 8 sub-triangles, outward-facing).
fn subdivided_unit_cube_surface() -> Mesh {
    #[rustfmt::skip]
    let corners: [[f32; 3]; 8] = [
        [-0.5, -0.5, -0.5], [ 0.5, -0.5, -0.5],
        [-0.5,  0.5, -0.5], [ 0.5,  0.5, -0.5],
        [-0.5, -0.5,  0.5], [ 0.5, -0.5,  0.5],
        [-0.5,  0.5,  0.5], [ 0.5,  0.5,  0.5],
    ];
    #[rustfmt::skip]
    let edges: [[f32; 3]; 12] = [
        [ 0.0, -0.5, -0.5], [-0.5,  0.0, -0.5], [ 0.5,  0.0, -0.5], [ 0.0,  0.5, -0.5],
        [ 0.0, -0.5,  0.5], [-0.5,  0.0,  0.5], [ 0.5,  0.0,  0.5], [ 0.0,  0.5,  0.5],
        [-0.5, -0.5,  0.0], [ 0.5, -0.5,  0.0], [-0.5,  0.5,  0.0], [ 0.5,  0.5,  0.0],
    ];
    #[rustfmt::skip]
    let face_centers: [[f32; 3]; 6] = [
        [ 0.0,  0.0, -0.5], [ 0.0,  0.0,  0.5],
        [ 0.0, -0.5,  0.0], [ 0.0,  0.5,  0.0],
        [-0.5,  0.0,  0.0], [ 0.5,  0.0,  0.0],
    ];

    let mut vertices: Vec<f32> = Vec::with_capacity(26 * 3);
    for c in &corners { vertices.extend_from_slice(c); }
    for e in &edges   { vertices.extend_from_slice(e); }
    for f in &face_centers { vertices.extend_from_slice(f); }
    assert_eq!(vertices.len(), 78);

    #[rustfmt::skip]
    let indices: Vec<u32> = vec![
        // Bottom (z=-0.5)
        0, 9,20,  0,20, 8,  8,20,10,  8,10, 1,
        9, 2,11,  9,11,20, 20,11, 3, 20, 3,10,
        // Top (z=0.5)
        4,12,21,  4,21,13, 12, 5,14, 12,14,21,
       13,21,15, 13,15, 6, 21,14, 7, 21, 7,15,
        // Front (y=-0.5)
        0, 8,22,  0,22,16,  8, 1,17,  8,17,22,
       16,22,12, 16,12, 4, 22,17, 5, 22, 5,12,
        // Back (y=0.5)
        2,18,23,  2,23,11, 11,23,19, 11,19, 3,
       18, 6,15, 18,15,23, 23,15, 7, 23, 7,19,
        // Left (x=-0.5)
        0,16,24,  0,24, 9,  9,24,18,  9,18, 2,
       16, 4,13, 16,13,24, 24,13, 6, 24, 6,18,
        // Right (x=0.5)
        1,10,25,  1,25,17, 10, 3,19, 10,19,25,
       17,25,14, 17,14, 5, 25,19, 7, 25, 7,14,
    ];
    assert_eq!(indices.len(), 144);

    Mesh { vertices, indices, normals: None }
}

/// Count how many of the 8 cube-corner positions are present in the output
/// VolumeMesh's vertex array (within squared tolerance `1e-6`).
fn count_corners_recovered(volume: &reify_ir::VolumeMesh) -> usize {
    let corners: [[f32; 3]; 8] = [
        [-0.5, -0.5, -0.5], [ 0.5, -0.5, -0.5],
        [-0.5,  0.5, -0.5], [ 0.5,  0.5, -0.5],
        [-0.5, -0.5,  0.5], [ 0.5, -0.5,  0.5],
        [-0.5,  0.5,  0.5], [ 0.5,  0.5,  0.5],
    ];
    let m = volume.vertices.len() / 3;
    corners
        .iter()
        .filter(|c| {
            (0..m).any(|j| {
                let dx = volume.vertices[j * 3] - c[0];
                let dy = volume.vertices[j * 3 + 1] - c[1];
                let dz = volume.vertices[j * 3 + 2] - c[2];
                dx * dx + dy * dy + dz * dz < 1e-6
            })
        })
        .count()
}

/// Documents that `classify_surfaces` + `create_geometry` + `mesh_generate(3)`
/// recovers all 8 geometric corners of a 2×2-subdivided unit cube, while still
/// re-meshing the rest of the vertex set. With `--nocapture` the test prints
/// the full vertex layout for follow-up investigation.
///
/// This is a *property-witness* test, not a contract test: it pins gmsh's
/// observed behaviour under the production classify angles so that a future
/// change to either half — corners lost, or the output collapsing back onto
/// the input vertex set — surfaces here.
#[test]
fn classify_plus_create_geometry_recovers_corners_but_remeshes_the_rest() {
    let surface = subdivided_unit_cube_surface();
    let report = mesh_surface_to_volume_with_diagnostics(
        &surface,
        &MeshingOptions { mesh_size: None, deterministic: true, ..Default::default() },
        ElementOrderTag::P1,
        None,
        None,
        None,
    )
    .expect("diagnostics meshing must succeed on a closed cube");

    let m = report.volume.vertices.len() / 3;
    let recovered = count_corners_recovered(&report.volume);

    eprintln!(
        "Total output vertices: {m} (input had 26); corners recovered: {recovered}/8"
    );
    if std::env::var("REIFY_DUMP_GMSH_DIAG").is_ok() {
        for j in 0..m {
            let x = report.volume.vertices[j * 3];
            let y = report.volume.vertices[j * 3 + 1];
            let z = report.volume.vertices[j * 3 + 2];
            eprintln!("  out[{j:3}]: [{x:.4}, {y:.4}, {z:.4}]");
        }
    }

    // Property 1 — the corners survive. Under the production feature angle
    // (`CLASSIFY_FEATURE_ANGLE` = FRAC_PI_4) the B-rep decomposition emits
    // dim-0 corner entities and gmsh nodes each one, so every cube corner is
    // coordinate-matchable in the output. A regression to a feature angle of
    // FRAC_PI_2 or above drops these back to 4-or-fewer (that was #6200, and
    // it cost ~26% of the solid's volume). Set REIFY_DUMP_GMSH_DIAG=1 to
    // inspect the layout.
    assert_eq!(
        recovered, 8,
        "expected all 8 cube corners to survive classify+create_geometry as \
         B-rep corner (dim-0) entities, but only {recovered} were \
         coordinate-matchable in the output (m={m}). A feature angle of \
         FRAC_PI_2 or above reproduces exactly this (see #6200): gmsh's \
         sharp-edge test is strictly-greater-than and a cube's dihedral angle \
         is exactly 90°. Check reify_kernel_gmsh::CLASSIFY_FEATURE_ANGLE."
    );

    // Property 2 — the rest of the vertex set is still gmsh's choice, not the
    // input's. Only the direction is pinned, not the measured m=344: HXT is
    // free to revise node placement across versions, but collapsing back onto
    // the 26 input vertices would mean the pipeline had stopped re-meshing,
    // which would invalidate the per-B-rep-entity attribution design that
    // exists precisely because index-identity does not hold.
    assert!(
        m > 26,
        "expected gmsh to re-mesh from the reconstructed geometry and emit \
         more nodes than the 26 input vertices, but got m={m}. If the output \
         now tracks the input vertex set, the NodeAttachment producer's \
         no-index-identity premise needs revisiting."
    );
}
