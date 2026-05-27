//! Through-thickness element-count diagnostic — post-mesh under-resolution check.
//!
//! Per the v0.3 FEA PRD: a thin region with only one tet through its thickness
//! produces an FEA mesh that's almost guaranteed to under-predict deflection
//! by an order of magnitude — the linear shape functions of a P1 tet can't
//! capture bending across a thickness without at least two layers. This
//! diagnostic scans the produced volume mesh and emits a warning whenever
//! the count of layers along the smallest BBox dimension falls below the
//! configured threshold (default 2).
//!
//! # v0.3 simplification (single-region body)
//!
//! True per-face thickness identification requires the body's topology
//! (face graph) to map mesh elements back to their originating B-rep face —
//! that infrastructure is part of the FEA-engine wiring (sibling task #2924)
//! and the topology-selectors PRD, not the meshing layer. For v0.3 the whole
//! body is treated as a single region with `region_index = 0`. The struct
//! exposes a `region_index: usize` slot so v0.4+ refinement can attach
//! face/region IDs without changing the warning shape.
//!
//! # Algorithm
//!
//! 1. Compute the surface-mesh axis-aligned bounding box dimensions
//!    `(Δx, Δy, Δz)`.
//! 2. Pick the smallest of the three as the "thickness direction".
//! 3. Project tet corner-vertex extents onto that axis to estimate the
//!    typical per-tet thinnest-axis span (average over all tets) — this is
//!    the bin width for layer detection. Using a per-tet measurement (rather
//!    than `thickness / cbrt(n_tets)`) avoids the isotropy assumption that
//!    breaks for thin slabs where tets stack in the thinnest direction.
//! 4. Project tet centroids onto the thinnest axis, sort the resulting 1-D
//!    scalars, and count distinct layers — two consecutive sorted centroids
//!    are different layers when their gap exceeds half the bin width.
//! 5. If the layer count is below `cfg.min_elements_through_thickness`,
//!    emit a single warning naming the count, the thickness, and a
//!    suggested smaller `mesh_size`.

use reify_ir::{Mesh, VolumeMesh};

/// Configuration for the [`through_thickness_check`] diagnostic.
#[derive(Debug, Clone, Copy)]
pub struct ThroughThicknessConfig {
    /// Minimum acceptable number of tet layers through any thickness
    /// direction. Default is `2` — fewer than two layers means a P1 tet
    /// element basis can't capture bending in that direction.
    pub min_elements_through_thickness: u32,
}

impl Default for ThroughThicknessConfig {
    fn default() -> Self {
        Self {
            min_elements_through_thickness: 2,
        }
    }
}

/// One under-resolution finding produced by [`through_thickness_check`].
#[derive(Debug, Clone)]
pub struct ThroughThicknessWarning {
    /// Region identifier. v0.3 always emits `0` (single-region body); the
    /// field exists so v0.4+ per-face refinement can attach face/region IDs
    /// without changing the warning shape.
    pub region_index: usize,
    /// Thickness span in millimetres along the smallest BBox dimension.
    pub thickness: f64,
    /// Layer count detected along the thickness direction.
    pub element_count: u32,
    /// Human-readable diagnostic message; includes the count and a numeric
    /// `mesh_size` suggestion.
    pub message: String,
}

/// Scan a volume mesh for through-thickness under-resolution and return one
/// warning per region (v0.3: at most one — single-region body).
pub fn through_thickness_check(
    volume: &VolumeMesh,
    surface: &Mesh,
    cfg: ThroughThicknessConfig,
) -> Vec<ThroughThicknessWarning> {
    if surface.vertices.is_empty() || volume.tet_indices.is_empty() {
        return Vec::new();
    }

    // -----------------------------------------------------------------
    // (1) Axis-aligned BBox of the surface mesh, plus identify the
    //     thickness axis (smallest dim).
    // -----------------------------------------------------------------
    let (mut min_x, mut min_y, mut min_z) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y, mut max_z) =
        (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for v in surface.vertices.chunks_exact(3) {
        let (x, y, z) = (v[0] as f64, v[1] as f64, v[2] as f64);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        min_z = min_z.min(z);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        max_z = max_z.max(z);
    }
    let dx = max_x - min_x;
    let dy = max_y - min_y;
    let dz = max_z - min_z;
    let mut thickness = dx;
    let mut axis: usize = 0;
    if dy < thickness {
        thickness = dy;
        axis = 1;
    }
    if dz < thickness {
        thickness = dz;
        axis = 2;
    }

    // -----------------------------------------------------------------
    // (2) Project tet centroids onto the thickness axis, then count
    //     distinct discrete layers using a bin-width derived from the
    //     thickness span and tet count (one tet per cell along the
    //     thinnest direction is the v0.3 first-cut estimator).
    // -----------------------------------------------------------------
    let stride = match volume.element_order {
        reify_ir::ElementOrderTag::P1 => 4usize,
        reify_ir::ElementOrderTag::P2 => 10usize,
    };
    let n_tets = volume.tet_indices.len() / stride;
    if n_tets == 0 {
        return Vec::new();
    }

    let pick_axis = |x: f64, y: f64, z: f64| match axis {
        0 => x,
        1 => y,
        _ => z,
    };

    let mut centroids: Vec<f64> = Vec::with_capacity(n_tets);
    let mut tet_extents_sum = 0.0_f64;
    for tet in volume.tet_indices.chunks_exact(stride) {
        // Use only the first 4 corner indices for the centroid; for P2
        // tets, indices [4..10] are edge midpoints — including them would
        // bias the centroid and is not the geometric centroid Gmsh would
        // report.
        let mut sum = 0.0_f64;
        let mut min_axis = f64::INFINITY;
        let mut max_axis = f64::NEG_INFINITY;
        for &i in &tet[..4] {
            let off = i as usize * 3;
            let (x, y, z) = (
                volume.vertices[off] as f64,
                volume.vertices[off + 1] as f64,
                volume.vertices[off + 2] as f64,
            );
            let projected = pick_axis(x, y, z);
            sum += projected;
            min_axis = min_axis.min(projected);
            max_axis = max_axis.max(projected);
        }
        centroids.push(sum * 0.25);
        tet_extents_sum += max_axis - min_axis;
    }

    // Fail-closed: non-finite centroids corrupt the layer-counting walk in two
    // distinct ways. NaN poisons partial_cmp (NaN is treated as Equal against
    // every value), silently scrambling the sort. Inf poisons bin_width:
    // avg_tet_extent → Inf → half_bin → Inf → `(w[1] - w[0]).abs() > Inf` is
    // always false → layer_count collapses to 1 regardless of geometry. Both
    // are upstream pathology in the volume mesh vertex data — conditions that
    // must surface to operators rather than producing a meaningless layer count
    // that the FEA pipeline trusts. Emit a WARN and early-return with no
    // findings. Operators can filter via
    // `RUST_LOG=reify_kernel_gmsh::through_thickness=warn`.
    //
    // Checking centroids is sufficient because the early-return below skips all
    // downstream uses of tet_extents_sum; we do not rely on tet_extents_sum
    // being non-finite itself (f64::min/max with one NaN operand returns the
    // finite operand). A future refactor that uses tet_extents_sum on a
    // different code path would need to extend this guard.
    //
    // Surface vertex finiteness is assumed; an Inf surface vertex would be
    // visible as a non-finite `thickness`/`axis` from the BBox walk above,
    // but NaN surface vertices are silently absorbed by `f64::min`/`f64::max`
    // and remain the caller's responsibility.
    if centroids.iter().any(|c| !c.is_finite()) {
        tracing::warn!(
            target: "reify_kernel_gmsh::through_thickness",
            reason = "non_finite_centroid",
            n_tets = n_tets,
            "Through-thickness diagnostic skipped: encountered non-finite centroid \
             (likely upstream pathology in volume mesh vertex data); returning \
             no warnings to avoid silently corrupting the layer-counting walk"
        );
        return Vec::new();
    }

    centroids.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Bin width: average per-tet extent along the thinnest axis. This is the
    // typical "vertical reach" of a single tet in the thickness direction,
    // so two centroids separated by more than half a bin width sit in
    // distinct layers. Unlike a `thickness / cbrt(n_tets)` heuristic, this
    // measurement is correct for *both* isotropic meshes and thin-slab
    // meshes where tets stack along the thinnest axis. v0.4+ may refine this
    // to a per-region clustering analysis once topology selectors carry face
    // IDs into mesh metadata.
    let avg_tet_extent = tet_extents_sum / (n_tets as f64);
    // Floor to avoid zero/NaN on degenerate meshes (tets fully collapsed in
    // the thinnest direction would otherwise produce a 0-width bin).
    let bin_width = avg_tet_extent.max(thickness * 1e-9);

    // Count distinct bins by walking the sorted centroids and bumping the
    // count whenever the gap to the previous centroid exceeds half a bin.
    let mut layer_count: u32 = 1;
    let half_bin = bin_width * 0.5;
    for w in centroids.windows(2) {
        if (w[1] - w[0]).abs() > half_bin {
            layer_count += 1;
        }
    }

    // -----------------------------------------------------------------
    // (3) Compare against threshold; emit a single warning if under-
    //     resolved.
    // -----------------------------------------------------------------
    if layer_count < cfg.min_elements_through_thickness {
        let suggested_size = thickness / (cfg.min_elements_through_thickness as f64 * 2.0);
        let message = format!(
            "Through-thickness under-resolution: detected {} tet layer(s) along the thinnest \
             body dimension ({:.6}), fewer than {} elements through thickness. \
             Suggest setting mesh_size = {:.6} (≈ thickness / {}) for adequate FEA resolution.",
            layer_count,
            thickness,
            cfg.min_elements_through_thickness,
            suggested_size,
            cfg.min_elements_through_thickness * 2,
        );
        return vec![ThroughThicknessWarning {
            region_index: 0,
            thickness,
            element_count: layer_count,
            message,
        }];
    }

    Vec::new()
}
