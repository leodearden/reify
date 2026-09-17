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
//! Nor can a consumer GAIN a WRONG output, in EITHER direction. A
//! newly-admitted voxel is measured across the wall it actually crosses rather
//! than along the axis it happened to be walked down — see the next section —
//! so the honest `NoMeasurement` this replaces cannot turn into a confident
//! OVER-read. And the factor that buys that is itself floored at
//! [`MIN_RIDGE_NORMAL_COSINE`], so it cannot become a confident UNDER-read
//! either: a factor below any obliquity a unit normal can produce is proof the
//! voxel is not a medial kink at all, and the fallback declines rather than
//! scaling a reading by it. An under-read is no better than an over-read — it
//! reaches the same DFM min-wall verdict, as a spurious violation instead of a
//! missed one.
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
//! # Walking an axis, measuring a perpendicular
//!
//! The fallback walks a grid AXIS, but the medial plane it crosses need not be
//! perpendicular to that axis. On an oblique plane the walk is stretched by
//! `1/|n̂·axis|`, so `d⁺ + d⁻` reads `t / |n̂·axis|` — an over-read of up to
//! √3×, which would break `min_wall_thickness`'s conservative-lower-bound
//! contract and let a too-thin wall pass a DFM constraint. [`WalkDirection`]
//! therefore carries the conversion factor alongside the direction it belongs
//! to, so no call site can walk without converting.
//!
//! The factor costs no extra sampling. At a kink voxel the field along the
//! chosen axis is `φ(x₀ ± s) = |n_a·s| − t/2`, so the one-sided slopes are
//! exactly `±|n_a|` and their mean IS the obliquity factor — already computed
//! by the ridge search, as half its `sharpness`.
//!
//! The MASK is deliberately left in the walk-axis metric. Keeping its equality
//! test uncorrected is what preserves the monotone invariant above, and it
//! costs nothing: the `1/|n_a|` stretch of the admitted band in axis terms is
//! exactly cancelled by the `|n_a|` projection back to perpendicular, so the
//! perpendicular band the mask admits is unchanged. Hence
//! `min_feature_size_measure`, whose `2|φ|` reduction is already
//! perpendicular, reads identically with and without this.
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
//!
//! The obliquity factor is exact only at a voxel sitting ON the medial plane;
//! one `δ` off it reads `|n_a| − |δ|/h` instead, and the `min(1, ·)` clamp
//! holds a non-Lipschitz field to `1.0`. Both err LOW, which is the direction
//! that keeps `min_wall_thickness` a conservative lower bound — but only down
//! to [`MIN_RIDGE_NORMAL_COSINE`], past which the reading is declined outright
//! rather than trusted.
//!
//! That floor is what keeps the fallback safe against a producer whose narrow
//! band does NOT cover the whole interior. OpenVDB's `meshToLevelSet` saturates
//! `φ` beyond the band, and a saturated interior plateau exactly one voxel
//! thick presents a strict valley whose one-sided slopes are a fraction of a
//! unit — `0.5` for a 2-voxel band on a 5-voxel wall — with no obliquity
//! anywhere in sight. Read as a conversion factor it would halve the wall.
//! Every in-tree producer sizes the band to cover the interior
//! (`MeshToVoxelOptions::{honest_floor, for_resolution}`), so this is
//! belt-and-braces rather than a live path; the point is that the fallback's
//! safety no longer rests on a producer-side invariant this module can neither
//! state nor check.

use std::cmp::Ordering;

use reify_ir::value::SampledField;

use crate::medial::{normalize3, sample_at_index};

/// Smallest `|n̂ · a|` a genuine medial kink can present to the axis `a` the
/// ridge search picks — `1/√3`, attained on the body diagonal.
///
/// The search takes the axis of MAXIMUM one-sided slope, and `max_a |n_a| ≥
/// 1/√3` for every unit normal, so the bound is exact and free rather than
/// tuned. Below it the valley is not an obliquity being observed down a grid
/// axis; it is a field that is not a unit-slope distance function near this
/// voxel, and the "conversion factor" read off it would rescale a thickness by
/// an arbitrary amount instead of correcting it.
const MIN_RIDGE_NORMAL_COSINE: f64 = 0.577_350_269_189_625_8;

/// Relative slack on [`MIN_RIDGE_NORMAL_COSINE`], for the body-diagonal
/// orientation that sits exactly ON it.
///
/// The slope is recovered from differences of stored samples, so it can land a
/// few ulp below the bound it should meet exactly. `1e-6` covers that with ~9
/// orders of magnitude to spare, and still covers a grid stored as `f32`
/// (relative ε `1.2e-7`) — while staying far below the gap to anything this is
/// meant to reject.
const RIDGE_NORMAL_COSINE_SLACK: f64 = 1e-6;

/// A direction to walk, together with the factor that converts a distance
/// walked along it into a PERPENDICULAR one.
///
/// The two are always produced and consumed together — a caller that walks
/// without converting over-reads an oblique wall's thickness — so they travel
/// as one value rather than as a pair of parallel results the call sites could
/// drift apart on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WalkDirection {
    /// Unit vector; callers walk `±direction` from the voxel.
    pub(crate) direction: [f64; 3],
    /// `|n̂ · direction|`, where `n̂` is the crossed medial plane's normal.
    /// Multiply a distance walked along `direction` by it to get the
    /// perpendicular distance. Exactly `1.0` whenever the walk already follows
    /// the gradient, which is normal to that plane by construction.
    pub(crate) normal_cosine: f64,
}

/// Direction to walk from voxel `idx`, given the field value `phi` and the
/// already-computed raw `gradient` there.
///
/// Both are parameters rather than re-read here, for the same reason: each call
/// site already holds them — `compute_medial_mask` from its narrow-band test and
/// its precomputed gradient grid — and a second read would decouple the fallback
/// from the values that call site gates and asserts on.
pub(crate) fn medial_walk_direction(
    sdf: &SampledField,
    idx: [usize; 3],
    phi: f64,
    gradient: [f64; 3],
) -> Option<WalkDirection> {
    normalize3(gradient)
        .map(|direction| WalkDirection {
            direction,
            normal_cosine: 1.0,
        })
        .or_else(|| interior_ridge_axis(sdf, idx, phi))
}

/// Walk along the axis whose one-sided differences form the sharpest strict
/// valley at an interior voxel, or `None` if there is no such axis.
///
/// The returned SIGN is arbitrary (always `+axis`): every caller walks `±g`
/// and every downstream test is symmetric under swapping `d⁺` with `d⁻`.
fn interior_ridge_axis(sdf: &SampledField, idx: [usize; 3], phi: f64) -> Option<WalkDirection> {
    // Not `phi >= 0.0`: a NaN sample is not `Less` either, so this rejects it
    // rather than walking from it.
    if phi.partial_cmp(&0.0) != Some(Ordering::Less) {
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

    best.and_then(|(sharpness, axis)| {
        // Half the sharpness is the mean one-sided slope, which at a kink voxel
        // IS `|n_a|`. Clamped above so the conversion can only ever reduce a
        // reading: a super-unit slope is not an obliquity.
        let normal_cosine = (0.5 * sharpness).min(1.0);
        // Bounded below for the converse reason: a slope no obliquity could
        // produce is not a correction factor, and scaling by it would replace an
        // honest `NoMeasurement` with a confident under-read.
        if normal_cosine < MIN_RIDGE_NORMAL_COSINE * (1.0 - RIDGE_NORMAL_COSINE_SLACK) {
            return None;
        }
        let mut direction = [0.0; 3];
        direction[axis] = 1.0;
        Some(WalkDirection {
            direction,
            normal_cosine,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::value::{InterpolationKind, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    /// [`medial_walk_direction`] with the voxel's own `φ`, which is what both
    /// production call sites pass.
    fn walk_at(sdf: &SampledField, idx: [usize; 3], gradient: [f64; 3]) -> Option<WalkDirection> {
        medial_walk_direction(sdf, idx, sample_at_index(sdf, idx), gradient)
    }

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

    /// The expected result for a walk that already crosses its medial plane
    /// perpendicularly, so needs no obliquity conversion.
    fn perpendicular(direction: [f64; 3]) -> Option<WalkDirection> {
        Some(WalkDirection {
            direction,
            normal_cosine: 1.0,
        })
    }

    /// Interior solid with a `k`-axis kink at `k = 1`: `φ = |k − 1| − 2`.
    fn interior_k_valley(n: usize, spacing: [f64; 3]) -> SampledField {
        index_field(n, spacing, |_i, _j, k| (k as f64 - 1.0).abs() - 2.0)
    }

    #[test]
    fn a_usable_gradient_is_returned_normalised_and_otherwise_unchanged() {
        let sdf = interior_k_valley(3, [1.0; 3]);
        assert_eq!(
            walk_at(&sdf, [1, 1, 1], [0.0, 3.0, 4.0]),
            perpendicular([0.0, 0.6, 0.8])
        );
    }

    #[test]
    fn a_flat_region_has_no_walk_direction() {
        let sdf = index_field(3, [1.0; 3], |_i, _j, _k| -1.0);
        assert_eq!(walk_at(&sdf, [1, 1, 1], [0.0; 3]), None);
    }

    #[test]
    fn a_voxel_with_no_interior_neighbour_pair_on_the_kink_axis_has_no_walk_direction() {
        let sdf = interior_k_valley(3, [1.0; 3]);
        assert_eq!(walk_at(&sdf, [1, 1, 0], [0.0; 3]), None);
    }

    #[test]
    fn an_exterior_kink_is_not_a_medial_axis_of_the_material() {
        let sdf = index_field(3, [1.0; 3], |_i, _j, k| (k as f64 - 1.0).abs() + 1.0);
        assert_eq!(walk_at(&sdf, [1, 1, 1], [0.0; 3]), None);
    }

    #[test]
    fn an_interior_valley_yields_the_unit_vector_along_its_axis() {
        let sdf = interior_k_valley(3, [1.0; 3]);
        assert_eq!(
            walk_at(&sdf, [1, 1, 1], [0.0; 3]),
            perpendicular([0.0, 0.0, 1.0])
        );
    }

    #[test]
    fn the_sharper_of_two_valleys_wins() {
        // Square-ish bar with a twice-as-sharp kink on the k axis.
        let sdf = index_field(3, [1.0; 3], |i, _j, k| {
            (i as f64 - 1.0).abs().max(2.0 * (k as f64 - 1.0).abs()) - 3.0
        });
        assert_eq!(
            walk_at(&sdf, [1, 1, 1], [0.0; 3]),
            perpendicular([0.0, 0.0, 1.0])
        );
    }

    #[test]
    fn two_equally_sharp_valleys_resolve_to_the_lowest_axis_index() {
        let sdf = index_field(3, [1.0; 3], |i, _j, k| {
            (i as f64 - 1.0).abs().max((k as f64 - 1.0).abs()) - 3.0
        });
        assert_eq!(
            walk_at(&sdf, [1, 1, 1], [0.0; 3]),
            perpendicular([1.0, 0.0, 0.0])
        );
    }

    /// A valley perpendicular to its walk axis needs no conversion, so every
    /// pre-existing axis-aligned reading — the square bar's `4h` cross-section,
    /// the whole sub-voxel alignment sweep — is provably unchanged by #7527's
    /// obliquity correction.
    #[test]
    fn an_axis_aligned_valley_needs_no_obliquity_conversion() {
        let sdf = interior_k_valley(3, [1.0; 3]);
        assert_eq!(
            walk_at(&sdf, [1, 1, 1], [0.0; 3]).map(|walk| walk.normal_cosine),
            Some(1.0)
        );
    }

    /// The central difference cancels on an OBLIQUE medial plane too, so the
    /// fallback fires and walks an axis that crosses the wall diagonally. The
    /// conversion factor is what stops `d⁺ + d⁻` being read as a thickness it
    /// is not: here the walk is √2 too long, and the factor is `1/√2`.
    ///
    /// Exactness: the one-sided difference recovers `1/√2` bit-for-bit, because
    /// subtracting and re-adding the same `−2.0` offset is exact at this
    /// magnitude.
    #[test]
    fn an_oblique_valley_reports_the_cosine_between_its_plane_and_the_walk_axis() {
        let diagonal = 1.0 / 2f64.sqrt();
        // φ = |n̂·p| − 2 about (1, ·, 1), with n̂ = (1, 0, 1)/√2.
        let sdf = index_field(3, [1.0; 3], |i, _j, k| {
            diagonal * ((i as f64 - 1.0) + (k as f64 - 1.0)).abs() - 2.0
        });
        assert_eq!(
            walk_at(&sdf, [1, 1, 1], [0.0; 3]),
            Some(WalkDirection {
                // The x and z valleys are equally sharp; the tie-break picks x.
                direction: [1.0, 0.0, 0.0],
                normal_cosine: diagonal,
            })
        );
    }

    /// A slope above 1 is not an obliquity — a true distance field is
    /// 1-Lipschitz — so the factor is clamped. Without the clamp such a field
    /// would INFLATE a reading, and the whole point of carrying the factor is
    /// that it can only ever reduce one.
    #[test]
    fn a_super_unit_slope_is_clamped_rather_than_inflating_the_reading() {
        let sdf = index_field(3, [1.0; 3], |_i, _j, k| 3.0 * (k as f64 - 1.0).abs() - 5.0);
        assert_eq!(
            walk_at(&sdf, [1, 1, 1], [0.0; 3]),
            perpendicular([0.0, 0.0, 1.0])
        );
    }

    /// A valley far too shallow to be an obliquity is DECLINED, not scaled by.
    ///
    /// `φ = clamp(|k − 1| − 2.5, ±2)` is the shape a narrow-band-saturated SDF
    /// takes where its interior clamp plateau is exactly one voxel thick: a
    /// strict valley whose one-sided slopes are `±0.5`, on a wall of true
    /// thickness 5. Scaling by that `0.5` would halve the wall and hand
    /// `min_wall_thickness` a confident 2× under-read in place of an honest
    /// `NoMeasurement`.
    #[test]
    fn a_sub_unit_slope_valley_is_declined_rather_than_scaling_the_reading() {
        let sdf = index_field(3, [1.0; 3], |_i, _j, k| {
            ((k as f64 - 1.0).abs() - 2.5).clamp(-2.0, 2.0)
        });
        assert_eq!(walk_at(&sdf, [1, 1, 1], [0.0; 3]), None);
    }

    /// The body diagonal is the shallowest orientation a real medial plane can
    /// present to its own sharpest axis, so it sits exactly ON
    /// [`MIN_RIDGE_NORMAL_COSINE`] and must still be ACCEPTED — the bound
    /// separates non-kinks from obliquities, and must not clip the worst
    /// obliquity off the top of the range it admits.
    #[test]
    fn the_body_diagonal_sits_on_the_bound_and_is_still_accepted() {
        let diagonal = 1.0 / 3f64.sqrt();
        // φ = |n̂·p| − 2 about (1, 1, 1), with n̂ = (1, 1, 1)/√3.
        let sdf = index_field(3, [1.0; 3], |i, j, k| {
            diagonal * ((i as f64 - 1.0) + (j as f64 - 1.0) + (k as f64 - 1.0)).abs() - 2.0
        });
        let walk = walk_at(&sdf, [1, 1, 1], [0.0; 3])
            .expect("the body diagonal is ON the bound, not below it");
        // All three axes are equally sharp here; the tie-break picks x.
        assert_eq!(walk.direction, [1.0, 0.0, 0.0]);
        assert!(
            (walk.normal_cosine - diagonal).abs() <= 1e-12,
            "body-diagonal cosine {} is not 1/√3",
            walk.normal_cosine
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
            walk_at(&sdf, [1, 1, 1], [0.0; 3]),
            perpendicular([0.0, 0.0, 1.0])
        );
    }
}
