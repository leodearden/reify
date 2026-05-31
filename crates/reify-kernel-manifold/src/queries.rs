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
use reify_ir::DEFAULT_GEO_EQUIV_SAMPLE_COUNT;

// ---------------------------------------------------------------------------
// Bounding-box geometry helpers (private)
// ---------------------------------------------------------------------------

/// Returns the Euclidean diagonal length of a bounding box (min-corner to
/// max-corner distance).
///
/// Shared by [`distance`] (both bounding boxes and centre-to-centre gap) and
/// [`contains`] (bbox diagonal + query-point-to-corner distance).  Extracting
/// this avoids copy-paste sign/axis mistakes in each caller.
fn bbox_diagonal(min: [f64; 3], max: [f64; 3]) -> f64 {
    let dx = max[0] - min[0];
    let dy = max[1] - min[1];
    let dz = max[2] - min[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Returns the centre point of a bounding box.
///
/// Used by [`distance`] to compute the centre-to-centre separation between
/// the two input bounding boxes.
fn bbox_center(min: [f64; 3], max: [f64; 3]) -> [f64; 3] {
    [
        (min[0] + max[0]) / 2.0,
        (min[1] + max[1]) / 2.0,
        (min[2] + max[2]) / 2.0,
    ]
}

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

        let a_min = bb_a.min();
        let a_max = bb_a.max();
        let b_min = bb_b.min();
        let b_max = bb_b.max();
        let diag_a = bbox_diagonal(a_min, a_max);
        let diag_b = bbox_diagonal(b_min, b_max);
        let ca = bbox_center(a_min, a_max);
        let cb = bbox_center(b_min, b_max);
        // Centre-to-centre: reuse bbox_diagonal as a Euclidean point-distance
        // (squaring removes the sign, so parameter order is immaterial).
        let c2c = bbox_diagonal(ca, cb);

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
    let bb_min = bb.min();
    let bbox_diag = bbox_diagonal(bb_min, bb.max());
    // Also include query-point-to-bbox distance for points far outside;
    // bbox_diagonal used here as a Euclidean point-distance.
    let to_corner = bbox_diagonal(bb_min, [px, py, pz]);
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

/// Test whether two [`Manifold`] solids are geometrically equivalent within
/// `tolerance` using a topology-signature comparison followed by sampled-vertex
/// proximity checking.
///
/// # STRICT-VARIANT NOTE
///
/// This is the **approximate** geo_equiv implementation (topology counts + N=8
/// sampled vertices).  A future `geo_equiv_strict` using symmetric Hausdorff
/// distance is deferred to v0.4 per PRD §5.1 + Open Question §10.  When that
/// follow-up lands, the body of this function should be superseded, not
/// extended.
///
/// # Algorithm
///
/// 1. **Topology signature**: `num_vert`, `num_tri`, `num_edge`, and `genus`
///    must all match.  A mismatch returns `false` immediately — these counts
///    are a cheap structural fingerprint that detects fundamentally different
///    meshes without touching the vertex data.
///
/// 2. **Sampled vertices**: Extract both vertex lists via [`extract_xyz`], sort
///    each lexicographically (compare x, then y, then z with `total_cmp`) to
///    make the comparison **vertex-ordering-independent** across two
///    independently-built manifolds.  Then for `N = DEFAULT_GEO_EQUIV_SAMPLE_COUNT`
///    evenly-spaced indices (clamped to the actual vertex count), require
///    `‖va − vb‖ < tolerance` on the sorted lists.  Any sample that exceeds the
///    tolerance returns `false`.
///
/// # Ordering independence
///
/// Two independently-constructed identical manifolds may have different
/// internal vertex orderings (e.g. different winding / reindex outcomes from
/// `Manifold::from_mesh_f64`).  Lexicographic sorting before sampling ensures
/// that the comparison is order-independent for vertices in general position.
/// Duplicate vertices at the same position will cluster together in both sorted
/// lists — they compare equal, satisfying the tolerance check.
///
/// # Approximation gap (known limitation)
///
/// The N=8 sampled-vertex check cannot detect per-vertex differences at
/// positions that fall between sample indices.  Two meshes differing only at
/// unsampled vertices would compare equal here.  This is an intentional
/// approximation; the exact symmetric-Hausdorff variant (`geo_equiv_strict`,
/// deferred to v0.4 per PRD §5.1 / Open Question §10) would close this gap.
/// For the axis-aligned cube fixtures in the integration tests the 8-sample
/// stride always covers the critical positions; a regression that widened the
/// stride would be visible in the `geo_equiv_*` integration tests.
pub(crate) fn geo_equiv(a: &Manifold, b: &Manifold, tolerance: f64) -> bool {
    // Step 1: Topology signature — cheap early-out.
    if a.num_vert() != b.num_vert() {
        return false;
    }
    if a.num_tri() != b.num_tri() {
        return false;
    }
    if a.num_edge() != b.num_edge() {
        return false;
    }
    if a.genus() != b.genus() {
        return false;
    }

    // Step 2: Sampled vertices — ordering-independent.
    let mut verts_a = extract_xyz(a);
    let mut verts_b = extract_xyz(b);

    if verts_a.len() != verts_b.len() {
        // Defensive: topology counts matched but extract_xyz lengths differ
        // (e.g. degenerate mesh with n_props < 3).
        return false;
    }

    if verts_a.is_empty() {
        // Both empty — topology matched (all zeros), no vertex samples needed.
        return true;
    }

    // Lexicographic sort for vertex-ordering independence.
    let lex_cmp = |p: &[f64; 3], q: &[f64; 3]| -> std::cmp::Ordering {
        p[0].total_cmp(&q[0])
            .then(p[1].total_cmp(&q[1]))
            .then(p[2].total_cmp(&q[2]))
    };
    verts_a.sort_unstable_by(lex_cmp);
    verts_b.sort_unstable_by(lex_cmp);

    // N evenly-spaced sample indices (DEFAULT_GEO_EQUIV_SAMPLE_COUNT = 8).
    let n = DEFAULT_GEO_EQUIV_SAMPLE_COUNT.min(verts_a.len());
    let step = verts_a.len() / n; // ≥ 1 since n ≤ verts_a.len()

    for i in 0..n {
        let idx = i * step;
        let va = verts_a[idx];
        let vb = verts_b[idx];
        let dx = va[0] - vb[0];
        let dy = va[1] - vb[1];
        let dz = va[2] - vb[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist >= tolerance {
            return false;
        }
    }

    true
}

/// Extract the xyz vertex triplets **and** the flat triangle-corner index
/// list from a [`Manifold`]'s mesh in a single `to_mesh_f64` call.
///
/// Returns `(vertices, tri_indices)` where `vertices[i]` is the xyz of mesh
/// vertex `i`, and `tri_indices` is the flat `3·T`-length array whose every
/// consecutive triplet names one triangle's three corner vertices (the
/// `to_mesh_f64` contract).  Returns `(empty, empty)` when the mesh is empty
/// or degenerate (`n_props < 3`) — the same guard [`extract_xyz`] and
/// `ManifoldKernel::tessellate` apply.
///
/// This is the single extraction entry-point shared by the topology
/// selectors (`extract_faces` / `extract_edges`, `canonical_edges`,
/// `triangles_of`) and the mass-property integrator (`mass_properties`),
/// so every consumer indexes the same vertex/triangle data identically.
pub(crate) fn mesh_geometry(m: &Manifold) -> (Vec<[f64; 3]>, Vec<u64>) {
    let (vert_props, n_props, tri_indices) = m.to_mesh_f64();
    if n_props < 3 || vert_props.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let n_verts = vert_props.len() / n_props;
    let mut verts = Vec::with_capacity(n_verts);
    for v in 0..n_verts {
        let base = v * n_props;
        verts.push([vert_props[base], vert_props[base + 1], vert_props[base + 2]]);
    }
    (verts, tri_indices)
}

/// Order an unordered vertex-index pair into a canonical undirected key
/// `(min, max)`. Shared by [`canonical_edges`] (deduping triangle edges) and
/// `SharedEdges` (matching a shared vertex-pair to a canonical edge index).
#[inline]
fn undirected(a: u64, b: u64) -> (u64, u64) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Canonical undirected-edge enumeration for a triangle mesh.
///
/// Returns `(index_pairs, endpoints)` — the unique undirected edges of the
/// mesh — where for each edge `e`:
/// - `index_pairs[e] = (min_vertex_index, max_vertex_index)` is the canonical
///   undirected vertex-index pair, and
/// - `endpoints[e] = [p0, p1]` is the xyz of those two vertices.
///
/// Both vectors share one ordering: **ascending by `(min_idx, max_idx)`**
/// (a `BTreeSet` supplies dedup + sort in a single pass). This is THE
/// canonical edge index space of the kernel:
/// - `extract_edges` stores `endpoints` in this order, so edge sub-handle
///   `e` corresponds to `index_pairs[e]`;
/// - `SharedEdges` maps a shared vertex-pair back to its position `e` in
///   `index_pairs`.
///
/// Deriving both queries from this single helper guarantees their edge-index
/// spaces never drift (mirrors OCCT's single global-edge enumeration).
pub(crate) fn canonical_edges(
    verts: &[[f64; 3]],
    tri_indices: &[u64],
) -> (Vec<(u64, u64)>, Vec<[[f64; 3]; 2]>) {
    let mut set: std::collections::BTreeSet<(u64, u64)> = std::collections::BTreeSet::new();
    for tri in tri_indices.chunks_exact(3) {
        set.insert(undirected(tri[0], tri[1]));
        set.insert(undirected(tri[1], tri[2]));
        set.insert(undirected(tri[2], tri[0]));
    }
    // BTreeSet iterates ascending by (min,max) — the canonical order.
    let index_pairs: Vec<(u64, u64)> = set.into_iter().collect();
    let endpoints: Vec<[[f64; 3]; 2]> = index_pairs
        .iter()
        .map(|&(i, j)| [verts[i as usize], verts[j as usize]])
        .collect();
    (index_pairs, endpoints)
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
