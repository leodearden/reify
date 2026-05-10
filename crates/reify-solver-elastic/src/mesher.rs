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

use reify_types::GeometryError;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Which 3D element a downstream swept-body sweep step targets.
///
/// `HexPreferred` requests Gmsh's blossom-recombination so 2D quads can be
/// extruded into 3D hex elements; if recombine yields a low-quality quad
/// mesh, [`mesh_swept_profile_2d`] falls back to triangles
/// ([`Mesh2d::Triangle`]) and reports `recombine_attempted=true,
/// recombine_quality_ok=false`. `WedgeOnly` skips recombine entirely and
/// always returns triangles (which a subsequent sweep step turns into
/// wedge elements).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepElementTarget {
    /// Try to produce a quad mesh (\u{2192} hex extrusion). Falls back to
    /// triangles if recombination fails the skew-quality threshold.
    HexPreferred,
    /// Always produce a triangle mesh (\u{2192} wedge extrusion).
    WedgeOnly,
}

/// 2D mesh of a swept-body cross-section — either all triangles or all
/// quads, never both.
///
/// Vertex coordinates are flat `[x0, y0, x1, y1, …]` with stride 2;
/// connectivity is `[i0, i1, i2, …]` with stride 3 for triangles and 4 for
/// quads. The discriminator carries the element shape implicitly so
/// downstream consumers (`task 2988` sweep step) don't need a separate
/// `ElementShape2d` tag field.
#[derive(Debug, Clone)]
pub enum Mesh2d {
    /// All-triangle mesh. `indices.len() % 3 == 0`.
    Triangle {
        /// Flat `[x0,y0,x1,y1,...]` 2D vertex buffer, stride 2.
        vertices: Vec<f32>,
        /// Flat triangle connectivity, stride 3.
        indices: Vec<u32>,
    },
    /// All-quad mesh. `indices.len() % 4 == 0`.
    Quad {
        /// Flat `[x0,y0,x1,y1,...]` 2D vertex buffer, stride 2.
        vertices: Vec<f32>,
        /// Flat quad connectivity, stride 4 (CCW corner order).
        indices: Vec<u32>,
    },
}

/// Output of [`mesh_swept_profile_2d`] — the produced 2D mesh bundled with
/// recombine-quality diagnostics.
///
/// `recombine_attempted` + `recombine_quality_ok` together let task 2989's
/// diagnostic code emit a "hex meshed" vs "wedge fallback" vs "wedge
/// native" distinction without re-running quality checks downstream.
#[derive(Debug, Clone)]
pub struct Mesh2dReport {
    /// Produced 2D mesh — triangles or quads depending on caller target and
    /// recombine-quality outcome.
    pub mesh: Mesh2d,
    /// `true` iff [`mesh_swept_profile_2d`] asked Gmsh to recombine to
    /// quads (i.e. caller target was [`SweepElementTarget::HexPreferred`]).
    pub recombine_attempted: bool,
    /// `true` iff every quad in the recombined output passed the
    /// `recombine_skew_threshold` quality check, OR no recombine was
    /// attempted (vacuous true). `false` only when recombine was attempted
    /// and at least one quad exceeded the threshold (triggering fall-back
    /// to triangles).
    pub recombine_quality_ok: bool,
}

/// Polygonal description of a 2D cross-section: an outer ring and any
/// number of holes.
///
/// Convention: outer ring CCW, hole rings CW (Gmsh accepts both
/// orientations, but downstream code may use the sign of the shoelace area
/// to disambiguate). All coordinates are 2D `[x, y]` in the profile's
/// local plane. Curved boundaries (arcs, splines) are pre-sampled by the
/// caller into polyline segments — this contract is closed against
/// upstream discretisation strategy.
#[derive(Debug, Clone)]
pub struct ProfileBoundary {
    /// Outer-boundary points (CCW for positive area).
    pub outer: Vec<[f64; 2]>,
    /// Zero or more hole rings (CW for positive holes-out-of-solid area).
    pub holes: Vec<Vec<[f64; 2]>>,
}

/// Errors from [`mesh_swept_profile_2d`].
#[derive(Debug)]
pub enum Mesh2dError {
    /// Outer ring is empty — caller passed nothing to mesh.
    EmptyBoundary,
    /// One of the rings (outer or a hole) has <3 distinct points, or the
    /// outer ring is collinear (zero signed area). Geometrically a non-
    /// surface; rejected before any Gmsh call.
    DegenerateBoundary,
    /// Underlying Gmsh call failed. Wraps the original `GeometryError` for
    /// diagnostic chains.
    GmshFailed(GeometryError),
    /// Gmsh is not available in this build (stub build — libgmsh not
    /// detected at compile time). Callers can choose to fall back to a
    /// different mesher or surface this as a configuration error.
    GmshUnavailable,
}

/// User-tunable knobs for one [`mesh_swept_profile_2d`] call.
///
/// Mirrors the [`reify_kernel_gmsh::MeshingOptions`] shape with one
/// addition (`recombine_skew_threshold`) and one omission (no `threads` —
/// 2D meshing is single-threaded in Gmsh's default algorithm regardless of
/// `General.NumThreads`).
#[derive(Debug, Clone)]
pub struct Mesh2dOptions {
    /// Target characteristic mesh edge length in profile-plane units.
    /// `None` triggers auto-derivation via
    /// [`auto_mesh_size_from_boundary`] with `multiplier=1.0`.
    pub mesh_size: Option<f64>,
    /// When `true`, force single-threaded 2D meshing for bit-deterministic
    /// output. Mirrors `MeshingOptions.deterministic`.
    pub deterministic: bool,
    /// Maximum per-quad skew angle (radians, `|corner_angle - \u{3c0}/2|`)
    /// tolerated before triangle fall-back. Default: \u{3c0}/4 (45° off
    /// square). Pointy/triangular profiles cleanly exceed this; reasonable
    /// rectangular profiles stay well under.
    pub recombine_skew_threshold: f64,
}

impl Default for Mesh2dOptions {
    fn default() -> Self {
        Self {
            mesh_size: None,
            deterministic: false,
            recombine_skew_threshold: std::f64::consts::FRAC_PI_4,
        }
    }
}

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

    // ---- step-3: compute_quad_skew ----
    //
    // Definition: per-corner unsigned deviation from \u{3c0}/2; the function
    // returns the maximum over the four corners.

    #[test]
    fn compute_quad_skew_unit_square_is_zero() {
        // Each corner is exactly \u{3c0}/2 — skew is 0.
        let q = [[0.0_f64, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let s = compute_quad_skew(&q);
        assert!(s.abs() < 1e-12, "unit-square skew = {s}, expected 0.0");
    }

    #[test]
    fn compute_quad_skew_collapsed_corner_is_large() {
        // Repeated vertex collapses one edge to zero length; the two
        // adjacent corners are degenerate. The function must return a
        // value at least \u{3c0}/4 to flag this as bad.
        let q = [[0.0_f64, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let s = compute_quad_skew(&q);
        assert!(
            s >= std::f64::consts::FRAC_PI_4,
            "collapsed-corner skew = {s}, expected >= \u{3c0}/4"
        );
    }

    #[test]
    fn compute_quad_skew_parallelogram_matches_geometry() {
        // Parallelogram with top side shifted by 0.5: two corners are
        // atan(2) above \u{3c0}/2 and two are below. The max deviation is
        // |\u{3c0}/2 - atan(2)| = atan(0.5) \u{2248} 0.4636 rad.
        let q = [[0.0_f64, 0.0], [1.0, 0.0], [1.5, 1.0], [0.5, 1.0]];
        let expected = (0.5_f64).atan();
        let s = compute_quad_skew(&q);
        assert!(
            (s - expected).abs() < 1e-9,
            "parallelogram skew = {s}, expected {expected}"
        );
    }

    #[test]
    fn compute_quad_skew_is_orientation_agnostic() {
        // CCW and CW orderings of the same shape must produce the same
        // skew score: skew is an unsigned geometric property.
        let ccw = [[0.0_f64, 0.0], [1.0, 0.0], [1.5, 1.0], [0.5, 1.0]];
        let cw = [[0.0_f64, 0.0], [0.5, 1.0], [1.5, 1.0], [1.0, 0.0]];
        let s_ccw = compute_quad_skew(&ccw);
        let s_cw = compute_quad_skew(&cw);
        assert!(
            (s_ccw - s_cw).abs() < 1e-12,
            "skew is sign-dependent: CCW={s_ccw} CW={s_cw}"
        );
    }
}
