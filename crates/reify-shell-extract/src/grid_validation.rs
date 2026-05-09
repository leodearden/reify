//! Shared Regular3D input validation for `reify-shell-extract` algorithms.
//!
//! Extracted from the duplicated validation prelude in `medial.rs` and
//! `mid_surface.rs`. See design decisions in the task plan.

#[cfg(test)]
mod tests {
    use super::{validate_regular3d, GridValidationError};
    use crate::medial::MedialError;
    use crate::mid_surface::MidSurfaceError;
    use reify_types::value::{InterpolationKind, SampledField, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    // ── Fixture helpers ───────────────────────────────────────────────────────

    /// Minimal valid Regular3D field: 3×3×3 at unit spacing, all φ = +1.
    fn minimal_3d_field() -> SampledField {
        SampledField {
            name: "test-3x3x3".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![2.0, 2.0, 2.0],
            spacing: vec![1.0, 1.0, 1.0],
            axis_grids: vec![
                vec![0.0, 1.0, 2.0],
                vec![0.0, 1.0, 2.0],
                vec![0.0, 1.0, 2.0],
            ],
            interpolation: InterpolationKind::Linear,
            data: vec![1.0; 27],
            oob_emitted: AtomicBool::new(false),
        }
    }

    fn one_d_field() -> SampledField {
        SampledField {
            name: "test-1d".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![2.0],
            spacing: vec![1.0],
            axis_grids: vec![vec![0.0, 1.0, 2.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![1.0, -1.0, 1.0],
            oob_emitted: AtomicBool::new(false),
        }
    }

    fn two_d_field() -> SampledField {
        SampledField {
            name: "test-2d".to_string(),
            kind: SampledGridKind::Regular2D,
            bounds_min: vec![0.0, 0.0],
            bounds_max: vec![2.0, 2.0],
            spacing: vec![1.0, 1.0],
            axis_grids: vec![vec![0.0, 1.0, 2.0], vec![0.0, 1.0, 2.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![1.0; 9],
            oob_emitted: AtomicBool::new(false),
        }
    }

    // ── validate_regular3d tests ──────────────────────────────────────────────

    /// A valid Regular3D field must return `Ok(())`.
    #[test]
    fn validate_regular3d_accepts_valid_field() {
        let sdf = minimal_3d_field();
        assert!(
            validate_regular3d(&sdf).is_ok(),
            "valid Regular3D field must pass validation"
        );
    }

    /// A Regular1D field must be rejected with `UnsupportedGridKind`.
    #[test]
    fn validate_regular3d_rejects_regular1d() {
        let sdf = one_d_field();
        let err = validate_regular3d(&sdf).expect_err("1D field must be rejected");
        assert_eq!(
            err,
            GridValidationError::UnsupportedGridKind {
                found: SampledGridKind::Regular1D
            }
        );
    }

    /// A Regular2D field must be rejected with `UnsupportedGridKind`.
    #[test]
    fn validate_regular3d_rejects_regular2d() {
        let sdf = two_d_field();
        let err = validate_regular3d(&sdf).expect_err("2D field must be rejected");
        assert_eq!(
            err,
            GridValidationError::UnsupportedGridKind {
                found: SampledGridKind::Regular2D
            }
        );
    }

    /// Regular3D field with `bounds_min.len() != 3` → `AxisLengthMismatch`.
    #[test]
    fn validate_regular3d_rejects_axis_length_mismatch() {
        let mut sdf = minimal_3d_field();
        sdf.bounds_min = vec![0.0]; // length 1, not 3
        let err =
            validate_regular3d(&sdf).expect_err("axis length mismatch must be rejected");
        assert_eq!(
            err,
            GridValidationError::AxisLengthMismatch {
                bounds_min_len: 1,
                bounds_max_len: 3,
                spacing_len: 3,
                axis_grids_len: 3,
            }
        );
    }

    /// Regular3D with `axis_grids[0] = []` → `EmptyAxisGrid { axis: 0 }`.
    #[test]
    fn validate_regular3d_rejects_empty_axis_grid() {
        let mut sdf = minimal_3d_field();
        sdf.axis_grids[0] = vec![]; // empty first axis
        let err =
            validate_regular3d(&sdf).expect_err("empty axis grid must be rejected");
        assert_eq!(err, GridValidationError::EmptyAxisGrid { axis: 0 });
    }

    // ── From<GridValidationError> for MidSurfaceError tests ──────────────────

    #[test]
    fn from_grid_validation_error_for_mid_surface_error_empty_axis_grid() {
        let gve = GridValidationError::EmptyAxisGrid { axis: 1 };
        let mse = MidSurfaceError::from(gve);
        assert_eq!(mse, MidSurfaceError::EmptyAxisGrid { axis: 1 });
    }

    #[test]
    fn from_grid_validation_error_for_mid_surface_error_unsupported_grid_kind() {
        let gve = GridValidationError::UnsupportedGridKind {
            found: SampledGridKind::Regular1D,
        };
        let mse = MidSurfaceError::from(gve);
        assert_eq!(
            mse,
            MidSurfaceError::UnsupportedGridKind {
                found: SampledGridKind::Regular1D
            }
        );
    }

    #[test]
    fn from_grid_validation_error_for_mid_surface_error_axis_length_mismatch() {
        let gve = GridValidationError::AxisLengthMismatch {
            bounds_min_len: 1,
            bounds_max_len: 3,
            spacing_len: 3,
            axis_grids_len: 3,
        };
        let mse = MidSurfaceError::from(gve);
        assert_eq!(
            mse,
            MidSurfaceError::AxisLengthMismatch {
                bounds_min_len: 1,
                bounds_max_len: 3,
                spacing_len: 3,
                axis_grids_len: 3,
            }
        );
    }

    // ── From<GridValidationError> for MedialError tests ───────────────────────

    #[test]
    fn from_grid_validation_error_for_medial_error_empty_axis_grid() {
        let gve = GridValidationError::EmptyAxisGrid { axis: 2 };
        let me = MedialError::from(gve);
        assert_eq!(me, MedialError::EmptyAxisGrid { axis: 2 });
    }

    #[test]
    fn from_grid_validation_error_for_medial_error_unsupported_grid_kind() {
        let gve = GridValidationError::UnsupportedGridKind {
            found: SampledGridKind::Regular2D,
        };
        let me = MedialError::from(gve);
        assert_eq!(
            me,
            MedialError::UnsupportedGridKind {
                found: SampledGridKind::Regular2D
            }
        );
    }

    #[test]
    fn from_grid_validation_error_for_medial_error_axis_length_mismatch() {
        let gve = GridValidationError::AxisLengthMismatch {
            bounds_min_len: 2,
            bounds_max_len: 3,
            spacing_len: 3,
            axis_grids_len: 3,
        };
        let me = MedialError::from(gve);
        assert_eq!(
            me,
            MedialError::AxisLengthMismatch {
                bounds_min_len: 2,
                bounds_max_len: 3,
                spacing_len: 3,
                axis_grids_len: 3,
            }
        );
    }
}
