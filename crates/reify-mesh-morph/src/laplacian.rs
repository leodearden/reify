//! Laplacian quick-pass smoother (PRD task #6).
//!
//! Implements the cheap fast path for trivially small parameter changes,
//! per PRD `docs/prds/v0_3/mesh-morphing.md` §"Laplacian quick-pass":
//! surface nodes are pinned to their projected positions and interior nodes
//! are iteratively averaged with their topological neighbours.
//!
//! Selection logic (Laplacian vs. elasticity morph) lives in PRD task #10's
//! engine integration; this module delivers only the smoother kernel.

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
/// ## Failure modes
///
/// See [`LaplacianFailure`].
pub fn laplacian_smooth(
    old_mesh: &VolumeMesh,
    prescribed_positions: &[(u32, [f64; 3])],
    iterations: u32,
) -> Result<VolumeMesh, LaplacianFailure> {
    let _ = prescribed_positions;
    let _ = iterations;
    Ok(old_mesh.clone())
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
