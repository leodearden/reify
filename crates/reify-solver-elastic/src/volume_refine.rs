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
use reify_ir::{ElementOrderTag, GeometryError, Mesh, VolumeMesh};

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
///
/// # Caller contract
///
/// The caller MUST validate
/// `per_element_sizes.len() == volume_mesh.tet_indices.len() / nodes_per_elem`
/// BEFORE invoking. The implementation indexes `per_element_sizes[elem_idx]`
/// without a bounds check; an out-of-bounds element will panic. The only
/// safe caller is [`refine_with_size_field`], which performs this length
/// validation up front (see lines 161-166).
///
/// The panic contract is pinned by the regression test
/// `project_panics_on_too_short_per_element_sizes` in the in-module `tests`
/// block — future authors who silently misbehave on short slices (e.g. via
/// `get(elem_idx).copied().unwrap_or(...)`) will see that test fail.
///
/// Visibility is `pub(crate)` to prevent external callers from misusing the
/// function with a short slice. The reviewer_comprehensive robustness
/// finding (option (a)) chose visibility narrowing over a `Result`-typed
/// length check; the up-front check in `refine_with_size_field` already
/// covers the validation duty for in-tree callers.
// At time of writing, consumed by same-file caller `refine_with_size_field`
// (~line 199). The G-tool flags same-file callers as orphans; the call
// site is live.
// G-allow: same-file consumer `refine_with_size_field` (G-tool same-file-caller heuristic limitation).
pub(crate) fn project_per_element_sizes_to_vertices(
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
// At time of writing, the intended production consumer is pending task
// #2997 (a-posteriori-error-estimation PRD #2 — "Refinement loop control
// + budget enforcement"). 2997's details reference calling this function
// as "task A4" (the Gmsh size-field driver). This function is task #2999
// (done); it landed ahead of its caller. Once 2997 lands the adaptive
// refinement loop in `reify-solver-elastic::adaptive`, verify a non-test
// caller of `refine_with_size_field` exists and this marker comes off.
// G-allow: producer for pending task #2997 (a-posteriori-error-estimation PRD #2: adaptive refinement loop).
pub fn refine_with_size_field(
    surface: &Mesh,
    volume_mesh: &VolumeMesh,
    size_hints: &[f64],
    options: &MeshingOptions,
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

    // Project per-element hints → per-volume-vertex sizes (conservative min).
    let vol_vertex_sizes = project_per_element_sizes_to_vertices(volume_mesh, size_hints);

    // Map per-volume-vertex sizes → per-surface-vertex sizes.
    //
    // The surface boundary vertices of `volume_mesh` correspond to the input
    // `surface` vertices (same positions, f32 coords).  For each surface
    // vertex we find the nearest volume-mesh vertex by squared-distance and
    // adopt its projected size.  This is O(n_surf × n_vol) but acceptable for
    // test-scale meshes; a spatial index would be needed for production-scale
    // refinement loops.
    let surface_vertex_sizes =
        project_volume_to_surface_vertices(surface, volume_mesh, &vol_vertex_sizes);

    // Delegate to the kernel-gmsh helper for the full-remesh with size hints.
    reify_kernel_gmsh::refine_volume_with_size_field(
        surface,
        &surface_vertex_sizes,
        options,
        volume_mesh.element_order,
    )
    .map_err(map_geometry_error)
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Map per-volume-mesh-vertex sizes to per-surface-vertex sizes via
/// nearest-neighbour coordinate matching.
///
/// The boundary vertices of `volume_mesh` are the same points as the surface
/// mesh vertices (both stored as f32 flat XYZ coords, same positions).  For
/// each surface vertex we scan all volume vertices and adopt the size of the
/// closest one.  The scan is O(n_surf × n_vol) — acceptable for test-scale
/// meshes (n_surf ≪ n_vol is typical); a spatial index is the right upgrade
/// if this path shows up in profiling.
///
/// If no volume vertex is found within a finite distance (shouldn't happen
/// for a well-formed surface/volume pair), the surface vertex receives the
/// global minimum of `vol_vertex_sizes` as a safe fallback.
fn project_volume_to_surface_vertices(
    surface: &Mesh,
    volume_mesh: &VolumeMesh,
    vol_vertex_sizes: &[f64],
) -> Vec<f64> {
    let n_surf = surface.vertices.len() / 3;
    let n_vol = volume_mesh.vertices.len() / 3;

    // Compute global minimum over FINITE sizes only.
    // `vol_vertex_sizes` may contain f64::INFINITY for volume vertices that
    // are not referenced by any tet element (orphaned surface/boundary nodes
    // produced by gmsh's classify_surfaces + create_geometry step). These
    // orphaned nodes must be excluded from the nearest-neighbour search so
    // the surface vertex sizes are not contaminated by the orphaned infinity.
    let finite_min = vol_vertex_sizes
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f64::INFINITY, f64::min);
    // Safe fallback: if somehow ALL vol_vertex_sizes are infinite, every
    // surface vertex receives f64::INFINITY too (signals a misconfiguration
    // upstream; callers are responsible for passing a well-formed volume mesh).
    let fallback = finite_min;

    let mut result = vec![fallback; n_surf];
    for (s, result_slot) in result.iter_mut().enumerate() {
        let sx = surface.vertices[s * 3];
        let sy = surface.vertices[s * 3 + 1];
        let sz = surface.vertices[s * 3 + 2];

        let mut best_dist_sq = f32::INFINITY;
        let mut best_size = fallback;
        for (v, &vol_size) in vol_vertex_sizes.iter().enumerate().take(n_vol) {
            // Skip orphaned nodes (not part of any tet) — they carry
            // f64::INFINITY and would pollute the result if chosen as the
            // nearest neighbour.
            if !vol_size.is_finite() {
                continue;
            }
            let vx = volume_mesh.vertices[v * 3];
            let vy = volume_mesh.vertices[v * 3 + 1];
            let vz = volume_mesh.vertices[v * 3 + 2];
            let dist_sq =
                (sx - vx) * (sx - vx) + (sy - vy) * (sy - vy) + (sz - vz) * (sz - vz);
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                best_size = vol_size;
            }
        }
        *result_slot = best_size;
    }
    result
}

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

    // ---- step-11 pins: project_per_element_sizes_to_vertices ----

    /// Conservative min projection over shared vertices.
    ///
    /// Two-tet bipyramid: vertices [0,1,2,3] in tet A (size 0.5), vertices
    /// [0,1,2,4] in tet B (size 1.0). Shared vertices 0..=2 take
    /// `min(0.5, 1.0) = 0.5`. Vertex 3 (only in A) stays 0.5. Vertex 4 (only
    /// in B) stays 1.0.
    ///
    /// Relocated from `tests/volume_refine_tests.rs` after step-12 restricted
    /// the projector to `pub(crate)` visibility.
    #[test]
    fn project_per_element_sizes_to_vertices_takes_min_over_incident_elements() {
        let vm = two_tet_bipyramid();
        let per_elem = [0.5_f64, 1.0_f64];

        let result = super::project_per_element_sizes_to_vertices(&vm, &per_elem);

        assert_eq!(
            result.len(),
            5,
            "returned slice must have length = n_vertices = 5"
        );
        assert_eq!(
            result,
            vec![0.5, 0.5, 0.5, 0.5, 1.0],
            "vertices 0-3 incident to tet A → min(0.5, 1.0) = 0.5; \
             vertex 4 only in tet B → stays 1.0"
        );
    }

    /// Caller contract: passing fewer `per_element_sizes` than the element
    /// count MUST panic (unguarded indexing).
    ///
    /// This pin documents the projector's caller-validation contract: the
    /// only safe caller is `refine_with_size_field`, which validates
    /// `size_hints.len() == n_elements` up front (see lines 161-166). Future
    /// authors who silently misbehave on short slices (e.g. via
    /// `get(elem_idx).copied().unwrap_or(...)`) will see this test fail and
    /// be forced to revisit the contract.
    #[test]
    fn project_panics_on_too_short_per_element_sizes() {
        let vm = two_tet_bipyramid(); // 2 tets
        let too_short = [0.5_f64]; // only 1 size for 2 elements

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::project_per_element_sizes_to_vertices(&vm, &too_short)
        }));
        assert!(
            result.is_err(),
            "project_per_element_sizes_to_vertices must panic on too-short \
             per_element_sizes (got 1, expected 2); contract is documented \
             as caller-validated indexing",
        );
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
