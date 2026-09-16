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

use std::collections::BTreeMap;

use reify_ir::{
    ElementOrderTag, GeometryError, GeometryHandleId, GeometryKernel, Mesh, NodeAttachment,
};
use reify_ir::geometry::MeshInvariant;
use reify_kernel_gmsh::GmshKernel;

fn h(n: u64) -> GeometryHandleId {
    GeometryHandleId(n)
}

/// Build a 2×2-subdivided unit cube (side 1.0, centred at origin):
/// 8 corners + 12 edge midpoints + 6 face centres = 26 unique vertices,
/// 48 triangles — watertight (shared corners). Mirrors the helper in
/// `tests/node_attachment_producer.rs`. Deliberately watertight so this
/// fixture exercises the attribution producer's already-watertight fast
/// path directly (`repair_cfg = None` on an already-watertight raw
/// surface skips the weld — `surface_needs_weld`, mesh_boundary.rs),
/// keeping this exact vertex numbering. Post-#5116 the producer no
/// longer REQUIRES a watertight surface (it welds unwelded input on
/// demand, see [`unwelded_subdivided_unit_cube_surface`] below);
/// watertightness here is a fixture property, not a producer
/// precondition.
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

/// The six unit-cube face centroids, one distinct handle each, in
/// `-Z, +Z, -Y, +Y, -X, +X` order (h(101)..h(106)). Every test in this file
/// attributes [`subdivided_unit_cube_surface`] (or a derivative of it)
/// against this exact list, so it is spelled once here.
///
/// The tolerance every caller pairs with it is 0.3: generous against the unit
/// side length, yet tight enough to reject gmsh's spurious seam points, which
/// sit 0.5 from any face centroid. No edge or vertex anchors are supplied, so
/// only dim-2 face entities are attributed.
fn six_face_anchors() -> Vec<(GeometryHandleId, [f64; 3])> {
    vec![
        (h(101), [0.0, 0.0, -0.5]),
        (h(102), [0.0, 0.0, 0.5]),
        (h(103), [0.0, -0.5, 0.0]),
        (h(104), [0.0, 0.5, 0.0]),
        (h(105), [-0.5, 0.0, 0.0]),
        (h(106), [0.5, 0.0, 0.0]),
    ]
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

/// GREEN (task ξ / #5116): the attributed producer now WELDS an unwelded
/// (per-face-exploded) surface — via
/// `repair_surface_mesh_with_correspondence` — before handing it to gmsh,
/// rather than rejecting it outright (the pre-ξ #4876 stopgap). Welding
/// merges the per-face-exploded cube's duplicate corners back into the
/// identical watertight cube the sibling test below builds directly, so the
/// watertightness preflight (`preflight_watertight_surface`, mesh_boundary.rs)
/// now runs on the WELDED surface and accepts it, and the producer proceeds
/// to attribute nodes as usual.
///
/// The watertight-cube sibling test below
/// (`gmsh_mesh_surface_to_volume_attributed_threads_boundary_onto_volume_mesh`)
/// remains the `Ok` positive control on already-welded input; this test pins
/// that UNWELDED (but position-quotient-closed) input is now equally
/// accepted, not merely tolerated.
#[test]
fn mesh_surface_to_volume_attributed_welds_unwelded_surface_and_attributes() {
    let kernel = GmshKernel::new();
    let unwelded = unwelded_subdivided_unit_cube_surface();

    let face_anchors = six_face_anchors();

    let vm = kernel
        .mesh_surface_to_volume_attributed(&unwelded, ElementOrderTag::P1, &face_anchors, 0.3)
        .expect(
            "an unwelded (per-face-exploded) cube surface must be welded and accepted by \
             the attributed producer (task ξ / #5116), not rejected",
        );

    let boundary = vm
        .boundary
        .as_ref()
        .expect("attributed producer must set VolumeMesh.boundary = Some on welded input");
    assert!(
        !boundary.is_empty(),
        "BoundaryAssociation must be non-empty for a welded unit-cube input"
    );
}

/// [`subdivided_unit_cube_surface`] with the entire TOP (z=+0.5) face's 8
/// triangles dropped, then per-face-exploded exactly like
/// [`unwelded_subdivided_unit_cube_surface`]. The result has a GENUINE open
/// boundary around the top perimeter: welding merges near-coincident
/// vertex *positions*, but it cannot fabricate the missing triangles that
/// would close the hole, so this fixture stays non-watertight even after
/// the producer's weld pre-stage.
///
/// `subdivided_unit_cube_surface`'s index buffer is emitted as six
/// contiguous 24-index (8-triangle) blocks in `Bottom, Top, Front, Back,
/// Left, Right` order (see that function's `#[rustfmt::skip]` comment
/// blocks); Top is the second block, i.e. `indices[24..48]`.
fn unwelded_subdivided_unit_cube_surface_missing_top_face() -> Mesh {
    let welded = subdivided_unit_cube_surface();
    let mut kept_indices: Vec<u32> = welded.indices[0..24].to_vec();
    kept_indices.extend_from_slice(&welded.indices[48..]);

    let mut vertices: Vec<f32> = Vec::with_capacity(kept_indices.len() * 3);
    for &raw_idx in &kept_indices {
        let base = raw_idx as usize * 3;
        vertices.extend_from_slice(&welded.vertices[base..base + 3]);
    }
    let indices: Vec<u32> = (0..kept_indices.len() as u32).collect();
    Mesh { vertices, indices, normals: None }
}

/// Negative control / fail-closed regression guard for the acceptance test
/// above: a surface with a REAL open boundary (the unit cube missing its
/// entire top face) must still be rejected by the #4876 watertightness
/// preflight even after the task-ξ weld pre-stage, because welding only
/// merges near-coincident vertex positions — it cannot synthesize the
/// missing triangles that would close the hole. Pins the fail-closed
/// invariant the old (pre-ξ) `mesh_surface_to_volume_attributed_rejects_unwelded_surface`
/// test used to cover, which the rework above (necessarily) no longer
/// exercises since it now asserts the opposite outcome on a *different*
/// (closed-on-quotient) input. Without this guard, a regression that let a
/// genuinely open surface reach gmsh's FFI (the original #4876 SIGSEGV)
/// would pass CI.
#[test]
fn mesh_surface_to_volume_attributed_rejects_surface_with_genuine_hole() {
    let kernel = GmshKernel::new();
    let holey = unwelded_subdivided_unit_cube_surface_missing_top_face();

    // Anchors are irrelevant here — the preflight rejects before any
    // entity-attribution matching runs — but reuse the sibling tests'
    // 6-anchor list for consistency.
    let face_anchors = six_face_anchors();

    let err = kernel
        .mesh_surface_to_volume_attributed(&holey, ElementOrderTag::P1, &face_anchors, 0.3)
        .expect_err(
            "a surface missing an entire face has a genuine open boundary that welding \
             cannot close, so the #4876 watertightness preflight must still reject it \
             (fail-closed, before any gmsh FFI call — no SIGSEGV)",
        );

    match err {
        GeometryError::MeshContractViolation {
            kernel: kernel_name,
            invariant,
            ..
        } => {
            assert_eq!(kernel_name, "gmsh", "violation must name the gmsh kernel");
            assert_eq!(
                invariant,
                MeshInvariant::Closed,
                "expected a Closed violation naming the missing-face open boundary"
            );
        }
        other => panic!(
            "expected Err(GeometryError::MeshContractViolation {{ invariant: Closed, .. }}), \
             got {other:?}"
        ),
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

    // The locus check below keys on the +Z (top) face handle.
    let h_top_z = h(102);
    let face_anchors = six_face_anchors();

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

/// The attributed producer must return a BIT-IDENTICAL mesh — and an
/// identical boundary association — for repeated calls on the same surface,
/// not merely a mesh of similar size.
///
/// # Why bit-identity is the right contract here
///
/// This producer is the morph arm's SOURCE-mesh supplier. `reify-eval`'s
/// `engine_build` stashes whatever `VolumeMesh` a realization produced as the
/// next tick's `MorphSource::source_mesh` — the stash itself accepts any
/// produced mesh — but `decide_morph_or_remesh` (`reify-eval`'s
/// `morph_producer`) then remeshes unless that source carries the task-4092
/// `BoundaryAssociation`, which only this attributed branch attaches. So in
/// practice this producer's output is the only mesh ever morphed.
/// `reify-mesh-morph` judges the MORPHED mesh against ABSOLUTE quality floors
/// (`MorphOptions::default`'s `quality_floor_min_scaled_jacobian` and
/// `quality_floor_pct_below_025`, both 0.01), so a source that varies
/// run-to-run makes the morph-or-remesh verdict a function of thread
/// scheduling rather than of the fixture. A count-tolerance contract would not
/// catch that; bit-identity does.
///
/// The boundary assertion is not implied by the two mesh ones. Attribution is
/// a separate post-`mesh_generate` gmsh entity-membership pass matched on
/// centroids within `match_tolerance`, so identical vertices and tets do not
/// by construction give an identical node→handle map — and it is that map,
/// not the buffers, that the morph projects each boundary node through.
///
/// MEASURED (task 7411): with `MeshingOptions::default()` at this file's
/// subject — `GmshKernel::mesh_surface_to_volume_attributed`, in
/// `reify-kernel-gmsh`'s `kernel_real` — this test is deterministically RED:
/// 3/3 separate processes failed on `tet_indices`, with per-rep tet counts
/// ranging 1187..1258 and every rep distinct. With `deterministic: true`
/// pinned there it is GREEN in 3/3 processes, all reps identical at verts=889
/// tets=1212 and identical ACROSS processes too. `MeshingOptions`'
/// `deterministic` field is deliberately excluded from the mesh cache key (see
/// its own doc, and `cache_key`), so the pin cannot alter cache-hit behaviour.
///
/// Deliberately takes no `MeshingOptions` parameter: the trait method has
/// none, which is the point. This exercises the production frame exactly as
/// `reify-eval`'s realization edge does, so it cannot pass by configuring
/// something production leaves unconfigured.
#[test]
fn attributed_producer_output_is_reproducible_across_repeated_calls() {
    /// One call's full observable output: both mesh buffers, plus the
    /// boundary association the morph arm actually consumes.
    struct Rep {
        vertices: Vec<f32>,
        tets: Vec<u32>,
        boundary: Vec<(u32, NodeAttachment)>,
    }

    /// Attributed-node count per B-rep handle — the coarse, legible boundary
    /// signal, next to which the full node→handle map is hundreds of pairs.
    fn nodes_per_handle(boundary: &[(u32, NodeAttachment)]) -> BTreeMap<u64, usize> {
        let mut counts: BTreeMap<u64, usize> = BTreeMap::new();
        for (_, attachment) in boundary {
            let handle = match attachment {
                NodeAttachment::OnFace(handle)
                | NodeAttachment::OnEdge(handle)
                | NodeAttachment::OnVertex(handle) => handle.0,
            };
            *counts.entry(handle).or_insert(0) += 1;
        }
        counts
    }

    let kernel = GmshKernel::new();
    let surface = subdivided_unit_cube_surface();
    let face_anchors = six_face_anchors();

    const REPS: usize = 4;
    let mut reps: Vec<Rep> = Vec::with_capacity(REPS);
    for rep in 0..REPS {
        let vm = kernel
            .mesh_surface_to_volume_attributed(&surface, ElementOrderTag::P1, &face_anchors, 0.3)
            .unwrap_or_else(|e| {
                panic!("attributed producer must succeed on the watertight unit cube (rep {rep}): {e:?}")
            });
        let tets = vm
            .tet_indices()
            .expect("P1 tet mesh must have tet_indices")
            .to_vec();
        let boundary = vm
            .boundary
            .as_ref()
            .expect("attributed producer must set VolumeMesh.boundary = Some")
            .iter()
            .collect();
        reps.push(Rep { vertices: vm.vertices.clone(), tets, boundary });
    }

    let first = &reps[0];
    for (rep, this) in reps.iter().enumerate().skip(1) {
        // tet_indices first: the coarser, more legible signal.
        assert_eq!(
            this.tets.len() / 4,
            first.tets.len() / 4,
            "tet COUNT differs between rep 0 ({} tets) and rep {rep} ({} tets) — the \
             attributed producer is not reproducible; check that \
             `GmshKernel::mesh_surface_to_volume_attributed` still pins \
             `deterministic: true` in the `MeshingOptions` it injects",
            first.tets.len() / 4,
            this.tets.len() / 4
        );
        assert_eq!(
            this.tets, first.tets,
            "tet_indices differ between rep 0 and rep {rep} at equal tet count ({} tets) — \
             the attributed producer is not bit-reproducible",
            first.tets.len() / 4
        );
        assert_eq!(
            this.vertices, first.vertices,
            "vertices differ between rep 0 and rep {rep} ({} verts) despite identical \
             tet_indices — the attributed producer is not bit-reproducible",
            first.vertices.len() / 3
        );

        // Boundary last, and asserted separately from the buffers: what the
        // morph arm consumes is the node→handle map.
        assert_eq!(
            nodes_per_handle(&this.boundary),
            nodes_per_handle(&first.boundary),
            "per-handle attributed-node COUNTS differ between rep 0 and rep {rep} — \
             `decide_morph_or_remesh` requires this association and the morph projects \
             every boundary node through it, so an unstable attribution flips the \
             morph-or-remesh verdict even on an otherwise identical mesh"
        );
        let divergence = this
            .boundary
            .iter()
            .zip(&first.boundary)
            .find(|(this_pair, first_pair)| this_pair != first_pair);
        assert!(
            divergence.is_none(),
            "boundary node→handle attribution differs between rep 0 and rep {rep} despite \
             identical per-handle counts and identical mesh buffers; first divergence \
             (rep {rep} pair, rep 0 pair): {divergence:?}. The morph projects each \
             boundary node through this map, so a reshuffle flips the morph-or-remesh \
             verdict `decide_morph_or_remesh` reaches"
        );
    }
}
