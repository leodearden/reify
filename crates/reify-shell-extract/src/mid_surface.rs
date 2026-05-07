#[cfg(test)]
mod tests {
    use super::*;
    use crate::medial::MedialMask;
    use reify_types::value::{InterpolationKind, SampledField, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    fn one_voxel_field() -> SampledField {
        SampledField {
            name: "test-1x1x1".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![0.0, 0.0, 0.0],
            spacing: vec![1.0, 1.0, 1.0],
            axis_grids: vec![vec![0.0], vec![0.0], vec![0.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![1.0],
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Public-surface compile-test: `MidSurfaceMesh`, `MidSurfaceOptions`,
    /// `MidSurfaceError`, and `extract_mid_surface` are reachable and callable.
    ///
    /// Empty mask on a valid 1×1×1 Regular3D SDF → `Ok(MidSurfaceMesh)` with
    /// all vecs empty (short-circuit before any triangulation).
    #[test]
    fn mid_surface_public_surface_is_callable_on_empty_mask() {
        let sdf = one_voxel_field();
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        };
        let mesh: MidSurfaceMesh =
            extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
                .expect("empty mask on valid 3D SDF should return empty mesh");
        assert!(
            mesh.vertices.is_empty() && mesh.triangles.is_empty() && mesh.thickness.is_empty(),
            "empty mask must produce empty mid-surface mesh"
        );

        // Compile-test: error type is publicly named.
        let _: MidSurfaceError = MidSurfaceError::EmptyAxisGrid { axis: 0 };
    }
}
