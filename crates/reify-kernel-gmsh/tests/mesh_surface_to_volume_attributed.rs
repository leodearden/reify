//! Tests for the `GmshKernel::mesh_surface_to_volume_attributed` trait method
//! (task 4092 — FEA face-selector boundary conditions).
//!
//! This is the gmsh override of the additive `GeometryKernel` trait method
//! added in step-4: it wraps the (previously orphaned) attribution producer
//! `mesh_surface_to_volume_with_attribution` and threads the resulting
//! [`reify_ir::BoundaryAssociation`] onto the produced
//! [`reify_ir::VolumeMesh`]'s `boundary` field, so the realization-read path
//! can surface it via `RealizationReadHandle::boundary()`.
//!
//! File-level gate: requires BOTH `has_gmsh` (real FFI build) AND the
//! `mesh-morph` feature, because the attribution producer it wraps is itself
//! `#[cfg(all(has_gmsh, feature = "mesh-morph"))]`. The self-dev-dep in
//! `Cargo.toml` activates `mesh-morph` for all integration test binaries.
#![cfg(all(has_gmsh, feature = "mesh-morph"))]

use reify_ir::{ElementOrderTag, GeometryError, GeometryHandleId, GeometryKernel, Mesh, NodeAttachment};
use reify_ir::geometry::MeshInvariant;
use reify_kernel_gmsh::GmshKernel;

fn h(n: u64) -> GeometryHandleId {
    GeometryHandleId(n)
}

/// Build a 2×2-subdivided unit cube (side 1.0, centred at origin):
/// 8 corners + 12 edge midpoints + 6 face centres = 26 unique vertices,
/// 48 triangles — watertight (shared corners). Mirrors the helper in
/// `tests/node_attachment_producer.rs`: the attribution producer requires a
/// watertight surface (it rejects vertex-merging repair, which would
/// invalidate per-node attribution).
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
    Mesh { vertices, indices, normals: None }
}

/// Per-face-explode [`subdivided_unit_cube_surface`]: duplicate every
/// triangle-corner reference into a fresh raw vertex slot, so
/// `Mesh::weldedness(_).raw_welded` is `false` (the raw index buffer no
/// longer references each distinct position exactly once) while the
/// position-welded quotient stays the identical closed, consistently-wound
/// cube (`Mesh::validate` still accepts it unchanged). Mirrors OCCT's
/// per-face-block tessellation output (occt_wrapper.cpp:5847) — the shape
/// the #4876 preflight (`preflight_watertight_surface`, mesh_boundary.rs)
/// and the gmsh attributed producer actually consume.
fn unwelded_subdivided_unit_cube_surface() -> Mesh {
    let welded = subdivided_unit_cube_surface();
    let mut vertices: Vec<f32> = Vec::with_capacity(welded.indices.len() * 3);
    for &raw_idx in &welded.indices {
        let base = raw_idx as usize * 3;
        vertices.extend_from_slice(&welded.vertices[base..base + 3]);
    }
    let indices: Vec<u32> = (0..welded.indices.len() as u32).collect();
    Mesh { vertices, indices, normals: None }
}

/// RED (task #4876): the attributed producer must reject a non-watertight
/// (unwelded) surface via the watertightness preflight
/// (`preflight_watertight_surface`, mesh_boundary.rs) BEFORE any gmsh FFI
/// call, returning `Err(GeometryError::MeshContractViolation{Closed})`
/// rather than crashing. Pins that the preflight is actually WIRED into
/// `mesh_surface_to_volume_attributed` (the trait override wraps
/// `mesh_surface_to_volume_with_attribution`).
///
/// The watertight-cube sibling test below
/// (`gmsh_mesh_surface_to_volume_attributed_threads_boundary_onto_volume_mesh`)
/// is the `Ok` negative control and must remain GREEN once the preflight is
/// wired (it accepts welded input unchanged).
///
/// RED now: with no preflight wired, the producer enters gmsh on the
/// unwelded surface (a crash under nextest process isolation, or a
/// non-`MeshContractViolation` error), so the
/// `Err(MeshContractViolation{Closed})` assertion fails. GREENed by the
/// mesh_boundary.rs wiring step.
#[test]
fn mesh_surface_to_volume_attributed_rejects_unwelded_surface() {
    let kernel = GmshKernel::new();
    let unwelded = unwelded_subdivided_unit_cube_surface();

    // Same 6 face anchors as the watertight-cube sibling test below.
    let face_anchors: Vec<(GeometryHandleId, [f64; 3])> = vec![
        (h(101), [0.0, 0.0, -0.5]),
        (h(102), [0.0, 0.0, 0.5]),
        (h(103), [0.0, -0.5, 0.0]),
        (h(104), [0.0, 0.5, 0.0]),
        (h(105), [-0.5, 0.0, 0.0]),
        (h(106), [0.5, 0.0, 0.0]),
    ];

    let err = kernel
        .mesh_surface_to_volume_attributed(&unwelded, ElementOrderTag::P1, &face_anchors, 0.3)
        .expect_err(
            "an unwelded (per-face-exploded) cube surface must be rejected by the \
             watertightness preflight before any gmsh FFI call — it must NOT crash",
        );

    match err {
        GeometryError::MeshContractViolation { invariant, .. } => {
            assert_eq!(
                invariant,
                MeshInvariant::Closed,
                "the preflight's synthesized violation must name the Closed obligation"
            );
        }
        other => panic!("expected GeometryError::MeshContractViolation, got {other:?}"),
    }
}

/// RED (task 4092 step-5): the gmsh `mesh_surface_to_volume_attributed` trait
/// method must produce a `VolumeMesh` whose `boundary` is `Some` and
/// non-empty, with the +Z face's nodes carrying positive Z (≈ +0.5).
///
/// The trait method takes only FACE anchors (the FEA face-selector use case),
/// so the gmsh override builds an `EntityAttribution` with only `faces`
/// populated (edges/vertices empty) and lifts the producer's boundary onto the
/// returned mesh. Mirrors `tests/node_attachment_producer.rs` assertions.
///
/// Fails until the gmsh override (step-6) lands: the trait default returns
/// `Err(GeometryError::OperationFailed(_))`, so `.expect(...)` panics.
#[test]
fn gmsh_mesh_surface_to_volume_attributed_threads_boundary_onto_volume_mesh() {
    let kernel = GmshKernel::new();
    let surface = subdivided_unit_cube_surface();

    // 6 unit-cube face centroids with distinct handles. +Z (top) face is h(102).
    // Tolerance 0.3 is generous vs the unit side length yet rejects gmsh's
    // spurious seam points (0.5 from any face centroid). No edge/vertex anchors
    // are supplied, so only dim-2 face entities are attributed.
    let h_top_z = h(102);
    let face_anchors: Vec<(GeometryHandleId, [f64; 3])> = vec![
        (h(101), [0.0, 0.0, -0.5]),   // bottom (−Z)
        (h_top_z, [0.0, 0.0, 0.5]),   // top (+Z)
        (h(103), [0.0, -0.5, 0.0]),   // front
        (h(104), [0.0, 0.5, 0.0]),    // back
        (h(105), [-0.5, 0.0, 0.0]),   // left
        (h(106), [0.5, 0.0, 0.0]),    // right
    ];

    let vm = kernel
        .mesh_surface_to_volume_attributed(&surface, ElementOrderTag::P1, &face_anchors, 0.3)
        .expect(
            "gmsh mesh_surface_to_volume_attributed must succeed on a watertight unit cube \
             (step-6 GREEN); the trait default returns Err",
        );

    // (1) boundary is threaded onto the returned VolumeMesh.
    let boundary = vm
        .boundary
        .as_ref()
        .expect("attributed producer must set VolumeMesh.boundary = Some");

    // (2) non-empty: some surface nodes are attributed.
    assert!(
        !boundary.is_empty(),
        "BoundaryAssociation must be non-empty for a unit-cube input"
    );

    // (3) the +Z face handle attributes some nodes, and every such node carries
    //     positive Z (≈ +0.5) — confirming the face→node-set mapping is
    //     geometrically sound (mirrors node_attachment_producer.rs locus check).
    let mut top_z_nodes = 0usize;
    for (idx, attachment) in boundary.iter() {
        if let NodeAttachment::OnFace(hid) = attachment
            && hid == h_top_z
        {
            let z = vm.vertices[idx as usize * 3 + 2] as f64;
            assert!(
                z > 0.25,
                "node idx={idx} attributed to the +Z face handle h(102) must have \
                 positive Z (≈ +0.5), got z={z}"
            );
            top_z_nodes += 1;
        }
    }
    assert!(
        top_z_nodes > 0,
        "expected at least one node attributed to the +Z face handle h(102)"
    );
}
