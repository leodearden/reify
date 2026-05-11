//! A-posteriori volume mesh refinement driven by per-element size hints.
//!
//! PRD reference: `docs/prds/v0_4/a-posteriori-error-estimation.md` task #2.
//!
//! This module is the FEA-domain orchestrator that bridges the a-posteriori
//! error-indicator output (per-element size hints from `ZzIndicator`) to the
//! kernel-gmsh remesher ([`reify_kernel_gmsh::refine_volume_with_size_field`]).
//!
//! # Projection algorithm: per-element → per-vertex (min over incident elements)
//!
//! Gmsh's `SetSize` API assigns a target characteristic length to each surface
//! vertex. The error indicator produces per-*element* hints. The projection
//! uses a conservative `min` over all elements incident to each vertex: any
//! element that wants a smaller mesh wins at the shared vertex. A mean would
//! dilute the refinement signal at marked/unmarked boundaries.
//!
//! # Stub-build routing
//!
//! When the kernel-gmsh crate is compiled without libgmsh
//! (`cfg(not(has_gmsh))`), `refine_volume_with_size_field` returns a
//! `GeometryError::OperationFailed` message containing
//! [`reify_kernel_gmsh::STUB_UNAVAILABLE_MARKER`].  [`map_geometry_error`]
//! routes that to [`RefineError::GmshUnavailable`] so callers can distinguish
//! "no libgmsh in this build" from "libgmsh failed at runtime".

use std::fmt;

use reify_kernel_gmsh::MeshingOptions;
use reify_types::{ElementOrderTag, GeometryError, Mesh, VolumeMesh};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`refine_with_size_field`].
#[derive(Debug)]
pub enum RefineError {
    /// `size_hints.len()` does not match the element count of `volume_mesh`.
    SizeHintsLengthMismatch { got: usize, expected: usize },
    /// A size hint at the given index is `<= 0.0`.
    NonPositiveSize { index: usize, size: f64 },
    /// A size hint at the given index is non-finite (NaN or ±inf).
    NonFiniteSize { index: usize },
    /// The kernel-gmsh crate was compiled without libgmsh — no meshing
    /// is possible in this build.
    GmshUnavailable,
    /// The kernel-gmsh FFI call failed at runtime.
    Gmsh(GeometryError),
}

impl fmt::Display for RefineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefineError::SizeHintsLengthMismatch { got, expected } => write!(
                f,
                "size_hints length mismatch: got {got}, expected {expected} (one per element)"
            ),
            RefineError::NonPositiveSize { index, size } => write!(
                f,
                "size_hints[{index}] = {size} is non-positive; all hints must be > 0"
            ),
            RefineError::NonFiniteSize { index } => {
                write!(f, "size_hints[{index}] is non-finite (NaN or ±inf)")
            }
            RefineError::GmshUnavailable => {
                write!(f, "libgmsh is not available in this build")
            }
            RefineError::Gmsh(e) => write!(f, "gmsh FFI error: {e}"),
        }
    }
}

impl std::error::Error for RefineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RefineError::Gmsh(e) => Some(e),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Project per-element size hints onto per-vertex sizes via a conservative
/// `min` over incident elements.
///
/// Returns a `Vec<f64>` of length `volume_mesh.vertices.len() / 3`, where
/// entry `v` is the minimum of `per_element_sizes[e]` for all elements `e`
/// incident to vertex `v`.
///
/// Vertices not touched by any element receive `f64::INFINITY` — callers
/// should validate inputs so this does not occur in practice.
///
/// # Design note: why `min` instead of mean?
///
/// The min-projection ensures any element that wants a locally smaller mesh
/// wins at its shared vertices. A mean would dilute the refinement signal at
/// the boundary between a marked and an unmarked region.
pub fn project_per_element_sizes_to_vertices(
    volume_mesh: &VolumeMesh,
    per_element_sizes: &[f64],
) -> Vec<f64> {
    let n_verts = volume_mesh.vertices.len() / 3;
    let nodes_per_elem: usize = match volume_mesh.element_order {
        ElementOrderTag::P1 => 4,
        ElementOrderTag::P2 => 10,
    };

    let mut vertex_sizes = vec![f64::INFINITY; n_verts];

    for (elem_idx, chunk) in volume_mesh.tet_indices.chunks(nodes_per_elem).enumerate() {
        let size = per_element_sizes[elem_idx];
        for &v_idx in chunk {
            let v = v_idx as usize;
            if vertex_sizes[v] > size {
                vertex_sizes[v] = size;
            }
        }
    }

    vertex_sizes
}

/// Remesh the volume enclosed by `surface` using per-element size hints.
///
/// Validates `size_hints`, projects them to per-vertex sizes (via
/// [`project_per_element_sizes_to_vertices`]), then delegates to
/// [`reify_kernel_gmsh::refine_volume_with_size_field`].
///
/// # Arguments
///
/// * `surface` — the original closed surface boundary (same surface used to
///   produce `volume_mesh`; needed for full remesh from surface).
/// * `volume_mesh` — the current mesh providing element count and topology
///   for size-hint validation and projection.
/// * `size_hints` — one `f64 > 0` per element of `volume_mesh` (in element
///   order: `size_hints[e]` is the target characteristic edge length for tet
///   `e`). Pass the element-wise sizes derived from the Z-Z error indicator.
/// * `options` — forwarded to the kernel-gmsh mesher unchanged.
///
/// # Errors
///
/// Returns `RefineError::SizeHintsLengthMismatch` if
/// `size_hints.len() != element_count`, `RefineError::NonFiniteSize` on NaN
/// or ±∞, `RefineError::NonPositiveSize` on `<= 0`, or kernel errors on
/// Gmsh failures.
pub fn refine_with_size_field(
    _surface: &Mesh,
    volume_mesh: &VolumeMesh,
    size_hints: &[f64],
    _options: &MeshingOptions,
) -> Result<VolumeMesh, RefineError> {
    // Validate size_hints length.
    let nodes_per_elem: usize = match volume_mesh.element_order {
        ElementOrderTag::P1 => 4,
        ElementOrderTag::P2 => 10,
    };
    let n_elements = volume_mesh.tet_indices.len() / nodes_per_elem;
    if size_hints.len() != n_elements {
        return Err(RefineError::SizeHintsLengthMismatch {
            got: size_hints.len(),
            expected: n_elements,
        });
    }

    // Validate individual hint values.
    for (i, &s) in size_hints.iter().enumerate() {
        if !s.is_finite() {
            return Err(RefineError::NonFiniteSize { index: i });
        }
        if s <= 0.0 {
            return Err(RefineError::NonPositiveSize { index: i, size: s });
        }
    }

    // step-8 will replace this placeholder with the wired kernel call.
    Err(RefineError::Gmsh(GeometryError::OperationFailed(
        "refine_with_size_field: not yet wired to kernel-gmsh (step-8)".into(),
    )))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Map a `GeometryError` from the kernel-gmsh layer to a `RefineError`,
/// routing stub-build errors to [`RefineError::GmshUnavailable`].
///
/// The substring anchor is the `pub const STUB_UNAVAILABLE_MARKER` from
/// `reify_kernel_gmsh::mesh_profile_2d` — both this function and the stub
/// body in `refine_volume.rs` reference the same constant, so any reword of
/// the stub message goes through the constant and is caught here at compile
/// time.
///
/// This mirrors the `mesher::map_geometry_error` convention at
/// `crates/reify-solver-elastic/src/mesher.rs:535-544`.
pub(crate) fn map_geometry_error(err: GeometryError) -> RefineError {
    match &err {
        GeometryError::OperationFailed(msg)
            if msg.contains(reify_kernel_gmsh::STUB_UNAVAILABLE_MARKER) =>
        {
            RefineError::GmshUnavailable
        }
        _ => RefineError::Gmsh(err),
    }
}

// ---------------------------------------------------------------------------
// Unit tests (run in both stub and real builds)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn two_tet_bipyramid() -> VolumeMesh {
        // 5-vertex bipyramid:
        //   tet A = [0, 1, 2, 3]
        //   tet B = [0, 1, 2, 4]
        // Vertices 0,1,2,3 are in tet A; vertices 0,1,2,4 are in tet B.
        VolumeMesh {
            vertices: vec![
                0.0, 0.0, 0.0, // 0
                1.0, 0.0, 0.0, // 1
                0.0, 1.0, 0.0, // 2
                0.0, 0.0, 1.0, // 3
                0.0, 0.0, -1.0, // 4
            ],
            tet_indices: vec![
                0, 1, 2, 3, // tet A
                0, 1, 2, 4, // tet B
            ],
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    // ---- step-9 pins: map_geometry_error routing ----

    #[test]
    fn stub_marker_message_maps_to_gmsh_unavailable() {
        let stub_err = GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: {} in this build",
            reify_kernel_gmsh::STUB_UNAVAILABLE_MARKER,
        ));
        let mapped = map_geometry_error(stub_err);
        assert!(
            matches!(mapped, RefineError::GmshUnavailable),
            "stub marker must map to GmshUnavailable, got: {mapped:?}",
        );
    }

    #[test]
    fn non_stub_operation_failed_maps_to_gmsh_variant() {
        let runtime_err =
            GeometryError::OperationFailed("some runtime gmsh failure".into());
        let mapped = map_geometry_error(runtime_err);
        assert!(
            matches!(mapped, RefineError::Gmsh(_)),
            "non-stub OperationFailed must map to RefineError::Gmsh(_), got: {mapped:?}",
        );
    }
}
