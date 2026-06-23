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
//!   lengths differ, the aspect-ratio-factor comparison is skipped
//!   (threshold 3 is effectively disabled); the hard-fail and min-scaled-J /
//!   pct-below-025 checks still run on the morphed mesh.
//! - **Valid vertex indices.** Elements referencing out-of-range vertex indices
//!   are silently skipped (same defensive discipline as `laplacian.rs`).

use crate::options::MorphOptions;
use crate::types::{InversionDetails, SoftFailDetails};
use reify_ir::VolumeMesh;
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

// ── Aspect-ratio helpers ─────────────────────────────────────────────────────

/// Pairs of corner indices for the 6 tetrahedral edges.
const EDGE_PAIRS: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

/// Face vertex triplets for each of the 4 tet faces (vertices NOT including the
/// opposite corner k). Indices into the 4-node array.
///
/// Face opposite corner k = the 3 nodes other than k.
/// Corner 0 → nodes 1,2,3; corner 1 → nodes 0,2,3; …
const FACE_VERTICES: [[usize; 3]; 4] = [[1, 2, 3], [0, 2, 3], [0, 1, 3], [0, 1, 2]];

/// Compute the aspect ratio of a tetrahedron: `max_edge / min_height`.
///
/// Heights are computed via `h_k = 3V / face_area_k` where `V = |det| / 6`
/// and `face_area_k = ½ ||(b-a) × (c-a)||`.
///
/// Returns `f64::INFINITY` if `min_height == 0` (degenerate).
fn element_aspect_ratio(nodes: &[[f64; 3]; 4]) -> f64 {
    // Maximum edge length.
    let max_edge = EDGE_PAIRS
        .iter()
        .map(|&(i, j)| norm(sub(nodes[j], nodes[i])))
        .fold(0.0_f64, f64::max);

    // Volume from corner-0 determinant.
    let e1 = sub(nodes[1], nodes[0]);
    let e2 = sub(nodes[2], nodes[0]);
    let e3 = sub(nodes[3], nodes[0]);
    let vol = dot(e1, cross(e2, e3)).abs() / 6.0;

    // Minimum height across the 4 faces.
    let min_height = FACE_VERTICES
        .iter()
        .map(|&[a, b, c]| {
            let ab = sub(nodes[b], nodes[a]);
            let ac = sub(nodes[c], nodes[a]);
            let face_area = norm(cross(ab, ac)) / 2.0;
            if face_area > 0.0 {
                3.0 * vol / face_area
            } else {
                f64::INFINITY
            }
        })
        .fold(f64::INFINITY, f64::min);

    if min_height <= 0.0 || min_height.is_infinite() {
        f64::INFINITY
    } else {
        max_edge / min_height
    }
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
    SoftFail(SoftFailDetails),
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
/// aspect-ratio-factor comparison is skipped (`max_aspect_ratio_factor`
/// stays `None`). The hard-fail, min-scaled-J / pct-below-025, and
/// `degenerate_morphed_element` checks still run on the morphed mesh.
///
/// ## Source argument
///
/// `source` is a reference mesh used as the denominator of the per-element
/// aspect-ratio factor (`morphed_AR / source_AR`) and as the lockstep
/// iterator pivot for element-pair alignment. It does not need to be the
/// literal pre-morph mesh — any reference of matching connectivity (e.g., a
/// from-scratch remesh of the target geometry produced by the same procedural
/// generator) is admissible. The calibration rig in
/// `tests/calibration/sweep.rs` exploits this by passing a from-scratch
/// target as the `source` argument to compute the true
/// `morphed_AR / from_scratch_AR` ratio.
// G-allow: mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh); re-homed from cancelled #3429/#2947
pub fn quality_check(
    morphed: &VolumeMesh,
    source: &VolumeMesh,
    options: &MorphOptions,
) -> QualityVerdict {
    let morphed_vertex_count = morphed.vertices.len() / 3;
    let source_vertex_count = source.vertices.len() / 3;
    let matched_connectivity = morphed.tet_indices.len() == source.tet_indices.len();

    // Single pass over morphed elements: track inversions and soft-fail metrics.
    let mut hard_fail: Option<InversionDetails> = None;
    let mut global_min_scaled_j = f64::INFINITY;
    let mut total_evaluated: usize = 0;
    let mut count_below_025: usize = 0;
    let mut max_ar_ratio: f64 = 0.0;
    let mut first_degenerate_morphed: Option<usize> = None;

    // Build a lazy source iterator when connectivity matches; None otherwise.
    // Using an iterator (rather than collecting a Vec) avoids an O(n) heap
    // allocation proportional to mesh size and keeps source elements in
    // lockstep with the morphed loop without upfront allocation.
    let mut source_iter = if matched_connectivity {
        Some(source.tet_indices.chunks_exact(4))
    } else {
        None
    };

    for (elem_idx, chunk) in morphed.tet_indices.chunks_exact(4).enumerate() {
        // Advance source iterator in lockstep — even when the morphed element
        // is skipped below, so that source[k] always aligns with morphed[k].
        let src_chunk_opt = source_iter.as_mut().and_then(|it| it.next());

        // Read 4 corner positions, widening f32 → f64 at the read boundary.
        let idx = [
            chunk[0] as usize,
            chunk[1] as usize,
            chunk[2] as usize,
            chunk[3] as usize,
        ];
        // Skip elements with out-of-range indices (defensive; same discipline
        // as laplacian.rs:141-149). Source iterator was already advanced above,
        // preserving lockstep alignment even on skip.
        if idx.iter().any(|&i| i >= morphed_vertex_count) {
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
        total_evaluated += 1;
        if sj < global_min_scaled_j {
            global_min_scaled_j = sj;
        }
        // 0.25 is the fixed metric split point per PRD spec; only the trip
        // fraction is configurable via quality_floor_pct_below_025.
        if sj < 0.25 {
            count_below_025 += 1;
        }

        if sj < 0.0 {
            // First inverted element wins for HardFail. The `break` below
            // ensures the loop cannot reach a second inversion, so the
            // `is_none()` guard was dead-defensive. The debug_assert! makes
            // the invariant explicit without runtime cost in release builds.
            debug_assert!(hard_fail.is_none());
            hard_fail = Some(InversionDetails {
                element_index: elem_idx,
                jacobian: sj,
            });
            // Exit the loop — soft-fail bookkeeping after this point would
            // be discarded by the early-return at the HardFail check below.
            break;
        }

        // sj is now in [0, +inf). Exactly 0.0 means zero-volume (coplanar) or
        // coincident-edge (collapsed) tet — both are degenerate. First-wins.
        if sj == 0.0 && first_degenerate_morphed.is_none() {
            first_degenerate_morphed = Some(elem_idx);
        }

        // Aspect-ratio increase comparison (only when connectivity matches).
        // src_chunk_opt is None when matched_connectivity is false, so this
        // naturally skips when topologies differ without an extra branch.
        if let Some(src_chunk) = src_chunk_opt {
            let src_idx = [
                src_chunk[0] as usize,
                src_chunk[1] as usize,
                src_chunk[2] as usize,
                src_chunk[3] as usize,
            ];
            // When a source element has out-of-range indices, skip the AR
            // comparison for this element only; morphed-only metrics (scaled J,
            // pct_below_025) were already accumulated above.
            if src_idx.iter().any(|&i| i >= source_vertex_count) {
                continue;
            }
            let sp: [[f64; 3]; 4] = std::array::from_fn(|k| {
                let base = src_idx[k] * 3;
                [
                    source.vertices[base] as f64,
                    source.vertices[base + 1] as f64,
                    source.vertices[base + 2] as f64,
                ]
            });
            let morphed_ar = element_aspect_ratio(&p);
            let source_ar = element_aspect_ratio(&sp);
            // Skip comparison when either AR is degenerate.
            // source degenerate: zero-volume source tet → undefined ratio baseline.
            // morphed degenerate: AR=INFINITY (zero-volume coplanar/collapsed tet).
            //   Surfacing +inf in the public SoftFailDetails.max_aspect_ratio_factor
            //   field is awkward for serialization (JSON/MessagePack lack standard
            //   +inf encoding). The degenerate morphed tet itself is surfaced via
            //   `SoftFailDetails.degenerate_morphed_element` (populated unconditionally
            //   when sj == 0.0), making the AR signal redundant *for failure detection*
            //   regardless of caller-configured floors.
            // is_finite() already excludes NaN, so the redundant !is_nan() check
            // is dropped. Order: is_finite() first short-circuits the > 0.0 compare
            // on the rare NaN/Inf input.
            if source_ar.is_finite() && source_ar > 0.0 && morphed_ar.is_finite() {
                let ratio = morphed_ar / source_ar;
                if ratio > max_ar_ratio {
                    max_ar_ratio = ratio;
                }
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

    let pct = if total_evaluated > 0 {
        count_below_025 as f64 / total_evaluated as f64
    } else {
        0.0
    };
    let pct_below_025 = if pct > options.quality_floor_pct_below_025 {
        Some(pct)
    } else {
        None
    };

    // Aspect-ratio factor threshold (threshold 3).
    let max_aspect_ratio_factor =
        if matched_connectivity && max_ar_ratio > options.quality_aspect_ratio_factor_max {
            Some(max_ar_ratio)
        } else {
            None
        };

    let metrics = SoftFailDetails {
        min_scaled_jacobian,
        pct_below_025,
        max_aspect_ratio_factor,
        degenerate_morphed_element: first_degenerate_morphed,
    };

    if metrics.min_scaled_jacobian.is_some()
        || metrics.pct_below_025.is_some()
        || metrics.max_aspect_ratio_factor.is_some()
        || metrics.degenerate_morphed_element.is_some()
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
    use reify_ir::{ElementOrderTag, VolumeMesh};

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
    fn quality_check_with_single_inverted_tet_returns_hard_fail_with_element_index_and_negative_jacobian()
     {
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
    fn quality_check_with_near_degenerate_tet_returns_soft_fail_with_min_scaled_jacobian_populated()
    {
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
        // Set pct threshold above 1.0 so it can never trip, isolating the
        // min-scaled-J check. Mirrors the isolation pattern used in the
        // AR-increase test (step-9) where other thresholds are disabled.
        // quality_floor_min_scaled_jacobian = 0.15 from default.
        let opts = MorphOptions {
            quality_floor_pct_below_025: 1.01,
            ..MorphOptions::default()
        };
        let result = quality_check(&morphed, &source, &opts);
        match result {
            QualityVerdict::SoftFail(ref metrics) => {
                let observed = metrics
                    .min_scaled_jacobian
                    .expect("min_scaled_jacobian should be Some");
                assert!(observed < 0.15, "expected observed < 0.15, got {observed}");
            }
            other => panic!("expected SoftFail, got: {other:?}"),
        }
    }

    // ── Step-7: pct_below_025 soft-fail threshold ─────────────────────────────

    #[test]
    fn quality_check_with_more_than_threshold_pct_of_elements_below_025_returns_soft_fail_with_pct_below_025_populated()
     {
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
        let opts = MorphOptions {
            quality_floor_pct_below_025: 0.5,
            ..MorphOptions::default()
        };

        let result = quality_check(&mesh, &mesh, &opts);
        match &result {
            QualityVerdict::SoftFail(metrics) => {
                assert!(
                    metrics.min_scaled_jacobian.is_none(),
                    "min_scaled_jacobian should be None (global min ≈ 0.177 > 0.15 floor), \
                     got {:?}",
                    metrics.min_scaled_jacobian
                );
                let pct = metrics.pct_below_025.expect("pct_below_025 should be Some");
                assert!(
                    (0.7..=0.8).contains(&pct),
                    "expected pct in [0.7, 0.8], got {pct}"
                );
                assert!(
                    metrics.max_aspect_ratio_factor.is_none(),
                    "max_aspect_ratio_factor should be None"
                );
            }
            other => panic!("expected SoftFail, got: {other:?}"),
        }
    }

    // ── Step-9: max_aspect_ratio_factor soft-fail threshold ──────────────────

    #[test]
    fn quality_check_with_morphed_aspect_ratio_more_than_threshold_x_source_returns_soft_fail_with_max_aspect_ratio_factor_populated()
     {
        // source: regular unit tet (0,0,0),(1,0,0),(0,1,0),(0,0,1)
        //   AR_source ≈ max_edge / min_height
        //   max_edge = sqrt(2) ≈ 1.414, min_height = 1/sqrt(3) * 3 ≈ 1/sqrt(3)
        //   (rough, not needed — just need morphed AR >> 2× source AR)
        //
        // morphed: same connectivity, but node 3 moved from (0,0,1) to (0,0,5).
        //   max_edge ≈ sqrt(1+25) = sqrt(26) ≈ 5.1
        //   Volume = |det((1,0,0),(0,1,0),(0,0,5))| / 6 = 5/6
        //   Heights: computed below, but morphed AR >> 2× source AR.
        //
        // min scaled J check: morphed tet has det = 5 > 0, and a stretched tet
        // stays well above 0.25 in scaled J → min_scaled_jacobian=None,
        // pct_below_025=None. Only AR factor trips.

        // source
        #[rustfmt::skip]
        let src_vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
        ];
        let tet_indices = vec![0u32, 1, 2, 3];
        let source = VolumeMesh {
            vertices: src_vertices,
            tet_indices: tet_indices.clone(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // morphed: node 3 at (0,0,5) to greatly increase aspect ratio
        #[rustfmt::skip]
        let morphed_vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 5.0,
        ];
        let morphed = VolumeMesh {
            vertices: morphed_vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // Disable min_scaled_jacobian and pct_below_025 checks so this test
        // isolates the aspect-ratio-factor threshold.
        //
        // The morphed tet (node 3 at z=5) has scaled J ≈ 0.054 at corner 3
        // (far from the base plane), which would trip the 0.15 floor if left
        // at default. Setting floor to 0.0 means only a strictly-negative
        // observed J fires (impossible for a non-inverted tet that reaches
        // the SoftFail branch). pct threshold > 1.0 is also unreachable.
        // quality_aspect_ratio_factor_max stays at 2.0 (default).
        let opts = MorphOptions {
            quality_floor_min_scaled_jacobian: 0.0,
            quality_floor_pct_below_025: 1.01,
            ..MorphOptions::default()
        };

        let result = quality_check(&morphed, &source, &opts);
        match &result {
            QualityVerdict::SoftFail(metrics) => {
                assert!(
                    metrics.min_scaled_jacobian.is_none(),
                    "min_scaled_jacobian should be None, got {:?}",
                    metrics.min_scaled_jacobian
                );
                assert!(
                    metrics.pct_below_025.is_none(),
                    "pct_below_025 should be None, got {:?}",
                    metrics.pct_below_025
                );
                let ar_factor = metrics
                    .max_aspect_ratio_factor
                    .expect("max_aspect_ratio_factor should be Some");
                assert!(
                    ar_factor > 2.0,
                    "expected max_aspect_ratio_factor > 2.0, got {ar_factor}"
                );
            }
            other => panic!("expected SoftFail, got: {other:?}"),
        }
    }

    // ── Connectivity-mismatch contract ────────────────────────────────────────

    /// When `morphed.tet_indices.len() != source.tet_indices.len()`, the
    /// aspect-ratio-factor comparison must be skipped (`max_aspect_ratio_factor`
    /// stays `None`). The hard-fail and scaled-J checks must still run on the
    /// morphed mesh as documented in the module-level preconditions section.
    ///
    /// This test pins that contract so a future refactor that accidentally
    /// panics on the index mismatch or omits the AR check condition is caught.
    #[test]
    fn quality_check_with_mismatched_connectivity_skips_ar_factor_but_runs_morphed_checks() {
        // morphed: 1 wildly-stretched tet (AR >> 2× any reasonable source AR)
        //   node 3 at (0,0,5) — scaled J ≈ 0.054 at corner 3 < 0.15 → soft trips.
        // source:  2 regular tets (different element count → connectivity mismatch)
        // Expected:
        //   - max_aspect_ratio_factor = None  (AR comparison skipped)
        //   - min_scaled_jacobian = Some(...)   (proves morphed-only checks ran)
        //   - HardFail not returned             (morphed tet is right-handed)
        #[rustfmt::skip]
        let morphed_vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 5.0, // stretched — high AR, low scaled J
        ];
        let morphed = VolumeMesh {
            vertices: morphed_vertices,
            tet_indices: vec![0u32, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // source: 2 tets → tet_indices.len() = 8 ≠ 4 (morphed) → mismatch
        #[rustfmt::skip]
        let source_vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
            0.0, 0.0, 2.0,
        ];
        let source = VolumeMesh {
            vertices: source_vertices,
            tet_indices: vec![0u32, 1, 2, 3, 0, 1, 2, 4],
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // Explicit threshold: quality_floor_min_scaled_jacobian = 0.15 (was
        // the pre-task-#2950 PRD seed). The stretched tet has scaled J ≈ 0.054,
        // which trips this floor regardless of the calibrated Default (which
        // task #2950 lowered to accommodate procedurally-meshed fixtures). The
        // test pins the *mechanism* — morphed-only checks still run when
        // connectivity is mismatched — not the absolute floor value.
        let opts = MorphOptions {
            quality_floor_min_scaled_jacobian: 0.15,
            ..MorphOptions::default()
        };
        let result = quality_check(&morphed, &source, &opts);

        match &result {
            QualityVerdict::SoftFail(metrics) => {
                assert!(
                    metrics.max_aspect_ratio_factor.is_none(),
                    "max_aspect_ratio_factor must be None on connectivity mismatch, \
                     got {:?}",
                    metrics.max_aspect_ratio_factor
                );
                // Verify the morphed-only checks still ran: scaled J < 0.15 so
                // min_scaled_jacobian should be Some (not None).
                assert!(
                    metrics.min_scaled_jacobian.is_some(),
                    "min_scaled_jacobian should be Some — proves morphed-only checks \
                     ran despite connectivity mismatch; got None"
                );
            }
            QualityVerdict::Pass => {
                panic!("expected SoftFail (stretched tet should trip min-scaled-J)");
            }
            QualityVerdict::HardFail(details) => {
                panic!(
                    "expected SoftFail, got HardFail({details:?}) — \
                     stretched tet is right-handed and must not invert"
                );
            }
        }
    }

    // ── Step-1 (task 3172) / Step-1 (task 3196): degenerate morphed tet ─────
    //
    // task 3172 fix: AR comparison skipped when morphed_ar.is_infinite().
    // task 3196 fix: degenerate morphed tet surfaces in
    //   SoftFailDetails.degenerate_morphed_element regardless of caller floors.

    /// Regression guard for task 3196 — a degenerate morphed tet must surface in
    /// `SoftFailDetails.degenerate_morphed_element` regardless of caller-configured
    /// floors, not silently pass.
    ///
    /// Fixture: coplanar morphed tet (all z=0); source = regular unit tet.
    /// `quality_floor_min_scaled_jacobian = 0.0` (floor disabled — proves the
    /// detection is floor-independent), `quality_floor_pct_below_025 = 1.01`
    /// (pct disabled).
    ///
    /// Why scaled J = 0 (not inverted → no HardFail):
    /// All 4 nodes lie in the z=0 plane, so every corner's det(ea,eb,ec) = 0.
    /// The degenerate fallback is 0.0, which is not < 0 → no HardFail.
    ///
    /// Why AR = INFINITY (and must be skipped, per task 3172):
    /// vol = 0 → all face heights = 0 → min_height = 0 →
    /// `element_aspect_ratio` returns `f64::INFINITY`. Surfacing +inf in
    /// `max_aspect_ratio_factor` is awkward for serialization; the
    /// `degenerate_morphed_element` field makes the AR signal redundant for
    /// failure detection.
    #[test]
    fn quality_check_degenerate_morphed_element_populated_when_floors_disabled() {
        // Morphed: coplanar tet (all z=0) — AR = INFINITY, scaled J = 0 (not inverted).
        #[rustfmt::skip]
        let morphed_vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,  // node 0
            1.0, 0.0, 0.0,  // node 1
            0.0, 1.0, 0.0,  // node 2
            0.5, 0.5, 0.0,  // node 3 — coplanar with 0,1,2 (z=0 plane)
        ];
        let tet_indices = vec![0u32, 1, 2, 3];
        let morphed = VolumeMesh {
            vertices: morphed_vertices,
            tet_indices: tet_indices.clone(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // Source: regular unit tet — finite positive AR.
        #[rustfmt::skip]
        let source_vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
        ];
        let source = VolumeMesh {
            vertices: source_vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // Disable min-J and pct floors to isolate degenerate_morphed_element detection.
        // Scaled J = 0 → floor 0.0: 0 < 0 is false → min_scaled_jacobian = None.
        // pct = 1.0 (1/1 below 0.25) → threshold 1.01: 1.0 > 1.01 is false → pct_below_025 = None.
        // AR = INFINITY → skipped (task-3172 fix) → max_aspect_ratio_factor = None.
        // degenerate_morphed_element = Some(0) because sj == 0.0 at element 0.
        let opts = MorphOptions {
            quality_floor_min_scaled_jacobian: 0.0,
            quality_floor_pct_below_025: 1.01,
            ..MorphOptions::default()
        };
        // quality_aspect_ratio_factor_max stays at 2.0 (default).

        let result = quality_check(&morphed, &source, &opts);
        match &result {
            QualityVerdict::SoftFail(metrics) => {
                assert_eq!(
                    metrics.degenerate_morphed_element,
                    Some(0),
                    "degenerate coplanar tet must surface as degenerate_morphed_element=Some(0)"
                );
                assert_eq!(
                    metrics.max_aspect_ratio_factor, None,
                    "AR comparison must be skipped for degenerate morphed tet (task-3172 contract)"
                );
                assert_eq!(
                    metrics.min_scaled_jacobian, None,
                    "min_scaled_jacobian must be None — proves floor=0.0 disabling is the regime"
                );
            }
            QualityVerdict::Pass => {
                panic!(
                    "expected SoftFail with degenerate_morphed_element=Some(0), \
                     got Pass — degenerate morphed tet must not silently pass (task 3196)"
                );
            }
            QualityVerdict::HardFail(d) => {
                panic!(
                    "expected SoftFail, got HardFail({d:?}) — \
                     coplanar tet has sj=0 (not < 0) and must not invert"
                );
            }
        }
    }

    // ── Task 3196: degenerate_morphed_element contract guards ────────────────

    /// Regression guard for task 3196 — first-degenerate-wins contract.
    ///
    /// A multi-tet morphed mesh where elements **0** and **2** are coplanar
    /// (sj=0) and element **1** is a valid regular tet. `degenerate_morphed_element`
    /// must be `Some(0)` — the *first* degenerate element in iteration order —
    /// not `Some(2)`.
    ///
    /// Floors are disabled (`quality_floor_min_scaled_jacobian = 0.0`,
    /// `quality_floor_pct_below_025 = 1.01`) to isolate the detection from
    /// the threshold-driven paths.
    #[test]
    fn quality_check_degenerate_morphed_element_first_wins() {
        // Element 0 (nodes 0-3):  coplanar, z=0 — degenerate.
        // Element 1 (nodes 4-7):  regular unit tet offset to x=10 — not degenerate.
        // Element 2 (nodes 8-11): coplanar, z=0, offset to x=20 — degenerate.
        #[rustfmt::skip]
        let morphed_vertices: Vec<f32> = vec![
            // Tet 0: coplanar z=0
            0.0,  0.0, 0.0,
            1.0,  0.0, 0.0,
            0.0,  1.0, 0.0,
            0.5,  0.5, 0.0,
            // Tet 1: regular unit tet at x=10
            10.0, 0.0, 0.0,
            11.0, 0.0, 0.0,
            10.0, 1.0, 0.0,
            10.0, 0.0, 1.0,
            // Tet 2: coplanar z=0 at x=20
            20.0, 0.0, 0.0,
            21.0, 0.0, 0.0,
            20.0, 1.0, 0.0,
            20.5, 0.5, 0.0,
        ];
        #[rustfmt::skip]
        let tet_indices: Vec<u32> = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let morphed = VolumeMesh {
            vertices: morphed_vertices,
            tet_indices: tet_indices.clone(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // Source: three regular tets (no degenerate elements).
        #[rustfmt::skip]
        let source_vertices: Vec<f32> = vec![
            0.0,  0.0, 0.0,  1.0,  0.0, 0.0,  0.0,  1.0, 0.0,  0.0,  0.0, 1.0,
            10.0, 0.0, 0.0,  11.0, 0.0, 0.0,  10.0, 1.0, 0.0,  10.0, 0.0, 1.0,
            20.0, 0.0, 0.0,  21.0, 0.0, 0.0,  20.0, 1.0, 0.0,  20.0, 0.0, 1.0,
        ];
        let source = VolumeMesh {
            vertices: source_vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        let opts = MorphOptions {
            quality_floor_min_scaled_jacobian: 0.0,
            quality_floor_pct_below_025: 1.01,
            ..MorphOptions::default()
        };

        let result = quality_check(&morphed, &source, &opts);
        match &result {
            QualityVerdict::SoftFail(metrics) => {
                assert_eq!(
                    metrics.degenerate_morphed_element,
                    Some(0),
                    "first degenerate element must be Some(0); elements 0 and 2 are degenerate"
                );
            }
            other => panic!(
                "expected SoftFail with degenerate_morphed_element=Some(0), got {:?}",
                other
            ),
        }
    }

    /// Regression guard for task 3196 — coincident-vertex degenerate tet.
    ///
    /// Nodes 0 and 1 are at the same position: the edge from corner 0 toward
    /// node 1 has zero length → `norm(ea) == 0` → `product == 0` →
    /// `element_scaled_jacobian` returns `0.0` via the zero-edge-product
    /// fallback (`product > 0.0` else `0.0`). This exercises a different code
    /// path than the coplanar test: the determinant is not relevant because the
    /// zero product short-circuits first.
    #[test]
    fn quality_check_degenerate_morphed_element_coincident_vertex() {
        // Nodes 0 and 1 coincide at origin → zero-length edge at corners 0 and 1.
        #[rustfmt::skip]
        let morphed_vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,  // node 0
            0.0, 0.0, 0.0,  // node 1 — same position as node 0
            0.0, 1.0, 0.0,  // node 2
            0.0, 0.0, 1.0,  // node 3
        ];
        let tet_indices = vec![0u32, 1, 2, 3];
        let morphed = VolumeMesh {
            vertices: morphed_vertices,
            tet_indices: tet_indices.clone(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // Source: regular unit tet.
        #[rustfmt::skip]
        let source_vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
        ];
        let source = VolumeMesh {
            vertices: source_vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        let opts = MorphOptions {
            quality_floor_min_scaled_jacobian: 0.0,
            quality_floor_pct_below_025: 1.01,
            ..MorphOptions::default()
        };

        let result = quality_check(&morphed, &source, &opts);
        match &result {
            QualityVerdict::SoftFail(metrics) => {
                assert_eq!(
                    metrics.degenerate_morphed_element,
                    Some(0),
                    "coincident-vertex tet must surface as degenerate_morphed_element=Some(0)"
                );
            }
            other => panic!(
                "expected SoftFail with degenerate_morphed_element=Some(0), got {:?}",
                other
            ),
        }
    }

    /// Regression guard for task 3196 — HardFail strictly preempts degenerate.
    ///
    /// When element 0 is degenerate (sj=0, not inverted) and element 1 is
    /// inverted (sj<0), the loop detects the inversion at element 1 and
    /// returns `HardFail`. The `degenerate_morphed_element` detection for
    /// element 0 does not downgrade the verdict to `SoftFail`.
    ///
    /// This pins the `break`-on-inversion ordering invariant introduced in
    /// step-3 (task 3196): the inversion early-break fires before the
    /// degenerate field can influence the final verdict.
    #[test]
    fn quality_check_hard_fail_preempts_degenerate_morphed_element() {
        // Element 0 (nodes 0-3): coplanar tet — degenerate, sj=0.
        // Element 1 (nodes 4-7): inverted tet — sj<0 → HardFail.
        #[rustfmt::skip]
        let vertices: Vec<f32> = vec![
            // Tet 0: coplanar z=0
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.5, 0.5, 0.0,
            // Tet 1: inverted — nodes 2 and 3 swapped vs canonical right-hand tet
            10.0, 0.0, 0.0,
            11.0, 0.0, 0.0,
            10.0, 0.0, 1.0,  // swapped
            10.0, 1.0, 0.0,  // swapped
        ];
        #[rustfmt::skip]
        let tet_indices: Vec<u32> = vec![0, 1, 2, 3, 4, 5, 6, 7];
        let mesh = VolumeMesh {
            vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        let opts = MorphOptions {
            quality_floor_min_scaled_jacobian: 0.0,
            quality_floor_pct_below_025: 1.01,
            ..MorphOptions::default()
        };

        let result = quality_check(&mesh, &mesh, &opts);
        match result {
            QualityVerdict::HardFail(details) => {
                assert_eq!(
                    details.element_index, 1,
                    "inverted element is at index 1; got {}",
                    details.element_index
                );
                assert!(
                    details.jacobian < 0.0,
                    "inverted tet jacobian must be negative, got {}",
                    details.jacobian
                );
            }
            QualityVerdict::SoftFail(_) => {
                panic!(
                    "expected HardFail, got SoftFail — HardFail must preempt degenerate_morphed_element"
                );
            }
            QualityVerdict::Pass => {
                panic!("expected HardFail, got Pass");
            }
        }
    }

    // ── Step-11: HardFail preempts SoftFail (regression guard) ──────────────

    /// Regression guard: when any element is inverted, `HardFail` is returned
    /// even if soft-fail thresholds also trip on other elements.
    ///
    /// This test pins the ordering contract from the design decision:
    /// "HardFail strictly preempts SoftFail."
    #[test]
    fn quality_check_returns_hard_fail_even_when_soft_thresholds_also_trip() {
        // Tet 0: inverted (left-handed), element_index=0.
        //   Vertices: (0,0,0),(1,0,0),(0,0,1),(0,1,0) — nodes 2 and 3 swapped
        //   from canonical right-handed tet → corner-0 det = -1 < 0.
        //   Scaled J < 0 → HardFail.
        //
        // Tet 1: mildly degraded but right-handed, min scaled J in (0.15, 0.25).
        //   Uses the h=0.18 degraded tet from the pct_below_025 test.
        //   min scaled J ≈ 0.177 — above 0.15 floor, below 0.25 split point.
        //
        // source: same connectivity, same vertices (identity morph so AR ratio = 1).
        //
        // Expected: HardFail(InversionDetails { element_index: 0, .. })
        // because tet 0 is inverted. SoftFail from tet 1's scaled J is preempted.
        #[rustfmt::skip]
        let vertices: Vec<f32> = vec![
            // Tet 0: inverted (nodes 0-3)
            0.0, 0.0, 0.0,  // node 0
            1.0, 0.0, 0.0,  // node 1
            0.0, 0.0, 1.0,  // node 2  (swapped — inverted tet)
            0.0, 1.0, 0.0,  // node 3  (swapped — inverted tet)
            // Tet 1: degraded h=0.18, min scaled J ≈ 0.177 (nodes 4-7)
            4.0, 0.0, 0.0,
            5.0, 0.0, 0.0,
            4.0, 1.0, 0.0,
            4.0, 0.0, 0.18,
        ];
        #[rustfmt::skip]
        let tet_indices: Vec<u32> = vec![
            0, 1, 2, 3,  // inverted
            4, 5, 6, 7,  // degraded
        ];
        let mesh = VolumeMesh {
            vertices,
            tet_indices,
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        let opts = MorphOptions::default();
        let result = quality_check(&mesh, &mesh, &opts);

        match result {
            QualityVerdict::HardFail(details) => {
                assert_eq!(
                    details.element_index, 0,
                    "expected element_index 0 (the inverted tet), got {}",
                    details.element_index
                );
                assert!(
                    details.jacobian < 0.0,
                    "inverted tet jacobian must be negative, got {}",
                    details.jacobian
                );
            }
            QualityVerdict::SoftFail(_) => {
                panic!("expected HardFail, got SoftFail — HardFail must preempt SoftFail");
            }
            QualityVerdict::Pass => {
                panic!("expected HardFail, got Pass");
            }
        }
    }
}
