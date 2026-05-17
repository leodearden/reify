//! NodeAttachment producer — B-rep attribution threading through the
//! surface→volume meshing path.
//!
//! Implements task γ (M-005) from PRD
//! `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.3: emit a
//! [`BoundaryAssociation`] alongside the produced [`VolumeMesh`], threading
//! caller-supplied per-input-vertex B-rep attribution through the HXT meshing
//! path.
//!
//! # Design
//!
//! All types and functions in this module are feature-gated behind
//! `#[cfg(feature = "mesh-morph")]` (applied at the `pub mod mesh_boundary`
//! declaration in `lib.rs`). The `#[cfg(has_gmsh)]`-gated orchestrating
//! function `mesh_surface_to_volume_with_attribution` is additionally gated on
//! `has_gmsh` because it calls `mesh_surface_to_volume_with_diagnostics` which
//! only exists in the real FFI build.

use reify_types::{BoundaryAssociation, GeometryHandleId, Mesh, NodeAttachment};

// ---------------------------------------------------------------------------
// Input type
// ---------------------------------------------------------------------------

/// Caller-supplied B-rep attribution to thread through the surface→volume
/// meshing path.
///
/// Mirrors the shape described in PRD `docs/prds/v0_3/mesh-morphing-phase-2.md`
/// §3.3 task γ (M-005). The caller constructs this from the OCCT
/// tessellation metadata it already holds; the producer does not reach back
/// into any geometry kernel.
///
/// # Snap-to-vertex override
///
/// `vertex_candidates` supplies `(handle, position)` pairs from OCCT's
/// `extract_vertices` + `vertex_point`. The producer overrides
/// `per_vertex[i]` with `NodeAttachment::OnVertex(handle)` when input vertex
/// `i` lies within `snap_tolerance` of a candidate position (Euclidean
/// distance, strict `<` comparison so `snap_tolerance = 0.0` disables
/// overrides entirely). First-match-wins; candidate ordering is
/// caller-controlled.
#[derive(Debug, Clone)]
pub struct BoundaryAttributionInput {
    /// Per-input-vertex B-rep attribution from the caller's tessellation
    /// metadata. Length must equal `surface.vertices.len() / 3`.
    pub per_vertex: Vec<NodeAttachment>,

    /// Snap-to-vertex candidates: `(vertex_handle, position)` from OCCT's
    /// `extract_vertices` + `vertex_point` (BRep_Tool::Pnt). Empty = no
    /// snap overrides applied.
    pub vertex_candidates: Vec<(GeometryHandleId, [f64; 3])>,

    /// Snap tolerance for the vertex-coincidence test (Euclidean distance).
    /// `0.0` disables all overrides (strict-less-than comparison).
    pub snap_tolerance: f64,
}

// ---------------------------------------------------------------------------
// Pure-Rust helper: compute_boundary_association
// ---------------------------------------------------------------------------

/// Build a [`BoundaryAssociation`] from caller-supplied per-vertex attribution,
/// applying the snap-to-vertex override pass.
///
/// # Snap-to-vertex pass
///
/// For each input vertex `i`, the surface position
/// `surface.vertices[i*3..i*3+3]` is compared (f32→f64 widening) against
/// every entry in `attribution.vertex_candidates`. If the Euclidean distance
/// is strictly less than `attribution.snap_tolerance`, `per_vertex[i]` is
/// overridden with `NodeAttachment::OnVertex(candidate_handle)`.
/// First-match-wins. `snap_tolerance = 0.0` disables all overrides.
///
/// # BTreeMap iteration order
///
/// The returned [`BoundaryAssociation`] iterates in ascending `node_index`
/// order (BTreeMap discipline, per `boundary.rs:51-58`). This is
/// load-bearing for FEA warm-start cache stability.
///
/// Interior tet nodes added by HXT (indices `>= attribution.per_vertex.len()`)
/// are correctly absent from the association — only surface input vertices
/// `0..N` are inserted.
pub fn compute_boundary_association(
    attribution: &BoundaryAttributionInput,
    surface: &Mesh,
) -> BoundaryAssociation {
    let n = attribution.per_vertex.len();
    let tol_sq = attribution.snap_tolerance * attribution.snap_tolerance;
    let mut ba = BoundaryAssociation::default();

    for i in 0..n {
        // Widen surface vertex position from f32 to f64 for the snap test.
        let x = surface.vertices[i * 3] as f64;
        let y = surface.vertices[i * 3 + 1] as f64;
        let z = surface.vertices[i * 3 + 2] as f64;

        // Snap-to-vertex override: walk candidates and take first match.
        // `snap_tolerance == 0.0` ⟹ `tol_sq == 0.0` ⟹ dist_sq < tol_sq is always
        // false (dist_sq ≥ 0.0), so the override is disabled — strict-less-than
        // is intentional per PRD §3.3 snap contract.
        let attachment = if tol_sq > 0.0 {
            let mut snapped = None;
            for &(handle, [cx, cy, cz]) in &attribution.vertex_candidates {
                let dx = x - cx;
                let dy = y - cy;
                let dz = z - cz;
                let dist_sq = dx * dx + dy * dy + dz * dz;
                if dist_sq < tol_sq {
                    snapped = Some(NodeAttachment::OnVertex(handle));
                    break; // first-match-wins; caller controls candidate ordering
                }
            }
            snapped.unwrap_or(attribution.per_vertex[i])
        } else {
            attribution.per_vertex[i]
        };

        ba.associate(i as u32, attachment);
    }

    ba
}
