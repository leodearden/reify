//! Manifold-kernel geometry queries.
//!
//! This module provides query helpers for [`crate::ManifoldKernel`] wired to
//! the PRD §5.4 geometry-query surface.  It is the **skeleton** created by
//! task 3610 (KGQ-α); later tasks extend it:
//!
//! - KGQ-ο/π (depends on KGQ-α/β/γ/δ): generalise mesh-to-mesh distance to
//!   vertex-to-triangle and add the cross-kernel #kernel(manifold) parity gate.
//!
//! ## Implementation note — manifold3d 0.1 distance API
//!
//! `manifold3d` 0.1 (`zmerlynn/manifold-csg` fork) does **not** expose a
//! built-in distance / nearest-point primitive.  `GeometryQuery::Distance`
//! carries two handles only (no query point), so point-to-mesh routing via
//! this kernel is deferred to KGQ-ο.  The current implementation computes the
//! minimum pairwise vertex-to-vertex Euclidean distance by iterating the
//! `to_mesh_f64()` vertex arrays — exact for axis-aligned mesh fixtures.

use manifold3d::Manifold;

/// Compute the minimum vertex-to-vertex Euclidean distance between two
/// [`Manifold`] meshes.
///
/// Iterates the `xyz` triplets produced by `to_mesh_f64()` for each manifold
/// and returns the global minimum pairwise distance.
///
/// # Exactness
///
/// For axis-aligned meshes (e.g. the `unit_cube_mesh` test fixture) the
/// closest vertex pair is always a surface vertex, so the result is exact.
/// For general meshes, the true distance may be smaller (a point interior to
/// a face on one mesh could be closer to a vertex on the other) — vertex
/// parity is generalised to vertex-to-triangle in KGQ-ο.
///
/// # Panics
///
/// Does not panic.  An empty or degenerate mesh (zero vertices after the
/// `n_props` guard) yields `f64::INFINITY` as the minimum, which the caller
/// can detect as a sentinel.
pub(crate) fn distance(a: &Manifold, b: &Manifold) -> f64 {
    let verts_a = extract_xyz(a);
    let verts_b = extract_xyz(b);

    let mut min_dist_sq = f64::INFINITY;
    for va in &verts_a {
        for vb in &verts_b {
            let dx = va[0] - vb[0];
            let dy = va[1] - vb[1];
            let dz = va[2] - vb[2];
            let dist_sq = dx * dx + dy * dy + dz * dz;
            if dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
            }
        }
    }

    min_dist_sq.sqrt()
}

/// Extract `xyz` vertex triplets from a [`Manifold`]'s mesh.
///
/// Mirrors the `n_props` guard in [`crate::kernel::ManifoldKernel::tessellate`]
/// (kernel.rs lines 178–215): `to_mesh_f64()` returns an interleaved
/// `n_props`-wide vertex-property array; the first three columns are always
/// `x`, `y`, `z`.  Returns an empty `Vec` when `n_props < 3` or the mesh has
/// no vertices.
fn extract_xyz(m: &Manifold) -> Vec<[f64; 3]> {
    let (vert_props, n_props, _tri_indices) = m.to_mesh_f64();
    if n_props < 3 || vert_props.is_empty() {
        return Vec::new();
    }
    let n_verts = vert_props.len() / n_props;
    let mut out = Vec::with_capacity(n_verts);
    for v in 0..n_verts {
        let base = v * n_props;
        out.push([vert_props[base], vert_props[base + 1], vert_props[base + 2]]);
    }
    out
}
