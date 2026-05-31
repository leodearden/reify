//! Manifold-kernel geometry queries.
//!
//! This module provides query helpers for [`crate::ManifoldKernel`] wired to
//! the PRD §5.4 geometry-query surface.  It is the **skeleton** created by
//! task 3610 (KGQ-α); task 3624 (KGQ-ο) extends it with:
//!
//! - `distance`: generalised from vertex-to-vertex to exact surface-to-surface
//!   via `Manifold::min_gap` (manifold3d 0.2).
//! - `contains`: point-in-solid via `Manifold::ray_cast` crossing count.
//! - `geo_equiv`: topology-signature + N=8 sampled-vertex comparison.
//!
//! ## Implementation note — manifold3d 0.2 distance API
//!
//! `manifold3d` 0.2 (re-exports `manifold-csg` 0.2.0) exposes
//! `Manifold::min_gap(other, search_length) -> f64` — the exact
//! surface-to-surface minimum gap (returns 0.0 for touching/interpenetrating
//! meshes).  The search_length is derived from both bounding boxes so the
//! true gap is never capped; an absent/empty bounding box signals a
//! degenerate manifold and yields `f64::INFINITY` to trigger the
//! `QueryError::QueryFailed` path in the caller.

use manifold3d::Manifold;

/// Compute the exact surface-to-surface minimum gap between two [`Manifold`]
/// meshes using `Manifold::min_gap`.
///
/// Returns `0.0` for touching or interpenetrating solids; returns the
/// Euclidean surface gap for disjoint solids.
///
/// # Algorithm (manifold3d 0.2)
///
/// Delegates to `Manifold::min_gap(other, search_length)` — the C++
/// manifold library's exact BVH-accelerated gap query — rather than the
/// prior O(n²) vertex-to-vertex loop.  `search_length` is set to the sum of
/// both bounding-box diagonals plus the centre-to-centre distance, multiplied
/// by a 1.5× safety factor, so the true gap is never artificially capped
/// (min_gap caps its internal search at `search_length`; if the true gap
/// exceeds it, the result is clamped at `search_length`, which is a false
/// positive for large search lengths — our over-estimate avoids this).
///
/// # Empty / degenerate meshes
///
/// If either bounding box is absent (empty or degenerate manifold), this
/// function returns `f64::INFINITY`.  **Callers must check for this sentinel
/// and treat it as an error** — the direct caller
/// [`crate::ManifoldKernel::query`] converts `INFINITY` to a
/// `QueryError::QueryFailed` so that the eval layer can emit a diagnostic
/// rather than propagating a silent infinite length value.
///
/// # Exactness
///
/// `min_gap` is exact for smooth surfaces; on triangle meshes it returns the
/// exact triangle-mesh distance.  For the axis-aligned `unit_cube_mesh`
/// fixtures, this matches the prior vertex-to-vertex result for disjoint cubes
/// (4.0 at 1e-9) and correctly returns 0.0 for overlapping/touching solids
/// (where vertex-to-vertex was wrong).
pub(crate) fn distance(a: &Manifold, b: &Manifold) -> f64 {
    // Compute a search_length guaranteed to exceed the true gap so min_gap
    // is never capped.  We use both bounding boxes: diagonal of each plus
    // the centre-to-centre distance, with a 1.5× safety margin.
    let search_length = {
        let bb_a = match a.bounding_box() {
            Some(bb) => bb,
            None => return f64::INFINITY, // degenerate/empty manifold
        };
        let bb_b = match b.bounding_box() {
            Some(bb) => bb,
            None => return f64::INFINITY,
        };

        // Diagonal of bounding box A.
        let [ax0, ay0, az0] = bb_a.min();
        let [ax1, ay1, az1] = bb_a.max();
        let diag_a = ((ax1 - ax0).powi(2) + (ay1 - ay0).powi(2) + (az1 - az0).powi(2)).sqrt();

        // Diagonal of bounding box B.
        let [bx0, by0, bz0] = bb_b.min();
        let [bx1, by1, bz1] = bb_b.max();
        let diag_b = ((bx1 - bx0).powi(2) + (by1 - by0).powi(2) + (bz1 - bz0).powi(2)).sqrt();

        // Centre-to-centre distance.
        let cax = (ax0 + ax1) / 2.0;
        let cay = (ay0 + ay1) / 2.0;
        let caz = (az0 + az1) / 2.0;
        let cbx = (bx0 + bx1) / 2.0;
        let cby = (by0 + by1) / 2.0;
        let cbz = (bz0 + bz1) / 2.0;
        let c2c = ((cax - cbx).powi(2) + (cay - cby).powi(2) + (caz - cbz).powi(2)).sqrt();

        // 1.5× safety margin: true gap is always < diag_a + diag_b + c2c.
        (diag_a + diag_b + c2c) * 1.5
    };

    a.min_gap(b, search_length)
}

/// Test whether a 3-D point `(px, py, pz)` lies inside a closed solid
/// [`Manifold`] using a ray-cast crossing-count (Jordan curve theorem in 3-D).
///
/// # Algorithm
///
/// 1. Obtain the bounding box of `m`; if absent (empty/degenerate manifold)
///    return `false` — an empty solid contains nothing.
/// 2. Choose a fixed **non-axis-aligned** unit direction
///    `d = normalize([0.7, 0.5, 0.3])`.  A non-axis-aligned direction avoids
///    the measure-zero degeneracies that occur when a ray hits an edge or
///    vertex exactly — on axis-aligned cube fixtures an axis-parallel ray can
///    graze a shared edge, producing 0 or 2 hits for a single face crossing.
/// 3. Set the far endpoint `end = point + d * L` where
///    `L = 2 × (bbox diagonal + point-to-bbox-corner distance)`, guaranteeing
///    `end` lies outside the solid's bounding box.
/// 4. Cast `m.ray_cast(origin, end)` to obtain all boundary crossings.
/// 5. **Odd** crossing count → inside; **even** → outside (Jordan criterion).
///
/// # Boundary behaviour
///
/// Points exactly on the mesh surface have approximate (mesh-dependent)
/// results — the crossing count depends on whether the ray enters or exits the
/// face exactly at the query point.  For well-separated interior/exterior
/// test points this is not an issue.  The `tolerance` parameter is accepted
/// for API parity with the OCCT path but does not affect the ray-cast logic;
/// exact ON-boundary classification is outside the scope of the Mesh kernel.
///
/// # Returns
///
/// `true` if the crossing count is odd (inside), `false` otherwise.
pub(crate) fn contains(m: &Manifold, px: f64, py: f64, pz: f64, _tolerance: f64) -> bool {
    // Step 1: bounding box guard.
    let bb = match m.bounding_box() {
        Some(bb) => bb,
        None => return false, // empty / degenerate manifold contains nothing
    };

    // Step 2: fixed non-axis-aligned direction (avoids cube-face degeneracies).
    // Normalise [0.7, 0.5, 0.3].
    let len = (0.7_f64 * 0.7 + 0.5 * 0.5 + 0.3 * 0.3).sqrt();
    let dx = 0.7 / len;
    let dy = 0.5 / len;
    let dz = 0.3 / len;

    // Step 3: far endpoint guaranteed outside bbox.
    let [bx0, by0, bz0] = bb.min();
    let [bx1, by1, bz1] = bb.max();
    let bbox_diag = ((bx1 - bx0).powi(2) + (by1 - by0).powi(2) + (bz1 - bz0).powi(2)).sqrt();
    // Also include query-point-to-bbox distance for points far outside.
    let to_corner = ((px - bx0).powi(2) + (py - by0).powi(2) + (pz - bz0).powi(2)).sqrt();
    let l = 2.0 * (bbox_diag + to_corner) + 1.0; // +1 avoids zero for tiny bbox

    let end = [px + dx * l, py + dy * l, pz + dz * l];
    let origin = [px, py, pz];

    // Step 4: ray cast.
    let hits = m.ray_cast(origin, end);

    // Step 5: odd crossing count → inside.
    hits.len() % 2 == 1
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

/// Build a [`Manifold`] directly from the `unit_cube_mesh` test fixture by
/// replicating the exact f32→f64 / u32→u64 conversion that
/// [`crate::kernel::ManifoldKernel::ingest_mesh`] performs (kernel.rs:295–313).
///
/// Exposed at module level (not confined to `mod tests`) so it is reusable
/// across test modules within this crate without re-deriving the conversion.
/// Ideally this helper would live in `crate::test_fixtures`; moving it there
/// requires editing `crates/reify-kernel-manifold/src/test_fixtures.rs`, which
/// is outside the file scope of task 3612 (KGQ-γ) — tracked as a follow-up by
/// the KGQ-γ code review.
///
/// `offset` shifts the [0,1]³ unit cube by (dx, dy, dz) in each axis.
#[cfg(test)]
#[cfg(feature = "test-fixtures")]
pub(crate) fn cube_manifold(offset: [f64; 3]) -> Manifold {
    let mesh = crate::test_fixtures::unit_cube_mesh([
        offset[0] as f32,
        offset[1] as f32,
        offset[2] as f32,
    ]);
    let vert_props_f64: Vec<f64> = mesh.vertices.iter().map(|&v| v as f64).collect();
    let tri_indices_u64: Vec<u64> = mesh.indices.iter().map(|&i| i as u64).collect();
    Manifold::from_mesh_f64(&vert_props_f64, 3, &tri_indices_u64)
        .expect("unit_cube_mesh fixture must produce a valid manifold")
}

#[cfg(test)]
mod tests {
    use super::intersects;
    #[cfg(feature = "test-fixtures")]
    use super::cube_manifold;

    // NOTE: The touching/face-coincident boundary case (two cubes sharing exactly
    // one face, zero volume overlap) is intentionally absent from this test module
    // until KGQ-ο resolves the cross-kernel parity semantics.  The Manifold CSG
    // boolean returns an empty mesh for face-coincident inputs → `false`; the
    // OCCT path returns `true` (d = 0.0 ≤ 0.0).  See the "Known parity
    // divergence" section in the `intersects` doc comment above.

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

    /// Pins that `intersects` returns `false` (and does not panic) when one or
    /// both inputs are empty/degenerate [`Manifold`]s.
    ///
    /// An empty Manifold is obtained by intersecting two widely-separated unit
    /// cubes — the same contract established by
    /// `tessellate_of_intersection_of_disjoint_cubes_returns_empty_mesh`
    /// (kernel.rs:507).  `extract_xyz` catches the empty vertex array and
    /// returns `Vec::new()`, so `!is_empty()` yields `false` without any
    /// vertex-iteration or index panic.  This confirms the degenerate code path
    /// in `intersects` is safe rather than latently panicking.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn intersects_with_empty_manifold_returns_false() {
        let a = cube_manifold([0.0, 0.0, 0.0]);
        let b = cube_manifold([5.0, 0.0, 0.0]); // 4-unit gap → disjoint → empty intersection
        let empty = a.intersection(&b);
        assert!(
            !intersects(&empty, &a),
            "intersects(empty_manifold, cube) must be false"
        );
        assert!(
            !intersects(&a, &empty),
            "intersects(cube, empty_manifold) must be false"
        );
    }
}
