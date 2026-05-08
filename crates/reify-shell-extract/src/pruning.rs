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
    /// Number of prune-iteration rounds that actually ran (0 if no tip
    /// triangles were found on the first pass).
    pub iterations: u32,
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
            },
        });
    }

    // ── 4. Prune iterations ───────────────────────────────────────────────────
    let mut triangles: Vec<[u32; 3]> = mesh.triangles.clone();
    let vertices = &mesh.vertices;
    let thickness = &mesh.thickness;
    let mut total_pruned: usize = 0;
    let mut iterations: u32 = 0;

    for _ in 0..options.max_prune_iterations {
        // Build edge → incident-triangle count map.
        // Key: sorted vertex pair [u32; 2]; value: count of incident triangles.
        let mut edge_counts: FxHashMap<[u32; 2], u32> = FxHashMap::default();
        for tri in &triangles {
            let [a, b, c] = *tri;
            for edge in [[a, b], [b, c], [a, c]] {
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
            let edges = [
                sorted_pair(a, b),
                sorted_pair(b, c),
                sorted_pair(a, c),
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

            if local_thickness > 0.0 && branch_length / local_thickness < ratio {
                pruned_in_round[tri_idx] = true;
                any_pruned = true;
            }
        }

        if !any_pruned {
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
        let result = prune_branches(&mesh, &PruneOptions::default())
            .expect("valid mesh should not error");

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
        let result = prune_branches(&mesh, &PruneOptions::default())
            .expect("valid mesh should not error");
        assert_eq!(
            result.metrics.pruned_triangle_count, 1,
            "exactly one triangle (the spike) must be pruned"
        );
        assert_eq!(
            result.mesh.triangles.len(), 1,
            "one triangle must survive (the body)"
        );
        // The surviving triangle's vertices must be a subset of the original body triangle.
        // After compaction v3 is gone; v0, v1, v2 survive (possibly re-indexed).
        assert_eq!(
            result.mesh.vertices.len(), 3,
            "three vertices survive (body triangle)"
        );
        assert!(
            result.metrics.iterations >= 1,
            "at least one iteration ran"
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
        let result = prune_branches(&mesh, &PruneOptions::default())
            .expect("valid mesh should not error");
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
            vertices: vec![[0.0, 0.0, 0.0]],             // only index 0 is valid
            triangles: vec![[0, 1, 0]],                   // index 1 is out of range
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
                        assert_eq!(
                            value, bad_ratio,
                            "error value should echo the bad input"
                        );
                    }
                }
                other => panic!(
                    "expected InvalidRatio for ratio={bad_ratio}, got {other:?}"
                ),
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

    // ── Step 3: defaults-pin test ─────────────────────────────────────────────

    /// Pin `PruneOptions::default()` struct shape via pattern destructuring.
    ///
    /// The full-field destructure is a compile-time field-rename guard: if any
    /// field is renamed or removed, this test fails at compile time rather than
    /// silently passing with stale bindings.
    ///
    /// Asserts `shell_branch_prune_ratio == 1.0` (PRD §89 conservative default)
    /// and `max_prune_iterations == 8` (chain-collapse bound doubled for safety).
    ///
    /// Mirrors `mesher_options_defaults_pin_empirical_constants` (mesher.rs)
    /// and `mid_surface_options_defaults_pin_empirical_constants` (mid_surface.rs).
    #[test]
    fn prune_options_defaults_pin_empirical_constants() {
        // All fields named explicitly — compile error on any field rename.
        let PruneOptions {
            shell_branch_prune_ratio,
            max_prune_iterations,
        } = PruneOptions::default();
        assert_eq!(
            shell_branch_prune_ratio, 1.0,
            "shell_branch_prune_ratio default must be 1.0 (PRD §89 conservative threshold)"
        );
        assert_eq!(
            max_prune_iterations, 8,
            "max_prune_iterations default must be 8 (chain-collapse bound)"
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

        let result: PruneResult = prune_branches(&mesh, &PruneOptions::default())
            .expect("empty mesh should return Ok");
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

        // Compile probes: all four error variants are publicly named and
        // constructible.
        let _: PruneError = PruneError::InvalidRatio { value: 0.0 };
        let _: PruneError = PruneError::InvalidMaxIterations { value: 0 };
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
}
