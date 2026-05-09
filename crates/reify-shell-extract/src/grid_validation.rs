//! Shared Regular3D input validation for `reify-shell-extract` algorithms.
//!
//! Extracted from the duplicated validation prelude in `medial.rs` and
//! `mid_surface.rs`. See design decisions in the task plan.

use reify_types::value::{SampledField, SampledGridKind};

/// Structural validation errors for Regular3D [`SampledField`] inputs,
/// shared across the algorithms in this crate.
///
/// Produced by [`validate_regular3d`]; converted to each algorithm's
/// error enum via `From<GridValidationError>` impls.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GridValidationError {
    /// The input [`SampledField`] is not 3D — only
    /// [`SampledGridKind::Regular3D`] is supported by the 3D algorithms
    /// in this crate.
    UnsupportedGridKind {
        /// The actual kind found on the input field.
        found: SampledGridKind,
    },
    /// One or more of `bounds_min` / `bounds_max` / `spacing` /
    /// `axis_grids` does not have length 3 on a `Regular3D` field.
    AxisLengthMismatch {
        /// Length of the supplied `bounds_min` vector.
        bounds_min_len: usize,
        /// Length of the supplied `bounds_max` vector.
        bounds_max_len: usize,
        /// Length of the supplied `spacing` vector.
        spacing_len: usize,
        /// Length of the supplied `axis_grids` vector.
        axis_grids_len: usize,
    },
    /// One axis's grid coordinate vector is empty.
    EmptyAxisGrid {
        /// Index of the offending axis (0 / 1 / 2 for x / y / z).
        axis: usize,
    },
}

impl std::fmt::Display for GridValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GridValidationError::UnsupportedGridKind { found } => write!(
                f,
                "reify-shell-extract requires a Regular3D SampledField input \
                 (the medial-axis test walks the SDF gradient in 3-space); \
                 got {found:?}"
            ),
            GridValidationError::AxisLengthMismatch {
                bounds_min_len,
                bounds_max_len,
                spacing_len,
                axis_grids_len,
            } => write!(
                f,
                "Regular3D SampledField axis-vector length mismatch: \
                 bounds_min has {bounds_min_len}, bounds_max has {bounds_max_len}, \
                 spacing has {spacing_len}, axis_grids has {axis_grids_len} \
                 (all four must be 3)"
            ),
            GridValidationError::EmptyAxisGrid { axis } => write!(
                f,
                "Regular3D SampledField axis_grids[{axis}] is empty \
                 (a non-empty per-axis grid is required)"
            ),
        }
    }
}

impl std::error::Error for GridValidationError {}

/// Validate that `sdf` is a structurally correct Regular3D [`SampledField`].
///
/// Checks in order:
/// 1. `sdf.kind == Regular3D` (returns [`GridValidationError::UnsupportedGridKind`])
/// 2. All four axis vectors have length 3 (returns [`GridValidationError::AxisLengthMismatch`])
/// 3. No `axis_grids[i]` is empty (returns [`GridValidationError::EmptyAxisGrid`])
pub(crate) fn validate_regular3d(sdf: &SampledField) -> Result<(), GridValidationError> {
    // (1) Reject non-3D inputs up front.
    if sdf.kind != SampledGridKind::Regular3D {
        return Err(GridValidationError::UnsupportedGridKind { found: sdf.kind });
    }

    // (2) Defend downstream indexing: Regular3D requires every axis vector
    // to have length 3.
    if sdf.bounds_min.len() != 3
        || sdf.bounds_max.len() != 3
        || sdf.spacing.len() != 3
        || sdf.axis_grids.len() != 3
    {
        return Err(GridValidationError::AxisLengthMismatch {
            bounds_min_len: sdf.bounds_min.len(),
            bounds_max_len: sdf.bounds_max.len(),
            spacing_len: sdf.spacing.len(),
            axis_grids_len: sdf.axis_grids.len(),
        });
    }

    // (3) Each axis grid must be non-empty.
    for (axis, axis_grid) in sdf.axis_grids.iter().enumerate() {
        if axis_grid.is_empty() {
            return Err(GridValidationError::EmptyAxisGrid { axis });
        }
    }

    Ok(())
}

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
