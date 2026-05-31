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
/// # Complexity
///
/// **O(|verts_a| × |verts_b|)** — quadratic in the vertex counts of both
/// meshes.  `extract_xyz` also materialises both vertex `Vec`s before the
/// nested loop begins, so peak allocation is O(|verts_a| + |verts_b|).
/// This is acceptable for the small axis-aligned test fixtures used in
/// KGQ-α; it is **not** production-ready for large meshes.  KGQ-ο
/// generalises this to vertex-to-triangle and should introduce a spatial
/// acceleration structure (BVH/kd-tree) or at minimum stream one side to
/// avoid the second `Vec` allocation.
///
/// # Exactness
///
/// For axis-aligned meshes (e.g. the `unit_cube_mesh` test fixture) the
/// closest vertex pair is always a surface vertex, so the result is exact.
/// For general meshes, the true distance may be smaller (a point interior to
/// a face on one mesh could be closer to a vertex on the other) — vertex
/// parity is generalised to vertex-to-triangle in KGQ-ο.
///
/// # Empty / degenerate meshes
///
/// An empty or degenerate mesh (zero vertices after the `n_props` guard)
/// yields `f64::INFINITY` as the minimum.  **Callers must check for this
/// sentinel and treat it as an error** — the direct caller
/// [`crate::ManifoldKernel::query`] converts `INFINITY` to a
/// `QueryError::QueryFailed` so that the eval layer can emit a diagnostic
/// rather than propagating a silent infinite length value.  Do not rely on
/// an `INFINITY` result flowing cleanly to the user.
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

/// Test whether two [`Manifold`] meshes intersect (have non-empty boolean
/// intersection).
///
/// # Implementation
///
/// Computes `a.intersection(b)` — the same CSG boolean that powers
/// `GeometryOp::Intersection` in [`crate::kernel::ManifoldKernel`]
/// (kernel.rs:135) — and returns `true` iff the result mesh has at least one
/// vertex (i.e. the intersection volume is non-empty).
///
/// The empty-mesh-for-disjoint-inputs contract is established by
/// `tessellate_of_intersection_of_disjoint_cubes_returns_empty_mesh`
/// (kernel.rs:507), which confirms that `Manifold::intersection` on two cubes
/// with a 4-unit gap returns an empty Manifold with no vertices.
///
/// # Why NOT reuse `queries::distance`
///
/// Vertex-to-vertex distance **cannot** detect interpenetration: two
/// overlapping boxes share no coincident vertices (each box retains its own
/// surface geometry), so their minimum pairwise vertex distance is always > 0.
/// A `distance ≤ 0` test would therefore wrongly report "no intersection" for
/// fully overlapping solids.  The CSG boolean test is the correct tool for
/// this query — it detects shared volume rather than surface proximity.
///
/// # Forward reference
///
/// This standalone function is wired into `ManifoldKernel::query()` and the
/// cross-kernel `#kernel(manifold)` parity gate by KGQ-ο (Phase 5).  This
/// task (KGQ-γ/3612) ships the function + unit tests only; the `query()`
/// wiring lives in `kernel.rs` which is out of this task's file scope.
///
/// # Known parity divergence (KGQ-ο concern)
///
/// The OCCT eval path (`geometry_ops.rs` `Intersects` arm) classifies
/// `d ≤ 0.0` as intersecting, **including** solids that share only a
/// coincident face (BRep min distance = 0.0, zero overlap volume).  This
/// function uses a stricter definition — **positive shared volume** via the
/// CSG boolean — so face-coincident solids produce an empty intersection mesh
/// and return `false`.  These two semantics diverge at the touching/zero-volume
/// boundary:
///
/// | Scenario | OCCT path (`d ≤ 0`) | Manifold path (CSG non-empty) |
/// |---|---|---|
/// | Clear overlap | `true` | `true` |
/// | Face-coincident (d = 0, no volume) | `true` | `false` |
/// | Gap (d > 0) | `false` | `false` |
///
/// When KGQ-ο wires this function into the cross-kernel parity gate, a
/// face-coincident test case will **fail parity**.  The Phase-5 author must
/// decide the canonical semantics before enabling the gate — likely: define
/// `intersects` as `d ≤ 0` inclusive of touching, and update the Manifold
/// side to use a distance-based predicate rather than strict CSG non-emptiness.
#[allow(dead_code)] // wired into ManifoldKernel::query() by KGQ-ο (Phase 5)
pub(crate) fn intersects(a: &Manifold, b: &Manifold) -> bool {
    !extract_xyz(&a.intersection(b)).is_empty()
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

#[cfg(test)]
mod tests {
    use super::intersects;
    use manifold3d::Manifold;

    // `unit_cube_mesh` test fixture shared with kernel.rs integration tests.
    // Gated on `test-fixtures` feature (the self-dev-dep in Cargo.toml activates
    // it for all `cargo test` builds of this crate, matching the kernel.rs pattern).
    #[cfg(feature = "test-fixtures")]
    use crate::test_fixtures::unit_cube_mesh;

    /// Build a Manifold directly from the `unit_cube_mesh` test fixture by
    /// replicating the exact f32→f64 / u32→u64 conversion that
    /// `ManifoldKernel::ingest_mesh` performs (kernel.rs:295–313).
    ///
    /// `offset` shifts the [0,1]³ unit cube by (dx, dy, dz) in each axis —
    /// the same semantics as `unit_cube_mesh([f32; 3])` but exposed as f64 for
    /// convenient literal use in test assertions.
    ///
    /// Not gated on `test-fixtures` itself — callers are already in `cfg(test)`
    /// and gate the `unit_cube_mesh` import separately.
    #[cfg(feature = "test-fixtures")]
    fn cube_manifold(offset: [f64; 3]) -> Manifold {
        let mesh = unit_cube_mesh([offset[0] as f32, offset[1] as f32, offset[2] as f32]);
        let vert_props_f64: Vec<f64> = mesh.vertices.iter().map(|&v| v as f64).collect();
        let tri_indices_u64: Vec<u64> = mesh.indices.iter().map(|&i| i as u64).collect();
        Manifold::from_mesh_f64(&vert_props_f64, 3, &tri_indices_u64)
            .expect("unit_cube_mesh fixture must produce a valid manifold")
    }

    /// Pins that two unit cubes overlapping by 0.5 in X have a non-empty
    /// boolean intersection → `intersects` returns `true`.
    ///
    /// Cubes: [0,1]³ and [0.5,1.5]×[0,1]² share the volume [0.5,1]×[0,1]²
    /// → positive-volume intersection.  This is the same overlapping pair used
    /// by the kernel.rs boolean-op tests (kernel.rs:478 fixture pair).
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn intersects_overlapping_cubes_returns_true() {
        let a = cube_manifold([0.0, 0.0, 0.0]);
        let b = cube_manifold([0.5, 0.0, 0.0]);
        assert!(
            intersects(&a, &b),
            "overlapping cubes (offset 0.5 in x) must intersect"
        );
    }

    /// Pins that two unit cubes 5 units apart in X have an empty boolean
    /// intersection → `intersects` returns `false`.
    ///
    /// Cubes: [0,1]³ and [5,6]×[0,1]² are disjoint (4 unit gap in X).
    /// This is the same disjoint pair used by the kernel.rs empty-intersection
    /// contract test (kernel.rs:507).
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn intersects_disjoint_cubes_returns_false() {
        let a = cube_manifold([0.0, 0.0, 0.0]);
        let b = cube_manifold([5.0, 0.0, 0.0]);
        assert!(
            !intersects(&a, &b),
            "disjoint cubes (offset 5.0 in x) must not intersect"
        );
    }
}
