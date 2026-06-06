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

use std::collections::HashMap;

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
// Bit-exact vertex weld (mirrors manifold_from_reify_mesh in reify-kernel-manifold)
// ---------------------------------------------------------------------------

/// Map every distinct (x, y, z) float triple to a canonical index.
///
/// Keying: `(c + 0.0f32).to_bits()` normalises −0.0 → +0.0 so origin-plane
/// corners weld, exactly as in `manifold_from_reify_mesh`.
///
/// Returns `(canonical_vertices, old_to_canonical)` where
/// `old_to_canonical[old_idx] == canonical_idx`.
fn weld_vertices(vertices: &[f32]) -> (Vec<[f32; 3]>, Vec<u32>) {
    assert_eq!(vertices.len() % 3, 0, "vertices must be flat xyz triples");
    let n = vertices.len() / 3;
    let mut key_to_canon: HashMap<(u32, u32, u32), u32> = HashMap::new();
    let mut canon_verts: Vec<[f32; 3]> = Vec::new();
    let mut remap = Vec::with_capacity(n);
    for i in 0..n {
        let x = vertices[i * 3];
        let y = vertices[i * 3 + 1];
        let z = vertices[i * 3 + 2];
        let key = (
            (x + 0.0_f32).to_bits(),
            (y + 0.0_f32).to_bits(),
            (z + 0.0_f32).to_bits(),
        );
        let next_idx = canon_verts.len() as u32;
        let canon_idx = *key_to_canon.entry(key).or_insert_with(|| {
            canon_verts.push([x, y, z]);
            next_idx
        });
        remap.push(canon_idx);
    }
    (canon_verts, remap)
}

// ---------------------------------------------------------------------------
// Geometric helper shared by both tests
// ---------------------------------------------------------------------------

/// Compute the geometric normal of triangle (pa, pb, pc) from the emitted
/// winding order: AB × AC.  All inputs and the result are in f64.
fn tri_winding_normal(pa: [f64; 3], pb: [f64; 3], pc: [f64; 3]) -> [f64; 3] {
    let ab = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    let ac = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
    [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ]
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
/// winding) points AWAY from the box centroid — i.e. is outward-wound.
///
/// RED on base (before the winding fix): REVERSED faces are inward-wound, so
/// a shared edge between a FORWARD and a REVERSED face is traversed in the
/// same direction by both bordering triangles → directed edge (u,v) count == 2
/// with (v,u) count == 0 → invariant violated.
#[test]
fn tessellated_box_welded_winding_is_closed_orientable_manifold() {
    let mesh = tessellate_box(10.0, 20.0, 30.0, 0.1);

    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "index count must be a multiple of 3"
    );

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

    let (canon_verts, remap) = weld_vertices(&mesh.vertices);

    // Remap the per-face (unwelded) triangle indices to canonical indices.
    let welded: Vec<u32> = mesh.indices.iter().map(|&i| remap[i as usize]).collect();

    // Build directed-edge occurrence count.
    let mut edge_count: HashMap<(u32, u32), i32> = HashMap::new();
    let num_tris = welded.len() / 3;
    for t in 0..num_tris {
        let a = welded[t * 3];
        let b = welded[t * 3 + 1];
        let c = welded[t * 3 + 2];
        *edge_count.entry((a, b)).or_insert(0) += 1;
        *edge_count.entry((b, c)).or_insert(0) += 1;
        *edge_count.entry((c, a)).or_insert(0) += 1;
    }

    // Closed-orientable-manifold winding invariant.
    for (&(u, v), &count) in &edge_count {
        assert_eq!(
            count, 1,
            "directed edge ({u},{v}) appears {count} times (expected 1); \
             a count > 1 means mixed winding on a shared edge"
        );
        let rev = *edge_count.get(&(v, u)).unwrap_or(&0);
        assert_eq!(
            rev, 1,
            "reverse edge ({v},{u}) of directed edge ({u},{v}) appears {rev} times \
             (expected 1); a count == 0 means an open boundary or missing face"
        );
    }

    // Outward-orientation check.
    // Box centroid from the AABB (min+max per axis / 2).  Using the analytic
    // bounding-box center rather than the vertex-cloud average makes this
    // robust to non-uniform vertex distributions — e.g. if a tighter
    // tolerance caused OCCT to add interior tessellation nodes on any face,
    // the vertex mean would shift toward the denser face whereas the AABB
    // center is unaffected.
    let box_centroid = {
        let mut min = [f64::MAX; 3];
        let mut max = [f64::MIN; 3];
        for v in &canon_verts {
            for k in 0..3 {
                let coord = v[k] as f64;
                if coord < min[k] {
                    min[k] = coord;
                }
                if coord > max[k] {
                    max[k] = coord;
                }
            }
        }
        [
            (min[0] + max[0]) / 2.0,
            (min[1] + max[1]) / 2.0,
            (min[2] + max[2]) / 2.0,
        ]
    };

    for t in 0..num_tris {
        let a = welded[t * 3] as usize;
        let b = welded[t * 3 + 1] as usize;
        let c = welded[t * 3 + 2] as usize;
        let pa = canon_verts[a];
        let pb = canon_verts[b];
        let pc = canon_verts[c];

        // Geometric normal from the emitted winding order (AB × AC).
        let pa_f64 = [pa[0] as f64, pa[1] as f64, pa[2] as f64];
        let pb_f64 = [pb[0] as f64, pb[1] as f64, pb[2] as f64];
        let pc_f64 = [pc[0] as f64, pc[1] as f64, pc[2] as f64];
        let normal = tri_winding_normal(pa_f64, pb_f64, pc_f64);

        // Outward direction: triangle centroid → box centroid reversed.
        let tri_centroid = [
            (pa[0] as f64 + pb[0] as f64 + pc[0] as f64) / 3.0,
            (pa[1] as f64 + pb[1] as f64 + pc[1] as f64) / 3.0,
            (pa[2] as f64 + pb[2] as f64 + pc[2] as f64) / 3.0,
        ];
        let outward = [
            tri_centroid[0] - box_centroid[0],
            tri_centroid[1] - box_centroid[1],
            tri_centroid[2] - box_centroid[2],
        ];

        let dot = normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2];

        assert!(
            dot > 0.0,
            "triangle {t} (verts {a},{b},{c}): geometric normal from emitted winding \
             points inward (dot = {dot:.6}); all triangles must be outward-wound"
        );
    }
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

    let supplied = mesh
        .normals
        .as_ref()
        .expect("tessellate should emit per-vertex normals for a box");

    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "index count must be a multiple of 3"
    );
    assert_eq!(
        supplied.len(),
        mesh.vertices.len(),
        "normals array must have same length as vertices array"
    );

    let num_tris = mesh.indices.len() / 3;
    for t in 0..num_tris {
        let i0 = mesh.indices[t * 3] as usize;
        let i1 = mesh.indices[t * 3 + 1] as usize;
        let i2 = mesh.indices[t * 3 + 2] as usize;

        // Vertex positions.
        let pa = [
            mesh.vertices[i0 * 3] as f64,
            mesh.vertices[i0 * 3 + 1] as f64,
            mesh.vertices[i0 * 3 + 2] as f64,
        ];
        let pb = [
            mesh.vertices[i1 * 3] as f64,
            mesh.vertices[i1 * 3 + 1] as f64,
            mesh.vertices[i1 * 3 + 2] as f64,
        ];
        let pc = [
            mesh.vertices[i2 * 3] as f64,
            mesh.vertices[i2 * 3 + 1] as f64,
            mesh.vertices[i2 * 3 + 2] as f64,
        ];

        // Geometric normal from the emitted winding (AB × AC).
        let winding_normal = tri_winding_normal(pa, pb, pc);

        // Average of the three supplied per-vertex normals.
        let avg_supplied = [
            (supplied[i0 * 3] as f64 + supplied[i1 * 3] as f64 + supplied[i2 * 3] as f64)
                / 3.0,
            (supplied[i0 * 3 + 1] as f64
                + supplied[i1 * 3 + 1] as f64
                + supplied[i2 * 3 + 1] as f64)
                / 3.0,
            (supplied[i0 * 3 + 2] as f64
                + supplied[i1 * 3 + 2] as f64
                + supplied[i2 * 3 + 2] as f64)
                / 3.0,
        ];

        let dot = winding_normal[0] * avg_supplied[0]
            + winding_normal[1] * avg_supplied[1]
            + winding_normal[2] * avg_supplied[2];

        assert!(
            dot > 0.0,
            "triangle {t}: supplied normals (avg [{:.4},{:.4},{:.4}]) disagree with \
             the geometric winding normal (dot = {dot:.6}); \
             supplied normals must agree with the outward-wound triangles",
            avg_supplied[0],
            avg_supplied[1],
            avg_supplied[2],
        );
    }
}
