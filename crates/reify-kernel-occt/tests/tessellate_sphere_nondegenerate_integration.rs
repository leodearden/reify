//! Producer-level regression test for INV-GEO-1: `tessellate_shape` must not
//! emit zero-area triangles at a periodic surface's poles.
//!
//! Background: a full sphere is a periodic surface with two poles.
//! `BRepMesh_IncrementalMesh` emits one raw pole node per meridian, all at a
//! bit-identical 3D position. The seam fan triangle whose two non-ring
//! corners are duplicate pole nodes therefore has two coincident corners —
//! literally zero area (this shows up directly on the RAW, unwelded vertex
//! buffer: `Mesh::validate`'s `NonDegenerate` obligation computes the
//! `(b-a) × (c-a)` cross product from raw `Mesh.vertices` positions indexed
//! by raw `Mesh.indices` — see `reify_ir::geometry::Mesh::check_contract`,
//! `crates/reify-ir/src/geometry.rs:2838-2880` — so no welding is needed for
//! two coincident raw positions to zero out the cross product).
//!
//! Exercised at TWO deflections (1e-4 and 1e-3) to prove the artifact is
//! topological — a pole always produces coincident nodes regardless of mesh
//! resolution — not a resolution/tolerance workaround.
//!
//! RED on base: `mesh.validate(0.0)` returns
//! `MeshContractViolation { invariant: NonDegenerate, counts.degenerate_tris: 2, .. }`
//! (one degenerate seam triangle per pole) at both deflections. GREEN after
//! task #5164's `tessellate_shape` fix (skip zero-area triangle emission at
//! index-emission time; see `docs/prds/kernel-seam-contracts.md` §2
//! obligation 3).

#![cfg(has_occt)]

use reify_ir::{GeometryOp, Value};
use reify_kernel_occt::OcctKernel;

// ---------------------------------------------------------------------------
// Shared helper — build + tessellate an 8mm-radius sphere
// ---------------------------------------------------------------------------

fn tessellate_sphere(tol: f64) -> reify_ir::Mesh {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(8.0e-3),
        })
        .expect("sphere creation should succeed");
    kernel
        .tessellate(h.id, tol)
        .expect("tessellate should succeed")
}

// ---------------------------------------------------------------------------
// Test — no zero-area pole-seam triangles, at either deflection (step-1)
// ---------------------------------------------------------------------------

/// A full sphere is a periodic surface with two poles. Real OCCT
/// tessellation must not leave zero-area pole-seam triangles behind:
/// `validate(0.0)` must be Ok, and every emitted triangle must have
/// strictly-positive twice-area, at BOTH a coarse (1e-3) and fine (1e-4)
/// deflection — proving the fix is topological, not resolution-dependent.
#[test]
fn tessellated_sphere_has_no_zero_area_pole_triangles() {
    for deflection in [1.0e-4, 1.0e-3] {
        let mesh = tessellate_sphere(deflection);

        mesh.validate(0.0).unwrap_or_else(|e| {
            panic!(
                "real OCCT sphere tessellation at deflection {deflection} must satisfy \
                 the mesh contract (NonDegenerate obligation on periodic-surface pole \
                 triangles): {e:?}"
            )
        });

        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "index count must be a multiple of 3 (deflection {deflection})"
        );
    }
}
