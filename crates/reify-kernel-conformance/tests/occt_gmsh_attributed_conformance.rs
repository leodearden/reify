//! OCCT→gmsh `mesh_surface_to_volume_attributed` conformance arm — kernel-
//! seam ξ (task #5116) acceptance test (kernel-seam ε, INV-GEO-2).
//!
//! produce (OCCT box + tessellate) → validate(0.0) Ok → build face_anchors
//! (`common::occt_face_anchors`: `extract_faces` + face centroid) → consume
//! (`GeometryKernel::mesh_surface_to_volume_attributed`) → re-validate
//! (per-node attribution preserved + VolumeMesh structural invariants). See
//! `docs/prds/kernel-seam-contracts.md` §5 for the producer×consumer matrix
//! this crate implements.
//!
//! File-level gate: `has_occt` (producer) + `has_gmsh` (consumer) +
//! `feature = "mesh-morph"` (the attributed producer's own gate — see
//! `kernel_real.rs`'s `mesh_surface_to_volume_attributed` override).

#![cfg(all(has_occt, has_gmsh, feature = "mesh-morph"))]

mod common;

use reify_ir::{ElementOrderTag, GeometryKernel, GeometryOp, Value};
use reify_kernel_gmsh::GmshKernel;
use reify_kernel_occt::OcctKernel;

/// ξ (#5116) threads a vertex-merge correspondence map through
/// `repair_surface_mesh` so per-node attribution survives welding: the
/// attributed producer now WELDS unwelded OCCT input (merging near-coincident
/// vertices via `repair_surface_mesh_with_correspondence`) before handing it
/// to gmsh, rather than rejecting vertex-merge repair outright (the pre-ξ
/// #4876 stopgap that made this test crash rather than fail cleanly, so it
/// had to stay `#[ignore]`d). This test exercises exactly that path against a
/// REAL OCCT box tessellation (unwelded by design, per-face vertex blocks)
/// and must now pass unchanged (#5116's DoD) — this task's id is what this
/// test's now-removed `#[ignore]` cited.
#[test]
fn occt_box_attributed_volume_preserves_face_attribution() {
    let mut kernel = OcctKernel::new();
    let solid = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(20.0e-3),
            depth: Value::Real(30.0e-3),
        })
        .expect("box creation should succeed");
    let mesh = kernel
        .tessellate(solid.id, 1.0e-4)
        .expect("box tessellate should succeed");

    mesh.validate(0.0)
        .expect("real OCCT-tessellated box must satisfy the mesh contract");

    let face_anchors = common::occt_face_anchors(&mut kernel, solid.id);

    // 1 mm — generous vs gmsh's mesh size, yet well under half the box's
    // smallest (10 mm width) face-to-face-centroid separation (>=5 mm), so
    // it cannot accidentally match a neighbouring face's anchor. Mirrors the
    // ratio behind `mesh_surface_to_volume_attributed.rs`'s tolerance choice
    // for its unit-cube fixture.
    const MATCH_TOLERANCE_M: f64 = 1.0e-3;

    let gmsh = GmshKernel::new();
    let vm = gmsh
        .mesh_surface_to_volume_attributed(
            &mesh,
            ElementOrderTag::P1,
            &face_anchors,
            MATCH_TOLERANCE_M,
        )
        .expect(
            "mesh_surface_to_volume_attributed must accept a validated (but unwelded) OCCT box \
             mesh once #5116's attribution-aware repair lands",
        );

    assert!(
        vm.boundary.is_some(),
        "per-node attribution must be preserved on the returned VolumeMesh"
    );
    common::assert_valid_volume_mesh(&vm);
}
