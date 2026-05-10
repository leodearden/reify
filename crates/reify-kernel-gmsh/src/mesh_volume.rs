//! Orchestrating pipeline wrapper around `GmshKernel::mesh_to_volume`.
//!
//! This module composes three pure-Rust pre/post helpers around the FFI-backed
//! `GmshKernel::mesh_to_volume` call:
//!
//! 1. **Pre-stage**: `apply_repair_if_requested` — collapse slivers and merge
//!    near-coincident vertices before handing the surface mesh to gmsh.
//! 2. **Size resolution**: `resolve_mesh_size` — honour the caller's explicit
//!    `mesh_size`, or derive one via `auto_mesh_size_from_features`, or fall
//!    back to `mesh_to_volume`'s internal default.
//! 3. **Post-stage**: `compute_thickness_warnings` — check the produced volume
//!    mesh for under-resolved thin regions.
//!
//! The three helpers are unconditional (no `cfg(has_gmsh)` gate) so they
//! compile and are unit-testable in stub builds on hosts without libgmsh.
//!
//! The orchestrating wrapper `mesh_surface_to_volume_with_diagnostics` is
//! `cfg(has_gmsh)`-gated because it calls `GmshKernel::mesh_to_volume`, which
//! only exists in `kernel_real.rs`.

use std::borrow::Cow;

use reify_types::Mesh;

use crate::repair::{repair_surface_mesh, RepairConfig};
use crate::through_thickness::ThroughThicknessWarning;

// ---------------------------------------------------------------------------
// Output type
// ---------------------------------------------------------------------------

/// Output of [`mesh_surface_to_volume_with_diagnostics`].
///
/// Bundles the produced volume mesh with any through-thickness under-resolution
/// warnings collected by the post-stage. Callers that don't need the warnings
/// can simply destructure `report.volume`.
#[derive(Debug, Clone)]
pub struct MeshSurfaceToVolumeReport {
    /// The produced volume mesh (tetrahedral).
    pub volume: reify_types::VolumeMesh,
    /// Through-thickness under-resolution warnings from the post-stage.
    /// Empty when the post-stage was skipped (`thickness_cfg = None`) or when
    /// no under-resolved regions were found.
    pub through_thickness_warnings: Vec<ThroughThicknessWarning>,
}

// ---------------------------------------------------------------------------
// Pure-Rust helpers (unconditional — no cfg(has_gmsh) required)
// ---------------------------------------------------------------------------

/// Apply the repair pre-stage if requested, returning a `Cow<'_, Mesh>`.
///
/// - `None` — returns `Cow::Borrowed(input)` without any allocation or repair.
/// - `Some(cfg)` — delegates to `repair_surface_mesh(input, cfg)`, returning
///   `Cow::Owned(repaired)` and emitting a `tracing::debug!` event at the
///   `reify_kernel_gmsh::mesh_volume` target to record that repair fired.
///
/// Using `Cow` avoids cloning the potentially large surface mesh in the common
/// "skip repair" case — `cow.as_ref()` works for both arms downstream.
pub fn apply_repair_if_requested(input: &Mesh, cfg: Option<RepairConfig>) -> Cow<'_, Mesh> {
    match cfg {
        None => Cow::Borrowed(input),
        Some(c) => {
            tracing::debug!(
                target: "reify_kernel_gmsh::mesh_volume",
                "repair pre-stage applied"
            );
            Cow::Owned(repair_surface_mesh(input, c))
        }
    }
}
