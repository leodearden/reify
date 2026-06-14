//! Laplacian quick-pass smoother (PRD task #6).
//!
//! Implements the cheap fast path for trivially small parameter changes,
//! per PRD `docs/prds/v0_3/mesh-morphing.md` §"Laplacian quick-pass":
//! surface nodes are pinned to their projected positions and interior nodes
//! are iteratively averaged with their topological neighbours.
//!
//! Selection logic (Laplacian vs. elasticity morph) lives in PRD task #10's
//! engine integration; this module delivers only the smoother kernel.

use std::collections::BTreeSet;

use reify_ir::{ElementOrderTag, VolumeMesh};

// ── LaplacianFailure ──────────────────────────────────────────────────────────

/// Failure modes from [`laplacian_smooth`].
///
/// Mirrors the shape of [`crate::ProjectionFailure`] (structured variants
/// carrying the offending input) so engine wiring (PRD task #10) sees uniform
/// `Result<…, *Failure>` returns from `compute_dirichlet_bcs` and
/// `laplacian_smooth`.
#[derive(Debug, Clone, PartialEq)]
pub enum LaplacianFailure {
    /// A node index in `prescribed_positions` is out of range for
    /// `old_mesh.vertices` (i.e. `node_idx * 3 + 2 >= old_mesh.vertices.len()`).
    InvalidNodeIndex(u32),
    /// `old_mesh.element_order` is not [`ElementOrderTag::P1`].
    ///
    /// P2 adjacency rules (corner-corner + corner-midnode edges) are out of
    /// scope for the quick-pass; engine wiring (PRD task #10) gates the
    /// Laplacian path on `element_order == P1` and falls through to the
    /// elasticity morph otherwise.
    UnsupportedElementOrder(ElementOrderTag),
}

// ── laplacian_smooth ──────────────────────────────────────────────────────────

/// Constrained Laplacian smoother — boundary nodes pinned to
/// `prescribed_positions`, interior nodes iteratively averaged with their
/// topological neighbours (Jacobi iteration).
///
/// ## Parameters
///
/// - `old_mesh` — the current tetrahedral mesh. `old_mesh.tet_indices` must
///   reference only valid vertex indices (`< old_mesh.vertices.len() / 3`)
///   and be a length-multiple of 4 (P1 tet stride). The same mesh-validity
///   precondition `boundary.rs::compute_dirichlet_bcs` delegates to its
///   caller; out-of-range entries in `tet_indices` are silently skipped
///   when building the adjacency graph rather than reported as failures.
/// - `prescribed_positions` — `(node_index, new_position)` pairs identifying
///   "boundary" nodes pinned to their projected targets. The natural producer
///   is [`crate::compute_dirichlet_bcs`] (PRD task #5), which emits each node
///   exactly once via its `BTreeMap`-backed `BoundaryAssociation`. If a node
///   index appears more than once in the slice, the last entry wins (the
///   boundary mask and pinned position are overwritten on subsequent
///   occurrences); duplicates are not reported as a failure.
/// - `iterations` — number of Jacobi smoothing passes. Engine wiring (PRD
///   task #10) reads [`crate::MorphOptions::laplacian_iterations`] and passes
///   it in (5–10 typical, default 8).
///
/// ## Element-order restriction
///
/// Only [`ElementOrderTag::P1`] is supported. P2 tets carry corner + edge-
/// midnode adjacency rules (corner-corner + corner-midnode edges; midnode-
/// midnode are *not* topologically adjacent), so naively treating "all 10
/// nodes in a tet are pairwise neighbours" would smear interior smoothing
/// across non-adjacent nodes. PRD task #6 doesn't specify P2 support; engine
/// integration (PRD task #10) gates on `element_order == P1` for this fast
/// path and falls through to the elasticity morph otherwise. P2 input
/// returns [`LaplacianFailure::UnsupportedElementOrder`].
///
/// ## Output normals
///
/// The returned mesh always has `normals: None`, regardless of whether the
/// input mesh carried per-vertex normals. Vertex positions change during
/// smoothing, so any pre-existing normals would be geometrically stale after
/// the pass. Silently forwarding them would cause downstream consumers (engine
/// integration, PRD task #10) to render with incorrect lighting rather than
/// triggering an obvious failure. Dropping them instead fails closed: a
/// consumer that needs surface normals must recompute them after morphing.
///
/// Pinned by
/// `laplacian_smooth_drops_normals_on_output_even_when_input_has_some_normals`.
///
/// ## Failure modes
///
/// See [`LaplacianFailure`].
// G-allow: mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #3429 (Mesh-morph engine wiring via ComputeNode at VolumeMesh realization dispatch, engine_build.rs)
pub fn laplacian_smooth(
    old_mesh: &VolumeMesh,
    prescribed_positions: &[(u32, [f64; 3])],
    iterations: u32,
) -> Result<VolumeMesh, LaplacianFailure> {
    if old_mesh.element_order != ElementOrderTag::P1 {
        return Err(LaplacianFailure::UnsupportedElementOrder(
            old_mesh.element_order,
        ));
    }

    // Validate every prescribed_positions index up front (before any
    // allocation) — delegates to VolumeMesh::vertex for the overflow-safe
    // bounds check.
    for (node_idx, _) in prescribed_positions {
        old_mesh
            .vertex(*node_idx)
            .ok_or(LaplacianFailure::InvalidNodeIndex(*node_idx))?;
    }

    let vertex_count = old_mesh.vertices.len() / 3;

    // f32 → f64 widening — vertices is a flat [x, y, z, …] buffer;
    // chunks_exact(3) slices each triple directly, avoiding per-iteration
    // bounds checks that vertex_f64 would re-run unnecessarily.
    let mut current: Vec<[f64; 3]> = old_mesh
        .vertices
        .chunks_exact(3)
        .map(|c| [c[0] as f64, c[1] as f64, c[2] as f64])
        .collect();

    // Boundary classification — node is "boundary" iff it appears in
    // prescribed_positions. Materialised as a Vec<bool> for O(1) lookup
    // inside the iteration loop.
    let mut is_boundary = vec![false; vertex_count];
    for (node_idx, position) in prescribed_positions {
        let i = *node_idx as usize;
        is_boundary[i] = true;
        current[i] = *position;
    }

    // Build node→neighbours adjacency from the P1 tet index buffer. Each tet
    // contributes 6 unordered topological edges (4 corners → C(4,2) = 6).
    // BTreeSet for deterministic iteration order — same load-bearing reason
    // BoundaryAssociation uses BTreeMap (FEA warm-start bit-stability).
    let mut adjacency: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); vertex_count];
    for tet in old_mesh.tet_indices.chunks_exact(4) {
        // Each unordered pair (i, j) inserts j into adjacency[i] and i into
        // adjacency[j]. Skip the diagonal (i == j) — degenerate tets where a
        // node repeats would otherwise self-link. Out-of-range indices on
        // either side are silently dropped: storing an out-of-range j would
        // panic on `current[j as usize]` inside the iteration loop. The
        // doc-comment delegates the "tet_indices reference valid vertices"
        // precondition to the caller, so we defensively guard rather than
        // returning a failure here.
        for &i in tet {
            for &j in tet {
                if i != j && (i as usize) < vertex_count && (j as usize) < vertex_count {
                    adjacency[i as usize].insert(j);
                }
            }
        }
    }

    // Jacobi double-buffer: each iteration reads exclusively from `current`
    // and writes exclusively to `next`. In-place mutation would convert
    // this into Gauss-Seidel and make the result depend on traversal order.
    // Allocate the second buffer once and reuse via std::mem::swap so the
    // amortised allocation cost is paid exactly once across all iterations.
    let mut next: Vec<[f64; 3]> = vec![[0.0; 3]; vertex_count];
    for _ in 0..iterations {
        for i in 0..vertex_count {
            if is_boundary[i] {
                next[i] = current[i];
                continue;
            }
            let neighbours = &adjacency[i];
            if neighbours.is_empty() {
                // Design decision: "interior nodes with zero topological
                // neighbours retain their original position across all
                // iterations" — keeps the function total when the mesh
                // contains an orphan node (no incident tet) without
                // injecting NaN via 0/0. Pinned by
                // laplacian_smooth_with_orphan_interior_node_leaves_position_unchanged.
                next[i] = current[i];
                continue;
            }
            let mut sum = [0.0_f64; 3];
            for &j in neighbours {
                let p = current[j as usize];
                sum[0] += p[0];
                sum[1] += p[1];
                sum[2] += p[2];
            }
            let n = neighbours.len() as f64;
            next[i] = [sum[0] / n, sum[1] / n, sum[2] / n];
        }
        std::mem::swap(&mut current, &mut next);
    }

    // f64 → f32 narrowing at the write boundary, restoring the canonical
    // [x0,y0,z0,x1,…] flat layout.
    let mut out_vertices = Vec::with_capacity(old_mesh.vertices.len());
    for p in &current {
        out_vertices.push(p[0] as f32);
        out_vertices.push(p[1] as f32);
        out_vertices.push(p[2] as f32);
    }

    Ok(VolumeMesh {
        vertices: out_vertices,
        tet_indices: old_mesh.tet_indices.clone(),
        element_order: old_mesh.element_order,
        normals: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::{ElementOrderTag, VolumeMesh};

    fn empty_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: Vec::new(),
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    // ── Step-3: smoke test for the public API surface ─────────────────────────

    #[test]
    fn laplacian_smooth_with_empty_mesh_and_no_prescribed_positions_returns_empty_mesh() {
        let result = laplacian_smooth(&empty_mesh(), &[], 0);
        assert!(result.is_ok(), "got: {result:?}");
        let mesh = result.unwrap();
        assert!(mesh.vertices.is_empty());
        assert!(mesh.tet_indices.is_empty());
        assert_eq!(mesh.element_order, ElementOrderTag::P1);
        assert!(mesh.normals.is_none());
    }

    // ── Step-5: P2 element order rejection ────────────────────────────────────

    #[test]
    fn laplacian_smooth_rejects_p2_element_order_with_unsupported_element_order_failure() {
        let mesh = VolumeMesh {
            vertices: Vec::new(),
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P2,
            normals: None,
        };
        let result = laplacian_smooth(&mesh, &[], 1);
        // VolumeMesh has no PartialEq, so destructure the Err arm rather than
        // assert_eq! on the Result.
        match result {
            Err(LaplacianFailure::UnsupportedElementOrder(order)) => {
                assert_eq!(order, ElementOrderTag::P2);
            }
            other => panic!("expected UnsupportedElementOrder(P2), got: {other:?}"),
        }
    }

    // ── Step-7: out-of-range prescribed-position node index ──────────────────

    /// Mirrors `compute_dirichlet_bcs_node_index_out_of_mesh_vertices_range_*`
    /// from boundary.rs:635 — same overflow-safe index validation, same
    /// structured failure shape.
    #[test]
    fn laplacian_smooth_with_node_index_out_of_mesh_vertices_range_returns_invalid_node_index() {
        // 2 nodes → vertices.len() == 6; node 5 → base = 15 >= 6
        let mesh = VolumeMesh {
            vertices: vec![0.0_f32, 0.0, 0.0, 1.0, 1.0, 1.0],
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let result = laplacian_smooth(&mesh, &[(5, [9.0, 9.0, 9.0])], 1);
        match result {
            Err(LaplacianFailure::InvalidNodeIndex(idx)) => {
                assert_eq!(idx, 5);
            }
            other => panic!("expected InvalidNodeIndex(5), got: {other:?}"),
        }
    }

    // ── Step-9: prescribed positions applied; structural fields preserved ────

    /// With `iterations = 0`, every node listed in `prescribed_positions` must
    /// be at its prescribed position in the output (boundary nodes are pinned
    /// at the start of every pass, including the zero'th). Structural fields
    /// (`tet_indices`, `element_order`) must be carried through unchanged.
    /// `normals` is always `None` on output regardless of input — vertex motion
    /// invalidates per-vertex normals; see
    /// `laplacian_smooth_drops_normals_on_output_even_when_input_has_some_normals`.
    #[test]
    fn laplacian_smooth_with_zero_iterations_pins_boundary_nodes_and_preserves_structural_fields() {
        // Single-tet mesh: 4 vertices, 1 tet, P1, no normals.
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // node 0
                1.0, 0.0, 0.0, // node 1
                0.0, 1.0, 0.0, // node 2
                0.0, 0.0, 1.0, // node 3
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // All 4 nodes pinned to displaced positions.
        let prescribed = vec![
            (0, [0.1_f64, 0.2, 0.3]),
            (1, [1.1, 0.2, 0.3]),
            (2, [0.1, 1.2, 0.3]),
            (3, [0.1, 0.2, 1.3]),
        ];

        let out = laplacian_smooth(&mesh, &prescribed, 0).unwrap();

        // Expected vertices: f64 prescribed values cast to f32, in flat layout.
        let expected: Vec<f32> = vec![
            0.1, 0.2, 0.3, // node 0
            1.1, 0.2, 0.3, // node 1
            0.1, 1.2, 0.3, // node 2
            0.1, 0.2, 1.3, // node 3
        ];
        assert_eq!(out.vertices, expected);

        // Structural fields carry through bit-equal.
        assert_eq!(out.tet_indices, vec![0u32, 1, 2, 3]);
        assert_eq!(out.element_order, ElementOrderTag::P1);
        assert!(out.normals.is_none());
    }

    // ── Step-11: one iteration averages interior node to neighbour centroid ──

    /// 4-tet "cone" fixture: 5 vertices `a, b, c, d, p` where `a, b, c, d` are
    /// the four boundary nodes and `p` is the only interior node, shared by
    /// every tet. After one Jacobi smoothing pass with `a, b, c, d` pinned
    /// to displaced positions, `p` should be at exactly `(a + b + c + d) / 4`
    /// — its only topological neighbours.
    #[test]
    fn laplacian_smooth_with_one_iteration_smooths_interior_node_to_centroid_of_its_topological_neighbors()
     {
        // Layout: nodes 0..3 = a, b, c, d; node 4 = p.
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // 0: a
                1.0, 0.0, 0.0, // 1: b
                0.0, 1.0, 0.0, // 2: c
                0.0, 0.0, 1.0, // 3: d
                0.5, 0.5, 0.5, // 4: p (off-centre)
            ],
            // Four tets all sharing p (node 4).
            tet_indices: vec![
                0, 1, 2, 4, // a, b, c, p
                0, 1, 3, 4, // a, b, d, p
                0, 2, 3, 4, // a, c, d, p
                1, 2, 3, 4, // b, c, d, p
            ],
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // Pin a, b, c, d to displaced positions; leave p free.
        let displaced_a = [0.1_f64, 0.0, 0.0];
        let displaced_b = [1.1, 0.0, 0.0];
        let displaced_c = [0.0, 1.1, 0.0];
        let displaced_d = [0.0, 0.0, 1.1];
        let prescribed = vec![
            (0_u32, displaced_a),
            (1, displaced_b),
            (2, displaced_c),
            (3, displaced_d),
        ];

        let out = laplacian_smooth(&mesh, &prescribed, 1).unwrap();

        // p's neighbours in the topological-edge graph are exactly {a, b, c, d}
        // — every tet contributes the unordered pairs (a,p), (b,p), (c,p),
        // (d,p) and only those four pairs touch p.
        let expected_p = [
            (displaced_a[0] + displaced_b[0] + displaced_c[0] + displaced_d[0]) / 4.0,
            (displaced_a[1] + displaced_b[1] + displaced_c[1] + displaced_d[1]) / 4.0,
            (displaced_a[2] + displaced_b[2] + displaced_c[2] + displaced_d[2]) / 4.0,
        ];

        // f32-narrowed comparison; round-trip cast for tolerance ~ 1e-6_f32.
        let tol = 1e-6_f32;

        // a, b, c, d at their prescribed positions (cast to f32).
        for (node_idx, prescribed_pos) in &prescribed {
            let base = (*node_idx as usize) * 3;
            assert!((out.vertices[base] - prescribed_pos[0] as f32).abs() <= tol);
            assert!((out.vertices[base + 1] - prescribed_pos[1] as f32).abs() <= tol);
            assert!((out.vertices[base + 2] - prescribed_pos[2] as f32).abs() <= tol);
        }
        // p at the centroid of its neighbours.
        let p_base = 4 * 3;
        assert!((out.vertices[p_base] - expected_p[0] as f32).abs() <= tol);
        assert!((out.vertices[p_base + 1] - expected_p[1] as f32).abs() <= tol);
        assert!((out.vertices[p_base + 2] - expected_p[2] as f32).abs() <= tol);
    }

    // ── Step-13: multi-iteration Jacobi propagates through interior chain ────

    /// Two interior nodes `p` and `q` connected to each other and each to a
    /// disjoint subset of pinned boundary nodes. Builds the topology so that
    /// `p`'s neighbours = `{a, b, c, q}` and `q`'s neighbours = `{p, d, e, f}`,
    /// then asserts the closed-form Jacobi iterates after 1 and 2 passes.
    /// Comparing iter=1 vs. iter=2 also pins that more iterations move the
    /// interior nodes further from their initial positions toward the boundary
    /// — i.e. the iteration count is genuinely consumed by the loop.
    #[test]
    fn laplacian_smooth_with_multiple_iterations_jacobi_propagates_interior_displacement_through_chain()
     {
        // Layout: 0=a, 1=b, 2=c, 3=p, 4=q, 5=d, 6=e, 7=f.
        // p (3) is the only interior node in tets {a,b,c,p} and {a,b,p,q};
        // q (4) is the only interior node in tets {p,q,d,e} and {q,d,e,f}.
        //
        // Per-tet edge contributions (each tet's C(4,2) = 6 unordered pairs):
        //   {a,b,c,p}: ab ac ap bc bp cp
        //   {a,b,p,q}: ab ap aq bp bq pq
        //   {p,q,d,e}: pq pd pe qd qe de
        //   {q,d,e,f}: qd qe qf de df ef
        //
        // p's unique neighbours (across all tets): a, b, c, q  ✓
        // q's unique neighbours: a, b, p, d, e, f
        //
        // To make q's neighbours exactly {p, d, e, f}, drop the {a,b,p,q} tet
        // and use {p,q,c,?} instead. Re-design with a simpler two-tet topology:
        //
        // Tet 1: {a, b, c, p}  → p neighbours = {a, b, c}
        // Tet 2: {p, q, d, e}  → adds {p, q} and gives q neighbours = {p, d, e}
        //                       adds q to p's neighbours
        // Result: p's neighbours = {a, b, c, q}; q's neighbours = {p, d, e}.
        //
        // For symmetry and to reach 4 neighbours per node, add Tet 3:
        // Tet 3: {q, d, e, f}  → q's neighbours = {p, d, e, f}.
        //
        // 8 vertices: 0=a, 1=b, 2=c, 3=p, 4=q, 5=d, 6=e, 7=f.
        let mesh = VolumeMesh {
            vertices: vec![
                10.0_f32, 0.0, 0.0, // 0: a
                0.0, 10.0, 0.0, // 1: b
                0.0, 0.0, 10.0, // 2: c
                1.0, 1.0, 1.0, // 3: p (interior)
                2.0, 2.0, 2.0, // 4: q (interior)
                20.0, 0.0, 0.0, // 5: d
                0.0, 20.0, 0.0, // 6: e
                0.0, 0.0, 20.0, // 7: f
            ],
            tet_indices: vec![
                0, 1, 2, 3, // a, b, c, p
                3, 4, 5, 6, // p, q, d, e
                4, 5, 6, 7, // q, d, e, f
            ],
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // Pin all six boundary nodes to themselves (no displacement) — the
        // test focus is the interior propagation, not the boundary motion.
        let a = [10.0_f64, 0.0, 0.0];
        let b = [0.0, 10.0, 0.0];
        let c = [0.0, 0.0, 10.0];
        let d = [20.0, 0.0, 0.0];
        let e = [0.0, 20.0, 0.0];
        let f = [0.0, 0.0, 20.0];
        let prescribed = vec![(0_u32, a), (1, b), (2, c), (5, d), (6, e), (7, f)];

        // Initial interior positions (cast from the f32 mesh).
        let p0 = [1.0_f64, 1.0, 1.0];
        let q0 = [2.0_f64, 2.0, 2.0];

        // Topological neighbours:
        //   p's neighbours = {a, b, c, q}      (from tets 1 and 2 above)
        //   q's neighbours = {p, d, e, f}      (from tets 2 and 3 above)
        // (Tet 2 also adds d, e to p's neighbours? Let's recompute:
        //   Tet 2 = {p, q, d, e} → pairs pq, pd, pe, qd, qe, de
        //   So p's neighbours include {q, d, e}, plus from Tet 1 {a, b, c}.
        //   p's full neighbours = {a, b, c, q, d, e} (6 neighbours).
        //   q's neighbours from Tet 2 = {p, d, e}, from Tet 3 = {d, e, f}.
        //   q's full neighbours = {p, d, e, f}.
        // )
        // Recompute the Jacobi iterates with the CORRECT neighbour sets.
        //
        // p's neighbours = {a, b, c, q, d, e} (6 nodes)
        // q's neighbours = {p, d, e, f}       (4 nodes)

        // Iteration 1:
        let p1 = [
            (a[0] + b[0] + c[0] + q0[0] + d[0] + e[0]) / 6.0,
            (a[1] + b[1] + c[1] + q0[1] + d[1] + e[1]) / 6.0,
            (a[2] + b[2] + c[2] + q0[2] + d[2] + e[2]) / 6.0,
        ];
        let q1 = [
            (p0[0] + d[0] + e[0] + f[0]) / 4.0,
            (p0[1] + d[1] + e[1] + f[1]) / 4.0,
            (p0[2] + d[2] + e[2] + f[2]) / 4.0,
        ];
        // Iteration 2 (Jacobi: reads from iter-1 values):
        let p2 = [
            (a[0] + b[0] + c[0] + q1[0] + d[0] + e[0]) / 6.0,
            (a[1] + b[1] + c[1] + q1[1] + d[1] + e[1]) / 6.0,
            (a[2] + b[2] + c[2] + q1[2] + d[2] + e[2]) / 6.0,
        ];
        let q2 = [
            (p1[0] + d[0] + e[0] + f[0]) / 4.0,
            (p1[1] + d[1] + e[1] + f[1]) / 4.0,
            (p1[2] + d[2] + e[2] + f[2]) / 4.0,
        ];

        let tol = 1e-5_f32;

        // iter = 1
        let out1 = laplacian_smooth(&mesh, &prescribed, 1).unwrap();
        let p_at = |out: &VolumeMesh, idx: usize| -> [f32; 3] {
            let b = idx * 3;
            [out.vertices[b], out.vertices[b + 1], out.vertices[b + 2]]
        };
        let p_out1 = p_at(&out1, 3);
        let q_out1 = p_at(&out1, 4);
        for axis in 0..3 {
            assert!(
                (p_out1[axis] - p1[axis] as f32).abs() <= tol,
                "iter=1 p[{axis}]: out={} expected={}",
                p_out1[axis],
                p1[axis] as f32
            );
            assert!(
                (q_out1[axis] - q1[axis] as f32).abs() <= tol,
                "iter=1 q[{axis}]: out={} expected={}",
                q_out1[axis],
                q1[axis] as f32
            );
        }

        // iter = 2
        let out2 = laplacian_smooth(&mesh, &prescribed, 2).unwrap();
        let p_out2 = p_at(&out2, 3);
        let q_out2 = p_at(&out2, 4);
        for axis in 0..3 {
            assert!(
                (p_out2[axis] - p2[axis] as f32).abs() <= tol,
                "iter=2 p[{axis}]: out={} expected={}",
                p_out2[axis],
                p2[axis] as f32
            );
            assert!(
                (q_out2[axis] - q2[axis] as f32).abs() <= tol,
                "iter=2 q[{axis}]: out={} expected={}",
                q_out2[axis],
                q2[axis] as f32
            );
        }

        // Iter=1 and iter=2 must produce *different* interior positions
        // (otherwise the loop's iteration parameter is a no-op).
        assert!(
            p_out1 != p_out2 || q_out1 != q_out2,
            "iter=1 and iter=2 should produce different interior positions; \
             got p1={p_out1:?} p2={p_out2:?} q1={q_out1:?} q2={q_out2:?}"
        );
    }

    // ── Step-15: orphan interior node leaves position unchanged ──────────────

    /// Defensive: a node with no topological neighbours (not in any tet) and
    /// not in `prescribed_positions` must keep its initial position across
    /// every iteration. Documents the "no neighbours → unchanged" branch from
    /// the design decisions (prevents 0/0 division injecting NaN).
    #[test]
    fn laplacian_smooth_with_orphan_interior_node_leaves_position_unchanged() {
        // 5 vertices but tet_indices only references nodes 0..3 (node 4 is
        // an orphan: not in any tet, not prescribed).
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // 0
                1.0, 0.0, 0.0, // 1
                0.0, 1.0, 0.0, // 2
                0.0, 0.0, 1.0, // 3
                42.0, 43.0, 44.0, // 4: orphan
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        // Pin nodes 0..3 so the only "free" node is the orphan node 4.
        let prescribed = vec![
            (0_u32, [0.0_f64, 0.0, 0.0]),
            (1, [1.0, 0.0, 0.0]),
            (2, [0.0, 1.0, 0.0]),
            (3, [0.0, 0.0, 1.0]),
        ];

        let out = laplacian_smooth(&mesh, &prescribed, 5).unwrap();

        // Orphan must keep its initial position (no NaN).
        let base = 4 * 3;
        assert_eq!(out.vertices[base], 42.0);
        assert_eq!(out.vertices[base + 1], 43.0);
        assert_eq!(out.vertices[base + 2], 44.0);
    }

    // ── Step-17: determinism across runs with same input ────────────────────
    //
    // Two `laplacian_smooth` calls on the same input must produce bit-equal
    // outputs. Defends against a future refactor swapping the
    // `Vec<BTreeSet<u32>>` adjacency for a `Vec<HashSet<u32>>` — HashSet would
    // silently re-randomise iteration order across runs, perturbing
    // floating-point sums and breaking the FEA warm-start cache stability the
    // BoundaryAssociation BTreeMap discipline already protects (see boundary.rs).

    /// Reuses the step-13 fixture so the test exercises both interior nodes
    /// and multi-iteration accumulation — the regimes most sensitive to
    /// non-deterministic neighbour iteration order.
    #[test]
    fn laplacian_smooth_is_deterministic_across_runs_with_same_input() {
        let mesh = VolumeMesh {
            vertices: vec![
                10.0_f32, 0.0, 0.0, // 0: a
                0.0, 10.0, 0.0, // 1: b
                0.0, 0.0, 10.0, // 2: c
                1.0, 1.0, 1.0, // 3: p (interior)
                2.0, 2.0, 2.0, // 4: q (interior)
                20.0, 0.0, 0.0, // 5: d
                0.0, 20.0, 0.0, // 6: e
                0.0, 0.0, 20.0, // 7: f
            ],
            tet_indices: vec![
                0, 1, 2, 3, // a, b, c, p
                3, 4, 5, 6, // p, q, d, e
                4, 5, 6, 7, // q, d, e, f
            ],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let prescribed = vec![
            (0_u32, [10.0_f64, 0.0, 0.0]),
            (1, [0.0, 10.0, 0.0]),
            (2, [0.0, 0.0, 10.0]),
            (5, [20.0, 0.0, 0.0]),
            (6, [0.0, 20.0, 0.0]),
            (7, [0.0, 0.0, 20.0]),
        ];

        let out_a = laplacian_smooth(&mesh, &prescribed, 8).unwrap();
        let out_b = laplacian_smooth(&mesh, &prescribed, 8).unwrap();

        assert_eq!(out_a.vertices, out_b.vertices);
        assert_eq!(out_a.tet_indices, out_b.tet_indices);
        assert_eq!(out_a.element_order, out_b.element_order);
        // `normals` is unconditionally `None` after the step-20 contract change;
        // asserting `out_a.normals == out_b.normals` would be a tautology (`None == None`),
        // so we omit it here. The contract is pinned by `laplacian_smooth_drops_normals_on_output_even_when_input_has_some_normals`.
    }

    // ── Step-19: drops stale normals on output ──────────────────────────────

    /// Regression test: `laplacian_smooth` must set `normals: None` on the
    /// returned mesh *regardless of whether the input had normals*, because
    /// vertex motion makes any pre-existing per-vertex normals geometrically
    /// stale. Pinned by the contract-change in step-20.
    ///
    /// Fixture: a single-vertex mesh with `normals: Some(...)` on input.
    /// After any smoothing call (including zero iterations), the output must
    /// have `normals: None`.
    #[test]
    fn laplacian_smooth_drops_normals_on_output_even_when_input_has_some_normals() {
        let mesh = VolumeMesh {
            vertices: vec![0.0_f32, 0.0, 0.0],
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: Some(vec![1.0_f32, 0.0, 0.0]),
        };
        let out = laplacian_smooth(&mesh, &[], 0).unwrap();
        assert!(
            out.normals.is_none(),
            "expected normals: None, got: {:?}",
            out.normals
        );
    }

    /// Path-independence variant: same contract holds when `iterations >= 1`
    /// and the smoothing loop actually executes (not just the zero-iteration
    /// short-circuit). Uses a single-tet mesh so the loop runs one real pass;
    /// the `normals: None` result is independent of the smoothed vertex positions.
    #[test]
    fn laplacian_smooth_drops_normals_on_output_even_when_iterations_nonzero() {
        // 4-vertex single-tet mesh: node 3 is free, nodes 0-2 are pinned.
        // 4 f32x3 normals supplied on input (12 floats).
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // 0
                1.0, 0.0, 0.0, // 1
                0.0, 1.0, 0.0, // 2
                0.5, 0.5, 0.5, // 3 (interior, will be smoothed)
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: Some(vec![
                1.0_f32, 0.0, 0.0, // normal for node 0
                0.0, 1.0, 0.0, // normal for node 1
                0.0, 0.0, 1.0, // normal for node 2
                1.0, 1.0, 0.0, // normal for node 3
            ]),
        };
        let prescribed = vec![
            (0_u32, [0.0_f64, 0.0, 0.0]),
            (1, [1.0, 0.0, 0.0]),
            (2, [0.0, 1.0, 0.0]),
        ];
        let out = laplacian_smooth(&mesh, &prescribed, 1).unwrap();
        assert!(
            out.normals.is_none(),
            "expected normals: None even after 1 iteration, got: {:?}",
            out.normals
        );
    }

    // ── Step-3: exhaustive variant fence for LaplacianFailure ─────────────────
    //
    // No-wildcard match guarantees that adding/removing/renaming a variant
    // breaks compilation immediately — same discipline as MorphFailure's
    // four-variant fence in options.rs.
    #[test]
    fn laplacian_failure_variants_construct_and_pattern_match_exhaustively() {
        let invalid = LaplacianFailure::InvalidNodeIndex(5);
        let unsupported = LaplacianFailure::UnsupportedElementOrder(ElementOrderTag::P2);

        for failure in [&invalid, &unsupported] {
            match failure {
                LaplacianFailure::InvalidNodeIndex(idx) => {
                    assert_eq!(*idx, 5);
                }
                LaplacianFailure::UnsupportedElementOrder(order) => {
                    assert_eq!(*order, ElementOrderTag::P2);
                }
            }
        }
    }
}
