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
//! Task 3625 (KGQ-π) adds the topology selectors and mass properties:
//! `extract_faces`/`extract_edges` sub-shape enumeration; the per-element
//! `tri_area` / `tri_unit_normal` / `edge_unit_tangent` / `points_bbox`
//! helpers; the `canonical_edges` / `triangles_of` / `adjacent_faces` /
//! `shared_edges` topology helpers; and `mass_properties` (signed-tetrahedron
//! volume / centroid / inertia integration). Every reply reproduces OCCT's
//! exact wire format so KGQ-ρ's cross-kernel parity gate reads both kernels
//! identically. See the semantic-gap note below.
//!
//! ## Manifold face semantics after task-4262
//!
//! [`crate::kernel::ManifoldKernel::extract_faces`] now groups coplanar
//! triangles into **planar faces** via [`coalesce_coplanar_faces`], yielding
//! **6** sub-handles for a unit cube — matching OCCT's BRep face count and
//! resolving PRD Open Question §10.5
//! (`docs/prds/v0_3/kernel-geometry-queries.md`).
//!
//! **Important:** [`adjacent_faces`] and [`shared_edges`] — and the
//! `GeometryQuery::AdjacentFaces` / `GeometryQuery::SharedEdges` arms — still
//! operate on the **raw mesh triangle** index space (0..12 for a unit cube).
//! The `face_index` / `face_a` / `face_b` arguments to those queries are
//! **raw triangle indices**, NOT handles or indices into the coalesced planar
//! faces returned by `extract_faces`.  These two index spaces are disjoint;
//! see the per-arm doc notes in `kernel.rs`.
//!
//! ### `EdgeLength` is BRep-only (no `EdgesByLength` parity)
//!
//! Per the task-3623 (KGQ-ξ) capability table, `EdgeLength` is
//! `QueryCapability::BRepOnly` ("Manifold has no curves"). This crate
//! deliberately does **not** implement a mesh `EdgeLength`, so the
//! `edges_by_length` stdlib selector has **no Manifold parity** — on a mesh
//! solid it is BRep-only (eval emits `QueryNotSupportedOnRepr`). The other
//! eight topology selectors and both mass properties do have Manifold parity.
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

/// Return type of [`canonical_edges`]: the two parallel vectors describing a
/// mesh's unique undirected edges in one shared canonical ordering —
/// `index_pairs[e] = (min_idx, max_idx)` and `endpoints[e] = [p0, p1]`.
///
/// Named to keep the [`canonical_edges`] signature under clippy's
/// `type_complexity` threshold (the verify gate runs `-D warnings`).
pub(crate) type CanonicalEdges = (Vec<(u64, u64)>, Vec<[[f64; 3]; 2]>);

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
pub(crate) fn canonical_edges(verts: &[[f64; 3]], tri_indices: &[u64]) -> CanonicalEdges {
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

/// The mesh's triangles as vertex-index triplets.
///
/// Each consecutive triplet of `tri_indices` (the `to_mesh_f64` flat
/// corner-index list) becomes one `[v0, v1, v2]`. Shared by `AdjacentFaces`
/// and `SharedEdges`, which both reason over triangles as vertex-index sets
/// (their indices into this `Vec` are the "face indices" the queries report).
pub(crate) fn triangles_of(tri_indices: &[u64]) -> Vec<[u64; 3]> {
    tri_indices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect()
}

/// The three undirected edges of a triangle, as canonical `(min, max)`
/// vertex-index pairs (the same keys [`canonical_edges`] dedups on).
#[inline]
fn tri_edges(t: &[u64; 3]) -> [(u64, u64); 3] {
    [
        undirected(t[0], t[1]),
        undirected(t[1], t[2]),
        undirected(t[2], t[0]),
    ]
}

/// Triangle indices sharing at least one undirected edge with triangle
/// `face_index` — self excluded, ascending, distinct.
///
/// Returns `None` when `face_index` is out of range (the caller maps this to a
/// `QueryError`). On a closed 2-manifold every triangle has exactly 3 such
/// neighbours: each of its 3 edges is shared with exactly one other triangle,
/// and two distinct triangles cannot share two edges without coinciding.
pub(crate) fn adjacent_faces(triangles: &[[u64; 3]], face_index: usize) -> Option<Vec<usize>> {
    let target_edges = tri_edges(triangles.get(face_index)?);
    let mut neighbours: Vec<usize> = Vec::new();
    for (i, t) in triangles.iter().enumerate() {
        if i == face_index {
            continue;
        }
        if tri_edges(t).iter().any(|e| target_edges.contains(e)) {
            neighbours.push(i);
        }
    }
    // Ascending by construction (i increases monotonically; each i pushed at
    // most once → distinct). The explicit sort makes the contract robust to
    // any future reordering of the scan.
    neighbours.sort_unstable();
    Some(neighbours)
}

/// Canonical edge indices shared by triangles `face_a` and `face_b`, ascending.
///
/// Returns:
/// - `Some(empty)` when `face_a == face_b` (a face shares nothing with itself —
///   the documented design decision, checked before any range validation);
/// - `None` when either (distinct) index is out of range (the caller maps this
///   to a `QueryError`);
/// - otherwise the sorted canonical edge indices of the shared undirected
///   vertex-pairs.
///
/// `index_pairs` is [`canonical_edges`]' enumeration (its position `e` is the
/// edge index `extract_edges` / `SharedEdges` report); each shared `(min, max)`
/// pair is mapped to its index by binary search, since `index_pairs` is sorted
/// ascending. Two distinct triangles share at most one undirected edge, so the
/// result has length 0 or 1 on a closed 2-manifold — but the general
/// intersect-and-map logic holds for any triangle pair.
pub(crate) fn shared_edges(
    triangles: &[[u64; 3]],
    index_pairs: &[(u64, u64)],
    face_a: usize,
    face_b: usize,
) -> Option<Vec<usize>> {
    if face_a == face_b {
        return Some(Vec::new());
    }
    let a_edges = tri_edges(triangles.get(face_a)?);
    let b_edges = tri_edges(triangles.get(face_b)?);
    let mut out: Vec<usize> = Vec::new();
    for e in a_edges {
        if !b_edges.contains(&e) {
            continue;
        }
        // A shared edge must be in the canonical enumeration (every triangle
        // edge is); binary_search Err is therefore unreachable for a consistent
        // mesh — skipped defensively rather than panicking.
        if let Ok(idx) = index_pairs.binary_search(&e) {
            out.push(idx);
        }
    }
    out.sort_unstable();
    Some(out)
}

// ---------------------------------------------------------------------------
// Mass properties via signed-tetrahedron (divergence-theorem) integration
// ---------------------------------------------------------------------------

/// Dot product `a · b`.
#[inline]
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Meshes whose absolute signed volume falls below this are treated as
/// degenerate/empty (centroid/inertia then undefined). The unit cube has
/// `V = 1`; any genuine solid is many orders of magnitude above this floor.
const MIN_VOLUME: f64 = 1e-12;

/// Mass properties of a closed triangle mesh, from [`mass_properties`].
///
/// `centroid` is the geometric (uniform-density) centre of mass — density-free,
/// matching OCCT's density-ignoring `CenterOfMass`. `inertia` is the centroidal
/// inertia tensor **per unit density**; callers multiply by ρ for kg·m².
#[derive(Debug, Clone, Copy)]
pub(crate) struct MassProperties {
    /// Volume centroid `(x, y, z)`.
    pub centroid: [f64; 3],
    /// Centroidal inertia tensor per unit density, row-major 3×3, in OCCT's
    /// sign convention: diagonal `Iₖₖ = ∫ (x_l² + x_m²) dV` (the two axes ≠ k),
    /// off-diagonal `Iᵢⱼ = −∫ xᵢ xⱼ dV`, taken about the centroid. Symmetric.
    /// Multiply every entry by density ρ to obtain the mass-weighted tensor.
    pub inertia: [[f64; 3]; 3],
}

/// Volume, centroid, and centroidal inertia tensor of a closed triangle mesh,
/// by the signed-tetrahedron (divergence-theorem) method.
///
/// Each triangle `(a, b, c)` forms a signed tetrahedron with the origin whose
/// signed volume is `v = a · (b × c) / 6`. Summing over an outward-wound closed
/// mesh:
/// - `V = Σ v` — the enclosed volume;
/// - `Σ v · (a+b+c)/4` — the first volume moment `∫ x_i dV`; `C = moment / V`;
/// - the per-tet second-moment contribution (the canonical origin-tetrahedron
///   covariance, exact for polyhedra) gives `P_ij = ∫ x_i x_j dV` about the
///   origin.
///
/// `P` is shifted to the centroid by the parallel-axis theorem
/// (`P_ij' = P_ij − V·C_i·C_j`) and assembled into the centroidal inertia
/// tensor (per unit density) in OCCT's sign convention. Exact for polyhedra
/// (zero tessellation error), so the unit cube yields exactly `V = 1`,
/// `C = (0.5, 0.5, 0.5)`, diagonal `1/6`, off-diagonal `0`.
///
/// Returns `None` when `|V| < MIN_VOLUME` (empty/degenerate mesh) so callers
/// surface a `QueryError` rather than dividing by zero. Assumes consistent
/// outward winding (Manifold solids are so oriented); centroid and inertia are
/// in fact winding-sign-independent.
pub(crate) fn mass_properties(verts: &[[f64; 3]], tri_indices: &[u64]) -> Option<MassProperties> {
    let mut volume = 0.0;
    let mut moment = [0.0; 3]; // ∫ x_i dV = Σ v_tet · (a_i + b_i + c_i)/4
    let mut p = [[0.0f64; 3]; 3]; // ∫ x_i x_j dV about the origin
    for tri in tri_indices.chunks_exact(3) {
        let a = verts[tri[0] as usize];
        let b = verts[tri[1] as usize];
        let c = verts[tri[2] as usize];
        let v_tet = dot3(a, cross3(b, c)) / 6.0;
        volume += v_tet;
        for k in 0..3 {
            moment[k] += v_tet * (a[k] + b[k] + c[k]) / 4.0;
        }
        // Canonical second-moment integral of the origin-tetrahedron (0,a,b,c):
        // ∫ x_i x_j dV = v·[ (aᵢaⱼ+bᵢbⱼ+cᵢcⱼ)/10
        //                  + (aᵢbⱼ+aⱼbᵢ + aᵢcⱼ+aⱼcᵢ + bᵢcⱼ+bⱼcᵢ)/20 ].
        for i in 0..3 {
            for j in 0..3 {
                p[i][j] += v_tet
                    * ((a[i] * a[j] + b[i] * b[j] + c[i] * c[j]) / 10.0
                        + (a[i] * b[j]
                            + a[j] * b[i]
                            + a[i] * c[j]
                            + a[j] * c[i]
                            + b[i] * c[j]
                            + b[j] * c[i])
                            / 20.0);
            }
        }
    }
    if volume.abs() < MIN_VOLUME {
        return None;
    }
    let centroid = [moment[0] / volume, moment[1] / volume, moment[2] / volume];
    // Parallel-axis shift of the second moments to the centroid:
    // ∫ x_i' x_j' dV = ∫ x_i x_j dV − V·C_i·C_j.
    let mut pc = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            pc[i][j] = p[i][j] - volume * centroid[i] * centroid[j];
        }
    }
    // Assemble the centroidal inertia tensor (per unit density), OCCT sign
    // convention: diagonal Iₖₖ = sum of the other two ∫x_l² ; off-diagonal
    // Iᵢⱼ = −∫xᵢxⱼ. `pc` is symmetric, so the tensor is symmetric.
    let inertia = [
        [pc[1][1] + pc[2][2], -pc[0][1], -pc[0][2]],
        [-pc[1][0], pc[0][0] + pc[2][2], -pc[1][2]],
        [-pc[2][0], -pc[2][1], pc[0][0] + pc[1][1]],
    ];
    Some(MassProperties { centroid, inertia })
}

// ---------------------------------------------------------------------------
// Sub-element geometry helpers + OCCT-compatible JSON wire formatters
// ---------------------------------------------------------------------------

/// `a − b` for two 3-vectors.
#[inline]
fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Cross product `a × b`.
#[inline]
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Euclidean length `‖a‖`.
#[inline]
fn norm3(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Area of a triangle from its three corners: `½‖(v1−v0)×(v2−v0)‖`.
///
/// Exact for a flat triangle. For the unit-cube fixture each facet is a right
/// triangle with legs 1,1 → area `0.5`, and the 12 facets sum to `6.0`.
pub(crate) fn tri_area(tri: &[[f64; 3]; 3]) -> f64 {
    0.5 * norm3(cross3(sub3(tri[1], tri[0]), sub3(tri[2], tri[0])))
}

/// Unit normal of a triangle: `normalize((v1−v0)×(v2−v0))`.
///
/// Sign follows the triangle's winding order; the `FaceNormal` contract is
/// sign-agnostic (callers needing a definite orientation must resolve it
/// themselves). Returns `[0,0,0]` for a degenerate (zero-area) triangle so
/// callers never divide by zero.
pub(crate) fn tri_unit_normal(tri: &[[f64; 3]; 3]) -> [f64; 3] {
    let n = cross3(sub3(tri[1], tri[0]), sub3(tri[2], tri[0]));
    let len = norm3(n);
    if len == 0.0 {
        return [0.0, 0.0, 0.0];
    }
    [n[0] / len, n[1] / len, n[2] / len]
}

// ---------------------------------------------------------------------------
// Planar-face geometry helpers (task-4262)
// ---------------------------------------------------------------------------

/// Total area of a planar face = sum of its constituent triangle areas.
///
/// A degenerate (zero-area) triangle contributes 0.0, which is correct.
/// Used by the `SurfaceArea` query arm for a `SubShape::Face`.
pub(crate) fn face_area(tris: &[[[f64; 3]; 3]]) -> f64 {
    tris.iter().map(tri_area).sum()
}

/// Shared planar normal of a face: unit normal of the first non-degenerate
/// constituent triangle.
///
/// All triangles in a coalesced face share the same (sign-consistent) normal
/// by construction: [`coalesce_coplanar_faces`] uses the quantised unit
/// normal as part of the plane key, so a triangle wound oppositely produces
/// a **distinct** key and is grouped into a separate face rather than placed
/// in this one.  Returns `[0,0,0]` if all triangles are degenerate
/// (zero-area).  Used by the `FaceNormal` query arm for a `SubShape::Face`.
pub(crate) fn face_unit_normal(tris: &[[[f64; 3]; 3]]) -> [f64; 3] {
    for tri in tris {
        let n = tri_unit_normal(tri);
        if n != [0.0, 0.0, 0.0] {
            return n;
        }
    }
    [0.0, 0.0, 0.0]
}

/// Axis-aligned bounding box spanning all corners of all constituent triangles.
///
/// Folds min/max directly over the flattened corner iterator — avoids
/// allocating an intermediate `Vec` compared to collecting first. Used by
/// the `BoundingBox` query arm for a `SubShape::Face`.
pub(crate) fn face_points_bbox(tris: &[[[f64; 3]; 3]]) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in tris.iter().flat_map(|tri| tri.iter()) {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    (min, max)
}

/// Quantisation tolerance used by [`coalesce_coplanar_faces`] to group mesh
/// triangles by their supporting plane.  Normals and offsets are rounded to
/// the nearest multiple of this value (1e-6).
///
/// 1e-6 comfortably separates the six faces of a unit cube (offsets differ by
/// 1.0) while tolerating the f32→f64 round-off from Reify's ingest pipeline.
const PLANE_TOL: f64 = 1e-6;

/// Group mesh triangles into coplanar planar faces.
///
/// For each triangle the supporting **plane key** is
/// `(quantised(nx), quantised(ny), quantised(nz), quantised(d))` where
/// `(nx,ny,nz)` is the unit normal and `d = dot(normal, v0)` is the signed
/// offset.  Quantisation rounds each component to the nearest multiple of
/// [`PLANE_TOL`] (1e-6), so coplanar triangles collide on the same key.
/// Degenerate (zero-area) triangles — those whose cross-product length is
/// zero — are skipped because they have no well-defined plane.
///
/// Groups are accumulated in a [`std::collections::BTreeMap`] keyed on the
/// quantised `(i64, i64, i64, i64)` tuple; `BTreeMap` iterates in ascending
/// lexicographic order, giving deterministic face order across runs.
///
/// Returns one inner `Vec` per planar face; each inner Vec contains the
/// triangles (`[v0, v1, v2]` in winding order) that share that face's plane.
/// For a unit cube aligned on integer vertices this yields **6** groups of 2,
/// matching BRep face cardinality.
///
/// Used by [`crate::kernel::ManifoldKernel::extract_faces`] (step-2 of
/// task-4262) to replace the previous one-triangle-per-face enumeration.
///
/// ## Known limitations (v0.2)
///
/// **Disjoint coplanar patches are merged.** Two geometrically separate regions
/// that happen to lie on the same infinite plane (e.g., two bosses on a common
/// top face, an L-shaped prism, or a part with a coplanar step) will be grouped
/// into a **single** `SubShape::Face` handle because they share the same plane
/// key.  Connectivity is not checked.  For the unit-cube acceptance fixture this
/// is benign (each plane has exactly one connected region), but non-convex or
/// boolean-result meshes can produce under-counted, merged faces.  A robust
/// fix would add a connectivity pass (union-find over shared triangle edges
/// within each plane bucket) and is deferred to a future task.
///
/// **Boundary-straddle splitting.** The snap-to-grid quantisation does not
/// provide a true tolerance window — it gives hard cell edges.  Two genuinely
/// coplanar triangles whose normal/offset components straddle a cell boundary
/// (i.e., round to opposite sides of a [`PLANE_TOL`] grid line) will land in
/// different buckets and fail to coalesce.  Integer-vertex fixtures (e.g., the
/// unit cube) are exact and unaffected; rotated or floating-origin faces could
/// split.  A robust fix would cluster planes by tolerance against existing
/// buckets (rather than snapping to a fixed grid) and is deferred.
///
/// BTreeMap keyed by a quantised plane key `(nx, ny, nz, d)` mapping to the
/// list of triangles on that plane.  Factored out to keep the type readable.
type PlaneGroupMap = std::collections::BTreeMap<(i64, i64, i64, i64), Vec<[[f64; 3]; 3]>>;

pub(crate) fn coalesce_coplanar_faces(
    verts: &[[f64; 3]],
    tri_indices: &[u64],
) -> Vec<Vec<[[f64; 3]; 3]>> {
    let mut groups: PlaneGroupMap = std::collections::BTreeMap::new();

    let quant = |f: f64| -> i64 { (f / PLANE_TOL).round() as i64 };

    for chunk in tri_indices.chunks_exact(3) {
        let v0 = verts[chunk[0] as usize];
        let v1 = verts[chunk[1] as usize];
        let v2 = verts[chunk[2] as usize];
        let tri = [v0, v1, v2];

        let n = tri_unit_normal(&tri);
        // Skip degenerate triangles — they have no well-defined plane.
        if n == [0.0, 0.0, 0.0] {
            continue;
        }

        // Signed plane offset d = dot(n, v0).
        let d = n[0] * v0[0] + n[1] * v0[1] + n[2] * v0[2];

        let key = (quant(n[0]), quant(n[1]), quant(n[2]), quant(d));
        groups.entry(key).or_default().push(tri);
    }

    // BTreeMap iterates ascending by key → deterministic face order.
    groups.into_values().collect()
}

/// Format an xyz vector as the OCCT-compatible `{"x":_,"y":_,"z":_}` JSON
/// wire string.
///
/// Reproduced here — rather than importing `reify-eval` (a dev-dep only) — so
/// the Manifold `FaceNormal` / `EdgeTangent` / `CenterOfMass` arms emit a
/// byte-identical wire format to OCCT's (`centroid_json` /
/// `crates/reify-kernel-occt/src/lib.rs`). `reify-eval`'s `parse_xyz_value`
/// decoder and KGQ-ρ's parity gate therefore read both kernels identically.
pub(crate) fn json_xyz(v: [f64; 3]) -> String {
    format!("{{\"x\":{},\"y\":{},\"z\":{}}}", v[0], v[1], v[2])
}

/// Unit tangent of an edge: `normalize(p1 − p0)`.
///
/// Sign follows the stored endpoint order (canonical `(min_idx, max_idx)`);
/// the `EdgeTangent` contract is sign-agnostic. Returns `[0,0,0]` for a
/// degenerate (zero-length) edge so callers never divide by zero.
pub(crate) fn edge_unit_tangent(edge: &[[f64; 3]; 2]) -> [f64; 3] {
    let d = sub3(edge[1], edge[0]);
    let len = norm3(d);
    if len == 0.0 {
        return [0.0, 0.0, 0.0];
    }
    [d[0] / len, d[1] / len, d[2] / len]
}

/// Axis-aligned min/max corners over a set of points, as
/// `([min_x,min_y,min_z], [max_x,max_y,max_z])`. Used to bound a sub-shape
/// (a face's 3 points or an edge's 2). An empty slice yields
/// `([+∞;3], [−∞;3])` (callers always pass a non-empty sub-shape).
pub(crate) fn points_bbox(points: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in points {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    (min, max)
}

/// Format an axis-aligned bounding box as the OCCT-compatible
/// `{"xmin":_,"ymin":_,"zmin":_,"xmax":_,"ymax":_,"zmax":_}` JSON wire string.
///
/// Byte-identical to OCCT's `BoundingBox` arm
/// (`crates/reify-kernel-occt/src/lib.rs`) so reify-eval's bbox decoder and
/// KGQ-ρ's parity gate read both kernels identically.
pub(crate) fn json_bbox(min: [f64; 3], max: [f64; 3]) -> String {
    format!(
        "{{\"xmin\":{},\"ymin\":{},\"zmin\":{},\"xmax\":{},\"ymax\":{},\"zmax\":{}}}",
        min[0], min[1], min[2], max[0], max[1], max[2]
    )
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
/// is outside the file scope of task 3612 (KGQ-γ) — deferred per
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
