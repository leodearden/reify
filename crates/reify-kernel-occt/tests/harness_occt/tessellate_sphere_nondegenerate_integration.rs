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
/// `validate(0.0)` must be Ok (which transitively requires every emitted
/// triangle to have strictly-positive twice-area — see
/// `Mesh::check_contract`'s `NonDegenerate` obligation), and the mesh must
/// retain a tight, near-exact expected triangle count (see the band check
/// below), at BOTH a coarse (1e-3) and fine (1e-4) deflection — proving the
/// fix is topological, not resolution-dependent.
#[test]
fn tessellated_sphere_has_no_zero_area_pole_triangles() {
    // Empirically observed against this workspace's pinned real OCCT build:
    // an 8mm-radius sphere tessellates to 826 triangles at deflection 1e-4
    // and 304 at deflection 1e-3 (both counts already net of the two
    // genuinely-degenerate pole-seam triangles this task's gate drops). The
    // per-deflection minimum below sits ~15% under that observed count.
    for (deflection, min_expected_tris) in [(1.0e-4, 700_usize), (1.0e-3, 260_usize)] {
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

        // Band sanity check — closes a vacuous-pass gap: both assertions
        // above would still hold for an empty or heavily-decimated index
        // buffer (e.g. an FMA/arithmetic divergence that misclassifies real
        // slivers as degenerate, or an off-by-one in the base-index
        // readback dropping valid triangles), so neither actually proves
        // the producer emitted a real mesh rather than over-dropping
        // valid (non-pole) triangles. A loose `num_tris > 100` floor closes
        // the empty/heavily-decimated extreme but not the moderate range —
        // e.g. a regression silently dropping 20% of valid triangles would
        // still clear 100 and pass. `min_expected_tris` is a tight,
        // deflection-specific floor (~15% under the measured count) that
        // does catch that: the producer's degeneracy gate should only ever
        // drop the two genuinely-degenerate pole-seam triangles, so the
        // real count should sit almost exactly at the measured value, not
        // just "in the hundreds".
        //
        // Deliberately a tight *floor*, not an exact pin or a two-sided
        // band around it: OCCT's adaptive triangulation could plausibly
        // emit a handful more/fewer triangles across OCCT versions or
        // platforms for the same deflection, and this gate can only ever
        // drop triangles (never add them), so only under-shooting the
        // expected count is a signal this gate is misbehaving. Pinning an
        // exact expected drop count of 2 would need OCCT's raw
        // (pre-drop) triangle count, which is not observable from Rust
        // without a debug hook through the FFI surface (out of this task's
        // locked-file scope).
        let num_tris = mesh.indices.len() / 3;
        assert!(
            num_tris >= min_expected_tris,
            "expected at least {min_expected_tris} triangles for an 8mm-radius sphere at \
             deflection {deflection} (measured baseline: 826 at 1e-4, 304 at 1e-3); got \
             {num_tris}, which is consistent with an over-aggressive degeneracy gate \
             dropping valid (non-pole) triangles rather than just the two \
             genuinely-degenerate pole-seam ones"
        );
    }
}
