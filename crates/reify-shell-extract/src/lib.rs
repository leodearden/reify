//! `reify-shell-extract` — Voxel-medial mid-surface extraction for shell-element FEA.
//!
//! # PRD reference
//!
//! `docs/prds/v0_4/structural-analysis-shells.md` task **T1** (per-voxel
//! medial-mask algorithm). This crate identifies the voxels that lie on the
//! medial axis (mid-surface) of a thin solid by querying each active voxel's
//! nearest surface point in two opposing directions and tagging it as medial
//! iff:
//!
//! 1. opposing distances are within ~5%, AND
//! 2. the two surface-hit points are geometrically distinct — observable as
//!    antiparallel SDF gradients at the two hit points (the gradient
//!    discontinuity at the medial axis itself).
//!
//! The follow-up tasks T2 (mid-surface mesh extraction), T3 (branch pruning),
//! and T4 (region segmentation) build on this mask.
//!
//! # Dependency relationship
//!
//! Input is `&reify_ir::value::SampledField` (Regular3D narrow-band SDF).
//! The shipping `OpenVdbGridSource → SampledField` lowering pipeline in
//! `reify-kernel-openvdb::ingest::lower_to_sampled` is the eventual producer
//! once the OpenVDB FFI lands; until then, callers (and this crate's own
//! tests) construct `SampledField` instances directly from analytic SDFs.
//! This mirrors the `reify-solver-elastic` skeleton-crate template: ship the
//! algorithm against synthetic inputs, wire real producers in a follow-up.
//!
//! Output is a self-defined sparse [`MedialMask`] (`Vec<[i32; 3]>` of voxel
//! indices). The PRD permits `openvdb::BoolGrid OR EQUIVALENT`; a pure-Rust
//! sparse list is sufficient for downstream T2/T3/T4 consumers, all of which
//! iterate the mask voxels regardless of underlying storage. When the
//! OpenVDB FFI lands, the storage backing can be swapped behind the same
//! public API without changing T2/T3/T4 callers.
//!
//! # Branch-pruning smoke test (T3)
//!
//! ```
//! use reify_shell_extract::{prune_branches, PruneOptions, PruneResult, MidSurfaceMesh};
//!
//! let mesh = MidSurfaceMesh { vertices: vec![], triangles: vec![], thickness: vec![] };
//! let result: PruneResult =
//!     prune_branches(&mesh, &PruneOptions::default()).unwrap();
//! assert!(result.mesh.vertices.is_empty() && result.metrics.output_triangle_count == 0);
//! ```
//!
//! # Mid-surface mesher smoke test (T9)
//!
//! ```
//! use reify_shell_extract::{mesh_mid_surface, MesherOptions, MesherResult, MidSurfaceMesh};
//!
//! let mesh = MidSurfaceMesh { vertices: vec![], triangles: vec![], thickness: vec![] };
//! let result: MesherResult =
//!     mesh_mid_surface(&mesh, &MesherOptions::default()).unwrap();
//! assert!(result.mesh.vertices.is_empty() && result.metrics.triangle_count == 0);
//! ```
//!
//! # Region-segmentation smoke test
//!
//! ```
//! use reify_shell_extract::{
//!     segment_regions, MedialMask, MidSurfaceMesh,
//!     SegmentationError, SegmentationOptions, SegmentationResult, SingleBodyMask,
//! };
//!
//! let mask = MedialMask { spacing: [1.0, 1.0, 1.0], origin: [0.0, 0.0, 0.0], voxels: vec![] };
//! let mesh = MidSurfaceMesh { vertices: vec![], triangles: vec![], thickness: vec![] };
//! let single_body = SingleBodyMask::new(mask);
//! let result: SegmentationResult =
//!     segment_regions(&single_body, &mesh, &SegmentationOptions::default()).unwrap();
//! assert!(result.regions.is_empty() && result.vertex_labels.is_empty()
//!         && result.triangle_labels.is_empty());
//! let _: SegmentationError = SegmentationError::InvalidThreshold { value: 0.0 };
//! ```
//!
//! # Mid-surface extraction smoke test
//!
//! ```
//! use reify_shell_extract::{
//!     extract_mid_surface, GridValidationError, MedialMask, MidSurfaceError, MidSurfaceMesh,
//!     MidSurfaceOptions,
//! };
//! use reify_ir::value::{InterpolationKind, SampledField, SampledGridKind};
//! use std::sync::atomic::AtomicBool;
//!
//! let sdf = SampledField {
//!     name: "smoke-mid".to_string(),
//!     kind: SampledGridKind::Regular3D,
//!     bounds_min: vec![0.0, 0.0, 0.0],
//!     bounds_max: vec![0.0, 0.0, 0.0],
//!     spacing: vec![1.0, 1.0, 1.0],
//!     axis_grids: vec![vec![0.0], vec![0.0], vec![0.0]],
//!     interpolation: InterpolationKind::Linear,
//!     data: vec![1.0],
//!     oob_emitted: AtomicBool::new(false),
//! };
//! let mask = MedialMask { spacing: [1.0, 1.0, 1.0], origin: [0.0, 0.0, 0.0], voxels: vec![] };
//! let mesh: MidSurfaceMesh =
//!     extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default()).unwrap();
//! assert!(mesh.vertices.is_empty() && mesh.triangles.is_empty() && mesh.thickness.is_empty());
//! let _: MidSurfaceError =
//!     MidSurfaceError::GridValidation(GridValidationError::EmptyAxisGrid { axis: 0 });
//! let _: MidSurfaceError = MidSurfaceError::MaskVoxelOutOfBounds { voxel: [0, 0, 0], grid_extent: [1, 1, 1] };
//! ```
//!
//! # Medial-mask extraction smoke test
//!
//! ```
//! use reify_shell_extract::{
//!     GridValidationError, MedialError, MedialMask, MedialOptions, compute_medial_mask,
//! };
//! use reify_ir::value::{InterpolationKind, SampledField, SampledGridKind};
//! use std::sync::atomic::AtomicBool;
//!
//! // Trivial 1×1×1 grid with a single voxel at SDF = +1.0. The mask comes
//! // back empty because a 1×1×1 grid has identically-zero central-difference
//! // gradient (every axis collapses to a single sample), so the lone voxel
//! // is rejected by the GRADIENT_EPSILON degenerate-gradient filter — NOT
//! // by the narrow-band threshold (|φ|=1.0 is well inside the default
//! // 3-voxel band at unit spacing). This still smoke-tests the public
//! // surface end-to-end without invoking the algorithm body.
//! let sdf = SampledField {
//!     name: "smoke".to_string(),
//!     kind: SampledGridKind::Regular3D,
//!     bounds_min: vec![0.0, 0.0, 0.0],
//!     bounds_max: vec![0.0, 0.0, 0.0],
//!     spacing: vec![1.0, 1.0, 1.0],
//!     axis_grids: vec![vec![0.0], vec![0.0], vec![0.0]],
//!     interpolation: InterpolationKind::Linear,
//!     data: vec![1.0],
//!     oob_emitted: AtomicBool::new(false),
//! };
//! let mask: MedialMask = compute_medial_mask(&sdf, &MedialOptions::default()).unwrap();
//! assert!(mask.voxels.is_empty());
//! let _: MedialError =
//!     MedialError::GridValidation(GridValidationError::EmptyAxisGrid { axis: 0 });
//! ```

pub(crate) mod grid_validation;
pub mod medial;
pub mod mesher;
pub mod mid_surface;
pub mod mid_surface_naming;
pub mod partition;
pub mod pruning;
pub mod result;
pub mod segmentation;

pub use grid_validation::GridValidationError;
pub use medial::{MedialError, MedialMask, MedialOptions, compute_medial_mask};
pub use mesher::{MesherError, MesherOptions, MesherResult, QualityMetrics, mesh_mid_surface};
pub use mid_surface::{MidSurfaceError, MidSurfaceMesh, MidSurfaceOptions, extract_mid_surface};
pub use mid_surface_naming::{
    MidSurfaceAttributes, MidSurfaceEdgeRecord, populate_mid_surface_attributes,
};
pub use pruning::{PruneError, PruneMetrics, PruneOptions, PruneResult, prune_branches};
pub use result::{ShellExtractionResult, ShellExtractionResultError};
pub use segmentation::{
    RegionClassification, RegionInfo, SegmentationError, SegmentationOptions, SegmentationResult,
    SingleBodyMask, segment_regions,
};
