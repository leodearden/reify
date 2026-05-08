//! Per-voxel medial-mask algorithm for thin-solid mid-surface extraction.
//!
//! Implements PRD task T1
//! (`docs/prds/v0_4/structural-analysis-shells.md`): for each active voxel
//! in a 3D narrow-band SDF, walk the SDF gradient in two opposing
//! directions to find the nearest surface points; tag the voxel as medial
//! iff the two distances are within `distance_tolerance` AND the two
//! surface-hit gradients are roughly antiparallel (encoding the gradient
//! discontinuity at the medial axis).
//!
//! # Performance
//!
//! This is a v0.4-T1 *shippable skeleton*: per-voxel work in the inner
//! loop performs two `gradient_at_world` calls (12 trilinear samples
//! each) plus a bidirectional walk of up to `4 × max_thickness_voxels`
//! trilinear samples. At PRD-realistic 256³ grids (~16 M voxels) the
//! cost is on the order of `O(N³ · max_thickness_voxels)` trilinear
//! interpolations. The current narrow-band-filtered slab/sphere
//! fixtures (≈ 512 active voxels each) make this a non-issue for tests,
//! but production use will need optimization before the T2/T3/T4
//! follow-up tasks land. Deferred optimizations:
//!
//! - cache an explicit gradient grid (avoid recomputing central
//!   differences inside the walk),
//! - parallelize the outer voxel loop with `rayon`,
//! - replace the dense iteration with OpenVDB's sparse active-voxel
//!   iterator once the FFI ships (the `narrow_band_half_width_voxels`
//!   filter here is an explicit emulation of that iterator on a dense
//!   `SampledField`).
//!
//! No bug today — just a flag for the perf-tuning pass that follows
//! once the OpenVDB FFI is wired in.

use reify_types::value::{SampledField, SampledGridKind};

/// Sparse voxel mask: indices `(i, j, k)` of every voxel tagged as medial
/// by [`compute_medial_mask`].
///
/// Storage is a `Vec<[i32; 3]>` rather than `openvdb::BoolGrid` because
/// the OpenVDB FFI is upstream and not yet shipping. The PRD permits
/// `openvdb::BoolGrid OR EQUIVALENT`; downstream T2/T3/T4 consumers
/// (mid-surface mesh extraction, branch pruning, region segmentation) all
/// iterate the mask voxels regardless of underlying storage. The pure-Rust
/// representation here lets the algorithm ship now and the storage backing
/// can be swapped behind the same public API once the FFI lands.
#[derive(Debug, Clone, PartialEq)]
pub struct MedialMask {
    /// Per-axis voxel spacing copied from the input
    /// [`SampledField::spacing`]. Length always 3.
    pub spacing: [f64; 3],
    /// Origin of voxel index `(0, 0, 0)` in world coordinates, copied
    /// from the input [`SampledField::bounds_min`]. Length always 3.
    pub origin: [f64; 3],
    /// Voxel indices flagged as medial. Each entry is the
    /// `(i, j, k)` index in the input grid (axis-0 outermost convention,
    /// matching `SampledField::data` row-major layout).
    pub voxels: Vec<[i32; 3]>,
}

/// Tunable thresholds for the medial-axis test.
///
/// Defaults are pinned to PRD-derived values
/// (`docs/prds/v0_4/structural-analysis-shells.md` task T1). Each
/// field documents its rationale; the
/// [`medial_options_defaults_pin_empirical_constants`] regression
/// test asserts the values do not drift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MedialOptions {
    /// Relative-distance equality tolerance for the bidirectional ray
    /// walk: a voxel is medial only if `|d⁺ − d⁻| < distance_tolerance ·
    /// max(d⁺, d⁻) + ½·voxelization_slack` (the slack is implementation
    /// -side; see [`compute_medial_mask`]). Default `0.05` matches the
    /// PRD's "~5%" language (T1 step-2).
    ///
    /// **Caveat — discriminative range.** The implementation composes
    /// this *relative* tolerance with an *absolute* one-voxel slack to
    /// admit voxels whose centers sit ½-voxel off the analytic medial
    /// axis (which already produces a full-voxel asymmetry in d⁺/d⁻ on
    /// even-N grids — see the inline comment in [`compute_medial_mask`]).
    /// Because that absolute term is unconditional, this field only
    /// *meaningfully* discriminates when `distance_tolerance · dmax >>
    /// 1 voxel`, i.e. for solids whose half-thickness is much greater
    /// than `1 / distance_tolerance` voxels (≈ 20 voxels at the default).
    /// For thin shells (PRD-typical 3–10 voxels thick), the absolute
    /// slack dominates and tightening this field has near-zero effect
    /// on the mask. Loosening it to e.g. `0.5` *is* effective even for
    /// thin shells (it admits more off-centerline voxels). The
    /// `tightening_distance_tolerance_reduces_slab_mask_size` test
    /// exercises this discriminative regime by bracketing the slab's
    /// relative-error spectrum directly.
    pub distance_tolerance: f64,
    /// Narrow-band half-width measured in voxel-spacing units. Voxels
    /// with `|φ(v)| > narrow_band_half_width_voxels × spacing` are
    /// excluded from the inner loop, emulating OpenVDB's sparse
    /// active-voxel iterator on top of a dense `SampledField`.
    /// Default `3.0` covers the smallest medial axis at the PRD's
    /// `thickness/3` voxel-size default (smallest medial slab is 3
    /// voxels thick → half-width 1.5; 3 leaves headroom for
    /// gradient-stencil sampling at the boundary voxels).
    pub narrow_band_half_width_voxels: f64,
    /// Surface-patch distinctness threshold on the dot product of the
    /// SDF gradients sampled at the two surface-hit points. The
    /// gradient at a surface point IS the outward normal; a voxel is
    /// medial iff `g_a · g_b < normal_antiparallel_threshold`
    /// (gradients are roughly antiparallel — the gradient
    /// discontinuity at the medial axis itself). Default `-0.5`
    /// (≈ cos 120°) accepts hits whose normals are at least
    /// 60° beyond perpendicular, the empirical signature of "opposing
    /// faces of a thin slab".
    pub normal_antiparallel_threshold: f64,
    /// Maximum bidirectional ray-walk distance in voxel-spacing units.
    /// Truncates the walk if the gradient-direction ray fails to find
    /// a zero crossing within this many voxels — guards against
    /// runaway walks on degenerate gradients. Default `64.0` covers
    /// thick (≪ 64-voxel half-thickness) solids.
    pub max_thickness_voxels: f64,
}

impl Default for MedialOptions {
    fn default() -> Self {
        Self {
            distance_tolerance: 0.05,
            narrow_band_half_width_voxels: 3.0,
            normal_antiparallel_threshold: -0.5,
            max_thickness_voxels: 64.0,
        }
    }
}

/// Errors returned by [`compute_medial_mask`].
///
/// Carries typed structural information in each variant rather than
/// stringly-typed messages, mirroring the
/// [`reify_kernel_openvdb::ingest::IngestError`] precedent: the caller
/// can pattern-match on the variant to drive recovery logic.
#[derive(Debug, Clone, PartialEq)]
pub enum MedialError {
    /// The input [`SampledField`] is not 3D — only [`SampledGridKind::Regular3D`]
    /// is supported. The medial-axis test is intrinsically 3D (it walks
    /// the SDF gradient in two opposing directions in 3-space); 1D / 2D
    /// inputs are rejected up front rather than silently producing an
    /// empty mask.
    UnsupportedGridKind {
        /// The actual kind found on the input field.
        found: SampledGridKind,
    },
    /// One or more of [`SampledField::bounds_min`] / `bounds_max` /
    /// `spacing` / `axis_grids` does not have length 3, contradicting
    /// the field's `kind = Regular3D`. Defends downstream indexing
    /// (e.g. `bounds_min[i]`) against a caller-side construction
    /// mistake that would otherwise panic mid-loop.
    AxisLengthMismatch {
        /// Length of the supplied `bounds_min` vector.
        bounds_min_len: usize,
        /// Length of the supplied `bounds_max` vector.
        bounds_max_len: usize,
        /// Length of the supplied `spacing` vector.
        spacing_len: usize,
        /// Length of the supplied `axis_grids` vector.
        axis_grids_len: usize,
    },
    /// One axis's grid coordinate vector is empty, which would yield a
    /// zero-extent grid and break the narrow-band iteration.
    EmptyAxisGrid {
        /// Index of the offending axis (0/1/2 for x/y/z).
        axis: usize,
    },
    /// A per-axis geometry invariant is violated. Covers two related
    /// classes of caller-side construction error:
    ///
    /// - **Non-positive / non-finite spacing** (`!(spacing > 0.0 &&
    ///   spacing.is_finite())`): zero → divide-by-zero in
    ///   `sample_at_world`; negative → flipped index direction in
    ///   trilinear interpolation; NaN or ±Inf → NaN propagation through
    ///   fractional-index math. The `is_finite` guard in `sample_at_world`
    ///   would silently discard all of these as OOB rather than surfacing
    ///   them as a typed error.
    /// - **Inverted bounds** (`bounds_min > bounds_max`): `world_at_index`
    ///   and `sample_at_world` produce geometrically nonsensical results;
    ///   the `is_finite` guard masks them as OOB. (Equal bounds —
    ///   zero-extent axis — is allowed: a 1-voxel axis is a valid
    ///   degenerate-but-legal configuration.)
    ///
    /// Carries all four per-axis data fields so callers (and the Display
    /// impl) can determine which condition fired without sub-enum
    /// scaffolding.
    InvalidAxisGeometry {
        /// Index of the offending axis (0/1/2 for x/y/z).
        axis: usize,
        /// The `spacing` value on the offending axis.
        spacing: f64,
        /// The `bounds_min` value on the offending axis.
        bounds_min: f64,
        /// The `bounds_max` value on the offending axis.
        bounds_max: f64,
    },
    /// The flat `data` vector length does not match `nx * ny * nz`
    /// (where `nx/ny/nz = axis_grids[i].len()`). Defends the inner
    /// triple-nested loop's `sample_at_index` (`data[i*nj*nk + j*nk + k]`)
    /// from a caller-side construction error that would otherwise produce
    /// an opaque out-of-bounds panic mid-iteration.
    DataLengthMismatch {
        /// The required flat-data length: `nx * ny * nz`.
        expected: usize,
        /// The actual length of `sdf.data`.
        found: usize,
    },
}

impl std::fmt::Display for MedialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MedialError::UnsupportedGridKind { found } => write!(
                f,
                "reify-shell-extract requires a Regular3D SampledField input \
                 (the medial-axis test walks the SDF gradient in 3-space); \
                 got {found:?}"
            ),
            MedialError::AxisLengthMismatch {
                bounds_min_len,
                bounds_max_len,
                spacing_len,
                axis_grids_len,
            } => write!(
                f,
                "Regular3D SampledField axis-vector length mismatch: \
                 bounds_min has {bounds_min_len}, bounds_max has {bounds_max_len}, \
                 spacing has {spacing_len}, axis_grids has {axis_grids_len} \
                 (all four must be 3)"
            ),
            MedialError::EmptyAxisGrid { axis } => write!(
                f,
                "Regular3D SampledField axis_grids[{axis}] is empty \
                 (a non-empty per-axis grid is required for narrow-band iteration)"
            ),
            MedialError::InvalidAxisGeometry {
                axis,
                spacing,
                bounds_min,
                bounds_max,
            } => {
                let sp_bad = !(spacing.is_finite() && *spacing > 0.0);
                let bn_bad = !bounds_min.is_finite()
                    || !bounds_max.is_finite()
                    || bounds_min > bounds_max;
                match (sp_bad, bn_bad) {
                    (true, true) => write!(
                        f,
                        "Regular3D SampledField axis {axis} has invalid spacing {spacing} \
                         AND invalid bounds (bounds_min={bounds_min}, bounds_max={bounds_max}): \
                         spacing must be finite and positive; \
                         bounds must be finite and non-inverted"
                    ),
                    (true, false) => write!(
                        f,
                        "Regular3D SampledField axis {axis} has invalid spacing {spacing}: \
                         spacing must be finite and positive (got {spacing}); \
                         bounds_min={bounds_min}, bounds_max={bounds_max}"
                    ),
                    (false, true) => write!(
                        f,
                        "Regular3D SampledField axis {axis} has invalid bounds: \
                         bounds_min={bounds_min}, bounds_max={bounds_max} \
                         (bounds must be finite and bounds_min ≤ bounds_max; \
                         spacing={spacing})"
                    ),
                    (false, false) => write!(
                        f,
                        "Regular3D SampledField axis {axis}: InvalidAxisGeometry \
                         constructed with spacing={spacing}, \
                         bounds_min={bounds_min}, bounds_max={bounds_max} \
                         (no violation detected — variant was constructed outside the validator)"
                    ),
                }
            }
            MedialError::DataLengthMismatch { expected, found } => write!(
                f,
                "Regular3D SampledField data length mismatch: \
                 expected {expected} values (nx*ny*nz) but found {found} \
                 (caller-side construction error: flat data does not match axis grid extents)"
            ),
        }
    }
}

impl std::error::Error for MedialError {}

/// Compute the per-voxel medial mask for a Regular3D narrow-band SDF.
///
/// # Algorithm overview
///
/// For each voxel inside the narrow band, walk the normalized SDF
/// gradient in `+g` and `−g` until each ray crosses the zero level set;
/// tag the voxel as medial iff (a) `|d⁺ − d⁻| / max(d⁺, d⁻) <
/// distance_tolerance` AND (b) the gradients sampled at the two hit
/// points are roughly antiparallel (`g_a · g_b < normal_antiparallel_threshold`).
pub fn compute_medial_mask(
    sdf: &SampledField,
    options: &MedialOptions,
) -> Result<MedialMask, MedialError> {
    // (1) Reject non-3D inputs up front. The medial-axis test is
    // intrinsically 3D (walks the SDF gradient in 3-space).
    if sdf.kind != SampledGridKind::Regular3D {
        return Err(MedialError::UnsupportedGridKind { found: sdf.kind });
    }

    // (2) Defend downstream indexing: `Regular3D` requires every axis
    // vector to have length 3. A caller-side construction mistake
    // (e.g. building a `Regular3D` SampledField with 1-element
    // bounds_min) would otherwise panic on `bounds_min[i]` mid-loop.
    if sdf.bounds_min.len() != 3
        || sdf.bounds_max.len() != 3
        || sdf.spacing.len() != 3
        || sdf.axis_grids.len() != 3
    {
        return Err(MedialError::AxisLengthMismatch {
            bounds_min_len: sdf.bounds_min.len(),
            bounds_max_len: sdf.bounds_max.len(),
            spacing_len: sdf.spacing.len(),
            axis_grids_len: sdf.axis_grids.len(),
        });
    }

    // (3) Geometry validity: spacing must be finite and positive, and
    // bounds must not be inverted. Safe here because step 2 confirmed
    // all axis vectors have length 3, so sdf.spacing[axis] /
    // sdf.bounds_min[axis] / sdf.bounds_max[axis] are all in-bounds.
    //
    // Spacing rule — rejects zero (divide-by-zero in sample_at_world),
    // negative (flipped index direction), NaN and +Inf (NaN propagation
    // through fractional-index math). The single predicate
    // `is_finite() && > 0.0` covers all four classes.
    //
    // Bound rule — rejects non-finite bounds (NaN bounds bypass the
    // `bmin > bmax` comparison via IEEE-754 false return) and inverted
    // bounds (geometrically inverted grid). Equal bounds (zero-extent,
    // single-voxel axis) is explicitly allowed because the existing
    // doctest and one_voxel_field fixture both use that configuration.
    //
    // Both failures produce the same variant because they arise from
    // the same root cause (corrupt axis geometry) and both have the
    // same symptom (sample_at_world's is_finite guard silently masking
    // the corruption as OOB).
    for axis in 0..3 {
        let sp = sdf.spacing[axis];
        let bmin = sdf.bounds_min[axis];
        let bmax = sdf.bounds_max[axis];
        if !(sp.is_finite() && sp > 0.0 && bmin.is_finite() && bmax.is_finite() && bmin <= bmax) {
            return Err(MedialError::InvalidAxisGeometry {
                axis,
                spacing: sp,
                bounds_min: bmin,
                bounds_max: bmax,
            });
        }
    }

    // (4) Each axis grid must be non-empty — a zero-extent axis
    // collapses the iteration domain.
    for (axis, axis_grid) in sdf.axis_grids.iter().enumerate() {
        if axis_grid.is_empty() {
            return Err(MedialError::EmptyAxisGrid { axis });
        }
    }

    let spacing = [sdf.spacing[0], sdf.spacing[1], sdf.spacing[2]];
    let origin = [sdf.bounds_min[0], sdf.bounds_min[1], sdf.bounds_min[2]];

    let nx = sdf.axis_grids[0].len();
    let ny = sdf.axis_grids[1].len();
    let nz = sdf.axis_grids[2].len();

    // (5) Validate the flat data vector covers exactly nx*ny*nz voxels.
    // Safe here because step 4 (EmptyAxisGrid) confirmed nx, ny, nz ≥ 1.
    // Without this check a caller-side construction error (mismatched
    // data vs. axis grid extents) would produce an opaque OOB panic
    // inside the inner loop's `sample_at_index`.
    //
    // checked_mul guards against wrapping overflow on a malformed input
    // with astronomically large axis_grids (e.g. nx=ny=nz=2_000_000
    // wraps to a small product on 64-bit usize and could pass a naive
    // equality check with a tiny data.len()). On overflow we use
    // usize::MAX as the sentinel expected value: no real data vector can
    // have length usize::MAX, so the DataLengthMismatch error is still
    // surfaced.
    let expected_data_len = nx
        .checked_mul(ny)
        .and_then(|p| p.checked_mul(nz))
        .unwrap_or(usize::MAX);
    if sdf.data.len() != expected_data_len {
        return Err(MedialError::DataLengthMismatch {
            expected: expected_data_len,
            found: sdf.data.len(),
        });
    }

    // Narrow band threshold uses the smallest axis spacing so that
    // anisotropic grids still cover the full thickness band.
    let min_spacing = spacing[0].min(spacing[1]).min(spacing[2]);
    let band_width = options.narrow_band_half_width_voxels * min_spacing;

    // Truncation distance for the bidirectional walk in absolute
    // (world) units.
    let max_walk_dist = options.max_thickness_voxels * min_spacing;
    // Step size for the walk: 1/4 of the smallest voxel spacing keeps
    // the sub-voxel zero-crossing refinement accurate while bounding
    // the per-voxel work to ≈ 4 × max_thickness_voxels samples.
    let walk_step = 0.25 * min_spacing;
    let max_steps = ((max_walk_dist / walk_step).ceil() as usize).max(2);

    let mut voxels: Vec<[i32; 3]> = Vec::new();

    for i in 0..nx {
        for j in 0..ny {
            for k in 0..nz {
                let phi = sample_at_index(sdf, [i, j, k]);

                // (a) narrow-band filter
                if phi.abs() > band_width {
                    continue;
                }

                // (b) gradient at the voxel; reject degenerate
                let grad = gradient_at_index(sdf, [i, j, k]);
                let gnorm = (grad[0] * grad[0] + grad[1] * grad[1] + grad[2] * grad[2]).sqrt();
                if gnorm < GRADIENT_EPSILON {
                    continue;
                }
                let g = [grad[0] / gnorm, grad[1] / gnorm, grad[2] / gnorm];

                // (c) bidirectional ray walk from the voxel's world
                // coordinate in ±g, with sub-voxel zero-crossing
                // refinement.
                let world = world_at_index(sdf, [i, j, k]);
                let Some((d_plus, d_minus, hit_plus, hit_minus)) =
                    bidirectional_distances(sdf, world, g, max_steps, walk_step)
                else {
                    continue;
                };

                // (d) gradient at each surface hit; reject if either
                // is degenerate (we cannot make a distinctness call
                // without two well-defined normals).
                let gp_raw = gradient_at_world(sdf, hit_plus);
                let gm_raw = gradient_at_world(sdf, hit_minus);
                let Some(gp) = normalize3(gp_raw) else { continue };
                let Some(gm) = normalize3(gm_raw) else { continue };

                // (e) gradient-discontinuity test: opposing-face
                // hits have antiparallel normals (dot near -1).
                if !surface_patches_distinct(gp, gm, options.normal_antiparallel_threshold) {
                    continue;
                }

                // (f) bidirectional-distance equality.
                let dmax = d_plus.max(d_minus);
                if dmax <= 0.0 {
                    continue;
                }
                // Defends against a long-walk-on-one-side / short-on-
                // the-other ratio coincidence: if the sum exceeds the
                // configured maximum thickness, treat the voxel as
                // outside the band rather than letting two
                // non-comparable walks pass the relative test.
                if d_plus + d_minus > 2.0 * max_walk_dist {
                    continue;
                }
                // Equality threshold combines two terms:
                //  - relative `distance_tolerance × max(d⁺, d⁻)` — the
                //    PRD's "~5%" rule, sharply discriminates centerline
                //    voxels when the medial axis aligns with a voxel.
                //  - absolute one-voxel slack `min_spacing` — handles
                //    voxelized medial axes that fall *between* grid
                //    points (the analytic medial of an axis-aligned
                //    slab on an even-N grid lies on a half-voxel
                //    boundary, so the two closest voxels see a
                //    one-voxel asymmetry in d⁺/d⁻ — namely
                //    `min_spacing` — that the strict relative rule
                //    would reject). The slack is therefore exactly
                //    `min_spacing`, NOT `½·min_spacing`: a half-voxel
                //    *offset* of the voxel center from the analytic
                //    medial produces a full-voxel asymmetry in
                //    d⁺/d⁻ along the gradient direction. Equivalent
                //    to requiring the bisecting midpoint of the two
                //    surface hits to lie within `½(min_spacing +
                //    distance_tolerance × max(d⁺, d⁻))` of the voxel
                //    center along the gradient direction — i.e.
                //    inside the voxel itself, with a small
                //    relative-error cushion.
                //
                //  CONSEQUENCE: for thicknesses ≪ 20 voxels the
                //  absolute slack dominates and `distance_tolerance`
                //  is effectively inert at typical (≤ 0.05) values.
                //  See the doc comment on
                //  `MedialOptions::distance_tolerance` for the
                //  discriminative-regime caveat.
                let abs_diff = (d_plus - d_minus).abs();
                let equality_threshold = options.distance_tolerance * dmax + min_spacing;
                if abs_diff < equality_threshold {
                    voxels.push([i as i32, j as i32, k as i32]);
                }
            }
        }
    }

    Ok(MedialMask {
        spacing,
        origin,
        voxels,
    })
}

/// Floor on `‖∇φ‖` below which the voxel's gradient is treated as
/// degenerate and the voxel is skipped. Catches the central-difference
/// zero at exact-medial points (where the SDF gradient is genuinely
/// undefined) and at flat-interior voxels far from the surface.
pub(crate) const GRADIENT_EPSILON: f64 = 1e-6;

/// Floor on `|φ|` for treating an SDF sample as bit-exact zero in the
/// zero-crossing walk. Used both to short-circuit the walk when a
/// stepped sample lands exactly on the surface (a real possibility
/// for analytic SDFs — e.g. a slab `|z| − h` produces an exact zero
/// at `z == h` when the walk steps land there) and as the
/// denominator-magnitude guard for the linear interpolation between
/// bracketing samples. Keeping the two zero-related thresholds
/// numerically consistent avoids a class of "exact-zero comparison"
/// fragility issues.
pub(crate) const ZERO_PHI_EPSILON: f64 = 1e-30;

/// Look up `φ` at integer voxel indices `[i, j, k]`. Assumes
/// row-major axis-0-outermost layout (matches
/// [`SampledField::data`]).
pub(crate) fn sample_at_index(sdf: &SampledField, idx: [usize; 3]) -> f64 {
    let [i, j, k] = idx;
    let nj = sdf.axis_grids[1].len();
    let nk = sdf.axis_grids[2].len();
    sdf.data[i * nj * nk + j * nk + k]
}

/// Convert integer voxel indices to world coordinates via
/// `bounds_min[i] + idx[i] * spacing[i]`. Pulls from `axis_grids`
/// (rather than re-computing from `bounds_min/spacing`) so the result
/// stays consistent with whatever the SampledField producer chose
/// (e.g. whether the axis grid is `linspace(min, max, n)` exactly or
/// has been adjusted for floating-point tightening).
pub(crate) fn world_at_index(sdf: &SampledField, idx: [usize; 3]) -> [f64; 3] {
    [
        sdf.axis_grids[0][idx[0]],
        sdf.axis_grids[1][idx[1]],
        sdf.axis_grids[2][idx[2]],
    ]
}

/// Trilinear interpolation of `φ` at a world coordinate. Returns
/// `None` if the world coordinate falls outside the grid (callers
/// treat that as a ray-walk failure, not an error).
pub(crate) fn sample_at_world(sdf: &SampledField, world: [f64; 3]) -> Option<f64> {
    let nx = sdf.axis_grids[0].len();
    let ny = sdf.axis_grids[1].len();
    let nz = sdf.axis_grids[2].len();
    if nx == 0 || ny == 0 || nz == 0 {
        return None;
    }

    // Convert world → fractional index via uniform linspace assumption.
    // (Regular3D fields constructed by `OpenVdbGridSource → SampledField`
    // have uniform axis spacing by construction.)
    let fi = (world[0] - sdf.bounds_min[0]) / sdf.spacing[0];
    let fj = (world[1] - sdf.bounds_min[1]) / sdf.spacing[1];
    let fk = (world[2] - sdf.bounds_min[2]) / sdf.spacing[2];

    if !fi.is_finite() || !fj.is_finite() || !fk.is_finite() {
        return None;
    }

    if fi < 0.0 || fj < 0.0 || fk < 0.0 {
        return None;
    }
    let imax = (nx as f64) - 1.0;
    let jmax = (ny as f64) - 1.0;
    let kmax = (nz as f64) - 1.0;
    if fi > imax || fj > jmax || fk > kmax {
        return None;
    }

    // Clamp to interior cell: each integer corner index is in [0, n-1]
    // and the fractional offset in [0, 1].
    let i0 = (fi.floor() as usize).min(nx - 1);
    let j0 = (fj.floor() as usize).min(ny - 1);
    let k0 = (fk.floor() as usize).min(nz - 1);
    let i1 = (i0 + 1).min(nx - 1);
    let j1 = (j0 + 1).min(ny - 1);
    let k1 = (k0 + 1).min(nz - 1);
    let tx = (fi - i0 as f64).clamp(0.0, 1.0);
    let ty = (fj - j0 as f64).clamp(0.0, 1.0);
    let tz = (fk - k0 as f64).clamp(0.0, 1.0);

    let c000 = sample_at_index(sdf, [i0, j0, k0]);
    let c100 = sample_at_index(sdf, [i1, j0, k0]);
    let c010 = sample_at_index(sdf, [i0, j1, k0]);
    let c110 = sample_at_index(sdf, [i1, j1, k0]);
    let c001 = sample_at_index(sdf, [i0, j0, k1]);
    let c101 = sample_at_index(sdf, [i1, j0, k1]);
    let c011 = sample_at_index(sdf, [i0, j1, k1]);
    let c111 = sample_at_index(sdf, [i1, j1, k1]);

    let c00 = c000 * (1.0 - tx) + c100 * tx;
    let c10 = c010 * (1.0 - tx) + c110 * tx;
    let c01 = c001 * (1.0 - tx) + c101 * tx;
    let c11 = c011 * (1.0 - tx) + c111 * tx;

    let c0 = c00 * (1.0 - ty) + c10 * ty;
    let c1 = c01 * (1.0 - ty) + c11 * ty;

    Some(c0 * (1.0 - tz) + c1 * tz)
}

/// Central-difference gradient at integer voxel indices, falling
/// back to forward/backward differences at boundaries. Returns the
/// raw gradient (not normalized).
pub(crate) fn gradient_at_index(sdf: &SampledField, idx: [usize; 3]) -> [f64; 3] {
    let [i, j, k] = idx;
    let nx = sdf.axis_grids[0].len();
    let ny = sdf.axis_grids[1].len();
    let nz = sdf.axis_grids[2].len();
    let dx = sdf.spacing[0];
    let dy = sdf.spacing[1];
    let dz = sdf.spacing[2];

    let gx = if nx == 1 {
        0.0
    } else if i == 0 {
        (sample_at_index(sdf, [1, j, k]) - sample_at_index(sdf, [0, j, k])) / dx
    } else if i == nx - 1 {
        (sample_at_index(sdf, [nx - 1, j, k]) - sample_at_index(sdf, [nx - 2, j, k])) / dx
    } else {
        (sample_at_index(sdf, [i + 1, j, k]) - sample_at_index(sdf, [i - 1, j, k])) / (2.0 * dx)
    };
    let gy = if ny == 1 {
        0.0
    } else if j == 0 {
        (sample_at_index(sdf, [i, 1, k]) - sample_at_index(sdf, [i, 0, k])) / dy
    } else if j == ny - 1 {
        (sample_at_index(sdf, [i, ny - 1, k]) - sample_at_index(sdf, [i, ny - 2, k])) / dy
    } else {
        (sample_at_index(sdf, [i, j + 1, k]) - sample_at_index(sdf, [i, j - 1, k])) / (2.0 * dy)
    };
    let gz = if nz == 1 {
        0.0
    } else if k == 0 {
        (sample_at_index(sdf, [i, j, 1]) - sample_at_index(sdf, [i, j, 0])) / dz
    } else if k == nz - 1 {
        (sample_at_index(sdf, [i, j, nz - 1]) - sample_at_index(sdf, [i, j, nz - 2])) / dz
    } else {
        (sample_at_index(sdf, [i, j, k + 1]) - sample_at_index(sdf, [i, j, k - 1])) / (2.0 * dz)
    };
    [gx, gy, gz]
}

/// Gradient at a world coordinate via central finite differences over
/// `sample_at_world`. Returns `[0, 0, 0]` (caller treats as degenerate)
/// if either side falls outside the grid.
///
/// **Per-axis stencil offsets.** Each component uses
/// `h_axis = 0.5 · spacing[axis]` for its own central-difference
/// stencil, rather than a single global `h = 0.5 · min(spacing)`. The
/// per-axis form is numerically defensible on anisotropic grids: the
/// stencil offset along each axis matches the local voxel size, so
/// the gradient estimate reflects underlying sampled values rather
/// than trilinear-interpolation artifacts at sub-voxel offsets along
/// the coarse axis. (No anisotropic test fixture currently exercises
/// this — every fixture in the in-crate test suite uses unit spacing
/// on all three axes; full anisotropic-grid coverage is deferred to
/// integration tests against real `.vdb` files once the OpenVDB FFI
/// lands. The per-axis form here at least removes one numerical
/// pitfall in advance.)
pub(crate) fn gradient_at_world(sdf: &SampledField, world: [f64; 3]) -> [f64; 3] {
    let hx = 0.5 * sdf.spacing[0];
    let hy = 0.5 * sdf.spacing[1];
    let hz = 0.5 * sdf.spacing[2];

    let gx = match (
        sample_at_world(sdf, [world[0] + hx, world[1], world[2]]),
        sample_at_world(sdf, [world[0] - hx, world[1], world[2]]),
    ) {
        (Some(p), Some(m)) => (p - m) / (2.0 * hx),
        _ => 0.0,
    };
    let gy = match (
        sample_at_world(sdf, [world[0], world[1] + hy, world[2]]),
        sample_at_world(sdf, [world[0], world[1] - hy, world[2]]),
    ) {
        (Some(p), Some(m)) => (p - m) / (2.0 * hy),
        _ => 0.0,
    };
    let gz = match (
        sample_at_world(sdf, [world[0], world[1], world[2] + hz]),
        sample_at_world(sdf, [world[0], world[1], world[2] - hz]),
    ) {
        (Some(p), Some(m)) => (p - m) / (2.0 * hz),
        _ => 0.0,
    };
    [gx, gy, gz]
}

/// Walk the SDF along `+gradient_unit` and `-gradient_unit` from
/// `voxel_world`, find the zero-crossing distance and world location
/// in each direction via linear sub-step interpolation, and return
/// `(d_plus, d_minus, hit_plus, hit_minus)`. Returns `None` if either
/// ray fails to find a zero crossing within `max_steps`.
pub(crate) fn bidirectional_distances(
    sdf: &SampledField,
    voxel_world: [f64; 3],
    gradient_unit: [f64; 3],
    max_steps: usize,
    step_size: f64,
) -> Option<(f64, f64, [f64; 3], [f64; 3])> {
    let plus = walk_to_zero(sdf, voxel_world, gradient_unit, max_steps, step_size)?;
    let neg = [-gradient_unit[0], -gradient_unit[1], -gradient_unit[2]];
    let minus = walk_to_zero(sdf, voxel_world, neg, max_steps, step_size)?;
    Some((plus.0, minus.0, plus.1, minus.1))
}

/// Single-direction zero-crossing walk. Steps by `step_size` along
/// `direction` (assumed unit) until the SDF sign changes, then
/// linearly interpolates between the bracketing samples to recover the
/// sub-voxel zero-crossing distance + world coordinate.
///
/// # Preconditions
///
/// **Monotonicity along `direction`** — caller is responsible for
/// ensuring `φ` is monotone (or at least sign-monotone: no spurious
/// zero-crossings before the geometric nearest surface) along the
/// half-line `start + t·direction` for `t ∈ [0, max_steps · step_size]`.
/// On non-monotone SDFs (e.g. a thin solid with an interior re-entrant
/// feature, or a curvature-induced re-entry along the gradient
/// half-line) this function returns the *first* zero-crossing in walk
/// order, which may NOT be the geometric nearest surface point.
/// [`compute_medial_mask`] satisfies this precondition for the slab,
/// sphere, and thick-block fixtures (each is sign-monotone along the
/// SDF gradient direction by construction); a non-convex thin solid
/// (e.g. a C-channel cross-section) would violate it and silently
/// misclassify boundary voxels — the regression-test surface here is
/// thin, by design, until T2 lands the iso-surface extraction that
/// will tighten correctness on irregular geometry.
fn walk_to_zero(
    sdf: &SampledField,
    start: [f64; 3],
    direction: [f64; 3],
    max_steps: usize,
    step_size: f64,
) -> Option<(f64, [f64; 3])> {
    let phi0 = sample_at_world(sdf, start)?;
    let mut prev_t = 0.0;
    let mut prev_phi = phi0;
    for s in 1..=max_steps {
        let t = (s as f64) * step_size;
        let p = [
            start[0] + direction[0] * t,
            start[1] + direction[1] * t,
            start[2] + direction[2] * t,
        ];
        // Stepped off the grid before crossing zero — caller treats
        // the voxel as non-medial (the `?` propagates `None`).
        let phi = sample_at_world(sdf, p)?;
        // Sign change (including landing exactly on zero) marks the
        // bracketing pair. Linear interpolation between (prev_t, phi)
        // and (t, phi) recovers the zero-crossing.
        // `prev_phi == 0` (effectively): the previous step landed on
        // the surface itself — return that t/world directly without
        // attempting a linear interpolation that would divide by a
        // near-zero denominator. Use the same `ZERO_PHI_EPSILON`
        // threshold as the linear-interpolation denominator guard so
        // both zero-related branches stay numerically consistent.
        if prev_phi.abs() < ZERO_PHI_EPSILON {
            return Some((prev_t, point_at(start, direction, prev_t)));
        }
        if (prev_phi > 0.0 && phi <= 0.0) || (prev_phi < 0.0 && phi >= 0.0) {
            let denom = phi - prev_phi;
            // `denom == 0` would have been caught by the prev_phi
            // checks above; defensively guard anyway with the same
            // ZERO_PHI_EPSILON threshold.
            if denom.abs() < ZERO_PHI_EPSILON {
                return Some((t, p));
            }
            let alpha = -prev_phi / denom;
            let t_zero = prev_t + alpha * (t - prev_t);
            return Some((t_zero, point_at(start, direction, t_zero)));
        }
        prev_t = t;
        prev_phi = phi;
    }
    None
}

fn point_at(start: [f64; 3], direction: [f64; 3], t: f64) -> [f64; 3] {
    [
        start[0] + direction[0] * t,
        start[1] + direction[1] * t,
        start[2] + direction[2] * t,
    ]
}

/// Two surface-hit gradients are "distinct surface patches" iff their
/// dot product is below `threshold` — i.e. the gradients are roughly
/// antiparallel. The threshold defaults to `-0.5` (cos 120°): the
/// gradient discontinuity at a medial axis manifests as opposing-face
/// normals at the two hit points; the test rejects same-patch hits
/// (dot near +1) and near-orthogonal hits (dot near 0).
pub(crate) fn surface_patches_distinct(
    g_plus: [f64; 3],
    g_minus: [f64; 3],
    threshold: f64,
) -> bool {
    let dot = g_plus[0] * g_minus[0] + g_plus[1] * g_minus[1] + g_plus[2] * g_minus[2];
    dot < threshold
}

fn normalize3(v: [f64; 3]) -> Option<[f64; 3]> {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n < GRADIENT_EPSILON {
        None
    } else {
        Some([v[0] / n, v[1] / n, v[2] / n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::value::{InterpolationKind, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    /// Build a trivial 1×1×1 Regular3D `SampledField` with the given
    /// scalar SDF value at the single voxel. Used as a public-surface
    /// smoke test that exercises every type the crate re-exports
    /// without invoking the algorithm body.
    fn one_voxel_field(phi: f64) -> SampledField {
        SampledField {
            name: "test-1x1x1".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![0.0, 0.0, 0.0],
            spacing: vec![1.0, 1.0, 1.0],
            axis_grids: vec![vec![0.0], vec![0.0], vec![0.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![phi],
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Public-surface compile-test: `MedialMask`, `MedialOptions`,
    /// `MedialError`, and `compute_medial_mask` are reachable from
    /// the crate root, and the function is callable.
    ///
    /// The single voxel sits at `phi = +1.0` (which IS inside the
    /// default 3-voxel narrow band at unit spacing); the mask comes
    /// back empty because a 1×1×1 grid has identically-zero
    /// central-difference gradient (every axis collapses to a single
    /// sample) so the lone voxel is rejected by the
    /// `GRADIENT_EPSILON` degenerate-gradient filter, NOT by the
    /// narrow-band threshold. The test still validates that the
    /// public surface compiles and the function returns Ok regardless
    /// of which guard fires.
    #[test]
    fn public_surface_is_callable_on_empty_field() {
        let sdf = one_voxel_field(1.0);
        let opts = MedialOptions::default();
        let mask: MedialMask = compute_medial_mask(&sdf, &opts).expect("Ok mask");
        assert!(
            mask.voxels.is_empty(),
            "single-voxel grid has zero central-difference gradient and \
             must be rejected by the GRADIENT_EPSILON filter, yielding an \
             empty medial mask"
        );

        // Reach the error type from the crate root too — sanity-checks
        // that `MedialError` is publicly named.
        let _: MedialError = MedialError::EmptyAxisGrid { axis: 0 };
    }

    /// Build a Regular1D `SampledField` with three nodes along x. The
    /// medial-axis test is intrinsically 3D; 1D inputs must be
    /// rejected up front rather than silently producing an empty
    /// mask.
    fn one_d_field() -> SampledField {
        SampledField {
            name: "test-1d".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![2.0],
            spacing: vec![1.0],
            axis_grids: vec![vec![0.0, 1.0, 2.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![1.0, -1.0, 1.0],
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Build a Regular2D `SampledField` over a 3×3 grid.
    fn two_d_field() -> SampledField {
        SampledField {
            name: "test-2d".to_string(),
            kind: SampledGridKind::Regular2D,
            bounds_min: vec![0.0, 0.0],
            bounds_max: vec![2.0, 2.0],
            spacing: vec![1.0, 1.0],
            axis_grids: vec![vec![0.0, 1.0, 2.0], vec![0.0, 1.0, 2.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![1.0; 9],
            oob_emitted: AtomicBool::new(false),
        }
    }

    #[test]
    fn compute_medial_mask_rejects_regular1d_grids() {
        let sdf = one_d_field();
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("1D input must be rejected");
        assert_eq!(
            err,
            MedialError::UnsupportedGridKind {
                found: SampledGridKind::Regular1D
            }
        );
    }

    #[test]
    fn compute_medial_mask_rejects_regular2d_grids() {
        let sdf = two_d_field();
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("2D input must be rejected");
        assert_eq!(
            err,
            MedialError::UnsupportedGridKind {
                found: SampledGridKind::Regular2D
            }
        );
    }

    /// Build an analytic-slab Regular3D `SampledField` representing
    /// `φ(x, y, z) = |z| − half_thickness` over a `voxel_count^3`
    /// grid centered on the origin with unit voxel spacing. Useful
    /// for testing the medial-axis algorithm against a ground-truth
    /// configuration whose medial axis is exactly the centerline
    /// `z = 0` plane.
    fn slab_sdf_3d(half_thickness_voxels: f64, voxel_count: usize) -> SampledField {
        assert!(voxel_count >= 2, "slab grid needs ≥ 2 voxels per axis");
        let n = voxel_count;
        let spacing: f64 = 1.0;
        let half_extent = (n as f64 - 1.0) / 2.0;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;

        let axis_grid: Vec<f64> = (0..n)
            .map(|i| bounds_min + (i as f64) * spacing)
            .collect();
        // Row-major flat layout: data[i*n*n + j*n + k] at index (i,j,k).
        let mut data = Vec::with_capacity(n * n * n);
        for &_x in &axis_grid {
            for &_y in &axis_grid {
                for &z in &axis_grid {
                    data.push(z.abs() - half_thickness_voxels);
                }
            }
        }
        SampledField {
            name: format!(
                "slab-3d-h{half_thickness_voxels}-n{voxel_count}"
            ),
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

    /// Slab `φ = |z| − 3` on a 16×16×16 grid: the medial axis is the
    /// `z = 0` (or nearest-voxel) centerline plane. Asserts:
    ///
    /// (a) the returned mask is non-empty;
    /// (b) every voxel in the mask is on or adjacent to the
    ///     centerline z-plane (`|k − center_k| ≤ 1`);
    /// (c) the centerline plane is mostly populated
    ///     (`mask.voxels.len() ≥ 16*16/2 = 128`).
    #[test]
    fn compute_medial_mask_flags_slab_centerline_voxels() {
        let n = 16usize;
        let sdf = slab_sdf_3d(3.0, n);
        let mask = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect("slab compute succeeds");

        // (a) non-empty
        assert!(
            !mask.voxels.is_empty(),
            "slab medial mask must be non-empty"
        );

        // (b) every voxel near the centerline z-plane
        let center_k = (n as i32 - 1) / 2;
        for &[i, j, k] in &mask.voxels {
            let dz = (k - center_k).abs();
            assert!(
                dz <= 1,
                "slab voxel ({i},{j},{k}) is too far from centerline plane \
                 (k={k}, center_k={center_k}, |dk|={dz})"
            );
        }

        // (c) at least half the centerline plane is medial
        let min_expected = n * n / 2;
        assert!(
            mask.voxels.len() >= min_expected,
            "slab medial mask has {} voxels; expected ≥ {min_expected} \
             on a 16×16 centerline plane",
            mask.voxels.len()
        );
    }

    /// Build an analytic-slab Regular3D `SampledField` perpendicular to
    /// the x-axis: `φ(x, y, z) = |x| − half_thickness`. Identical
    /// construction to [`slab_sdf_3d`] except the active axis is x
    /// rather than z. Used to give the algorithm a second positive
    /// load-bearing assertion on a *different* axis — catches
    /// regressions specific to gradient indexing or walk direction
    /// along x that the z-slab test would miss.
    fn slab_sdf_3d_along_x(half_thickness_voxels: f64, voxel_count: usize) -> SampledField {
        assert!(voxel_count >= 2, "x-slab grid needs ≥ 2 voxels per axis");
        let n = voxel_count;
        let spacing: f64 = 1.0;
        let half_extent = (n as f64 - 1.0) / 2.0;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;

        let axis_grid: Vec<f64> = (0..n)
            .map(|i| bounds_min + (i as f64) * spacing)
            .collect();
        // Row-major flat layout: data[i*n*n + j*n + k] at index (i,j,k).
        // Note `x` is now the OUTER loop variable so the φ value
        // depends on the leading index i — the algorithm must walk in
        // the i-direction (NOT k) to find the medial axis.
        let mut data = Vec::with_capacity(n * n * n);
        for &x in &axis_grid {
            for &_y in &axis_grid {
                for &_z in &axis_grid {
                    data.push(x.abs() - half_thickness_voxels);
                }
            }
        }
        SampledField {
            name: format!(
                "slab-x-3d-h{half_thickness_voxels}-n{voxel_count}"
            ),
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

    /// Slab `φ = |x| − 3` on a 16×16×16 grid — second positive
    /// load-bearing assertion complementing
    /// [`compute_medial_mask_flags_slab_centerline_voxels`].
    ///
    /// **Why this test, not an odd-N sphere/thick-block?** The natural
    /// "add a positive assertion to the radial fixtures" idea fails
    /// because point-medial geometry on this algorithm is fundamentally
    /// un-flaggable: on even-N grids no voxel sits at the exact medial,
    /// and on odd-N grids the exact-medial voxel has degenerate
    /// (zero-by-symmetry) central-difference gradient and is skipped by
    /// `GRADIENT_EPSILON`; the off-by-one voxels then fail the
    /// equality test by construction (their `abs_diff/dmax` exceeds the
    /// default tolerance + absolute slack). A second slab
    /// orientation gives a clean positive assertion that exercises a
    /// genuinely different code path: gradient indexing along the
    /// outer-loop axis (`i`) rather than the inner-loop axis (`k`). A
    /// regression that swapped i↔k somewhere in the inner loop, or that
    /// only exercised gradient_at_index's z-axis branch, would fail
    /// this test while leaving the z-slab test green.
    ///
    /// Asserts the same three load-bearing properties as the z-slab
    /// test, but on the i-index instead of k.
    #[test]
    fn compute_medial_mask_flags_slab_centerline_voxels_along_x_axis() {
        let n = 16usize;
        let sdf = slab_sdf_3d_along_x(3.0, n);
        let mask = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect("x-slab compute succeeds");

        // (a) non-empty
        assert!(
            !mask.voxels.is_empty(),
            "x-slab medial mask must be non-empty"
        );

        // (b) every voxel near the centerline x-plane (i-index)
        let center_i = (n as i32 - 1) / 2;
        for &[i, j, k] in &mask.voxels {
            let di = (i - center_i).abs();
            assert!(
                di <= 1,
                "x-slab voxel ({i},{j},{k}) is too far from centerline plane \
                 (i={i}, center_i={center_i}, |di|={di}) — likely a regression \
                 in i-axis gradient indexing or walk direction"
            );
        }

        // (c) at least half the centerline plane is medial
        let min_expected = n * n / 2;
        assert!(
            mask.voxels.len() >= min_expected,
            "x-slab medial mask has {} voxels; expected ≥ {min_expected} \
             on a 16×16 centerline plane",
            mask.voxels.len()
        );
    }

    /// Build an analytic-sphere Regular3D `SampledField` representing
    /// `φ(p) = |p| - radius` over a `voxel_count³` grid centered on
    /// the origin with unit voxel spacing. The analytic medial axis
    /// of a sphere is its single center point; under voxelization at
    /// unit spacing it spreads to a small cluster of voxels near the
    /// grid center, NOT a ring or shell.
    fn sphere_sdf_3d(radius_voxels: f64, voxel_count: usize) -> SampledField {
        assert!(voxel_count >= 2, "sphere grid needs ≥ 2 voxels per axis");
        let n = voxel_count;
        let spacing: f64 = 1.0;
        let half_extent = (n as f64 - 1.0) / 2.0;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;

        let axis_grid: Vec<f64> = (0..n)
            .map(|i| bounds_min + (i as f64) * spacing)
            .collect();
        // Row-major flat layout: data[i*n*n + j*n + k] at index (i,j,k).
        let mut data = Vec::with_capacity(n * n * n);
        for &x in &axis_grid {
            for &y in &axis_grid {
                for &z in &axis_grid {
                    let r = (x * x + y * y + z * z).sqrt();
                    data.push(r - radius_voxels);
                }
            }
        }
        SampledField {
            name: format!("sphere-3d-r{radius_voxels}-n{voxel_count}"),
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

    /// Sphere `φ = |p| − 5` on a 16×16×16 grid — **negative-signal-only**
    /// regression check.
    ///
    /// The analytic medial axis of a sphere is a single point at the
    /// grid center, but per-voxel registration (a voxel V is medial iff
    /// `d⁺(V) ≈ d⁻(V)`) cannot reliably tag it on this fixture: on an
    /// even-N grid no voxel sits exactly at origin, so every in-band
    /// voxel V at distance `|V| > 0` from origin sees
    /// `abs_diff = 2|V|` and fails the equality test by construction;
    /// the exact-center voxel that would clear it (odd-N grids only)
    /// has degenerate gradient and is skipped by the
    /// `GRADIENT_EPSILON` filter. An algorithm that always returned
    /// an empty mask would therefore PASS this test trivially —
    /// non-emptiness is intentionally NOT asserted.
    ///
    /// What this test DOES catch is the **negative** signature: no
    /// band-shell ring, no face-adjacent or far-from-center false
    /// positives appear in the mask. That is the real regression
    /// risk for the bidirectional-walk + distinctness pipeline on
    /// radial geometry — a relaxed distinctness check would let the
    /// band-shell voxels (where the bidirectional walk hits the same
    /// surface patch on both sides with poorly-defined gradient at
    /// the medial) leak into the mask. Positive verification of the
    /// algorithm's mediality decision lives in
    /// [`compute_medial_mask_flags_slab_centerline_voxels`] (slab
    /// fixture, plane-medial, voxel-aligned).
    #[test]
    fn compute_medial_mask_on_sphere_admits_no_far_voxels() {
        let n = 16usize;
        let sdf = sphere_sdf_3d(5.0, n);
        let mask = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect("sphere compute succeeds");

        // Every voxel in the mask (if any) must be within 2 voxels of
        // the grid center. The grid center for n=16 is between indices
        // 7 and 8 on each axis (no voxel exactly at origin); the
        // cluster radius caps at 2.0 voxels in Euclidean index space.
        // An empty mask is acceptable — point-medial geometries on
        // even-N grids cannot trip per-voxel registration (see test
        // doc).
        let center = (n as f64 - 1.0) / 2.0;
        for &[i, j, k] in &mask.voxels {
            let di = (i as f64) - center;
            let dj = (j as f64) - center;
            let dk = (k as f64) - center;
            let dist = (di * di + dj * dj + dk * dk).sqrt();
            assert!(
                dist <= 2.0,
                "sphere medial voxel ({i},{j},{k}) is too far from grid \
                 center (dist={dist:.3} voxels; cap=2.0). The medial \
                 axis of a sphere is its center point; far-from-center \
                 voxels are false positives — likely from the bidirectional \
                 ray walk hitting the same surface patch on both sides \
                 with poorly-defined gradient at the medial."
            );
        }
    }

    /// Pin the empirical constants that the PRD's task-T1 description
    /// implicitly asserts:
    ///
    /// - `distance_tolerance == 0.05` — the PRD's "~5%" relative-distance
    ///   equality threshold for the bidirectional ray walk.
    /// - `narrow_band_half_width_voxels == 3.0` — covers the smallest medial
    ///   axis at the PRD's `thickness/3` voxel-size default (smallest
    ///   medial slab is 3 voxels thick → half-width 1.5; 3 leaves headroom
    ///   for gradient sampling at the boundary voxels).
    /// - `normal_antiparallel_threshold == -0.5` — opposing-face hits whose
    ///   gradients dot to less than this (i.e. roughly antiparallel,
    ///   ≥120° between normals) count as "distinct surface patches" per
    ///   the gradient-discontinuity signature.
    /// - `max_thickness_voxels == 64.0` — bidirectional ray walk
    ///   truncation in voxel units; covers thick (≪64-voxel half-thickness)
    ///   solids without a runaway walk on degenerate gradients.
    ///
    /// Also destructures the public field set: catches accidental
    /// renames at compile time (the destructuring fails to compile if
    /// any field is renamed, removed, or added).
    #[test]
    fn medial_options_defaults_pin_empirical_constants() {
        // Pattern-destructure all public fields. Two purposes:
        //  - field-rename guard: the destructuring fails to compile
        //    if any field is renamed, removed, or added.
        //  - value-pin: the trailing assert_eq!s pin each numeric
        //    default to the PRD-derived constant.
        let MedialOptions {
            distance_tolerance,
            narrow_band_half_width_voxels,
            normal_antiparallel_threshold,
            max_thickness_voxels,
        } = MedialOptions::default();
        assert_eq!(distance_tolerance, 0.05);
        assert_eq!(narrow_band_half_width_voxels, 3.0);
        assert_eq!(normal_antiparallel_threshold, -0.5);
        assert_eq!(max_thickness_voxels, 64.0);
    }

    /// Build an analytic thick-block Regular3D `SampledField` representing
    /// `φ(p) = max(|p_x|, |p_y|, |p_z|) - half_size` (Chebyshev-distance
    /// SDF approximation) over a `voxel_count³` grid centered on the
    /// origin with unit voxel spacing. Sufficient for the medial-mask
    /// test because it preserves the "deep interior is far from the
    /// surface" property that drives the narrow-band filter; the exact
    /// Euclidean-distance SDF would only differ near corners/edges where
    /// the mask should be empty regardless.
    fn thick_block_sdf_3d(half_size_voxels: f64, voxel_count: usize) -> SampledField {
        assert!(voxel_count >= 2, "thick block grid needs ≥ 2 voxels per axis");
        let n = voxel_count;
        let spacing: f64 = 1.0;
        let half_extent = (n as f64 - 1.0) / 2.0;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;

        let axis_grid: Vec<f64> = (0..n)
            .map(|i| bounds_min + (i as f64) * spacing)
            .collect();
        // Row-major flat layout: data[i*n*n + j*n + k] at index (i,j,k).
        let mut data = Vec::with_capacity(n * n * n);
        for &x in &axis_grid {
            for &y in &axis_grid {
                for &z in &axis_grid {
                    let m = x.abs().max(y.abs()).max(z.abs());
                    data.push(m - half_size_voxels);
                }
            }
        }
        SampledField {
            name: format!("thick-block-3d-h{half_size_voxels}-n{voxel_count}"),
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

    /// Tightening `distance_tolerance` must monotonically shrink the
    /// slab mask: every voxel in the strict mask must also be in the
    /// loose mask, the strict cardinality must be **strictly smaller**,
    /// and both must be non-empty. Pins the `distance_tolerance` field
    /// as actually wired to the medial decision in `compute_medial_mask`'s
    /// inner loop — a future regression that hardcoded the threshold,
    /// removed it from the inner-loop computation, or renamed it would
    /// surface here.
    ///
    /// **Test-tolerance choice — discriminative bracket of the slab's
    /// relative-error spectrum.** The 16×16×16 thickness-3 slab fixture
    /// has only three relative-error regimes (centerline `abs_diff/dmax
    /// ≈ 0.286`, one-voxel-off `≈ 0.667`, two-voxel-off `≈ 0.909`); a
    /// tolerance in `(0.286, 0.667]` admits centerline only, a tolerance
    /// in `(0.667, 0.909]` admits both centerline and one-voxel-off.
    /// Tightening the production default `0.05` to `0.001` (the obvious
    /// 50×-stricter choice) does NOT shrink the mask on this fixture
    /// because the absolute one-voxel slack `+ min_spacing` in the
    /// equality threshold (see [`compute_medial_mask`] inline comment
    /// and the [`MedialOptions::distance_tolerance`] doc-comment caveat)
    /// dominates for thicknesses ≪ 20 voxels — both `0.05*3.5 + 1.0` and
    /// `0.001*3.5 + 1.0` round to ≈ `1.0` and admit only the centerline
    /// `abs_diff = 1.0` voxels. We therefore bracket directly: `loose =
    /// 0.7` (relative term dominates → admits both regimes →
    /// abs_diff < 0.7·dmax + 1.0 admits centerline AND one-voxel-off)
    /// and `strict = 0.05` (= production default → relative term inert →
    /// admits centerline only). This triggers the strict-cardinality
    /// assertion below, catching a regression that the prior `0.05 vs
    /// 0.001` test would have missed (see code-review suggestion #2).
    #[test]
    fn tightening_distance_tolerance_reduces_slab_mask_size() {
        // std HashSet deliberately: FxHash is scoped to production code
        // where [i32;3] voxel sets reach ~16 M entries on the 256³
        // workload. This fixture produces at most a few hundred voxels;
        // the hasher difference is unmeasurable at that scale, and the
        // rest of the crate's test code (segmentation.rs) uses std types.
        use std::collections::HashSet;

        let n = 16usize;
        let sdf = slab_sdf_3d(3.0, n);

        // Loose (0.7) admits both centerline (ratio ≈ 0.286) and
        // one-voxel-off (ratio ≈ 0.667). Strict (0.05 = production
        // default) admits centerline only.
        let loose_opts = MedialOptions {
            distance_tolerance: 0.7,
            ..MedialOptions::default()
        };
        let strict_opts = MedialOptions::default();

        let loose_mask = compute_medial_mask(&sdf, &loose_opts)
            .expect("slab compute (loose tolerance) succeeds");
        let strict_mask = compute_medial_mask(&sdf, &strict_opts)
            .expect("slab compute (strict tolerance = production default) succeeds");

        // (a) both non-zero
        assert!(
            !loose_mask.voxels.is_empty(),
            "loose-tolerance slab mask must be non-empty"
        );
        assert!(
            !strict_mask.voxels.is_empty(),
            "strict-tolerance slab mask must be non-empty (the centerline \
             voxels see d⁺ ≈ d⁻ to within one voxel and clear the \
             absolute one-voxel slack at any tolerance ≥ 0)"
        );

        // (b) STRICT cardinality reduction: the strict tolerance MUST
        // shrink the mask. If these counts are equal, the
        // distance_tolerance field is effectively inert in the inner
        // loop on this fixture — either hardcoded, unread, or wired to
        // a different field. Catches the regression that the
        // weaker-than-strict `<=` assertion would miss.
        assert!(
            strict_mask.voxels.len() < loose_mask.voxels.len(),
            "tightening distance_tolerance from 0.7 to 0.05 must shrink \
             the slab mask (loose admits one-voxel-off voxels at \
             ratio≈0.667, strict admits centerline only): strict={} loose={}",
            strict_mask.voxels.len(),
            loose_mask.voxels.len()
        );

        // (c) strict ⊂ loose (voxel-set inclusion)
        let loose_set: HashSet<[i32; 3]> = loose_mask.voxels.iter().copied().collect();
        for v in &strict_mask.voxels {
            assert!(
                loose_set.contains(v),
                "strict-tolerance voxel {v:?} is missing from the loose \
                 mask — distance_tolerance is not monotonically wired \
                 (a stricter threshold should never accept a voxel that \
                 the looser threshold rejected)"
            );
        }
    }

    /// Thick block `φ = max(|p_i|) − 6` on a 16×16×16 grid —
    /// **negative-signal-only** regression check.
    ///
    /// The analytic medial axis is a single point at the cube
    /// centroid (origin). The deep interior (`|φ| ≫ 3`) is excluded
    /// by the narrow-band filter, and the band-shell voxels
    /// (`|φ| ≤ 3`) lie near a single face/edge/corner so their
    /// bidirectional ray walks are profoundly asymmetric
    /// (`d⁺ ≈ 0.5` vs `d⁻ ≈ 11.5` on a face, which fails the relative
    /// equality test by a wide margin). Per-voxel registration cannot
    /// reliably tag the centroid on an even-N grid (no voxel at exact
    /// origin); an algorithm that always returned an empty mask would
    /// PASS this test trivially — non-emptiness is intentionally NOT
    /// asserted.
    ///
    /// What this test DOES catch is the **negative** signature: NO
    /// face-adjacent or near-corner voxels appear in the mask. Those
    /// are the false-positive risk for an under-defended distinctness
    /// check (a relaxed `normal_antiparallel_threshold` would admit
    /// same-face hits at the band shell and produce a face-adjacent
    /// ring of false positives). Positive verification of the
    /// algorithm's mediality decision lives in
    /// [`compute_medial_mask_flags_slab_centerline_voxels`].
    #[test]
    fn compute_medial_mask_on_thick_block_admits_no_face_voxels() {
        let n = 16usize;
        let sdf = thick_block_sdf_3d(6.0, n);
        let mask = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect("thick block compute succeeds");

        // The grid centroid for n=16 lies between indices 7 and 8 on
        // each axis (no voxel sits exactly at origin); a "within 1
        // voxel of the centroid" cluster fits inside Euclidean radius
        // √3 ≈ 1.732 in index space (the 8 corner voxels of the
        // central cell).
        let center = (n as f64 - 1.0) / 2.0;
        for &[i, j, k] in &mask.voxels {
            let di = (i as f64) - center;
            let dj = (j as f64) - center;
            let dk = (k as f64) - center;
            let dist = (di * di + dj * dj + dk * dk).sqrt();
            assert!(
                dist <= 1.8,
                "thick-block medial voxel ({i},{j},{k}) is too far from \
                 grid center (dist={dist:.3} voxels; cap=1.8). The medial \
                 axis of a thick block is its centroid; far-from-center \
                 voxels are false positives — likely from an under-defended \
                 surface-patch distinctness check accepting same-face \
                 hits at the band shell."
            );
        }
    }

    /// Positive coverage test for `AxisLengthMismatch`: constructs a
    /// Regular3D `SampledField` with `bounds_min.len() == 1` while
    /// all other axis vectors have length 3. Verifies that
    /// `compute_medial_mask` returns the correct error with exact
    /// field values rather than panicking or silently succeeding.
    #[test]
    fn compute_medial_mask_rejects_axis_length_mismatch() {
        let sdf = SampledField {
            name: "test-axis-len-mismatch".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0],               // length 1 — mismatch
            bounds_max: vec![0.0, 0.0, 0.0],     // length 3
            spacing: vec![1.0, 1.0, 1.0],        // length 3
            axis_grids: vec![vec![0.0], vec![0.0], vec![0.0]], // length 3
            interpolation: InterpolationKind::Linear,
            data: vec![1.0],
            oob_emitted: AtomicBool::new(false),
        };
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("axis-length-mismatch input must be rejected");
        assert_eq!(
            err,
            MedialError::AxisLengthMismatch {
                bounds_min_len: 1,
                bounds_max_len: 3,
                spacing_len: 3,
                axis_grids_len: 3,
            }
        );
    }

    /// RED test for `DataLengthMismatch`: constructs a 2×2×2 Regular3D
    /// `SampledField` (so `nx*ny*nz = 8`) but provides only 4 data
    /// values. Asserts that `compute_medial_mask` returns
    /// `DataLengthMismatch { expected: 8, found: 4 }` rather than
    /// panicking mid-loop with an opaque out-of-bounds index.
    #[test]
    fn compute_medial_mask_rejects_data_length_mismatch() {
        let sdf = SampledField {
            name: "test-data-len-mismatch".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![1.0, 1.0, 1.0],
            spacing: vec![1.0, 1.0, 1.0],
            axis_grids: vec![vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 1.0]], // 2×2×2
            interpolation: InterpolationKind::Linear,
            data: vec![0.0; 4], // should be 8
            oob_emitted: AtomicBool::new(false),
        };
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("data-length-mismatch input must be rejected");
        assert_eq!(
            err,
            MedialError::DataLengthMismatch {
                expected: 8,
                found: 4,
            }
        );
    }

    /// Positive coverage test for `EmptyAxisGrid`: constructs a
    /// Regular3D `SampledField` whose axis-0 grid is empty (outer
    /// length 3, inner length 0 on axis 0). Passes `AxisLengthMismatch`
    /// because the outer vector has the required 3 entries; triggers
    /// `EmptyAxisGrid` on the first per-axis liveness check.
    #[test]
    fn compute_medial_mask_rejects_empty_axis_grid() {
        let sdf = SampledField {
            name: "test-empty-axis-grid".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![0.0, 0.0, 0.0],
            spacing: vec![1.0, 1.0, 1.0],
            axis_grids: vec![vec![], vec![0.0], vec![0.0]], // axis-0 empty
            interpolation: InterpolationKind::Linear,
            data: vec![],
            oob_emitted: AtomicBool::new(false),
        };
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("empty-axis-grid input must be rejected");
        assert_eq!(err, MedialError::EmptyAxisGrid { axis: 0 });
    }

    /// Helper: build a 2×2×2 Regular3D SampledField with the given
    /// per-axis spacing and bounds overrides. Used by the
    /// `InvalidAxisGeometry` test family so each test only specifies
    /// the one value it wants to violate.
    fn geometry_test_field(
        spacing: [f64; 3],
        bounds_min: [f64; 3],
        bounds_max: [f64; 3],
    ) -> SampledField {
        SampledField {
            name: "test-geom".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: bounds_min.to_vec(),
            bounds_max: bounds_max.to_vec(),
            spacing: spacing.to_vec(),
            axis_grids: vec![vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 1.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![0.0; 8],
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// RED — zero spacing on axis 0 must return `InvalidAxisGeometry`.
    /// `spacing[0] = 0.0` makes `sample_at_world` divide by zero and
    /// return NaN, which the `is_finite()` guard silently discards as
    /// OOB. The new check rejects it up front with a typed error.
    #[test]
    fn compute_medial_mask_rejects_zero_spacing() {
        let sdf = geometry_test_field(
            [0.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        );
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("zero spacing must be rejected");
        assert_eq!(
            err,
            MedialError::InvalidAxisGeometry {
                axis: 0,
                spacing: 0.0,
                bounds_min: 0.0,
                bounds_max: 1.0,
            }
        );
    }

    /// RED — negative spacing on axis 0 must return `InvalidAxisGeometry`.
    /// `spacing[0] = -1.0` flips index direction in trilinear
    /// interpolation, producing silent geometric nonsense.
    #[test]
    fn compute_medial_mask_rejects_negative_spacing() {
        let sdf = geometry_test_field(
            [-1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        );
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("negative spacing must be rejected");
        assert_eq!(
            err,
            MedialError::InvalidAxisGeometry {
                axis: 0,
                spacing: -1.0,
                bounds_min: 0.0,
                bounds_max: 1.0,
            }
        );
    }

    /// RED — NaN spacing on axis 0 must return `InvalidAxisGeometry`.
    /// Uses pattern-match + `is_nan()` because `f64::NAN != f64::NAN`
    /// under IEEE-754 defeats `assert_eq!` on the derived `PartialEq`.
    #[test]
    fn compute_medial_mask_rejects_nan_spacing() {
        let sdf = geometry_test_field(
            [f64::NAN, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        );
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("NaN spacing must be rejected");
        match err {
            MedialError::InvalidAxisGeometry {
                axis: 0,
                spacing,
                ..
            } if spacing.is_nan() => {}
            other => panic!(
                "expected InvalidAxisGeometry {{ axis: 0, spacing: NaN, .. }}, got {other:?}"
            ),
        }
    }

    /// NaN bounds_min on axis 0 must return `InvalidAxisGeometry`.
    /// `f64::NAN > f64::NAN` evaluates to false under IEEE-754, so a
    /// naive `bmin > bmax` check silently passes NaN bounds. The
    /// `!bmin.is_finite()` predicate catches this class.
    /// Uses pattern-match + `is_nan()` because NaN ≠ NaN under `PartialEq`.
    #[test]
    fn compute_medial_mask_rejects_nan_bounds() {
        let sdf = geometry_test_field(
            [1.0, 1.0, 1.0],
            [f64::NAN, 0.0, 0.0], // bounds_min[0] = NaN
            [1.0, 1.0, 1.0],
        );
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("NaN bounds_min must be rejected");
        match err {
            MedialError::InvalidAxisGeometry {
                axis: 0,
                bounds_min,
                ..
            } if bounds_min.is_nan() => {}
            other => panic!(
                "expected InvalidAxisGeometry {{ axis: 0, bounds_min: NaN, .. }}, got {other:?}"
            ),
        }
    }

    /// RED — inverted bounds on axis 0 (`bounds_min > bounds_max`)
    /// must return `InvalidAxisGeometry`. The `world_at_index` and
    /// `sample_at_world` helpers produce geometrically nonsensical
    /// results for inverted bounds; the `is_finite` guard then masks
    /// the corruption as silent OOB.
    #[test]
    fn compute_medial_mask_rejects_inverted_bounds() {
        let sdf = geometry_test_field(
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 0.0], // bounds_min[0] > bounds_max[0]
            [0.0, 1.0, 1.0],
        );
        let err = compute_medial_mask(&sdf, &MedialOptions::default())
            .expect_err("inverted bounds must be rejected");
        assert_eq!(
            err,
            MedialError::InvalidAxisGeometry {
                axis: 0,
                spacing: 1.0,
                bounds_min: 1.0,
                bounds_max: 0.0,
            }
        );
    }
}
