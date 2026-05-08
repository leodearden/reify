//! Laplacian quick-pass smoother (PRD task #6).
//!
//! Stub module — populated by step-4. Tests below are RED until then.

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
