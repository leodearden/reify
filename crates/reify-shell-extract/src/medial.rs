//! Per-voxel medial-mask algorithm for thin-solid mid-surface extraction.
//!
//! Implements PRD task T1
//! (`docs/prds/v0_4/structural-analysis-shells.md`): for each active voxel
//! in a 3D narrow-band SDF, walk the SDF gradient in two opposing
//! directions to find the nearest surface points; tag the voxel as medial
//! iff the two distances are within `distance_tolerance` AND the two
//! surface-hit gradients are roughly antiparallel (encoding the gradient
//! discontinuity at the medial axis).

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

impl MedialMask {
    /// Construct an empty mask carrying the input grid's voxel
    /// metadata. Used as the trivial result for grids that contain no
    /// active narrow-band voxels.
    pub fn empty(spacing: [f64; 3], origin: [f64; 3]) -> Self {
        Self {
            spacing,
            origin,
            voxels: Vec::new(),
        }
    }
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
    /// walk: a voxel is medial only if `|d⁺ − d⁻| / max(d⁺, d⁻) <
    /// distance_tolerance`. Default `0.05` matches the PRD's "~5%"
    /// language (T1 step-2). Tightening this (e.g. `0.001`) culls
    /// near-medial voxels and converges the mask towards the exact
    /// medial axis at the cost of mask sparsity.
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
        }
    }
}

impl std::error::Error for MedialError {}

/// Compute the per-voxel medial mask for a Regular3D narrow-band SDF.
///
/// # Algorithm overview (filled in step-8)
///
/// For each voxel inside the narrow band, walk the normalized SDF
/// gradient in `+g` and `−g` until each ray crosses the zero level set;
/// tag the voxel as medial iff (a) `|d⁺ − d⁻| / max(d⁺, d⁻) <
/// distance_tolerance` AND (b) the gradients sampled at the two hit
/// points are roughly antiparallel (`g_a · g_b < normal_antiparallel_threshold`).
///
/// # Step-2 stub
///
/// Currently returns an empty mask carrying the input grid's
/// `spacing` / `bounds_min`. The narrow-band loop is wired in step-8
/// once steps 3–6 fix the input-validation surface and the options
/// constants.
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

    // (3) Each axis grid must be non-empty — a zero-extent axis
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
                //    one-voxel asymmetry in d⁺/d⁻ that the strict
                //    relative rule would reject). Equivalent to
                //    requiring the bisecting midpoint of the two
                //    surface hits to lie within `½(min_spacing +
                //    distance_tolerance × max(d⁺, d⁻))` of the voxel
                //    center along the gradient direction — i.e. inside
                //    the voxel itself, with a small relative-error
                //    cushion.
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
pub(crate) fn gradient_at_world(sdf: &SampledField, world: [f64; 3]) -> [f64; 3] {
    let dx = sdf.spacing[0];
    let dy = sdf.spacing[1];
    let dz = sdf.spacing[2];
    let h = 0.5 * dx.min(dy).min(dz);

    let gx = match (
        sample_at_world(sdf, [world[0] + h, world[1], world[2]]),
        sample_at_world(sdf, [world[0] - h, world[1], world[2]]),
    ) {
        (Some(p), Some(m)) => (p - m) / (2.0 * h),
        _ => 0.0,
    };
    let gy = match (
        sample_at_world(sdf, [world[0], world[1] + h, world[2]]),
        sample_at_world(sdf, [world[0], world[1] - h, world[2]]),
    ) {
        (Some(p), Some(m)) => (p - m) / (2.0 * h),
        _ => 0.0,
    };
    let gz = match (
        sample_at_world(sdf, [world[0], world[1], world[2] + h]),
        sample_at_world(sdf, [world[0], world[1], world[2] - h]),
    ) {
        (Some(p), Some(m)) => (p - m) / (2.0 * h),
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
        let phi = match sample_at_world(sdf, p) {
            Some(v) => v,
            // Stepped off the grid before crossing zero — caller
            // treats the voxel as non-medial.
            None => return None,
        };
        // Sign change (including landing exactly on zero) marks the
        // bracketing pair. Linear interpolation between (prev_t, phi)
        // and (t, phi) recovers the zero-crossing.
        if prev_phi == 0.0 {
            return Some((prev_t, point_at(start, direction, prev_t)));
        }
        if (prev_phi > 0.0 && phi <= 0.0) || (prev_phi < 0.0 && phi >= 0.0) {
            let denom = phi - prev_phi;
            // `denom == 0` would have been caught by the prev_phi
            // checks above; defensively guard anyway.
            if denom.abs() < 1e-30 {
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
    /// The single voxel sits at `phi = +1.0`, well outside any
    /// reasonable narrow band — the mask must be empty regardless of
    /// downstream-algorithm behaviour.
    #[test]
    fn public_surface_is_callable_on_empty_field() {
        let sdf = one_voxel_field(1.0);
        let opts = MedialOptions::default();
        let mask: MedialMask = compute_medial_mask(&sdf, &opts).expect("Ok mask");
        assert!(
            mask.voxels.is_empty(),
            "single-voxel grid with phi=+1.0 (entirely outside any narrow band) \
             must yield an empty medial mask"
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
        let min_expected = (n * n / 2) as usize;
        assert!(
            mask.voxels.len() >= min_expected,
            "slab medial mask has {} voxels; expected ≥ {min_expected} \
             on a 16×16 centerline plane",
            mask.voxels.len()
        );
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
        let opts = MedialOptions::default();
        assert_eq!(opts.distance_tolerance, 0.05);
        assert_eq!(opts.narrow_band_half_width_voxels, 3.0);
        assert_eq!(opts.normal_antiparallel_threshold, -0.5);
        assert_eq!(opts.max_thickness_voxels, 64.0);

        // Pattern-destructure all public fields; smoke-tests the
        // struct shape so a future field rename is caught here.
        let MedialOptions {
            distance_tolerance,
            narrow_band_half_width_voxels,
            normal_antiparallel_threshold,
            max_thickness_voxels,
        } = opts;
        assert_eq!(distance_tolerance, 0.05);
        assert_eq!(narrow_band_half_width_voxels, 3.0);
        assert_eq!(normal_antiparallel_threshold, -0.5);
        assert_eq!(max_thickness_voxels, 64.0);
    }
}
