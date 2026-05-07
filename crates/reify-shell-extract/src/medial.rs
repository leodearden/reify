//! Per-voxel medial-mask algorithm for thin-solid mid-surface extraction.
//!
//! Implements PRD task T1
//! (`docs/prds/v0_4/structural-analysis-shells.md`): for each active voxel
//! in a 3D narrow-band SDF, walk the SDF gradient in two opposing
//! directions to find the nearest surface points; tag the voxel as medial
//! iff the two distances are within `distance_tolerance` AND the two
//! surface-hit gradients are roughly antiparallel (encoding the gradient
//! discontinuity at the medial axis).

use reify_types::value::SampledField;

// Step-2 will introduce the public types (`MedialMask`, `MedialOptions`,
// `MedialError`) and the stub `compute_medial_mask` here.
//
// Without a placeholder use, the import would be flagged unused.
#[allow(dead_code)]
fn _force_use(_sdf: &SampledField) {}
