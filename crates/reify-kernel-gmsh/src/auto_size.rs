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

use reify_types::Mesh;

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

/// Derive the auto-suggested `mesh_size` from the smallest triangle-edge
/// length in `mesh`.
///
/// Iterates every triangle in `mesh.indices` (chunks of 3), computes the
/// three edge lengths per triangle (Euclidean distance between the
/// referenced vertex positions), tracks the global minimum, and returns
/// `min_edge_length * cfg.feature_multiplier`.
///
/// Returns `0.0` when `mesh.indices` is empty (no triangles → no edges
/// → no minimum). Callers should treat a zero return as "auto-size
/// unavailable" and fall back to a configured default.
pub fn auto_mesh_size_from_features(mesh: &Mesh, cfg: AutoSizeConfig) -> f64 {
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
        return 0.0;
    }
    min_edge * cfg.feature_multiplier
}
