//! Mid-surface mesh extraction from a medial mask (PRD task T2).
//!
//! Implements the binary marching-cubes step that converts a sparse
//! [`crate::medial::MedialMask`] into a triangle mesh ([`MidSurfaceMesh`])
//! with per-vertex through-thickness annotations.
//!
//! # Algorithm overview
//!
//! For each voxel cell `(i,j,k)` in the SDF grid, the 8 corner voxels are
//! tested against the medial mask. The resulting 8-bit indicator drives a
//! standard marching-cubes triangle table lookup; triangle vertices are placed
//! at the midpoints of the active edges (binary indicator `+1/−1` linearly
//! interpolates to zero at the midpoint). Per-vertex thickness is sampled from
//! the SDF at the mask corner of each edge: `thickness = 2 × |φ(voxel_center)|`.

use std::collections::HashSet;

use reify_ir::value::SampledField;

use crate::grid_validation::{GridValidationError, validate_regular3d};
use crate::medial::{MedialMask, sample_at_index};

/// Triangle mesh representing the mid-surface of a thin solid.
///
/// Output of [`extract_mid_surface`]. Vertices are in world coordinates;
/// triangles index into the vertex list; thickness gives the full
/// through-thickness at each vertex (`2 × |φ|` at the corresponding
/// medial voxel center).
///
/// **Vertex de-duplication note.** Shared edges between adjacent cells
/// produce duplicate vertex entries (`vertices.len()` may be up to
/// `3 × triangles.len()`). Downstream T9 (mid-surface mesher) de-duplicates
/// during quality remediation. See the design decision in `plan.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct MidSurfaceMesh {
    /// Vertex positions in world coordinates. Length may exceed
    /// `3 × triangles.len()` if the algorithm emits duplicate vertices
    /// for shared edges.
    pub vertices: Vec<[f64; 3]>,
    /// Triangle faces: each entry is three indices into [`vertices`].
    pub triangles: Vec<[u32; 3]>,
    /// Per-vertex through-thickness: `thickness[i]` corresponds to
    /// `vertices[i]`. Always the same length as [`vertices`].
    pub thickness: Vec<f64>,
}

/// Tunable parameters for [`extract_mid_surface`].
///
/// The only current field is the grid-alignment tolerance between the SDF
/// and the medial mask. Additional smoothing/refinement parameters are
/// deferred to future tasks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidSurfaceOptions {
    /// Maximum absolute difference between `sdf.spacing[i]` and
    /// `mask.spacing[i]` (and likewise between `sdf.bounds_min[i]`
    /// and `mask.origin[i]`) that is still considered "aligned". Default
    /// `1e-9` — effectively bit-exact for the internal producer pipeline
    /// (where `compute_medial_mask` propagates these values verbatim)
    /// while admitting float jitter from external producers.
    ///
    /// **Rationale.** `compute_medial_mask` copies `sdf.spacing` and
    /// `sdf.bounds_min` verbatim into `MedialMask::spacing` and `::origin`,
    /// so internal-pipeline masks are bit-exact at `1e-9`. A larger slack
    /// (e.g. `0.5 × min(spacing)`) would silently accept off-by-one-voxel
    /// masks and produce mid-surfaces with consistently wrong thickness —
    /// the exact failure mode the alignment check guards against.
    pub grid_alignment_tolerance: f64,
}

impl Default for MidSurfaceOptions {
    fn default() -> Self {
        Self {
            grid_alignment_tolerance: 1e-9,
        }
    }
}

/// Errors returned by [`extract_mid_surface`].
///
/// `#[non_exhaustive]` lets future variants be added without breaking
/// external exhaustive-match consumers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum MidSurfaceError {
    /// A structural validation error produced by the shared
    /// [`crate::grid_validation::validate_regular3d`] check. Covers
    /// unsupported grid kind, axis-vector length mismatch, and empty
    /// axis-grid — see [`GridValidationError`] variants for details.
    GridValidation(GridValidationError),
    /// A voxel in `mask.voxels` lies outside the SDF grid extent
    /// `[0, nx) × [0, ny) × [0, nz)`. Voxels outside this range would
    /// be silently unreachable in the corner lookup (the `mask_set`
    /// probe is only called for cell-corner indices inside the grid),
    /// hiding caller-side off-by-one or grid-swap bugs.
    MaskVoxelOutOfBounds {
        /// The offending voxel index.
        voxel: [i32; 3],
        /// The grid extent `[nx, ny, nz]` = `[axis_grids[i].len(); 3]`.
        grid_extent: [usize; 3],
    },
    /// The medial mask's grid geometry does not align with the SDF grid
    /// within `tolerance`. This guard prevents off-by-one-voxel masks from
    /// silently producing mid-surfaces with wrong thickness.
    MaskGridMismatch {
        /// SDF per-axis spacing (length-3).
        sdf_spacing: [f64; 3],
        /// Mask per-axis spacing (length-3).
        mask_spacing: [f64; 3],
        /// SDF `bounds_min` (length-3).
        sdf_origin: [f64; 3],
        /// Mask origin (length-3).
        mask_origin: [f64; 3],
        /// Tolerance used in the comparison.
        tolerance: f64,
    },
}

impl From<GridValidationError> for MidSurfaceError {
    fn from(e: GridValidationError) -> Self {
        MidSurfaceError::GridValidation(e)
    }
}

impl std::fmt::Display for MidSurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MidSurfaceError::GridValidation(inner) => write!(f, "extract_mid_surface: {inner}"),
            MidSurfaceError::MaskVoxelOutOfBounds { voxel, grid_extent } => write!(
                f,
                "medial mask voxel {voxel:?} is outside grid extent {grid_extent:?}; \
                 voxels must satisfy 0 ≤ i < nx, 0 ≤ j < ny, 0 ≤ k < nz"
            ),
            MidSurfaceError::MaskGridMismatch {
                sdf_spacing,
                mask_spacing,
                sdf_origin,
                mask_origin,
                tolerance,
            } => write!(
                f,
                "medial mask grid does not align with SDF grid within tolerance {tolerance}: \
                 sdf_spacing={sdf_spacing:?}, mask_spacing={mask_spacing:?}, \
                 sdf_origin={sdf_origin:?}, mask_origin={mask_origin:?}"
            ),
        }
    }
}

impl std::error::Error for MidSurfaceError {}

// ── Marching-cubes reference tables (Paul Bourke / public domain) ────────────

/// Maps each of the 12 cube edges to its two corner indices.
///
/// Corner numbering (same as Paul Bourke):
/// - bit 0 = +x, bit 1 = +y, bit 2 = +z
/// - Corner 0: (0,0,0), Corner 1: (1,0,0), Corner 2: (1,1,0), Corner 3: (0,1,0)
/// - Corner 4: (0,0,1), Corner 5: (1,0,1), Corner 6: (1,1,1), Corner 7: (0,1,1)
const EDGE_VERTICES: [[u8; 2]; 12] = [
    [0, 1], // edge  0: bottom-x
    [1, 2], // edge  1: bottom-y (right)
    [2, 3], // edge  2: bottom-x (far)
    [3, 0], // edge  3: bottom-y (left)
    [4, 5], // edge  4: top-x
    [5, 6], // edge  5: top-y (right)
    [6, 7], // edge  6: top-x (far)
    [7, 4], // edge  7: top-y (left)
    [0, 4], // edge  8: z (near-left)
    [1, 5], // edge  9: z (near-right)
    [2, 6], // edge 10: z (far-right)
    [3, 7], // edge 11: z (far-left)
];

/// Per-corner 3D offsets: `CORNER_OFFSETS[c] = (dx, dy, dz)` for corner `c`.
const CORNER_OFFSETS: [[u8; 3]; 8] = [
    [0, 0, 0], // corner 0
    [1, 0, 0], // corner 1
    [1, 1, 0], // corner 2
    [0, 1, 0], // corner 3
    [0, 0, 1], // corner 4
    [1, 0, 1], // corner 5
    [1, 1, 1], // corner 6
    [0, 1, 1], // corner 7
];

/// Standard 256-entry marching-cubes triangle table (Paul Bourke / public domain).
///
/// `TRI_TABLE[corner_mask]` lists edge-index triplets forming triangles,
/// terminated by `-1`. Each entry has at most 15 values (5 triangles × 3 edges
/// + sentinel).
///
/// The table is transcribed from Paul Bourke's canonical implementation
/// (https://paulbourke.net/geometry/polygonise/) which is the reference
/// implementation for marching cubes and is in the public domain.
#[rustfmt::skip]
const TRI_TABLE: [[i8; 16]; 256] = [
    [-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,8,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,1,9,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,8,3,9,8,1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,2,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,8,3,1,2,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [9,2,10,0,2,9,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [2,8,3,2,10,8,10,9,8,-1,-1,-1,-1,-1,-1,-1],
    [3,11,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,11,2,8,11,0,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,9,0,2,3,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,11,2,1,9,11,9,8,11,-1,-1,-1,-1,-1,-1,-1],
    [3,10,1,11,10,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,10,1,0,8,10,8,11,10,-1,-1,-1,-1,-1,-1,-1],
    [3,9,0,3,11,9,11,10,9,-1,-1,-1,-1,-1,-1,-1],
    [9,8,10,10,8,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [4,7,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [4,3,0,7,3,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,1,9,8,4,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [4,1,9,4,7,1,7,3,1,-1,-1,-1,-1,-1,-1,-1],
    [1,2,10,8,4,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [3,4,7,3,0,4,1,2,10,-1,-1,-1,-1,-1,-1,-1],
    [9,2,10,9,0,2,8,4,7,-1,-1,-1,-1,-1,-1,-1],
    [2,10,9,2,9,7,2,7,3,7,9,4,-1,-1,-1,-1],
    [8,4,7,3,11,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [11,4,7,11,2,4,2,0,4,-1,-1,-1,-1,-1,-1,-1],
    [9,0,1,8,4,7,2,3,11,-1,-1,-1,-1,-1,-1,-1],
    [4,7,11,9,4,11,9,11,2,9,2,1,-1,-1,-1,-1],
    [3,10,1,3,11,10,7,8,4,-1,-1,-1,-1,-1,-1,-1],
    [1,11,10,1,4,11,1,0,4,7,11,4,-1,-1,-1,-1],
    [4,7,8,9,0,11,9,11,10,11,0,3,-1,-1,-1,-1],
    [4,7,11,4,11,9,9,11,10,-1,-1,-1,-1,-1,-1,-1],
    [9,5,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [9,5,4,0,8,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,5,4,1,5,0,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [8,5,4,8,3,5,3,1,5,-1,-1,-1,-1,-1,-1,-1],
    [1,2,10,9,5,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [3,0,8,1,2,10,4,9,5,-1,-1,-1,-1,-1,-1,-1],
    [5,2,10,5,4,2,4,0,2,-1,-1,-1,-1,-1,-1,-1],
    [2,10,5,3,2,5,3,5,4,3,4,8,-1,-1,-1,-1],
    [9,5,4,2,3,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,11,2,0,8,11,4,9,5,-1,-1,-1,-1,-1,-1,-1],
    [0,5,4,0,1,5,2,3,11,-1,-1,-1,-1,-1,-1,-1],
    [2,1,5,2,5,8,2,8,11,4,8,5,-1,-1,-1,-1],
    [10,3,11,10,1,3,9,5,4,-1,-1,-1,-1,-1,-1,-1],
    [4,9,5,0,8,1,8,10,1,8,11,10,-1,-1,-1,-1],
    [5,4,0,5,0,11,5,11,10,11,0,3,-1,-1,-1,-1],
    [5,4,8,5,8,10,10,8,11,-1,-1,-1,-1,-1,-1,-1],
    [9,7,8,5,7,9,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [9,3,0,9,5,3,5,7,3,-1,-1,-1,-1,-1,-1,-1],
    [0,7,8,0,1,7,1,5,7,-1,-1,-1,-1,-1,-1,-1],
    [1,5,3,3,5,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [9,7,8,9,5,7,10,1,2,-1,-1,-1,-1,-1,-1,-1],
    [10,1,2,9,5,0,5,3,0,5,7,3,-1,-1,-1,-1],
    [8,0,2,8,2,5,8,5,7,10,5,2,-1,-1,-1,-1],
    [2,10,5,2,5,3,3,5,7,-1,-1,-1,-1,-1,-1,-1],
    [7,9,5,7,8,9,3,11,2,-1,-1,-1,-1,-1,-1,-1],
    [9,5,7,9,7,2,9,2,0,2,7,11,-1,-1,-1,-1],
    [2,3,11,0,1,8,1,7,8,1,5,7,-1,-1,-1,-1],
    [11,2,1,11,1,7,7,1,5,-1,-1,-1,-1,-1,-1,-1],
    [9,5,8,8,5,7,10,1,3,10,3,11,-1,-1,-1,-1],
    [5,7,0,5,0,9,7,11,0,1,0,10,11,10,0,-1],
    [11,10,0,11,0,3,10,5,0,8,0,7,5,7,0,-1],
    [11,10,5,7,11,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [10,6,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,8,3,5,10,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [9,0,1,5,10,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,8,3,1,9,8,5,10,6,-1,-1,-1,-1,-1,-1,-1],
    [1,6,5,2,6,1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,6,5,1,2,6,3,0,8,-1,-1,-1,-1,-1,-1,-1],
    [9,6,5,9,0,6,0,2,6,-1,-1,-1,-1,-1,-1,-1],
    [5,9,8,5,8,2,5,2,6,3,2,8,-1,-1,-1,-1],
    [2,3,11,10,6,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [11,0,8,11,2,0,10,6,5,-1,-1,-1,-1,-1,-1,-1],
    [0,1,9,2,3,11,5,10,6,-1,-1,-1,-1,-1,-1,-1],
    [5,10,6,1,9,2,9,11,2,9,8,11,-1,-1,-1,-1],
    [6,3,11,6,5,3,5,1,3,-1,-1,-1,-1,-1,-1,-1],
    [0,8,11,0,11,5,0,5,1,5,11,6,-1,-1,-1,-1],
    [3,11,6,0,3,6,0,6,5,0,5,9,-1,-1,-1,-1],
    [6,5,9,6,9,11,11,9,8,-1,-1,-1,-1,-1,-1,-1],
    [5,10,6,4,7,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [4,3,0,4,7,3,6,5,10,-1,-1,-1,-1,-1,-1,-1],
    [1,9,0,5,10,6,8,4,7,-1,-1,-1,-1,-1,-1,-1],
    [10,6,5,1,9,7,1,7,3,7,9,4,-1,-1,-1,-1],
    [6,1,2,6,5,1,4,7,8,-1,-1,-1,-1,-1,-1,-1],
    [1,2,5,5,2,6,3,0,4,3,4,7,-1,-1,-1,-1],
    [8,4,7,9,0,5,0,6,5,0,2,6,-1,-1,-1,-1],
    [7,3,9,7,9,4,3,2,9,5,9,6,2,6,9,-1],
    [3,11,2,7,8,4,10,6,5,-1,-1,-1,-1,-1,-1,-1],
    [5,10,6,4,7,2,4,2,0,2,7,11,-1,-1,-1,-1],
    [0,1,9,4,7,8,2,3,11,5,10,6,-1,-1,-1,-1],
    [9,2,1,9,11,2,9,4,11,7,11,4,5,10,6,-1],
    [8,4,7,3,11,5,3,5,1,5,11,6,-1,-1,-1,-1],
    [5,1,11,5,11,6,1,0,11,7,11,4,0,4,11,-1],
    [0,5,9,0,6,5,0,3,6,11,6,3,8,4,7,-1],
    [6,5,9,6,9,11,4,7,9,7,11,9,-1,-1,-1,-1],
    [10,4,9,6,4,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [4,10,6,4,9,10,0,8,3,-1,-1,-1,-1,-1,-1,-1],
    [10,0,1,10,6,0,6,4,0,-1,-1,-1,-1,-1,-1,-1],
    [8,3,1,8,1,6,8,6,4,6,1,10,-1,-1,-1,-1],
    [1,4,9,1,2,4,2,6,4,-1,-1,-1,-1,-1,-1,-1],
    [3,0,8,1,2,9,2,4,9,2,6,4,-1,-1,-1,-1],
    [0,2,4,4,2,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [8,3,2,8,2,4,4,2,6,-1,-1,-1,-1,-1,-1,-1],
    [10,4,9,10,6,4,11,2,3,-1,-1,-1,-1,-1,-1,-1],
    [0,8,2,2,8,11,4,9,10,4,10,6,-1,-1,-1,-1],
    [3,11,2,0,1,6,0,6,4,6,1,10,-1,-1,-1,-1],
    [6,4,1,6,1,10,4,8,1,2,1,11,8,11,1,-1],
    [9,6,4,9,3,6,9,1,3,11,6,3,-1,-1,-1,-1],
    [8,11,1,8,1,0,11,6,1,9,1,4,6,4,1,-1],
    [3,11,6,3,6,0,0,6,4,-1,-1,-1,-1,-1,-1,-1],
    [6,4,8,11,6,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [7,10,6,7,8,10,8,9,10,-1,-1,-1,-1,-1,-1,-1],
    [0,7,3,0,10,7,0,9,10,6,7,10,-1,-1,-1,-1],
    [10,6,7,1,10,7,1,7,8,1,8,0,-1,-1,-1,-1],
    [10,6,7,10,7,1,1,7,3,-1,-1,-1,-1,-1,-1,-1],
    [1,2,6,1,6,8,1,8,9,8,6,7,-1,-1,-1,-1],
    [2,6,9,2,9,1,6,7,9,0,9,3,7,3,9,-1],
    [7,8,0,7,0,6,6,0,2,-1,-1,-1,-1,-1,-1,-1],
    [7,3,2,6,7,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [2,3,11,10,6,8,10,8,9,8,6,7,-1,-1,-1,-1],
    [2,0,7,2,7,11,0,9,7,6,7,10,9,10,7,-1],
    [1,8,0,1,7,8,1,10,7,6,7,10,2,3,11,-1],
    [11,2,1,11,1,7,10,6,1,6,7,1,-1,-1,-1,-1],
    [8,9,6,8,6,7,9,1,6,11,6,3,1,3,6,-1],
    [0,9,1,11,6,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [7,8,0,7,0,6,3,11,0,11,6,0,-1,-1,-1,-1],
    [7,11,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [7,6,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [3,0,8,11,7,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,1,9,11,7,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [8,1,9,8,3,1,11,7,6,-1,-1,-1,-1,-1,-1,-1],
    [10,1,2,6,11,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,2,10,3,0,8,6,11,7,-1,-1,-1,-1,-1,-1,-1],
    [2,9,0,2,10,9,6,11,7,-1,-1,-1,-1,-1,-1,-1],
    [6,11,7,2,10,3,10,8,3,10,9,8,-1,-1,-1,-1],
    [7,2,3,6,2,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [7,0,8,7,6,0,6,2,0,-1,-1,-1,-1,-1,-1,-1],
    [2,7,6,2,3,7,0,1,9,-1,-1,-1,-1,-1,-1,-1],
    [1,6,2,1,8,6,1,9,8,8,7,6,-1,-1,-1,-1],
    [10,7,6,10,1,7,1,3,7,-1,-1,-1,-1,-1,-1,-1],
    [10,7,6,1,7,10,1,8,7,1,0,8,-1,-1,-1,-1],
    [0,3,7,0,7,10,0,10,9,6,10,7,-1,-1,-1,-1],
    [7,6,10,7,10,8,8,10,9,-1,-1,-1,-1,-1,-1,-1],
    [6,8,4,11,8,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [3,6,11,3,0,6,0,4,6,-1,-1,-1,-1,-1,-1,-1],
    [8,6,11,8,4,6,9,0,1,-1,-1,-1,-1,-1,-1,-1],
    [9,4,6,9,6,3,9,3,1,11,3,6,-1,-1,-1,-1],
    [6,8,4,6,11,8,2,10,1,-1,-1,-1,-1,-1,-1,-1],
    [1,2,10,3,0,11,0,6,11,0,4,6,-1,-1,-1,-1],
    [4,11,8,4,6,11,0,2,9,2,10,9,-1,-1,-1,-1],
    [10,9,3,10,3,2,9,4,3,11,3,6,4,6,3,-1],
    [8,2,3,8,4,2,4,6,2,-1,-1,-1,-1,-1,-1,-1],
    [0,4,2,4,6,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,9,0,2,3,4,2,4,6,4,3,8,-1,-1,-1,-1],
    [1,9,4,1,4,2,2,4,6,-1,-1,-1,-1,-1,-1,-1],
    [8,1,3,8,6,1,8,4,6,6,10,1,-1,-1,-1,-1],
    [10,1,0,10,0,6,6,0,4,-1,-1,-1,-1,-1,-1,-1],
    [4,6,3,4,3,8,6,10,3,0,3,9,10,9,3,-1],
    [10,9,4,6,10,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [4,9,5,7,6,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,8,3,4,9,5,11,7,6,-1,-1,-1,-1,-1,-1,-1],
    [5,0,1,5,4,0,7,6,11,-1,-1,-1,-1,-1,-1,-1],
    [11,7,6,8,3,4,3,5,4,3,1,5,-1,-1,-1,-1],
    [9,5,4,10,1,2,7,6,11,-1,-1,-1,-1,-1,-1,-1],
    [6,11,7,1,2,10,0,8,3,4,9,5,-1,-1,-1,-1],
    [7,6,11,5,4,10,4,2,10,4,0,2,-1,-1,-1,-1],
    [3,4,8,3,5,4,3,2,5,10,5,2,11,7,6,-1],
    [7,2,3,7,6,2,5,4,9,-1,-1,-1,-1,-1,-1,-1],
    [9,5,4,0,8,6,0,6,2,6,8,7,-1,-1,-1,-1],
    [3,6,2,3,7,6,1,5,0,5,4,0,-1,-1,-1,-1],
    [6,2,8,6,8,7,2,1,8,4,8,5,1,5,8,-1],
    [9,5,4,10,1,6,1,7,6,1,3,7,-1,-1,-1,-1],
    [1,6,10,1,7,6,1,0,7,8,7,0,9,5,4,-1],
    [4,0,10,4,10,5,0,3,10,6,10,7,3,7,10,-1],
    [7,6,10,7,10,8,5,4,10,4,8,10,-1,-1,-1,-1],
    [6,9,5,6,11,9,11,8,9,-1,-1,-1,-1,-1,-1,-1],
    [3,6,11,0,6,3,0,5,6,0,9,5,-1,-1,-1,-1],
    [0,11,8,0,5,11,0,1,5,5,6,11,-1,-1,-1,-1],
    [6,11,3,6,3,5,5,3,1,-1,-1,-1,-1,-1,-1,-1],
    [1,2,10,9,5,11,9,11,8,11,5,6,-1,-1,-1,-1],
    [0,11,3,0,6,11,0,9,6,5,6,9,1,2,10,-1],
    [11,8,5,11,5,6,8,0,5,10,5,2,0,2,5,-1],
    [6,11,3,6,3,5,2,10,3,10,5,3,-1,-1,-1,-1],
    [5,8,9,5,2,8,5,6,2,3,8,2,-1,-1,-1,-1],
    [9,5,6,9,6,0,0,6,2,-1,-1,-1,-1,-1,-1,-1],
    [1,5,8,1,8,0,5,6,8,3,8,2,6,2,8,-1],
    [1,5,6,2,1,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,3,6,1,6,10,3,8,6,5,6,9,8,9,6,-1],
    [10,1,0,10,0,6,9,5,0,5,6,0,-1,-1,-1,-1],
    [0,3,8,5,6,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [10,5,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [11,5,10,7,5,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [11,5,10,11,7,5,8,3,0,-1,-1,-1,-1,-1,-1,-1],
    [5,11,7,5,10,11,1,9,0,-1,-1,-1,-1,-1,-1,-1],
    [10,7,5,10,11,7,9,8,1,8,3,1,-1,-1,-1,-1],
    [11,1,2,11,7,1,7,5,1,-1,-1,-1,-1,-1,-1,-1],
    [0,8,3,1,2,7,1,7,5,7,2,11,-1,-1,-1,-1],
    [9,7,5,9,2,7,9,0,2,2,11,7,-1,-1,-1,-1],
    [7,5,2,7,2,11,5,9,2,3,2,8,9,8,2,-1],
    [2,5,10,2,3,5,3,7,5,-1,-1,-1,-1,-1,-1,-1],
    [8,2,0,8,5,2,8,7,5,10,2,5,-1,-1,-1,-1],
    [9,0,1,2,3,5,2,5,10,5,3,7,-1,-1,-1,-1],
    [8,2,9,8,9,7,2,10,9,5,9,3,10,3,9,-1],
    [1,3,5,3,7,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,8,7,0,7,1,1,7,5,-1,-1,-1,-1,-1,-1,-1],
    [9,0,3,9,3,5,5,3,7,-1,-1,-1,-1,-1,-1,-1],
    [9,8,7,5,9,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [5,8,4,5,10,8,10,11,8,-1,-1,-1,-1,-1,-1,-1],
    [5,0,4,5,11,0,5,10,11,11,3,0,-1,-1,-1,-1],
    [0,1,9,8,4,10,8,10,11,10,4,5,-1,-1,-1,-1],
    [10,11,4,10,4,5,11,3,4,9,4,1,3,1,4,-1],
    [2,5,1,2,8,5,2,11,8,4,5,8,-1,-1,-1,-1],
    [0,4,11,0,11,3,4,5,11,2,11,1,5,1,11,-1],
    [0,2,5,0,5,9,2,11,5,4,5,8,11,8,5,-1],
    [9,4,5,2,11,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [2,5,10,3,5,2,3,4,5,3,8,4,-1,-1,-1,-1],
    [5,10,2,5,2,4,4,2,0,-1,-1,-1,-1,-1,-1,-1],
    [3,10,2,3,5,10,3,8,5,4,5,8,0,1,9,-1],
    [5,10,2,5,2,4,1,9,2,9,4,2,-1,-1,-1,-1],
    [8,4,5,8,5,3,3,5,1,-1,-1,-1,-1,-1,-1,-1],
    [0,4,5,1,0,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [8,4,5,8,5,3,9,0,5,0,3,5,-1,-1,-1,-1],
    [9,4,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [4,11,7,4,9,11,9,10,11,-1,-1,-1,-1,-1,-1,-1],
    [0,8,3,4,9,7,9,11,7,9,10,11,-1,-1,-1,-1],
    [1,10,11,1,11,4,1,4,0,7,4,11,-1,-1,-1,-1],
    [3,1,4,3,4,8,1,10,4,7,4,11,10,11,4,-1],
    [4,11,7,9,11,4,9,2,11,9,1,2,-1,-1,-1,-1],
    [9,7,4,9,11,7,9,1,11,2,11,1,0,8,3,-1],
    [11,7,4,11,4,2,2,4,0,-1,-1,-1,-1,-1,-1,-1],
    [11,7,4,11,4,2,8,3,4,3,2,4,-1,-1,-1,-1],
    [2,9,10,2,7,9,2,3,7,7,4,9,-1,-1,-1,-1],
    [9,10,7,9,7,4,10,2,7,8,7,0,2,0,7,-1],
    [3,7,10,3,10,2,7,4,10,1,10,0,4,0,10,-1],
    [1,10,2,8,7,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [4,9,1,4,1,7,7,1,3,-1,-1,-1,-1,-1,-1,-1],
    [4,9,1,4,1,7,0,8,1,8,7,1,-1,-1,-1,-1],
    [4,0,3,7,4,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [4,8,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [9,10,8,10,11,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [3,0,9,3,9,11,11,9,10,-1,-1,-1,-1,-1,-1,-1],
    [0,1,10,0,10,8,8,10,11,-1,-1,-1,-1,-1,-1,-1],
    [3,1,10,11,3,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,2,11,1,11,9,9,11,8,-1,-1,-1,-1,-1,-1,-1],
    [3,0,9,3,9,11,1,2,9,2,11,9,-1,-1,-1,-1],
    [0,2,11,8,0,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [3,2,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [2,3,8,2,8,10,10,8,9,-1,-1,-1,-1,-1,-1,-1],
    [9,10,2,0,9,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [2,3,8,2,8,10,0,1,8,1,10,8,-1,-1,-1,-1],
    [1,10,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [1,3,8,9,1,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,9,1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [0,3,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
];

/// Extract a triangle mid-surface mesh from a medial mask.
///
/// # Errors
///
/// Returns [`MidSurfaceError`] if:
/// - `sdf.kind != Regular3D`
/// - Any of `sdf.bounds_min / bounds_max / spacing / axis_grids` has length ≠ 3
/// - Any `sdf.axis_grids[i]` is empty
/// - `mask.spacing[i]` or `mask.origin[i]` differs from the SDF values by more
///   than `options.grid_alignment_tolerance`
/// - `mask.voxels` contains an index outside `[0, nx) × [0, ny) × [0, nz)`
///   (see [`MidSurfaceError::MaskVoxelOutOfBounds`])
///
/// # Returns
///
/// An empty [`MidSurfaceMesh`] if `mask.voxels` is empty (after all validation
/// passes). Otherwise a triangulated mesh of the medial surface with
/// per-vertex through-thickness.
///
/// # Performance
///
/// This is a v0.4 skeleton implementation. The hot path is the per-cell
/// `mask_set` `HashSet` probe (8 corner lookups per cell), iterated over the
/// full `nx × ny × nz` SDF grid even when the medial mask is sparse. For
/// PRD-realistic grids (256³ ≈ 16 M cells) this dominates wall time.
///
/// The obvious optimisation — iterating only over cells adjacent to voxels in
/// `mask.voxels` — is deferred to a follow-up task once T3 / T4 land and
/// clarify the downstream latency budget. See the parallel note in
/// `medial::compute_medial_mask` which makes the same tradeoff for dense
/// voxel iteration.
///
/// # T2 → T3 shared-edge contract
///
/// Downstream T3 (`prune_branches`) canonicalises shared-edge midpoints by
/// quantising coordinates to `PruneOptions::grid_alignment_tolerance` (default
/// `1e-9`, mirroring [`MidSurfaceOptions::grid_alignment_tolerance`]).  Two
/// vertices whose coordinates round to the same integer grid cell at that
/// tolerance are merged into a single canonical index before edge-incidence
/// counting.
///
/// **Obligation for future T2 changes:** any change to the midpoint-emission
/// formula `(wa + wb) * 0.5` (see the binary-MC midpoint computation in the
/// per-cell loop) must keep shared-edge midpoints from adjacent cells within
/// `grid_alignment_tolerance` of each other.  If midpoints from adjacent cells
/// diverge beyond that tolerance, T3 mis-classifies the shared edges as
/// boundary edges and spuriously prunes the mesh body.  The regression test
/// `prune_branches_real_t2_adjacent_cells_pipeline_pins_body_survival` in
/// `pruning.rs` catches this failure mode end-to-end.
pub fn extract_mid_surface(
    sdf: &SampledField,
    mask: &MedialMask,
    options: &MidSurfaceOptions,
) -> Result<MidSurfaceMesh, MidSurfaceError> {
    // (1) Structural Regular3D validation: kind, axis-vector lengths, non-empty
    // axis grids. The `?` converts GridValidationError → MidSurfaceError via
    // the From impl above, preserving the existing variant names and PartialEq
    // contract for all callers.
    validate_regular3d(sdf)?;

    // (2) Mask must be aligned with the SDF grid within tolerance.
    let sdf_spacing = [sdf.spacing[0], sdf.spacing[1], sdf.spacing[2]];
    let sdf_origin = [sdf.bounds_min[0], sdf.bounds_min[1], sdf.bounds_min[2]];
    let tol = options.grid_alignment_tolerance;
    for i in 0..3 {
        if (sdf_spacing[i] - mask.spacing[i]).abs() > tol
            || (sdf_origin[i] - mask.origin[i]).abs() > tol
        {
            return Err(MidSurfaceError::MaskGridMismatch {
                sdf_spacing,
                mask_spacing: mask.spacing,
                sdf_origin,
                mask_origin: mask.origin,
                tolerance: tol,
            });
        }
    }

    // Grid extents used by both the voxel-bounds check and the main loop.
    // Safe: validate_regular3d (step 1) confirmed axis_grids[i] is non-empty.
    let nx = sdf.axis_grids[0].len();
    let ny = sdf.axis_grids[1].len();
    let nz = sdf.axis_grids[2].len();

    // (3) Validate mask voxel bounds before building the HashSet.
    // Any voxel index outside [0, nx) × [0, ny) × [0, nz) would be silently
    // unreachable in the corner-lookup loop below (cell corners only range
    // over [0, nx-1] × [0, ny-1] × [0, nz-1]). Surface it as a typed error
    // so callers can debug off-by-one mask construction and grid-swap bugs.
    // Mirrors the strict treatment of MaskGridMismatch (off-by-one alignment
    // is also rejected rather than silently absorbed).
    for &[vi, vj, vk] in &mask.voxels {
        // Check negativity first; only then cast to usize for the upper-bound
        // comparison — avoids any signed→unsigned narrowing that `nx as i32`
        // would introduce for pathological grids with n >= 2^31.
        if vi < 0
            || vj < 0
            || vk < 0
            || (vi as usize) >= nx
            || (vj as usize) >= ny
            || (vk as usize) >= nz
        {
            return Err(MidSurfaceError::MaskVoxelOutOfBounds {
                voxel: [vi, vj, vk],
                grid_extent: [nx, ny, nz],
            });
        }
    }

    // Short-circuit on empty mask after the bounds check (the loop above
    // is a no-op for empty masks, so the order is purely for clarity).
    if mask.voxels.is_empty() {
        return Ok(MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        });
    }

    // (4) Build a HashSet for O(1) corner lookups.
    let mask_set: HashSet<[i32; 3]> = mask.voxels.iter().copied().collect();

    let mut vertices: Vec<[f64; 3]> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut thickness: Vec<f64> = Vec::new();

    // (5) Iterate over each cube cell (i,j,k) → (i+1,j+1,k+1).
    for i in 0..nx.saturating_sub(1) {
        for j in 0..ny.saturating_sub(1) {
            for k in 0..nz.saturating_sub(1) {
                // Build 8-bit corner mask.
                let mut corner_mask: u8 = 0;
                for c in 0..8u8 {
                    let off = CORNER_OFFSETS[c as usize];
                    let ci = (i as i32) + (off[0] as i32);
                    let cj = (j as i32) + (off[1] as i32);
                    let ck = (k as i32) + (off[2] as i32);
                    if mask_set.contains(&[ci, cj, ck]) {
                        corner_mask |= 1 << c;
                    }
                }
                if corner_mask == 0 || corner_mask == 255 {
                    continue;
                }

                // Lookup triangulation.
                let tris = &TRI_TABLE[corner_mask as usize];
                let mut tri_idx = 0;
                while tri_idx + 2 < 16 && tris[tri_idx] >= 0 {
                    let e0 = tris[tri_idx] as usize;
                    let e1 = tris[tri_idx + 1] as usize;
                    let e2 = tris[tri_idx + 2] as usize;

                    let base = vertices.len() as u32;

                    for &edge in &[e0, e1, e2] {
                        let [ca, cb] = EDGE_VERTICES[edge];
                        let off_a = CORNER_OFFSETS[ca as usize];
                        let off_b = CORNER_OFFSETS[cb as usize];

                        // World coordinates of the two edge corners.
                        let wa = [
                            sdf.axis_grids[0][i + off_a[0] as usize],
                            sdf.axis_grids[1][j + off_a[1] as usize],
                            sdf.axis_grids[2][k + off_a[2] as usize],
                        ];
                        let wb = [
                            sdf.axis_grids[0][i + off_b[0] as usize],
                            sdf.axis_grids[1][j + off_b[1] as usize],
                            sdf.axis_grids[2][k + off_b[2] as usize],
                        ];
                        // Binary indicator: midpoint of the edge.
                        let vx = (wa[0] + wb[0]) * 0.5;
                        let vy = (wa[1] + wb[1]) * 0.5;
                        let vz = (wa[2] + wb[2]) * 0.5;
                        vertices.push([vx, vy, vz]);

                        // Per-vertex thickness: 2 × |φ(mask-corner voxel center)|.
                        // Pick the mask corner (bit is set in corner_mask).
                        //
                        // Binary-MC invariant: every edge emitted by TRI_TABLE has
                        // exactly one in-mask and one out-of-mask endpoint (because
                        // corner_mask == 0 and corner_mask == 255 are skipped above).
                        // The assert below fires only when TRI_TABLE is wrong (e.g.
                        // a transcription error emits an edge between two same-state
                        // corners), which would silently corrupt thickness values.
                        debug_assert_ne!(
                            (corner_mask >> ca) & 1,
                            (corner_mask >> cb) & 1,
                            "binary-MC invariant violated: edge {edge} (corners {ca},{cb}) \
                             has same mask state in corner_mask={corner_mask:#010b}"
                        );
                        let ia = (i as i32) + (off_a[0] as i32);
                        let ja = (j as i32) + (off_a[1] as i32);
                        let ka = (k as i32) + (off_a[2] as i32);
                        // Direct integer-index lookup at the mask corner —
                        // infallible by construction: corner indices lie in
                        // [0, nx)×[0, ny)×[0, nz) because (a) mask voxels were
                        // validated by MaskVoxelOutOfBounds above, so the in-mask
                        // corner is guaranteed in-bounds, and (b) the per-cell loop
                        // bounds (i < nx-1, off ∈ {0,1}) keep the out-of-mask
                        // corner in [0, nx) too.  Skips the 8-corner trilinear
                        // collapse that `sample_at_world` would perform at this
                        // grid-aligned point.
                        let mask_corner_idx = if mask_set.contains(&[ia, ja, ka]) {
                            [
                                i + off_a[0] as usize,
                                j + off_a[1] as usize,
                                k + off_a[2] as usize,
                            ]
                        } else {
                            [
                                i + off_b[0] as usize,
                                j + off_b[1] as usize,
                                k + off_b[2] as usize,
                            ]
                        };
                        let phi = sample_at_index(sdf, mask_corner_idx);
                        thickness.push(2.0 * phi.abs());
                    }

                    triangles.push([base, base + 1, base + 2]);
                    tri_idx += 3;
                }
            }
        }
    }

    Ok(MidSurfaceMesh {
        vertices,
        triangles,
        thickness,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid_validation::GridValidationError;
    use crate::medial::MedialMask;
    use reify_ir::value::{InterpolationKind, SampledField, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    // ── Fixture helpers ───────────────────────────────────────────────────────

    /// Build a trivial 1×1×1 Regular3D `SampledField` with the given
    /// scalar SDF value at the single voxel.
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

    /// Build a Regular1D SampledField (for rejection tests).
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

    /// Build a Regular2D SampledField over a 3×3 grid (for rejection tests).
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

    /// Build an analytic-slab Regular3D SampledField: `φ(x,y,z) = |z| - half_thickness`
    /// on an N×N×N grid centered at the origin with unit spacing.
    ///
    /// Mirrors `medial.rs`'s `slab_sdf_3d` test helper.
    fn slab_sdf_3d(half_thickness_voxels: f64, voxel_count: usize) -> SampledField {
        assert!(voxel_count >= 2, "slab grid needs ≥ 2 voxels per axis");
        let n = voxel_count;
        let spacing: f64 = 1.0;
        let half_extent = (n as f64 - 1.0) / 2.0;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;
        let axis_grid: Vec<f64> = (0..n)
            .map(|idx| bounds_min + (idx as f64) * spacing)
            .collect();
        let mut data = Vec::with_capacity(n * n * n);
        for &_x in &axis_grid {
            for &_y in &axis_grid {
                for &z in &axis_grid {
                    data.push(z.abs() - half_thickness_voxels);
                }
            }
        }
        SampledField {
            name: format!("slab-3d-h{half_thickness_voxels}-n{voxel_count}"),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![bounds_min, bounds_min, bounds_min],
            bounds_max: vec![bounds_max, bounds_max, bounds_max],
            spacing: vec![spacing, spacing, spacing],
            axis_grids: vec![axis_grid.clone(), axis_grid.clone(), axis_grid],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Build the centerline MedialMask for a z-slab on an N×N×N grid:
    /// every voxel `(i, j, center_k)` for `i, j ∈ 0..n`.
    fn centerline_mask(n: usize, sdf: &SampledField) -> MedialMask {
        let center_k = (n as i32 - 1) / 2;
        let voxels: Vec<[i32; 3]> = (0..n as i32)
            .flat_map(|i| (0..n as i32).map(move |j| [i, j, center_k]))
            .collect();
        MedialMask {
            spacing: [sdf.spacing[0], sdf.spacing[1], sdf.spacing[2]],
            origin: [sdf.bounds_min[0], sdf.bounds_min[1], sdf.bounds_min[2]],
            voxels,
        }
    }

    // ── Step 1: smoke test ────────────────────────────────────────────────────

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
        let mesh: MidSurfaceMesh = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("empty mask on valid 3D SDF should return empty mesh");
        assert!(
            mesh.vertices.is_empty() && mesh.triangles.is_empty() && mesh.thickness.is_empty(),
            "empty mask must produce empty mid-surface mesh"
        );

        // Compile-test: error type and wrapper variant are publicly named.
        let _: MidSurfaceError =
            MidSurfaceError::GridValidation(GridValidationError::EmptyAxisGrid { axis: 0 });
    }

    // ── Step 3: kind-check rejection tests ───────────────────────────────────

    /// 1D SampledField must be rejected with `UnsupportedGridKind`.
    #[test]
    fn extract_mid_surface_rejects_regular1d_grids() {
        let sdf = one_d_field();
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        };
        let err = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect_err("1D input must be rejected");
        assert_eq!(
            err,
            MidSurfaceError::GridValidation(GridValidationError::UnsupportedGridKind {
                found: SampledGridKind::Regular1D,
            })
        );
    }

    /// 2D SampledField must be rejected with `UnsupportedGridKind`.
    #[test]
    fn extract_mid_surface_rejects_regular2d_grids() {
        let sdf = two_d_field();
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        };
        let err = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect_err("2D input must be rejected");
        assert_eq!(
            err,
            MidSurfaceError::GridValidation(GridValidationError::UnsupportedGridKind {
                found: SampledGridKind::Regular2D,
            })
        );
    }

    // ── Step 5: structural-validation tests ──────────────────────────────────

    /// Regular3D field with 1-element bounds_min → `AxisLengthMismatch`.
    #[test]
    fn extract_mid_surface_rejects_axis_length_mismatch() {
        let mut sdf = minimal_3d_field();
        sdf.bounds_min = vec![0.0]; // length 1, not 3
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        };
        let err = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect_err("axis length mismatch must be rejected");
        assert_eq!(
            err,
            MidSurfaceError::GridValidation(GridValidationError::AxisLengthMismatch {
                bounds_min_len: 1,
                bounds_max_len: 3,
                spacing_len: 3,
                axis_grids_len: 3,
            })
        );
    }

    /// Regular3D field with `axis_grids[0] = []` → `EmptyAxisGrid { axis: 0 }`.
    #[test]
    fn extract_mid_surface_rejects_empty_axis_grid() {
        let mut sdf = minimal_3d_field();
        sdf.axis_grids[0] = vec![]; // empty first axis
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        };
        let err = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect_err("empty axis grid must be rejected");
        assert_eq!(
            err,
            MidSurfaceError::GridValidation(GridValidationError::EmptyAxisGrid { axis: 0 })
        );
    }

    // ── Step 7: mask-alignment tests ─────────────────────────────────────────

    /// Mask with wrong spacing → `MaskGridMismatch`.
    #[test]
    fn extract_mid_surface_rejects_mask_spacing_mismatch() {
        let sdf = minimal_3d_field();
        let mask = MedialMask {
            spacing: [2.0, 1.0, 1.0], // sdf.spacing = [1.0, 1.0, 1.0]
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        };
        let err = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect_err("mask spacing mismatch must be rejected");
        match err {
            MidSurfaceError::MaskGridMismatch {
                sdf_spacing,
                mask_spacing,
                sdf_origin,
                mask_origin,
                tolerance,
            } => {
                assert_eq!(sdf_spacing, [1.0, 1.0, 1.0]);
                assert_eq!(mask_spacing, [2.0, 1.0, 1.0]);
                assert_eq!(sdf_origin, [0.0, 0.0, 0.0]);
                assert_eq!(mask_origin, [0.0, 0.0, 0.0]);
                assert_eq!(tolerance, 1e-9);
            }
            other => panic!("expected MaskGridMismatch, got {other:?}"),
        }
    }

    /// Mask with wrong origin → `MaskGridMismatch`.
    #[test]
    fn extract_mid_surface_rejects_mask_origin_mismatch() {
        let sdf = minimal_3d_field(); // bounds_min = [0.0, 0.0, 0.0]
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [1.0, 0.0, 0.0], // differs from sdf.bounds_min[0] = 0.0
            voxels: vec![],
        };
        let err = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect_err("mask origin mismatch must be rejected");
        match err {
            MidSurfaceError::MaskGridMismatch { .. } => {}
            other => panic!("expected MaskGridMismatch, got {other:?}"),
        }
    }

    // ── MaskVoxelOutOfBounds rejection tests ─────────────────────────────────

    /// A voxel index [10, 0, 0] lies outside a 3×3×3 grid (nx=3) and must
    /// be rejected with `MaskVoxelOutOfBounds`.
    #[test]
    fn extract_mid_surface_rejects_mask_voxel_out_of_bounds_positive() {
        let sdf = minimal_3d_field(); // 3×3×3, nx=ny=nz=3
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![[10, 0, 0]],
        };
        let err = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect_err("out-of-bounds voxel must be rejected");
        assert_eq!(
            err,
            MidSurfaceError::MaskVoxelOutOfBounds {
                voxel: [10, 0, 0],
                grid_extent: [3, 3, 3],
            }
        );
    }

    /// A voxel index [-1, 0, 0] (negative i) lies outside the grid and must
    /// be rejected with `MaskVoxelOutOfBounds`.
    #[test]
    fn extract_mid_surface_rejects_mask_voxel_out_of_bounds_negative() {
        let sdf = minimal_3d_field(); // 3×3×3, nx=ny=nz=3
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![[-1, 0, 0]],
        };
        let err = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect_err("negative out-of-bounds voxel must be rejected");
        assert_eq!(
            err,
            MidSurfaceError::MaskVoxelOutOfBounds {
                voxel: [-1, 0, 0],
                grid_extent: [3, 3, 3],
            }
        );
    }

    // ── Step 9: slab-centerline MC test ──────────────────────────────────────

    /// Slab φ = |z| − 3 on a 17×17×17 grid with the centerline plane mask.
    ///
    /// Uses n=17 (odd) so the center voxel falls exactly at z=0 (phi = −3,
    /// thickness = 6). An even-N grid (n=16) puts the center between voxels,
    /// giving phi = −2.5 at the nearest voxel center and breaking the
    /// thickness assertion in step 11.
    ///
    /// Asserts: (a) non-empty triangles, (b) non-empty vertices, (c)
    /// `thickness.len() == vertices.len()`, (d) every index `< vertices.len()`.
    #[test]
    fn extract_mid_surface_on_slab_centerline_yields_non_empty_indexed_mesh() {
        let n = 17usize;
        let sdf = slab_sdf_3d(3.0, n);
        let mask = centerline_mask(n, &sdf);

        let mesh = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("slab centerline extract succeeds");

        // (a) non-empty triangles
        assert!(
            !mesh.triangles.is_empty(),
            "slab centerline must yield at least one triangle"
        );
        // (b) non-empty vertices
        assert!(
            !mesh.vertices.is_empty(),
            "slab centerline must yield at least one vertex"
        );
        // (c) thickness length matches vertices
        assert_eq!(
            mesh.thickness.len(),
            mesh.vertices.len(),
            "thickness.len() must equal vertices.len()"
        );
        // (d) all triangle indices valid
        let nv = mesh.vertices.len() as u32;
        for &[a, b, c] in &mesh.triangles {
            assert!(a < nv, "triangle index {a} >= vertices.len() {nv}");
            assert!(b < nv, "triangle index {b} >= vertices.len() {nv}");
            assert!(c < nv, "triangle index {c} >= vertices.len() {nv}");
        }
        // (e) all vertex z-coordinates lie near the analytic mid-surface (z ≈ 0).
        // On the slab fixture every emitted vertex is the midpoint of a z-aligned
        // edge that spans one non-mask and one mask voxel, so |z| ≤ 0.5 < 1.0.
        // A regression in axis ordering, corner-index permutation, or
        // `world_at_index` indexing would produce |z| ≥ 1.0 and trip this check.
        for &v in &mesh.vertices {
            assert!(
                v[2].abs() < 1.0,
                "slab mid-surface vertex z={:.6} should be near z=0 plane (|z| < 1.0)",
                v[2]
            );
        }
    }

    // ── Step 11: thickness accuracy test ─────────────────────────────────────

    /// Every per-vertex thickness on the slab fixture must be close to
    /// `2 × half_thickness × spacing = 6.0` and positive.
    ///
    /// Uses n=17 (odd, same as the step-9 fixture) so the center voxel is at
    /// z=0 where phi = −3 and thickness = 6.
    #[test]
    fn extract_mid_surface_per_vertex_thickness_matches_slab_full_thickness() {
        let n = 17usize;
        let half_thickness: f64 = 3.0;
        let spacing: f64 = 1.0;
        let expected = 2.0 * half_thickness * spacing; // 6.0

        let sdf = slab_sdf_3d(half_thickness, n);
        let mask = centerline_mask(n, &sdf);

        let mesh = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("slab centerline extract succeeds");

        assert!(
            !mesh.thickness.is_empty(),
            "thickness vector must be non-empty on a non-empty mask"
        );
        for &t in &mesh.thickness {
            assert!(
                t > 0.0,
                "all per-vertex thickness values must be positive (got {t})"
            );
            assert!(
                (t - expected).abs() < 0.5,
                "per-vertex thickness {t} is too far from expected {expected} \
                 (tolerance 0.5 voxel)"
            );
        }
    }

    // ── Bit-exact thickness contract test ────────────────────────────────────

    /// Every per-vertex thickness on the slab fixture must equal `6.0` within
    /// `1e-12` (effectively bit-exact for this analytic fixture).
    ///
    /// On `slab_sdf_3d(3.0, 17)` the center voxel is at z=0, where
    /// `φ = |0| − 3 = −3` and `thickness = 2 × |−3| = 6.0`.  All mask
    /// corners used for thickness sampling are grid-aligned to z=0, so both
    /// `sample_at_world` (trilinear at a grid-aligned point) and
    /// `sample_at_index` (direct lookup) must return `−3.0` bit-exactly.
    ///
    /// The `1e-12` tolerance (much tighter than the `0.5`-voxel slack in the
    /// existing `extract_mid_surface_per_vertex_thickness_matches_slab_full_thickness`
    /// test) pins the contract that thickness sampling cannot introduce
    /// interpolation error or silently substitute a fallback value.
    #[test]
    fn extract_mid_surface_per_vertex_thickness_is_bit_exact_on_grid_aligned_slab() {
        let n = 17usize;
        let expected = 6.0_f64; // 2 × |φ| = 2 × 3 = 6

        let sdf = slab_sdf_3d(3.0, n);
        let mask = centerline_mask(n, &sdf);

        let mesh = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("slab centerline extract must succeed");

        assert!(
            !mesh.thickness.is_empty(),
            "thickness must be non-empty on a non-empty mask"
        );
        for &t in &mesh.thickness {
            assert!(
                (t - expected).abs() < 1e-12,
                "per-vertex thickness {t} differs from expected {expected} by more than 1e-12; \
                 mask corner is grid-aligned so sampling must be bit-exact"
            );
        }
    }

    // ── Per-axis MaskVoxelOutOfBounds characterization tests ─────────────────

    /// Table-driven out-of-bounds test covering y and z axes (positive overflow
    /// and negative), complementing the existing x-axis tests.
    ///
    /// Pins all six conditions of the bounds AND-chain at
    /// `extract_mid_surface`'s voxel-validation loop. A regression dropping
    /// `vk < 0` or `(vj as usize) >= ny` would trip a named failing case.
    #[test]
    fn extract_mid_surface_rejects_mask_voxel_out_of_bounds_per_axis_table_driven() {
        // 3×3×3 grid → valid voxel indices are [0..3) on each axis.
        // Each case below violates exactly one axis/sign condition.
        let cases: &[([i32; 3], &str)] = &[
            ([0, 10, 0], "y-axis positive overflow"),
            ([0, -1, 0], "y-axis negative"),
            ([0, 0, 10], "z-axis positive overflow"),
            ([0, 0, -1], "z-axis negative"),
        ];
        for &(voxel, label) in cases {
            let sdf = minimal_3d_field(); // 3×3×3
            let mask = MedialMask {
                spacing: [1.0, 1.0, 1.0],
                origin: [0.0, 0.0, 0.0],
                voxels: vec![voxel],
            };
            let err = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
                .expect_err(&format!("{label}: out-of-bounds voxel must be rejected"));
            assert_eq!(
                err,
                MidSurfaceError::MaskVoxelOutOfBounds {
                    voxel,
                    grid_extent: [3, 3, 3],
                },
                "{label}: must produce MaskVoxelOutOfBounds"
            );
        }
    }

    // ── Step 13: defaults pin test ────────────────────────────────────────────

    /// Pin `MidSurfaceOptions::default()` field values.
    ///
    /// Pattern-destructures all fields (compile-time field-rename guard).
    #[test]
    fn mid_surface_options_defaults_pin_empirical_constants() {
        let MidSurfaceOptions {
            grid_alignment_tolerance,
        } = MidSurfaceOptions::default();
        assert_eq!(
            grid_alignment_tolerance, 1e-9,
            "grid_alignment_tolerance default must be 1e-9 (effectively bit-exact \
             for the internal producer pipeline)"
        );
    }
}
