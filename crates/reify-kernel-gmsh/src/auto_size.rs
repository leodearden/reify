//! Auto mesh-size derivation from the smallest geometric feature.
//!
//! Per the v0.3 FEA PRD's Tier-1 addition: the default `mesh_size` for a
//! volume mesh comes from the smallest geometric feature in the body, not
//! from the overall geometry tolerance or bounding-box diagonal.
//! Bounding-box-derived defaults under-resolve thin features by 5–10× —
//! the canonical failure case is a "thin slab embedded in a tall body"
//! where the slab thickness is 100× smaller than the body diagonal.
//!
//! # Surface-mesh approximation
//!
//! The PRD wording reads "smallest face dim / edge length × multiplier".
//! B-rep face dimensions live in the OCCT topology layer and are not
//! carried by the surface [`Mesh`] struct (which is a triangle soup with
//! `Vec<f32>` vertices and `Vec<u32>` indices, no face-tagged metadata).
//! This implementation uses the **smallest triangle-edge length** as the
//! surface-mesh-level approximation of the smallest geometric feature.
//! Engineering-equivalent: a thin slab's surface mesh has its shortest
//! triangle edges along the thickness direction, which is exactly the
//! dimension the auto-size heuristic needs to resolve.
//!
//! When B-rep face IDs eventually flow into surface-mesh metadata (after
//! topology selectors evolve), the function signature is stable enough to
//! swap implementations without changing callers.
//!
//! # Caller-provided override
//!
//! `MeshingOptions::mesh_size = Some(...)` overrides this auto-derived
//! default at the dispatcher level — this function returns only the
//! auto-suggested value.

use std::fmt;

use reify_ir::Mesh;

/// Configuration for the [`auto_mesh_size_from_features`] heuristic.
#[derive(Debug, Clone, Copy)]
pub struct AutoSizeConfig {
    /// Multiplier applied to the smallest triangle-edge length. `1.0`
    /// gives "one element per smallest feature"; `0.5` gives "two
    /// elements per smallest feature" (finer); `2.0` gives "half an
    /// element per smallest feature" (coarser, only useful for very
    /// permissive defaults).
    pub feature_multiplier: f64,
}

impl Default for AutoSizeConfig {
    fn default() -> Self {
        Self {
            feature_multiplier: 1.0,
        }
    }
}

/// Errors returned by [`auto_mesh_size_from_features`].
///
/// The variant carries structured fields so callers can surface diagnostics
/// without parsing message strings — mirrors the shape of
/// `reify_types::QueryError::NonFiniteParameter { u, v }`.
#[derive(Debug, Clone, PartialEq)]
pub enum AutoSizeError {
    /// A value in `mesh.indices` references a vertex slot beyond
    /// `mesh.vertices.len() / 3`.
    IndexOutOfBounds {
        /// The offending index value.
        index: u32,
        /// The vertex count (`mesh.vertices.len() / 3`) at call time.
        n_vertices: usize,
    },
}

impl fmt::Display for AutoSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutoSizeError::IndexOutOfBounds { index, n_vertices } => write!(
                f,
                "auto_mesh_size_from_features: index {index} is out of bounds \
                 for a mesh with {n_vertices} vertices (valid range 0..{n_vertices})"
            ),
        }
    }
}

impl std::error::Error for AutoSizeError {}

/// Derive the auto-suggested `mesh_size` from the smallest triangle-edge
/// length in `mesh`.
///
/// Iterates every triangle in `mesh.indices` (chunks of 3), computes the
/// three edge lengths per triangle (Euclidean distance between the
/// referenced vertex positions), tracks the global minimum, and returns
/// `Ok(min_edge_length * cfg.feature_multiplier)`.
///
/// Returns `Ok(0.0)` when `mesh.indices` is empty (no triangles → no edges
/// → no minimum). Callers should treat a zero return as "auto-size
/// unavailable" and fall back to a configured default.
///
/// # Errors
///
/// Returns [`AutoSizeError::IndexOutOfBounds`] if any value in
/// `mesh.indices` is ≥ `mesh.vertices.len() / 3`. The check is a single
/// up-front pass over all indices; the inner per-triangle loop keeps
/// unconditional indexing so the well-formed common case stays fast.
///
/// # Caller invariants
///
/// `mesh.vertices.len()` must be a multiple of 3 (one `(x, y, z)` triple
/// per vertex). This is not validated by the function — in practice all
/// upstream producers (OCCT tessellator, manifold adapter) emit
/// triplet-aligned buffers. A malformed buffer whose length is not divisible
/// by 3 will be silently treated as if the trailing partial coordinate did
/// not exist (`n_vertices = mesh.vertices.len() / 3` truncates).
pub fn auto_mesh_size_from_features(
    mesh: &Mesh,
    cfg: AutoSizeConfig,
) -> Result<f64, AutoSizeError> {
    if mesh.indices.is_empty() {
        return Ok(0.0);
    }

    // Single up-front validation pass: fail closed on any out-of-range index
    // before the inner loop runs. O(n) over mesh.indices; negligible compared
    // to the O(triangles × 3 edges × sqrt) main computation below.
    let n_vertices = mesh.vertices.len() / 3;
    for &idx in &mesh.indices {
        if idx as usize >= n_vertices {
            return Err(AutoSizeError::IndexOutOfBounds {
                index: idx,
                n_vertices,
            });
        }
    }

    let mut min_edge: f64 = f64::INFINITY;
    for tri in mesh.indices.chunks_exact(3) {
        let positions: [(f64, f64, f64); 3] = [
            (
                mesh.vertices[tri[0] as usize * 3] as f64,
                mesh.vertices[tri[0] as usize * 3 + 1] as f64,
                mesh.vertices[tri[0] as usize * 3 + 2] as f64,
            ),
            (
                mesh.vertices[tri[1] as usize * 3] as f64,
                mesh.vertices[tri[1] as usize * 3 + 1] as f64,
                mesh.vertices[tri[1] as usize * 3 + 2] as f64,
            ),
            (
                mesh.vertices[tri[2] as usize * 3] as f64,
                mesh.vertices[tri[2] as usize * 3 + 1] as f64,
                mesh.vertices[tri[2] as usize * 3 + 2] as f64,
            ),
        ];
        for (i, j) in [(0, 1), (1, 2), (0, 2)] {
            let (ax, ay, az) = positions[i];
            let (bx, by, bz) = positions[j];
            let dx = ax - bx;
            let dy = ay - by;
            let dz = az - bz;
            let len = (dx * dx + dy * dy + dz * dz).sqrt();
            if len < min_edge {
                min_edge = len;
            }
        }
    }
    if min_edge.is_infinite() {
        return Ok(0.0);
    }
    Ok(min_edge * cfg.feature_multiplier)
}
