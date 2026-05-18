//! Documents an observed gmsh property: `classify_surfaces` plus
//! `create_geometry` plus `mesh_generate(3)` does NOT preserve the input
//! discrete surface vertex set. The HXT pipeline re-meshes from the resulting
//! parametric geometry, so output node positions and indices are gmsh's
//! choice.
//!
//! This file pins the observation as a runtime diagnostic. It does not assert
//! "expected" output positions — gmsh may legitimately revise its meshing
//! choices across versions. The test only verifies that the well-known
//! re-meshing behaviour is in force (input cube corners are NOT all
//! coordinate-matchable against the output mesh) so that a future regression
//! to a different behaviour would surface here rather than silently
//! invalidate assumptions made by downstream consumers.
//!
//! Background: the original PRD `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.3
//! producer plan (task 3591) assumed input vertex `i` maps to output vertex
//! `i` after meshing. The diagnostic below disproves that premise on a
//! 2×2-subdivided unit cube and motivates the per-B-rep-entity attribution
//! redesign captured in the task description.
//!
//! Run with `--nocapture` to print the full vertex layout.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::MeshingOptions;
use reify_kernel_gmsh::mesh_volume::mesh_surface_to_volume_with_diagnostics;
use reify_types::{ElementOrderTag, Mesh};

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
fn count_corners_recovered(volume: &reify_types::VolumeMesh) -> usize {
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
/// does NOT preserve the input discrete surface vertex set on a
/// 2×2-subdivided unit cube. With `--nocapture` the test prints the full
/// vertex layout for follow-up investigation.
///
/// This is a *property-witness* test, not a contract test: it pins gmsh's
/// observed re-meshing behaviour so that a future change that DID preserve
/// input vertices would surface here, prompting reconsideration of the
/// NodeAttachment-producer design (task 3591).
#[test]
fn classify_plus_create_geometry_does_not_preserve_input_vertex_set() {
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

    // The observed behaviour as of gmsh 4.15: re-meshing throws away the
    // input discrete vertex set, so few or none of the 8 input corners
    // survive coordinate-matchable in the output. Assert the "few" bound
    // (< 8) — a future gmsh release that preserved all 8 would still trip
    // this test (intentionally, so the producer design assumption can be
    // revisited). Set REIFY_DUMP_GMSH_DIAG=1 to inspect the layout.
    assert!(
        recovered < 8,
        "expected gmsh's classify+create_geometry pipeline to drop some input \
         vertices (per task-3591 diagnostic finding), but found all 8 cube \
         corners in the output (m={m}). If this is intentional in a newer \
         gmsh, revisit the NodeAttachment producer redesign in task 3591."
    );
}
