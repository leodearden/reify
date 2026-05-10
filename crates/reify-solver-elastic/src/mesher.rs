//! 2D cross-section meshing for the hex/wedge swept-body pipeline.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #6.
//!
//! This module is the typed orchestrator that turns a 2D profile boundary
//! (outer ring + optional holes) into a triangle or quad surface mesh,
//! routing the actual Gmsh call through
//! [`reify_kernel_gmsh::mesh_profile_2d::mesh_plane_2d`]. Pure-Rust quality
//! helpers ([`compute_quad_skew`], [`recombine_quality_ok`],
//! [`auto_mesh_size_from_boundary`]) live here so they remain unit-testable
//! in stub builds without libgmsh present.

#[cfg(test)]
mod tests {
    use super::*;

    // ---- (a) SweepElementTarget public surface ----
    #[test]
    fn sweep_element_target_variants_are_partial_eq_and_copy() {
        let hex: SweepElementTarget = SweepElementTarget::HexPreferred;
        let wedge: SweepElementTarget = SweepElementTarget::WedgeOnly;
        // Copy: re-use both bindings after a shadow copy.
        let _hex_copy: SweepElementTarget = hex;
        let _wedge_copy: SweepElementTarget = wedge;
        assert_ne!(hex, wedge);
        assert_eq!(hex, SweepElementTarget::HexPreferred);
        assert_eq!(wedge, SweepElementTarget::WedgeOnly);
    }

    // ---- (b) Mesh2d variants accept f32 vertices / u32 indices ----
    #[test]
    fn mesh2d_triangle_and_quad_construct_with_expected_types() {
        let _tri = Mesh2d::Triangle {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 0.5, 1.0],
            indices: vec![0_u32, 1, 2],
        };
        let _quad = Mesh2d::Quad {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
            indices: vec![0_u32, 1, 2, 3],
        };
    }

    // ---- (c) Mesh2dReport struct shape ----
    #[test]
    fn mesh2d_report_struct_literal_constructs() {
        let report = Mesh2dReport {
            mesh: Mesh2d::Triangle {
                vertices: vec![0.0_f32; 6],
                indices: vec![0_u32, 1, 2],
            },
            recombine_attempted: false,
            recombine_quality_ok: true,
        };
        assert!(!report.recombine_attempted);
        assert!(report.recombine_quality_ok);
    }

    // ---- (d) ProfileBoundary accepts Vec<[f64;2]> ----
    #[test]
    fn profile_boundary_accepts_2d_points() {
        let pb = ProfileBoundary {
            outer: vec![[0.0_f64, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            holes: vec![vec![[0.25_f64, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]]],
        };
        assert_eq!(pb.outer.len(), 4);
        assert_eq!(pb.holes.len(), 1);
    }

    // ---- (e) Mesh2dError variants ----
    #[test]
    fn mesh2d_error_has_required_variants() {
        // Each line constructs one variant — a missing variant or renamed
        // field would fail to compile.
        let _empty = Mesh2dError::EmptyBoundary;
        let _degen = Mesh2dError::DegenerateBoundary;
        let _unavail = Mesh2dError::GmshUnavailable;
        // GmshFailed wraps a GeometryError — construct the simplest variant.
        let _failed = Mesh2dError::GmshFailed(reify_types::GeometryError::OperationFailed(
            "test".to_string(),
        ));
    }

    // ---- (f) Mesh2dOptions::default() ----
    #[test]
    fn mesh2d_options_default_matches_spec() {
        let opts = Mesh2dOptions::default();
        assert_eq!(opts.mesh_size, None);
        assert!(!opts.deterministic);
        assert_eq!(opts.recombine_skew_threshold, std::f64::consts::FRAC_PI_4);
    }
}
