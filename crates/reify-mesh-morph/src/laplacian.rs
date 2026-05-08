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

use reify_types::{ElementOrderTag, VolumeMesh};

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
/// - `old_mesh` — the current tetrahedral mesh.
/// - `prescribed_positions` — `(node_index, new_position)` pairs identifying
///   "boundary" nodes pinned to their projected targets. The natural producer
///   is [`crate::compute_dirichlet_bcs`] (PRD task #5).
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
/// ## Failure modes
///
/// See [`LaplacianFailure`].
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
    // allocation) — same overflow-safe pattern as boundary.rs:222-227.
    for (node_idx, _) in prescribed_positions {
        let i = *node_idx as usize;
        let base = i
            .checked_mul(3)
            .ok_or(LaplacianFailure::InvalidNodeIndex(*node_idx))?;
        let end = base
            .checked_add(3)
            .ok_or(LaplacianFailure::InvalidNodeIndex(*node_idx))?;
        if end > old_mesh.vertices.len() {
            return Err(LaplacianFailure::InvalidNodeIndex(*node_idx));
        }
    }

    let vertex_count = old_mesh.vertices.len() / 3;

    // f32 → f64 widening at the read boundary — all interior arithmetic in
    // f64. Same discipline as boundary.rs (compute_dirichlet_bcs).
    let mut current: Vec<[f64; 3]> = (0..vertex_count)
        .map(|i| {
            let base = i * 3;
            [
                old_mesh.vertices[base] as f64,
                old_mesh.vertices[base + 1] as f64,
                old_mesh.vertices[base + 2] as f64,
            ]
        })
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
        // node repeats would otherwise self-link.
        for &i in tet {
            for &j in tet {
                if i != j && (i as usize) < vertex_count {
                    adjacency[i as usize].insert(j);
                }
            }
        }
    }

    // Jacobi double-buffer: each iteration reads exclusively from `current`
    // and writes exclusively to `next`. In-place mutation would convert
    // this into Gauss-Seidel and make the result depend on traversal order.
    let mut next: Vec<[f64; 3]> = vec![[0.0; 3]; vertex_count];
    let _ = iterations; // step-14 wraps the body below in a 0..iterations loop.
    {
        for i in 0..vertex_count {
            if is_boundary[i] {
                next[i] = current[i];
                continue;
            }
            let neighbours = &adjacency[i];
            if neighbours.is_empty() {
                // No neighbours → orphan; carry forward unchanged so 0/0
                // doesn't inject NaN. Test pinned in step-15.
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
        normals: old_mesh.normals.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{ElementOrderTag, VolumeMesh};

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
        assert!(matches!(result, Ok(_)), "got: {result:?}");
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
    /// (`tet_indices`, `element_order`, `normals`) must be carried through
    /// unchanged.
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
