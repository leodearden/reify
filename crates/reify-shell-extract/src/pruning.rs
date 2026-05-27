//! Spurious-branch pruning on mid-surface meshes (PRD task T3).
//!
//! Detects and removes medial-surface branches whose length-to-local-thickness
//! ratio falls below a configurable threshold.
//!
//! # Algorithm overview
//!
//! A "spurious branch" is a thin protrusion at body edges or corners —
//! topologically a triangle (or short chain) attached to the main surface
//! body with ≥ 2 boundary edges (an edge incident to ≤ 1 triangle is a
//! boundary edge).  These are the leaves of the dual surface graph.
//!
//! Each iteration:
//! 1. Build an edge → incident-triangle count map (FxHashMap keyed on sorted
//!    vertex pair `[u32; 2]`).
//! 2. Identify "tip triangles": those with ≥ 2 boundary edges.
//! 3. For each tip triangle, compute:
//!    - `branch_length` = max edge length (Euclidean distance) of the three
//!      triangle edges — the "stem" of the protrusion.
//!    - `local_thickness` = mean of `mesh.thickness[v]` over the three
//!      vertices.
//! 4. Prune if `branch_length / local_thickness < shell_branch_prune_ratio`.
//! 5. Repeat until no more removals or `max_prune_iterations` is reached.
//! 6. Compact: drop orphan vertices and re-index `triangles` + `thickness`.
//!
//! **Complexity.** O(n_triangles × iterations) — computationally cheap and
//! deterministic.
//!
//! **v0.4 skeleton note.** The algorithm is shippable but defaults
//! (`shell_branch_prune_ratio = 1.0`, `max_prune_iterations = 8`) are
//! empirical placeholders pending real-corpus tuning, mirroring the language
//! used by `MidSurfaceOptions::grid_alignment_tolerance` and
//! `MesherOptions::min_aspect_ratio` rationale comments.

use rustc_hash::FxHashMap;

use crate::mesher::is_quantization_tolerance_valid;
use crate::mid_surface::MidSurfaceMesh;

// ── Public types ─────────────────────────────────────────────────────────────

/// Tunable parameters for [`prune_branches`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PruneOptions {
    /// Prune threshold: a tip triangle is removed when
    /// `branch_length / local_thickness < shell_branch_prune_ratio`.
    ///
    /// Must be strictly positive and finite.
    ///
    /// **Rationale.** PRD §89 defers this value ("Default ratio TBD
    /// empirically once the extractor is implemented"). `1.0` is the most
    /// conservative choice that still removes obvious edge/corner spikes — a
    /// triangle whose extent is less than the body's local thickness is by
    /// definition not a meaningful shell feature.  Documented as a v0.4
    /// empirical default pending real-corpus tuning.
    pub shell_branch_prune_ratio: f64,

    /// Maximum number of prune-iteration rounds.
    ///
    /// Must be ≥ 1. After each round, newly exposed tip triangles may qualify
    /// for removal; this bound prevents runaway behaviour on pathological
    /// meshes while still collapsing realistic chains.
    ///
    /// **Rationale.** A length-N boundary chain collapses in ≤ ⌊log₂ N⌋ ≈ 4
    /// rounds when adjacent tips disappear simultaneously; `8` doubles that
    /// bound for safety.  Mirrors `MesherOptions::max_remesh_iterations`
    /// (also a `u32` bound). Documented as v0.4 empirical default.
    pub max_prune_iterations: u32,

    /// Spatial-hash bin width for the canonical-vertex dedup step that
    /// recognises shared-edge midpoints emitted by T2 (`extract_mid_surface`).
    ///
    /// Two vertices whose coordinates all round to the same integer grid at
    /// this tolerance are merged into a single canonical index; edges between
    /// such merged vertices are counted as interior (incidence ≥ 2), not as
    /// boundary edges.  This is the **T2→T3 shared-edge contract**: T2 must
    /// emit shared-edge midpoints whose coordinates agree within this
    /// tolerance, or T3 will mis-classify those edges as boundary edges and
    /// spuriously prune the mesh body.
    ///
    /// Must be strictly positive and finite.
    ///
    /// **Rationale.** Default `1e-9` is effectively bit-exact for the internal
    /// producer pipeline: binary-MC emits midpoints from identical axis-grid
    /// lookups, so adjacent-cell shared edges produce bit-exact duplicates
    /// (minimum vertex separation `0.5 × min_spacing`, orders of magnitude
    /// larger than `1e-9`).  The default simultaneously admits float jitter
    /// from any future T2 variant that uses value-weighted interpolation,
    /// provided coordinates stay within `1e-9`.
    ///
    /// **Cross-references.**
    /// - `MidSurfaceOptions::grid_alignment_tolerance` (T2's matching default
    ///   `1e-9`): the two defaults are intentionally equal so callers who
    ///   loosen T2 can loosen T3 by the same amount.
    /// - `MesherOptions::merge_tolerance` (mesher.rs): the same
    ///   `(coord * inv_tol).round() as i64` dedup pattern with default `1e-9`.
    ///   Pruning's canonical-dedup is structurally identical — the precedent
    ///   establishes the documented NaN/±Inf saturation semantics shared by
    ///   both paths (see the COUPLING NOTE in `prune_branches`).
    ///
    /// **Caveat — upper end.** Validation rejects the lower-end pathologies
    /// (zero, negative, NaN, ±Inf, subnormal). Values much larger than the
    /// caller's maximum coordinate magnitude (e.g., `1e30` against unit-magnitude
    /// vertex coords) are accepted but reduce pruning to a no-op:
    /// `(coord * inv_tol).round() as i64` rounds every coordinate to `0`,
    /// collapsing every vertex into canonical index 0, every edge becomes
    /// interior (incidence ≥ 2), no triangle qualifies as a tip, and
    /// `prune_branches` leaves the input mesh unchanged. The upper end is left
    /// to the caller because a hard bound would require knowing the coordinate
    /// magnitude in advance; for the internal T2 pipeline (binary-MC midpoints
    /// in unit-cell coordinates) any tolerance below ~`1e-3` is safe. The
    /// symmetric LARGE-COORD failure mode (large coordinates against the default
    /// tolerance) is documented in the `# Preconditions` block on
    /// [`prune_branches`].
    pub grid_alignment_tolerance: f64,
}

impl Default for PruneOptions {
    fn default() -> Self {
        Self {
            // v0.4 empirical default — PRD §89 ("Default ratio TBD empirically
            // once the extractor is implemented"). 1.0 prunes branches shorter
            // than the local through-thickness, the most conservative artifact
            // threshold the medial-axis literature endorses.
            shell_branch_prune_ratio: 1.0,
            // v0.4 empirical default — ample for chain-collapse (length-17 chain
            // collapses in ≤ ⌊log₂ 17⌋ = 4 rounds; doubled for safety).
            max_prune_iterations: 8,
            // Matches MidSurfaceOptions::grid_alignment_tolerance and
            // MesherOptions::merge_tolerance (both 1e-9).  Effectively bit-exact
            // for the internal T2 pipeline; admits float jitter from future T2
            // variants.  See field doc for the T2→T3 shared-edge contract.
            grid_alignment_tolerance: 1e-9,
        }
    }
}

/// Per-run pruning metrics returned inside [`PruneResult`].
#[derive(Debug, Clone, PartialEq)]
pub struct PruneMetrics {
    /// Number of triangles in the input mesh.
    pub input_triangle_count: usize,
    /// Number of triangles in the output mesh (after pruning and compaction).
    pub output_triangle_count: usize,
    /// Total number of triangles removed across all iterations.
    pub pruned_triangle_count: usize,
    /// Number of vertices removed during compaction (orphan vertices).
    pub pruned_vertex_count: usize,
    /// Number of rounds in which at least one triangle was pruned.
    /// `0` if no triangles qualified for removal on the first pass —
    /// this occurs when there are no tip triangles, or when all tip
    /// triangles clear the ratio threshold.
    pub iterations: u32,
    /// `true` when the loop terminated naturally (a round saw `any_pruned ==
    /// false` and exited via the `break`), meaning no prunable tips remained.
    /// `false` when `max_prune_iterations` was hit while the final round was
    /// still actively pruning — callers should treat `false` as a signal that
    /// residual prunable tips may remain in the output mesh.
    pub converged: bool,
}

/// Output of a successful [`prune_branches`] call.
#[derive(Debug, Clone, PartialEq)]
pub struct PruneResult {
    /// The pruned and vertex-compacted mesh. Same type as the input
    /// ([`MidSurfaceMesh`]); the `Ok` return is the type invariant that all
    /// triangle indices are in-range and `thickness.len() == vertices.len()`.
    pub mesh: MidSurfaceMesh,
    /// Pruning metrics over this run.
    pub metrics: PruneMetrics,
}

/// Errors returned by [`prune_branches`].
#[derive(Debug, Clone, PartialEq)]
pub enum PruneError {
    /// `shell_branch_prune_ratio` must be finite and strictly positive.
    InvalidRatio {
        /// The offending value supplied by the caller.
        value: f64,
    },
    /// `max_prune_iterations` must be ≥ 1.
    InvalidMaxIterations {
        /// The offending value supplied by the caller.
        value: u32,
    },
    /// `grid_alignment_tolerance` must be strictly positive, finite, and normal
    /// (not subnormal).
    ///
    /// A zero or negative tolerance produces `inv_tol = 1/tol = ±Inf`, which
    /// saturates all non-zero coordinates to `i64::MAX`/`i64::MIN` and
    /// collapses them into one or two canonical buckets — every edge appears
    /// shared and tip detection degrades silently.  A NaN tolerance collapses
    /// every vertex into a single canonical index for the same reason.
    /// Subnormal values are also rejected: even the largest subnormals have
    /// reciprocals large enough (~2^1022) that `coord * inv_tol` overflows to
    /// ±Inf for any non-tiny coordinate, producing the same silent collapse.
    InvalidGridAlignmentTolerance {
        /// The offending value supplied by the caller.
        value: f64,
    },
    /// `mesh.thickness.len()` must equal `mesh.vertices.len()`.
    InconsistentInputMesh {
        /// Number of vertex positions in the mesh.
        vertices_len: usize,
        /// Number of thickness entries in the mesh.
        thickness_len: usize,
    },
    /// A triangle index references a vertex beyond `mesh.vertices.len()`.
    OutOfRangeTriangleIndex {
        /// Zero-based index of the offending triangle in `mesh.triangles`.
        triangle_index: usize,
        /// The vertex index that is out of range.
        vertex_index: u32,
        /// The total number of vertices in the mesh.
        vertices_len: usize,
    },
}

impl std::fmt::Display for PruneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PruneError::InvalidRatio { value } => write!(
                f,
                "shell_branch_prune_ratio must be finite and strictly positive \
                 (got {value}); use 1.0 for the conservative v0.4 default"
            ),
            PruneError::InvalidMaxIterations { value } => write!(
                f,
                "max_prune_iterations must be ≥ 1 (got {value}); zero would \
                 force a no-op even on prunable input"
            ),
            PruneError::InvalidGridAlignmentTolerance { value } => write!(
                f,
                "grid_alignment_tolerance must be strictly positive, finite, and normal \
                 (got {value}); subnormal values produce reciprocals that overflow vertex \
                 bin keys; use 1e-9 for the default (matches \
                 MidSurfaceOptions::grid_alignment_tolerance)"
            ),
            PruneError::InconsistentInputMesh {
                vertices_len,
                thickness_len,
            } => write!(
                f,
                "mesh.thickness.len() ({thickness_len}) ≠ mesh.vertices.len() \
                 ({vertices_len}); the two parallel arrays must be the same length"
            ),
            PruneError::OutOfRangeTriangleIndex {
                triangle_index,
                vertex_index,
                vertices_len,
            } => write!(
                f,
                "triangle {triangle_index} references vertex index {vertex_index} \
                 which is out of range (mesh has {vertices_len} vertices)"
            ),
        }
    }
}

impl std::error::Error for PruneError {}

// ── Public function ───────────────────────────────────────────────────────────

/// Prune spurious branches from a mid-surface mesh.
///
/// Iteratively removes "tip triangles" (those with ≥ 2 boundary edges) whose
/// `branch_length / local_thickness` ratio falls below
/// `options.shell_branch_prune_ratio`. Compacts orphan vertices after pruning.
///
/// # Errors
///
/// Returns `Err` if options are invalid or the input mesh is inconsistent.
/// See [`PruneError`] for all variants.
///
/// # Preconditions
///
/// The canonical-vertex dedup step computes bin keys via
/// `(coord * (1.0 / grid_alignment_tolerance)).round() as i64`; Rust's
/// `as` cast saturates on overflow, so coordinates with
/// `|coord / grid_alignment_tolerance| ≥ i64::MAX as f64` (~9.22e18)
/// saturate to `i64::MIN` / `i64::MAX`, producing spurious canonical-index
/// collisions and degrading tip detection.
///
/// At the default `grid_alignment_tolerance = 1e-9` the saturation
/// threshold is `|coord| ≈ 9.2e9`, far outside any practical CAD coordinate
/// range. Inputs from untrusted sources should validate that coordinates
/// are within reasonable magnitude before calling this function.
///
/// NaN and ±Inf vertex coordinates are not actively rejected here — NaN
/// coordinates round to `0` (collide at the origin bucket); ±Inf coordinates
/// saturate to `i64::MIN` / `i64::MAX`. These behaviours match
/// `mesher.rs::dedup_vertices` and are documented at the quantisation site
/// above.
pub fn prune_branches(
    mesh: &MidSurfaceMesh,
    options: &PruneOptions,
) -> Result<PruneResult, PruneError> {
    // ── 1. Validate options ───────────────────────────────────────────────────
    if options.shell_branch_prune_ratio <= 0.0 || !options.shell_branch_prune_ratio.is_finite() {
        return Err(PruneError::InvalidRatio {
            value: options.shell_branch_prune_ratio,
        });
    }
    if options.max_prune_iterations == 0 {
        return Err(PruneError::InvalidMaxIterations {
            value: options.max_prune_iterations,
        });
    }
    if !is_quantization_tolerance_valid(options.grid_alignment_tolerance) {
        return Err(PruneError::InvalidGridAlignmentTolerance {
            value: options.grid_alignment_tolerance,
        });
    }

    // ── 2. Validate input mesh ────────────────────────────────────────────────
    if mesh.thickness.len() != mesh.vertices.len() {
        return Err(PruneError::InconsistentInputMesh {
            vertices_len: mesh.vertices.len(),
            thickness_len: mesh.thickness.len(),
        });
    }
    for (tri_idx, tri) in mesh.triangles.iter().enumerate() {
        for &vi in tri.iter() {
            if vi as usize >= mesh.vertices.len() {
                return Err(PruneError::OutOfRangeTriangleIndex {
                    triangle_index: tri_idx,
                    vertex_index: vi,
                    vertices_len: mesh.vertices.len(),
                });
            }
        }
    }

    // ── 3. Empty-input short-circuit ──────────────────────────────────────────
    let input_triangle_count = mesh.triangles.len();
    if mesh.triangles.is_empty() {
        return Ok(PruneResult {
            mesh: MidSurfaceMesh {
                vertices: mesh.vertices.clone(),
                triangles: vec![],
                thickness: mesh.thickness.clone(),
            },
            metrics: PruneMetrics {
                input_triangle_count: 0,
                output_triangle_count: 0,
                pruned_triangle_count: 0,
                pruned_vertex_count: 0,
                iterations: 0,
                converged: true,
            },
        });
    }

    // ── 4. Prune iterations ───────────────────────────────────────────────────
    let mut triangles: Vec<[u32; 3]> = mesh.triangles.clone();
    let vertices = &mesh.vertices;
    let thickness = &mesh.thickness;
    let mut total_pruned: usize = 0;
    let mut iterations: u32 = 0;

    // T2 (extract_mid_surface) emits duplicate vertex positions for shared
    // edges (binary-MC midpoints are geometrically identical but stored at
    // separate indices).  Building the edge-incidence map over raw indices
    // would treat every edge as a boundary (incidence count 1), making every
    // triangle a tip and pruning the entire mesh.
    //
    // Solution: build a coordinate → canonical index map keyed on quantised
    // integer coordinates.  Edge-incidence is computed over canonical indices;
    // original indices are still used for vertex positions and thickness
    // lookups.
    //
    // COUPLING NOTE: T2 must emit shared-edge midpoints whose coordinates
    // agree within `options.grid_alignment_tolerance` (default 1e-9).  Two
    // vertices whose coordinates round to the same integer grid cell at that
    // tolerance are merged into a single canonical index.  This is effectively
    // bit-exact for the current internal T2 pipeline (binary-MC midpoints from
    // identical axis-grid lookups), while admitting small float jitter from any
    // future value-weighted T2 interpolation.
    //
    // Quantisation formula: `(coord * inv_tol).round() as i64`, with
    // `inv_tol = 1.0 / grid_alignment_tolerance` precomputed — same pattern as
    // `mesher.rs::dedup_vertices` (`MesherOptions::merge_tolerance`, default
    // 1e-9) which is the established precedent for vertex dedup in this crate.
    // Cross-reference: `MidSurfaceOptions::grid_alignment_tolerance` carries
    // the matching default 1e-9 at T2's end of the contract.
    //
    // NaN/Inf saturation: NaN coordinates round to `0` via
    // `f64::NAN.round() as i64 = 0` and collide at the origin.  ±Inf saturate
    // to `i64::MIN`/`i64::MAX`.  These edge cases match the mesher's documented
    // behaviour (mesher.rs:258–271) and are acceptable — the former bit-exact
    // code did not handle NaN differently either (NaN has many distinct bit
    // patterns).  The `−0.0`/`+0.0` sign-of-zero distinction falls out
    // automatically: `(-0.0_f64 * inv_tol).round() as i64 = 0`, same as for
    // `+0.0`.
    let canonical: Vec<u32> = {
        let inv_tol = 1.0 / options.grid_alignment_tolerance;
        let mut coord_to_canon: FxHashMap<[i64; 3], u32> = FxHashMap::default();
        vertices
            .iter()
            .map(|v| {
                let key = [
                    (v[0] * inv_tol).round() as i64,
                    (v[1] * inv_tol).round() as i64,
                    (v[2] * inv_tol).round() as i64,
                ];
                let next_id = coord_to_canon.len() as u32;
                *coord_to_canon.entry(key).or_insert(next_id)
            })
            .collect()
    };

    let mut converged = false;
    for _ in 0..options.max_prune_iterations {
        // Build edge → incident-triangle count map using canonical indices.
        // Key: sorted canonical vertex pair [u32; 2]; value: incident count.
        let mut edge_counts: FxHashMap<[u32; 2], u32> = FxHashMap::default();
        for tri in &triangles {
            let [a, b, c] = *tri;
            let ca = canonical[a as usize];
            let cb = canonical[b as usize];
            let cc = canonical[c as usize];
            for edge in [[ca, cb], [cb, cc], [ca, cc]] {
                let key = if edge[0] < edge[1] {
                    edge
                } else {
                    [edge[1], edge[0]]
                };
                *edge_counts.entry(key).or_insert(0) += 1;
            }
        }

        // Find tip triangles (≥ 2 boundary edges) and apply prune predicate.
        let ratio = options.shell_branch_prune_ratio;
        let mut pruned_in_round: Vec<bool> = vec![false; triangles.len()];
        let mut any_pruned = false;

        for (tri_idx, tri) in triangles.iter().enumerate() {
            let [a, b, c] = *tri;
            // Canonical indices for boundary-edge detection.
            let ca = canonical[a as usize];
            let cb = canonical[b as usize];
            let cc = canonical[c as usize];
            let edges = [
                sorted_pair(ca, cb),
                sorted_pair(cb, cc),
                sorted_pair(ca, cc),
            ];
            let boundary_count = edges
                .iter()
                .filter(|&&e| edge_counts.get(&e).copied().unwrap_or(0) == 1)
                .count();

            if boundary_count < 2 {
                continue; // Not a tip triangle.
            }

            // Compute branch_length = max edge length of the tip triangle.
            let va = vertices[a as usize];
            let vb = vertices[b as usize];
            let vc = vertices[c as usize];
            let lab = edge_length(va, vb);
            let lbc = edge_length(vb, vc);
            let lac = edge_length(va, vc);
            let branch_length = lab.max(lbc).max(lac);

            // local_thickness = mean thickness over the three vertices.
            let local_thickness =
                (thickness[a as usize] + thickness[b as usize] + thickness[c as usize]) / 3.0;

            // Non-positive or non-finite local_thickness is treated as
            // automatically prunable: the branch_length/thickness ratio is
            // conceptually infinite for any positive branch_length, so the
            // prune condition is always satisfied.  This avoids the opposite
            // failure mode of the rest of the validation (which prefers
            // explicit errors over silent fallback): silently retaining
            // degenerate zero-thickness tips as un-prunable.
            let prune = local_thickness <= 0.0
                || !local_thickness.is_finite()
                || branch_length / local_thickness < ratio;
            if prune {
                pruned_in_round[tri_idx] = true;
                any_pruned = true;
            }
        }

        if !any_pruned {
            converged = true;
            break;
        }

        // Remove pruned triangles.
        let before = triangles.len();
        let mut surviving: Vec<[u32; 3]> = Vec::with_capacity(before);
        for (idx, tri) in triangles.into_iter().enumerate() {
            if !pruned_in_round[idx] {
                surviving.push(tri);
            }
        }
        triangles = surviving;
        total_pruned += before - triangles.len();
        iterations += 1;
    }

    // ── 5. Vertex compaction ──────────────────────────────────────────────────
    let original_vertex_count = vertices.len();
    let mut live = vec![false; original_vertex_count];
    for tri in &triangles {
        for &vi in tri.iter() {
            live[vi as usize] = true;
        }
    }

    // Build remap: old index → new index (None if orphan).
    let mut remap: Vec<Option<u32>> = vec![None; original_vertex_count];
    let mut new_vertices: Vec<[f64; 3]> = Vec::new();
    let mut new_thickness: Vec<f64> = Vec::new();
    for (old_idx, &is_live) in live.iter().enumerate() {
        if is_live {
            let new_idx = new_vertices.len() as u32;
            remap[old_idx] = Some(new_idx);
            new_vertices.push(vertices[old_idx]);
            new_thickness.push(thickness[old_idx]);
        }
    }

    // Rewrite triangle indices.
    for tri in &mut triangles {
        for vi in tri.iter_mut() {
            *vi = remap[*vi as usize].expect("live vertex must have a remap entry");
        }
    }

    let pruned_vertex_count = original_vertex_count - new_vertices.len();
    let output_triangle_count = triangles.len();

    Ok(PruneResult {
        mesh: MidSurfaceMesh {
            vertices: new_vertices,
            triangles,
            thickness: new_thickness,
        },
        metrics: PruneMetrics {
            input_triangle_count,
            output_triangle_count,
            pruned_triangle_count: total_pruned,
            pruned_vertex_count,
            iterations,
            converged,
        },
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

#[inline]
fn sorted_pair(a: u32, b: u32) -> [u32; 2] {
    if a < b { [a, b] } else { [b, a] }
}

#[inline]
fn edge_length(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mid_surface::MidSurfaceMesh;

    // ── Steps 15-16: slab end-to-end pipeline test ───────────────────────────
    //
    // Test helpers duplicated from mid_surface.rs / mesher.rs.
    // Duplication is intentional: pruning.rs must be self-contained, mirroring
    // the established pattern between mid_surface.rs, segmentation.rs, and
    // mesher.rs (see mesher.rs:1121-1124 for the rationale comment).

    use crate::medial::MedialMask;
    use crate::mid_surface::{MidSurfaceOptions, extract_mid_surface};
    use reify_ir::value::{InterpolationKind, SampledField, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    /// Analytic-slab Regular3D SampledField: `φ(x,y,z) = |z| - half` on an
    /// N×N×N grid centred at the origin with unit spacing.
    fn slab_sdf_3d(half: f64, n: usize) -> SampledField {
        let spacing = 1.0_f64;
        let half_extent = (n as f64 - 1.0) / 2.0;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;
        let axis_grid: Vec<f64> = (0..n).map(|i| bounds_min + (i as f64) * spacing).collect();
        let mut data = Vec::with_capacity(n * n * n);
        for &_x in &axis_grid {
            for &_y in &axis_grid {
                for &z in &axis_grid {
                    data.push(z.abs() - half);
                }
            }
        }
        SampledField {
            name: format!("slab-prune-h{half}-n{n}"),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![bounds_min, bounds_min, bounds_min],
            bounds_max: vec![bounds_max, bounds_max, bounds_max],
            spacing: vec![spacing, spacing, spacing],
            axis_grids: vec![axis_grid.clone(), axis_grid.clone(), axis_grid],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Centerline MedialMask for a z-slab: every `(i, j, center_k)`.
    fn centerline_mask(n: usize, sdf: &SampledField) -> MedialMask {
        let center_k = (n as i32 - 1) / 2;
        let voxels: Vec<[i32; 3]> = (0..n as i32)
            .flat_map(|i| (0..n as i32).map(move |j| [i, j, center_k]))
            .collect();
        MedialMask {
            spacing: [sdf.spacing[0], sdf.spacing[1], sdf.spacing[2]],
            origin: [sdf.bounds_min[0], sdf.bounds_min[1], sdf.bounds_min[2]],
            voxels,
        }
    }

    /// Full T2 → T3 pipeline on a 17×17×17 slab.
    ///
    /// Validates that pruning returns `Ok(_)`, the slab body survives (triangles
    /// remain), metrics are consistent, all indices are in-range, and the
    /// parallel-array invariant `thickness.len() == vertices.len()` holds.
    ///
    /// Mirrors `mesh_mid_surface_slab_end_to_end_pipeline` (mesher.rs).
    #[test]
    fn prune_branches_slab_end_to_end_pipeline() {
        let sdf = slab_sdf_3d(3.0, 17);
        let mask = centerline_mask(17, &sdf);

        // T2: raw mid-surface extraction.
        let raw = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("slab extract_mid_surface should succeed");
        assert!(
            !raw.triangles.is_empty(),
            "17×17×17 slab must produce a non-empty raw mesh"
        );

        // T3: prune branches.
        let result = prune_branches(&raw, &PruneOptions::default())
            .expect("slab prune_branches should succeed");

        assert!(
            !result.mesh.triangles.is_empty(),
            "slab body must survive pruning"
        );
        assert_eq!(
            result.metrics.input_triangle_count,
            raw.triangles.len(),
            "input_triangle_count must equal raw triangle count"
        );
        assert!(
            result.metrics.output_triangle_count <= result.metrics.input_triangle_count,
            "output_triangle_count must not exceed input"
        );
        assert!(
            result.metrics.iterations <= PruneOptions::default().max_prune_iterations,
            "iterations must be within the configured bound"
        );
        assert!(
            result.metrics.converged,
            "slab prune must converge naturally within the iteration bound"
        );

        // Parallel-array invariant.
        assert_eq!(
            result.mesh.thickness.len(),
            result.mesh.vertices.len(),
            "thickness.len() must equal vertices.len() after pruning"
        );

        // All triangle indices in-range.
        let vlen = result.mesh.vertices.len();
        for tri in &result.mesh.triangles {
            for &vi in tri.iter() {
                assert!(
                    (vi as usize) < vlen,
                    "triangle index {vi} out of range for {vlen} vertices"
                );
            }
        }
    }

    // ── Amendment: duplicate-vertex regression test (suggestion 1) ──────────────
    //
    // Regression guard: simulates T2's duplicate-vertex output where the
    // shared-edge vertices are stored at different indices with identical
    // coordinates.  The canonical-dedup must merge them so the shared edge is
    // recognised as internal (incidence count 2), not two separate boundary
    // edges (which would make both triangles appear as tips).
    //
    // If T2 is ever changed so that midpoint coordinates are no longer
    // bit-exact duplicates (e.g. via averaging), the canonical-dedup silently
    // fails.  This test would catch that regression if the body happened to
    // be pruned — though the body's ratio (≈10) is well above the default
    // threshold, so what the test primarily validates is correct spike pruning
    // in the presence of duplicate-index topology.

    /// A mesh whose shared-edge vertices are stored at duplicate positions and
    /// different indices (as T2 emits) must still correctly prune the spike.
    ///
    /// Layout: body = [v0, v1, v2], spike = [v3, v4, v5],
    /// where v3 == v0 and v4 == v1 (same coordinates, different indices).
    /// The canonical-dedup merges v0/v3 and v1/v4 so edge (v0,v1)~(v3,v4)
    /// is counted as an internal edge with incidence 2.
    #[test]
    fn prune_branches_handles_duplicate_vertex_positions() {
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],   // v0 — body side of shared edge
                [0.5, 0.0, 0.0],   // v1 — body side of shared edge
                [0.25, 10.0, 0.0], // v2 — body apex
                [0.0, 0.0, 0.0],   // v3 == v0 duplicate (spike side)
                [0.5, 0.0, 0.0],   // v4 == v1 duplicate (spike side)
                [0.25, -0.1, 0.0], // v5 — spike apex
            ],
            triangles: vec![[0, 1, 2], [3, 4, 5]],
            thickness: vec![1.0, 1.0, 1.0, 1.0, 1.0, 10.0],
        };
        let result = prune_branches(&mesh, &PruneOptions::default())
            .expect("mesh with duplicate-position vertices should not error");
        // spike [3,4,5]: branch_length=0.5, local_t≈4.0 → ratio≈0.125 < 1.0 → pruned
        // body  [0,1,2]: branch_length≈10,  local_t=1.0 → ratio≈10   >> 1.0 → survives
        assert_eq!(
            result.mesh.triangles.len(),
            1,
            "body triangle must survive when spike has duplicate-position vertices"
        );
        assert_eq!(
            result.metrics.pruned_triangle_count, 1,
            "spike triangle must be pruned"
        );
        assert_eq!(
            result.mesh.vertices.len(),
            3,
            "only the three body vertices survive (v0, v1, v2)"
        );
        assert!(
            result.metrics.converged,
            "duplicate-vertex fixture must converge naturally"
        );
    }

    // ── Step 13: vertex-compaction test ──────────────────────────────────────

    /// After pruning the spike triangle, the spike apex vertex must be removed
    /// (compacted) and all triangle indices must remain in-range.
    ///
    /// Reuses the body+spike fixture from step 11.
    #[test]
    fn prune_branches_compacts_orphan_vertices_after_pruning() {
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],   // v0, thickness 1.0
                [0.5, 0.0, 0.0],   // v1, thickness 1.0
                [0.25, 10.0, 0.0], // v2, thickness 1.0 — body apex
                [0.25, -0.1, 0.0], // v3, thickness 10.0 — spike apex (orphan after pruning)
            ],
            triangles: vec![[0, 1, 2], [0, 1, 3]],
            thickness: vec![1.0, 1.0, 1.0, 10.0],
        };
        let result =
            prune_branches(&mesh, &PruneOptions::default()).expect("valid mesh should not error");

        // Spike apex (v3) must be gone.
        assert_eq!(result.mesh.vertices.len(), 3, "only 3 vertices survive");
        assert_eq!(
            result.mesh.thickness.len(),
            result.mesh.vertices.len(),
            "thickness parallel-array must be same length as vertices"
        );
        assert_eq!(result.metrics.pruned_vertex_count, 1, "one orphan removed");

        // Every triangle index must be in-range.
        for tri in &result.mesh.triangles {
            for &vi in tri.iter() {
                assert!(
                    (vi as usize) < result.mesh.vertices.len(),
                    "all triangle indices must be in-range after compaction"
                );
            }
        }

        // Body triangle thickness values: v0=1.0, v1=1.0, v2=1.0.
        // After compaction the body vertices are re-indexed 0..3 in original order.
        // All three should have thickness 1.0.
        for &t in &result.mesh.thickness {
            assert!(
                (t - 1.0).abs() < 1e-12,
                "surviving vertices all have thickness 1.0, got {t}"
            );
        }
        assert!(
            result.metrics.converged,
            "vertex-compaction fixture must converge naturally"
        );
    }

    // ── Step 11: prune-spike test ─────────────────────────────────────────────

    /// Two-triangle fixture: body survives, spike is pruned.
    ///
    /// Topology:
    /// - `[0,1,2]` — body (tall, thin vertices, ratio ≫ 1.0 → survives).
    /// - `[0,1,3]` — spike (stubby, thick vertex, ratio ≪ 1.0 → pruned).
    ///
    /// Shared edge is `(0,1)` with length 0.5.
    /// Spike branch_length = 0.5, local_thickness ≈ 4.0, ratio ≈ 0.125 < 1.0 → removed.
    /// Body branch_length ≈ 10.0, local_thickness = 1.0, ratio ≈ 10 ≫ 1.0 → retained.
    #[test]
    fn prune_branches_removes_short_spike_triangle() {
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],   // v0, thickness 1.0
                [0.5, 0.0, 0.0],   // v1, thickness 1.0 — shared edge (0,1) = 0.5
                [0.25, 10.0, 0.0], // v2, thickness 1.0 — body apex (tall, not pruned)
                [0.25, -0.1, 0.0], // v3, thickness 10.0 — spike apex (short, high t)
            ],
            triangles: vec![
                [0, 1, 2], // body: longest edge ≈10.0, local_t=1.0 → ratio≈10 → survives
                [0, 1, 3], // spike: longest edge=0.5, local_t≈4.0 → ratio≈0.125 → pruned
            ],
            thickness: vec![1.0, 1.0, 1.0, 10.0],
        };
        let result =
            prune_branches(&mesh, &PruneOptions::default()).expect("valid mesh should not error");
        assert_eq!(
            result.metrics.pruned_triangle_count, 1,
            "exactly one triangle (the spike) must be pruned"
        );
        assert_eq!(
            result.mesh.triangles.len(),
            1,
            "one triangle must survive (the body)"
        );
        // The surviving triangle's vertices must be a subset of the original body triangle.
        // After compaction v3 is gone; v0, v1, v2 survive (possibly re-indexed).
        assert_eq!(
            result.mesh.vertices.len(),
            3,
            "three vertices survive (body triangle)"
        );
        assert!(result.metrics.iterations >= 1, "at least one iteration ran");
        assert!(
            result.metrics.converged,
            "spike fixture must converge naturally after spike is pruned"
        );
    }

    // ── Amendment: threshold-straddling tests (suggestion 5) ─────────────────
    //
    // These tests ensure the prune predicate `branch_length / local_thickness < ratio`
    // is correctly sensitive to the configured threshold.  The spike fixture has
    // ratio exactly SPIKE_RATIO (1.0); testing at `SPIKE_RATIO + 0.05` and
    // `SPIKE_RATIO − 0.05` directly straddles the fixture boundary.  A regression
    // that flipped `<` to `<=` or changed the metric definition (e.g. min vs max
    // edge length) would cause at least one of these to fail.  The range assertion
    // in `prune_options_defaults_pin_empirical_constants` bounds the default to
    // [0.9, 1.1], ensuring the default stays close enough to SPIKE_RATIO that the
    // straddle tests remain well-calibrated.

    // Anchor for straddle-test thresholds: the spike-fixture triangle has
    // branch_length / local_thickness = 1.0.  Both straddle tests derive their
    // threshold from this constant so the coupling to the fixture is explicit.
    const SPIKE_RATIO: f64 = 1.0;

    /// Shared fixture for threshold-straddling tests.
    ///
    /// Two triangles sharing edge (v0, v1):
    /// - Body  [0, 1, 2]: branch_length ≈ 10.0, local_thickness = 1.0 → ratio ≈ 10 (always survives).
    /// - Spike [0, 1, 3]: branch_length = 1.0,  local_thickness = 1.0 → ratio = 1.0 (straddles threshold).
    fn threshold_straddle_fixture() -> MidSurfaceMesh {
        MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],    // v0, thickness 1.0 — shared edge start
                [1.0, 0.0, 0.0],    // v1, thickness 1.0 — shared edge end (length = 1.0)
                [0.5, 10.0, 0.0],   // v2, thickness 1.0 — body apex (edge ≈ 10)
                [0.5, -0.001, 0.0], // v3, thickness 1.0 — spike apex (edges ≈ 0.5)
            ],
            triangles: vec![[0, 1, 2], [0, 1, 3]],
            thickness: vec![1.0, 1.0, 1.0, 1.0],
        }
    }

    /// With `shell_branch_prune_ratio = SPIKE_RATIO + 0.05` (= 1.05), the spike
    /// (ratio 1.0 < 1.05) is pruned; the body (ratio ≈ 10 ≫ threshold) survives.
    ///
    /// The threshold is derived from `SPIKE_RATIO` (the fixture's actual ratio)
    /// rather than from the current default, so the straddle semantics are anchored
    /// to the fixture geometry, not to what the default happens to be right now.
    #[test]
    fn prune_branches_prunes_spike_just_below_threshold() {
        let threshold = SPIKE_RATIO + 0.05;
        let mesh = threshold_straddle_fixture();
        let opts = PruneOptions {
            shell_branch_prune_ratio: threshold,
            ..PruneOptions::default()
        };
        let result = prune_branches(&mesh, &opts).expect("valid mesh");
        assert_eq!(
            result.metrics.pruned_triangle_count, 1,
            "spike (ratio=1.0) must be pruned when threshold={threshold} (SPIKE_RATIO+0.05)"
        );
        assert_eq!(result.mesh.triangles.len(), 1, "body must survive");
        assert!(
            result.metrics.converged,
            "just-below-threshold spike must converge naturally after pruning"
        );
    }

    /// With `shell_branch_prune_ratio = SPIKE_RATIO - 0.05` (= 0.95), the spike
    /// (ratio 1.0 ≥ 0.95) survives; no triangles are pruned.
    ///
    /// The threshold is derived from `SPIKE_RATIO` (the fixture's actual ratio)
    /// rather than from the current default, so the straddle semantics are anchored
    /// to the fixture geometry, not to what the default happens to be right now.
    #[test]
    fn prune_branches_retains_spike_just_above_threshold() {
        let threshold = SPIKE_RATIO - 0.05;
        let mesh = threshold_straddle_fixture();
        let opts = PruneOptions {
            shell_branch_prune_ratio: threshold,
            ..PruneOptions::default()
        };
        let result = prune_branches(&mesh, &opts).expect("valid mesh");
        assert_eq!(
            result.metrics.pruned_triangle_count, 0,
            "spike (ratio=1.0) must survive when threshold={threshold} (SPIKE_RATIO-0.05)"
        );
        assert_eq!(
            result.mesh.triangles.len(),
            2,
            "both triangles must survive"
        );
        assert!(
            result.metrics.converged,
            "just-above-threshold spike (no pruning) must converge naturally"
        );
    }

    /// With a very low threshold (0.05), the original spike (ratio ≈ 0.125)
    /// falls above the threshold and is NOT pruned.  Verifies that the
    /// `shell_branch_prune_ratio` parameter actually controls pruning — a
    /// lower threshold means fewer pruned branches.
    #[test]
    fn prune_branches_retains_spike_when_threshold_too_low() {
        // Re-uses the spike fixture from step 11:
        // spike ratio ≈ 0.5/4.0 = 0.125; with threshold=0.05, spike survives.
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],   // v0, thickness 1.0
                [0.5, 0.0, 0.0],   // v1, thickness 1.0
                [0.25, 10.0, 0.0], // v2, thickness 1.0 — body apex
                [0.25, -0.1, 0.0], // v3, thickness 10.0 — spike apex
            ],
            triangles: vec![[0, 1, 2], [0, 1, 3]],
            thickness: vec![1.0, 1.0, 1.0, 10.0],
        };
        let opts = PruneOptions {
            shell_branch_prune_ratio: 0.05, // below spike ratio ≈ 0.125
            ..PruneOptions::default()
        };
        let result = prune_branches(&mesh, &opts).expect("valid mesh");
        assert_eq!(
            result.metrics.pruned_triangle_count, 0,
            "spike (ratio≈0.125) must survive when threshold=0.05"
        );
        assert_eq!(
            result.mesh.triangles.len(),
            2,
            "both triangles must survive"
        );
        assert!(
            result.metrics.converged,
            "threshold-too-low fixture (no pruning) must converge naturally"
        );
    }

    // ── Step 9: no-prune baseline test ───────────────────────────────────────

    /// Single equilateral-ish triangle with large edges and thin local thickness:
    /// `branch_length / local_thickness ≫ default ratio` → no triangles pruned.
    ///
    /// Exercises the full topology + ratio plumbing on benign input (one
    /// triangle has 3 boundary edges, all boundary, so it IS a tip; but its
    /// ratio is well above the threshold so it survives).
    ///
    /// A single isolated triangle has all 3 of its edges as boundary edges
    /// (each edge is incident to exactly 1 triangle), so it satisfies ≥ 2
    /// boundary edges and IS a tip.  The test verifies that the ratio guard
    /// prevents removal when `branch_length / local_thickness ≫ 1.0`.
    #[test]
    fn prune_branches_no_prune_when_all_branches_above_threshold() {
        // Equilateral triangle with edge length ≈ 10, thickness = 1.0.
        // branch_length ≈ 10, local_thickness = 1.0 → ratio ≈ 10 ≫ default 1.0.
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [5.0, 8.660_254_037_844_386, 0.0], // equilateral apex
            ],
            triangles: vec![[0, 1, 2]],
            thickness: vec![1.0, 1.0, 1.0],
        };
        let result =
            prune_branches(&mesh, &PruneOptions::default()).expect("valid mesh should not error");
        assert_eq!(result.mesh.triangles.len(), 1, "triangle must survive");
        assert_eq!(result.mesh.vertices.len(), 3, "all vertices must survive");
        assert_eq!(
            result.metrics.pruned_triangle_count, 0,
            "no triangles pruned"
        );
        assert_eq!(result.metrics.pruned_vertex_count, 0, "no vertices pruned");
        assert!(
            result.metrics.iterations <= 1,
            "at most one pass needed to settle"
        );
        assert!(
            result.metrics.converged,
            "no-prune baseline must converge naturally"
        );
    }

    // ── Step 7: input-mesh validation tests ──────────────────────────────────

    /// `prune_branches` rejects a mesh where `thickness.len() != vertices.len()`.
    ///
    /// Mirrors `mesh_mid_surface_rejects_inconsistent_mesh_lengths` (mesher.rs).
    #[test]
    fn prune_branches_rejects_inconsistent_mesh_lengths() {
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0]],
            triangles: vec![],
            thickness: vec![], // len 0 ≠ vertices len 1
        };
        match prune_branches(&mesh, &PruneOptions::default()) {
            Err(PruneError::InconsistentInputMesh {
                vertices_len: 1,
                thickness_len: 0,
            }) => {}
            other => panic!(
                "expected InconsistentInputMesh {{vertices_len:1, thickness_len:0}}, got {other:?}"
            ),
        }
    }

    /// `prune_branches` rejects a triangle whose vertex index is out of range.
    ///
    /// Mirrors `mesh_mid_surface_rejects_out_of_range_triangle_index` (mesher.rs).
    #[test]
    fn prune_branches_rejects_out_of_range_triangle_index() {
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0]], // only index 0 is valid
            triangles: vec![[0, 1, 0]],      // index 1 is out of range
            thickness: vec![1.0],
        };
        match prune_branches(&mesh, &PruneOptions::default()) {
            Err(PruneError::OutOfRangeTriangleIndex {
                triangle_index: 0,
                vertex_index: 1,
                vertices_len: 1,
            }) => {}
            other => panic!(
                "expected OutOfRangeTriangleIndex {{triangle_index:0, vertex_index:1, vertices_len:1}}, \
                 got {other:?}"
            ),
        }
    }

    // ── Step 5: options-validation tests ─────────────────────────────────────

    /// `prune_branches` rejects non-positive, non-finite, and NaN ratio values.
    ///
    /// Mirrors `mesh_mid_surface_rejects_invalid_merge_tolerance` (mesher.rs).
    #[test]
    fn prune_branches_rejects_invalid_ratio() {
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        for bad_ratio in [0.0_f64, -1.0, f64::NAN, f64::INFINITY] {
            let opts = PruneOptions {
                shell_branch_prune_ratio: bad_ratio,
                ..PruneOptions::default()
            };
            match prune_branches(&mesh, &opts) {
                Err(PruneError::InvalidRatio { value }) => {
                    // NaN != NaN, so use is_nan check for that case.
                    if bad_ratio.is_nan() {
                        assert!(value.is_nan(), "expected NaN, got {value}");
                    } else {
                        assert_eq!(value, bad_ratio, "error value should echo the bad input");
                    }
                }
                other => panic!("expected InvalidRatio for ratio={bad_ratio}, got {other:?}"),
            }
        }
    }

    /// `prune_branches` rejects `max_prune_iterations == 0`.
    ///
    /// Mirrors `mesh_mid_surface_rejects_invalid_merge_tolerance` (mesher.rs).
    #[test]
    fn prune_branches_rejects_invalid_max_iterations() {
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let opts = PruneOptions {
            max_prune_iterations: 0,
            ..PruneOptions::default()
        };
        match prune_branches(&mesh, &opts) {
            Err(PruneError::InvalidMaxIterations { value: 0 }) => {}
            other => panic!("expected InvalidMaxIterations {{value:0}}, got {other:?}"),
        }
    }

    /// `prune_branches` rejects non-positive, non-finite, and NaN
    /// `grid_alignment_tolerance` values.
    ///
    /// A zero/negative/NaN/±Inf tolerance would cause `inv_tol = 1/tol` to be
    /// infinite or NaN, silently collapsing every vertex into one or two
    /// canonical buckets — tip detection degrades without any error signal.
    /// This guard makes the contract explicit, consistent with the validation
    /// of `shell_branch_prune_ratio` and the field's doc-comment ("Must be
    /// strictly positive and finite").
    ///
    /// Mirrors `prune_branches_rejects_invalid_ratio` above.
    #[test]
    fn prune_branches_rejects_invalid_grid_alignment_tolerance() {
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        for bad_tol in [
            0.0_f64,
            -1.0,
            -1e-9,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let opts = PruneOptions {
                grid_alignment_tolerance: bad_tol,
                ..PruneOptions::default()
            };
            match prune_branches(&mesh, &opts) {
                Err(PruneError::InvalidGridAlignmentTolerance { value }) => {
                    // NaN != NaN, so use is_nan check for that case.
                    if bad_tol.is_nan() {
                        assert!(value.is_nan(), "expected NaN, got {value}");
                    } else {
                        assert_eq!(value, bad_tol, "error value should echo the bad input");
                    }
                }
                other => panic!(
                    "expected InvalidGridAlignmentTolerance for tol={bad_tol}, got {other:?}"
                ),
            }
        }
    }

    /// `prune_branches` rejects all subnormal `grid_alignment_tolerance` values.
    ///
    /// The `is_subnormal()` guard covers all denormals, including the gap class
    /// (`2^-1023`) whose reciprocal is still finite (~8.99e307) but large enough
    /// to overflow `coord * inv_tol` to ±Inf for any non-tiny coordinate,
    /// silently collapsing all vertices into the ±Inf saturation buckets.
    /// The earlier `!(1.0/x).is_finite()` guard (before this fix) missed that
    /// class.
    ///
    /// Mirrors `mesh_mid_surface_rejects_subnormal_merge_tolerance` in mesher.rs.
    ///
    /// Test values:
    /// - `f64::MIN_POSITIVE / 4.0` = `2^-1024`: subnormal, `1/x` overflows to
    ///   `+inf` (caught by both old and new gate).
    /// - `f64::MIN_POSITIVE / 2.0` = `2^-1023`: subnormal, but `1/x ≈ 8.99e307`
    ///   is **finite** — the discriminating case for the new `is_subnormal()` gate.
    /// - `5e-324`: the smallest positive denormal (`2^-1074`), `1/x` = `+inf`.
    /// - `f64::MIN_POSITIVE` (`2^-1022`): the smallest **normal** positive —
    ///   must still be accepted (not subnormal).
    #[test]
    fn prune_branches_rejects_subnormal_grid_alignment_tolerance() {
        let empty = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };

        // `f64::MIN_POSITIVE / 4.0` = 2^-1024 — subnormal, 1/x overflows to +inf.
        let subnormal_a = f64::MIN_POSITIVE / 4.0;
        assert!(
            subnormal_a.is_subnormal() && subnormal_a.is_sign_positive(),
            "test setup: subnormal_a should be a positive subnormal"
        );
        let err_a = prune_branches(
            &empty,
            &PruneOptions {
                grid_alignment_tolerance: subnormal_a,
                ..PruneOptions::default()
            },
        )
        .expect_err("subnormal grid_alignment_tolerance (f64::MIN_POSITIVE/4) must be rejected");
        assert!(
            matches!(err_a, PruneError::InvalidGridAlignmentTolerance { value } if value == subnormal_a),
            "expected InvalidGridAlignmentTolerance({subnormal_a}), got {err_a:?}"
        );

        // `f64::MIN_POSITIVE / 2.0` = 2^-1023 — subnormal whose reciprocal IS finite
        // (~8.99e307).  This is the class that the old `!(1.0/x).is_finite()` gate
        // MISSED; the new `is_subnormal()` gate must catch it.
        let subnormal_c = f64::MIN_POSITIVE / 2.0;
        assert!(
            subnormal_c.is_subnormal() && subnormal_c.is_sign_positive(),
            "test setup: subnormal_c (2^-1023) should be a positive subnormal"
        );
        assert!(
            (1.0_f64 / subnormal_c).is_finite(),
            "test setup: 1.0 / subnormal_c must be finite — this is the gap the old gate missed"
        );
        let err_c = prune_branches(
            &empty,
            &PruneOptions {
                grid_alignment_tolerance: subnormal_c,
                ..PruneOptions::default()
            },
        )
        .expect_err("subnormal grid_alignment_tolerance (f64::MIN_POSITIVE/2) must be rejected");
        assert!(
            matches!(err_c, PruneError::InvalidGridAlignmentTolerance { value } if value == subnormal_c),
            "expected InvalidGridAlignmentTolerance({subnormal_c}), got {err_c:?}"
        );

        // `5e-324` ≈ 2^-1074 — the smallest positive denormal; reciprocal is +inf.
        let subnormal_b = 5e-324_f64;
        assert!(
            subnormal_b.is_subnormal(),
            "test setup: 5e-324 must be subnormal"
        );
        let err_b = prune_branches(
            &empty,
            &PruneOptions {
                grid_alignment_tolerance: subnormal_b,
                ..PruneOptions::default()
            },
        )
        .expect_err("subnormal grid_alignment_tolerance (5e-324) must be rejected");
        assert!(
            matches!(err_b, PruneError::InvalidGridAlignmentTolerance { value } if value == subnormal_b),
            "expected InvalidGridAlignmentTolerance(5e-324), got {err_b:?}"
        );

        // `f64::MIN_POSITIVE` is the smallest NORMAL positive (2^-1022) — must be ACCEPTED.
        assert!(
            !f64::MIN_POSITIVE.is_subnormal(),
            "test setup: f64::MIN_POSITIVE must be normal (not subnormal)"
        );
        prune_branches(
            &empty,
            &PruneOptions {
                grid_alignment_tolerance: f64::MIN_POSITIVE,
                ..PruneOptions::default()
            },
        )
        .expect("f64::MIN_POSITIVE is the smallest normal positive — must still be accepted");
    }

    // ── Step 3: defaults-pin test ─────────────────────────────────────────────

    /// Pin `PruneOptions::default()` struct shape via pattern destructuring.
    ///
    /// The full-field destructure is a compile-time field-rename guard: if any
    /// field is renamed or removed, this test fails at compile time rather than
    /// silently passing with stale bindings.
    ///
    /// `shell_branch_prune_ratio` is range-checked against `(0.9..=1.1)` rather
    /// than pinned exactly; the effective straddle-test boundary is the
    /// spike-fixture ratio (`SPIKE_RATIO` = 1.0) ± 0.05.  Any default outside
    /// `[0.9, 1.1]` would cause the straddle tests to stop truly straddling the
    /// fixture — this tighter range catches that drift at test time instead of
    /// silently letting the straddle tests pass with a mis-calibrated default.
    /// Behaviour coverage is provided by
    /// `prune_branches_prunes_spike_just_below_threshold` and
    /// `prune_branches_retains_spike_just_above_threshold`.
    ///
    /// `max_prune_iterations` is pinned exactly to 8: it is documented as twice the
    /// ⌊log₂ 17⌋ = 4 chain-collapse minimum — a deliberate structural constant,
    /// not a tuning parameter.  Behaviour coverage is provided by
    /// `prune_branches_slab_end_to_end_pipeline`.
    ///
    /// `grid_alignment_tolerance` retains its exact `1e-9` pin because it is the
    /// T2→T3 shared-edge contract value, not an empirical default.
    ///
    /// Mirrors `mesher_options_defaults_pin_empirical_constants` (mesher.rs)
    /// and `mid_surface_options_defaults_pin_empirical_constants` (mid_surface.rs).
    #[test]
    fn prune_options_defaults_pin_empirical_constants() {
        // All fields named explicitly — compile error on any field rename.
        let PruneOptions {
            shell_branch_prune_ratio,
            max_prune_iterations,
            // controls canonical-vertex dedup tolerance for the T2→T3 shared-edge contract
            grid_alignment_tolerance,
        } = PruneOptions::default();
        assert!(
            (0.9..=1.1).contains(&shell_branch_prune_ratio),
            "shell_branch_prune_ratio default {shell_branch_prune_ratio} outside expected range [0.9, 1.1]; \
             effective straddle-test boundary is SPIKE_RATIO (1.0) ± 0.05 — update straddle tests if default changes"
        );
        assert_eq!(
            max_prune_iterations, 8,
            "max_prune_iterations default must be 8 (twice the ⌊log₂ 17⌋ = 4 chain-collapse \
             minimum; a deliberate structural constant, not a tuning parameter)"
        );
        assert_eq!(
            grid_alignment_tolerance, 1e-9,
            "grid_alignment_tolerance default must be 1e-9 (effectively bit-exact for \
             internal T2 pipeline while admitting float jitter from future T2 variants)"
        );
    }

    // ── Step 1: smoke test ────────────────────────────────────────────────────

    /// Public-surface compile-test: all public types are reachable from
    /// `crate::pruning` and `crate::` re-export paths; `prune_branches` is
    /// callable; empty input → `Ok` with empty mesh and zero metrics.
    ///
    /// Mirrors `mesher_public_surface_is_callable_on_empty_mesh` in `mesher.rs`.
    #[test]
    fn prune_branches_public_surface_is_callable_on_empty_mesh() {
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };

        let result: PruneResult =
            prune_branches(&mesh, &PruneOptions::default()).expect("empty mesh should return Ok");
        assert!(
            result.mesh.vertices.is_empty(),
            "empty input → empty output vertices"
        );
        assert!(
            result.mesh.triangles.is_empty(),
            "empty input → empty output triangles"
        );
        assert!(
            result.mesh.thickness.is_empty(),
            "empty input → empty output thickness"
        );
        assert_eq!(result.metrics.iterations, 0, "no iterations on empty input");
        assert!(
            result.metrics.converged,
            "empty input must report converged=true (trivially settled)"
        );

        // Compile probes: all error variants are publicly named and
        // constructible.
        let _: PruneError = PruneError::InvalidRatio { value: 0.0 };
        let _: PruneError = PruneError::InvalidMaxIterations { value: 0 };
        let _: PruneError = PruneError::InvalidGridAlignmentTolerance { value: 0.0 };
        let _: PruneError = PruneError::InconsistentInputMesh {
            vertices_len: 0,
            thickness_len: 0,
        };
        let _: PruneError = PruneError::OutOfRangeTriangleIndex {
            triangle_index: 0,
            vertex_index: 0,
            vertices_len: 0,
        };

        // Compile probes: types reachable from crate root (re-export path).
        let _: crate::PruneOptions = crate::PruneOptions::default();
        let _: Option<crate::PruneMetrics> = None;
        let _: Option<crate::PruneResult> = None;
        let _: Option<crate::PruneError> = None;
        // Function reachable from crate root:
        let _: fn(
            &crate::MidSurfaceMesh,
            &crate::PruneOptions,
        ) -> Result<crate::PruneResult, crate::PruneError> = crate::prune_branches;
    }

    // ── Task 3161: converged-flag tests ──────────────────────────────────────

    /// Chain-pruning fixture: body + two-step prune chain.
    ///
    /// Topology:
    /// - body = [0,1,2]: tall apex at v2=(0.15,10,0); ratio ≈ 10 → always survives.
    /// - T1   = [0,1,3]: hinge at v3=(0.15,-0.1,0); branch_length=0.3, ratio=0.3 → prunable.
    /// - T2   = [0,3,4]: apex at v4=(0,-0.2,0); branch_length=0.2, ratio=0.2 → prunable.
    ///
    /// Initial tips: T2 has 2 boundary edges ({0,4},{3,4}) → pruned in iter 1.
    /// After T2 pruned, edge {0,3} becomes boundary → T1 becomes a tip in iter 2.
    /// With `max_prune_iterations=1`, only T2 is pruned; T1 remains as residual.
    fn chain_pruning_fixture() -> MidSurfaceMesh {
        MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],   // v0
                [0.3, 0.0, 0.0],   // v1 — shared body/T1 base edge length = 0.3
                [0.15, 10.0, 0.0], // v2 — body apex (ratio ≈ 10, survives)
                [0.15, -0.1, 0.0], // v3 — T1/T2 hinge
                [0.0, -0.2, 0.0],  // v4 — T2 apex
            ],
            triangles: vec![
                [0, 1, 2], // body
                [0, 1, 3], // T1: shares edge {0,1} with body; {0,3} shared with T2
                [0, 3, 4], // T2: tip initially (boundary edges {0,4} and {3,4})
            ],
            thickness: vec![1.0, 1.0, 1.0, 1.0, 1.0],
        }
    }

    /// With `max_prune_iterations=1`, the prune loop is truncated after pruning
    /// T2 (iter 1).  T1 is now a tip (after T2's removal exposes edge {0,3})
    /// but the loop bound prevents iter 2 from running.  The result must
    /// report `converged == false` to signal residual prunable tips.
    #[test]
    fn prune_branches_reports_converged_false_when_loop_truncated_at_max_iterations() {
        let mesh = chain_pruning_fixture();
        let opts = PruneOptions {
            max_prune_iterations: 1,
            ..PruneOptions::default()
        };
        let result = prune_branches(&mesh, &opts).expect("chain-pruning fixture should not error");

        // Loop ran one round and actively pruned → truncated, not converged.
        assert!(
            !result.metrics.converged,
            "converged must be false when max_prune_iterations truncated the loop \
             while pruning was still active"
        );
        assert_eq!(
            result.metrics.iterations, 1,
            "exactly one iteration should have run"
        );
        assert!(
            result.metrics.pruned_triangle_count >= 1,
            "T2 must have been pruned in the one allowed iteration"
        );
        // Body triangle (ratio ≈ 10) must survive.
        assert!(
            !result.mesh.triangles.is_empty(),
            "body triangle must survive"
        );
    }

    /// With default options (`max_prune_iterations=8`), the chain fully prunes
    /// (T2 in iter 1, T1 in iter 2, body settles in iter 3) and the loop exits
    /// naturally on the no-prune pass → `converged == true`.
    #[test]
    fn prune_branches_reports_converged_true_when_loop_settles_naturally() {
        let mesh = chain_pruning_fixture();
        let result = prune_branches(&mesh, &PruneOptions::default())
            .expect("chain-pruning fixture should not error");

        assert!(
            result.metrics.converged,
            "converged must be true when the loop settled naturally (no-prune pass)"
        );
        // Both T1 and T2 pruned; body remains.
        assert_eq!(
            result.metrics.pruned_triangle_count, 2,
            "T1 and T2 must both be pruned with default max iterations"
        );
        assert_eq!(
            result.mesh.triangles.len(),
            1,
            "only the body triangle must survive"
        );
    }

    // ── Task 3162 step 5: real-T2 adjacent-cells regression ─────────────────
    //
    // Pins the T2→T3 contract end-to-end using a real (not hand-built) mesh
    // produced by `extract_mid_surface`.  The 5×5×5 slab fixture has 4×4 = 16
    // centerline cells per layer × 2 z-layers = 32 adjacent cells — enough to
    // exercise non-trivial shared-edge adjacency while remaining fast and easy
    // to reason about manually.
    //
    // Any regression in either T2's midpoint emission or T3's canonical dedup
    // that breaks the shared-edge-merging contract would cause the body to be
    // fully pruned to 0 triangles, which the `triangles.len() >= 8` assertion
    // catches immediately.
    //
    // Sharper than `prune_branches_slab_end_to_end_pipeline` (n=17,
    // `triangles.len() > 0` only) — the `>= 8` lower bound, the
    // `pruned_triangle_count < raw.triangles.len()` guard, the
    // parallel-array invariant, and the mid-plane z-coordinate sanity check
    // together catch failure modes the n=17 test would miss.

    /// Pins the T2→T3 contract end-to-end on a 5×5×5 slab with real
    /// `extract_mid_surface` output.
    ///
    /// The fixture has 32 adjacent cells in two z-layers around the
    /// centerline, producing shared-edge midpoints that the quantised dedup
    /// must recognise as internal.  A regression that breaks shared-edge
    /// merging would prune the entire body to 0 triangles.
    ///
    /// Sharper assertions than the existing 17×17×17 slab test:
    /// - `triangles.len() >= 8` (meaningful interior survives, not a degenerate
    ///   artifact)
    /// - `pruned_triangle_count < raw.triangles.len()` (some triangles survive)
    /// - parallel-array invariant `thickness.len() == vertices.len()`
    /// - all triangle indices in-range
    /// - `converged == true`
    /// - most surviving vertex z-coordinates lie near the slab mid-plane
    ///   (sanity-checks that the surviving topology is the shell mid-surface)
    #[test]
    fn prune_branches_real_t2_adjacent_cells_pipeline_pins_body_survival() {
        // 5×5×5 grid: 4×4 = 16 centerline cells per z-layer, 2 z-layers = 32
        // adjacent cells.  half=2.0 gives a slab of thickness 4 around z=0.
        let sdf = slab_sdf_3d(2.0, 5);
        let mask = centerline_mask(5, &sdf);

        // T2: real mid-surface extraction — produces shared-edge midpoints.
        let raw = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("5×5×5 slab extract_mid_surface should succeed");
        assert!(
            !raw.triangles.is_empty(),
            "5×5×5 slab must produce a non-empty raw mesh"
        );

        // T3: prune branches.
        let result = prune_branches(&raw, &PruneOptions::default())
            .expect("5×5×5 slab prune_branches should succeed");

        // (a) Body must retain N≈60 triangles. The 5×5×5 slab has 4×4=16
        // centerline cells per z-layer × 2 layers = 32 raw cells; after T2
        // emits 2 triangles per cell and T3 prunes boundary tips, ≈60 interior
        // triangles remain (the mid-surface). The narrow window (±2) catches
        // both the over-prune regression (full body collapse to 0, e.g.
        // shared-edge dedup broken) AND the under-prune regression (stragglers
        // retained, e.g. tip detection broken).
        assert!(
            (58..=62).contains(&result.mesh.triangles.len()),
            "body must retain N≈60 triangles after pruning (got {})",
            result.mesh.triangles.len()
        );

        // (b) At least some triangles survive (redundant with (a) but explicit).
        assert!(
            result.metrics.output_triangle_count > 0,
            "output_triangle_count must be > 0"
        );

        // (c) Some pruning must have occurred (boundary branches removed).
        assert!(
            result.metrics.pruned_triangle_count < raw.triangles.len(),
            "pruned_triangle_count ({}) must be less than raw triangle count ({})",
            result.metrics.pruned_triangle_count,
            raw.triangles.len()
        );

        // (d) Parallel-array invariant.
        assert_eq!(
            result.mesh.thickness.len(),
            result.mesh.vertices.len(),
            "thickness.len() must equal vertices.len() after pruning"
        );

        // (e) All triangle indices in-range.
        let vlen = result.mesh.vertices.len();
        for tri in &result.mesh.triangles {
            for &vi in tri.iter() {
                assert!(
                    (vi as usize) < vlen,
                    "triangle index {vi} out of range for {vlen} vertices after pruning"
                );
            }
        }

        // (f) converged.
        assert!(result.metrics.converged, "5×5×5 slab prune must converge");

        // (g) Sanity: most surviving vertices lie near the slab mid-plane
        // (z ≈ ±0.5 of the centerline at z=0 for this SDF).  The mid-surface
        // is at z=0; after pruning the surviving shell should predominantly
        // have |z| ≤ 1.0.  We allow a few outliers (boundary triangles).
        let near_midplane = result
            .mesh
            .vertices
            .iter()
            .filter(|v| v[2].abs() <= 1.0)
            .count();
        let total_v = result.mesh.vertices.len();
        assert!(
            near_midplane >= total_v / 2,
            "at least half of surviving vertices must be within |z|≤1 of the mid-plane; \
             {near_midplane}/{total_v} qualify"
        );
    }

    // ── Task 3162 step 3: quantised-dedup jittered-vertex test ──────────────
    //
    // Discriminates between bit-exact dedup (`[u64; 3]` `to_bits()`) and
    // quantised dedup (`[i64; 3]` `(coord * inv_tol).round()`).
    //
    // Topology: body B shares one edge with each of two tall permanent
    // neighbours N1 and N2 via jittered vertex positions (offset 1e-12).
    //
    //   - Quantised dedup: jittered shared-edge vertices collapse to the same
    //     canonical key → B's two shared edges both become internal → B has
    //     only 1 boundary edge → NOT a tip → never pruned.  N1 and N2 are
    //     tips (2 boundary edges each) but ratio ≈ 10 → survive.  All 3
    //     triangles survive, pruned_count == 0.
    //
    //   - Bit-exact dedup (the regression): jittered vertices get distinct
    //     keys → all 9 edges appear with incidence 1 → B has 3 boundary
    //     edges → IS a tip, ratio = 0.2 < 1.0 → B pruned.  Only N1 and N2
    //     survive (triangles.len() == 2, pruned_count == 1).
    //
    // The 2-triangle topology used in the original test could not discriminate
    // because with only one shared edge the body always has ≥ 2 boundary edges
    // and is therefore always a tip in both dedup modes.  The 3-triangle
    // topology here gives B exactly 1 boundary edge when both shared edges are
    // recognised, crossing the `boundary_count < 2` threshold.
    //
    // `(1e-12 * 1e9).round() = 0.001.round() = 0` — three orders of safety
    // margin between jitter and `grid_alignment_tolerance = 1e-9`.
    //
    // See the COUPLING NOTE block in `prune_branches` (above the canonical-
    // index map) for the T2→T3 shared-edge contract this test exercises.

    /// Quantised dedup keeps body triangle B alive by recognising its two
    /// shared edges even when neighbour-side vertices carry a near-tolerance
    /// jitter (`1e-12`).
    ///
    /// Layout — 3 triangles, 9 vertices:
    ///   B  = [v0,v1,v2] thickness=5 → local_t=5, branch_length=1 → ratio=0.2<1.
    ///   N1 = [v3,v4,v5] v3≈v0, v4≈v1 (jitter=1e-12); v5 far apex.
    ///                    thickness=1 → local_t=1, branch_length≈10 → ratio≈10>1.
    ///   N2 = [v6,v7,v8] v6≈v0, v7≈v2 (jitter=1e-12); v8 far apex.
    ///                    thickness=1 → local_t=1, branch_length≈10 → ratio≈10>1.
    ///
    /// With `grid_alignment_tolerance = 1e-9` (default):
    ///   - {v0,v3,v6} collapse; {v1,v4} collapse; {v2,v7} collapse.
    ///   - Edges (v0,v1) and (v0,v2) each have incidence 2 → internal.
    ///   - B has 1 boundary edge (v1,v2) → NOT a tip → NOT pruned.
    ///   - N1, N2: tips (2 boundary edges), ratio≈10 → NOT pruned.
    ///   - All 3 triangles survive; pruned_count == 0.  ← asserted
    ///
    /// Without quantised dedup (bit-exact regression):
    ///   - Jittered vertices get distinct keys → B has 3 boundary edges → IS tip.
    ///   - B ratio = 0.2 < 1.0 → B pruned; triangles.len() == 2.  ← test FAILS
    #[test]
    fn prune_branches_quantised_dedup_merges_within_tolerance_jittered_vertices() {
        const JITTER: f64 = 1e-12; // 3 orders of magnitude below default tol 1e-9
        // B: small flat triangle, ratio=0.2<1 — pruned if classified as a tip.
        // N1 shares a jittered copy of B's edge (v0-v1); N2 shares B's (v0-v2).
        // Quantised dedup collapses jitter → both shared edges become internal
        // → B has only 1 boundary edge → NOT a tip → NOT pruned.
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],                   // v0 — B vertex 0
                [1.0, 0.0, 0.0],                   // v1 — B vertex 1
                [0.5, 0.8, 0.0],                   // v2 — B vertex 2
                [JITTER, 0.0, 0.0],                // v3 ≈ v0 — N1 base (edge v0-v1)
                [1.0 + JITTER, 0.0, 0.0],          // v4 ≈ v1 — N1 base
                [0.5, -10.0, 0.0],                 // v5 — N1 far apex (ratio≈10)
                [JITTER, 0.0, 0.0],                // v6 ≈ v0 — N2 base (edge v0-v2)
                [0.5 + JITTER, 0.8 + JITTER, 0.0], // v7 ≈ v2 — N2 base
                [-10.0, 0.4, 0.0],                 // v8 — N2 far apex (ratio≈10)
            ],
            triangles: vec![[0, 1, 2], [3, 4, 5], [6, 7, 8]],
            // B: thickness 5 → local_t=5, ratio=1/5=0.2<1 (prunable tip).
            // N1, N2: thickness 1 → local_t=1, ratio≈10>1 (survive as tips).
            thickness: vec![5.0, 5.0, 5.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        };

        let result = prune_branches(&mesh, &PruneOptions::default())
            .expect("jittered-vertex mesh should not error");

        // With quantised dedup: B's edges (v0,v1) and (v0,v2) each get
        // incidence 2 → B has 1 boundary edge → NOT a tip → NOT pruned.
        // N1 and N2 are tips but ratio ≈ 10 → also NOT pruned.
        //
        // Regression (bit-exact dedup): all 9 edges get incidence 1 → B has
        // 3 boundary edges → IS tip, ratio 0.2 < 1.0 → B pruned; these
        // assertions would fail (pruned_count=1, triangles.len()=2).
        assert_eq!(
            result.metrics.pruned_triangle_count, 0,
            "no triangle pruned: B is not a tip (1 boundary edge via quantised dedup), \
             N1/N2 survive as tips with ratio≈10"
        );
        assert_eq!(
            result.mesh.triangles.len(),
            3,
            "all 3 triangles survive; len==2 would indicate B was wrongly \
             classified as a tip (regression to bit-exact dedup)"
        );

        // Parallel-array invariant.
        assert_eq!(
            result.mesh.thickness.len(),
            result.mesh.vertices.len(),
            "thickness.len() must equal vertices.len() after pruning"
        );

        assert!(
            result.metrics.converged,
            "fixture converges naturally (no pruning occurs)"
        );
    }
}
