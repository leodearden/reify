//! Choosing the direction to walk from a candidate medial voxel.
//!
//! # The problem
//!
//! The medial test walks `±g` from a voxel and compares the two distances to
//! the surface, where `g` is the normalised SDF gradient. But a solid is
//! symmetric about its own medial surface, so wherever that surface passes
//! exactly through a sample plane the central difference across it cancels
//! IDENTICALLY — `(φ[k+1] − φ[k−1]) / 2h = 0` — and the gradient carries no
//! direction at all. The voxel that is most certainly medial is the one the
//! gradient describes worst.
//!
//! Skipping those voxels does not merely lose them: the neighbours one voxel
//! off the medial surface see `|d⁺ − d⁻| = 2h`, which exceeds the equality
//! threshold `distance_tolerance·dmax + h` for any thin wall, so they are
//! rejected too and the entire mask comes back EMPTY. Whether that happens is
//! decided by where the body sits relative to the grid — a half-voxel shift
//! measures correctly (task #7527).
//!
//! # The fallback
//!
//! Where the central difference cancels, the distance field still has a KINK:
//! its one-sided differences along the through-thickness axis have OPPOSITE
//! signs (`forward > 0`, `backward < 0`), a strict valley. That signature
//! survives the cancellation, and the axis carrying it is the axis to walk.
//!
//! # Invariant that bounds the blast radius
//!
//! [`medial_walk_direction`] returns exactly what `normalize3(gradient)`
//! returned whenever that was `Some`. The fallback fires ONLY where the
//! existing code already gave up, so no voxel that is medial today can stop
//! being medial: the mask can only GROW, never shrink, and no consumer of it
//! can lose output.
//!
//! # Why the fallback is interior-only
//!
//! An exterior ridge — the mid-plane of the GAP between two plates, say — is
//! equally grid-aligned and equally gradient-degenerate, but it is a medial
//! axis of the COMPLEMENT, not of the material. Tagging it would put a voxel
//! with small `|φ|` into the mask, and `min_feature_size_measure`'s
//! `2·min|φ|` reduction would report a feature size that does not exist.
//! Hence the `φ < 0` guard.
//!
//! # Residual limitation
//!
//! The fallback is reached only when the gradient is degenerate to within
//! `GRADIENT_EPSILON`. A producer whose medial-plane asymmetry noise exceeds
//! that would neither trip the fallback nor yield a usable walk direction.
//! The threshold is deliberately not widened: at the half-voxel alignment the
//! voxels adjacent to the mid-plane have `‖∇φ‖` of exactly half a unit and no
//! strict valley (their backward difference is zero, not negative), so a
//! looser degeneracy test would route them to a fallback that declines — and
//! DROP them, turning a currently-working alignment into a regression.

use reify_ir::value::SampledField;

use crate::medial::{normalize3, sample_at_index};

/// Direction to walk from voxel `idx`, given the already-computed raw
/// `gradient` there.
///
/// The gradient is a parameter rather than recomputed here so that
/// `compute_medial_mask` keeps using its precomputed gradient grid.
pub(crate) fn medial_walk_direction(
    sdf: &SampledField,
    idx: [usize; 3],
    gradient: [f64; 3],
) -> Option<[f64; 3]> {
    normalize3(gradient).or_else(|| interior_ridge_axis(sdf, idx))
}

/// Unit vector along the axis whose one-sided differences form the sharpest
/// strict valley at an interior voxel, or `None` if there is no such axis.
///
/// The returned SIGN is arbitrary (always `+axis`): every caller walks `±g`
/// and every downstream test is symmetric under swapping `d⁺` with `d⁻`.
fn interior_ridge_axis(sdf: &SampledField, idx: [usize; 3]) -> Option<[f64; 3]> {
    let phi = sample_at_index(sdf, idx);
    if !(phi < 0.0) {
        return None;
    }

    let mut best: Option<(f64, usize)> = None;
    for axis in 0..3 {
        if idx[axis] < 1 || idx[axis] + 1 >= sdf.axis_grids[axis].len() {
            continue;
        }
        let mut lower = idx;
        lower[axis] -= 1;
        let mut upper = idx;
        upper[axis] += 1;

        // Each one-sided difference is divided by its own axis spacing so the
        // sharpnesses stay comparable across an anisotropic grid.
        let spacing = sdf.spacing[axis];
        let forward = (sample_at_index(sdf, upper) - phi) / spacing;
        let backward = (phi - sample_at_index(sdf, lower)) / spacing;
        if !(forward > 0.0 && backward < 0.0) {
            continue;
        }

        // Strictly greater, so the lowest axis index wins a tie and the choice
        // stays deterministic (`compute_medial_mask` has a determinism
        // contract over the voxel ordering the mask is built in).
        let sharpness = forward - backward;
        if best.is_none_or(|(best_sharpness, _)| sharpness > best_sharpness) {
            best = Some((sharpness, axis));
        }
    }

    best.map(|(_, axis)| {
        let mut direction = [0.0; 3];
        direction[axis] = 1.0;
        direction
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::value::{InterpolationKind, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    /// `n³` Regular3D field whose value at `(i, j, k)` is `phi(i, j, k)`, with
    /// grid point `(i, j, k)` at world `(i·sx, j·sy, k·sz)`.
    fn index_field(
        n: usize,
        spacing: [f64; 3],
        phi: impl Fn(usize, usize, usize) -> f64,
    ) -> SampledField {
        let axis_grids: Vec<Vec<f64>> = (0..3)
            .map(|a| (0..n).map(|i| (i as f64) * spacing[a]).collect())
            .collect();
        let mut data = Vec::with_capacity(n * n * n);
        for i in 0..n {
            for j in 0..n {
                for k in 0..n {
                    data.push(phi(i, j, k));
                }
            }
        }
        SampledField {
            name: "walk-direction-fixture".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: (0..3).map(|a| (n as f64 - 1.0) * spacing[a]).collect(),
            spacing: spacing.to_vec(),
            axis_grids,
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Interior solid with a `k`-axis kink at `k = 1`: `φ = |k − 1| − 2`.
    fn interior_k_valley(n: usize, spacing: [f64; 3]) -> SampledField {
        index_field(n, spacing, |_i, _j, k| (k as f64 - 1.0).abs() - 2.0)
    }

    #[test]
    fn a_usable_gradient_is_returned_normalised_and_otherwise_unchanged() {
        let sdf = interior_k_valley(3, [1.0; 3]);
        assert_eq!(
            medial_walk_direction(&sdf, [1, 1, 1], [0.0, 3.0, 4.0]),
            Some([0.0, 0.6, 0.8])
        );
    }

    #[test]
    fn a_flat_region_has_no_walk_direction() {
        let sdf = index_field(3, [1.0; 3], |_i, _j, _k| -1.0);
        assert_eq!(medial_walk_direction(&sdf, [1, 1, 1], [0.0; 3]), None);
    }

    #[test]
    fn a_voxel_with_no_interior_neighbour_pair_on_the_kink_axis_has_no_walk_direction() {
        let sdf = interior_k_valley(3, [1.0; 3]);
        assert_eq!(medial_walk_direction(&sdf, [1, 1, 0], [0.0; 3]), None);
    }

    #[test]
    fn an_exterior_kink_is_not_a_medial_axis_of_the_material() {
        let sdf = index_field(3, [1.0; 3], |_i, _j, k| (k as f64 - 1.0).abs() + 1.0);
        assert_eq!(medial_walk_direction(&sdf, [1, 1, 1], [0.0; 3]), None);
    }

    #[test]
    fn an_interior_valley_yields_the_unit_vector_along_its_axis() {
        let sdf = interior_k_valley(3, [1.0; 3]);
        assert_eq!(
            medial_walk_direction(&sdf, [1, 1, 1], [0.0; 3]),
            Some([0.0, 0.0, 1.0])
        );
    }

    #[test]
    fn the_sharper_of_two_valleys_wins() {
        // Square-ish bar with a twice-as-sharp kink on the k axis.
        let sdf = index_field(3, [1.0; 3], |i, _j, k| {
            (i as f64 - 1.0).abs().max(2.0 * (k as f64 - 1.0).abs()) - 3.0
        });
        assert_eq!(
            medial_walk_direction(&sdf, [1, 1, 1], [0.0; 3]),
            Some([0.0, 0.0, 1.0])
        );
    }

    #[test]
    fn two_equally_sharp_valleys_resolve_to_the_lowest_axis_index() {
        let sdf = index_field(3, [1.0; 3], |i, _j, k| {
            (i as f64 - 1.0).abs().max((k as f64 - 1.0).abs()) - 3.0
        });
        assert_eq!(
            medial_walk_direction(&sdf, [1, 1, 1], [0.0; 3]),
            Some([1.0, 0.0, 0.0])
        );
    }

    /// Sharpness is a derivative, so equal per-step drops on a coarser axis are
    /// a SHALLOWER valley. Same field as the tie above, stretched along `i`.
    #[test]
    fn sharpness_is_measured_per_unit_length_not_per_voxel() {
        let sdf = index_field(3, [2.0, 1.0, 1.0], |i, _j, k| {
            (i as f64 - 1.0).abs().max((k as f64 - 1.0).abs()) - 3.0
        });
        assert_eq!(
            medial_walk_direction(&sdf, [1, 1, 1], [0.0; 3]),
            Some([0.0, 0.0, 1.0])
        );
    }
}
