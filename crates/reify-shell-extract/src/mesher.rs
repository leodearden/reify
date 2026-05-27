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
//! **Deferred (v0.4):** The full Laplacian smoothing
//! (`max_remesh_iterations > 0`) and MMG2D-style remeshing pipeline. Callers
//! may set `max_remesh_iterations > 0` but will receive the same
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
    /// `merge_tolerance` must be strictly positive, finite, and normal (not
    /// subnormal). Subnormal (denormal) positive values satisfy `> 0.0 &&
    /// is_finite()`, but produce reciprocals large enough (~2^1022 for the
    /// largest subnormals) that `coord * inv_tol` overflows to ±Inf for any
    /// non-tiny coordinate, silently collapsing all vertices into the ±Inf
    /// saturation buckets and corrupting the dedup hash.
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
    /// A vertex coordinate in `mesh.vertices` is non-finite (`NaN`, `+Inf`, or
    /// `-Inf`). Non-finite coordinates silently corrupt the dedup hash:
    /// `NaN` casts to `0` (all NaN vertices collapse into the origin bin) and
    /// `±Inf` saturates to `i64::MIN`/`i64::MAX` (all infinite vertices
    /// merge into the same boundary bin). Both failure modes produce incorrect
    /// mesh topology without any runtime error.
    NonFiniteVertex {
        /// Zero-based index of the vertex in `mesh.vertices` whose coordinate
        /// is non-finite.
        vertex_index: usize,
        /// The specific non-finite coordinate value (`NaN`, `+Inf`, or `-Inf`).
        coord: f64,
    },
    /// A thickness entry in `mesh.thickness` is non-finite (`NaN`, `+Inf`, or
    /// `-Inf`). Non-finite thickness values would poison the per-vertex averaged
    /// thickness during duplicate-vertex merging in `dedup_vertices`, propagating
    /// a `NaN`/`±Inf` thickness to the output mesh and downstream FEA stiffness
    /// assembly without any diagnostic.
    NonFiniteThickness {
        /// Zero-based index into `mesh.thickness` (also the corresponding index
        /// in `mesh.vertices`) whose thickness value is non-finite.
        vertex_index: usize,
        /// The specific non-finite thickness value (`NaN`, `+Inf`, or `-Inf`).
        value: f64,
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
    /// rounds of smoothing. Carries the full quality metrics and the
    /// de-duplicated mesh so callers can inspect failing triangles, drive
    /// their own repair, or forward the mesh for visualisation.
    QualityBelowThreshold {
        /// Full quality metrics over the de-duplicated mesh (worst-case
        /// values, failed count, total counts).
        metrics: QualityMetrics,
        /// The de-duplicated mesh that failed the quality gate. Exposed so
        /// callers can attempt their own remediation without re-running dedup.
        mesh: MidSurfaceMesh,
        /// Number of smoothing iterations attempted before failing.
        remesh_iterations: u32,
    },
}

impl std::fmt::Display for MesherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MesherError::InvalidMergeTolerance { value } => write!(
                f,
                "merge_tolerance must be strictly positive, finite, and normal \
                 (got {value}); subnormal values produce reciprocals that overflow \
                 vertex bin keys, silently collapsing all vertices into the ±Inf \
                 saturation buckets; use 1e-9 for bit-exact binary-MC output or a \
                 larger value for float-noisy producers"
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
            MesherError::NonFiniteVertex {
                vertex_index,
                coord,
            } => write!(
                f,
                "vertex {vertex_index} contains non-finite coordinate {coord}; \
                 vertex coordinates must be finite (NaN silently collapses into \
                 the dedup origin bin; ±Inf saturates to i64 boundary bins, \
                 merging unrelated vertices and corrupting mesh topology)"
            ),
            MesherError::NonFiniteThickness {
                vertex_index,
                value,
            } => write!(
                f,
                "thickness[{vertex_index}] is non-finite ({value}); thickness values \
                 must be finite (NaN/±Inf would poison averaged thickness on \
                 duplicate-vertex merges and propagate to downstream FEA stiffness \
                 matrix assembly)"
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
                metrics,
                remesh_iterations,
                ..
            } => write!(
                f,
                "mesh quality below threshold after {remesh_iterations} remesh \
                 iteration(s): {} triangle(s) failed; \
                 worst aspect_ratio={:.6}, \
                 worst min_angle={:.3}°",
                metrics.failed_triangle_count, metrics.min_aspect_ratio, metrics.min_angle_degrees,
            ),
        }
    }
}

impl std::error::Error for MesherError {}

// ── Crate-visible quantization utility ───────────────────────────────────────

/// Returns `true` if `value` is a valid inverse-tolerance operand: strictly
/// positive, finite, and normal (not subnormal).
///
/// **Structural boundary, not safety boundary.**  The gate rejects subnormals
/// at the float type-system boundary.  This is *not* a magnitude-based safety
/// check: the smallest accepted value, `f64::MIN_POSITIVE` (2^-1022 ≈ 2.2e-308),
/// has a reciprocal of ~4.5e307 — large enough that
/// `(coord * inv_tol).round() as i64` saturates to `i64::MIN` / `i64::MAX`
/// for any coordinate with `|coord| ≥ 1.0`.  Very small *normal* tolerances
/// therefore produce the same silent vertex-collapse pathology that this gate's
/// previous docstring described as exclusive to subnormals.
///
/// **Residual hazard.**  For the full magnitude-based caller obligation —
/// `coord_max / merge_tolerance < i64::MAX as f64` (~9.2e18) — see the
/// **Preconditions** sections of [`mesh_mid_surface`] and
/// [`crate::pruning::prune_branches`].
///
/// Used by both [`mesh_mid_surface`] and [`crate::pruning::prune_branches`]
/// to gate their respective tolerance parameters at the same structural rule.
#[inline]
pub(crate) fn is_quantization_tolerance_valid(value: f64) -> bool {
    value > 0.0 && value.is_finite() && !value.is_subnormal()
}

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
///
/// **NaN / infinity saturation.** Bin keys are computed via
/// `(coord * inv_tol).round() as i64`. Rust's `as` cast saturates:
///
/// - `NaN * inv_tol` is `NaN`, which rounds to `NaN`, which casts to `0` —
///   all NaN coordinates land in the same bin at the origin.
/// - `|coord * inv_tol| ≥ i64::MAX as f64` (≈ 9.22e18) saturates to
///   `i64::MIN` or `i64::MAX`; multiple extreme-magnitude vertices will
///   collide into the same boundary bin with averaged thickness.
///
/// At the default `merge_tolerance = 1e-9`, the saturation threshold is
/// `|coord| ≈ 9.2e9` — far outside any practical CAD coordinate range. For
/// typical inputs this is benign; for untrusted sources, callers should
/// validate coordinates (finite, bounded magnitude) before invoking
/// [`mesh_mid_surface`].
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
    let area = 0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();

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
        (dot(v1, v2) / (l1 * l2))
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    };

    let a0 = angle_between(e01, e02); // angle at p0
    let a1 = angle_between(neg(e01), e12); // angle at p1
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
/// | [`MesherError::InvalidMergeTolerance`] | `merge_tolerance ≤ 0`, non-finite, or subnormal  |
/// | [`MesherError::InvalidMinAspectRatio`] | `min_aspect_ratio ∉ (0, 1]` |
/// | [`MesherError::InvalidMinAngleDegrees`] | `min_angle_degrees ∉ (0, 60)` |
/// | [`MesherError::InconsistentInputMesh`] | `thickness.len() ≠ vertices.len()` |
/// | [`MesherError::NonFiniteVertex`] | any vertex coordinate is `NaN` or `±Inf` |
/// | [`MesherError::NonFiniteThickness`] | any thickness entry is `NaN` or `±Inf` |
/// | [`MesherError::OutOfRangeTriangleIndex`] | any triangle index ≥ `vertices.len()` |
/// | [`MesherError::QualityBelowThreshold`] | quality gate fails after all remesh iterations |
///
/// # Preconditions
///
/// NaN and ±Inf vertex coordinates are actively rejected — see
/// [`MesherError::NonFiniteVertex`]. The residual hazard is **large but finite**
/// coordinates: the vertex-dedup step computes bin keys via
/// `(coord * (1.0 / merge_tolerance)).round() as i64`; Rust's `as` cast
/// saturates on overflow, so coordinates with
/// `|coord / merge_tolerance| ≥ i64::MAX as f64` (~9.22e18) saturate to
/// `i64::MIN` / `i64::MAX`, merging unrelated extreme-magnitude vertices with
/// averaged thickness.
///
/// At the default `merge_tolerance = 1e-9` the saturation threshold is
/// `|coord| ≈ 9.2e9`, far outside any practical CAD coordinate range.
/// Inputs from untrusted sources should validate that coordinates are within
/// reasonable magnitude before calling this function.
pub fn mesh_mid_surface(
    mesh: &MidSurfaceMesh,
    options: &MesherOptions,
) -> Result<MesherResult, MesherError> {
    // ── 1. Validate options ───────────────────────────────────────────────────
    // Reject merge_tolerance if it is ≤ 0, non-finite, or subnormal.
    // `is_quantization_tolerance_valid` centralises the rule shared with
    // `prune_branches`.  See its doc for the structural-boundary rationale
    // and the residual saturation hazard for small-but-normal tolerances.
    if !is_quantization_tolerance_valid(options.merge_tolerance) {
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
    // Scan vertices and thickness in a single pass (lengths confirmed equal above).
    // Non-finite coords would silently corrupt bin keys in `dedup_vertices`
    // (NaN→0, ±Inf→i64 extremes); a NaN/±Inf thickness would poison the averaged
    // thickness on duplicate-vertex merges and propagate to downstream FEA.
    // Vertex-coordinate errors are reported before thickness errors at the same index.
    for (vi, (v, &t)) in mesh.vertices.iter().zip(mesh.thickness.iter()).enumerate() {
        for &c in v.iter() {
            if !c.is_finite() {
                return Err(MesherError::NonFiniteVertex {
                    vertex_index: vi,
                    coord: c,
                });
            }
        }
        if !t.is_finite() {
            return Err(MesherError::NonFiniteThickness {
                vertex_index: vi,
                value: t,
            });
        }
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
    let (new_vertices, new_thickness, remap) = dedup_vertices(mesh, options.merge_tolerance);
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
    // Compute counts here (before the gate) so both the error and success
    // paths can reuse them without moving the vectors prematurely.
    let vertex_count = new_vertices.len();
    let triangle_count = new_triangles.len();
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

    // Canary: if no triangles were iterated the loop body never ran, so both
    // sentinels must still hold their initial `f64::INFINITY` values.  This
    // is now a debug_assert rather than a defensive reset: the reset is
    // redundant (sentinels start at INFINITY and no assignment occurs when the
    // loop is empty), and the assert documents the loop invariant and catches
    // any future change that modifies the sentinel initialisation or loop body
    // without preserving this post-condition.
    if new_triangles.is_empty() {
        debug_assert!(
            worst_aspect_ratio.is_infinite() && worst_min_angle.is_infinite(),
            "loop invariant violated: empty triangle list must leave sentinels at \
             f64::INFINITY (worst_aspect_ratio={worst_aspect_ratio}, \
             worst_min_angle={worst_min_angle})"
        );
    }

    // ── 6. Quality gate ───────────────────────────────────────────────────────
    if failed_count > 0 {
        // Deferred (v0.4): Laplacian smoothing / MMG2D-style remeshing.
        // Both max_remesh_iterations == 0 and > 0 fall through to the same
        // error path. Full smoothing is a follow-up task.
        //
        // The de-duplicated mesh is returned inside the error so callers can
        // inspect failing triangles, attempt their own repair, or visualise
        // the geometry without re-running the dedup step.
        return Err(MesherError::QualityBelowThreshold {
            metrics: QualityMetrics {
                triangle_count,
                vertex_count,
                min_aspect_ratio: worst_aspect_ratio,
                min_angle_degrees: worst_min_angle,
                failed_triangle_count: failed_count,
            },
            mesh: MidSurfaceMesh {
                vertices: new_vertices,
                triangles: new_triangles,
                thickness: new_thickness,
            },
            remesh_iterations: 0,
        });
    }

    // ── 7. Return success ─────────────────────────────────────────────────────
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
    use super::{MesherError, MesherOptions, MesherResult, QualityMetrics, mesh_mid_surface};
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
        assert_eq!(
            result.metrics.triangle_count, 0,
            "empty input → 0 triangles"
        );
        assert_eq!(result.metrics.vertex_count, 0, "empty input → 0 vertices");
        assert_eq!(result.remesh_iterations, 0, "no remeshing on empty input");

        // Compile probes: all error variants are publicly named and constructible.
        let _: MesherError = MesherError::InvalidMergeTolerance { value: 0.0 };
        let _: MesherError = MesherError::InvalidMinAspectRatio { value: 0.0 };
        let _: MesherError = MesherError::InvalidMinAngleDegrees { value: 0.0 };
        let _: MesherError = MesherError::InconsistentInputMesh {
            vertices_len: 0,
            thickness_len: 0,
        };
        let _: MesherError = MesherError::NonFiniteVertex {
            vertex_index: 0,
            coord: 0.0,
        };
        let _: MesherError = MesherError::NonFiniteThickness {
            vertex_index: 0,
            value: 0.0,
        };
        let _: MesherError = MesherError::OutOfRangeTriangleIndex {
            triangle_index: 0,
            vertex_index: 0,
            vertices_len: 0,
        };
        let _: MesherError = MesherError::QualityBelowThreshold {
            metrics: QualityMetrics {
                triangle_count: 0,
                vertex_count: 0,
                min_aspect_ratio: 0.0,
                min_angle_degrees: 0.0,
                failed_triangle_count: 0,
            },
            mesh: MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
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

    /// Pin `MesherOptions::default()` struct shape via pattern destructuring.
    ///
    /// The full-field destructure is a compile-time field-rename guard: if any
    /// field is renamed or removed, this test fails at compile time rather than
    /// silently passing with stale bindings.
    ///
    /// Value semantics (the *meaning* of each default) are covered by the
    /// behavioural tests below (sliver-rejection, equilateral-pass,
    /// options-validation), so value `assert_eq!`s are not duplicated here.
    ///
    /// Mirrors `mid_surface_options_defaults_pin_empirical_constants` in
    /// `mid_surface.rs`.
    #[test]
    fn mesher_options_defaults_pin_empirical_constants() {
        // All four fields named explicitly — compile error on any field rename.
        let MesherOptions {
            merge_tolerance: _,
            min_aspect_ratio: _,
            min_angle_degrees: _,
            max_remesh_iterations: _,
        } = MesherOptions::default();
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
            &MesherOptions {
                merge_tolerance: -1.0,
                ..MesherOptions::default()
            },
        )
        .expect_err("negative merge_tolerance must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMergeTolerance { value } if value == -1.0),
            "expected InvalidMergeTolerance(-1.0), got {err:?}"
        );

        // Zero tolerance
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions {
                merge_tolerance: 0.0,
                ..MesherOptions::default()
            },
        )
        .expect_err("zero merge_tolerance must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMergeTolerance { value } if value == 0.0),
            "expected InvalidMergeTolerance(0.0), got {err:?}"
        );

        // NaN tolerance
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions {
                merge_tolerance: f64::NAN,
                ..MesherOptions::default()
            },
        )
        .expect_err("NaN merge_tolerance must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMergeTolerance { value } if value.is_nan()),
            "expected InvalidMergeTolerance(NaN), got {err:?}"
        );

        // +Infinity tolerance
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions {
                merge_tolerance: f64::INFINITY,
                ..MesherOptions::default()
            },
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
            &MesherOptions {
                min_aspect_ratio: 0.0,
                ..MesherOptions::default()
            },
        )
        .expect_err("min_aspect_ratio = 0.0 must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAspectRatio { value } if value == 0.0),
            "expected InvalidMinAspectRatio(0.0), got {err:?}"
        );

        // Negative
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions {
                min_aspect_ratio: -0.5,
                ..MesherOptions::default()
            },
        )
        .expect_err("negative min_aspect_ratio must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAspectRatio { value } if value == -0.5),
            "expected InvalidMinAspectRatio(-0.5), got {err:?}"
        );

        // Above 1.0
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions {
                min_aspect_ratio: 1.001,
                ..MesherOptions::default()
            },
        )
        .expect_err("min_aspect_ratio > 1.0 must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAspectRatio { value } if value == 1.001),
            "expected InvalidMinAspectRatio(1.001), got {err:?}"
        );

        // Exactly 1.0 is valid (equilateral-only gate)
        mesh_mid_surface(
            &empty,
            &MesherOptions {
                min_aspect_ratio: 1.0,
                ..MesherOptions::default()
            },
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
            &MesherOptions {
                min_angle_degrees: 0.0,
                ..MesherOptions::default()
            },
        )
        .expect_err("min_angle_degrees = 0.0 must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAngleDegrees { value } if value == 0.0),
            "expected InvalidMinAngleDegrees(0.0), got {err:?}"
        );

        // Negative
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions {
                min_angle_degrees: -10.0,
                ..MesherOptions::default()
            },
        )
        .expect_err("negative min_angle_degrees must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAngleDegrees { value } if value == -10.0),
            "expected InvalidMinAngleDegrees(-10.0), got {err:?}"
        );

        // Exactly 60.0 (equilateral upper bound, excluded)
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions {
                min_angle_degrees: 60.0,
                ..MesherOptions::default()
            },
        )
        .expect_err(
            "min_angle_degrees = 60.0 must be rejected (equilateral upper bound, excluded)",
        );
        assert!(
            matches!(err, MesherError::InvalidMinAngleDegrees { value } if value == 60.0),
            "expected InvalidMinAngleDegrees(60.0), got {err:?}"
        );

        // Above 60.0
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions {
                min_angle_degrees: 90.0,
                ..MesherOptions::default()
            },
        )
        .expect_err("min_angle_degrees > 60.0 must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMinAngleDegrees { value } if value == 90.0),
            "expected InvalidMinAngleDegrees(90.0), got {err:?}"
        );

        // A small positive value is valid
        mesh_mid_surface(
            &empty,
            &MesherOptions {
                min_angle_degrees: 0.001,
                ..MesherOptions::default()
            },
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

    // ── Steps 9-10: vertex-deduplication test ─────────────────────────────────

    /// Vertex de-duplication merges bit-exact duplicate vertices and averages
    /// their thickness values.
    ///
    /// Fixture: 4 input vertices where vertices 1 and 2 are identical at
    /// `(1, 0, 0)`. Two triangles share this edge; the duplicate should be
    /// merged into one vertex with averaged thickness.
    #[test]
    fn mesh_mid_surface_deduplicates_duplicate_vertices() {
        // Vertex layout:
        //   0: (0,0,0)  thickness 1.0
        //   1: (1,0,0)  thickness 2.0  ← duplicate
        //   2: (1,0,0)  thickness 4.0  ← duplicate of vertex 1
        //   3: (0,1,0)  thickness 1.0
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0], // bit-exact duplicate of vertex 1
                [0.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 3], [3, 2, 0]],
            thickness: vec![1.0, 2.0, 4.0, 1.0],
        };

        // Use very relaxed quality thresholds so the quality gate doesn't fire.
        let opts = MesherOptions {
            min_aspect_ratio: 1e-6,
            min_angle_degrees: 0.001,
            ..MesherOptions::default()
        };

        let result = mesh_mid_surface(&mesh, &opts)
            .expect("two-triangle mesh with duplicate vertex should succeed");

        // De-duplication: 4 input vertices → 3 unique vertices.
        assert_eq!(
            result.mesh.vertices.len(),
            3,
            "4 input vertices (one duplicate pair) → 3 unique vertices"
        );
        assert_eq!(
            result.metrics.vertex_count, 3,
            "metrics.vertex_count must equal de-duplicated vertex count"
        );
        // Triangle count preserves topology.
        assert_eq!(
            result.metrics.triangle_count, 2,
            "de-duplication must not remove triangles"
        );
        // All remapped triangle indices are in-range.
        for tri in &result.mesh.triangles {
            for &vi in tri.iter() {
                assert!(
                    (vi as usize) < result.mesh.vertices.len(),
                    "triangle index {vi} is out of range for {} vertices",
                    result.mesh.vertices.len()
                );
            }
        }
        // Thickness for the merged (1,0,0) vertex = average of 2.0 and 4.0 = 3.0.
        // Find the index of the (1,0,0) vertex in the de-duplicated set.
        let merged_idx = result
            .mesh
            .vertices
            .iter()
            .position(|&v| (v[0] - 1.0).abs() < 1e-12 && v[1].abs() < 1e-12 && v[2].abs() < 1e-12)
            .expect("de-duplicated mesh must contain the (1,0,0) vertex");
        assert!(
            (result.mesh.thickness[merged_idx] - 3.0).abs() < 1e-12,
            "merged thickness must be average of 2.0 and 4.0 = 3.0, got {}",
            result.mesh.thickness[merged_idx]
        );
    }

    // ── Steps 11-12: quality-metrics correctness tests ────────────────────────

    /// Quality metrics for an equilateral triangle: aspect ratio = 1.0, min
    /// angle = 60°.
    ///
    /// Vertices: `[0,0,0]`, `[1,0,0]`, `[0.5, sqrt(3)/2, 0]` — side length 1.
    #[test]
    fn mesh_mid_surface_quality_metrics_equilateral_triangle() {
        let h = (3.0f64).sqrt() / 2.0; // height of unit equilateral triangle
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.5, h, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![1.0, 1.0, 1.0],
        };

        // Relaxed thresholds: well below 1.0 and 60°
        let opts = MesherOptions {
            min_aspect_ratio: 1e-6,
            min_angle_degrees: 0.001,
            ..MesherOptions::default()
        };
        let result = mesh_mid_surface(&mesh, &opts).expect("equilateral triangle should pass");

        assert!(
            (result.metrics.min_aspect_ratio - 1.0).abs() < 1e-9,
            "equilateral triangle must have aspect ratio 1.0, got {}",
            result.metrics.min_aspect_ratio
        );
        assert!(
            (result.metrics.min_angle_degrees - 60.0).abs() < 1e-9,
            "equilateral triangle must have min angle 60°, got {}",
            result.metrics.min_angle_degrees
        );
    }

    /// Quality metrics for a right-isosceles triangle: `[0,0,0]`, `[1,0,0]`,
    /// `[0,1,0]`. Min angle should be ~45° and aspect ratio < 1.0.
    #[test]
    fn mesh_mid_surface_quality_metrics_right_triangle() {
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![1.0, 1.0, 1.0],
        };

        let opts = MesherOptions {
            min_aspect_ratio: 1e-6,
            min_angle_degrees: 0.001,
            ..MesherOptions::default()
        };
        let result = mesh_mid_surface(&mesh, &opts).expect("right triangle should pass");

        // The two equal legs have 45° angles; the right angle is 90°.
        assert!(
            (result.metrics.min_angle_degrees - 45.0).abs() < 1e-9,
            "right-isosceles triangle must have min angle 45°, got {}",
            result.metrics.min_angle_degrees
        );
        assert!(
            result.metrics.min_aspect_ratio > 0.0 && result.metrics.min_aspect_ratio < 1.0,
            "right triangle aspect ratio must be in (0, 1), got {}",
            result.metrics.min_aspect_ratio
        );
    }

    // ── Steps 13-14: quality-gate test ───────────────────────────────────────

    /// The quality gate fires on a near-degenerate sliver triangle.
    ///
    /// `[0,0,0]`, `[1,0,0]`, `[0.5, 1e-3, 0]` — very flat, tiny min angle —
    /// should fail the default quality thresholds (`min_aspect_ratio: 0.1`,
    /// `min_angle_degrees: 20.0`).
    #[test]
    fn mesh_mid_surface_quality_gate_fires_on_sliver_triangle() {
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.5, 1e-3, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![1.0, 1.0, 1.0],
        };

        // Use default options: min_aspect_ratio=0.1, min_angle_degrees=20.0,
        // max_remesh_iterations=0 (fail-fast).
        let err = mesh_mid_surface(&mesh, &MesherOptions::default())
            .expect_err("sliver triangle must fail the quality gate");

        match err {
            MesherError::QualityBelowThreshold {
                ref metrics,
                remesh_iterations,
                ..
            } => {
                assert!(
                    metrics.min_aspect_ratio < 0.1,
                    "sliver aspect ratio must be < 0.1 (default threshold), \
                     got {}",
                    metrics.min_aspect_ratio
                );
                assert!(
                    metrics.min_angle_degrees < 20.0,
                    "sliver min angle must be < 20° (default threshold), \
                     got {}°",
                    metrics.min_angle_degrees
                );
                assert_eq!(
                    metrics.failed_triangle_count, 1,
                    "exactly 1 triangle fails the gate"
                );
                assert_eq!(
                    remesh_iterations, 0,
                    "fail-fast: 0 remesh iterations with max_remesh_iterations=0"
                );
            }
            other => panic!("expected QualityBelowThreshold, got {other:?}"),
        }
    }

    /// Quality gate: `max_remesh_iterations > 0` still returns
    /// `QualityBelowThreshold` in v0.4 (smoothing is deferred).
    #[test]
    fn mesh_mid_surface_quality_gate_deferred_remesh_returns_same_error() {
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.5, 1e-3, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![1.0, 1.0, 1.0],
        };

        let opts = MesherOptions {
            max_remesh_iterations: 5, // non-zero, but smoothing is deferred
            ..MesherOptions::default()
        };

        let err = mesh_mid_surface(&mesh, &opts)
            .expect_err("deferred smoother: sliver still fails even with max_remesh_iterations=5");

        assert!(
            matches!(err, MesherError::QualityBelowThreshold { .. }),
            "expected QualityBelowThreshold even with max_remesh_iterations=5, got {err:?}"
        );
    }

    // ── Steps 15-16: slab end-to-end pipeline test ────────────────────────────
    //
    // Test helpers (mirrored from mid_surface.rs and segmentation.rs).
    // Duplication is intentional: mesher.rs must be self-contained, mirroring
    // the established pattern between mid_surface.rs and segmentation.rs.

    use crate::medial::MedialMask;
    use crate::mid_surface::{MidSurfaceOptions, extract_mid_surface};
    use reify_ir::value::{InterpolationKind, SampledField, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    /// Build an analytic-slab Regular3D SampledField:
    /// `φ(x,y,z) = |z| - half_thickness_voxels` on an N×N×N grid
    /// centred at the origin with unit spacing.
    fn slab_sdf_3d(half_thickness_voxels: f64, voxel_count: usize) -> SampledField {
        let n = voxel_count;
        let spacing: f64 = 1.0;
        let half_extent = (n as f64 - 1.0) / 2.0;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;
        let axis_grid: Vec<f64> = (0..n)
            .map(|idx| bounds_min + (idx as f64) * spacing)
            .collect();
        let mut data = Vec::with_capacity(n * n * n);
        for &_x in &axis_grid {
            for &_y in &axis_grid {
                for &z in &axis_grid {
                    data.push(z.abs() - half_thickness_voxels);
                }
            }
        }
        SampledField {
            name: format!("slab-{half_thickness_voxels}-{voxel_count}"),
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

    /// Build the centerline MedialMask for a z-slab on an N×N×N grid:
    /// every voxel `(i, j, center_k)` for `i, j ∈ 0..n`.
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

    /// Full T2 → T9 pipeline on a 17×17×17 slab.
    ///
    /// Validates:
    /// - `Ok(_)` from `mesh_mid_surface`
    /// - `metrics.vertex_count > 0` (non-trivial mesh)
    /// - `metrics.vertex_count < raw.vertices.len()` (dedup actually reduced count)
    /// - `metrics.triangle_count == raw.triangles.len()` (topology preserved)
    /// - All metrics are finite
    /// - `remesh_iterations == 0`
    /// - All triangle indices in result are `< result.mesh.vertices.len()`
    #[test]
    fn mesh_mid_surface_slab_end_to_end_pipeline() {
        let n = 17usize;
        let half_thickness = 3.0;

        let sdf = slab_sdf_3d(half_thickness, n);
        let mask = centerline_mask(n, &sdf);

        // T2: extract raw mid-surface mesh (may contain duplicate vertices).
        let raw = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("slab mid-surface extraction should succeed");

        assert!(
            !raw.vertices.is_empty(),
            "17×17×17 slab must produce a non-empty raw mesh"
        );

        // T9: mesh with relaxed quality thresholds so the noisy MC mesh passes
        // the gate (the MC triangles may be non-equilateral).
        let opts = MesherOptions {
            min_aspect_ratio: 1e-6,
            min_angle_degrees: 0.001,
            ..MesherOptions::default()
        };
        let result = mesh_mid_surface(&raw, &opts)
            .expect("slab mesh_mid_surface with relaxed thresholds should succeed");

        assert!(
            result.metrics.vertex_count > 0,
            "de-duplicated slab mesh must have at least one vertex"
        );
        assert!(
            result.metrics.vertex_count < raw.vertices.len(),
            "binary-MC produces 3 vertices per triangle (one per edge), so \
             de-duplication must reduce vertex_count below raw.vertices.len() \
             ({} raw → {} dedup)",
            raw.vertices.len(),
            result.metrics.vertex_count
        );
        assert_eq!(
            result.metrics.triangle_count,
            raw.triangles.len(),
            "de-duplication must preserve triangle count"
        );
        assert!(
            result.metrics.min_aspect_ratio.is_finite(),
            "min_aspect_ratio must be finite on a non-empty mesh"
        );
        assert!(
            result.metrics.min_angle_degrees.is_finite(),
            "min_angle_degrees must be finite on a non-empty mesh"
        );
        assert_eq!(
            result.remesh_iterations, 0,
            "no remeshing iterations on first-pass quality success"
        );
        // Internal consistency: all triangle indices in range.
        let vlen = result.mesh.vertices.len();
        for tri in &result.mesh.triangles {
            for &vi in tri.iter() {
                assert!(
                    (vi as usize) < vlen,
                    "triangle index {vi} is out of range for {vlen} de-duplicated vertices"
                );
            }
        }

        // Regression guard: binary-MC on a planar slab should produce
        // non-degenerate triangles (typical output is right-isosceles:
        // aspect_ratio ≈ 0.866, min_angle ≈ 45°). The bounds below are much
        // more permissive than the FEA defaults (0.1 / 20.0°) to tolerate any
        // boundary-cell effects, but would catch a catastrophic regression
        // where the extractor begins emitting near-zero-area triangles.
        assert!(
            result.metrics.min_aspect_ratio > 0.01,
            "extractor regression: binary-MC slab min_aspect_ratio ({}) must \
             be above 0.01 (expected ≈ 0.866 for right-isosceles triangles)",
            result.metrics.min_aspect_ratio
        );
        assert!(
            result.metrics.min_angle_degrees > 1.0,
            "extractor regression: binary-MC slab min_angle_degrees ({}°) must \
             be above 1.0° (expected ≈ 45° for right-isosceles triangles)",
            result.metrics.min_angle_degrees
        );
    }

    // ── task-3194 step-7: empty-triangle metrics invariant pin ────────────────

    /// When `mesh.vertices` is non-empty but `mesh.triangles` is empty, the
    /// de-duplication step runs but the quality-metrics loop iterates zero
    /// times.  The sentinel values `worst_aspect_ratio` and `worst_min_angle`
    /// are initialised to `f64::INFINITY` and must remain `INFINITY` in the
    /// returned metrics.
    ///
    /// This test pins the invariant that the upcoming `debug_assert!` canary
    /// (step-8) will encode. Removing or changing the sentinel initialisation
    /// without a pin would let a future regression slip through silently.
    #[test]
    fn mesh_mid_surface_metrics_are_infinity_when_no_triangles_to_iterate() {
        // 1 vertex, no triangles: passes all validation (consistent lengths,
        // no non-finite values, no out-of-range indices), reaches dedup, then
        // the triangle loop iterates zero times.
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0]],
            triangles: vec![],
            thickness: vec![1.0],
        };
        let opts = MesherOptions::default();

        let result = mesh_mid_surface(&mesh, &opts)
            .expect("non-empty vertices with empty triangles must return Ok");

        assert_eq!(
            result.metrics.triangle_count, 0,
            "no triangles → triangle_count = 0"
        );
        assert_eq!(
            result.metrics.failed_triangle_count, 0,
            "no triangles → no failures"
        );
        assert!(
            result.metrics.min_aspect_ratio.is_infinite(),
            "empty triangle list → min_aspect_ratio must be f64::INFINITY (sentinel), \
             got {}",
            result.metrics.min_aspect_ratio
        );
        assert!(
            result.metrics.min_angle_degrees.is_infinite(),
            "empty triangle list → min_angle_degrees must be f64::INFINITY (sentinel), \
             got {}",
            result.metrics.min_angle_degrees
        );
        assert_eq!(
            result.remesh_iterations, 0,
            "no remeshing on empty triangle list"
        );
    }

    // ── task-3194 step-3: NonFiniteVertex rejection ───────────────────────────

    /// `mesh_mid_surface` rejects meshes with non-finite vertex coordinates.
    ///
    /// A `NaN`, `+Inf`, or `-Inf` coordinate in `mesh.vertices` would silently
    /// collapse all affected vertices into the dedup origin bin (NaN→0) or into
    /// a `i64::MIN`/`i64::MAX` boundary bin (±inf), corrupting mesh topology.
    ///
    /// The check fires **before** `OutOfRangeTriangleIndex` — even a triangle
    /// that references a valid (but non-finite) vertex triggers
    /// `NonFiniteVertex` first.
    #[test]
    fn mesh_mid_surface_rejects_non_finite_vertex_coord() {
        // 2-vertex mesh: vertex 0 is valid, vertex 1 carries the bad coordinate.
        // We use triangles that only reference valid indices (0 and 1) to show
        // the check fires before OutOfRangeTriangleIndex would.
        let opts = MesherOptions::default();

        for (label, bad_coord) in [
            ("NaN x", f64::NAN),
            ("+Inf y", f64::INFINITY),
            ("-Inf z", f64::NEG_INFINITY),
        ] {
            // Substitute the bad value into one coordinate of vertex 1.
            let bad_vertex = [bad_coord, 0.0, 0.0];
            let mesh = MidSurfaceMesh {
                vertices: vec![[0.0, 0.0, 0.0], bad_vertex],
                triangles: vec![[0, 1, 0]], // valid indices — only vertex 1 is bad
                thickness: vec![1.0, 1.0],
            };

            let err = mesh_mid_surface(&mesh, &opts)
                .expect_err(&format!("mesh with {label} must be rejected"));

            match err {
                MesherError::NonFiniteVertex {
                    vertex_index,
                    coord,
                } => {
                    assert_eq!(
                        vertex_index, 1,
                        "{label}: expected vertex_index 1, got {vertex_index}"
                    );
                    if bad_coord.is_nan() {
                        assert!(coord.is_nan(), "{label}: expected NaN coord, got {coord}");
                    } else {
                        assert_eq!(
                            coord, bad_coord,
                            "{label}: expected coord {bad_coord}, got {coord}"
                        );
                    }
                }
                other => panic!("{label}: expected NonFiniteVertex, got {other:?}"),
            }
        }

        // Also verify: NaN in y coordinate, ±Inf in z coordinate.
        for (label, vi, coord_idx, bad_val) in [
            ("NaN y at vi=1", 1usize, 1usize, f64::NAN),
            ("+Inf z at vi=1", 1, 2, f64::INFINITY),
        ] {
            let mut verts = vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
            verts[vi][coord_idx] = bad_val;
            let mesh = MidSurfaceMesh {
                vertices: verts,
                triangles: vec![],
                thickness: vec![1.0, 1.0],
            };
            let err = mesh_mid_surface(&mesh, &opts)
                .expect_err(&format!("mesh with {label} must be rejected"));
            assert!(
                matches!(err, MesherError::NonFiniteVertex { vertex_index, .. } if vertex_index == vi),
                "{label}: expected NonFiniteVertex {{ vertex_index: {vi} }}, got {err:?}"
            );
        }
    }

    // ── task-3194 step-5: NonFiniteThickness rejection ────────────────────────

    /// `mesh_mid_surface` rejects meshes with non-finite thickness entries.
    ///
    /// A `NaN`, `+Inf`, or `-Inf` thickness value would poison the averaged
    /// thickness on duplicate-vertex merges and propagate to downstream FEA
    /// stiffness matrix assembly without any diagnostic.
    ///
    /// The check fires from validation (before dedup), so it fires even for
    /// vertices that are duplicates of each other — preventing silent
    /// thickness-poisoning in the merge step.
    #[test]
    fn mesh_mid_surface_rejects_non_finite_thickness() {
        // 3-vertex mesh with all-finite vertices; substitute one non-finite
        // thickness at index 1.
        let opts = MesherOptions::default();

        for (label, bad_val) in [
            ("NaN thickness", f64::NAN),
            ("+Inf thickness", f64::INFINITY),
            ("-Inf thickness", f64::NEG_INFINITY),
        ] {
            let mesh = MidSurfaceMesh {
                vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                triangles: vec![],
                thickness: vec![1.0, bad_val, 1.0],
            };

            let err = mesh_mid_surface(&mesh, &opts)
                .expect_err(&format!("mesh with {label} at index 1 must be rejected"));

            match err {
                MesherError::NonFiniteThickness {
                    vertex_index,
                    value,
                } => {
                    assert_eq!(
                        vertex_index, 1,
                        "{label}: expected vertex_index 1, got {vertex_index}"
                    );
                    if bad_val.is_nan() {
                        assert!(value.is_nan(), "{label}: expected NaN value, got {value}");
                    } else {
                        assert_eq!(
                            value, bad_val,
                            "{label}: expected value {bad_val}, got {value}"
                        );
                    }
                }
                other => panic!("{label}: expected NonFiniteThickness, got {other:?}"),
            }
        }

        // Extra fixture: duplicate vertices (0 == 1 at same position) where
        // vertex 1 has NaN thickness — the check fires from validation (step
        // ordering), not from the dedup merge step.
        let mesh_dup = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0], // duplicate of vertex 0
            ],
            triangles: vec![],
            thickness: vec![1.0, f64::NAN],
        };
        let err_dup = mesh_mid_surface(&mesh_dup, &opts).expect_err(
            "duplicate-vertex mesh with NaN thickness[1] must be rejected before dedup",
        );
        assert!(
            matches!(
                err_dup,
                MesherError::NonFiniteThickness { vertex_index: 1, value } if value.is_nan()
            ),
            "expected NonFiniteThickness {{ vertex_index: 1, NaN }}, got {err_dup:?}"
        );
    }

    // ── task-3194/3222 step-1: subnormal merge_tolerance rejection ───────────

    /// `mesh_mid_surface` rejects all subnormal `merge_tolerance` values.
    ///
    /// Subnormal (denormal) positive values are rejected by the
    /// `is_subnormal()` guard.  Even subnormals whose reciprocal fits in f64
    /// (e.g. `2^-1023` → `1/x ≈ 8.99e307`) still cause `coord * inv_tol` to
    /// overflow to ±Inf for any non-tiny coordinate, silently collapsing all
    /// vertices into one or two extreme buckets.  The earlier
    /// `!(1.0/x).is_finite()` guard failed to reject this class.
    ///
    /// Test values:
    /// - `f64::MIN_POSITIVE / 4.0` = `2^-1024`: subnormal, `1/x` overflows to
    ///   `+inf` (caught by both old and new gate).
    /// - `f64::MIN_POSITIVE / 2.0` = `2^-1023`: subnormal, but `1/x ≈ 8.99e307`
    ///   is **finite** — the new `is_subnormal()` gate catches this; the old
    ///   `!(1.0/x).is_finite()` gate did NOT.
    /// - `5e-324`: the smallest positive denormal (`2^-1074`), `1/x` = `+inf`.
    /// - `f64::MIN_POSITIVE` (`2^-1022`): the smallest **normal** positive —
    ///   must still be accepted (not subnormal).
    #[test]
    fn mesh_mid_surface_rejects_subnormal_merge_tolerance() {
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
        let err = mesh_mid_surface(
            &empty,
            &MesherOptions {
                merge_tolerance: subnormal_a,
                ..MesherOptions::default()
            },
        )
        .expect_err("subnormal merge_tolerance (f64::MIN_POSITIVE/4) must be rejected");
        assert!(
            matches!(err, MesherError::InvalidMergeTolerance { value } if value == subnormal_a),
            "expected InvalidMergeTolerance({subnormal_a}), got {err:?}"
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
        let err_c = mesh_mid_surface(
            &empty,
            &MesherOptions {
                merge_tolerance: subnormal_c,
                ..MesherOptions::default()
            },
        )
        .expect_err("subnormal merge_tolerance (f64::MIN_POSITIVE/2) must be rejected");
        assert!(
            matches!(err_c, MesherError::InvalidMergeTolerance { value } if value == subnormal_c),
            "expected InvalidMergeTolerance({subnormal_c}), got {err_c:?}"
        );

        // `5e-324` ≈ 2^-1074 — the smallest positive denormal; reciprocal is +inf.
        let subnormal_b = 5e-324_f64;
        assert!(
            subnormal_b.is_subnormal(),
            "test setup: 5e-324 must be subnormal"
        );
        let err2 = mesh_mid_surface(
            &empty,
            &MesherOptions {
                merge_tolerance: subnormal_b,
                ..MesherOptions::default()
            },
        )
        .expect_err("subnormal merge_tolerance (5e-324) must be rejected");
        assert!(
            matches!(err2, MesherError::InvalidMergeTolerance { value } if value == subnormal_b),
            "expected InvalidMergeTolerance(5e-324), got {err2:?}"
        );

        // `f64::MIN_POSITIVE` is the smallest NORMAL positive (2^-1022) — must be ACCEPTED.
        assert!(
            !f64::MIN_POSITIVE.is_subnormal(),
            "test setup: f64::MIN_POSITIVE must be normal (not subnormal)"
        );
        mesh_mid_surface(
            &empty,
            &MesherOptions {
                merge_tolerance: f64::MIN_POSITIVE,
                ..MesherOptions::default()
            },
        )
        .expect("f64::MIN_POSITIVE is the smallest normal positive — must still be accepted");
    }
}
