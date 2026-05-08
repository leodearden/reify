//! Mid-surface mesher: vertex de-duplication, quality gating, and MMG2D-style
//! remeshing for shell-element FEA (PRD task T9).
//!
//! # PRD reference
//!
//! `docs/prds/v0_4/structural-analysis-shells.md` §111 — "Default Gmsh 2D from
//! extractor mesh" + "MMG2D-style remeshing on quality failure".
//!
//! Neither Gmsh nor MMG2D has Rust FFI bindings in this workspace today.
//! Following the skeleton-crate convention of the sibling modules
//! (`mid_surface.rs`, `segmentation.rs`), this module ships a pure-Rust
//! algorithm:
//!
//! - **Vertex de-duplication** via spatial-hash binning at `merge_tolerance`
//!   granularity (O(n) expected).
//! - **Per-triangle quality metrics**: aspect ratio (`4·sqrt(3)·area /
//!   (l₁² + l₂² + l₃²)`, normalised to 1.0 for equilateral) and minimum
//!   interior angle via law of cosines.
//! - **Quality gate** with `Err(MesherError::QualityBelowThreshold { … })`
//!   carrying the worst-case metrics as a diagnostic payload.
//!
//! **Deferred (v0.4):** The full Laplacian smoothing (`max_remesh_iterations
//! > 0`) and MMG2D-style remeshing pipeline. Callers may set
//! `max_remesh_iterations > 0` but will receive the same
//! `QualityBelowThreshold` error; the smoothing implementation is a follow-up
//! task.

use std::collections::HashMap;

use crate::mid_surface::MidSurfaceMesh;

// ── Public types ─────────────────────────────────────────────────────────────

/// Tunable parameters for [`mesh_mid_surface`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MesherOptions {
    /// Vertex-dedup spatial-hash bin width. Two vertices whose coordinates
    /// all round to the same integer grid at this tolerance are merged; the
    /// merged vertex is placed at the first-seen position and per-vertex
    /// thickness values are averaged.
    ///
    /// Must be strictly positive and finite.
    ///
    /// **Rationale.** Binary-MC emits vertices at exact grid midpoints, so
    /// adjacent-cell shared edges produce bit-exact duplicates. Default `1e-9`
    /// collapses those without merging geometrically distinct vertices (minimum
    /// MC vertex separation is `0.5 × min_spacing`, orders of magnitude larger
    /// than `1e-9` at any practical grid spacing). Callers with float-noisy
    /// producers can loosen this value.
    pub merge_tolerance: f64,

    /// Sliver gate: minimum acceptable aspect ratio (0, 1]. Triangles with
    /// `aspect_ratio < min_aspect_ratio` fail the quality gate. `1.0` accepts
    /// only equilateral triangles; `1e-6` accepts all but truly degenerate
    /// (zero-area) triangles.
    ///
    /// **Rationale.** Default `0.1` rejects triangles whose shortest altitude
    /// is less than 10% of the longest edge — a common FEA mesh-quality
    /// threshold. Slivers with `aspect_ratio < 0.1` produce ill-conditioned
    /// element stiffness matrices.
    pub min_aspect_ratio: f64,

    /// Angle gate: minimum acceptable interior angle in degrees, `(0, 60)`.
    /// Triangles with any interior angle below this value fail the quality gate.
    /// `60.0` would accept only equilateral triangles; near-zero accepts all
    /// but degenerate triangles.
    ///
    /// **Rationale.** Default `20.0°` rejects high-aspect slivers with
    /// extremely acute angles, a standard threshold in FEA mesh quality checks.
    /// The equilateral upper bound of `60.0°` is excluded because it would make
    /// the gate unreachable for non-equilateral meshes.
    pub min_angle_degrees: f64,

    /// Maximum Laplacian smoothing iterations on quality failure. `0` (the v0.4
    /// default) = fail-fast: return `Err(QualityBelowThreshold)` immediately.
    /// `> 0` = attempt up to N rounds before failing.
    ///
    /// **Deferred (v0.4).** This field is documented but the smoothing
    /// implementation is not yet shipped. Setting `max_remesh_iterations > 0`
    /// produces the same `QualityBelowThreshold` error as `0`.
    ///
    /// **Rationale.** Defaulting to `0` makes the diagnostic path the obvious
    /// default, preventing silent smoothing of extractor quality bugs. Callers
    /// opt in to smoothing explicitly.
    pub max_remesh_iterations: u32,
}

impl Default for MesherOptions {
    fn default() -> Self {
        Self {
            merge_tolerance: 1e-9,
            min_aspect_ratio: 0.1,
            min_angle_degrees: 20.0,
            max_remesh_iterations: 0,
        }
    }
}

/// Per-mesh quality metrics returned inside [`MesherResult`].
#[derive(Debug, Clone, PartialEq)]
pub struct QualityMetrics {
    /// Number of triangles in the de-duplicated mesh.
    pub triangle_count: usize,
    /// Number of unique vertices in the de-duplicated mesh.
    pub vertex_count: usize,
    /// Worst (minimum) aspect ratio across all triangles.
    /// `1.0` for equilateral; → 0 for slivers.
    /// `f64::INFINITY` if the mesh is empty.
    pub min_aspect_ratio: f64,
    /// Worst (minimum) interior angle across all triangles, in degrees.
    /// `60.0` for equilateral; → 0 for slivers.
    /// `f64::INFINITY` if the mesh is empty.
    pub min_angle_degrees: f64,
    /// Number of triangles that failed the quality gate
    /// (`aspect_ratio < options.min_aspect_ratio` OR
    /// `min_angle < options.min_angle_degrees`).
    pub failed_triangle_count: usize,
}

/// Output of a successful [`mesh_mid_surface`] call.
#[derive(Debug, Clone, PartialEq)]
pub struct MesherResult {
    /// The de-duplicated, quality-passing mesh. Same type as the input
    /// ([`MidSurfaceMesh`]); no new type is introduced because the structural
    /// shape (vertices + triangles + per-vertex thickness) is identical.
    /// The `Ok` return is the type invariant that the mesh is dedup'd and
    /// quality-passing.
    pub mesh: MidSurfaceMesh,
    /// Quality metrics computed over the de-duplicated mesh.
    pub metrics: QualityMetrics,
    /// Number of Laplacian smoothing iterations actually applied.
    /// `0` when `options.max_remesh_iterations == 0` or when the mesh passed
    /// quality on the first check.
    pub remesh_iterations: u32,
}

/// Errors returned by [`mesh_mid_surface`].
#[derive(Debug, Clone, PartialEq)]
pub enum MesherError {
    /// `merge_tolerance` must be finite and strictly positive.
    InvalidMergeTolerance {
        /// The offending value supplied by the caller.
        value: f64,
    },
    /// `min_aspect_ratio` must be in the range `(0.0, 1.0]`.
    InvalidMinAspectRatio {
        /// The offending value supplied by the caller.
        value: f64,
    },
    /// `min_angle_degrees` must be in the range `(0.0, 60.0)`.
    InvalidMinAngleDegrees {
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
    /// One or more triangles failed the quality gate after `remesh_iterations`
    /// rounds of smoothing. Carries the worst-case metrics as a diagnostic
    /// payload.
    QualityBelowThreshold {
        /// Worst (minimum) aspect ratio seen across the mesh.
        min_aspect_ratio: f64,
        /// Worst (minimum) interior angle seen across the mesh, in degrees.
        min_angle_degrees: f64,
        /// Number of triangles that failed the quality gate.
        failed_triangle_count: usize,
        /// Number of smoothing iterations attempted before failing.
        remesh_iterations: u32,
    },
}

impl std::fmt::Display for MesherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MesherError::InvalidMergeTolerance { value } => write!(
                f,
                "merge_tolerance must be finite and strictly positive (got {value}); \
                 use 1e-9 for bit-exact binary-MC output or a larger value for \
                 float-noisy producers"
            ),
            MesherError::InvalidMinAspectRatio { value } => write!(
                f,
                "min_aspect_ratio must be in (0.0, 1.0] (got {value}); \
                 1.0 accepts only equilateral triangles, 0.1 is the recommended \
                 FEA minimum"
            ),
            MesherError::InvalidMinAngleDegrees { value } => write!(
                f,
                "min_angle_degrees must be in (0.0, 60.0) (got {value}); \
                 60.0 is the equilateral upper bound; values near 0.0 effectively \
                 disable the angle gate"
            ),
            MesherError::InconsistentInputMesh {
                vertices_len,
                thickness_len,
            } => write!(
                f,
                "mesh.thickness.len() ({thickness_len}) ≠ mesh.vertices.len() \
                 ({vertices_len}); the two parallel arrays must be the same length"
            ),
            MesherError::OutOfRangeTriangleIndex {
                triangle_index,
                vertex_index,
                vertices_len,
            } => write!(
                f,
                "triangle {triangle_index} references vertex index {vertex_index} \
                 which is out of range (mesh has {vertices_len} vertices)"
            ),
            MesherError::QualityBelowThreshold {
                min_aspect_ratio,
                min_angle_degrees,
                failed_triangle_count,
                remesh_iterations,
            } => write!(
                f,
                "mesh quality below threshold after {remesh_iterations} remesh \
                 iteration(s): {failed_triangle_count} triangle(s) failed; \
                 worst aspect_ratio={min_aspect_ratio:.6}, \
                 worst min_angle={min_angle_degrees:.3}°"
            ),
        }
    }
}

impl std::error::Error for MesherError {}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// De-duplicate mesh vertices by spatial-hash binning at `merge_tolerance`.
///
/// Returns `(new_vertices, new_thickness, remap)` where `remap[old_idx] =
/// new_idx`. Colliding vertices average their thickness values (preserving the
/// parallel-array invariant `thickness.len() == vertices.len()`).
///
/// **Complexity.** O(n) expected via `HashMap`. The bin key
/// `(round(x/tol), round(y/tol), round(z/tol))` is exact for binary-MC
/// output (vertices at grid midpoints with unit spacing) but may fail to
/// collapse near-boundary pairs for float-noisy producers.
fn dedup_vertices(
    mesh: &MidSurfaceMesh,
    merge_tolerance: f64,
) -> (Vec<[f64; 3]>, Vec<f64>, Vec<u32>) {
    let n = mesh.vertices.len();
    let mut grid: HashMap<[i64; 3], u32> = HashMap::with_capacity(n);
    let mut new_vertices: Vec<[f64; 3]> = Vec::with_capacity(n);
    let mut thickness_sum: Vec<f64> = Vec::with_capacity(n);
    let mut thickness_count: Vec<u32> = Vec::with_capacity(n);
    let mut remap: Vec<u32> = Vec::with_capacity(n);

    let inv_tol = 1.0 / merge_tolerance;

    for (i, &v) in mesh.vertices.iter().enumerate() {
        let key = [
            (v[0] * inv_tol).round() as i64,
            (v[1] * inv_tol).round() as i64,
            (v[2] * inv_tol).round() as i64,
        ];
        let t = mesh.thickness[i];
        let new_idx = *grid.entry(key).or_insert_with(|| {
            let idx = new_vertices.len() as u32;
            new_vertices.push(v);
            thickness_sum.push(0.0);
            thickness_count.push(0);
            idx
        });
        thickness_sum[new_idx as usize] += t;
        thickness_count[new_idx as usize] += 1;
        remap.push(new_idx);
    }

    // Average thickness for merged vertices.
    let new_thickness: Vec<f64> = thickness_sum
        .iter()
        .zip(&thickness_count)
        .map(|(&s, &c)| s / c as f64)
        .collect();

    (new_vertices, new_thickness, remap)
}

/// Compute the aspect ratio `4·sqrt(3)·area / (l₁² + l₂² + l₃²)`.
///
/// Returns `1.0` for an equilateral triangle and approaches `0.0` for slivers.
/// Returns `0.0` for degenerate (zero-area) triangles (`area < 1e-30`).
fn triangle_aspect_ratio(p0: [f64; 3], p1: [f64; 3], p2: [f64; 3]) -> f64 {
    let e01 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let e02 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
    let e12 = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];

    // area = 0.5 · ‖e01 × e02‖
    let cross = [
        e01[1] * e02[2] - e01[2] * e02[1],
        e01[2] * e02[0] - e01[0] * e02[2],
        e01[0] * e02[1] - e01[1] * e02[0],
    ];
    let area = 0.5
        * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();

    if area < 1e-30 {
        return 0.0;
    }

    let sq = |e: [f64; 3]| e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
    let sum_sq = sq(e01) + sq(e02) + sq(e12);

    if sum_sq < 1e-30 {
        return 0.0;
    }

    4.0 * 3f64.sqrt() * area / sum_sq
}

/// Compute the minimum interior angle of a triangle, in degrees, via law of
/// cosines applied to each pair of edges.
///
/// Returns `0.0` for degenerate triangles.
fn triangle_min_angle_degrees(p0: [f64; 3], p1: [f64; 3], p2: [f64; 3]) -> f64 {
    let e01 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let e02 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
    let e12 = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];

    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let neg = |a: [f64; 3]| [-a[0], -a[1], -a[2]];
    let len = |a: [f64; 3]| dot(a, a).sqrt();

    let angle_between = |v1: [f64; 3], v2: [f64; 3]| -> f64 {
        let l1 = len(v1);
        let l2 = len(v2);
        if l1 < 1e-30 || l2 < 1e-30 {
            return 0.0;
        }
        (dot(v1, v2) / (l1 * l2)).clamp(-1.0, 1.0).acos().to_degrees()
    };

    let a0 = angle_between(e01, e02);          // angle at p0
    let a1 = angle_between(neg(e01), e12);     // angle at p1
    let a2 = angle_between(neg(e02), neg(e12)); // angle at p2

    a0.min(a1).min(a2)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Produce a shell-element-ready mid-surface mesh from a [`MidSurfaceMesh`]
/// (T2 output) via vertex de-duplication, per-triangle quality gating, and
/// an optional MMG2D-style remeshing escape hatch.
///
/// # Errors
///
/// | Variant | Condition |
/// |---------|-----------|
/// | [`MesherError::InvalidMergeTolerance`] | `merge_tolerance ≤ 0` or non-finite |
/// | [`MesherError::InvalidMinAspectRatio`] | `min_aspect_ratio ∉ (0, 1]` |
/// | [`MesherError::InvalidMinAngleDegrees`] | `min_angle_degrees ∉ (0, 60)` |
/// | [`MesherError::InconsistentInputMesh`] | `thickness.len() ≠ vertices.len()` |
/// | [`MesherError::OutOfRangeTriangleIndex`] | any triangle index ≥ `vertices.len()` |
/// | [`MesherError::QualityBelowThreshold`] | quality gate fails after all remesh iterations |
pub fn mesh_mid_surface(
    mesh: &MidSurfaceMesh,
    options: &MesherOptions,
) -> Result<MesherResult, MesherError> {
    // ── 1. Validate options ───────────────────────────────────────────────────
    if options.merge_tolerance <= 0.0 || !options.merge_tolerance.is_finite() {
        return Err(MesherError::InvalidMergeTolerance {
            value: options.merge_tolerance,
        });
    }
    if options.min_aspect_ratio <= 0.0 || options.min_aspect_ratio > 1.0 {
        return Err(MesherError::InvalidMinAspectRatio {
            value: options.min_aspect_ratio,
        });
    }
    if options.min_angle_degrees <= 0.0 || options.min_angle_degrees >= 60.0 {
        return Err(MesherError::InvalidMinAngleDegrees {
            value: options.min_angle_degrees,
        });
    }

    // ── 2. Validate input mesh ────────────────────────────────────────────────
    if mesh.thickness.len() != mesh.vertices.len() {
        return Err(MesherError::InconsistentInputMesh {
            vertices_len: mesh.vertices.len(),
            thickness_len: mesh.thickness.len(),
        });
    }
    for (tri_idx, tri) in mesh.triangles.iter().enumerate() {
        for &vi in tri.iter() {
            if vi as usize >= mesh.vertices.len() {
                return Err(MesherError::OutOfRangeTriangleIndex {
                    triangle_index: tri_idx,
                    vertex_index: vi,
                    vertices_len: mesh.vertices.len(),
                });
            }
        }
    }

    // ── 3. Empty-input short-circuit ──────────────────────────────────────────
    if mesh.vertices.is_empty() {
        return Ok(MesherResult {
            mesh: MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            metrics: QualityMetrics {
                triangle_count: 0,
                vertex_count: 0,
                min_aspect_ratio: f64::INFINITY,
                min_angle_degrees: f64::INFINITY,
                failed_triangle_count: 0,
            },
            remesh_iterations: 0,
        });
    }

    // ── 4. Vertex de-duplication ──────────────────────────────────────────────
    let (new_vertices, new_thickness, remap) =
        dedup_vertices(mesh, options.merge_tolerance);
    let new_triangles: Vec<[u32; 3]> = mesh
        .triangles
        .iter()
        .map(|tri| {
            [
                remap[tri[0] as usize],
                remap[tri[1] as usize],
                remap[tri[2] as usize],
            ]
        })
        .collect();

    // ── 5. Per-triangle quality ───────────────────────────────────────────────
    let mut worst_aspect_ratio = f64::INFINITY;
    let mut worst_min_angle = f64::INFINITY;
    let mut failed_count = 0usize;

    for tri in &new_triangles {
        let p0 = new_vertices[tri[0] as usize];
        let p1 = new_vertices[tri[1] as usize];
        let p2 = new_vertices[tri[2] as usize];

        let ar = triangle_aspect_ratio(p0, p1, p2);
        let ma = triangle_min_angle_degrees(p0, p1, p2);

        if ar < worst_aspect_ratio {
            worst_aspect_ratio = ar;
        }
        if ma < worst_min_angle {
            worst_min_angle = ma;
        }
        if ar < options.min_aspect_ratio || ma < options.min_angle_degrees {
            failed_count += 1;
        }
    }

    // Guard: if the triangle list is somehow empty after dedup (shouldn't
    // happen given the vertices-is-empty check above), reset sentinels.
    if new_triangles.is_empty() {
        worst_aspect_ratio = f64::INFINITY;
        worst_min_angle = f64::INFINITY;
    }

    // ── 6. Quality gate ───────────────────────────────────────────────────────
    if failed_count > 0 {
        // Deferred (v0.4): Laplacian smoothing / MMG2D-style remeshing.
        // Both max_remesh_iterations == 0 and > 0 fall through to the same
        // error path. Full smoothing is a follow-up task.
        return Err(MesherError::QualityBelowThreshold {
            min_aspect_ratio: worst_aspect_ratio,
            min_angle_degrees: worst_min_angle,
            failed_triangle_count: failed_count,
            remesh_iterations: 0,
        });
    }

    // ── 7. Return success ─────────────────────────────────────────────────────
    let vertex_count = new_vertices.len();
    let triangle_count = new_triangles.len();
    Ok(MesherResult {
        mesh: MidSurfaceMesh {
            vertices: new_vertices,
            triangles: new_triangles,
            thickness: new_thickness,
        },
        metrics: QualityMetrics {
            triangle_count,
            vertex_count,
            min_aspect_ratio: worst_aspect_ratio,
            min_angle_degrees: worst_min_angle,
            failed_triangle_count: 0,
        },
        remesh_iterations: 0,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{mesh_mid_surface, MesherError, MesherOptions, MesherResult, QualityMetrics};
    use crate::MidSurfaceMesh;

    // ── Step 1: public-surface smoke test ────────────────────────────────────

    /// Public-surface compile-test: all public types are reachable from
    /// `crate::mesher` and `crate::` re-export paths; `mesh_mid_surface` is
    /// callable; empty input → `Ok` with empty mesh and zeroed metrics.
    #[test]
    fn mesher_public_surface_is_callable_on_empty_mesh() {
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };

        let result: MesherResult = mesh_mid_surface(&mesh, &MesherOptions::default())
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
        assert_eq!(result.metrics.triangle_count, 0, "empty input → 0 triangles");
        assert_eq!(result.metrics.vertex_count, 0, "empty input → 0 vertices");
        assert_eq!(result.remesh_iterations, 0, "no remeshing on empty input");

        // Compile probes: all six error variants are publicly named and constructible.
        let _: MesherError = MesherError::InvalidMergeTolerance { value: 0.0 };
        let _: MesherError = MesherError::InvalidMinAspectRatio { value: 0.0 };
        let _: MesherError = MesherError::InvalidMinAngleDegrees { value: 0.0 };
        let _: MesherError = MesherError::InconsistentInputMesh {
            vertices_len: 0,
            thickness_len: 0,
        };
        let _: MesherError = MesherError::OutOfRangeTriangleIndex {
            triangle_index: 0,
            vertex_index: 0,
            vertices_len: 0,
        };
        let _: MesherError = MesherError::QualityBelowThreshold {
            min_aspect_ratio: 0.0,
            min_angle_degrees: 0.0,
            failed_triangle_count: 0,
            remesh_iterations: 0,
        };

        // Compile probes: types reachable from crate root (re-export path).
        let _: crate::MesherOptions = crate::MesherOptions::default();
        let _: Option<crate::QualityMetrics> = None;
        let _: Option<crate::MesherResult> = None;
        let _: Option<crate::MesherError> = None;
        // Function reachable from crate root:
        let _: fn(
            &crate::MidSurfaceMesh,
            &crate::MesherOptions,
        ) -> Result<crate::MesherResult, crate::MesherError> = crate::mesh_mid_surface;
    }

    // ── Step 3: defaults-pin test ─────────────────────────────────────────────

    /// Pin `MesherOptions::default()` field values via pattern destructuring.
    ///
    /// The pattern-destructure serves as a compile-time field-rename guard:
    /// if any field is renamed this test fails at compile time rather than
    /// silently passing with a stale value.
    ///
    /// Mirrors `mid_surface_options_defaults_pin_empirical_constants` in
    /// `mid_surface.rs`.
    #[test]
    fn mesher_options_defaults_pin_empirical_constants() {
        let MesherOptions {
            merge_tolerance,
            min_aspect_ratio,
            min_angle_degrees,
            max_remesh_iterations,
        } = MesherOptions::default();

        assert_eq!(
            merge_tolerance, 1e-9,
            "merge_tolerance default must be 1e-9 (bit-exact for binary-MC \
             output; matches MidSurfaceOptions::grid_alignment_tolerance)"
        );
        assert_eq!(
            min_aspect_ratio, 0.1,
            "min_aspect_ratio default must be 0.1 (standard FEA sliver gate; \
             rejects triangles whose shortest altitude < 10% of longest edge)"
        );
        assert_eq!(
            min_angle_degrees, 20.0,
            "min_angle_degrees default must be 20.0° (standard FEA angle gate; \
             rejects high-aspect slivers with extremely acute angles)"
        );
        assert_eq!(
            max_remesh_iterations, 0,
            "max_remesh_iterations default must be 0 (fail-fast; forces callers \
             to opt into smoothing rather than silently hiding quality bugs)"
        );
    }

    // ── Step 5: options-validation tests ─────────────────────────────────────

    /// `mesh_mid_surface` rejects invalid `merge_tolerance` values.
    ///
    /// Each test uses an empty mesh so the validation order matters: options
    /// must be checked *before* the empty-mesh short-circuit.
    #[test]
    fn mesh_mid_surface_rejects_invalid_merge_tolerance() {
        let empty = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };

        // Negative tolerance
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { merge_tolerance: -1.0, ..MesherOptions::default() },
        )
        .expect_err("negative merge_tolerance must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMergeTolerance { value } if value == -1.0),
            "expected InvalidMergeTolerance(-1.0), got {err:?}"
        );

        // Zero tolerance
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { merge_tolerance: 0.0, ..MesherOptions::default() },
        )
        .expect_err("zero merge_tolerance must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMergeTolerance { value } if value == 0.0),
            "expected InvalidMergeTolerance(0.0), got {err:?}"
        );

        // NaN tolerance
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { merge_tolerance: f64::NAN, ..MesherOptions::default() },
        )
        .expect_err("NaN merge_tolerance must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMergeTolerance { value } if value.is_nan()),
            "expected InvalidMergeTolerance(NaN), got {err:?}"
        );

        // +Infinity tolerance
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { merge_tolerance: f64::INFINITY, ..MesherOptions::default() },
        )
        .expect_err("+Inf merge_tolerance must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMergeTolerance { value } if value.is_infinite()),
            "expected InvalidMergeTolerance(+Inf), got {err:?}"
        );
    }

    /// `mesh_mid_surface` rejects `min_aspect_ratio` outside `(0.0, 1.0]`.
    #[test]
    fn mesh_mid_surface_rejects_invalid_min_aspect_ratio() {
        let empty = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };

        // Zero
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { min_aspect_ratio: 0.0, ..MesherOptions::default() },
        )
        .expect_err("min_aspect_ratio = 0.0 must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAspectRatio { value } if value == 0.0),
            "expected InvalidMinAspectRatio(0.0), got {err:?}"
        );

        // Negative
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { min_aspect_ratio: -0.5, ..MesherOptions::default() },
        )
        .expect_err("negative min_aspect_ratio must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAspectRatio { value } if value == -0.5),
            "expected InvalidMinAspectRatio(-0.5), got {err:?}"
        );

        // Above 1.0
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { min_aspect_ratio: 1.001, ..MesherOptions::default() },
        )
        .expect_err("min_aspect_ratio > 1.0 must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAspectRatio { value } if value == 1.001),
            "expected InvalidMinAspectRatio(1.001), got {err:?}"
        );

        // Exactly 1.0 is valid (equilateral-only gate)
        mesh_mid_surface(
            &empty,
            &MesherOptions { min_aspect_ratio: 1.0, ..MesherOptions::default() },
        )
        .expect("min_aspect_ratio = 1.0 must be accepted (exactly at upper bound)");
    }

    /// `mesh_mid_surface` rejects `min_angle_degrees` outside `(0.0, 60.0)`.
    #[test]
    fn mesh_mid_surface_rejects_invalid_min_angle_degrees() {
        let empty = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };

        // Zero
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { min_angle_degrees: 0.0, ..MesherOptions::default() },
        )
        .expect_err("min_angle_degrees = 0.0 must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAngleDegrees { value } if value == 0.0),
            "expected InvalidMinAngleDegrees(0.0), got {err:?}"
        );

        // Negative
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { min_angle_degrees: -10.0, ..MesherOptions::default() },
        )
        .expect_err("negative min_angle_degrees must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAngleDegrees { value } if value == -10.0),
            "expected InvalidMinAngleDegrees(-10.0), got {err:?}"
        );

        // Exactly 60.0 (equilateral upper bound, excluded)
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { min_angle_degrees: 60.0, ..MesherOptions::default() },
        )
        .expect_err("min_angle_degrees = 60.0 must be rejected (equilateral upper bound, excluded)");
        assert!(
            matches!(err, MesherError::InvalidMinAngleDegrees { value } if value == 60.0),
            "expected InvalidMinAngleDegrees(60.0), got {err:?}"
        );

        // Above 60.0
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions { min_angle_degrees: 90.0, ..MesherOptions::default() },
        )
        .expect_err("min_angle_degrees > 60.0 must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAngleDegrees { value } if value == 90.0),
            "expected InvalidMinAngleDegrees(90.0), got {err:?}"
        );

        // A small positive value is valid
        mesh_mid_surface(
            &empty,
            &MesherOptions { min_angle_degrees: 0.001, ..MesherOptions::default() },
        )
        .expect("min_angle_degrees = 0.001 must be accepted");
    }

    // ── Steps 7-8: input-mesh validation tests ────────────────────────────────

    /// `mesh_mid_surface` rejects a mesh where `thickness.len() ≠ vertices.len()`.
    #[test]
    fn mesh_mid_surface_rejects_inconsistent_mesh_lengths() {
        // 1 vertex, 0 thickness entries
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0]],
            triangles: vec![],
            thickness: vec![], // length mismatch: 0 ≠ 1
        };
        let err = mesh_mid_surface(&mesh, &MesherOptions::default())
            .expect_err("thickness/vertices length mismatch must be rejected");
        assert!(
            matches!(
                err,
                MesherError::InconsistentInputMesh {
                    vertices_len: 1,
                    thickness_len: 0,
                }
            ),
            "expected InconsistentInputMesh {{ vertices_len: 1, thickness_len: 0 }}, got {err:?}"
        );

        // 0 vertices, 2 thickness entries (reversed mismatch)
        let mesh2 = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![1.0, 2.0], // length mismatch: 2 ≠ 0
        };
        let err2 = mesh_mid_surface(&mesh2, &MesherOptions::default())
            .expect_err("extra thickness entries must be rejected");
        assert!(
            matches!(
                err2,
                MesherError::InconsistentInputMesh {
                    vertices_len: 0,
                    thickness_len: 2,
                }
            ),
            "expected InconsistentInputMesh {{ vertices_len: 0, thickness_len: 2 }}, got {err2:?}"
        );
    }

    /// `mesh_mid_surface` rejects a triangle whose vertex index ≥ `vertices.len()`.
    #[test]
    fn mesh_mid_surface_rejects_out_of_range_triangle_index() {
        // 1 vertex, triangle references index 1 (out of range for 1-element array)
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0]],
            triangles: vec![[0, 1, 0]], // vertex_index 1 is out of range
            thickness: vec![1.0],
        };
        let err = mesh_mid_surface(&mesh, &MesherOptions::default())
            .expect_err("out-of-range triangle index must be rejected");
        assert!(
            matches!(
                err,
                MesherError::OutOfRangeTriangleIndex {
                    triangle_index: 0,
                    vertex_index: 1,
                    vertices_len: 1,
                }
            ),
            "expected OutOfRangeTriangleIndex {{ tri: 0, vi: 1, vlen: 1 }}, got {err:?}"
        );
    }
}
