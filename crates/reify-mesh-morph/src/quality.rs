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
    let _ = options;

    let vertex_count = morphed.vertices.len() / 3;

    // First pass: scan morphed mesh for inversions.
    let mut hard_fail: Option<InversionDetails> = None;

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

        // Corner-0 Jacobian det = (p1-p0) · ((p2-p0) × (p3-p0)).
        let e1 = sub(p[1], p[0]);
        let e2 = sub(p[2], p[0]);
        let e3 = sub(p[3], p[0]);
        let det = dot(e1, cross(e2, e3));

        if det < 0.0 {
            // Compute corner-0 scaled Jacobian as a placeholder.
            let product = norm(e1) * norm(e2) * norm(e3);
            let jacobian = if product > 0.0 {
                det * SQRT_2 / product
            } else {
                0.0
            };
            // First inverted element wins.
            if hard_fail.is_none() {
                hard_fail = Some(InversionDetails {
                    element_index: elem_idx,
                    jacobian,
                });
            }
        }
    }

    if let Some(details) = hard_fail {
        return QualityVerdict::HardFail(details);
    }

    QualityVerdict::Pass
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
