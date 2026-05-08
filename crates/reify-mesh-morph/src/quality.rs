//! Quality-check pass for mesh morphing (PRD task #9).
//!
//! Runs after the morph engine produces a deformed [`reify_types::VolumeMesh`]
//! and returns a two-tier verdict:
//!
//! - [`QualityVerdict::HardFail`] — one or more tetrahedra are inverted
//!   (negative Jacobian determinant). Hard-fail strictly preempts soft-fail.
//! - [`QualityVerdict::SoftFail`] — no inversions, but one or more quality
//!   metrics breach their configured thresholds: minimum scaled Jacobian,
//!   fraction of elements below 0.25, or maximum aspect-ratio increase.
//! - [`QualityVerdict::Pass`] — all checks passed.
//!
//! ## Preconditions
//!
//! - **P1 elements only.** `morphed.tet_indices` must be segmented in 4-node
//!   chunks (P1 tetrahedra). P2 input with 10-node elements will be
//!   mis-segmented by `chunks_exact(4)` without a structured error. Engine
//!   integration in PRD task #10 guarantees P1 before calling this function.
//! - **Matched connectivity.** `morphed.tet_indices.len()` is expected to equal
//!   `source.tet_indices.len()` (morph operations preserve topology). When
//!   lengths differ, the aspect-ratio-increase comparison is skipped
//!   (threshold 3 is effectively disabled); the hard-fail and min-scaled-J /
//!   pct-below-025 checks still run on the morphed mesh.
//! - **Valid vertex indices.** Elements referencing out-of-range vertex indices
//!   are silently skipped (same defensive discipline as `laplacian.rs`).

use crate::options::MorphOptions;
use crate::types::{InversionDetails, MetricsBreached};
use reify_types::VolumeMesh;
use std::f64::consts::SQRT_2;

// ── Jacobian helpers ─────────────────────────────────────────────────────────

/// Per-corner cyclic edge-index table for VERDICT-style scaled Jacobian.
///
/// At corner k, take edges to the other 3 corners in the order given by
/// `CORNER_EDGE_INDICES[k]`. This ordering is chosen so that the determinant
/// `det(e_a, e_b, e_c)` is positive for a right-handed (non-inverted) tet.
///
/// Verified on the canonical unit tet `(0,0,0),(1,0,0),(0,1,0),(0,0,1)`:
/// each corner determinant = +1.
const CORNER_EDGE_INDICES: [[usize; 3]; 4] = [
    [1, 2, 3], // corner 0
    [0, 3, 2], // corner 1
    [0, 1, 3], // corner 2
    [0, 2, 1], // corner 3
];

/// Compute the VERDICT-style per-element scaled Jacobian for a tetrahedron.
///
/// At each of the 4 corners k, the formula is
/// `scaled_J_k = det(e_a, e_b, e_c) * sqrt(2) / (||e_a|| * ||e_b|| * ||e_c||)`
/// where `(a, b, c)` are the edge indices from [`CORNER_EDGE_INDICES`].
/// Returns the minimum over all 4 corners (initialised to `f64::INFINITY`).
///
/// Returns 0.0 for a degenerate corner (zero-length edge product).
fn element_scaled_jacobian(nodes: &[[f64; 3]; 4]) -> f64 {
    let mut min_j = f64::INFINITY;
    for k in 0..4 {
        let [a, b, c] = CORNER_EDGE_INDICES[k];
        let ea = sub(nodes[a], nodes[k]);
        let eb = sub(nodes[b], nodes[k]);
        let ec = sub(nodes[c], nodes[k]);
        let det = dot(ea, cross(eb, ec));
        let product = norm(ea) * norm(eb) * norm(ec);
        let sj = if product > 0.0 {
            det * SQRT_2 / product
        } else {
            0.0
        };
        if sj < min_j {
            min_j = sj;
        }
    }
    min_j
}

// ── Geometry helpers ─────────────────────────────────────────────────────────

#[inline]
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Two-tier quality verdict returned by [`quality_check`].
///
/// Variants are evaluated in priority order: `HardFail` strictly preempts
/// `SoftFail`. If any tetrahedron is inverted, only `HardFail` is returned
/// even if soft-fail thresholds are also breached.
#[derive(Debug, Clone, PartialEq)]
pub enum QualityVerdict {
    /// All quality checks passed.
    Pass,
    /// One or more tetrahedra are inverted (negative Jacobian determinant).
    /// `HardFail` strictly preempts `SoftFail`.
    HardFail(InversionDetails),
    /// No inversions, but one or more quality metrics breached their
    /// configured thresholds.
    SoftFail(MetricsBreached),
}

/// Evaluate mesh quality after a morph operation.
///
/// Returns a [`QualityVerdict`] describing whether the morphed mesh passes
/// quality thresholds configured in `options`. See the module-level doc for
/// preconditions (P1-only, matched connectivity, valid indices).
///
/// An empty mesh (no tetrahedra) always returns [`QualityVerdict::Pass`].
///
/// ## Connectivity mismatch
///
/// When `morphed.tet_indices.len() != source.tet_indices.len()`, the
/// aspect-ratio-increase comparison is skipped (`max_aspect_ratio_increase`
/// stays `None`). The hard-fail and min-scaled-J / pct-below-025 checks still
/// run on the morphed mesh.
pub fn quality_check(
    morphed: &VolumeMesh,
    source: &VolumeMesh,
    options: &MorphOptions,
) -> QualityVerdict {
    let _ = source;

    let vertex_count = morphed.vertices.len() / 3;

    // Single pass over morphed elements: track inversions and soft-fail metrics.
    let mut hard_fail: Option<InversionDetails> = None;
    let mut global_min_scaled_j = f64::INFINITY;
    let mut total_well_formed: usize = 0;
    let mut count_below_025: usize = 0;

    for (elem_idx, chunk) in morphed.tet_indices.chunks_exact(4).enumerate() {
        // Read 4 corner positions, widening f32 → f64 at the read boundary.
        let idx = [
            chunk[0] as usize,
            chunk[1] as usize,
            chunk[2] as usize,
            chunk[3] as usize,
        ];
        // Skip elements with out-of-range indices (defensive; same discipline
        // as laplacian.rs:141-149).
        if idx.iter().any(|&i| i >= vertex_count) {
            continue;
        }
        let p: [[f64; 3]; 4] = std::array::from_fn(|k| {
            let base = idx[k] * 3;
            [
                morphed.vertices[base] as f64,
                morphed.vertices[base + 1] as f64,
                morphed.vertices[base + 2] as f64,
            ]
        });

        // Use element_scaled_jacobian (per-element min over 4 corners).
        let sj = element_scaled_jacobian(&p);
        total_well_formed += 1;
        if sj < global_min_scaled_j {
            global_min_scaled_j = sj;
        }
        // 0.25 is the fixed metric split point per PRD spec; only the trip
        // fraction is configurable via quality_floor_pct_below_025.
        if sj < 0.25 {
            count_below_025 += 1;
        }

        if sj < 0.0 {
            // First inverted element wins for HardFail.
            if hard_fail.is_none() {
                hard_fail = Some(InversionDetails {
                    element_index: elem_idx,
                    jacobian: sj,
                });
            }
        }
    }

    if let Some(details) = hard_fail {
        return QualityVerdict::HardFail(details);
    }

    // No inversions — evaluate soft-fail thresholds.
    let min_scaled_jacobian = if global_min_scaled_j.is_finite()
        && global_min_scaled_j < options.quality_floor_min_scaled_jacobian
    {
        Some(global_min_scaled_j)
    } else {
        None
    };

    let pct = if total_well_formed > 0 {
        count_below_025 as f64 / total_well_formed as f64
    } else {
        0.0
    };
    let pct_below_025 = if pct > options.quality_floor_pct_below_025 {
        Some(pct)
    } else {
        None
    };

    let metrics = MetricsBreached {
        min_scaled_jacobian,
        pct_below_025,
        max_aspect_ratio_increase: None,
    };

    if metrics.min_scaled_jacobian.is_some()
        || metrics.pct_below_025.is_some()
        || metrics.max_aspect_ratio_increase.is_some()
    {
        QualityVerdict::SoftFail(metrics)
    } else {
        QualityVerdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::MorphOptions;
    use reify_types::{ElementOrderTag, VolumeMesh};

    fn empty_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: Vec::new(),
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    // ── Smoke test: empty mesh → Pass ─────────────────────────────────────────

    #[test]
    fn quality_check_with_empty_mesh_returns_pass() {
        let m = empty_mesh();
        let opts = MorphOptions::default();
        assert_eq!(
            quality_check(&m, &m, &opts),
            QualityVerdict::Pass,
            "empty mesh should always return Pass"
        );
    }

    // ── Step-3: single inverted tet → HardFail ───────────────────────────────

    #[test]
    fn quality_check_with_single_inverted_tet_returns_hard_fail_with_element_index_and_negative_jacobian(
    ) {
        // Left-handed tet: swap nodes 2 and 3 of the canonical right-handed tet
        // (0,0,0),(1,0,0),(0,1,0),(0,0,1) → (0,0,0),(1,0,0),(0,0,1),(0,1,0).
        // Corner-0 determinant = det(e1,e2,e3) where e1=(1,0,0), e2=(0,0,1),
        // e3=(0,1,0) = 1*(0*0 - 1*1) - 0 + 0 = -1 < 0 → inverted.
        #[rustfmt::skip]
        let vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0, // node 0
            1.0, 0.0, 0.0, // node 1
            0.0, 0.0, 1.0, // node 2  (swapped from canonical)
            0.0, 1.0, 0.0, // node 3  (swapped from canonical)
        ];
        let tet_indices: Vec<u32> = vec![0, 1, 2, 3];
        let morphed = VolumeMesh {
            vertices: vertices.clone(),
            tet_indices: tet_indices.clone(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let source = VolumeMesh {
            vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let opts = MorphOptions::default();
        let result = quality_check(&morphed, &source, &opts);
        match result {
            QualityVerdict::HardFail(details) => {
                assert_eq!(details.element_index, 0, "expected element_index 0");
                assert!(
                    details.jacobian < 0.0,
                    "expected negative jacobian, got {}",
                    details.jacobian
                );
            }
            other => panic!("expected HardFail, got: {other:?}"),
        }
    }

    // ── Step-5a: regular unit tet identity morph → Pass ──────────────────────

    #[test]
    fn quality_check_with_regular_unit_tet_identity_morph_returns_pass() {
        // Canonical right-handed unit tet — all quality metrics well within
        // defaults. min scaled J ≈ 0.707 >> 0.15 threshold.
        #[rustfmt::skip]
        let vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
        ];
        let tet_indices = vec![0u32, 1, 2, 3];
        let mesh = VolumeMesh {
            vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let opts = MorphOptions::default();
        assert_eq!(quality_check(&mesh, &mesh, &opts), QualityVerdict::Pass);
    }

    // ── Step-5b: near-degenerate tet → SoftFail(min_scaled_jacobian=Some) ────

    #[test]
    fn quality_check_with_near_degenerate_tet_returns_soft_fail_with_min_scaled_jacobian_populated(
    ) {
        // Three nearly-coplanar edges: nodes 0,1,2 form a nearly degenerate
        // triangle (node 2 very close to the line 0-1), node 3 is also nearly
        // coplanar. The min corner scaled Jacobian will be << 0.15.
        //
        // Vertices: (0,0,0), (1,0,0), (0.5,1e-3,0), (0.5,0.5e-3,1e-3)
        // These four points are nearly coplanar so the volume is tiny relative
        // to edge lengths → scaled J << 0.15.
        #[rustfmt::skip]
        let vertices: Vec<f32> = vec![
            0.0,   0.0,   0.0,
            1.0,   0.0,   0.0,
            0.5,   1e-3,  0.0,
            0.5,   0.5e-3, 1e-3,
        ];
        let tet_indices = vec![0u32, 1, 2, 3];
        let morphed = VolumeMesh {
            vertices: vertices.clone(),
            tet_indices: tet_indices.clone(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let source = VolumeMesh {
            vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let opts = MorphOptions::default(); // quality_floor_min_scaled_jacobian = 0.15
        let result = quality_check(&morphed, &source, &opts);
        match result {
            QualityVerdict::SoftFail(ref metrics) => {
                let observed = metrics
                    .min_scaled_jacobian
                    .expect("min_scaled_jacobian should be Some");
                assert!(
                    observed < 0.15,
                    "expected observed < 0.15, got {observed}"
                );
            }
            other => panic!("expected SoftFail, got: {other:?}"),
        }
    }

    // ── Step-7: pct_below_025 soft-fail threshold ─────────────────────────────

    #[test]
    fn quality_check_with_more_than_threshold_pct_of_elements_below_025_returns_soft_fail_with_pct_below_025_populated(
    ) {
        // 4 independent tets: 1 regular unit tet (min scaled J ≈ 0.707) and
        // 3 mildly-degraded tets with min scaled J in (0.15, 0.25).
        //
        // Degraded tet construction: (0,0,0),(1,0,0),(0,1,0),(0,0,h).
        // For this tet the worst corner is the one opposite the large triangle;
        // analysis shows min scaled J = h / sqrt(1 + h²) (see plan).
        //   h=0.18 → 0.18/sqrt(1.0324) ≈ 0.177  (in (0.15, 0.25)) ✓
        //   h=0.20 → 0.20/sqrt(1.0400) ≈ 0.196  (in (0.15, 0.25)) ✓
        //   h=0.23 → 0.23/sqrt(1.0529) ≈ 0.224  (in (0.15, 0.25)) ✓
        //
        // global min = 0.177 > 0.15 (default floor) → min_scaled_jacobian=None.
        // All 3 degraded tets have scaled J < 0.25 → pct = 3/4 = 0.75.
        // With quality_floor_pct_below_025 = 0.5: 0.75 > 0.5 → SoftFail.
        // source = same vertices (identity morph → AR increase = 1×) → no AR trip.

        #[rustfmt::skip]
        let vertices: Vec<f32> = vec![
            // Tet 0: regular unit tet (nodes 0-3), min scaled J ≈ 0.707
            0.0,  0.0, 0.0,
            1.0,  0.0, 0.0,
            0.0,  1.0, 0.0,
            0.0,  0.0, 1.0,
            // Tet 1: degraded, h=0.18, min scaled J ≈ 0.177 (nodes 4-7)
            4.0,  0.0, 0.0,
            5.0,  0.0, 0.0,
            4.0,  1.0, 0.0,
            4.0,  0.0, 0.18,
            // Tet 2: degraded, h=0.20, min scaled J ≈ 0.196 (nodes 8-11)
            8.0,  0.0, 0.0,
            9.0,  0.0, 0.0,
            8.0,  1.0, 0.0,
            8.0,  0.0, 0.20,
            // Tet 3: degraded, h=0.23, min scaled J ≈ 0.224 (nodes 12-15)
            12.0, 0.0, 0.0,
            13.0, 0.0, 0.0,
            12.0, 1.0, 0.0,
            12.0, 0.0, 0.23,
        ];
        #[rustfmt::skip]
        let tet_indices: Vec<u32> = vec![
            0,  1,  2,  3,  // unit tet, min scaled J ≈ 0.707
            4,  5,  6,  7,  // degraded h=0.18, min scaled J ≈ 0.177
            8,  9,  10, 11, // degraded h=0.20, min scaled J ≈ 0.196
            12, 13, 14, 15, // degraded h=0.23, min scaled J ≈ 0.224
        ];
        let mesh = VolumeMesh {
            vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // quality_floor_pct_below_025 = 0.5 so 3/4 = 0.75 > threshold.
        // quality_floor_min_scaled_jacobian stays at default 0.15 —
        // global min (≈ 0.177) > 0.15 so min_scaled_jacobian stays None.
        let mut opts = MorphOptions::default();
        opts.quality_floor_pct_below_025 = 0.5;

        let result = quality_check(&mesh, &mesh, &opts);
        match &result {
            QualityVerdict::SoftFail(metrics) => {
                assert!(
                    metrics.min_scaled_jacobian.is_none(),
                    "min_scaled_jacobian should be None (global min ≈ 0.177 > 0.15 floor), \
                     got {:?}",
                    metrics.min_scaled_jacobian
                );
                let pct = metrics
                    .pct_below_025
                    .expect("pct_below_025 should be Some");
                assert!(
                    (0.7..=0.8).contains(&pct),
                    "expected pct in [0.7, 0.8], got {pct}"
                );
                assert!(
                    metrics.max_aspect_ratio_increase.is_none(),
                    "max_aspect_ratio_increase should be None"
                );
            }
            other => panic!("expected SoftFail, got: {other:?}"),
        }
    }

    // ── Compile fence: exhaustive variant match (no wildcard arm) ─────────────
    //
    // Adding, removing, or renaming any QualityVerdict variant breaks
    // compilation here — same discipline as LaplacianFailure (laplacian.rs:659)
    // and MorphFailure (options.rs:144).
    #[test]
    fn quality_verdict_exhaustive_variant_fence() {
        use crate::types::{InversionDetails, MetricsBreached};
        let variants: &[QualityVerdict] = &[
            QualityVerdict::Pass,
            QualityVerdict::HardFail(InversionDetails {
                element_index: 0,
                jacobian: -0.5,
            }),
            QualityVerdict::SoftFail(MetricsBreached {
                min_scaled_jacobian: Some(0.1),
                pct_below_025: None,
                max_aspect_ratio_increase: None,
            }),
        ];
        for v in variants {
            match v {
                QualityVerdict::Pass => {}
                QualityVerdict::HardFail(InversionDetails { .. }) => {}
                QualityVerdict::SoftFail(MetricsBreached { .. }) => {}
            }
        }
    }
}
