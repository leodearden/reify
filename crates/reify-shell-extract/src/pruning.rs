//! Spurious-branch pruning on mid-surface meshes (PRD task T3).
//!
//! Detects and removes medial-surface branches whose length-to-local-thickness
//! ratio falls below a configurable threshold.

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mid_surface::MidSurfaceMesh;

    // ── Step 1: smoke test ────────────────────────────────────────────────────

    /// Public-surface compile-test: all public types are reachable from
    /// `crate::pruning` and `crate::` re-export paths; `prune_branches` is
    /// callable; empty input → `Ok` with empty mesh and zero metrics.
    ///
    /// Mirrors `mesher_public_surface_is_callable_on_empty_mesh` in `mesher.rs`.
    #[test]
    fn prune_branches_public_surface_is_callable_on_empty_mesh() {
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };

        let result: PruneResult = prune_branches(&mesh, &PruneOptions::default())
            .expect("empty mesh should return Ok");
        assert!(
            result.mesh.vertices.is_empty(),
            "empty input → empty output vertices"
        );
        assert!(
            result.mesh.triangles.is_empty(),
            "empty input → empty output triangles"
        );
        assert!(
            result.mesh.thickness.is_empty(),
            "empty input → empty output thickness"
        );
        assert_eq!(result.metrics.iterations, 0, "no iterations on empty input");

        // Compile probes: all four error variants are publicly named and
        // constructible.
        let _: PruneError = PruneError::InvalidRatio { value: 0.0 };
        let _: PruneError = PruneError::InvalidMaxIterations { value: 0 };
        let _: PruneError = PruneError::InconsistentInputMesh {
            vertices_len: 0,
            thickness_len: 0,
        };
        let _: PruneError = PruneError::OutOfRangeTriangleIndex {
            triangle_index: 0,
            vertex_index: 0,
            vertices_len: 0,
        };

        // Compile probes: types reachable from crate root (re-export path).
        let _: crate::PruneOptions = crate::PruneOptions::default();
        let _: Option<crate::PruneMetrics> = None;
        let _: Option<crate::PruneResult> = None;
        let _: Option<crate::PruneError> = None;
        // Function reachable from crate root:
        let _: fn(
            &crate::MidSurfaceMesh,
            &crate::PruneOptions,
        ) -> Result<crate::PruneResult, crate::PruneError> = crate::prune_branches;
    }
}
