// step-1 RED: test module only — no production code yet.
// Compilation will fail because the types referenced below do not exist.

#[cfg(test)]
mod tests {
    use super::{mesh_mid_surface, MesherError, MesherOptions, MesherResult, QualityMetrics};
    use crate::MidSurfaceMesh;

    // ── Step 1: public-surface smoke test ────────────────────────────────────

    /// Public-surface compile-test: all public types are reachable from
    /// `crate::mesher` and `crate::` re-export paths; `mesh_mid_surface` is
    /// callable; empty input → `Ok` with empty mesh and zeroed metrics.
    #[test]
    fn mesher_public_surface_is_callable_on_empty_mesh() {
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };

        let result: MesherResult = mesh_mid_surface(&mesh, &MesherOptions::default())
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
        assert_eq!(result.metrics.triangle_count, 0, "empty input → 0 triangles");
        assert_eq!(result.metrics.vertex_count, 0, "empty input → 0 vertices");
        assert_eq!(result.remesh_iterations, 0, "no remeshing on empty input");

        // Compile probes: all six error variants are publicly named and constructible.
        let _: MesherError = MesherError::InvalidMergeTolerance { value: 0.0 };
        let _: MesherError = MesherError::InvalidMinAspectRatio { value: 0.0 };
        let _: MesherError = MesherError::InvalidMinAngleDegrees { value: 0.0 };
        let _: MesherError = MesherError::InconsistentInputMesh {
            vertices_len: 0,
            thickness_len: 0,
        };
        let _: MesherError = MesherError::OutOfRangeTriangleIndex {
            triangle_index: 0,
            vertex_index: 0,
            vertices_len: 0,
        };
        let _: MesherError = MesherError::QualityBelowThreshold {
            min_aspect_ratio: 0.0,
            min_angle_degrees: 0.0,
            failed_triangle_count: 0,
            remesh_iterations: 0,
        };

        // Compile probes: types reachable from crate root (re-export path).
        let _: crate::MesherOptions = crate::MesherOptions::default();
        let _: Option<crate::QualityMetrics> = None;
        let _: Option<crate::MesherResult> = None;
        let _: Option<crate::MesherError> = None;
        // Function reachable from crate root:
        let _: fn(
            &crate::MidSurfaceMesh,
            &crate::MesherOptions,
        ) -> Result<crate::MesherResult, crate::MesherError> = crate::mesh_mid_surface;
    }
}
