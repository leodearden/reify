//! Volume mesh refinement via Gmsh per-vertex size-field hints.
//!
//! PRD reference: `docs/prds/v0_4/a-posteriori-error-estimation.md` task #2.
//!
//! Exposes [`refine_volume_with_size_field`] with a uniform signature in both
//! `cfg(has_gmsh)` (real FFI) and `cfg(not(has_gmsh))` (stub) build modes —
//! mirrors the convention established by [`crate::mesh_profile_2d::mesh_plane_2d`].
//!
//! # Cache-invalidation contract
//!
//! Different `vertex_sizes` slices produce byte-distinct `VolumeMesh` outputs
//! (different tet counts and connectivity) so upstream cache keys — which are
//! keyed on all inputs — diverge automatically. No new cache-key field is
//! needed; the existing `volume_mesh_cache_key` derivation already covers this.
//!
//! # Cost basis: full remesh from surface
//!
//! `gmshModelMeshRefine()` refines uniformly across the entire existing mesh,
//! defeating localized-refinement requirements. Full remesh with per-vertex
//! `gmshModelMeshSetSize` and `Mesh.MeshSizeFromPoints=1` is the only Gmsh
//! path that honours localised size hints. This means every call regenerates
//! the entire volume mesh from the surface boundary.
//!
//! This is the explicit cost-basis the v0.4 PRD names as the trigger criterion
//! for the MMG3D bookmark (task #3003): if a refinement loop spends >30% of
//! wallclock in remeshing, swap to MMG3D.

use reify_types::{ElementOrderTag, GeometryError, Mesh, VolumeMesh};

use crate::options::MeshingOptions;

/// Remesh the volume enclosed by `surface` using per-vertex size hints.
///
/// `vertex_sizes[i]` is the target characteristic element edge length at
/// surface vertex `i` (same indexing as `surface.vertices / 3`). Every
/// surface vertex must have a hint; pass `vec![uniform_size; n_verts]` for
/// a uniform refinement.
///
/// The function performs a **full remesh** from the surface boundary rather
/// than incrementally refining the current volume mesh (see module-level doc
/// for the cost/accuracy rationale).
///
/// # Errors
///
/// `cfg(has_gmsh)`: returns `GeometryError::OperationFailed` on FFI failure
/// or if Gmsh produces no volume elements.
///
/// `cfg(not(has_gmsh))`: always returns `GeometryError::OperationFailed`
/// containing [`crate::STUB_UNAVAILABLE_MARKER`] — downstream callers
/// detect this via `msg.contains(STUB_UNAVAILABLE_MARKER)`.
// step-6 replaces this placeholder with the real FFI-backed implementation.
#[cfg(has_gmsh)]
pub fn refine_volume_with_size_field(
    _surface: &Mesh,
    _vertex_sizes: &[f64],
    _options: &MeshingOptions,
    _order: ElementOrderTag,
) -> Result<VolumeMesh, GeometryError> {
    Err(GeometryError::OperationFailed(
        "refine_volume_with_size_field: not yet implemented (placeholder — step-6 \
         will replace this with the full FFI-backed remesh)"
            .into(),
    ))
}

/// Stub-build companion: always returns `GeometryError::OperationFailed`
/// containing [`crate::STUB_UNAVAILABLE_MARKER`].
#[cfg(not(has_gmsh))]
pub fn refine_volume_with_size_field(
    _surface: &Mesh,
    _vertex_sizes: &[f64],
    _options: &MeshingOptions,
    _order: ElementOrderTag,
) -> Result<VolumeMesh, GeometryError> {
    Err(GeometryError::OperationFailed(format!(
        "refine_volume_with_size_field: {} in this build \
         (libgmsh not detected at build time)",
        crate::STUB_UNAVAILABLE_MARKER,
    )))
}
