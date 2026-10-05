//! Integration tests verifying that `tessellate_shape` emits consistently
//! outward-wound triangles for every face of a real OCCT solid, including
//! faces whose orientation flag is `TopAbs_REVERSED`.
//!
//! Background: OCCT `Poly_Triangulation` triangles are wound in the face's
//! NATURAL (FORWARD-surface) sense.  A face that is `TopAbs_REVERSED` in the
//! solid is emitted with INWARD winding unless `tessellate_shape` consults
//! `face.Orientation()` and swaps the index order.  After a bit-exact vertex
//! weld, the shared edge between a FORWARD and a REVERSED face would be
//! traversed in the SAME direction by both bordering triangles, violating the
//! closed-orientable-manifold condition that Manifold::from_mesh_f64 enforces.
//!
//! These tests exercise a REAL OCCT-tessellated box (not a synthetic mesh) to
//! catch exactly the gap that masked this bug before task-4336.

#![cfg(has_occt)]

use crate::common;
use reify_ir::{GeometryOp, Value};
use reify_kernel_occt::OcctKernel;

// ---------------------------------------------------------------------------
// Shared helper — build + tessellate a box solid
// ---------------------------------------------------------------------------

fn tessellate_box(width_mm: f64, height_mm: f64, depth_mm: f64, tol: f64) -> reify_ir::Mesh {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(width_mm * 1e-3),
            height: Value::Real(height_mm * 1e-3),
            depth: Value::Real(depth_mm * 1e-3),
        })
        .expect("Box creation should succeed");
    kernel
        .tessellate(h.id, tol)
        .expect("tessellate should succeed")
}

// ---------------------------------------------------------------------------
// Test A — closed-orientable-manifold winding invariant (step-1)
// ---------------------------------------------------------------------------

/// Tessellate a real OCCT box, bit-exact-weld the per-face vertices, and
/// assert that the welded mesh satisfies the closed-orientable-manifold
/// winding invariant:
///
///   For every directed edge (u, v): count == 1  AND  count(v, u) == 1.
///
/// Also asserts that every triangle's geometric normal (from the emitted
/// winding) points AWAY from the box's AABB centre — i.e. is outward-wound.
///
/// RED on base (before the winding fix): REVERSED faces are inward-wound, so
/// a shared edge between a FORWARD and a REVERSED face is traversed in the
/// same direction by both bordering triangles → directed edge (u,v) count == 2
/// with (v,u) count == 0 → invariant violated.
#[test]
fn tessellated_box_welded_winding_is_closed_orientable_manifold() {
    let mesh = tessellate_box(10.0, 20.0, 30.0, 0.1);

    // Validity note: `BRepPrimAPI_MakeBox` builds an oriented closed shell
    // that assigns some faces FORWARD and others REVERSED relative to the
    // solid's outward normal convention — this is a guaranteed invariant of
    // OCCT's oriented-solid construction.  Empirically confirmed: this test
    // was RED on the unfixed codebase (≥1 REVERSED face was inward-wound →
    // a shared edge's directed key appeared twice with its reverse absent).
    // The test is therefore a valid regression guard only so long as OCCT
    // continues to emit reversed faces for a box solid, which is guaranteed
    // by the oriented-topology construction (`BRepPrimAPI_MakeBox` always
    // produces a shell with mixed-orientation faces).

    // Closed-orientable-manifold winding invariant, checked on the
    // position-welded quotient topology via `Mesh::validate`'s Closed +
    // ConsistentWinding obligations (INV-GEO-1) — the same directed-edge
    // invariant this test used to check by hand.  `tol=0.0`: this asserts
    // real OCCT tessellation output, not a distance-tolerant approximation.
    let _validated = mesh.validate(0.0).expect(
        "real OCCT box tessellation must satisfy the mesh contract after internal welding",
    );

    common::assert_outward_wound(&mesh, "10x20x30 mm box");
}

// ---------------------------------------------------------------------------
// Test B — supplied normals agree with the emitted winding (step-3)
// ---------------------------------------------------------------------------

/// After the winding fix (step-2), emitted triangles are outward-wound.
/// But `tessellate_shape` also emits per-vertex normals; without the
/// corresponding normal flip for REVERSED faces, those normals still point
/// inward while the winding points outward — breaking GUI shading and export.
///
/// This test asserts that the average supplied normal for each triangle agrees
/// (dot > 0) with the geometric normal derived from the emitted winding order.
///
/// RED after step-2 / before step-4 (winding fixed, normals not yet flipped).
/// GREEN after step-4 (both winding and normals consistently outward).
#[test]
fn tessellated_box_supplied_normals_agree_with_winding() {
    let mesh = tessellate_box(10.0, 20.0, 30.0, 0.1);
    common::assert_supplied_normals_agree_with_winding(&mesh, "10x20x30 mm box");
}
