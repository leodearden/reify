//! OCCT→gmsh `mesh_surface_to_volume` conformance arm (kernel-seam ε,
//! INV-GEO-2).
//!
//! For each real OCCT-tessellated fixture: produce → validate(α) → consume
//! (the `GeometryKernel::mesh_surface_to_volume` trait method on
//! `GmshKernel`, whose trait impl runs `repair_surface_mesh` so unwelded
//! OCCT input is accepted) → re-validate (`common::assert_valid_volume_mesh`
//! — VolumeMesh structural invariants; α has no VolumeMesh validator). See
//! `docs/prds/kernel-seam-contracts.md` §5 for the producer×consumer matrix
//! this crate implements.
//!
//! File-level gate: both `has_occt` (producer) and `has_gmsh` (consumer).

#![cfg(all(has_occt, has_gmsh))]

mod common;

use reify_ir::{ElementOrderTag, GeometryKernel};
use reify_kernel_gmsh::GmshKernel;

/// Full seam cycle for every real (non-sphere — see `common::fixtures`'s doc
/// comment) fixture: produce (OCCT tessellate) → validate(0.0) Ok → consume
/// (`GeometryKernel::mesh_surface_to_volume`, P1) → re-validate
/// (`common::assert_valid_volume_mesh`).
#[test]
fn occt_fixtures_mesh_to_volume_and_revalidate_through_gmsh() {
    for (name, build) in common::fixtures() {
        let mesh = build();

        mesh.validate(0.0).unwrap_or_else(|e| {
            panic!("[{name}] real OCCT-tessellated mesh must satisfy the mesh contract: {e:?}")
        });

        let gmsh = GmshKernel::new();
        let vm = gmsh
            .mesh_surface_to_volume(&mesh, ElementOrderTag::P1)
            .unwrap_or_else(|e| {
                panic!(
                    "[{name}] GeometryKernel::mesh_surface_to_volume must accept a validated \
                     (possibly unwelded) OCCT mesh: {e:?}"
                )
            });

        common::assert_valid_volume_mesh(&vm);
    }
}
