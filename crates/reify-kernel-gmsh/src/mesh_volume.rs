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

use reify_ir::Mesh;

use crate::auto_size::{AutoSizeConfig, auto_mesh_size_from_features};
use crate::options::MeshingOptions;
use crate::repair::{RepairConfig, repair_surface_mesh};
use crate::through_thickness::{
    ThroughThicknessConfig, ThroughThicknessWarning, through_thickness_check,
};

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
    pub volume: reify_ir::VolumeMesh,
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

/// Resolve the effective `mesh_size` to pass to `mesh_to_volume`.
///
/// Priority (highest first):
/// 1. **Caller-explicit** — `options.mesh_size` is `Some(s)`: returns `Ok(Some(s))`.
/// 2. **Auto-derived** — `auto_cfg` is `Some(cfg)`: calls
///    `auto_mesh_size_from_features(surface, cfg)`. A zero result is collapsed
///    to `Ok(None)` (auto returned "unavailable"; defer to the kernel default).
///    An `AutoSizeError` is surfaced as `GeometryError::OperationFailed`.
/// 3. **Kernel default** — both are `None`: returns `Ok(None)` and lets
///    `mesh_to_volume`'s internal logic decide.
///
/// Emits a single `tracing::debug!` event at the `reify_kernel_gmsh::mesh_volume`
/// target recording which branch fired (`source`) and the resolved value. The
/// event is suppressed on the error path (auto-size derivation failure).
pub fn resolve_mesh_size(
    surface: &Mesh,
    options: &MeshingOptions,
    auto_cfg: Option<AutoSizeConfig>,
) -> Result<Option<f64>, reify_ir::GeometryError> {
    // AutoSizeConfig: Copy — auto_cfg is not consumed by the match below.
    let result = match (options.mesh_size, auto_cfg) {
        (Some(s), _) => Ok(Some(s)),
        (None, None) => Ok(None),
        (None, Some(cfg)) => match auto_mesh_size_from_features(surface, cfg) {
            Ok(0.0) => Ok(None),
            Ok(v) => Ok(Some(v)),
            Err(e) => Err(reify_ir::GeometryError::OperationFailed(format!(
                "auto_mesh_size_from_features failed: {e}"
            ))),
        },
    };
    if let Ok(resolved) = result.as_ref() {
        // Determine which branch fired for structured diagnostics.
        let source = match (options.mesh_size, auto_cfg.is_some(), *resolved) {
            (Some(_), _, _) => "caller",
            (None, true, Some(_)) => "auto",
            (None, true, None) => "auto_collapsed_to_kernel_default",
            (None, false, _) => "kernel_default",
        };
        tracing::debug!(
            target: "reify_kernel_gmsh::mesh_volume",
            source = source,
            mesh_size = ?resolved,
            "mesh_size resolved"
        );
    }
    result
}

/// Run the through-thickness post-stage if requested.
///
/// - `None` — returns an empty `Vec` immediately (stage skipped).
/// - `Some(cfg)` — delegates to `through_thickness_check(volume, surface, cfg)`.
pub fn compute_thickness_warnings(
    volume: &reify_ir::VolumeMesh,
    surface: &Mesh,
    cfg: Option<ThroughThicknessConfig>,
) -> Vec<ThroughThicknessWarning> {
    match cfg {
        Some(c) => through_thickness_check(volume, surface, c),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// FFI-backed orchestrating wrapper (cfg(has_gmsh) only)
// ---------------------------------------------------------------------------

/// Compose repair, auto-size, volume meshing, and through-thickness diagnostics
/// into a single call.
///
/// Gated on `cfg(has_gmsh)` because it calls `GmshKernel::mesh_to_volume`,
/// which is only available in the real FFI build. For stub builds the three
/// pure helpers above remain testable.
///
/// # Stage order
///
/// 1. Repair (if `repair_cfg = Some(cfg)`) — modifies the surface mesh before
///    it is handed to gmsh.
/// 2. Size resolution — determines the effective `mesh_size` to use.
/// 3. `GmshKernel::mesh_to_volume` — produce the volume mesh.
/// 4. Through-thickness check (if `thickness_cfg = Some(cfg)`) — post-process
///    the produced volume mesh.
#[cfg(has_gmsh)]
// G-allow: §3.2 Gmsh tet-mesher producer per engine-integration-norm §3.2; consumer pending task #3429 (eval-side tet fall-back binding) / mesh-morph #2947
pub fn mesh_surface_to_volume_with_diagnostics(
    surface: &Mesh,
    options: &MeshingOptions,
    order: reify_ir::ElementOrderTag,
    repair_cfg: Option<RepairConfig>,
    auto_size_cfg: Option<AutoSizeConfig>,
    thickness_cfg: Option<ThroughThicknessConfig>,
) -> Result<MeshSurfaceToVolumeReport, reify_ir::GeometryError> {
    // Stage 1: repair pre-stage (Cow avoids clone when skipped)
    let repaired = apply_repair_if_requested(surface, repair_cfg);

    // Stage 2: resolve effective mesh size
    let resolved = resolve_mesh_size(repaired.as_ref(), options, auto_size_cfg)?;
    let inner_options = MeshingOptions {
        mesh_size: resolved,
        ..options.clone()
    };

    // Stage 3: volume meshing (FFI-backed)
    let kernel = crate::GmshKernel::new();
    let volume = kernel.mesh_to_volume(repaired.as_ref(), &inner_options, order)?;

    // Stage 4: through-thickness post-stage
    let through_thickness_warnings =
        compute_thickness_warnings(&volume, repaired.as_ref(), thickness_cfg);

    Ok(MeshSurfaceToVolumeReport {
        volume,
        through_thickness_warnings,
    })
}
